//! Preflight: run a [`Profile`] of checks over a document in one read-only
//! pass and return one list of [`Finding`]s, each pointing at what it is
//! about.
//!
//! A check that a machine cannot decide is reported as
//! [`FindingKind::NeedsPerson`], and part of a document that could not be
//! read is reported as [`FindingKind::Incomplete`]. Neither is ever counted
//! as a pass. When the result cap is hit, [`PreflightReport::dropped`] says
//! how many findings were not kept.
//!
//! A [`FindingTarget`] for page content is the index `hit_test` and
//! [`crate::vector::decompose_page`] use, so a shell selects the finding's
//! object with the same index it already selects a click with.
//!
//! Fixups are a separate step and live on the editing session; nothing here
//! writes.

mod checks;
mod cos;
mod coverage;
mod fixup;
mod fixup_json;
pub(crate) mod hairlines;
mod pdfa;
mod pdfua;
mod pdfx;
mod profile;
mod report_text;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod walk;

use std::sync::atomic::{AtomicBool, Ordering};

pub use coverage::{CoverageMeter, TacFailure, TacMeasurement, TacRegion, TacRequest};
pub use fixup::{
    Fixup, FixupOutcome, FixupReport, FixupScope, FixupStep, FixupTools, rule_matches,
};
pub(crate) use hairlines::hairlines_in;
pub use hairlines::{HAIRLINES_NOT_COVERED, Hairline, hairlines};
pub use pdfa::{PdfaLevel, PdfaReport, PdfaVerdict, validate_pdfa, validate_pdfa_with};
pub use pdfua::{
    DecidedBy, PdfuaPart, PdfuaReport, Requirement, requirement, requirements, validate_pdfua,
};
pub(crate) use pdfua::{
    claimed_mcids, has_title as xmp_has_title, header_ok as pdfua_header_ok, lang_well_formed,
    plan_page,
};
pub use pdfx::{PdfxLevel, PdfxReport, PdfxVerdict, validate_pdfx, validate_pdfx_with};
pub use profile::{Profile, ProfileError, ProfileRule};
pub use report_text::report_text;

use crate::PdfVersion;
use crate::object::ObjId;
use crate::page_tree::Rect;
use crate::view::DocumentView;

/// How serious a rule's findings are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Worth knowing; not a defect for this profile.
    Info,
    /// Likely to print or display wrongly.
    Warning,
    /// Fails the profile.
    Error,
}

impl Severity {
    /// The lower-case name profiles and reports use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    /// Parse [`Self::as_str`]'s spelling.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "info" => Some(Self::Info),
            "warning" => Some(Self::Warning),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// A colour-space family a page can paint with (ISO 32000-2 §8.6).
/// `Indexed` is reported as its base family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ColourFamily {
    /// `DeviceGray`.
    DeviceGray,
    /// `DeviceRGB`.
    DeviceRgb,
    /// `DeviceCMYK`.
    DeviceCmyk,
    /// `CalGray`.
    CalGray,
    /// `CalRGB`.
    CalRgb,
    /// `Lab`.
    Lab,
    /// `ICCBased`, one component.
    IccGray,
    /// `ICCBased`, three components.
    IccRgb,
    /// `ICCBased`, four components.
    IccCmyk,
    /// `ICCBased` with another or unreadable `/N`.
    IccOther,
    /// `Separation`.
    Separation,
    /// `DeviceN`.
    DeviceN,
    /// `Pattern`.
    Pattern,
}

impl ColourFamily {
    /// Every family, in declaration order.
    pub const ALL: [Self; 13] = [
        Self::DeviceGray,
        Self::DeviceRgb,
        Self::DeviceCmyk,
        Self::CalGray,
        Self::CalRgb,
        Self::Lab,
        Self::IccGray,
        Self::IccRgb,
        Self::IccCmyk,
        Self::IccOther,
        Self::Separation,
        Self::DeviceN,
        Self::Pattern,
    ];

    /// The name profiles use: `device-rgb`, `icc-cmyk`, `lab`, and so on.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeviceGray => "device-gray",
            Self::DeviceRgb => "device-rgb",
            Self::DeviceCmyk => "device-cmyk",
            Self::CalGray => "cal-gray",
            Self::CalRgb => "cal-rgb",
            Self::Lab => "lab",
            Self::IccGray => "icc-gray",
            Self::IccRgb => "icc-rgb",
            Self::IccCmyk => "icc-cmyk",
            Self::IccOther => "icc-other",
            Self::Separation => "separation",
            Self::DeviceN => "device-n",
            Self::Pattern => "pattern",
        }
    }

    /// Parse [`Self::as_str`]'s spelling.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.as_str() == name)
    }
}

/// One check and its parameters.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Check {
    /// An image draws at fewer than `min_ppi` pixels per inch on either
    /// axis, measured from its placement on the page.
    ImageBelowPpi {
        /// The floor, in pixels per inch.
        min_ppi: f64,
    },
    /// An image draws at more than `max_ppi` pixels per inch on either
    /// axis, measured as for [`Self::ImageBelowPpi`]. A downsample fixup
    /// answers it ([`Fixup::DownsampleImages`]); an image also drawn larger
    /// elsewhere keeps the pixels that placement needs.
    ImageAbovePpi {
        /// The ceiling, in pixels per inch.
        max_ppi: f64,
    },
    /// A font's program is not embedded. Type 3 fonts are drawn by the
    /// document and always pass.
    FontNotEmbedded,
    /// The catalog has no `/OutputIntents`.
    NoOutputIntent,
    /// A page has neither `/TrimBox` nor `/ArtBox`.
    NoTrimBox,
    /// A page paints with one of these colour-space families.
    ColourSpaceUsed {
        /// The families that are findings.
        families: Vec<ColourFamily>,
    },
    /// A `Separation` or `DeviceN` space names a spot ink not in
    /// `allowed`. Process names, `All` and `None` are never spots.
    SpotNotAllowed {
        /// Ink names that may appear, compared exactly.
        allowed: Vec<String>,
    },
    /// A graphics state turns overprint on.
    OverprintUsed,
    /// A stroke prints thinner than `min_width_pt` on its thinnest axis, or
    /// has line width 0 (ISO 32000-2 §8.4.3.2). See [`hairlines`].
    StrokeBelow {
        /// The minimum printed width, in points; 0 finds width-0 strokes only.
        min_width_pt: f64,
    },
    /// A page or annotation declares transparency: constant alpha below
    /// 1, a non-`Normal` blend, a soft mask or a transparency group
    /// (ISO 32000-2 §11; [`crate::transparency::has_transparency`]).
    TransparencyUsed,
    /// The file is encrypted.
    Encrypted,
    /// The header version is above `max`.
    VersionAbove {
        /// The highest version allowed.
        max: PdfVersion,
    },
    /// Every machine-checkable requirement of a PDF/UA part, as
    /// [`validate_pdfua`] runs it. Violations and incompletes take the
    /// rule's severity; needs-a-person findings stay [`Severity::Info`].
    PdfUa {
        /// The part to validate against.
        part: PdfuaPart,
    },
    /// One requirement of a PDF/X level (ISO 15930), by its inventory row
    /// id (`PXQ-AN-1`). The built-in `pdfx-*` profiles are made of these;
    /// see [`validate_pdfx`].
    PdfxRequirement {
        /// The level whose clause applies.
        level: PdfxLevel,
        /// The row id.
        row: &'static str,
    },
    /// One requirement of a PDF/A level (ISO 19005), by its inventory row
    /// id (`PAI-FS-15`). The built-in `pdfa-*` profiles are made of these;
    /// see [`validate_pdfa`].
    PdfaRequirement {
        /// The level whose clause applies.
        level: PdfaLevel,
        /// The row id.
        row: &'static str,
    },
    /// The summed ink of every plate (C, M, Y, K and each spot) is above
    /// `limit_percent` somewhere on a page, measured by compositing the
    /// page in ink at `ppi`. Needs a [`CoverageMeter`]: see
    /// [`run_with_meter`]; without one the rule is reported incomplete.
    TotalAreaCoverage {
        /// The limit, in percent of one plate (300 is three solid plates).
        limit_percent: f64,
        /// The measuring resolution, in pixels per inch.
        ppi: f64,
    },
}

/// What a finding says about its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FindingKind {
    /// The check failed.
    Violation,
    /// The check cannot be decided by a machine; a person must look.
    NeedsPerson,
    /// The part of the document the check needed could not be read.
    Incomplete,
}

/// What a finding is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FindingTarget {
    /// The document as a whole.
    Document,
    /// The finding's page.
    Page,
    /// An index into `PageObjects::objects` for the finding's page.
    PageObject(usize),
    /// An index into `PageObjects::leaves`: an object inside a form.
    FormLeaf(usize),
    /// An annotation, by object.
    Annotation(ObjId),
    /// A font dictionary.
    Font(ObjId),
    /// A colour-space array.
    ColourSpace(ObjId),
    /// An image XObject.
    Image(ObjId),
    /// An `ExtGState` dictionary.
    GraphicsState(ObjId),
    /// A structure element dictionary.
    StructElem(ObjId),
}

/// The unit of a [`Measure`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MeasureUnit {
    /// Pixels per inch.
    Ppi,
    /// Points.
    Points,
    /// Percent.
    Percent,
    /// A PDF version, as `major.minor`.
    Version,
}

impl MeasureUnit {
    /// The lower-case name reports use: `ppi`, `pt`, `percent`, `version`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ppi => "ppi",
            Self::Points => "pt",
            Self::Percent => "percent",
            Self::Version => "version",
        }
    }
}

/// The value a check measured, beside the limit it was held to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measure {
    /// What was measured.
    pub value: f64,
    /// The rule's limit.
    pub limit: f64,
    /// Both values' unit.
    pub unit: MeasureUnit,
}

/// One result of one rule.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Finding {
    /// The [`ProfileRule::id`] that produced it.
    pub rule_id: String,
    /// The rule's severity.
    pub severity: Severity,
    /// Violation, needs-a-person or incomplete.
    pub kind: FindingKind,
    /// 1-based page, or `None` for a document-level finding.
    pub page: Option<usize>,
    /// What the finding is about.
    pub target: FindingTarget,
    /// The target's extent in page space, when known.
    pub rect: Option<Rect>,
    /// One sentence for a person.
    pub message: String,
    /// The measured value against the limit, for measuring checks.
    pub measured: Option<Measure>,
}

/// Everything one run found.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct PreflightReport {
    /// Findings in page order, document-level findings first.
    pub findings: Vec<Finding>,
    /// Findings produced past [`RunOptions::max_findings`] and not kept.
    pub dropped: usize,
    /// Pages examined.
    pub pages_checked: usize,
    /// The page count.
    pub page_count: usize,
    /// True when the run stopped at the caller's cancel flag.
    pub cancelled: bool,
}

impl PreflightReport {
    /// True when an `Error`-severity finding of any kind was produced,
    /// kept or dropped.
    #[must_use]
    pub fn fails(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Error) || self.dropped > 0
    }

    /// Findings a person must decide.
    pub fn needs_person(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.kind == FindingKind::NeedsPerson)
    }

    /// True when nothing was found, nothing was dropped and the run was
    /// not cancelled.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty() && self.dropped == 0 && !self.cancelled
    }
}

/// How a run reports progress and stops early.
pub struct RunOptions<'a> {
    /// Findings kept; the rest are counted in [`PreflightReport::dropped`].
    pub max_findings: usize,
    /// Checked before each page; set it to stop the run.
    pub cancel: Option<&'a AtomicBool>,
    /// Called after each page with `(pages done, page count)`.
    pub progress: Option<&'a dyn Fn(usize, usize)>,
}

impl Default for RunOptions<'_> {
    fn default() -> Self {
        Self {
            max_findings: Self::DEFAULT_MAX_FINDINGS,
            cancel: None,
            progress: None,
        }
    }
}

impl RunOptions<'_> {
    /// The default result cap.
    pub const DEFAULT_MAX_FINDINGS: usize = 25_000;
}

/// Run `profile` over the document with the default options.
///
/// Read-only: safe on a worker with its own view.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::preflight::{self, Profile};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../../fixtures/synthetic/hello.pdf").to_vec(),
/// )?;
/// let report = preflight::run(&doc.view(), &Profile::digital_printing());
/// assert_eq!(report.pages_checked, report.page_count);
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn run(view: &DocumentView<'_>, profile: &Profile) -> PreflightReport {
    run_with(view, profile, &RunOptions::default())
}

/// Run `profile` with a result cap, cancellation and progress.
///
/// [`Check::TotalAreaCoverage`] rules are reported incomplete; use
/// [`run_with_meter`] to measure them.
#[must_use]
pub fn run_with(
    view: &DocumentView<'_>,
    profile: &Profile,
    options: &RunOptions<'_>,
) -> PreflightReport {
    run_with_meter(view, profile, options, None)
}

/// [`run_with`], measuring [`Check::TotalAreaCoverage`] rules with `meter`
/// (`pdfcer_render::TacMeter` in the shipped shells).
#[must_use]
pub fn run_with_meter(
    view: &DocumentView<'_>,
    profile: &Profile,
    options: &RunOptions<'_>,
    meter: Option<&dyn CoverageMeter>,
) -> PreflightReport {
    let mut sink = checks::Sink::new(options.max_findings);
    checks::document(view, profile, &mut sink);
    let pages = match crate::page_tree::pages_in(view.graph()) {
        Ok(p) => p,
        Err(e) => {
            sink.incomplete_all(
                profile,
                None,
                &format!("the page tree could not be read: {e}"),
            );
            Vec::new()
        }
    };
    let mut report = PreflightReport {
        page_count: pages.len(),
        ..PreflightReport::default()
    };
    for (i, page) in pages.iter().enumerate() {
        if options.cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            report.cancelled = true;
            break;
        }
        checks::page(view, profile, page, i + 1, &mut sink);
        coverage::page(view, profile, page, i + 1, meter, &mut sink);
        report.pages_checked = i + 1;
        if let Some(progress) = options.progress {
            progress(i + 1, pages.len());
        }
    }
    (report.findings, report.dropped) = sink.finish();
    report
}

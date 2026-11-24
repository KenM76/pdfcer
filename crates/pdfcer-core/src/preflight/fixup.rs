//! Fixups: the named edits a profile rule can call for, the scope one is
//! applied over, the engines a caller supplies, and what a run reports.
//!
//! The edit itself is [`crate::edit::EditSession::apply_fixup`]; nothing in
//! this module writes.

use std::fmt;

use super::{Check, ColourFamily, Finding, FindingKind, PdfuaPart, PdfxLevel};
use crate::edit::{FixPdfuaOptions, PdfuaFixup};
use crate::optimize::ResampleMethod;
use crate::prepress::{
    ColourConversion, ColourConverter, ColourDestination, PdfxConversion, PdfxOutputIntent,
};
use crate::transparency::{FlattenOptions, TransparencyFlattener};

/// One named edit a preflight rule can call for. Each wraps an existing
/// [`crate::edit::EditSession`] verb.
///
/// A [`ColourDestination::Profile`] or [`PdfxOutputIntent::profile`] left
/// empty means "the profile the caller supplies at apply time"
/// ([`FixupTools::icc_profile`]); that is how a profile file, which holds
/// no ICC bytes, names a conversion.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Fixup {
    /// [`crate::edit::EditSession::flatten_transparency`] on the scope's
    /// pages. Needs [`FixupTools::flattener`].
    FlattenTransparency(FlattenOptions),
    /// [`crate::edit::EditSession::fix_hairlines`] on the scope's pages.
    FixHairlines {
        /// The width every thinner stroke is widened to, in points.
        min_width_pt: f64,
    },
    /// [`crate::edit::EditSession::alias_ink`]: every use of the spot ink
    /// `from` becomes `to`. Document-wide under any scope.
    AliasInk {
        /// The ink renamed.
        from: String,
        /// The ink it becomes.
        to: String,
    },
    /// [`crate::edit::EditSession::spot_to_process`] for every spot ink not
    /// in `keep`. Document-wide under any scope.
    SpotToProcess {
        /// Inks left as spots, compared exactly.
        keep: Vec<String>,
    },
    /// [`crate::edit::EditSession::convert_colours`] on the scope's pages.
    /// Needs [`FixupTools::converter`].
    ConvertColours(ColourConversion),
    /// [`crate::edit::EditSession::convert_to_pdfx`], document-wide. Needs
    /// both engines; the claim is written only when the file then
    /// validates. Not available inside an outer undo group.
    ConvertToPdfx {
        /// The level converted to.
        level: PdfxLevel,
        /// How. Boxed: it is far larger than the other variants.
        conversion: Box<PdfxConversion>,
    },
    /// [`crate::edit::EditSession::fix_pdfua`], or one of its parts when
    /// `step` is set. Document-wide under any scope.
    FixPdfua {
        /// The ISO 14289 part.
        part: PdfuaPart,
        /// What the document cannot say: the language and title.
        options: FixPdfuaOptions,
        /// One part only; `None` runs all five in order.
        step: Option<PdfuaFixup>,
    },
    /// Downsample images drawn above `above_ppi` to `to_ppi`, through
    /// [`crate::edit::EditSession::plan_optimize`] and
    /// [`crate::edit::EditSession::apply_optimize`]. Under a finding or rule
    /// scope only the images those findings point at are resampled. An
    /// image is measured where it is drawn largest; the pipeline never
    /// writes a larger stream, so an image whose resampled encoding is not
    /// smaller is kept and listed as not done.
    DownsampleImages {
        /// Resample only images whose binding resolution is above this.
        above_ppi: u16,
        /// The resolution they are resampled to; at most `above_ppi`.
        to_ppi: u16,
        /// The resampling filter.
        method: ResampleMethod,
    },
}

impl Fixup {
    /// Every [`Self::name`], in declaration order.
    pub const NAMES: [&'static str; 8] = [
        "flatten-transparency",
        "fix-hairlines",
        "alias-ink",
        "spot-to-process",
        "convert-colours",
        "convert-to-pdfx",
        "fix-pdfua",
        "downsample-images",
    ];

    /// The stable lower-case name reports and profile JSON use.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::FlattenTransparency(_) => "flatten-transparency",
            Self::FixHairlines { .. } => "fix-hairlines",
            Self::AliasInk { .. } => "alias-ink",
            Self::SpotToProcess { .. } => "spot-to-process",
            Self::ConvertColours(_) => "convert-colours",
            Self::ConvertToPdfx { .. } => "convert-to-pdfx",
            Self::FixPdfua { .. } => "fix-pdfua",
            Self::DownsampleImages { .. } => "downsample-images",
        }
    }

    /// The fixup pdfcer suggests for a check, when one can address it
    /// without a choice only the operator can make; `None` otherwise.
    ///
    /// | check | suggestion |
    /// |---|---|
    /// | `image-above-ppi` | downsample to the ceiling, bicubic |
    /// | `stroke-below` (> 0) | widen to the minimum |
    /// | `transparency-used` | flatten with the default options |
    /// | `spot-not-allowed` | spot to process, keeping the allowed inks |
    /// | `colour-space-used` (an RGB, Lab or grey family) | convert those families to the output intent's profile |
    /// | `pdfua` | every PDF/UA fixup |
    /// | `pdfx-requirement` | convert to that level with the supplied profile |
    ///
    /// Nothing is suggested for a low-resolution image (pixels cannot be
    /// recovered), an unembedded font (it needs a font program), a missing
    /// output intent or trim box, overprint, encryption, the version, or a
    /// PDF/A row.
    #[must_use]
    pub fn suggested(check: &Check) -> Option<Self> {
        match check {
            Check::ImageAbovePpi { max_ppi } => {
                let ceiling = ppi_u16(*max_ppi)?;
                Some(Self::DownsampleImages {
                    above_ppi: ceiling,
                    to_ppi: ceiling,
                    method: ResampleMethod::Bicubic,
                })
            }
            Check::StrokeBelow { min_width_pt } if *min_width_pt > 0.0 => {
                Some(Self::FixHairlines {
                    min_width_pt: *min_width_pt,
                })
            }
            Check::TransparencyUsed => Some(Self::FlattenTransparency(FlattenOptions::default())),
            Check::SpotNotAllowed { allowed } => Some(Self::SpotToProcess {
                keep: allowed.clone(),
            }),
            Check::ColourSpaceUsed { families } => colour_suggestion(families),
            Check::PdfUa { part } => Some(Self::FixPdfua {
                part: *part,
                options: FixPdfuaOptions::default(),
                step: None,
            }),
            Check::PdfxRequirement { level, .. } => Some(Self::ConvertToPdfx {
                level: *level,
                conversion: Box::new(PdfxConversion::new(PdfxOutputIntent::new(Vec::new()))),
            }),
            _ => None,
        }
    }
}

fn ppi_u16(ppi: f64) -> Option<u16> {
    if !ppi.is_finite() || ppi < 1.0 {
        return None;
    }
    // Clamped above, so the cast cannot truncate.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(ppi.round().min(f64::from(u16::MAX)) as u16)
}

fn colour_suggestion(families: &[ColourFamily]) -> Option<Fixup> {
    let has = |f: ColourFamily| families.contains(&f);
    let mut c = ColourConversion::new(ColourDestination::OutputIntent);
    c.device_rgb = has(ColourFamily::DeviceRgb);
    c.icc_rgb = has(ColourFamily::IccRgb);
    c.cal_rgb = has(ColourFamily::CalRgb);
    c.lab = has(ColourFamily::Lab);
    c.grey =
        has(ColourFamily::DeviceGray) || has(ColourFamily::CalGray) || has(ColourFamily::IccGray);
    (c.device_rgb || c.icc_rgb || c.cal_rgb || c.lab || c.grey).then_some(Fixup::ConvertColours(c))
}

/// What one [`crate::edit::EditSession::apply_fixup`] call covers.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum FixupScope<'a> {
    /// One finding: its page, or the image it points at.
    Finding(&'a Finding),
    /// Every finding of one rule.
    Rule {
        /// The rule's id; a PDF/UA finding's `"<rule>/<PUI-…>"` id matches
        /// its rule.
        rule_id: &'a str,
        /// The run's findings; those of other rules are ignored.
        findings: &'a [Finding],
    },
    /// The whole document.
    All,
}

impl<'a> FixupScope<'a> {
    /// The violations the scope covers; `None` for [`Self::All`].
    /// Needs-a-person and incomplete findings are not something a fixup
    /// answers, so they are never included.
    #[must_use]
    pub fn findings(&self) -> Option<Vec<&'a Finding>> {
        match *self {
            Self::Finding(f) => Some(vec![f].into_iter().filter(|f| is_fixable(f)).collect()),
            Self::Rule { rule_id, findings } => Some(
                findings
                    .iter()
                    .filter(|f| rule_matches(&f.rule_id, rule_id) && is_fixable(f))
                    .collect(),
            ),
            Self::All => None,
        }
    }
}

fn is_fixable(f: &Finding) -> bool {
    f.kind == FindingKind::Violation
}

/// Whether a finding's `rule_id` belongs to the rule `rule`: equal, or
/// `rule` followed by `/` (PDF/UA findings name their requirement).
#[must_use]
pub fn rule_matches(finding_rule: &str, rule: &str) -> bool {
    finding_rule == rule
        || finding_rule
            .strip_prefix(rule)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The engines and data a fixup may need that core does not carry.
/// Every field is optional; a fixup that needs a missing one is reported
/// [`FixupOutcome::Unavailable`] and writes nothing.
#[derive(Clone, Copy, Default)]
#[non_exhaustive]
pub struct FixupTools<'a> {
    /// The transparency flattener (`pdfcer_emf::RecordingFlattener`).
    pub flattener: Option<&'a dyn TransparencyFlattener>,
    /// The colour converter
    /// (`pdfcer_render::convert_colours::IcccerConverter`).
    pub converter: Option<&'a dyn ColourConverter>,
    /// The ICC profile an empty [`ColourDestination::Profile`] or
    /// [`PdfxOutputIntent::profile`] stands for.
    pub icc_profile: Option<&'a [u8]>,
    /// Seconds since 1970-01-01 UTC for a PDF/X conversion that names no
    /// [`PdfxConversion::at`]; core never reads the clock.
    pub at: Option<i64>,
    /// The `/Title` a PDF/X conversion that names no
    /// [`PdfxConversion::title`] writes when the document has none.
    pub title: Option<&'a str>,
}

impl fmt::Debug for FixupTools<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FixupTools")
            .field("flattener", &self.flattener.is_some())
            .field("converter", &self.converter.is_some())
            .field("icc_profile", &self.icc_profile.map(<[u8]>::len))
            .field("at", &self.at)
            .field("title", &self.title)
            .finish()
    }
}

impl<'a> FixupTools<'a> {
    /// No engines and no profile.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set [`Self::flattener`].
    #[must_use]
    pub fn with_flattener(mut self, flattener: &'a dyn TransparencyFlattener) -> Self {
        self.flattener = Some(flattener);
        self
    }

    /// Set [`Self::converter`].
    #[must_use]
    pub fn with_converter(mut self, converter: &'a dyn ColourConverter) -> Self {
        self.converter = Some(converter);
        self
    }

    /// Set [`Self::icc_profile`].
    #[must_use]
    pub fn with_icc_profile(mut self, profile: &'a [u8]) -> Self {
        self.icc_profile = Some(profile);
        self
    }

    /// Set [`Self::at`].
    #[must_use]
    pub fn with_time(mut self, at: i64) -> Self {
        self.at = Some(at);
        self
    }

    /// Set [`Self::title`].
    #[must_use]
    pub fn with_title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }
}

/// How one fixup step ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FixupOutcome {
    /// It wrote something.
    Applied,
    /// It ran and found nothing to change.
    NothingToDo,
    /// The verb refused or failed; nothing of this step was written.
    Failed,
    /// It could not run: an engine or profile is missing, the scope points
    /// at nothing it can reach, or the rule has no fixup.
    Unavailable,
}

impl FixupOutcome {
    /// A stable lower-case name for reports and JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::NothingToDo => "nothing-to-do",
            Self::Failed => "failed",
            Self::Unavailable => "unavailable",
        }
    }
}

/// One fixup's result in a [`FixupReport`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FixupStep {
    /// The [`Fixup::name`], or `"none"` for a rule with findings and no
    /// fixup.
    pub fixup: &'static str,
    /// The rules it was run for, under a profile run or a rule scope. A
    /// profile run that meets the same fixup on several rules runs it once
    /// and lists every rule here.
    pub rule_ids: Vec<String>,
    /// How it ended.
    pub outcome: FixupOutcome,
    /// The verb's own count of what it changed: objects for most fixups,
    /// strokes for `fix-hairlines`, parts applied for `fix-pdfua`, images
    /// resampled for `downsample-images`.
    pub count: usize,
    /// What changed, one sentence each.
    pub changed: Vec<String>,
    /// What it could not do, one sentence each. Never empty when the
    /// outcome is `Failed` or `Unavailable`.
    pub not_done: Vec<String>,
    /// What it inferred, passed through from the verb (rule 4).
    pub disclosures: Vec<String>,
}

impl FixupStep {
    /// An empty step for `fixup`, outcome [`FixupOutcome::NothingToDo`].
    pub(crate) fn new(fixup: &'static str, rule_id: Option<&str>) -> Self {
        Self {
            fixup,
            rule_ids: rule_id.map(str::to_owned).into_iter().collect(),
            outcome: FixupOutcome::NothingToDo,
            count: 0,
            changed: Vec::new(),
            not_done: Vec::new(),
            disclosures: Vec::new(),
        }
    }

    /// Mark the step as not run, for `why`.
    pub(crate) fn unavailable(mut self, why: impl Into<String>) -> Self {
        self.outcome = FixupOutcome::Unavailable;
        self.not_done.push(why.into());
        self
    }

    /// Mark the step as refused by its verb.
    pub(crate) fn failed(mut self, why: impl Into<String>) -> Self {
        self.outcome = FixupOutcome::Failed;
        self.not_done.push(why.into());
        self
    }

    /// Record `count` objects written, `what` describing them.
    pub(crate) fn wrote(&mut self, count: usize, what: impl Into<String>) {
        if count > 0 {
            self.outcome = FixupOutcome::Applied;
            self.count += count;
            self.changed.push(what.into());
        }
    }
}

/// What [`crate::edit::EditSession::apply_fixup`] or
/// [`crate::edit::EditSession::apply_profile_fixups`] did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct FixupReport {
    /// One entry per fixup run, in order.
    pub steps: Vec<FixupStep>,
    /// Undo entries the run left: 1 when anything changed, else 0.
    pub undo_entries: usize,
}

impl FixupReport {
    /// Whether any step wrote something.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.steps
            .iter()
            .any(|s| s.outcome == FixupOutcome::Applied)
    }

    /// Steps that failed, could not run, or left something undone.
    pub fn not_fixed(&self) -> impl Iterator<Item = &FixupStep> {
        self.steps.iter().filter(|s| {
            matches!(s.outcome, FixupOutcome::Failed | FixupOutcome::Unavailable)
                || !s.not_done.is_empty()
        })
    }

    /// The report as lines: per step `fixup <name> [rule <id>, …]:
    /// <outcome> (<n> changed)`, then `  changed: …`, `  not done: …` and
    /// `  disclosure: …` lines, then one `fixups: …` summary line.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for s in &self.steps {
            let rule = if s.rule_ids.is_empty() {
                String::new()
            } else {
                format!(" rule {}", s.rule_ids.join(", "))
            };
            out.push_str(&format!(
                "fixup {}{rule}: {} ({} changed)\n",
                s.fixup,
                s.outcome.as_str(),
                s.count
            ));
            for (label, lines) in [
                ("changed", &s.changed),
                ("not done", &s.not_done),
                ("disclosure", &s.disclosures),
            ] {
                for line in lines {
                    out.push_str(&format!("  {label}: {line}\n"));
                }
            }
        }
        let count = |o: FixupOutcome| self.steps.iter().filter(|s| s.outcome == o).count();
        out.push_str(&format!(
            "fixups: {} applied, {} nothing to do, {} failed, {} unavailable; {} undo step(s)\n",
            count(FixupOutcome::Applied),
            count(FixupOutcome::NothingToDo),
            count(FixupOutcome::Failed),
            count(FixupOutcome::Unavailable),
            self.undo_entries
        ));
        out
    }
}

#[cfg(test)]
#[path = "fixup_tests.rs"]
mod tests;

//! PDF/UA validation: ISO 14289-1 (PDF/UA-1, over PDF 1.7) and ISO 14289-2
//! (PDF/UA-2, over PDF 2.0), in one read-only run.
//!
//! Every requirement of the spec RAG's `pdfua__req__*` inventory ends up in
//! exactly one place in the [`PdfuaReport`]:
//!
//! * a machine test that failed is a [`FindingKind::Violation`];
//! * a machine test whose input could not be read is a
//!   [`FindingKind::Incomplete`];
//! * a requirement only a person can decide (Matterhorn's "human" checks,
//!   and their UA-2 carry-forwards) is a [`FindingKind::NeedsPerson`] on
//!   every run, whatever the machine tests found;
//! * a machine requirement this version does not test, a requirement with
//!   no test, and an unsourced UA-2 row are listed in
//!   [`PdfuaReport::not_checked`].
//!
//! Nothing is ever reported as passed: a report with no violations still
//! carries its needs-a-person findings and its not-checked list.
//!
//! The machine tests follow the veraPDF PDFUA-1 and PDFUA-2 rule
//! predicates (each check names its rule). Two deliberate divergences: a
//! `Popup` annotation is exempt from ISO 14289-1 §7.18.1 (it shows its
//! parent's `/Contents`, and ISO 14289-2 §8.9.2.4.9 forbids it in the
//! structure tree), and a numbered heading above `H6` counts as a standard
//! type under UA-1 (ISO 14289-1 §7.4.2 asks for `H7` and deeper).

mod annots;
mod content;
mod document;
mod fonts;
mod person;
mod requirements;
mod structure;
#[cfg(test)]
mod tests;

use super::{Finding, FindingKind, FindingTarget, Severity};
use crate::graph::ObjectGraph;
use crate::view::DocumentView;

/// Which part of ISO 14289 a document is validated against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PdfuaPart {
    /// ISO 14289-1:2014, PDF/UA-1.
    Ua1,
    /// ISO 14289-2:2024, PDF/UA-2.
    Ua2,
}

impl PdfuaPart {
    /// The part number: 1 or 2.
    #[must_use]
    pub const fn number(self) -> u8 {
        match self {
            Self::Ua1 => 1,
            Self::Ua2 => 2,
        }
    }

    /// The part with number `n`, or `None` for anything but 1 or 2.
    #[must_use]
    pub const fn from_number(n: u8) -> Option<Self> {
        match n {
            1 => Some(Self::Ua1),
            2 => Some(Self::Ua2),
            _ => None,
        }
    }

    /// The standard's number: `"ISO 14289-1"` or `"ISO 14289-2"`.
    #[must_use]
    pub const fn standard(self) -> &'static str {
        match self {
            Self::Ua1 => "ISO 14289-1",
            Self::Ua2 => "ISO 14289-2",
        }
    }
}

/// Who decides a requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecidedBy {
    /// Software can decide it from the file.
    Machine,
    /// Only a person can decide it; it is always reported as
    /// [`FindingKind::NeedsPerson`].
    Person,
    /// The standard sets no test for it.
    NoTest,
}

/// One row of the PDF/UA requirement inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Requirement {
    /// The stable id: `PUI-…` for UA-1, `PUI2-…` for UA-2. Findings carry
    /// it as [`Finding::rule_id`].
    pub id: &'static str,
    /// The part the row belongs to.
    pub part: PdfuaPart,
    /// The ISO 14289 clause, e.g. `"ISO 14289-1 §7.18.3 ¶1"`.
    pub clause: &'static str,
    /// The Matterhorn Protocol 1.1 checkpoint, e.g. `"28-008"`.
    pub matterhorn: Option<&'static str>,
    /// The veraPDF rule id(s), without the profile prefix.
    pub verapdf: Option<&'static str>,
    /// Who decides it.
    pub decided_by: DecidedBy,
    /// `false` for the UA-2 rows the inventory could not source to a
    /// clause; such rows are always listed as not checked.
    pub sourced: bool,
}

/// Everything one PDF/UA run found.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PdfuaReport {
    /// The part validated against.
    pub part: PdfuaPart,
    /// Violations, incompletes and needs-a-person findings; document-level
    /// first, then in page order. [`Finding::rule_id`] is a
    /// [`Requirement::id`].
    pub findings: Vec<Finding>,
    /// Requirements this run did not test: machine rows not implemented,
    /// rows with no test, and unsourced rows. Never counted as passed.
    pub not_checked: Vec<&'static Requirement>,
    /// The page count, or 0 when the page tree could not be read.
    pub page_count: usize,
    /// Pages whose content and annotations were examined.
    pub pages_checked: usize,
}

impl PdfuaReport {
    /// True when any requirement was violated or could not be read.
    /// Needs-a-person findings alone do not fail a report; they say a
    /// person must still look.
    #[must_use]
    pub fn fails(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Error)
    }

    /// Findings a person must decide.
    pub fn needs_person(&self) -> impl Iterator<Item = &Finding> {
        self.of_kind(FindingKind::NeedsPerson)
    }

    /// Requirements the document breaks.
    pub fn violations(&self) -> impl Iterator<Item = &Finding> {
        self.of_kind(FindingKind::Violation)
    }

    fn of_kind(&self, kind: FindingKind) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(move |f| f.kind == kind)
    }
}

/// The requirement rows of `part`, in inventory order.
#[must_use]
pub fn requirements(part: PdfuaPart) -> &'static [Requirement] {
    match part {
        PdfuaPart::Ua1 => &requirements::UA1,
        PdfuaPart::Ua2 => &requirements::UA2,
    }
}

/// The requirement with id `id`, in either part.
#[must_use]
pub fn requirement(id: &str) -> Option<&'static Requirement> {
    requirements::UA1
        .iter()
        .chain(requirements::UA2.iter())
        .find(|r| r.id == id)
}

/// Validate the document against `part` of ISO 14289.
///
/// Read-only. Recursive walks (structure tree, forms, field parents) are
/// depth- and cycle-guarded; a part that could not be read is reported as
/// [`FindingKind::Incomplete`], never skipped.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::preflight::{PdfuaPart, validate_pdfua};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../../../fixtures/synthetic/hello.pdf").to_vec(),
/// )?;
/// let report = validate_pdfua(&doc.view(), PdfuaPart::Ua1);
/// // An untagged file fails, and the human checks are still listed.
/// assert!(report.fails());
/// assert!(report.needs_person().count() > 0);
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn validate_pdfua(view: &DocumentView<'_>, part: PdfuaPart) -> PdfuaReport {
    let mut out = Out {
        part,
        findings: Vec::new(),
    };
    let tree = structure::read(view, part);
    document::check(view, &tree, &mut out);
    structure::check(view, &tree, &mut out);
    let pages = match crate::page_tree::pages_in(view.graph()) {
        Ok(p) => p,
        Err(e) => {
            let message = format!("The page tree could not be read: {e}.");
            out.incomplete(
                "PUI-01-005",
                "PUI2-8.2.2-1",
                None,
                FindingTarget::Document,
                message,
            );
            Vec::new()
        }
    };
    let mut used = fonts::Used::default();
    for (i, page) in pages.iter().enumerate() {
        content::check(view, &tree, page, i + 1, &mut used, &mut out);
        annots::check(view, &tree, page, i + 1, &mut out);
    }
    fonts::check(view, &used, &mut out);
    person::push_all(part, &mut out);
    let mut findings = out.findings;
    findings.sort_by_key(|f| f.page);
    PdfuaReport {
        part,
        findings,
        not_checked: not_checked(part),
        page_count: pages.len(),
        pages_checked: pages.len(),
    }
}

/// Machine rows a run tests, per part. A row not listed here (and every
/// unsourced or no-test row) goes to [`PdfuaReport::not_checked`].
const UA1_CHECKED: &[&str] = &[
    "PUI-01-003",
    "PUI-01-004",
    "PUI-01-005",
    "PUI-01-007",
    "PUI-02-001",
    "PUI-02-003",
    "PUI-02-004",
    "PUI-06-001",
    "PUI-06-002",
    "PUI-06-003",
    "PUI-07-001",
    "PUI-07-002",
    "PUI-09-004",
    "PUI-09-005",
    "PUI-09-006",
    "PUI-11-001",
    "PUI-11-002",
    "PUI-11-004",
    "PUI-11-005",
    "PUI-11-006",
    "PUI-13-004",
    "PUI-14-002",
    "PUI-14-003",
    "PUI-14-006",
    "PUI-14-007",
    "PUI-17-002",
    "PUI-19-003",
    "PUI-19-004",
    "PUI-20-001",
    "PUI-20-002",
    "PUI-20-003",
    "PUI-21-001",
    "PUI-25-001",
    "PUI-26-001",
    "PUI-26-002",
    "PUI-28-002",
    "PUI-28-004",
    "PUI-28-005",
    "PUI-28-007",
    "PUI-28-008",
    "PUI-28-009",
    "PUI-28-010",
    "PUI-28-011",
    "PUI-28-012",
    "PUI-28-017",
    "PUI-30-001",
    "PUI-31-004",
    "PUI-31-005",
    "PUI-31-009",
    "PUI-31-019",
    "PUI-31-020",
    "PUI-31-021",
    "PUI-31-022",
    "PUI-31-024",
    "PUI-31-027",
    "PUI-V-6.1-1",
    "PUI-V-6.2-1",
    "PUI-V-7.1-11",
    "PUI-V-7.1-12",
    "PUI-V-7.2-29",
    "PUI-V-7.18.4-2",
];

/// See [`UA1_CHECKED`].
const UA2_CHECKED: &[&str] = &[
    "PUI2-5-1",
    "PUI2-5-2",
    "PUI2-5-5",
    "PUI2-6.2-1",
    "PUI2-8.2.1-1",
    "PUI2-8.2.1-2",
    "PUI2-8.2.2-1",
    "PUI2-8.2.5.2-1",
    "PUI2-8.2.5.12-1",
    "PUI2-8.2.5.20-1",
    "PUI2-8.2.5.28.2-1",
    "PUI2-8.4.3-2",
    "PUI2-8.4.3-3",
    "PUI2-8.4.4-1",
    "PUI2-8.4.4-2",
    "PUI2-8.4.5.3.2-1",
    "PUI2-8.4.5.5.1-1",
    "PUI2-8.4.5.7-2",
    "PUI2-8.4.5.7-3",
    "PUI2-8.4.5.8-1",
    "PUI2-8.7-1",
    "PUI2-8.7-2",
    "PUI2-8.9.2.2-1",
    "PUI2-8.9.2.2-2",
    "PUI2-8.9.2.3-1",
    "PUI2-8.9.2.4.7-1",
    "PUI2-8.9.2.4.8-1",
    "PUI2-8.9.2.4.9-1",
    "PUI2-8.9.2.4.10-1",
    "PUI2-8.9.2.4.11-1",
    "PUI2-8.9.2.4.11-2",
    "PUI2-8.9.2.4.12-1",
    "PUI2-8.9.2.4.13-1",
    "PUI2-8.9.2.4.14-1",
    "PUI2-8.9.2.4.15-1",
    "PUI2-8.9.2.4.16-1",
    "PUI2-8.9.2.4.19-1",
    "PUI2-8.9.2.4.19-2",
    "PUI2-8.9.3.3-1",
    "PUI2-8.9.4.2-1",
    "PUI2-8.10.1-1",
    "PUI2-8.10.1-2",
    "PUI2-8.10.1-3",
    "PUI2-8.11.1-1",
    "PUI2-8.11.1-2",
    "PUI2-8.11.2-1",
    "PUI2-8.14.1-1",
];

fn checked(part: PdfuaPart) -> &'static [&'static str] {
    match part {
        PdfuaPart::Ua1 => UA1_CHECKED,
        PdfuaPart::Ua2 => UA2_CHECKED,
    }
}

fn not_checked(part: PdfuaPart) -> Vec<&'static Requirement> {
    let tested = checked(part);
    requirements(part)
        .iter()
        .filter(|r| match r.decided_by {
            DecidedBy::Machine => !r.sourced || !tested.contains(&r.id),
            DecidedBy::Person => !r.sourced,
            DecidedBy::NoTest => true,
        })
        .collect()
}

/// The findings sink every check writes to. A check names its UA-1 and
/// UA-2 rows; `""` means the part has no such row and nothing is pushed.
struct Out {
    part: PdfuaPart,
    findings: Vec<Finding>,
}

impl Out {
    const fn pick(&self, ua1: &'static str, ua2: &'static str) -> &'static str {
        match self.part {
            PdfuaPart::Ua1 => ua1,
            PdfuaPart::Ua2 => ua2,
        }
    }

    fn violation(
        &mut self,
        ua1: &'static str,
        ua2: &'static str,
        page: Option<usize>,
        target: FindingTarget,
        message: String,
    ) {
        self.push(FindingKind::Violation, ua1, ua2, page, target, message);
    }

    fn incomplete(
        &mut self,
        ua1: &'static str,
        ua2: &'static str,
        page: Option<usize>,
        target: FindingTarget,
        message: String,
    ) {
        self.push(FindingKind::Incomplete, ua1, ua2, page, target, message);
    }

    fn push(
        &mut self,
        kind: FindingKind,
        ua1: &'static str,
        ua2: &'static str,
        page: Option<usize>,
        target: FindingTarget,
        message: String,
    ) {
        let id = self.pick(ua1, ua2);
        if id.is_empty() {
            return;
        }
        debug_assert!(
            kind == FindingKind::NeedsPerson || checked(self.part).contains(&id),
            "{id} is pushed but not listed as checked"
        );
        let severity = if kind == FindingKind::NeedsPerson {
            Severity::Info
        } else {
            Severity::Error
        };
        self.findings.push(Finding {
            rule_id: id.to_owned(),
            severity,
            kind,
            page,
            target,
            rect: None,
            message,
            measured: None,
        });
    }
}

/// Decode a PDF text string for a message or a pattern test.
fn text(bytes: &[u8]) -> String {
    crate::textstring::decode_text_string(bytes).text
}

/// The bytes of a string object, resolved.
fn string_of<'a>(
    view: &'a DocumentView<'_>,
    o: Option<&'a crate::object::Object>,
) -> Option<&'a [u8]> {
    match o.map(|o| view.resolve(o)) {
        Some(crate::object::Object::String(s)) => Some(s.as_slice()),
        _ => None,
    }
}

/// A name object's bytes, resolved.
fn name_of<'a>(
    view: &'a DocumentView<'_>,
    o: Option<&'a crate::object::Object>,
) -> Option<&'a [u8]> {
    o.map(|o| view.resolve(o))
        .and_then(crate::object::Object::as_name)
        .map(|n| n.as_bytes())
}

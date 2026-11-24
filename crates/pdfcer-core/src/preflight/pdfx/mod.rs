//! PDF/X conformance validation (ISO 15930-1, -3, -4, -6 and -7): one
//! preflight rule per requirement of the spec RAG's inventory
//! (`pdfx__req__by_axis.md`), each finding naming the clause it rests on.
//!
//! A requirement a machine cannot decide is reported as
//! [`FindingKind::NeedsPerson`]; one whose clause for the level is not
//! sourced, or which pdfcer does not check yet, as an Info-severity
//! [`FindingKind::Incomplete`]. Neither is ever a pass, so
//! [`PdfxVerdict::Conforms`] is not reachable while the inventory has
//! NOT SOURCED rows for the level, and today every level has some.

mod doc_rows;
mod objects;
mod page_rows;
mod rows;
#[cfg(test)]
mod tests;

use crate::PdfVersion;
use crate::page_tree::Page;
use crate::prepress::{PdfxClaim, pdfx_claim};
use crate::view::DocumentView;

use super::checks::Sink;
use super::walk::{Event, Origin};
use super::{
    Check, Finding, FindingKind, FindingTarget, PreflightReport, Profile, ProfileRule, RunOptions,
    Severity,
};
use rows::{How, Row, State};

/// A PDF/X conformance level pdfcer validates against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PdfxLevel {
    /// PDF/X-1a:2001, ISO 15930-1:2001, on PDF 1.3.
    X1a2001,
    /// PDF/X-1a:2003, ISO 15930-4:2003, on PDF 1.4.
    X1a2003,
    /// PDF/X-3:2002, ISO 15930-3:2002, on PDF 1.3.
    X3_2002,
    /// PDF/X-3:2003, ISO 15930-6:2003, on PDF 1.4.
    X3_2003,
    /// PDF/X-4, ISO 15930-7:2010, on PDF 1.6.
    X4,
}

impl PdfxLevel {
    /// Every level, oldest first.
    pub const ALL: [Self; 5] = [
        Self::X1a2001,
        Self::X1a2003,
        Self::X3_2002,
        Self::X3_2003,
        Self::X4,
    ];

    /// The name the CLI takes: `x-1a:2001`, `x-1a:2003`, `x-3:2002`,
    /// `x-3:2003`, `x-4`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::X1a2001 => "x-1a:2001",
            Self::X1a2003 => "x-1a:2003",
            Self::X3_2002 => "x-3:2002",
            Self::X3_2003 => "x-3:2003",
            Self::X4 => "x-4",
        }
    }

    /// Parse [`Self::as_str`]'s spelling, case-insensitively; `x-1a` and
    /// `x-3` without a year mean the 2003 editions.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.to_ascii_lowercase();
        match name.as_str() {
            "x-1a" => Some(Self::X1a2003),
            "x-3" => Some(Self::X3_2003),
            n => Self::ALL.into_iter().find(|l| l.as_str() == n),
        }
    }

    /// The `GTS_PDFXVersion` value a conforming file claims
    /// (`PDF/X-1a:2001`, ..., `PDF/X-4`).
    #[must_use]
    pub const fn claim(self) -> &'static str {
        match self {
            Self::X1a2001 => "PDF/X-1a:2001",
            Self::X1a2003 => "PDF/X-1a:2003",
            Self::X3_2002 => "PDF/X-3:2002",
            Self::X3_2003 => "PDF/X-3:2003",
            Self::X4 => "PDF/X-4",
        }
    }

    /// The PDF version the level is based on; a feature introduced later
    /// is outside it.
    #[must_use]
    pub const fn based_on(self) -> PdfVersion {
        let minor = match self {
            Self::X1a2001 | Self::X3_2002 => 3,
            Self::X1a2003 | Self::X3_2003 => 4,
            Self::X4 => 6,
        };
        PdfVersion { major: 1, minor }
    }

    /// The built-in preflight profile name (`pdfx-1a-2001`, ..., `pdfx-4`).
    #[must_use]
    pub const fn profile_name(self) -> &'static str {
        match self {
            Self::X1a2001 => "pdfx-1a-2001",
            Self::X1a2003 => "pdfx-1a-2003",
            Self::X3_2002 => "pdfx-3-2002",
            Self::X3_2003 => "pdfx-3-2003",
            Self::X4 => "pdfx-4",
        }
    }

    /// The inventory's level code.
    pub(super) const fn code(self) -> &'static str {
        match self {
            Self::X1a2001 => "1a01",
            Self::X1a2003 => "1a03",
            Self::X3_2002 => "3-02",
            Self::X3_2003 => "3-03",
            Self::X4 => "4",
        }
    }

    /// The ISO 15930 part number defining the level.
    const fn part_digit(self) -> &'static str {
        match self {
            Self::X1a2001 => "1",
            Self::X1a2003 => "4",
            Self::X3_2002 => "3",
            Self::X3_2003 => "6",
            Self::X4 => "7",
        }
    }

    const fn is_2003(self) -> bool {
        matches!(self, Self::X1a2003 | Self::X3_2003)
    }
}

/// The overall answer of [`validate_pdfx`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PdfxVerdict {
    /// An error-severity requirement failed: the file is not this level.
    Fails,
    /// Nothing failed, but something warned, needs a person or was not
    /// checked, or the run was cancelled.
    Unconfirmed,
    /// Every requirement was checked and passed.
    Conforms,
}

impl PdfxVerdict {
    /// `fails`, `unconfirmed` or `conforms`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fails => "fails",
            Self::Unconfirmed => "unconfirmed",
            Self::Conforms => "conforms",
        }
    }
}

/// What [`validate_pdfx`] found.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PdfxReport {
    /// The level validated against.
    pub level: PdfxLevel,
    /// The level the file itself claims, if any.
    pub claim: Option<PdfxClaim>,
    /// The findings; each [`Finding::rule_id`] is an inventory row id
    /// (`PXQ-FT-1`) and each message ends with its clause in parentheses.
    pub preflight: PreflightReport,
}

impl PdfxReport {
    /// The overall answer.
    #[must_use]
    pub fn verdict(&self) -> PdfxVerdict {
        let p = &self.preflight;
        if p.fails() {
            PdfxVerdict::Fails
        } else if p.is_clean() {
            PdfxVerdict::Conforms
        } else {
            PdfxVerdict::Unconfirmed
        }
    }

    /// The clause a finding rests on, for this report's level, as text
    /// (`ISO 15930-7:2010 §6.17`), or `None` when its rule is not a PDF/X
    /// row.
    #[must_use]
    pub fn clause_of(&self, finding: &Finding) -> Option<String> {
        rows::find(&finding.rule_id).map(|r| r.clause(self.level))
    }
}

/// Validate the document against `level`, with the default options.
/// Read-only.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::preflight::{PdfxLevel, PdfxVerdict, validate_pdfx};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../../../fixtures/synthetic/hello.pdf").to_vec(),
/// )?;
/// let report = validate_pdfx(&doc.view(), PdfxLevel::X4);
/// assert_eq!(report.verdict(), PdfxVerdict::Fails);
/// assert!(report.preflight.findings.iter().any(|f| f.rule_id == "PXQ-OI-1"));
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn validate_pdfx(view: &DocumentView<'_>, level: PdfxLevel) -> PdfxReport {
    validate_pdfx_with(view, level, &RunOptions::default())
}

/// [`validate_pdfx`] with a result cap, cancellation and progress.
#[must_use]
pub fn validate_pdfx_with(
    view: &DocumentView<'_>,
    level: PdfxLevel,
    options: &RunOptions<'_>,
) -> PdfxReport {
    let mut preflight = super::run_with(view, &Profile::pdfx(level), options);
    for f in &mut preflight.findings {
        if let Some(clause) = rows::find(&f.rule_id).map(|r| r.clause(level))
            && !f.message.contains(&clause)
        {
            f.message = with_clause(&f.message, &clause);
        }
    }
    PdfxReport {
        level,
        claim: pdfx_claim(view),
        preflight,
    }
}

impl Profile {
    /// The built-in profile for `level`: one rule per inventory row that
    /// applies to it, id the row id. Rows a machine cannot decide warn;
    /// rows graded only by an all-parts summary warn; advisory rows warn;
    /// NOT SOURCED and not-yet-checked rows report at info.
    #[must_use]
    pub fn pdfx(level: PdfxLevel) -> Self {
        let rules = rows::all()
            .filter_map(|row| {
                let state = row.state(level)?;
                let check = Check::PdfxRequirement { level, row: row.id };
                Some(ProfileRule::new(
                    row.id,
                    check,
                    severity(&row, level, state),
                ))
            })
            .collect();
        Self {
            name: level.claim().to_owned(),
            description: format!(
                "{} requirements, each naming its ISO 15930 clause.",
                level.claim()
            ),
            rules,
        }
    }
}

fn severity(row: &Row, level: PdfxLevel, state: State) -> Severity {
    match state {
        State::NotSourced | State::NotYet => Severity::Info,
        State::Checked if matches!(row.id, "PXQ-BX-5" | "PXQ-ID-9") => Severity::Info,
        State::Checked if row.person || row.advisory || row.grade(level) == Some('K') => {
            Severity::Warning
        }
        State::Checked => Severity::Error,
    }
}

/// The static id of row `id` when it yields a rule for `level`.
pub(super) fn row_id(id: &str, level: PdfxLevel) -> Option<&'static str> {
    rows::find(id)
        .filter(|r| r.state(level).is_some())
        .map(|r| r.id)
}

/// Whether `row` is checked page by page for `level`.
pub(super) fn is_page_row(row: &str, level: PdfxLevel) -> bool {
    rows::find(row).is_some_and(|r| r.how == How::Page && r.state(level) == Some(State::Checked))
}

/// The rule, its row and its level, making the row's findings.
pub(super) struct Ctx<'r> {
    rule: &'r ProfileRule,
    row: Row,
    level: PdfxLevel,
}

impl Ctx<'_> {
    fn make(
        &self,
        kind: FindingKind,
        page: Option<usize>,
        target: FindingTarget,
        m: &str,
    ) -> Finding {
        let severity = match kind {
            FindingKind::NeedsPerson => self.rule.severity.min(Severity::Warning),
            _ => self.rule.severity,
        };
        Finding {
            rule_id: self.rule.id.clone(),
            severity,
            kind,
            page,
            target,
            rect: None,
            message: with_clause(m, &self.row.clause(self.level)),
            measured: None,
        }
    }

    /// A violation.
    fn found(&self, page: Option<usize>, target: FindingTarget, m: &str) -> Finding {
        self.make(FindingKind::Violation, page, target, m)
    }

    /// A document-level violation.
    fn doc(&self, m: &str) -> Finding {
        self.found(None, FindingTarget::Document, m)
    }

    /// A question for a person.
    fn person(&self, page: Option<usize>, target: FindingTarget, m: &str) -> Finding {
        self.make(FindingKind::NeedsPerson, page, target, m)
    }

    /// Data that could not be read, at the rule's severity.
    fn unread(&self, page: Option<usize>, m: &str) -> Finding {
        let target = page.map_or(FindingTarget::Document, |_| FindingTarget::Page);
        let m = format!("Not checked: {m}.");
        self.make(FindingKind::Incomplete, page, target, &m)
    }

    /// A limit of the check itself, at info severity.
    fn coverage(&self, page: Option<usize>, m: &str) -> Finding {
        Finding {
            severity: Severity::Info,
            ..self.unread(page, m)
        }
    }

    /// Findings from a shared preflight check run with `rule`, the clause
    /// appended.
    fn relay(
        &self,
        rule: &ProfileRule,
        sink: &mut Sink,
        run: impl FnOnce(&ProfileRule, &mut Sink),
    ) {
        let mut inner = Sink::new(usize::MAX);
        run(rule, &mut inner);
        for f in inner.finish().0 {
            let kind = f.kind;
            sink.push(Finding {
                rect: f.rect,
                measured: f.measured,
                ..self.make(kind, f.page, f.target, &f.message)
            });
        }
    }
}

/// `message` with `(clause)` before its final full stop.
fn with_clause(message: &str, clause: &str) -> String {
    let body = message.strip_suffix('.').unwrap_or(message);
    format!("{body} ({clause}).")
}

/// The document-level part of a PDF/X rule.
pub(super) fn document_row(
    view: &DocumentView<'_>,
    rule: &ProfileRule,
    level: PdfxLevel,
    id: &str,
    sink: &mut Sink,
) {
    let Some(row) = rows::find(id) else {
        return;
    };
    let ctx = Ctx { rule, row, level };
    match row.state(level) {
        Some(State::NotSourced) => sink.push(ctx.coverage(
            None,
            &format!(
                "{} ({}): the requirement text for {} is NOT SOURCED",
                row.id,
                row.title,
                level.claim()
            ),
        )),
        Some(State::NotYet) => sink.push(ctx.coverage(
            None,
            &format!("pdfcer does not check {} ({}) yet", row.id, row.title),
        )),
        _ => {}
    }
    // The object walk claims the rows it decides; the rest are document rows.
    if row.state(level) == Some(State::Checked)
        && row.how == How::Document
        && !objects::check(view, &ctx, sink)
    {
        doc_rows::check(view, &ctx, sink);
    }
}

/// What a page-level row looks at.
pub(super) struct PageCtx<'a, 'v> {
    pub(super) view: &'a DocumentView<'v>,
    pub(super) page: &'a Page,
    pub(super) number: usize,
    pub(super) events: &'a [(Origin, Event)],
}

/// The page-level part of a PDF/X rule.
pub(super) fn page_row(
    rule: &ProfileRule,
    level: PdfxLevel,
    id: &str,
    page: &PageCtx<'_, '_>,
    sink: &mut Sink,
) {
    if let Some(row) = rows::find(id).filter(|_| is_page_row(id, level)) {
        page_rows::check(&Ctx { rule, row, level }, page, sink);
    }
}

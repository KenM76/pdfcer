//! PDF/A conformance validation, levels 1b, 2b and 3b (ISO 19005-1, -2,
//! -3): one preflight rule per requirement of the spec RAG's inventory
//! (`pdfa__req__inventory_index.md`), each finding naming its clause.
//!
//! A row pdfcer does not check yet, or whose text is NOT SOURCED, is an
//! Info-severity [`FindingKind::Incomplete`]; so is every limit of a row
//! that is checked. Neither is a pass, so [`PdfaVerdict::Conforms`] is not
//! reachable while any such row applies, and today every level has some.

mod doc_rows;
mod fonts;
mod gstates;
mod icc;
mod objects;
mod page_rows;
mod rows;
mod spaces;
#[cfg(test)]
mod tests;
mod xmp;

use crate::PdfVersion;
use crate::font_unembed::{PdfaClaim, detect_pdfa};
use crate::page_tree::Page;
use crate::view::DocumentView;

use super::checks::Sink;
use super::walk::{Event, Origin};
use super::{
    Check, Finding, FindingKind, FindingTarget, PreflightReport, Profile, ProfileRule, RunOptions,
    Severity,
};
use rows::{How, Row, State};

/// A PDF/A conformance level pdfcer validates against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PdfaLevel {
    /// PDF/A-1b, ISO 19005-1:2005, on PDF 1.4.
    A1b,
    /// PDF/A-2b, ISO 19005-2:2011, on PDF 1.7 (ISO 32000-1).
    A2b,
    /// PDF/A-3b, ISO 19005-3:2012, on PDF 1.7 (ISO 32000-1).
    A3b,
}

impl PdfaLevel {
    /// Every level, oldest first.
    pub const ALL: [Self; 3] = [Self::A1b, Self::A2b, Self::A3b];

    /// The name the CLI takes: `1b`, `2b`, `3b`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A1b => "1b",
            Self::A2b => "2b",
            Self::A3b => "3b",
        }
    }

    /// Parse [`Self::as_str`]'s spelling, case-insensitively, with or
    /// without an `a-` or `pdfa-` prefix (`2B`, `a-2b`, `pdfa-2b`).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.to_ascii_lowercase();
        let bare = name
            .strip_prefix("pdfa-")
            .or_else(|| name.strip_prefix("a-"))
            .unwrap_or(&name);
        Self::ALL.into_iter().find(|l| l.as_str() == bare)
    }

    /// The level as written in prose: `PDF/A-1b`, `PDF/A-2b`, `PDF/A-3b`.
    #[must_use]
    pub const fn claim(self) -> &'static str {
        match self {
            Self::A1b => "PDF/A-1b",
            Self::A2b => "PDF/A-2b",
            Self::A3b => "PDF/A-3b",
        }
    }

    /// The ISO 19005 part, which a conforming file writes as
    /// `pdfaid:part`.
    #[must_use]
    pub const fn part(self) -> u8 {
        match self {
            Self::A1b => 1,
            Self::A2b => 2,
            Self::A3b => 3,
        }
    }

    /// The PDF version the level is based on; a feature introduced later
    /// is outside it.
    #[must_use]
    pub const fn based_on(self) -> PdfVersion {
        let minor = match self {
            Self::A1b => 4,
            Self::A2b | Self::A3b => 7,
        };
        PdfVersion { major: 1, minor }
    }

    /// The built-in preflight profile name (`pdfa-1b`, `pdfa-2b`,
    /// `pdfa-3b`).
    #[must_use]
    pub const fn profile_name(self) -> &'static str {
        match self {
            Self::A1b => "pdfa-1b",
            Self::A2b => "pdfa-2b",
            Self::A3b => "pdfa-3b",
        }
    }

    /// The standard, with its edition year.
    pub(super) const fn standard(self) -> &'static str {
        match self {
            Self::A1b => "ISO 19005-1:2005",
            Self::A2b => "ISO 19005-2:2011",
            Self::A3b => "ISO 19005-3:2012",
        }
    }
}

/// The overall answer of [`validate_pdfa`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PdfaVerdict {
    /// An error-severity requirement failed: the file is not this level.
    Fails,
    /// Nothing failed, but something warned or was not checked, or the run
    /// was cancelled.
    Unconfirmed,
    /// Every requirement was checked and passed.
    Conforms,
}

impl PdfaVerdict {
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

/// What [`validate_pdfa`] found.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PdfaReport {
    /// The level validated against.
    pub level: PdfaLevel,
    /// What the file itself claims (`pdfaid:part` and `pdfaid:conformance`).
    pub claim: PdfaClaim,
    /// The findings; each [`Finding::rule_id`] is an inventory row id
    /// (`PAI-FS-15`) and each message ends with its clause in parentheses.
    pub preflight: PreflightReport,
}

impl PdfaReport {
    /// The overall answer.
    #[must_use]
    pub fn verdict(&self) -> PdfaVerdict {
        let p = &self.preflight;
        if p.fails() {
            PdfaVerdict::Fails
        } else if p.is_clean() {
            PdfaVerdict::Conforms
        } else {
            PdfaVerdict::Unconfirmed
        }
    }

    /// The clause a finding rests on, for this report's level, as text
    /// (`ISO 19005-2:2011 §6.1.7.2`), or `None` when its rule is not a
    /// PDF/A row.
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
/// use pdfcer_core::preflight::{PdfaLevel, PdfaVerdict, validate_pdfa};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../../../fixtures/synthetic/hello.pdf").to_vec(),
/// )?;
/// let report = validate_pdfa(&doc.view(), PdfaLevel::A2b);
/// assert_eq!(report.verdict(), PdfaVerdict::Fails);
/// assert!(report.preflight.findings.iter().any(|f| f.rule_id == "PAI-MD-1"));
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn validate_pdfa(view: &DocumentView<'_>, level: PdfaLevel) -> PdfaReport {
    validate_pdfa_with(view, level, &RunOptions::default())
}

/// [`validate_pdfa`] with a result cap, cancellation and progress.
#[must_use]
pub fn validate_pdfa_with(
    view: &DocumentView<'_>,
    level: PdfaLevel,
    options: &RunOptions<'_>,
) -> PdfaReport {
    let mut preflight = super::run_with(view, &Profile::pdfa(level), options);
    for f in &mut preflight.findings {
        if let Some(clause) = rows::find(&f.rule_id).map(|r| r.clause(level))
            && !f.message.contains(&clause)
        {
            f.message = with_clause(&f.message, &clause);
        }
    }
    PdfaReport {
        level,
        claim: detect_pdfa(view),
        preflight,
    }
}

impl Profile {
    /// The built-in profile for `level`: one rule per inventory row that
    /// applies to it, id the row id. Advisory rows warn; NOT SOURCED and
    /// not-yet-checked rows report at info; the rest are errors.
    #[must_use]
    pub fn pdfa(level: PdfaLevel) -> Self {
        let rules = rows::all()
            .filter_map(|row| {
                let state = row.state(level)?;
                let check = Check::PdfaRequirement { level, row: row.id };
                Some(ProfileRule::new(row.id, check, severity(&row, state)))
            })
            .collect();
        Self {
            name: level.claim().to_owned(),
            description: format!(
                "{} requirements, each naming its ISO 19005 clause.",
                level.claim()
            ),
            rules,
        }
    }
}

fn severity(row: &Row, state: State) -> Severity {
    match state {
        State::NotSourced | State::NotYet => Severity::Info,
        State::Checked if row.advisory => Severity::Warning,
        State::Checked => Severity::Error,
    }
}

/// The static id of row `id` when it yields a rule for `level`.
pub(super) fn row_id(id: &str, level: PdfaLevel) -> Option<&'static str> {
    rows::find(id)
        .filter(|r| r.state(level).is_some())
        .map(|r| r.id)
}

/// Whether `row` is checked page by page for `level`.
pub(super) fn is_page_row(row: &str, level: PdfaLevel) -> bool {
    rows::find(row).is_some_and(|r| r.how == How::Page && r.state(level) == Some(State::Checked))
}

/// Whether `row` reads the preflight content walk.
pub(super) fn uses_walk(row: &str) -> bool {
    matches!(row, "PAI-CS-4" | "PAI-CS-5" | "PAI-CS-6")
}

/// The rule, its row and its level, making the row's findings.
pub(super) struct Ctx<'r> {
    rule: &'r ProfileRule,
    row: Row,
    level: PdfaLevel,
}

impl Ctx<'_> {
    fn make(
        &self,
        kind: FindingKind,
        page: Option<usize>,
        target: FindingTarget,
        m: &str,
    ) -> Finding {
        Finding {
            rule_id: self.rule.id.clone(),
            severity: self.rule.severity,
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
            sink.push(Finding {
                rect: f.rect,
                measured: f.measured,
                ..self.make(f.kind, f.page, f.target, &f.message)
            });
        }
    }
}

/// `message` with `(clause)` before its final full stop.
fn with_clause(message: &str, clause: &str) -> String {
    let body = message.strip_suffix('.').unwrap_or(message);
    format!("{body} ({clause}).")
}

/// The document-level part of a PDF/A rule: its coverage notes, and the
/// check itself when it is a document row.
pub(super) fn document_row(
    view: &DocumentView<'_>,
    rule: &ProfileRule,
    level: PdfaLevel,
    id: &str,
    sink: &mut Sink,
) {
    let Some(row) = rows::find(id) else {
        return;
    };
    let ctx = Ctx { rule, row, level };
    let what = format!("{} ({})", row.id, row.title);
    match row.state(level) {
        Some(State::NotSourced) => sink.push(ctx.coverage(
            None,
            &format!(
                "{what}: the requirement text for {} is NOT SOURCED",
                level.claim()
            ),
        )),
        Some(State::NotYet) => {
            sink.push(ctx.coverage(None, &format!("pdfcer does not check {what} yet")));
        }
        Some(State::Checked) => {
            if !row.note.is_empty() {
                sink.push(ctx.coverage(None, &format!("{}: {}", row.id, row.note)));
            }
            if row.how == How::Document {
                let _ = objects::check(view, &ctx, sink)
                    || spaces::check(view, &ctx, sink)
                    || gstates::check(view, &ctx, sink)
                    || fonts::check(view, &ctx, sink)
                    || xmp::check(view, &ctx, sink)
                    || doc_rows::check(view, &ctx, sink);
            }
        }
        None => {}
    }
}

/// What a page-level row looks at.
pub(super) struct PageCtx<'a, 'v> {
    pub(super) view: &'a DocumentView<'v>,
    pub(super) page: &'a Page,
    pub(super) number: usize,
    pub(super) events: &'a [(Origin, Event)],
}

/// The page-level part of a PDF/A rule.
pub(super) fn page_row(
    rule: &ProfileRule,
    level: PdfaLevel,
    id: &str,
    page: &PageCtx<'_, '_>,
    sink: &mut Sink,
) {
    if let Some(row) = rows::find(id).filter(|_| is_page_row(id, level)) {
        page_rows::check(&Ctx { rule, row, level }, page, sink);
    }
}

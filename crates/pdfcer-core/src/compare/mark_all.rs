//! One gesture that marks a visual comparison's regions and a text
//! comparison's changes, as one undo entry.

use crate::edit::{CommandKind, EditSession};

use super::CompareReport;
use super::region_marks::{RegionMark, RegionReport, mark_regions};
use super::result::{ResultError, ResultOptions, ResultReport, mark_changes};

/// What [`mark_comparison`] did.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ComparisonMarks {
    /// The region marks' report; `None` when no regions were given.
    pub regions: Option<RegionReport>,
    /// The change marks' report, including the summary page.
    pub changes: ResultReport,
}

/// Why [`mark_comparison`] failed, and what it had already authored.
///
/// [`ResultError::Pages`] from the second stage comes after the region marks
/// are in the session, so the error alone cannot say whether anything was
/// authored; this does.
#[derive(Debug)]
#[non_exhaustive]
pub struct MarkComparisonError {
    /// What failed.
    pub error: ResultError,
    /// The region marks' report when that stage completed before the
    /// failure; `None` when no regions were given or that stage failed.
    pub regions: Option<RegionReport>,
    /// Whether anything was authored. When `true` it is in the session as
    /// ONE undo entry of kind [`CommandKind::MarkComparison`].
    pub authored: bool,
}

impl std::fmt::Display for MarkComparisonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for MarkComparisonError {
    // Display already is `error`'s message, so the chain continues at its
    // source rather than repeating it.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(&self.error)
    }
}

/// [`mark_regions`] with `regions` (when given), then [`mark_changes`] with
/// `report`, on the NEW document's session — the order the summary page
/// needs. Everything authored is ONE undo entry of kind
/// [`CommandKind::MarkComparison`], so one Undo takes the whole marking back.
///
/// # Errors
///
/// [`MarkComparisonError`] (boxed: it carries a report) carrying the [`ResultError`] from either stage,
/// the region report when that stage had completed, and whether anything
/// was authored. Whatever was authored stays in the session as one undo
/// entry.
pub fn mark_comparison(
    session: &mut EditSession,
    regions: Option<&[RegionMark]>,
    report: &CompareReport,
    options: &ResultOptions,
) -> Result<ComparisonMarks, Box<MarkComparisonError>> {
    let before = session.undo_top_serial();
    let mut marked = None;
    let out = session.grouped(CommandKind::MarkComparison, |s| {
        if let Some(r) = regions {
            marked = Some(mark_regions(s, r, options)?);
        }
        mark_changes(s, report, options)
    });
    conclude(session, before, marked, out)
}

/// `before` is the undo top serial taken before the gesture; serials only
/// grow, so any change means something committed.
fn conclude(
    session: &EditSession,
    before: u64,
    regions: Option<RegionReport>,
    out: Result<ResultReport, ResultError>,
) -> Result<ComparisonMarks, Box<MarkComparisonError>> {
    match out {
        Ok(changes) => Ok(ComparisonMarks { regions, changes }),
        Err(error) => Err(Box::new(MarkComparisonError {
            error,
            regions,
            authored: session.undo_top_serial() != before,
        })),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)] // a test failing on a broken fixture is the point
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::edit::InfoField;
    use crate::text_edit::placetext::PlaceTextError;

    fn session() -> EditSession {
        let bytes = include_bytes!("../../../../fixtures/synthetic/hello.pdf");
        EditSession::new(Document::from_bytes(bytes.to_vec()).expect("fixture parses"))
    }

    fn failed() -> Result<ResultReport, ResultError> {
        Err(ResultError::Summary(PlaceTextError::EmptyText))
    }

    #[test]
    fn a_failure_after_a_commit_says_it_authored() {
        let mut s = session();
        let before = s.undo_top_serial();
        s.set_info_field(InfoField::Title, Some("t")).expect("sets");
        let err = conclude(&s, before, None, failed()).expect_err("fails");
        assert!(err.authored);
    }

    #[test]
    fn a_failure_with_nothing_committed_says_so() {
        let mut s = session();
        s.set_info_field(InfoField::Title, Some("t")).expect("sets");
        let before = s.undo_top_serial();
        let err = conclude(&s, before, None, failed()).expect_err("fails");
        assert!(!err.authored);
        assert_eq!(err.to_string(), err.error.to_string());
    }
}

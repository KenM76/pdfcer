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

/// [`mark_regions`] with `regions` (when given), then [`mark_changes`] with
/// `report`, on the NEW document's session — the order the summary page
/// needs. Everything authored is ONE undo entry of kind
/// [`CommandKind::MarkComparison`], so one Undo takes the whole marking back.
///
/// # Errors
///
/// As [`mark_regions`] and [`mark_changes`]. On an error, whatever was
/// already authored stays in the session, still as one undo entry.
pub fn mark_comparison(
    session: &mut EditSession,
    regions: Option<&[RegionMark]>,
    report: &CompareReport,
    options: &ResultOptions,
) -> Result<ComparisonMarks, ResultError> {
    session.grouped(CommandKind::MarkComparison, |s| {
        let regions = regions.map(|r| mark_regions(s, r, options)).transpose()?;
        let changes = mark_changes(s, report, options)?;
        Ok(ComparisonMarks { regions, changes })
    })
}

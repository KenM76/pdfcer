//! `dimension-area` — switch a closed perimeter ce dimension between its
//! perimeter and its enclosed-area reading.

use super::*;

/// Which quantity `dimension-area` makes a closed perimeter ce dimension
/// report.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum AreaReading {
    /// The length of the closed outline.
    Perimeter,
    /// The area the outline encloses, in the group's unit squared.
    Area,
}

impl AreaReading {
    /// A stable token for CLI output.
    pub(crate) const fn token(self) -> &'static str {
        match self {
            AreaReading::Perimeter => "perimeter",
            AreaReading::Area => "area",
        }
    }
}

/// `dimension-area`.
///
/// ## Contract
///
/// - Emits one `dimension-area …` line with the usual save-report fields,
///   then defers the exit code to [`finish_edit`].
/// - An unknown id, a ce dimension with no vertices, an open path or fewer
///   than three vertices is refused through [`report_edit_error`] before any
///   mutation, with core's named error.
pub(crate) fn cmd_dimension_area(
    input: &Path,
    dimension: u32,
    show: AreaReading,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    if let Err(err) = session.set_dimension_area(
        pdfcer_core::dimension::DimensionId(dimension),
        matches!(show, AreaReading::Area),
    ) {
        return report_edit_error(input, &err);
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "dimension-area {} dimension={dimension} show={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        show.token(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

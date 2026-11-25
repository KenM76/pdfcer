//! Display edits on a placed ce dimension: `dimension-area` (a closed
//! perimeter's perimeter or enclosed area), `dimension-circle` (a
//! radius/diameter ce dimension's circle shown or hidden) and
//! `dimension-centre-mark` (its centre mark set, replaced or removed).

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

/// Whether the circle of a radius/diameter ce dimension is drawn.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum CircleShownArg {
    /// Draw the circle (the default for a new ce dimension).
    Shown,
    /// Draw only the leader, label and any centre mark.
    Hidden,
}

impl CircleShownArg {
    const fn token(self) -> &'static str {
        match self {
            Self::Shown => "shown",
            Self::Hidden => "hidden",
        }
    }
}

/// `dimension-circle`: one `dimension-circle …` line with the usual save
/// fields, then [`finish_edit`]'s exit code. An unknown id or a ce dimension
/// that is not radius/diameter is refused before anything is written.
pub(crate) fn cmd_dimension_circle(
    input: &Path,
    dimension: u32,
    circle: CircleShownArg,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let id = pdfcer_core::dimension::DimensionId(dimension);
    if let Err(err) = session.set_circular_circle_shown(id, circle == CircleShownArg::Shown) {
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
        "dimension-circle {} dimension={dimension} circle={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        circle.token(),
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

/// The arguments of `dimension-centre-mark`; `mark: None` is `--clear`.
pub(crate) struct CentreMarkEdit<'a> {
    pub(crate) input: &'a Path,
    pub(crate) dimension: u32,
    pub(crate) mark: Option<pdfcer_core::dimension::CentreMarkSpec>,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `dimension-centre-mark` — set, replace or clear a ce dimension's centre
/// mark, disclosing a label the change moved.
pub(crate) fn cmd_dimension_centre_mark(args: &CentreMarkEdit<'_>) -> u8 {
    let input = args.input;
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let id = pdfcer_core::dimension::DimensionId(args.dimension);
    let moved = match session.set_centre_mark(id, args.mark) {
        Ok(outcome) => outcome.label_moved,
        Err(err) => return report_edit_error(input, &err),
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let mark = args.mark.map_or_else(
        || "none".to_owned(),
        |m| {
            format!(
                "on extension_lines={} mark_angle={}",
                if m.extension_lines { "on" } else { "off" },
                m.angle
            )
        },
    );
    let r = &outcome.report;
    println!(
        "dimension-centre-mark {} dimension={} centre_mark={mark} label_moved={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        args.dimension,
        u32::from(moved),
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    if moved {
        println!("  note: label moved outside the rim to clear the centre mark");
    }
    finish_edit(input, &outcome)
}

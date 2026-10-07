//! `set-object-paint` — recolour page paths' fill and/or stroke.

use super::*;
use pdfcer_core::edit::{PaintOutcome, PaintRefusalReason};

/// `set-object-paint` arguments, after clap.
pub(crate) struct SetObjectPaintArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) objects: &'a [usize],
    pub(crate) fill: Option<pdfcer_core::vector::Rgb>,
    pub(crate) stroke: Option<pdfcer_core::vector::Rgb>,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `set-object-paint` — wraps each path in `q <colour> … Q`; spot inks and
/// patterns are refused by name on stderr and counted on the result line.
pub(crate) fn cmd_set_object_paint(args: &SetObjectPaintArgs) -> u8 {
    let input = args.input;
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let page_index = (args.page.max(1) - 1) as usize;
    let paint = match session.set_object_paint(page_index, args.objects, args.fill, args.stroke) {
        Ok(paint) => paint,
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
    report_refusals(input, &paint);
    let r = &outcome.report;
    println!(
        "set-object-paint {} page={} mode={} -> {}; changed={} refused={} objects_written={} appended={} out_bytes={}",
        input.display(),
        args.page,
        args.mode.name(),
        args.output.display(),
        join_indices(&paint.changed),
        paint.refused.len(),
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
    );
    finish_edit(input, &outcome)
}

/// One stderr line per object left alone, naming the ink when there is one.
fn report_refusals(input: &Path, paint: &PaintOutcome) {
    for refusal in &paint.refused {
        let why = match refusal.reason {
            PaintRefusalReason::UndecodedColourSpace => {
                "its paint is a colour space pdfcer will not rewrite (a spot or calibrated ink)"
            }
            PaintRefusalReason::Pattern => "it is painted with a pattern, which has no colour",
            PaintRefusalReason::NotAPath => "it is not a path",
            _ => "pdfcer left it alone",
        };
        let space = refusal.space.as_deref().map_or_else(String::new, |s| {
            format!(" (colour space /{})", String::from_utf8_lossy(s))
        });
        eprintln!(
            "pdfcer: {}: object {} not recoloured: {why}{space}",
            input.display(),
            refusal.object
        );
    }
}

/// `none`, or the indices comma-separated.
pub(crate) fn join_indices(indices: &[usize]) -> String {
    if indices.is_empty() {
        return "none".to_owned();
    }
    indices
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

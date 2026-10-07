//! `set-object-stroke-style` — line width, dash and constant alpha on page
//! paths.

use super::*;
use pdfcer_core::vector::{Dash, StrokeStyle};

/// `set-object-stroke-style` arguments, after clap.
pub(crate) struct SetObjectStrokeStyleArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) objects: &'a [usize],
    pub(crate) style: StrokeStyle,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// A parsed `--dash` value (a newtype so clap takes it as one value).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DashArray(pub(crate) Vec<f64>);

/// `--dash`: `solid`, or dash and gap lengths separated by commas.
pub(crate) fn parse_dash_array(spec: &str) -> Result<DashArray, String> {
    if spec.eq_ignore_ascii_case("solid") {
        return Ok(DashArray(Vec::new()));
    }
    spec.split(',')
        .map(|v| {
            v.trim()
                .parse::<f64>()
                .map_err(|_| format!("`{v}` is not a number (expected e.g. 3,2 or solid)"))
        })
        .collect::<Result<_, _>>()
        .map(DashArray)
}

/// Assemble the core request from the parsed flags.
pub(crate) fn stroke_style_from(
    width: Option<f64>,
    dash: Option<DashArray>,
    dash_phase: f64,
    stroke_alpha: Option<f64>,
    fill_alpha: Option<f64>,
) -> StrokeStyle {
    StrokeStyle {
        width,
        dash: dash.map(|DashArray(array)| Dash::new(array, dash_phase)),
        stroke_alpha,
        fill_alpha,
    }
}

/// `set-object-stroke-style` — wraps each path in `q <w> <d> <gs> … Q`;
/// text and images are refused by index on stderr.
pub(crate) fn cmd_set_object_stroke_style(args: &SetObjectStrokeStyleArgs) -> u8 {
    let input = args.input;
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let page_index = (args.page.max(1) - 1) as usize;
    let styled = match session.set_object_stroke_style(page_index, args.objects, &args.style) {
        Ok(styled) => styled,
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
    for refusal in &styled.refused {
        eprintln!(
            "pdfcer: {}: object {} not styled: it is not a path",
            input.display(),
            refusal.object
        );
    }
    let r = &outcome.report;
    println!(
        "set-object-stroke-style {} page={} mode={} -> {}; changed={} refused={} objects_written={} appended={} out_bytes={}",
        input.display(),
        args.page,
        args.mode.name(),
        args.output.display(),
        join_indices(&styled.changed),
        styled.refused.len(),
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
    );
    finish_edit(input, &outcome)
}

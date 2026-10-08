//! `set-annot-opacity` and `set-marker-color` (pdfcer-gui request G150).

use super::*;
use pdfcer_core::edit::{AppearanceWrite, MarkerStyle, StyleEdit};

/// Open `input`, find the annotation, apply `edit`, save and print.
fn restyle(
    input: &Path,
    page: usize,
    index: usize,
    output: &Path,
    mode: SaveMode,
    edit: impl FnOnce(
        &mut pdfcer_core::edit::EditSession,
        pdfcer_core::object::ObjId,
    ) -> Result<String, pdfcer_core::edit::EditError>,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let annot_id = match resolve_annotation(&session, input, page, index) {
        Ok(id) => id,
        Err(code) => return code,
    };
    let line = match edit(&mut session, annot_id) {
        Ok(line) => line,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    println!("{line}");
    finish_edit(input, &saved)
}

/// Implement `pdfcer set-annot-opacity`.
pub(crate) fn cmd_set_annot_opacity(
    input: &Path,
    (page, index): (usize, usize),
    opacity: Option<f64>,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    let edit = opacity.map_or(StyleEdit::Clear, StyleEdit::Set);
    restyle(input, page, index, output, mode, |session, id| {
        let change = session.set_annot_opacity(id, edit)?;
        if change.clamped {
            eprintln!(
                "pdfcer: {}: opacity clamped to {} (the range is 0 to 1)",
                input.display(),
                change.current.unwrap_or(1.0)
            );
        }
        let show = |v: Option<f64>| v.map_or_else(|| "none".to_owned(), |a| format!("{a}"));
        Ok(format!(
            "set-annot-opacity obj={} previous={} current={} clamped={}",
            change.annot_id.num,
            show(change.previous),
            show(change.current),
            u32::from(change.clamped)
        ))
    })
}

/// Implement `pdfcer set-marker-color`.
pub(crate) fn cmd_set_marker_color(
    input: &Path,
    (page, index): (usize, usize),
    color: &str,
    redraw_as_plain: bool,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    let color = match parse_color(color) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("pdfcer: --color: {msg}");
            return exit::EDIT_REFUSED;
        }
    };
    let mut style = MarkerStyle::new(color);
    style.redraw_as_plain = redraw_as_plain;
    restyle(input, page, index, output, mode, |session, id| {
        let change = session.set_marker_style(id, &style)?;
        if change.appearance_was_foreign {
            eprintln!(
                "pdfcer: {}: the previous icon was drawn by another program and has been \
                 replaced with pdfcer's drawing.",
                input.display()
            );
        }
        Ok(format!(
            "set-marker-color obj={} subtype={} was_foreign={} appearance={}",
            change.annot_id.num,
            change.subtype,
            u32::from(change.appearance_was_foreign),
            match change.appearance {
                AppearanceWrite::InPlace(_) => "in-place",
                AppearanceWrite::Created(_) => "created",
                AppearanceWrite::CopiedOnWrite { .. } => "copied",
                _ => "other",
            }
        ))
    })
}

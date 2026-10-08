//! `respan-markup` — move a highlight, underline, strike-out or squiggly to
//! cover different text, and re-bake its appearance.

use super::*;

/// Implement `pdfcer respan-markup`.
pub(crate) fn cmd_respan_markup(
    input: &Path,
    (page, index): (usize, usize),
    (quads, rect): (Option<&str>, Option<&str>),
    modified: Option<&str>,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    let quads = match (quads, rect) {
        (Some(q), _) => parse_quads(q),
        (None, Some(r)) => {
            rect_from(r).map(|r| vec![pdfcer_core::annot_author::Quad::from_rect(r)])
        }
        (None, None) => Err("respan-markup needs --quads or --rect".to_owned()),
    };
    let quads = match quads {
        Ok(q) => q,
        Err(msg) => {
            eprintln!("pdfcer: {msg}");
            return exit::EDIT_REFUSED;
        }
    };
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let annot_id = match resolve_annotation(&session, input, page, index) {
        Ok(id) => id,
        Err(code) => return code,
    };
    let change = match session.respan_text_markup(annot_id, &quads, modified) {
        Ok(c) => c,
        Err(err) => return report_edit_error(input, &err),
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
    println!(
        "respan-markup {} page {page} index {index} -> {}",
        input.display(),
        output.display()
    );
    report_dropped(input, &change.dropped);
    println!(
        "  obj={} subtype={} quads_before={} quads_after={} rect={:.2},{:.2},{:.2},{:.2} \
         dropped={} mod_date_written={}",
        change.annot_id.num,
        change.subtype,
        change.quads_before,
        change.quads_after,
        change.rect_after.llx,
        change.rect_after.lly,
        change.rect_after.urx,
        change.rect_after.ury,
        change.dropped.len(),
        u32::from(change.mod_date_written),
    );
    finish_edit(input, &saved)
}

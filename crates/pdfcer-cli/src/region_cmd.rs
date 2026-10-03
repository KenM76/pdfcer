//! `pdfcer extract-region`: one page rectangle as a one-page PDF, with the
//! viewer's annotation and layer state applied.

use super::*;

use pdfcer_core::pageops::region::{RegionError, RegionExport, RegionReport, extract_region};

/// The operator's arguments, gathered so the command stays one call.
pub(crate) struct RegionArgs<'a> {
    pub input: &'a Path,
    pub page: u32,
    pub rect: &'a str,
    pub no_annotations: bool,
    pub show_layer: &'a [String],
    pub hide_layer: &'a [String],
    pub output: &'a Path,
}

/// Implement `pdfcer extract-region`.
pub(crate) fn cmd_extract_region(args: &RegionArgs<'_>) -> u8 {
    let doc = match open_for_read(args.input) {
        Ok(doc) => doc,
        Err(code) => return code,
    };
    let rect = match rect_from(args.rect) {
        Ok(rect) => rect,
        Err(message) => {
            eprintln!("pdfcer: --rect: {message}");
            return exit::EDIT_REFUSED;
        }
    };
    let Some(page) = (args.page as usize).checked_sub(1) else {
        eprintln!("pdfcer: --page is 1-based; 0 names no page");
        return exit::EDIT_REFUSED;
    };
    let (hidden, unmatched) = match hidden_layers(&doc, args.show_layer, args.hide_layer) {
        Ok(pair) => pair,
        Err(clash) => {
            eprintln!("pdfcer: layer {clash:?} is named by both --show-layer and --hide-layer");
            return exit::EDIT_REFUSED;
        }
    };
    let mut state = RegionExport::new().with_annotations(!args.no_annotations);
    if let Some(hidden) = hidden {
        state = state.with_hidden_layers(hidden);
    }
    let (bytes, report) = match extract_region(&doc.view(), page, rect, &state) {
        Ok(pair) => pair,
        Err(err) => return report_region_error(args.input, &err),
    };
    if let Err(err) = write_output(args.output, &bytes) {
        eprintln!("pdfcer: {}: {err}", args.output.display());
        return exit::IO_ERROR;
    }
    println!(
        "extract-region {} -> {}; page={} bytes={}",
        args.input.display(),
        args.output.display(),
        args.page,
        bytes.len()
    );
    print_report(&report);
    for name in unmatched {
        println!("note: no layer is named {name:?}; it was ignored");
    }
    exit::SUCCESS
}

/// `None` when neither list was given, so the document's `/D` state rules;
/// otherwise `/D`'s hidden set with the named layers moved in or out.
type LayerOverride = (Option<Vec<pdfcer_core::object::ObjId>>, Vec<String>);

fn hidden_layers(
    doc: &pdfcer_core::document::Document,
    show: &[String],
    hide: &[String],
) -> Result<LayerOverride, String> {
    if show.is_empty() && hide.is_empty() {
        return Ok((None, Vec::new()));
    }
    let (visibility, unmatched) = resolve_layer_override(doc, show, hide)?;
    let hidden = pdfcer_core::layers::read_layers(&doc.view())
        .layers
        .iter()
        .map(|l| l.id)
        .filter(|id| visibility.is_hidden(*id))
        .collect();
    Ok((Some(hidden), unmatched))
}

fn report_region_error(input: &Path, err: &RegionError) -> u8 {
    match err {
        RegionError::PageOp(err) => report_page_op_error(err),
        RegionError::InvalidRect | RegionError::Edit(_) | RegionError::ImageNotCut { .. } => {
            eprintln!("pdfcer: {}: {err}", input.display());
            exit::EDIT_REFUSED
        }
        _ => {
            eprintln!("pdfcer: {}: {err}", input.display());
            exit::RUNTIME_ERROR
        }
    }
}

/// The report, one `key=value` group per line, then each note.
fn print_report(r: &RegionReport) {
    println!(
        "region: rect=[{} {} {} {}] boxes_clamped={}",
        r.rect.llx, r.rect.lly, r.rect.urx, r.rect.ury, r.boxes_clamped
    );
    println!(
        "layers: hidden={} kept={} content_removed={}",
        r.layers_hidden, r.layers_kept, r.layer_content_removed
    );
    println!(
        "annotations: fields_flattened={} widgets_flattened={} flattened={} \
         ce_dimensions_flattened={} removed={}",
        r.fields_flattened,
        r.widgets_flattened,
        r.annotations_flattened,
        r.ce_dimensions_flattened,
        r.annotations_removed
    );
    println!(
        "forms: inlined={} dropped={} kept_straddling={}",
        r.forms_inlined, r.forms_dropped, r.forms_kept_straddling
    );
    println!(
        "cut: glyphs_removed={} paths_cut={} paths_dropped={} paths_uncut={} clips_kept={} \
         images_cleared={} images_removed={} shadings_cut={} shadings_uncut={}",
        r.glyphs_removed,
        r.paths_cut,
        r.paths_dropped,
        r.paths_uncut,
        r.clips_kept,
        r.images_cleared,
        r.images_removed,
        r.shadings_cut,
        r.shadings_uncut
    );
    println!("residuals={}", if r.has_residuals() { "yes" } else { "no" });
    for note in &r.notes {
        println!("note: {note}");
    }
}

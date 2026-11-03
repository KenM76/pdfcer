//! `xfa-materialize`: write a dynamic XFA form's computed pages into the
//! file and save (decision 204).

use super::*;

use pdfcer_core::edit::XfaSignatures;

use crate::xfa_layout::{dotted, print_merge_warnings, warning_fields};
use crate::xfa_page::lay_out;
use crate::xfa_png::note_label;

/// `xfa-materialize`: lay the form out at the default settings, replace its
/// placeholder pages through `EditSession::materialize_xfa`, and save. The
/// stdout lines report what was written; the stderr line discloses that
/// every page is pdfcer's layout (rule 4).
pub(crate) fn cmd_xfa_materialize(
    input: &Path,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
    remove_signatures: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let laid = open_document_bytes(source.clone())
        .map_err(|e| e.to_string())
        .and_then(|doc| lay_out(&doc));
    let (merged, layout) = match laid {
        Ok(m) => m,
        Err(e) => {
            eprintln!("pdfcer: {}: {e}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let paint = pdfcer_core::xfa::PaintOptions::new();
    let signatures = if remove_signatures {
        XfaSignatures::Remove
    } else {
        XfaSignatures::Refuse
    };
    let out = match session.materialize_xfa(&merged.template, &layout.pages, &paint, signatures) {
        Ok(out) => out,
        Err(err) => return report_edit_error(input, &err),
    };
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
    for (page, n) in &out.notes {
        println!(
            "paint-note page={page} kind={} path={}",
            note_label(n.kind),
            dotted(&n.path)
        );
    }
    for w in &layout.warnings {
        println!("warning {}", warning_fields(w));
    }
    print_merge_warnings(&merged);
    let r = &outcome.report;
    println!(
        "xfa-materialize {} pages={} replaced={} images={} fields_dropped={} \
usage_rights_removed={} signatures_removed={} notes={} warnings={} mode={} -> {}; changed={} objects={} \
out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        out.pages_written,
        out.pages_replaced,
        out.images,
        out.fields_dropped,
        u32::from(out.usage_rights_removed),
        out.signatures_removed,
        out.notes.len(),
        layout.warnings.len() + merged.warnings.len(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    eprintln!(
        "pdfcer: {}: the {} page(s) written are pdfcer's layout of the XFA template with {} value(s) merged from the form's data; the XFA form was removed, so field values are drawn, not fillable.",
        input.display(),
        out.pages_written,
        merged.bound.len()
    );
    if out.signatures_removed > 0 {
        eprintln!(
            "pdfcer: {}: {} signature(s) removed at your request; the saved file is no longer signed or certified.",
            input.display(),
            out.signatures_removed
        );
    }
    finish_edit(input, &outcome)
}

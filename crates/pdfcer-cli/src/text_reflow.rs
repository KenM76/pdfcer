//! `reflow`: re-wrap one recognised paragraph block.

use super::*;

/// `reflow`: Pass 15.1 within-block reflow surgery.
///
/// Recognises the block model on `--page` (with first-line-indent splitting
/// relaxed, matching `inspect --reflow-preview`), re-wraps the paragraph
/// `--block` under the requested width/alignment/leading via the 14.1
/// advance-preserving machinery, and saves INCREMENTALLY — only the block's
/// own content-stream object changes. Every gate (a composite/CJK block, a
/// rotated/skewed or shared/non-contiguous block, a missing-provenance or
/// bad-index/width condition) is a clean, named non-zero exit
/// ([`exit::EDIT_REFUSED`]), never a crash. All disclosures (derived-layout,
/// justify, page-overflow-emitted-not-clipped, tagged-stale, incremental/
/// prior-text) are surfaced verbatim.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_reflow(
    input: &Path,
    page: usize,
    block: usize,
    width: Option<f64>,
    align: Option<&str>,
    leading: Option<f64>,
    output: &Path,
) -> u8 {
    use pdfcer_core::text_edit::{BlockAlignment, ReflowRequest, apply_reflow};

    // Parse the alignment override up front so a typo fails cleanly before any
    // document work (the R27 fail-clean posture; identical to the preview
    // path's parse).
    let align_override = match align {
        None => None,
        Some(s) => match BlockAlignment::parse(s) {
            Some(a) => Some(a),
            None => {
                eprintln!(
                    "pdfcer: {}: --align {s}: expected left|right|center|justified",
                    input.display()
                );
                return exit::EDIT_REFUSED;
            }
        },
    };

    if page == 0 {
        eprintln!("pdfcer: --page is 1-based; 0 is not a valid page number");
        return exit::EDIT_REFUSED;
    }

    let source = match std::fs::read(input) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::IO_ERROR;
        }
    };
    let doc = match open_document_bytes(source) {
        Ok(d) => d,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };

    let req = ReflowRequest::new()
        .with_wrap_width_opt(width)
        .with_alignment_opt(align_override)
        .with_leading_opt(leading);

    let outcome = match apply_reflow(&doc, page - 1, block, &req) {
        Ok(o) => o,
        Err(err) => {
            eprintln!("pdfcer: reflow refused: {err}");
            return reflow_error_exit(&err);
        }
    };

    if let Err(err) = write_output(output, &outcome.bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    print_reflow_report(input, output, page, block, &outcome.report);
    exit::SUCCESS
}

/// Print the recoverable-refusal hint and map a reflow error to its exit code.
fn reflow_error_exit(err: &pdfcer_core::text_edit::ReflowApplyError) -> u8 {
    use pdfcer_core::text_edit::ReflowApplyError;
    // Rule 4: the invocation IS the commit here, so what pdfcer knows
    // about the operator's options is printed on the way past rather
    // than being available to ask for. `is_recoverable()` is the
    // engine's own answer -- not this shell's reading of the sentence
    // above, which is exactly the coupling `pdfcer-gui` refused.
    if err.is_recoverable() {
        eprintln!(
            "pdfcer: this one you CAN clear -- save the document and reopen it, then reflow. \
             Every other reason reflow declines is a property of how the page was drawn."
        );
    }
    // A refusal is a clean named non-zero; a save/runtime failure is a
    // distinct class. The `_` arm keeps this exhaustive as
    // `ReflowApplyError` grows (it is `#[non_exhaustive]`).
    //
    // `PageEditedThisSession` is listed EXPLICITLY rather than left
    // to the `_` arm, which would have called it a RUNTIME_ERROR. It
    // is a refusal -- the cleanest, most recoverable one there is --
    // and a new variant silently inheriting the catch-all is how a
    // correct engine change becomes a wrong exit code.
    match err {
        ReflowApplyError::Refused(_)
        | ReflowApplyError::Preview(_)
        | ReflowApplyError::NoProvenance
        | ReflowApplyError::Unsupported(_)
        | ReflowApplyError::PageEditedThisSession
        | ReflowApplyError::PageIndex(_)
        | ReflowApplyError::Encrypted => exit::EDIT_REFUSED,
        ReflowApplyError::Write(_) => exit::SAVE_REFUSED,
        ReflowApplyError::Extract(_)
        | ReflowApplyError::Content(_)
        | ReflowApplyError::PageTree(_) => exit::RUNTIME_ERROR,
        _ => exit::RUNTIME_ERROR,
    }
}

fn print_reflow_report(
    input: &Path,
    output: &Path,
    page: usize,
    block: usize,
    report: &pdfcer_core::text_edit::ReflowApplyReport,
) {
    println!("reflow {} -> {}", input.display(), output.display());
    println!(
        "  page={page} block={block} align={} lines_before={} lines_after={} \
justified_lines={} height_delta={:.1}",
        report.alignment.as_str(),
        report.lines_before,
        report.lines_after,
        report.justified_lines,
        report.height_delta,
    );
    println!(
        "  base_font={} glyph_source={} content_object={}",
        report.base_font,
        match report.glyph_source {
            pdfcer_core::text_edit::EditGlyphSource::Embedded => "Embedded",
            _ => "NonEmbedded",
        },
        report.content_object,
    );
    if let Some(ov) = report.overflow {
        println!(
            "  overflow: past_bottom={:.1}pt lines_outside={} (EMITTED off-page, not clipped)",
            ov.past_bottom_pt, ov.lines_outside
        );
    }
    if let Some(co) = report.cell_overflow {
        println!(
            "  cell_overflow: past_bottom={:.1}pt past_right={:.1}pt lines_outside={} (cell not resized)",
            co.past_bottom_pt, co.past_right_pt, co.lines_outside
        );
    }
    if let Some(mcid) = report.tagged_mcid {
        println!("  tagged_mcid={mcid}");
    }
    println!("  disclosures:");
    for d in &report.disclosures {
        println!("    - {d}");
    }
}

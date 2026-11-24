//! `fix-hairlines`: widen every stroke thinner than a minimum to exactly it.

use super::*;

/// `fix-hairlines` — `EditSession::fix_hairlines` over `--pages`.
///
/// ## Contract
///
/// - Emits one `fix-hairlines …` summary line, then `page N: K widened` per
///   changed page (from 1), `form …` per rewritten form XObject with its
///   reach, `skipped page N: …` per page that could not be read, and one
///   `disclosure: …` line each, always including what is not covered. Then
///   defers the exit code to [`finish_edit`].
/// - A bad `--pages` or `--min-width` is refused before any mutation
///   (exit `EDIT_REFUSED`).
pub(crate) fn cmd_fix_hairlines(
    input: &Path,
    pages: &str,
    min_width: f64,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let count = match session.pages() {
        Ok(p) => p.len(),
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let selected = match sign::parse_pages(pages, count) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("pdfcer: {}: --pages: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let fixed = match session.fix_hairlines(&selected, min_width) {
        Ok(r) => r,
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
    let r = &outcome.report;
    println!(
        "fix-hairlines {} min_width={min_width}pt widened={} zero_width={} forms={} skipped={} \
mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        fixed.changed,
        fixed.zero_width,
        fixed.forms.len(),
        fixed.skipped.len(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    print_details(&fixed);
    finish_edit(input, &outcome)
}

/// The per-page, per-form, skipped and disclosure lines.
fn print_details(fixed: &pdfcer_core::edit::HairlineFixReport) {
    for (page, n) in &fixed.per_page {
        println!("page {}: {n} widened", page + 1);
    }
    for f in &fixed.forms {
        println!(
            "form {}: drawn {} time(s) on {} page(s)",
            f.form, f.invocations, f.pages
        );
    }
    for (page, why) in &fixed.skipped {
        println!("skipped page {}: {why}", page + 1);
    }
    for d in &fixed.disclosures {
        println!("disclosure: {d}");
    }
}

use super::*;

/// `bake-appearances`: store the drawn look of every `/AP`-less markup
/// annotation as its `/AP` (decision 203). Prints one line per baked or
/// skipped annotation, with what was settled rather than read, so nothing
/// it decided is silent.
pub(crate) fn cmd_bake_appearances(
    input: &Path,
    output: &Path,
    mode: SaveMode,
    free_text_c: Option<crate::viewer_preview::FreeTextCArg>,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    // R169: the flag wins; otherwise the persisted choice, so a bake and
    // the render beside it read `/C` the same way.
    let free_text_c = match free_text_c {
        Some(arg) => arg.into(),
        None => {
            let (settings, report) =
                pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
            report_settings(&report);
            settings.free_text_c
        }
    };
    let outcome = match session.bake_missing_appearances(free_text_c) {
        Ok(o) => o,
        Err(err) => return report_edit_error(input, &err),
    };
    for b in &outcome.baked {
        let rect = if b.rect_changed { " rect=grown" } else { "" };
        let notes: String = b.notes.iter().map(|n| format!("; {n}")).collect();
        println!(
            "baked {} R /{} ap={} R{rect}{notes}",
            b.annot_id, b.subtype, b.ap_id
        );
    }
    for k in &outcome.skipped {
        println!("skipped {} R /{}: {}", k.annot_id, k.subtype, k.reason);
    }
    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(o) => o,
        Err(code) => return code,
    };
    println!(
        "bake-appearances {} baked={} skipped={} mode={} -> {}; objects={} out_bytes={}",
        input.display(),
        outcome.baked.len(),
        outcome.skipped.len(),
        mode.name(),
        output.display(),
        saved.report.objects_written,
        saved.report.bytes_written,
    );
    finish_edit(input, &saved)
}

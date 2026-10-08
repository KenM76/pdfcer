//! `turn-widget`: turn a form-field widget to any angle.

use super::*;

/// The `turn-widget` argument bundle.
pub(crate) struct TurnWidgetArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) name: &'a str,
    pub(crate) index: usize,
    pub(crate) degrees: f64,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `turn-widget`: one `turn-widget ...` line on stdout carrying `was=` /
/// `now=` (counterclockwise degrees on top of `/MK /R`) and the new `rect=`;
/// the engine's disclosures on stderr; exit code from [`finish_edit`].
pub(crate) fn cmd_turn_widget(args: &TurnWidgetArgs) -> u8 {
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let turn = match session.turn_widget(args.name, args.index, args.degrees) {
        Ok(t) => t,
        Err(err) => return report_edit_error(args.input, &err),
    };
    report_disclosures(&turn.disclosures);
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
    let r = &outcome.report;
    let rect = turn.rect_after.map_or_else(
        || "-".to_owned(),
        |r| format!("{},{},{},{}", r.llx, r.lly, r.urx, r.ury),
    );
    println!(
        "turn-widget {} name={} index={} was={} now={} rect={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.name,
        args.index,
        turn.was,
        turn.now,
        rect,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

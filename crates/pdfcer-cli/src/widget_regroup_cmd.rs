//! `move-widget-to-field`: move one radio button into another radio group.

use super::*;

/// The `move-widget-to-field` argument bundle.
pub(crate) struct MoveWidgetToFieldArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) name: &'a str,
    pub(crate) index: usize,
    pub(crate) into: &'a str,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// Run `move-widget-to-field`.
///
/// Prints one `move-widget-to-field …` line carrying `widget=` (object
/// number, unchanged by the move), `on_state=` (`-` when the widget has
/// none), `source_removed=` and `action_targets_orphaned=`; every disclosure
/// goes to stderr. Refusals go through [`report_edit_error`] before anything
/// is written; the exit code is [`finish_edit`]'s.
pub(crate) fn cmd_move_widget_to_field(a: &MoveWidgetToFieldArgs<'_>) -> u8 {
    let (source, mut session) = match open_for_edit(a.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let moved = match session.move_widget_to_field(a.name, a.index, a.into) {
        Ok(m) => m,
        Err(err) => return report_edit_error(a.input, &err),
    };
    for line in &moved.disclosures {
        eprintln!("pdfcer: {line}");
    }
    if moved.action_targets_orphaned > 0 {
        eprintln!(
            "pdfcer: field {:?}: {} button-action target(s) still name it and now point at nothing; pdfcer does not repoint them.",
            a.name, moved.action_targets_orphaned
        );
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        a.output,
        a.mode,
        ProducerArg::Preserve,
        a.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "move-widget-to-field {} name={:?} index={} into={:?} widget={} on_state={} source_removed={} action_targets_orphaned={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        a.input.display(),
        a.name,
        a.index,
        a.into,
        moved.widget.num,
        moved.on_state.as_deref().unwrap_or("-"),
        u32::from(moved.source_removed),
        moved.action_targets_orphaned,
        a.mode.name(),
        a.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(a.input, &outcome)
}

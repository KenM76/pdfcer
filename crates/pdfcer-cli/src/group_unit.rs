//! `group-set-unit`: show a ce dimension group in another unit without
//! changing what it measures.

use super::*;

/// `group-set-unit` — switch a group's display unit, keeping its calibration
/// (`EditSession::set_group_unit`).
///
/// ## Contract
///
/// - Emits one `group-set-unit …` line naming the group, the new unit and the
///   member count regenerated, then defers the exit code to [`finish_edit`].
/// - An unknown unit token or group is refused before any mutation
///   (exit `EDIT_REFUSED`).
pub(crate) fn cmd_group_set_unit(
    input: &Path,
    group: u32,
    unit: &str,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    use pdfcer_core::dimension::{GroupId, Unit};
    let Some(unit) = Unit::parse(unit) else {
        eprintln!(
            "pdfcer: {}: unknown --unit `{unit}` (mm|cm|m|km|in|ft|ft-in|yd|mi)",
            input.display()
        );
        return exit::EDIT_REFUSED;
    };
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let members = match session.set_group_unit(GroupId(group), unit) {
        Ok(n) => n,
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
        "group-set-unit {} group={group} unit={} members_regenerated={members} mode={} -> {}; \
changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        unit.token(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

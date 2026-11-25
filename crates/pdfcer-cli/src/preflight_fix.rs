//! `preflight --fix`: run a profile's fixups as one undo step, save, and
//! check the saved file again.

use crate::*;

use pdfcer_core::preflight::{self, FixupReport, FixupTools, Profile};

use super::{PreflightArgs, check, print_report, write_report};

/// `preflight --fix OUTPUT`.
///
/// ## Contract
///
/// - Checks `input`, then runs every fixup the profile's failing rules name
///   (`--fix-suggested` adds pdfcer's suggestion for a rule naming none) as
///   ONE undo step, with the transparency flattener and the icccer colour
///   converter supplied. `--icc FILE` is the profile an empty colour or
///   PDF/X destination stands for; without it those fixups are reported
///   unavailable and write nothing. A PDF/X conversion takes the clock
///   for its dates and the input's file stem as a missing `/Title`.
/// - Prints `FixupReport::to_text` (one `fixup …` line per step, its
///   `changed:`, `not done:` and `disclosure:` lines, then a totals line),
///   saves to OUTPUT, prints the `edit` line, then checks OUTPUT and prints
///   the same header and finding lines as a plain run.
/// - `--report FILE` describes OUTPUT, not the input.
/// - Exit `PREFLIGHT_FAILED` (18) when OUTPUT still fails the profile;
///   `EDIT_REFUSED` for an unreadable `--icc`; the save codes otherwise.
pub(crate) fn cmd_preflight_fix(
    a: &PreflightArgs<'_>,
    input: &Path,
    output: &Path,
    profile: &Profile,
) -> u8 {
    let icc = match a.icc.map(std::fs::read).transpose() {
        Ok(icc) => icc,
        Err(err) => {
            eprintln!("pdfcer: --icc: {err}");
            return exit::EDIT_REFUSED;
        }
    };
    let (source, mut session) = match open_for_edit(input) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let before = check(&session.view(), profile, a);
    let title = input.file_stem().map_or_else(
        || "Untitled".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );
    let fixed = run_fixups(
        &mut session,
        (profile, &before),
        a.suggested,
        icc.as_deref(),
        &title,
    );
    print!("{}", fixed.to_text());
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        a.mode,
        ProducerArg::Preserve,
        a.verify_undo,
    ) {
        Ok(o) => o,
        Err(code) => return code,
    };
    println!(
        "edit {} mode={} -> {}; changed={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        a.mode.name(),
        output.display(),
        outcome.changed,
        outcome.report.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    let code = finish_edit(input, &outcome);
    if code != exit::SUCCESS {
        return code;
    }
    recheck(a, output, profile)
}

/// Run the fixups with the CLI's engines.
fn run_fixups(
    session: &mut pdfcer_core::edit::EditSession,
    (profile, report): (&Profile, &preflight::PreflightReport),
    suggested: bool,
    icc: Option<&[u8]>,
    title: &str,
) -> FixupReport {
    let flattener = pdfcer_emf::RecordingFlattener::new();
    let converter = pdfcer_render::convert_colours::IcccerConverter::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let mut tools = FixupTools::new()
        .with_flattener(&flattener)
        .with_converter(&converter)
        .with_time(now)
        .with_title(title);
    if let Some(icc) = icc {
        tools = tools.with_icc_profile(icc);
    }
    session.apply_profile_fixups(profile, report, &tools, suggested)
}

/// Check the saved file and report what still stands.
fn recheck(a: &PreflightArgs<'_>, output: &Path, profile: &Profile) -> u8 {
    let doc = match open_for_read(output) {
        Ok(doc) => doc,
        Err(code) => return code,
    };
    let report = check(&doc.view(), profile, a);
    print_report(output, profile, &report);
    if let Err(code) = write_report(a, &report, profile) {
        return code;
    }
    if report.fails() {
        exit::PREFLIGHT_FAILED
    } else {
        exit::SUCCESS
    }
}

//! `inks`, `alias-ink` and `spot-to-process`: the Ink Manager.

use super::*;

use pdfcer_core::prepress::{InkEditReport, InkKind, InkSelection};

/// `inks` — read-only.
///
/// ## Contract
///
/// - One header line `inks <input> count=<n> complete=<0|1>`, then one
///   line per ink:
///   `<process|spot> "<name>" alternates=<a,b|-> pages=<1,2|-> uses=<n>`.
/// - A name is UTF-8 with `#` and any byte that is not UTF-8 written `#XX`
///   (ISO 32000-1 §7.3.5); `alias-ink` and `spot-to-process --ink` take it
///   as printed.
/// - `complete=0` is followed by one `note "..."` line: a `DeviceN` space
///   names more than 32 colorants (ISO 32000-1 Annex C) or nests too
///   deeply, and the list may be missing inks.
/// - Exit 0, or the usual read codes for an unreadable input.
pub(crate) fn cmd_inks(input: &Path) -> u8 {
    let doc = match open_for_read(input) {
        Ok(doc) => doc,
        Err(code) => return code,
    };
    let list = pdfcer_core::prepress::inks(&doc.view());
    println!(
        "inks {} count={} complete={}",
        input.display(),
        list.inks.len(),
        u8::from(list.complete)
    );
    if !list.complete {
        println!(
            "note \"a DeviceN space names more than 32 colorants or nests too deeply; inks past that were not read, so this list may be missing some\""
        );
    }
    for ink in &list.inks {
        let kind = match ink.kind {
            InkKind::Process => "process",
            InkKind::Spot => "spot",
        };
        let pages: Vec<String> = ink.pages.iter().map(ToString::to_string).collect();
        println!(
            "{kind} \"{}\" alternates={} pages={} uses={}",
            ink.name,
            list_or_dash(&ink.alternates),
            list_or_dash(&pages),
            ink.uses,
        );
    }
    exit::SUCCESS
}

fn list_or_dash(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_owned()
    } else {
        items.join(",")
    }
}

/// Print an ink edit's report after its summary line.
fn print_report(report: &InkEditReport) {
    for r in &report.refused {
        let at = r.object.map_or_else(|| "-".to_owned(), |id| id.to_string());
        println!("refused ink=\"{}\" object={at} \"{}\"", r.ink, r.reason);
    }
    for id in &report.overprint_lost {
        println!("overprint-lost object={id}");
    }
    for sentence in &report.disclosures {
        println!("note \"{sentence}\"");
    }
}

/// Save a committed ink edit and print its lines.
fn finish_ink_edit(
    verb: &str,
    input: &Path,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
    edited: (Vec<u8>, pdfcer_core::edit::EditSession, InkEditReport),
) -> u8 {
    let (source, mut session, report) = edited;
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
        "{verb} {} spaces={} paints={} images={} shadings={} lookups={} refused={} overprint_lost={} \
mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        report.spaces,
        report.paints,
        report.images,
        report.shadings,
        report.lookups,
        report.refused.len(),
        report.overprint_lost.len(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    print_report(&report);
    finish_edit(input, &outcome)
}

/// `alias-ink` — print spot ink `from` as `to` (`EditSession::alias_ink`).
///
/// ## Contract
///
/// - One summary line `alias-ink <input> spaces=… -> <output>; changed=…`,
///   then `refused …`, `note …` lines; exit per [`finish_edit`].
/// - Names are spelled as `inks` prints them (`#XX` is one byte).
/// - A refused name (empty, `All`, `None`, over 127 bytes decoded, a zero
///   byte, a `#` without two hex digits, a process `from`, `from == to`)
///   exits `EDIT_REFUSED` before anything is written, as does a search cut
///   short or a rename that would change an object a refused `DeviceN`
///   space shares.
pub(crate) fn cmd_alias_ink(
    input: &Path,
    from: &str,
    to: &str,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let report = match session.alias_ink(from, to) {
        Ok(r) => r,
        Err(err) => return report_edit_error(input, &err),
    };
    let edited = (source, session, report);
    finish_ink_edit("alias-ink", input, output, mode, verify_undo, edited)
}

/// `spot-to-process` — convert spot inks to CMYK
/// (`EditSession::spot_to_process`).
///
/// ## Contract
///
/// - `inks` empty converts every spot ink; otherwise exactly those named.
/// - One summary line, then `refused …`, `overprint-lost …` and `note …`
///   lines; exit per [`finish_edit`].
pub(crate) fn cmd_spot_to_process(
    input: &Path,
    inks: &[String],
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let selection = if inks.is_empty() {
        InkSelection::All
    } else {
        InkSelection::Only(inks.to_vec())
    };
    let report = match session.spot_to_process(&selection) {
        Ok(r) => r,
        Err(err) => return report_edit_error(input, &err),
    };
    let edited = (source, session, report);
    finish_ink_edit("spot-to-process", input, output, mode, verify_undo, edited)
}

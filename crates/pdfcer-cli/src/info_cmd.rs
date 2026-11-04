//! `set-info` and `list-info`: document-information entries of any name
//! (ISO 32000-1 §14.3.3), with the XMP packet (§14.3.2) kept in step.

use super::*;
use pdfcer_core::edit::{
    AltLanguages, AuthorItems, EditSession, InfoEditOutcome, InfoEntry, InfoField, InfoValue,
    SignedPacket, XmpMirror, XmpSkip, XmpSync, XmpSyncOptions,
};

/// `--xmp`: whether `/Info` edits are mirrored into the XMP packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum XmpArg {
    /// Mirror each edit into the packet, in the same save.
    Sync,
    /// Write /Info only; every edit reports the packet as skipped.
    Off,
}

/// `--xmp-signed`: what to do with a signed document's packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum XmpSignedArg {
    /// Leave it, and report it as skipped.
    Leave,
    /// Rewrite it like any other.
    Update,
}

/// `--xmp-languages`: what happens to a title's or subject's items in
/// other languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum XmpLanguagesArg {
    /// Drop the item that repeated the old value; keep the rest.
    ReplaceDuplicates,
    /// Keep every other language.
    Keep,
    /// Keep only the new default-language item.
    Drop,
}

/// `--xmp-authors`: how `/Author` becomes `dc:creator` items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum XmpAuthorsArg {
    /// One item holding the whole string.
    Whole,
    /// One item per semicolon-separated name (not under PDF/A-1).
    Split,
}

/// The core options the four `--xmp*` flags select.
pub(crate) const fn xmp_options(
    xmp: XmpArg,
    signed: XmpSignedArg,
    languages: XmpLanguagesArg,
    authors: XmpAuthorsArg,
) -> XmpSyncOptions {
    XmpSyncOptions {
        mirror: match xmp {
            XmpArg::Sync => XmpMirror::Always,
            XmpArg::Off => XmpMirror::Never,
        },
        signed: match signed {
            XmpSignedArg::Leave => SignedPacket::Leave,
            XmpSignedArg::Update => SignedPacket::Update,
        },
        languages: match languages {
            XmpLanguagesArg::ReplaceDuplicates => AltLanguages::ReplaceDuplicates,
            XmpLanguagesArg::Keep => AltLanguages::Keep,
            XmpLanguagesArg::Drop => AltLanguages::Drop,
        },
        authors: match authors {
            XmpAuthorsArg::Whole => AuthorItems::Whole,
            XmpAuthorsArg::Split => AuthorItems::SplitOnSemicolons,
        },
    }
}

/// `--set KEY=VALUE`, split at the first `=`.
pub(crate) fn parse_info_entry(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .ok_or_else(|| format!("expected KEY=VALUE, got {s:?}"))
}

/// The parsed arguments of `pdfcer set-info`.
pub(crate) struct SetInfoArgs<'a> {
    pub(crate) input: &'a Path,
    /// `--title`, `--author`, `--subject`, `--keywords`, `--creator`.
    pub(crate) fields: Vec<(InfoFieldArg, Option<String>)>,
    pub(crate) clears: &'a [InfoFieldArg],
    /// `--set KEY=VALUE`.
    pub(crate) entries: &'a [(String, String)],
    /// `--remove KEY`.
    pub(crate) removes: &'a [String],
    pub(crate) xmp: XmpSyncOptions,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) producer: ProducerArg,
    pub(crate) verify_undo: bool,
}

/// The edits in the order applied: values first, then removals. A key
/// both given a value and removed is **removed**, because removal is the
/// more explicit request; argument order, which a script author cannot
/// see in a generated command line, decides nothing.
fn planned_edits(args: &SetInfoArgs<'_>) -> Vec<(Vec<u8>, Option<String>)> {
    let removed: Vec<Vec<u8>> = args
        .clears
        .iter()
        .map(|f| InfoField::from(*f).key().to_vec())
        .chain(args.removes.iter().map(|k| k.as_bytes().to_vec()))
        .collect();
    let named = args.fields.iter().filter_map(|(f, v)| {
        v.as_ref()
            .map(|v| (InfoField::from(*f).key().to_vec(), v.clone()))
    });
    let custom = args
        .entries
        .iter()
        .map(|(k, v)| (k.as_bytes().to_vec(), v.clone()));
    let mut edits: Vec<_> = named
        .chain(custom)
        .filter(|(k, _)| !removed.contains(k))
        .map(|(k, v)| (k, Some(v)))
        .collect();
    edits.extend(removed.into_iter().map(|k| (k, None)));
    edits
}

/// Counts of what the edits did to the XMP packet.
#[derive(Default)]
struct XmpTally {
    updated: usize,
    in_step: usize,
    skipped: usize,
    no_packet: bool,
}

impl XmpTally {
    /// Count `outcome`, and say why when the packet was left disagreeing.
    fn record(&mut self, input: &Path, outcome: &InfoEditOutcome) {
        match &outcome.xmp {
            XmpSync::Updated { .. } => self.updated += 1,
            XmpSync::InStep { .. } => self.in_step += 1,
            XmpSync::Skipped { property, reason } => {
                self.skipped += 1;
                eprintln!(
                    "pdfcer: {}: XMP {} not updated ({}): {}",
                    input.display(),
                    property.as_deref().unwrap_or("property"),
                    reason.token(),
                    skip_reason(*reason)
                );
            }
            _ => self.no_packet = true,
        }
    }
}

fn skip_reason(reason: XmpSkip) -> String {
    match reason {
        XmpSkip::Off => "--xmp off was given".to_owned(),
        XmpSkip::Signed => {
            "the document is signed; pass --xmp-signed update to rewrite the packet".to_owned()
        }
        XmpSkip::PdfaCustomKey { part } => format!(
            "the file claims PDF/A-{part}, which allows no custom XMP property without an \
extension schema; the value is in /Info only"
        ),
        XmpSkip::Unrepresentable => {
            "the key or value holds a character XML cannot carry; the value is in /Info only"
                .to_owned()
        }
        _ => "the packet could not be read as XMP; it was left as it is".to_owned(),
    }
}

/// Implement `pdfcer set-info`.
pub(crate) fn cmd_set_info(args: &SetInfoArgs<'_>) -> u8 {
    let input = args.input;
    let edits = planned_edits(args);
    if edits.is_empty() {
        eprintln!(
            "pdfcer: {}: no fields given — pass at least one of --title/--author/--subject/\
--keywords/--creator/--set KEY=VALUE, or --clear <field>/--remove KEY",
            input.display()
        );
        return exit::EDIT_REFUSED;
    }
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    session.set_xmp_sync(args.xmp);
    let had_info = session.document().trailer().get(b"Info").is_some();
    let mut tally = XmpTally::default();
    let mut info_edits = 0usize;
    for (key, value) in &edits {
        match session.set_info_entry(key, value.as_deref()) {
            Ok(outcome) => {
                info_edits += usize::from(outcome.info_changed);
                tally.record(input, &outcome);
            }
            Err(err) => return report_edit_error(input, &err),
        }
    }
    let created = !had_info && session.dirty_set().trailer_patch().contains_key(b"Info");
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        args.producer,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    print_set_info(args, &outcome, info_edits, &tally, created);
    finish_edit(input, &outcome)
}

fn print_set_info(
    args: &SetInfoArgs<'_>,
    outcome: &EditOutcome,
    info_edits: usize,
    tally: &XmpTally,
    created: bool,
) {
    let r = &outcome.report;
    println!(
        "set-info {} mode={} -> {}; \
changed={} objects={} verbatim={} reserialized={} promoted={} appended={} out_bytes={} \
info_created={} info_edits={} xmp_packet={} xmp_updated={} xmp_in_step={} xmp_skipped={} \
undo_verified={} undo_identical={} delinearized={}",
        args.input.display(),
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.objects_verbatim,
        r.objects_reserialized,
        r.promoted.len(),
        r.bytes_appended,
        r.bytes_written,
        u32::from(created),
        info_edits,
        u32::from(!tally.no_packet),
        tally.updated,
        tally.in_step,
        tally.skipped,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
        u32::from(r.delinearized),
    );
    if created {
        // R41: pdfcer adds nothing to a file unasked, so when it does
        // because the operator asked, it says so.
        eprintln!(
            "pdfcer: {}: this file had no document information dictionary; one was created \
to hold the metadata you asked for, and the trailer now references it.",
            args.input.display()
        );
    }
}

/// Implement `pdfcer list-info`.
pub(crate) fn cmd_list_info(input: &Path, json: bool) -> u8 {
    let session = match open_document(input) {
        Ok(doc) => EditSession::new(doc),
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let entries = session.info_entries();
    if json {
        let rows: Vec<String> = entries.iter().map(entry_json).collect();
        println!("{{\"entries\":[{}]}}", rows.join(","));
    } else {
        println!("list-info {} entries={}", input.display(), entries.len());
        for entry in &entries {
            let (kind, value, exact) = entry_parts(entry);
            let flag = if exact { "" } else { "\t(inexact)" };
            println!(
                "  {}\t{kind}\t{}{flag}",
                String::from_utf8_lossy(&entry.key),
                value.escape_debug()
            );
        }
    }
    exit::SUCCESS
}

/// `(kind, value, exact)`: `exact` is false when a text value had bytes
/// with no known decoding, shown as U+FFFD.
fn entry_parts(entry: &InfoEntry) -> (&'static str, String, bool) {
    match &entry.value {
        InfoValue::Text(t) => ("text", t.text.clone(), t.exact),
        InfoValue::Name(n) => ("name", String::from_utf8_lossy(n).into_owned(), true),
        _ => ("other", String::new(), true),
    }
}

fn entry_json(entry: &InfoEntry) -> String {
    let (kind, value, exact) = entry_parts(entry);
    format!(
        "{{\"key\":\"{}\",\"kind\":\"{kind}\",\"value\":\"{}\",\"exact\":{exact}}}",
        json_escape(&String::from_utf8_lossy(&entry.key)),
        json_escape(&value)
    )
}

//! `add-emf` — place an EMF picture on a page as vector content.

use super::*;

/// The arguments of `add-emf`, borrowed from the parsed command.
pub(crate) struct AddEmfArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) emf: &'a Path,
    pub(crate) page: usize,
    pub(crate) rect: &'a str,
    pub(crate) fit: SvgFit,
    pub(crate) stamp: bool,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `add-emf` — import an EMF and draw it as a Form XObject of vector
/// operators (or, with `--stamp`, as a `/Stamp` annotation's appearance).
///
/// ## Contract
///
/// - Emits one `add-emf …` line on stdout with every disclosure as a field
///   (`skipped=`, `approximated=`, `fonts=`, `chars_replaced=`,
///   `emf_plus_ignored=`, `distorted=`), the prose form on stderr, then
///   defers the exit code to [`finish_edit`].
/// - A file that will not import (not an EMF, damaged, EMF+ only, over a
///   ceiling) is refused with exit 9 before the PDF is opened.
/// - `--page` is 1-based.
pub(crate) fn cmd_add_emf(args: &AddEmfArgs<'_>) -> u8 {
    let (page_index, requested) = match parse_page_and_rect(args.input, args.page, args.rect) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let bytes = match std::fs::read(args.emf) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.emf.display());
            return exit::IO_ERROR;
        }
    };
    let emf = match pdfcer_core::emf_import::import(&bytes) {
        Ok(emf) => emf,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.emf.display());
            return exit::EDIT_REFUSED;
        }
    };
    let rect = fit_rect(requested, emf.natural_size_pt(), args.fit);
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let result = if args.stamp {
        session.add_emf_stamp(page_index, rect, &emf)
    } else {
        session.add_emf(page_index, rect, &emf)
    };
    let placed = match result {
        Ok(placed) => placed,
        Err(err) => return report_edit_error(args.input, &err),
    };
    if !placed.notes.is_empty() {
        eprintln!(
            "pdfcer: {}: placed, but {}",
            args.emf.display(),
            placed.notes.summary()
        );
    }
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
    print_outcome(args, &emf, &placed, &outcome);
    finish_edit(args.input, &outcome)
}

/// `name:count,…`, spaces as underscores; `-` when empty.
fn tally(m: &std::collections::BTreeMap<String, usize>) -> String {
    if m.is_empty() {
        return "-".to_owned();
    }
    m.iter()
        .map(|(k, n)| format!("{}:{n}", k.replace(' ', "_")))
        .collect::<Vec<_>>()
        .join(",")
}

fn print_outcome(
    args: &AddEmfArgs<'_>,
    emf: &pdfcer_core::emf_import::ImportedEmf,
    placed: &pdfcer_core::edit::PlacedEmf,
    outcome: &EditOutcome,
) {
    let n = &placed.notes;
    let fonts = if n.fonts_substituted.is_empty() {
        "-".to_owned()
    } else {
        n.fonts_substituted
            .iter()
            .map(|(from, to)| format!("{}>{to}", from.replace(' ', "_")))
            .collect::<Vec<_>>()
            .join(",")
    };
    let (w, h) = emf.natural_size_pt();
    let p = placed.rect;
    let r = &outcome.report;
    let id = |o: Option<pdfcer_core::object::ObjId>| {
        o.map_or_else(
            || "-".to_owned(),
            |id| format!("{} {}", id.num, id.generation),
        )
    };
    println!(
        "add-emf {} emf={} page={} size_pt={w:.3}x{h:.3} records={} images={} \
         placed={:.3},{:.3},{:.3},{:.3} fit={} as={} form={} {} content={} annot={} \
         scale={:.4},{:.4} distorted={} emf_objects={} skipped={} approximated={} \
         fonts={fonts} chars_replaced={} emf_plus_ignored={} mode={} -> {}; changed={} \
         objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.emf.display(),
        args.page,
        emf.record_count(),
        emf.image_count(),
        p.llx,
        p.lly,
        p.urx,
        p.ury,
        match args.fit {
            SvgFit::Contain => "contain",
            SvgFit::Stretch => "stretch",
            SvgFit::Natural => "natural",
        },
        if args.stamp { "stamp" } else { "content" },
        placed.form_id.num,
        placed.form_id.generation,
        id(placed.content_id),
        id(placed.annot_id),
        placed.scale_x,
        placed.scale_y,
        u32::from(placed.distorted),
        placed.objects_written,
        tally(&n.skipped),
        tally(&n.approximated),
        n.characters_replaced,
        u32::from(n.emf_plus_ignored),
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
}

use super::*;

/// Implement `pdfcer rotate-page`.
///
/// `--page` is 1-based, matching every PDF reader and every human;
/// `pdfcer-core` is 0-based, and the conversion happens here rather than
/// in the engine. `0` and past-the-end take the same path — a named
/// refusal from the engine, reported with the real page count.
pub(crate) fn cmd_rotate_page(
    input: &Path,
    page: u32,
    degrees: i32,
    relative: bool,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // 1-based -> 0-based. `checked_sub` handles `--page 0` without a
    // wrap; the engine handles past-the-end and names the real count.
    let Some(index) = page.checked_sub(1).map(|i| i as usize) else {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            input.display()
        );
        return exit::EDIT_REFUSED;
    };

    let result = if relative {
        session.rotate_page_by(index, degrees)
    } else {
        session.set_page_rotation(index, degrees)
    };
    if let Err(err) = result {
        return report_edit_error(input, &err);
    }

    // Read the resulting rotation back out of the session rather than
    // recomputing it here: the normalization rules (positive modulo,
    // inheritance) live in one place, and a second copy would drift.
    let rotate = session
        .pages()
        .ok()
        .and_then(|pages| pages.get(index).map(|p| p.rotate))
        .unwrap_or(0);

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
        "rotate-page {} page {page} mode={} -> {}; \
rotate={rotate} changed={} objects={} verbatim={} reserialized={} promoted={} \
appended={} out_bytes={} undo_verified={} undo_identical={} delinearized={}",
        input.display(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.objects_verbatim,
        r.objects_reserialized,
        r.promoted.len(),
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
        u32::from(r.delinearized),
    );
    finish_edit(input, &outcome)
}

/// The four mutually-constrained flags that name `set-page-size`'s target
/// sheet, bundled so [`cmd_set_page_size`] stays inside clippy's
/// seven-argument limit.
///
/// A struct rather than an `#[allow(clippy::too_many_arguments)]`: these
/// four ARE one argument conceptually — "which sheet" — and clap already
/// enforces that exactly one of the two routes through them is populated,
/// so grouping them costs nothing and makes the constraint visible in the
/// type.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SheetArg<'a> {
    /// `--size NAME`, a [`pdfcer_core::paper::PaperSize`] identifier.
    pub(crate) size: Option<&'a str>,
    /// `--landscape`; meaningful only alongside `size`.
    pub(crate) landscape: bool,
    /// `--width PT`, the custom route's width.
    pub(crate) width: Option<f64>,
    /// `--height PT`, the custom route's height.
    pub(crate) height: Option<f64>,
}

/// Implement `pdfcer set-page-size`.
///
/// # How the target rectangle is worked out
///
/// Exactly one of two routes, enforced by clap rather than here:
///
/// * `--size NAME [--landscape]` → [`pdfcer_core::paper::PaperSize`],
///   which is in `pdfcer-core` precisely so this shell does not carry its
///   own copy of the numbers. An unknown name is a **named refusal**, not
///   a nearest match — resolving a typo to a plausible sheet size would
///   hand the operator a working file of the wrong size with no signal.
/// * `--width W --height H` in points, origin at `(0, 0)`.
///
/// # Why `--pages` rather than `--page`
///
/// A drawing set is resized as a set. `--pages` takes the same spec
/// every other multi-page subcommand takes (`3`, `1,4,7`, `2-5`, `all`),
/// which also means the 1-based/0-based conversion and the past-the-end
/// refusal are the shared, already-tested ones.
///
/// # What it prints, and why the counters are counters
///
/// Each page produces a [`pdfcer_core::edit::MediaBoxChange`], and three
/// of its fields are consequences the operator cannot see in the file:
/// a crop box the new sheet no longer contains, a sheet that lost area,
/// and a size outside Annex C.2's recommended range. Under rule 4 the
/// CLI **prints** those on the way past — the invocation is the commit,
/// there is no session to disclose into.
///
/// They are reported **twice on purpose**: as counters on the machine
/// line (so a script can branch on them) and as a per-page note on
/// stderr (so a human running one file sees which page). A counter alone
/// is easy to miss in a wall of `=0`s; a note alone is unparseable.
pub(crate) fn cmd_set_page_size(
    input: &Path,
    pages: &str,
    sheet: &SheetArg<'_>,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    use pdfcer_core::paper::{Orientation, PaperSize};

    let &SheetArg {
        size,
        landscape,
        width,
        height,
    } = sheet;

    // Resolve the rectangle BEFORE opening the file: a mistyped size
    // should not cost a parse, and the refusal reads better without a
    // preceding "opened 40 MB" delay.
    let rect = match (size, width, height) {
        (Some(name), _, _) => {
            let Some(paper) = PaperSize::from_id(name) else {
                eprintln!(
                    "pdfcer: `{name}` is not a known sheet size; try one of: {}",
                    PaperSize::ALL
                        .iter()
                        .map(|s| s.id())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return exit::EDIT_REFUSED;
            };
            paper.rect_with(if landscape {
                Orientation::Landscape
            } else {
                Orientation::Portrait
            })
        }
        (None, Some(w), Some(h)) => pdfcer_core::page_tree::Rect::from_corners(0.0, 0.0, w, h),
        // clap's `required_unless_present` + `requires` make this
        // unreachable; it is spelled out rather than `unreachable!()`
        // because a panic-free binary must not depend on an argument
        // parser's configuration staying correct.
        _ => {
            eprintln!("pdfcer: give either --size, or both --width and --height");
            return exit::EDIT_REFUSED;
        }
    };

    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let count = match session.pages() {
        Ok(pages) => pages.len(),
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let targets = match parse_pages(pages, count) {
        Ok(list) => list,
        Err(msg) => {
            eprintln!("pdfcer: {}: --pages: {msg}", input.display());
            return exit::EDIT_REFUSED;
        }
    };

    // One command for the whole selection, not one per page: §11.3 makes
    // one operator gesture one undo entry. The CLI has no undo stack, but
    // the verb it calls is the one the GUI will call, and the granularity
    // is the verb's, not the shell's.
    let changes = match session.set_media_boxes(&targets, rect) {
        Ok(changes) => changes,
        Err(err) => return report_edit_error(input, &err),
    };

    let (mut lost_area, mut crop_outside, mut advisories) = (0_usize, 0_usize, 0_usize);
    let (mut explicit, mut base_kept, mut inherited_removed) = (0_usize, 0_usize, 0_usize);
    for change in &changes {
        match change.entry {
            pdfcer_core::edit::MediaBoxEntry::ExplicitWritten => explicit += 1,
            pdfcer_core::edit::MediaBoxEntry::BaseSpellingKept => base_kept += 1,
            pdfcer_core::edit::MediaBoxEntry::InheritedSoOwnEntryRemoved => inherited_removed += 1,
            // `MediaBoxEntry` is #[non_exhaustive]; a future variant must
            // not silently vanish from the counters.
            _ => {}
        }
        let human = change.page_index + 1;
        if change.lost_area {
            lost_area += 1;
            eprintln!(
                "pdfcer: note: page {human}: the sheet lost area. pdfcer removed no content, \
                 but §14.11.2.1 lets any other tool discard content outside the media box \
                 \"without affecting the meaning of the PDF file\" — so the loss becomes \
                 permanent on the first round trip through one."
            );
        }
        if let Some(crop) = change.crop_box_outside {
            crop_outside += 1;
            eprintln!(
                "pdfcer: note: page {human}: /CropBox [{:.4} {:.4} {:.4} {:.4}] is no longer \
                 inside the sheet. It is left as-is (§5); every conforming reader intersects it \
                 with the media box (§14.11.2.1), so the visible region is now the smaller of \
                 the two.",
                crop.llx, crop.lly, crop.urx, crop.ury
            );
        }
        if let Some(advice) = change.size_advisory {
            advisories += 1;
            let which = match (advice.below_minimum, advice.above_maximum) {
                (true, true) => {
                    "below the recommended minimum on one edge and above the \
                                 recommended maximum on the other"
                }
                (true, false) => "below the recommended 3-unit minimum",
                _ => "above the recommended 14 400-unit maximum",
            };
            eprintln!(
                "pdfcer: note: page {human}: the sheet is {which} (ISO 32000-1 Annex C.2, \
                 which says \"should\" and which ISO 32000-2 dropped entirely). Written as \
                 asked; some readers may not handle it."
            );
        }
    }

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
        "set-page-size {} pages {pages} mode={} -> {}; \
size={:.4}x{:.4} pages_set={} explicit={explicit} base_kept={base_kept} \
inherited_removed={inherited_removed} lost_area={lost_area} crop_outside={crop_outside} \
size_advisory={advisories} changed={} objects={} verbatim={} reserialized={} promoted={} \
appended={} out_bytes={} undo_verified={} undo_identical={} delinearized={}",
        input.display(),
        mode.name(),
        output.display(),
        rect.width(),
        rect.height(),
        changes.len(),
        outcome.changed,
        r.objects_written,
        r.objects_verbatim,
        r.objects_reserialized,
        r.promoted.len(),
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
        u32::from(r.delinearized),
    );
    finish_edit(input, &outcome)
}

/// Implement `pdfcer set-info`.
///
/// `sets` is every `(field, Option<value>)` pair the flags produced;
/// `clears` is the `--clear` list. A field that appears in both is
/// **cleared**, because `--clear` is the more explicit request — and
/// that resolution is stated here rather than left to argument order,
/// which a script author cannot see.
pub(crate) fn cmd_set_info(
    input: &Path,
    sets: &[(InfoFieldArg, Option<String>)],
    clears: &[InfoFieldArg],
    output: &Path,
    mode: SaveMode,
    producer: ProducerArg,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let had_info = session
        .document()
        .trailer()
        .get(b"Info")
        .and_then(pdfcer_core::object::Object::as_reference)
        .is_some();

    if sets.iter().all(|(_, v)| v.is_none()) && clears.is_empty() {
        eprintln!(
            "pdfcer: {}: no fields given — pass at least one of --title/--author/\
--subject/--keywords, or --clear <field>",
            input.display()
        );
        return exit::EDIT_REFUSED;
    }

    for (field, value) in sets {
        if clears.contains(field) {
            continue;
        }
        let Some(text) = value else { continue };
        if let Err(err) = session.set_info_field((*field).into(), Some(text.as_str())) {
            return report_edit_error(input, &err);
        }
    }
    for field in clears {
        if let Err(err) = session.set_info_field((*field).into(), None) {
            return report_edit_error(input, &err);
        }
    }

    let created = !had_info && session.dirty_set().trailer_patch().contains_key(b"Info");

    let outcome = match save_edited(&mut session, &source, output, mode, producer, verify_undo) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };

    let r = &outcome.report;
    println!(
        "set-info {} mode={} -> {}; \
changed={} objects={} verbatim={} reserialized={} promoted={} appended={} out_bytes={} \
info_created={} undo_verified={} undo_identical={} delinearized={}",
        input.display(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.objects_verbatim,
        r.objects_reserialized,
        r.promoted.len(),
        r.bytes_appended,
        r.bytes_written,
        u32::from(created),
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
        u32::from(r.delinearized),
    );
    if created {
        // Creating an object is a bigger structural act than editing
        // one, and R41's whole discipline is that pdfcer does not add
        // things to a file unasked — so when it legitimately does,
        // because the operator asked, it says so.
        eprintln!(
            "pdfcer: {}: this file had no document information dictionary; one was created \
to hold the metadata you asked for, and the trailer now references it.",
            input.display()
        );
    }
    finish_edit(input, &outcome)
}

/// The parsed arguments of `pdfcer annotate`, grouped into one struct
/// so [`cmd_annotate`] stays under clippy's argument-count limit.
pub(crate) struct AnnotateArgs<'a> {
    pub(crate) input: &'a Path,
    /// `--note` / `--note-author` / `--note-date` (`Pass 150.0`). Owned
    /// rather than borrowed because they are assembled into a `MarkupNote`
    /// the session keeps.
    pub(crate) note: Option<String>,
    pub(crate) note_author: Option<String>,
    pub(crate) note_date: Option<String>,
    pub(crate) kind: AnnotKindArg,
    pub(crate) page: u32,
    pub(crate) rect: Option<&'a str>,
    pub(crate) line: Option<&'a str>,
    pub(crate) points: Option<&'a str>,
    pub(crate) strokes: Option<&'a str>,
    pub(crate) quads: Option<&'a str>,
    pub(crate) color: Option<&'a str>,
    pub(crate) fill: Option<&'a str>,
    pub(crate) width: f64,
    pub(crate) cloud: Option<f64>,
    /// `/CA` (Pass 81.1). `None` omits the key.
    pub(crate) opacity: Option<f64>,
    /// `--dash ON,OFF,...` (`Pass 258.0`). `None` authors a solid border.
    pub(crate) dash: Option<&'a str>,
    pub(crate) text: Option<&'a str>,
    pub(crate) font: &'a str,
    pub(crate) size: f64,
    pub(crate) quad: QuadArg,
    pub(crate) multiline: bool,
    pub(crate) icon: IconArg,
    pub(crate) stamp_name: StampArg,
    pub(crate) stamp_font_size: Option<f64>,
    pub(crate) stamp_fit: StampFitArg,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// Whether an [`AnnotKindArg`] is one of the Pass-6.2 text-bearing
/// subtypes (which take the variable-text path) or a Pass-6.1 geometric
/// one.
pub(crate) fn is_text_bearing(kind: AnnotKindArg) -> bool {
    matches!(
        kind,
        AnnotKindArg::Freetext | AnnotKindArg::Text | AnnotKindArg::Stamp
    )
}

/// Implement `pdfcer annotate` (Pass 6.1). Parses the per-subtype
/// geometry flags into a [`MarkupSpec`](pdfcer_core::annot_author::MarkupSpec)
/// and authors it through the same [`EditSession`] path the GUI uses.
pub(crate) fn cmd_annotate(args: &AnnotateArgs<'_>) -> u8 {
    let input = args.input;
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let Some(index) = args.page.checked_sub(1).map(|i| i as usize) else {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            input.display()
        );
        return exit::EDIT_REFUSED;
    };

    // The operator's `/QuadPoints` corner order (ambiguity `QP-A1`,
    // §12.5.6.10). Read from the store here, in the SHELL, and handed to the
    // session — the same convention `--unmappable-code` and the writer's
    // `xref_entry_eol` follow, and the reason `pdfcer-core` never opens a
    // settings file itself.
    //
    // Until this line existed the setting was parsed, validated and written
    // back, and READ BY NOTHING: an operator who chose `counterclockwise` got
    // reading order anyway, everywhere. R83 — a setting is a promise.
    let (settings, settings_report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    report_settings(&settings_report);
    session.set_quad_point_order(settings.quad_point_order);

    // Pass 6.2 text-bearing subtypes take the variable-text path
    // (add_text_annotation); the Pass 6.1 geometric subtypes take
    // add_markup. Both share every guard and the same save/undo plumbing.
    // `Pass 81.1`: one options value, threaded to BOTH authoring verbs.
    // Built once rather than at each call site so the two routes cannot
    // drift into disagreeing about what `--opacity` means -- which is R92's
    // failure mode, and is exactly how `/CA` came to be reachable from the
    // restyle verb and not the author verb in the first place.
    // `Pass 150.0`: the note rides in the same options value for the same
    // reason `--opacity` does — one build point, so the two author routes
    // cannot drift into disagreeing about what `--note` means.
    //
    // A note is built when ANY of the three flags is given, so
    // `--note-author` alone still writes a `/T`. Requiring `--note` first
    // would refuse a legitimate "attribute this shape to me" with nothing to
    // say about it.
    //
    // Built through the BUILDER, not a struct literal: `MarkupNote` is
    // `#[non_exhaustive]`, so a struct expression does not compile outside
    // `pdfcer-core` at all. That is the type doing its job -- a field added to
    // it later cannot break this crate, which is the opposite of
    // `MarkupOptions`, deliberately constructible and therefore breaking.
    let note = if args.note.is_some() || args.note_author.is_some() || args.note_date.is_some() {
        let mut n = pdfcer_core::edit::MarkupNote::new(args.note.clone().unwrap_or_default());
        if let Some(a) = args.note_author.clone() {
            n = n.by(a);
        }
        if let Some(d) = args.note_date.clone() {
            n = n.at(d);
        }
        Some(n)
    } else {
        None
    };
    // A parse failure is refused BEFORE the session is opened, so a bad
    // pattern leaves the file untouched rather than partway through.
    let dash = match parse_dash_edit(args.dash) {
        Ok(Some(pdfcer_core::edit::StyleEdit::Set(d))) => Some(d),
        // `--dash solid` on an AUTHORING call is the default, not an edit:
        // there is no existing dash to clear.
        Ok(Some(pdfcer_core::edit::StyleEdit::Clear) | None) => None,
        Err(msg) => {
            eprintln!("pdfcer: {msg}");
            return exit::EDIT_REFUSED;
        }
    };
    let markup_options = pdfcer_core::edit::MarkupOptions {
        opacity: args.opacity,
        note,
        dash,
    };
    // `Pass 291.0`: the reporting route, because the CLI's whole disclosure
    // mechanism is PRINTING (rule 11 -- the invocation is the commit, there is
    // no session to hold a status line). `add_text_annotation_with` discards
    // what the generator decided, and a stamp whose label was shrunk or
    // clipped reached the operator as silence.
    let mut authored_text: Option<pdfcer_core::edit::TextAnnotOutcome> = None;
    let add_result = if is_text_bearing(args.kind) {
        match build_text_annot_spec(args) {
            Ok(spec) => session
                .add_text_annotation_reporting(index, &spec, &markup_options)
                .map(|o| {
                    authored_text = Some(o);
                }),
            Err(msg) => {
                eprintln!("pdfcer: {}: {msg}", input.display());
                return exit::EDIT_REFUSED;
            }
        }
    } else {
        match build_markup_spec(args) {
            Ok(spec) => session
                .add_markup_with(index, &spec, &markup_options)
                .map(|_| ()),
            Err(msg) => {
                eprintln!("pdfcer: {}: {msg}", input.display());
                return exit::EDIT_REFUSED;
            }
        }
    };
    if let Err(err) = add_result {
        return report_edit_error(input, &err);
    }

    // Printed BEFORE the save line, in the same order the operator reads:
    // what pdfcer decided, then what it wrote. Nothing here is printed for an
    // annotation pdfcer did not have to decide anything about.
    if let Some(o) = &authored_text {
        report_text_annot_inferences(input, o);
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

    let r = &outcome.report;
    println!(
        "annotate {} type={:?} page={} mode={} -> {}; \
changed={} objects={} verbatim={} reserialized={} promoted={} appended={} out_bytes={} \
undo_verified={} undo_identical={} delinearized={}",
        input.display(),
        args.kind,
        args.page,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.objects_verbatim,
        r.objects_reserialized,
        r.promoted.len(),
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
        u32::from(r.delinearized),
    );
    finish_edit(input, &outcome)
}

// ---------------------------------------------------------------------------
// Redaction subcommands (Pass 8, ISO 32000-1 §12.5.6.23)
// ---------------------------------------------------------------------------

/// Parsed `redact-mark` flags.
pub(crate) struct RedactMarkArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) rect: Option<&'a str>,
    pub(crate) search: Option<&'a str>,
    pub(crate) pattern: Option<&'a str>,
    pub(crate) ignore_case: bool,
    pub(crate) page: u32,
    pub(crate) fill: Option<&'a str>,
    pub(crate) overlay_text: Option<&'a str>,
    pub(crate) output: &'a Path,
}

/// `redact-mark`: author reviewable `/Redact` marks (the non-destructive
/// MARK phase). Removal is a separate `redact-apply` (R52).
pub(crate) fn cmd_redact_mark(args: &RedactMarkArgs<'_>) -> u8 {
    use pdfcer_core::annot_author::{Quad, RedactAppearance};
    use pdfcer_core::vartext::Quadding;

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let fill = match args.fill {
        Some(h) => match parse_color(h) {
            Ok(c) => Some(c),
            Err(msg) => {
                eprintln!("pdfcer: {}: {msg}", args.input.display());
                return exit::EDIT_REFUSED;
            }
        },
        None => None,
    };
    let overlay_text = args.overlay_text.map(str::to_string);
    // ONE appearance, used by all three marking paths. It used to be built
    // only on the --rect path; --search printed a note saying the settings
    // were ignored, and --pattern dropped them with no note at all.
    let appearance = RedactAppearance {
        fill,
        overlay_text,
        quadding: Quadding::Left,
    };

    let created = if let Some(rectspec) = args.rect {
        let Some(index) = args.page.checked_sub(1).map(|i| i as usize) else {
            eprintln!(
                "pdfcer: {}: --page is 1-based; 0 is not a page",
                args.input.display()
            );
            return exit::EDIT_REFUSED;
        };
        let rect = match rect_from(rectspec) {
            Ok(r) => r,
            Err(msg) => {
                eprintln!("pdfcer: {}: {msg}", args.input.display());
                return exit::EDIT_REFUSED;
            }
        };
        let spec = appearance.to_spec(vec![Quad::from_rect(rect)]);
        match session.add_redaction(index, &spec) {
            Ok(_) => 1usize,
            Err(err) => return report_edit_error(args.input, &err),
        }
    } else if let Some(query) = args.search {
        let options =
            pdfcer_core::edit::TextSearchOptions::default().with_case_insensitive(args.ignore_case);
        // `search_and_mark_redactions_styled`, not the `Vec<ObjId>` sibling:
        // the diagnostics are what separate "the term is not here" from
        // "this document's text was never readable", and on a redaction path
        // those need opposite reactions from the operator.
        match session.search_and_mark_redactions_styled(query, &options, &appearance) {
            Ok(marked) => {
                report_unsearchable_redaction(args.input, &marked.diagnostics);
                marked.created.len()
            }
            Err(err) => return report_edit_error(args.input, &err),
        }
    } else if let Some(pattern) = args.pattern {
        // The same disclosure as `--search`, which this branch did NOT make
        // until `Pass 296.3`. The diagnostics were computed on this path all
        // along and thrown away one `.map` short of the caller, so `pdfcer`
        // itself was silent about unreadable text on a pattern redaction --
        // rule 4's failure mode in pdfcer's own shell, on the route an
        // operator reaches for wildcards to clean structured confidential
        // material.
        match session.search_and_mark_redactions_by_pattern_styled(
            pattern,
            args.ignore_case,
            &appearance,
        ) {
            Ok(marked) => {
                report_unsearchable_redaction(args.input, &marked.diagnostics);
                marked.created.len()
            }
            Err(err) => return report_edit_error(args.input, &err),
        }
    } else {
        eprintln!(
            "pdfcer: {}: give exactly one of --rect, --search, or --pattern",
            args.input.display()
        );
        return exit::EDIT_REFUSED;
    };

    if created == 0 {
        eprintln!(
            "pdfcer: {}: no content matched — no redaction marks authored",
            args.input.display()
        );
    }

    // Marks are additive, so an incremental save is correct and
    // signature-safe (they are not the destructive apply).
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        SaveMode::Incremental,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(o) => o,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "redact-mark {} marks_created={} -> {}; changed={} appended={} out_bytes={}",
        args.input.display(),
        created,
        args.output.display(),
        outcome.changed,
        r.bytes_appended,
        r.bytes_written,
    );
    if created > 0 {
        println!(
            "  {created} /Redact mark(s) authored — REVIEW then run `redact-apply` to remove the \
             content. The document is NOT yet redacted."
        );
    }
    exit::SUCCESS
}

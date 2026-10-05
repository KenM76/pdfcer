//! `place-text`: import a plain-text file as PDF pages.

use super::*;

/// Named arguments for [`cmd_place_text`] (grouped to dodge clippy's
/// `too_many_arguments`, matching [`AddTextArgs`]).
pub(crate) struct PlaceTextArgs<'a> {
    /// The plain-text file to import.
    pub(crate) text_file: &'a Path,
    pub(crate) output: &'a Path,
    /// The PDF to insert into, or `None` to have pdfcer create the document.
    pub(crate) input: Option<&'a Path>,
    /// `end` | `start` | `before:N` | `after:N`, N 1-based.
    pub(crate) position: &'a str,
    /// Named sheet size id (`letter`, `a4`, …).
    pub(crate) paper: &'a str,
    pub(crate) landscape: bool,
    /// Explicit `"W,H"` sheet size in points, overriding `paper`/`landscape`.
    pub(crate) page_size: Option<&'a str>,
    /// `(all, left, right, top, bottom)` — the per-side values override `all`.
    ///
    /// A tuple rather than five fields because they are one input with one
    /// resolution rule, and splitting them invites a caller to apply four of
    /// them and forget the fifth.
    pub(crate) margins: (f64, Option<f64>, Option<f64>, Option<f64>, Option<f64>),
    /// Standard-14 `BaseFont` name or `auto`.
    pub(crate) font: &'a str,
    pub(crate) size: f64,
    /// Leading in points, or `None` for the derived `1.2 x size`.
    pub(crate) leading: Option<f64>,
    /// Alignment keyword, or `None` (defaults to left).
    pub(crate) align: Option<&'a str>,
    /// `"r,g,b"` fill colour, or `None` for black.
    pub(crate) color: Option<&'a str>,
    /// Place the text and drop what the face cannot encode, instead of
    /// refusing the whole import.
    pub(crate) drop_unmappable: bool,
    pub(crate) mode: SaveMode,
    pub(crate) producer: ProducerArg,
}

/// `place-text`: import a plain-text file as PDF pages.
///
/// The batch half of `EditSession::place_text` (rule 11 — every feature ships
/// its `pdfcer` equivalent in the same session as the engine verb). The
/// operator's real input is a `.txt` on disk, so this reads a file rather than
/// taking a `--text` string the way `add-text` does: a shell that has to
/// inline a 40 KB document into an argument list has not been given a batch
/// tool.
///
/// ## Two shapes, one verb
///
/// With `--input`, pages are inserted into that document at `--position`.
/// Without it there is no document, and `EditSession::place_text` deliberately
/// refuses to insert beside nothing — so this builds a ONE-page scaffold with
/// `blank_document`, imports after it, and deletes the scaffold. That is three
/// engine calls rather than a fourth code path, and the CLI is the right place
/// for it: the invocation IS the commit here (rule 11), so the extra undo entry
/// the delete costs is not observable, whereas a "create a document" mode
/// inside the engine verb would be.
///
/// ## What it prints
///
/// Every field of the report, in `key=value` form for a script, then the
/// verbatim disclosures. The counts that matter most are the ones describing
/// what did NOT survive the import — dropped characters, collapsed tabs,
/// blank pages — because those are the ones nothing in the output file can
/// tell the operator (rule 4).
pub(crate) fn cmd_place_text(args: &PlaceTextArgs<'_>) -> u8 {
    // Everything the operator typed is validated before any file is read.
    let (template, media, align) = match place_text_template(args) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let text = match std::fs::read_to_string(args.text_file) {
        Ok(t) => t,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.text_file.display());
            return exit::IO_ERROR;
        }
    };
    let creating = args.input.is_none();
    let (source, mut session) = match place_text_session(args.input, media) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let position = match place_text_position(args.position, creating) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let report = match session.place_text(&text, &template, position) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("pdfcer: place-text refused: {err}");
            return match err {
                pdfcer_core::text_edit::PlaceTextError::Scaffold(_) => exit::RUNTIME_ERROR,
                _ => exit::EDIT_REFUSED,
            };
        }
    };
    if creating && let Err(code) = remove_scaffold_page(&mut session) {
        return code;
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        args.producer,
        false,
    ) {
        Ok(o) => o,
        Err(code) => return code,
    };
    print_place_text_report(args, &report, align, creating);
    finish_edit(args.text_file, &outcome)
}

/// The page template from `--page-size`/`--paper`, margins, font, size,
/// leading, alignment, colour and `--drop-unmappable`.
fn place_text_template(
    args: &PlaceTextArgs<'_>,
) -> Result<
    (
        pdfcer_core::text_edit::PageTemplate,
        pdfcer_core::page_tree::Rect,
        pdfcer_core::text_edit::BlockAlignment,
    ),
    u8,
> {
    use pdfcer_core::text_edit::{PageTemplate, Unmappable};
    let media = place_text_media(args)?;
    let font = place_text_font(args.font)?;
    let align = parse_block_align(args.align)?;
    let color = add_text_color(args.color)?;
    let (all, left, right, top, bottom) = args.margins;
    let template = PageTemplate::new()
        .with_media_box(media)
        .with_margins(
            left.unwrap_or(all),
            right.unwrap_or(all),
            top.unwrap_or(all),
            bottom.unwrap_or(all),
        )
        .with_font(font)
        .with_size(args.size)
        .with_leading(args.leading)
        .with_alignment(align)
        .with_color(color)
        .with_unmappable(if args.drop_unmappable {
            Unmappable::Drop
        } else {
            Unmappable::Refuse
        });
    Ok((template, media, align))
}

/// The sheet: an explicit `--page-size "W,H"`, else the named `--paper`.
fn place_text_media(args: &PlaceTextArgs<'_>) -> Result<pdfcer_core::page_tree::Rect, u8> {
    use pdfcer_core::page_tree::Rect;
    use pdfcer_core::paper::{Orientation, PaperSize};
    if let Some(s) = args.page_size {
        return match parse_at_pair(s) {
            Some((w, h)) if w > 0.0 && h > 0.0 => Ok(Rect::from_corners(0.0, 0.0, w, h)),
            _ => {
                eprintln!(
                    "pdfcer: --page-size expects two positive comma-separated numbers \"W,H\" \
                     (points), got {s:?}"
                );
                Err(exit::EDIT_REFUSED)
            }
        };
    }
    let orientation = if args.landscape {
        Orientation::Landscape
    } else {
        Orientation::Portrait
    };
    PaperSize::from_id(args.paper)
        .map(|p| p.rect_with(orientation))
        .ok_or_else(|| {
            eprintln!(
                "pdfcer: --paper {:?} is not a known sheet size (letter, legal, a0..a6, \
                 tabloid, executive, ansi-a..ansi-e)",
                args.paper
            );
            exit::EDIT_REFUSED
        })
}

/// `--font` for `place-text`: `auto` is Helvetica, otherwise a Standard-14
/// spelling.
fn place_text_font(name: &str) -> Result<pdfcer_core::fontdata::Std14, u8> {
    use pdfcer_core::fontdata::{Std14, std14_by_base_font};
    if name.eq_ignore_ascii_case("auto") {
        return Ok(Std14::Helvetica);
    }
    std14_by_base_font(name).ok_or_else(|| {
        eprintln!(
            "pdfcer: --font {name:?} is not a Standard-14 BaseFont name \
             (e.g. Helvetica, Times-Roman, Courier-Bold)"
        );
        exit::EDIT_REFUSED
    })
}

/// `--align`, left when absent.
pub(crate) fn parse_block_align(
    raw: Option<&str>,
) -> Result<pdfcer_core::text_edit::BlockAlignment, u8> {
    use pdfcer_core::text_edit::BlockAlignment;
    let Some(s) = raw else {
        return Ok(BlockAlignment::Left);
    };
    BlockAlignment::parse(s).ok_or_else(|| {
        eprintln!("pdfcer: --align {s:?}: expected left|center|right|justify");
        exit::EDIT_REFUSED
    })
}

/// The document to import into: `--input`, or a one-page scaffold to splice
/// beside (see [`cmd_place_text`] for why the engine verb does not do this).
fn place_text_session(
    input: Option<&Path>,
    media: pdfcer_core::page_tree::Rect,
) -> Result<(Vec<u8>, pdfcer_core::edit::EditSession), u8> {
    if let Some(input) = input {
        return open_for_edit(input);
    }
    let doc = pdfcer_core::text_edit::blank_document(media, 1).map_err(|err| {
        eprintln!("pdfcer: could not create a document: {err}");
        exit::RUNTIME_ERROR
    })?;
    let bytes = doc.bytes().to_vec();
    Ok((bytes, pdfcer_core::edit::EditSession::new(doc)))
}

/// `--position`, through the same parser `insert-pages --at` uses; a created
/// document always imports after its scaffold.
fn place_text_position(
    raw: &str,
    creating: bool,
) -> Result<pdfcer_core::pageops::InsertPosition, u8> {
    if creating {
        return Ok(pdfcer_core::pageops::InsertPosition::End);
    }
    parse_insert_position(raw).map_err(|message| {
        eprintln!("pdfcer: --position {raw:?}: {message}");
        exit::EDIT_REFUSED
    })
}

/// Delete the scaffold, page 0 (`End` imported after it), so a created
/// document holds exactly the pages the text needed.
fn remove_scaffold_page(session: &mut pdfcer_core::edit::EditSession) -> Result<(), u8> {
    match session.delete_pages(&[0]) {
        Ok(out) if out.pages_removed == 1 => Ok(()),
        Ok(out) => {
            eprintln!(
                "pdfcer: internal: removing the scaffold page removed {} page(s), not 1. \
                 This is a bug; refusing rather than writing a document with a stray page",
                out.pages_removed
            );
            Err(exit::RUNTIME_ERROR)
        }
        Err(err) => {
            eprintln!("pdfcer: internal: the scaffold page could not be removed: {err}");
            Err(exit::RUNTIME_ERROR)
        }
    }
}

fn print_place_text_report(
    args: &PlaceTextArgs<'_>,
    report: &pdfcer_core::text_edit::PlaceTextReport,
    align: pdfcer_core::text_edit::BlockAlignment,
    creating: bool,
) {
    println!(
        "place-text {} -> {}",
        args.text_file.display(),
        args.output.display()
    );
    println!(
        "  pages_created={} first_page={} blank_pages={} lines_placed={} lines_per_page={}",
        report.pages_created,
        // 1-based, on the finished document: a created one has lost its scaffold.
        if creating {
            1
        } else {
            report.first_page_index + 1
        },
        report.blank_pages,
        report.lines_placed,
        report.lines_per_page
    );
    println!(
        "  chars_input={} chars_placed={} whitespace_normalised={} controls_dropped={} \
         unmappable_dropped={}",
        report.chars_input,
        report.chars_placed,
        report.whitespace_normalised,
        report.chars_dropped_control,
        report.chars_dropped_unmappable
    );
    println!(
        "  bom_stripped={} crlf_normalised={} tabs_collapsed={} page_breaks={} \
         overlong_words={} paragraphs_split={}",
        report.bom_stripped,
        report.crlf_normalised,
        report.tabs_collapsed,
        report.explicit_page_breaks,
        report.overlong_words,
        report.paragraphs_split_across_pages
    );
    println!(
        "  leading={:.2}{} alignment={} box_overflow_lines={} undo_entries={} coalesced={}",
        report.leading,
        if report.leading_derived {
            " (derived)"
        } else {
            ""
        },
        align.as_str(),
        report.box_overflow_lines,
        report.undo_entries,
        report.coalesced
    );
    if !report.dropped_unmappable_chars.is_empty() {
        // Named, not just counted, so the operator can act on it.
        let named: Vec<String> = report
            .dropped_unmappable_chars
            .iter()
            .map(|(c, n)| format!("U+{:04X} x{n}", *c as u32))
            .collect();
        println!("  dropped_characters: {}", named.join(", "));
    }
    if creating && args.mode == SaveMode::Incremental {
        eprintln!(
            "pdfcer: {}: pdfcer created this document, so its base revision is the one blank \
scaffold page the import was placed beside. Under --mode incremental that page's object stays in \
the file's revision history (ISO 32000-1 §7.5.6 appends; it does not erase). --mode full writes \
the finished document without it.",
            args.output.display()
        );
    }
    println!("  disclosures:");
    for d in &report.disclosures {
        println!("    - {d}");
    }
}

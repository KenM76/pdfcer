//! `pdfcer copy-text`: a page's text as formatted paragraphs, on the
//! clipboard (`HTML Format`, `Rich Text Format`, `CF_UNICODETEXT`) or in a
//! file, so a word processor pastes it with its bold, sizes, families and
//! paragraph breaks. The engine builds the payloads
//! ([`EditableTextModel::rich_text`]); this module only places them.

use super::*;
use pdfcer_core::text_edit::model::{PageFonts, RichText, RichTextSources};
use pdfcer_core::text_edit::{EditableTextModel, reflow_recognition_options};
use pdfcer_core::text_extract::{self, ExtractOptions};

/// What `copy-text -o` writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum RichFormat {
    /// UTF-8 HTML with inline styles.
    Html,
    /// HTML behind the Windows CF_HTML clipboard header.
    CfHtml,
    /// Rich Text Format.
    Rtf,
    /// Plain text, one paragraph per line.
    Text,
}

/// Implement `pdfcer copy-text`.
pub(crate) fn cmd_copy_text(
    input: &Path,
    pages_spec: &str,
    output: Option<&Path>,
    format: RichFormat,
) -> u8 {
    let rich = match rich_pages(input, pages_spec) {
        Ok(rich) => rich,
        Err(code) => return code,
    };
    let placed = match output {
        Some(path) => {
            let bytes = match format {
                RichFormat::Html => rich.to_html(),
                RichFormat::CfHtml => rich.to_cf_html(),
                RichFormat::Rtf => rich.to_rtf(),
                RichFormat::Text => rich.plain_text() + "\n",
            };
            if let Err(err) = write_output(path, bytes.as_bytes()) {
                eprintln!("pdfcer: {}: {err}", path.display());
                return exit::IO_ERROR;
            }
            "file".to_owned()
        }
        None => match place(&rich) {
            Ok(formats) => formats,
            Err(message) => {
                eprintln!("pdfcer: {}: {message}", input.display());
                return exit::RUNTIME_ERROR;
            }
        },
    };
    let o = rich.outcome;
    println!(
        "copied paragraphs={} weights_derived={} families_missing={} colours_dropped={} decoration_forms_unread={} to={placed}",
        o.paragraphs,
        o.weights_derived,
        o.families_missing,
        o.colours_dropped,
        o.decoration_forms_unread
    );
    if o.decoration_forms_unread > 0 {
        eprintln!(
            "pdfcer: note: {} form XObject(s) could not be read for underline and strikethrough; their text pastes undecorated.",
            o.decoration_forms_unread
        );
    }
    if o.colours_dropped > 0 {
        eprintln!(
            "pdfcer: note: {} glyph(s) are in a colour space this copy does not carry; they paste in the target's default colour.",
            o.colours_dropped
        );
    }
    exit::SUCCESS
}

/// Every selected page's formatted text, concatenated in page order.
fn rich_pages(input: &Path, pages_spec: &str) -> Result<RichText, u8> {
    let doc = open_document(input).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", input.display());
        exit_code_for_doc(&err)
    })?;
    let pages = pdfcer_core::page_tree::pages(&doc).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", input.display());
        exit::RUNTIME_ERROR
    })?;
    let indices = parse_pages(pages_spec, pages.len()).map_err(|message| {
        eprintln!("pdfcer: {}: --pages {message}", input.display());
        exit::EDIT_REFUSED
    })?;
    let (settings, report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    report_settings(&report);
    let options = ExtractOptions::default()
        .with_word_gap_ratio(settings.word_gap_ratio)
        .with_word_gap_auto(settings.word_gap_auto)
        .with_unmappable_code(settings.unmappable_code)
        .with_actual_text(settings.actual_text)
        .with_provenance(true);
    let view = doc.view();
    let mut all: Option<RichText> = None;
    for index in indices {
        let Some(page) = pages.get(index) else {
            continue;
        };
        let text = text_extract::extract_page(&doc, page, index, &options).map_err(|err| {
            eprintln!("pdfcer: {}: page {}: {err}", input.display(), index + 1);
            exit::RUNTIME_ERROR
        })?;
        let model = EditableTextModel::recognize(&text, &reflow_recognition_options());
        let fonts = PageFonts::read(&view, page);
        let deco = pdfcer_core::text_edit::decoration::page_decorations(&view, page).ok();
        let mut sources = RichTextSources::default().with_fonts(&fonts);
        if let Some(deco) = &deco {
            sources = sources.with_decorations(deco);
        }
        let rich = model.rich_text(None, &sources);
        match &mut all {
            None => all = Some(rich),
            Some(acc) => append(acc, rich),
        }
    }
    Ok(all.unwrap_or_default())
}

fn append(acc: &mut RichText, more: RichText) {
    acc.paragraphs.extend(more.paragraphs);
    let (a, b) = (&mut acc.outcome, more.outcome);
    a.paragraphs += b.paragraphs;
    a.weights_derived += b.weights_derived;
    a.families_missing += b.families_missing;
    a.colours_dropped += b.colours_dropped;
    a.decoration_forms_unread += b.decoration_forms_unread;
}

/// Place HTML, RTF and plain text, all or nothing (`clipboard::fill`).
#[cfg(windows)]
fn place(rich: &RichText) -> Result<String, String> {
    use crate::clipboard::{self, CF_UNICODETEXT, ClipboardEntry, EntryKind};
    let nul = |s: String| {
        let mut v = s.into_bytes();
        v.push(0);
        v
    };
    let entries = [
        ClipboardEntry::registered("HTML Format", nul(rich.to_cf_html())),
        ClipboardEntry::registered("Rich Text Format", nul(rich.to_rtf())),
        ClipboardEntry {
            name: "CF_UNICODETEXT",
            kind: EntryKind::Standard(CF_UNICODETEXT),
            bytes: clipboard::unicode_text_payload(&rich.plain_text()),
        },
    ];
    let placed = clipboard::place_entries(&entries).map_err(|e| e.to_string())?;
    Ok(placed.formats.join(","))
}

#[cfg(not(windows))]
fn place(_rich: &RichText) -> Result<String, String> {
    Err("the clipboard is Windows-only in this build; use -o with --format".to_owned())
}

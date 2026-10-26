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
    /// HTML behind the Windows `CF_HTML` clipboard header.
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
        "copied paragraphs={} weights_derived={} families_missing={} colours_dropped={} to={placed}",
        o.paragraphs, o.weights_derived, o.families_missing, o.colours_dropped
    );
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
}

/// Place HTML, RTF and plain text in one clipboard transaction.
#[cfg(windows)]
fn place(rich: &RichText) -> Result<String, String> {
    use clipboard_win::{Clipboard, options::NoClear, raw};
    let _guard = Clipboard::new_attempts(10).map_err(|_| "the clipboard is busy".to_owned())?;
    raw::empty().map_err(|_| "the clipboard could not be emptied".to_owned())?;
    for (name, bytes) in [
        ("HTML Format", rich.to_cf_html()),
        ("Rich Text Format", rich.to_rtf()),
    ] {
        let id = raw::register_format(name).ok_or_else(|| format!("could not register {name}"))?;
        let mut data = bytes.into_bytes();
        data.push(0);
        raw::set_without_clear(id.get(), &data).map_err(|_| format!("could not place {name}"))?;
    }
    raw::set_string_with(&rich.plain_text(), NoClear)
        .map_err(|_| "could not place CF_UNICODETEXT".to_owned())?;
    Ok("HTML Format,Rich Text Format,CF_UNICODETEXT".to_owned())
}

#[cfg(not(windows))]
fn place(_rich: &RichText) -> Result<String, String> {
    Err("the clipboard is Windows-only in this build; use -o with --format".to_owned())
}

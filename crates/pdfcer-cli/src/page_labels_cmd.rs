//! `page-labels` / `set-page-labels` / `clear-page-labels`: page labels
//! (ISO 32000-1 §12.4.2).

use super::*;
use pdfcer_core::page_labels::{LabelFormat, label_ranges, page_labels};

/// `page-labels` — the stored ranges, then every page's label.
pub(crate) fn cmd_page_labels(input: &Path) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let ranges = label_ranges(&doc);
    for range in &ranges {
        println!(
            "range first={} style={} prefix=\"{}\" start={}",
            range.first_page + 1,
            LabelStyleArg::name(range.format.style),
            range.format.prefix.escape_default(),
            range.format.start
        );
    }
    let labels = match page_labels(&doc) {
        Ok(labels) => labels,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    for (index, label) in labels.iter().enumerate() {
        println!("page {} label=\"{}\"", index + 1, label.escape_default());
    }
    println!("ranges={} pages={}", ranges.len(), labels.len());
    exit::SUCCESS
}

pub(crate) struct SetPageLabelsArgs<'a> {
    pub input: &'a Path,
    pub pages: &'a str,
    pub style: LabelStyleArg,
    pub start: u32,
    pub prefix: &'a str,
    pub output: &'a Path,
    pub mode: SaveMode,
    pub verify_undo: bool,
}

/// `--pages N` or `--pages N-M`, 1-based, into a 0-based inclusive range.
fn parse_label_pages(raw: &str, count: usize) -> Result<(usize, usize), String> {
    let (first, last) = match raw.split_once('-') {
        Some((a, b)) => (parse_page_number(a, count)?, parse_page_number(b, count)?),
        None => {
            let page = parse_page_number(raw, count)?;
            (page, page)
        }
    };
    if first > last {
        return Err(format!("{raw} runs backwards; give the first page first"));
    }
    Ok((first, last))
}

/// `set-page-labels` — label one page range.
pub(crate) fn cmd_set_page_labels(a: &SetPageLabelsArgs<'_>) -> u8 {
    let Some(start) = std::num::NonZeroU32::new(a.start) else {
        eprintln!("pdfcer: {}: --start must be 1 or more", a.input.display());
        return exit::RUNTIME_ERROR;
    };
    let (source, mut session) = match open_for_edit(a.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let count = match session.page_slots() {
        Ok(slots) => slots.len(),
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let (first, last) = match parse_label_pages(a.pages, count) {
        Ok(range) => range,
        Err(msg) => {
            eprintln!("pdfcer: {}: --pages {msg}", a.input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let format = LabelFormat::new(a.style.to_core())
        .with_prefix(a.prefix)
        .with_start(start);
    let ranges = match session.set_page_labels(first, last, &format) {
        Ok(ranges) => ranges,
        Err(err) => return report_edit_error(a.input, &err),
    };
    let saved = match save_edited(
        &mut session,
        &source,
        a.output,
        a.mode,
        ProducerArg::Preserve,
        a.verify_undo,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    println!(
        "set-page-labels {} pages={}-{} style={} first_label=\"{}\" mode={} -> {}; ranges={ranges} {}",
        a.input.display(),
        first + 1,
        last + 1,
        LabelStyleArg::name(format.style),
        format.label(0).escape_default(),
        a.mode.name(),
        a.output.display(),
        edit_metrics(&saved)
    );
    finish_edit(a.input, &saved)
}

/// `clear-page-labels` — remove the document's labels.
pub(crate) fn cmd_clear_page_labels(
    input: &Path,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let cleared = match session.clear_page_labels() {
        Ok(cleared) => cleared,
        Err(err) => return report_edit_error(input, &err),
    };
    if !cleared {
        eprintln!(
            "pdfcer: {}: the document has no page labels; nothing changed and nothing was recorded.",
            input.display()
        );
    }
    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    println!(
        "clear-page-labels {} cleared={} mode={} -> {}; {}",
        input.display(),
        u8::from(cleared),
        mode.name(),
        output.display(),
        edit_metrics(&saved)
    );
    finish_edit(input, &saved)
}

//! `3d-list` / `3d-extract`: embedded 3D artwork (ISO 32000-1 §13.6,
//! ISO 32000-2 §13.7).

use super::*;
use pdfcer_core::threed::{ThreeDArtwork, ThreeDSource, extract_3d, list_3d_with_notes};

fn format_label(art: &ThreeDArtwork) -> String {
    art.declared
        .as_ref()
        .map_or_else(|| "-".to_owned(), |f| f.label())
}

/// `3d-list` — one line per artwork, then one line per listing note.
pub(crate) fn cmd_list_3d(input: &Path) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let (found, notes) = list_3d_with_notes(&doc);
    for (index, art) in found.iter().enumerate() {
        let source = match &art.source {
            ThreeDSource::Stream { shared: false } => "stream".to_owned(),
            ThreeDSource::Stream { shared: true } => "shared-stream".to_owned(),
            ThreeDSource::RichMediaAsset { name, .. } => {
                format!("richmedia name={:?}", name.as_deref().unwrap_or("-"))
            }
            _ => "unknown".to_owned(),
        };
        println!(
            "3d index={index} page={} format={} views={} poster={} source={source}",
            art.page_index + 1,
            format_label(art),
            art.view_count,
            if art.has_poster { "yes" } else { "no" },
        );
    }
    println!("count={}", found.len());
    if notes.annotations_without_stream > 0 {
        println!(
            "note: {} 3D annotation(s) name no readable data stream",
            notes.annotations_without_stream
        );
    }
    if notes.truncated {
        println!("note: listing truncated at a safety limit");
    }
    if notes.page_tree_unwalkable {
        println!("note: the page tree could not be walked; nothing was listed");
    }
    exit::SUCCESS
}

/// `3d-extract` — write one artwork's decoded bytes to `output`.
pub(crate) fn cmd_extract_3d(input: &Path, index: usize, output: &Path) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let found = pdfcer_core::threed::list_3d(&doc);
    let Some(art) = found.get(index) else {
        eprintln!(
            "pdfcer: {}: no 3D artwork at index {index} (the document has {}). Run \
             `pdfcer 3d-list` to see them.",
            input.display(),
            found.len()
        );
        return exit::EDIT_REFUSED;
    };
    let extracted = match extract_3d(&doc.view(), art) {
        Ok(extracted) => extracted,
        Err(err) => {
            eprintln!("pdfcer: {}: 3D artwork {index}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    if let Err(err) = write_output(output, &extracted.data) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    let sniffed = extracted
        .sniffed
        .as_ref()
        .map_or_else(|| "unrecognised".to_owned(), |f| f.label());
    println!(
        "extracted index={index} format={} bytes={} content={sniffed} -> {}",
        format_label(art),
        extracted.data.len(),
        output.display()
    );
    if extracted.contradicts(art.declared.as_ref()) {
        println!(
            "note: the document declares {} but the bytes are {sniffed}; written unchanged",
            format_label(art)
        );
    }
    exit::SUCCESS
}

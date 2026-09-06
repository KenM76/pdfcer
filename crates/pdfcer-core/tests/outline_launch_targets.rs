//! # `Pass 258.2` — which file does this bookmark open?
//!
//! ## The ask, and why two other tools could not answer it
//!
//! An operator, 2026-09-06: a folder of PDFs, one of which is a table of
//! contents whose bookmarks open the others. He wanted *"the name of the
//! bookmark and the filename it points to"* and got, from `cpdf`, the
//! cryptic `<</F 251 0 R/S/Launch>>` — an unresolved indirect reference to a
//! file specification — and from `pdftk`, `BookmarkPageNumber: 0`, which is
//! `pdftk` correctly reporting that the bookmark names no page in *this*
//! document while saying nothing about the file it does name.
//!
//! ## pdfcer's own gap was narrower and slightly worse
//!
//! `file_spec_bytes` — the resolver that turns `251 0 R` into
//! `chapter1.pdf` — **already existed** and was already used for `/GoToR`.
//! `read_action` simply did not call it for `/Launch`: everything but the
//! action's `/S` name was discarded, so `Destination::NonNavigation` could
//! say *"this bookmark launches something"* and never *what*.
//!
//! Worse, for the `/GoToR` case where the filename WAS parsed,
//! `list-outline` printed the destination with Rust's `{:?}`, so the answer
//! reached the operator as `file: Some([99, 104, 97, ...])` — a decimal byte
//! array. The information was right and unreadable. `list-links` had a
//! readable renderer for the same enum the whole time.
//!
//! ## What the fixture covers, and why four shapes rather than one
//!
//! Table 203 (ISO 32000-2: Table 207) makes `/Launch`'s `/F` *required
//! unless* `/Win`, `/Mac` or `/Unix` is present, and those three are
//! **deprecated in PDF 2.0** (§12.6.4.6: *"The `F` entry determines the file
//! specification platform to be launched"*). So a real corpus contains at
//! least three spellings of the same intent, and `fixtures/synthetic/
//! outline/toc-launch-targets.pdf` carries all of them plus `/GoToR`:
//!
//! | bookmark | action | file named by |
//! |---|---|---|
//! | Chapter 1 | `/Launch` | **indirect** `/F` filespec dict — the operator's exact shape |
//! | Chapter 2 | `/Launch` | direct `/F` string |
//! | Appendix | `/Launch` | `/Win << /F >>` only (2.0-deprecated fallback) |
//! | Chapter 3 | `/GoToR` | `/F` + `/D` page |
//!
//! ## ★ Reading is not running
//!
//! R13 is untouched by this. A `/Launch` target is a filename to **show**,
//! and pdfcer resolves it precisely so an operator can see what a document
//! would do without a viewer doing it.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::outline::{Destination, read_outline};
use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/outline/toc-launch-targets.pdf")
}

fn items() -> Vec<pdfcer_core::outline::OutlineItem> {
    let doc = Document::load(&fixture()).expect("load the ToC fixture");
    let session = EditSession::new(doc);
    read_outline(&session.graph()).items
}

/// The file a bookmark names, whichever action names it.
fn target_file(item: &pdfcer_core::outline::OutlineItem) -> Option<String> {
    match item.destination.as_ref()? {
        Destination::NonNavigation { file, .. } | Destination::Remote { file, .. } => file
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).into_owned()),
        _ => None,
    }
}

/// ★ **THE ASK.** Title → filename, for every bookmark that opens a file.
#[test]
fn every_bookmark_reports_the_file_it_opens() {
    let items = items();
    let pairs: Vec<(String, Option<String>)> = items
        .iter()
        .map(|it| (it.title.clone(), target_file(it)))
        .collect();

    assert_eq!(
        pairs,
        vec![
            (
                "Chapter 1 - Foundations".to_owned(),
                Some("chapter1.pdf".to_owned())
            ),
            (
                "Chapter 2 - Methods".to_owned(),
                Some("chapter2.pdf".to_owned())
            ),
            (
                "Appendix - Legacy".to_owned(),
                Some("appendix.pdf".to_owned())
            ),
            (
                "Chapter 3 - Results".to_owned(),
                Some("chapter3.pdf".to_owned())
            ),
        ],
        "this is the operator's whole question: bookmark title, and the \
         file it points at"
    );
}

/// The operator's exact shape — an INDIRECT `/F` filespec dictionary, which
/// is what `cpdf` showed him as `<</F 251 0 R/S/Launch>>`.
#[test]
fn an_indirect_file_specification_resolves() {
    let items = items();
    assert_eq!(
        target_file(&items[0]).as_deref(),
        Some("chapter1.pdf"),
        "`251 0 R` must be followed to the /Filespec dictionary and its \
         /UF or /F read — the resolver already existed and simply was not \
         called for /Launch"
    );
}

/// The `/Win` fallback: `/F` absent, a platform dictionary present. PDF 2.0
/// deprecates this, and files that predate 2.0 are exactly the ones a
/// twenty-year-old table of contents is made of.
#[test]
fn a_win_platform_dictionary_is_read_when_f_is_absent() {
    let items = items();
    assert_eq!(
        target_file(&items[2]).as_deref(),
        Some("appendix.pdf"),
        "Table 203 makes /F required UNLESS /Win, /Mac or /Unix is \
         present, so the fallback is not an edge case — it is the other \
         half of the rule"
    );
}

/// A `/Launch` is still **not navigation**: it names a file, not a page of
/// this document, and must not be reported as a page jump.
#[test]
fn a_launch_is_still_not_a_page_destination() {
    let items = items();
    match items[0].destination.as_ref().expect("has a destination") {
        Destination::NonNavigation { action, file } => {
            assert_eq!(
                action.as_ref().map(|a| a.as_bytes().to_vec()),
                Some(b"Launch".to_vec()),
                "the action type stays the disclosure"
            );
            assert!(file.is_some());
        }
        other => panic!("a /Launch must not become a navigation destination: {other:?}"),
    }
    assert_eq!(
        items[0].page_index(),
        None,
        "it names no page IN THIS DOCUMENT, which is exactly what pdftk \
         was right about"
    );
}

/// An action that names no file reports none — so the field's presence
/// means something rather than being decorated with a default.
#[test]
fn an_action_without_a_file_reports_none() {
    // The `/GoToR` in the fixture DOES name one; a `/JavaScript` or `/URI`
    // does not. Built inline rather than added to the shared fixture, to
    // keep that fixture about the one thing it is named for.
    let bytes = br#"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R /Outlines 10 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >> endobj
10 0 obj << /Type /Outlines /First 11 0 R /Last 11 0 R /Count 1 >> endobj
11 0 obj << /Title (Runs a script) /Parent 10 0 R /A 20 0 R >> endobj
20 0 obj << /S /JavaScript /JS (app.alert\(1\);) >> endobj
trailer << /Size 21 /Root 1 0 R >>
"#;
    let doc = Document::from_bytes(bytes.to_vec()).expect("a rebuildable document");
    let session = EditSession::new(doc);
    let outline = read_outline(&session.graph());
    let item = &outline.items[0];
    match item.destination.as_ref().expect("a destination") {
        Destination::NonNavigation { action, file } => {
            assert_eq!(
                action.as_ref().map(|a| a.as_bytes().to_vec()),
                Some(b"JavaScript".to_vec())
            );
            assert!(
                file.is_none(),
                "a /JavaScript action names no file, and reporting one \
                 would be an invention"
            );
        }
        other => panic!("expected NonNavigation, got {other:?}"),
    }
}

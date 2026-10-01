//! Typing a character an embedded TrueType subset already outlines but the
//! document never showed (decision 172, route A).
//!
//! Lives in `pdfcer-render`'s tests because the program reader
//! (`EmbeddedProgramGlyphs`) is this crate's. The fixture is
//! `word-shaped-subset.pdf`: `/F0` shows `ABC` on page 1 and `A` on page 2,
//! `/Widths` covers 65..67, and the program also outlines `D` and U+2013
//! while `E` is an empty slot.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{self, EditOptions, EditRequest};
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::font::embedded_glyphs::EmbeddedProgramGlyphs;
use pdfcer_render::{RenderOptions, render_page_with};

fn fixture_bytes() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/text/word-shaped-subset.pdf"
    ))
    .expect("fixture; run tools/gen-word-subset-fixture.py")
}

fn opts() -> EditOptions {
    EditOptions::default().with_embedded_glyphs(&EmbeddedProgramGlyphs)
}

fn edit(doc: &Document, replace: &str, opts: &EditOptions) -> Result<Vec<u8>, String> {
    text_edit::edit_text(doc, &EditRequest::find_replace(0, "ABC", replace), opts)
        .map(|o| o.bytes)
        .map_err(|e| e.to_string())
}

fn page_text(bytes: &[u8], index: usize) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    pdfcer_core::text_extract::extract_page(&doc, &pages[index], index, &Default::default())
        .unwrap()
        .sourced_text()
}

/// The saved `/Widths` entry for `code`, read from the newest `5 0 obj`.
fn saved_width(bytes: &[u8], code: usize) -> i64 {
    let s = String::from_utf8_lossy(bytes);
    let dict = &s[s.rfind("5 0 obj").expect("font object")..];
    let first: usize = number_after(dict, "/FirstChar");
    let widths = &dict[dict.find("/Widths").unwrap()..];
    let list = &widths[widths.find('[').unwrap() + 1..widths.find(']').unwrap()];
    list.split_whitespace()
        .nth(code - first)
        .unwrap()
        .parse()
        .unwrap()
}

fn number_after<T: std::str::FromStr>(s: &str, key: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    let rest = s[s.find(key).unwrap() + key.len()..].trim_start();
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap();
    rest[..end].parse().unwrap()
}

#[test]
fn an_outlined_unshown_letter_is_typed_and_its_width_comes_from_the_program() {
    let doc = Document::from_bytes(fixture_bytes()).unwrap();
    let out = text_edit::edit_text(&doc, &EditRequest::find_replace(0, "ABC", "ABD"), &opts())
        .expect("D is outlined in the program");
    assert!(page_text(&out.bytes, 0).contains("ABD"));
    // 1343 / 2048 em, in thousandths.
    assert_eq!(saved_width(&out.bytes, 68), 656);
    assert!(
        out.report
            .disclosures
            .iter()
            .any(|d| d.contains("code 68") && d.contains("width 656")),
        "the extension is disclosed: {:?}",
        out.report.disclosures
    );
}

#[test]
fn the_extended_font_draws_the_glyph_and_leaves_page_two_alone() {
    let base = fixture_bytes();
    let saved = edit(
        &Document::from_bytes(base.clone()).unwrap(),
        "AB\u{2013}D",
        &opts(),
    )
    .unwrap();
    assert_eq!(saved_width(&saved, 150), 500);
    assert_eq!(saved_width(&saved, 68), 656);

    let doc = Document::from_bytes(saved).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let r = render_page_with(&doc, &pages[0], 1.0, &RenderOptions::default()).unwrap();
    assert_eq!(
        r.diagnostics.glyphs_notdef, 0,
        "every new code reaches a real glyph"
    );
    assert_eq!(r.diagnostics.glyphs_substituted, 0);

    let before = Document::from_bytes(base).unwrap();
    let render = |d: &Document| {
        let p = page_tree::pages(d).unwrap().remove(1);
        render_page_with(d, &p, 1.0, &RenderOptions::default())
            .unwrap()
            .pixmap
            .data()
            .to_vec()
    };
    assert!(
        render(&doc) == render(&before),
        "page 2 shares /F0 and must render unchanged"
    );
}

#[test]
fn a_space_is_carried_by_its_advance_alone() {
    let doc = Document::from_bytes(fixture_bytes()).unwrap();
    let saved = edit(&doc, "AB D", &opts()).unwrap();
    assert_eq!(saved_width(&saved, 32), 278);
}

#[test]
fn an_empty_slot_is_refused_with_the_reason() {
    let doc = Document::from_bytes(fixture_bytes()).unwrap();
    let err = edit(&doc, "ABE", &opts()).unwrap_err();
    assert!(err.contains("could not be added to the font"), "{err}");
    assert!(err.contains("no outline"), "{err}");
}

#[test]
fn without_a_reader_the_subset_floor_is_unchanged() {
    let doc = Document::from_bytes(fixture_bytes()).unwrap();
    let err = edit(&doc, "ABD", &EditOptions::default()).unwrap_err();
    assert!(!err.contains("could not be added"), "{err}");
}

/// Page 2 is rewritten to show code 68 (`D`) with no `/Widths` entry, so its
/// width is MissingWidth there. Giving 68 a width would move page 2's text.
#[test]
fn a_code_shown_elsewhere_at_another_width_is_refused() {
    let base = fixture_bytes();
    let at = base
        .windows(6)
        .position(|w| w == b"(A) Tj")
        .expect("page 2 run");
    let mut bytes = base.clone();
    bytes[at + 1] = b'D';
    let doc = Document::from_bytes(bytes).unwrap();
    let err = edit(&doc, "ABD", &opts()).unwrap_err();
    assert!(err.contains("could not be added to the font"), "{err}");
}

#[test]
fn a_session_edit_writes_the_font_and_undo_removes_it() {
    let doc = Document::from_bytes(fixture_bytes()).unwrap();
    let before = doc.bytes().to_vec();
    let mut session = EditSession::new(doc);
    session
        .edit_text(&EditRequest::find_replace(0, "ABC", "ABD"), &opts())
        .unwrap();
    let saved = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0;
    assert_eq!(saved_width(&saved, 68), 656);
    assert!(page_text(&saved, 0).contains("ABD"));

    session.undo().expect("one undoable command");
    let reverted = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0;
    assert_eq!(reverted, before, "undo nets to nothing");
}

/// The keystroke query agrees with the edit, character for character.
#[test]
fn the_repertoire_with_a_reader_accepts_exactly_what_the_edit_adds() {
    let session = EditSession::new(Document::from_bytes(fixture_bytes()).unwrap());
    let rep = session
        .run_repertoire_with(0, "ABC", None, &opts())
        .unwrap();
    for ch in ['D', '\u{2013}', ' '] {
        assert!(rep.accepts(ch), "{ch:?} is addable");
    }
    assert!(!rep.accepts('E'), "E has no outline");

    let strict = session.run_repertoire(0, "ABC", None).unwrap();
    assert!(
        !strict.accepts('D'),
        "without a reader the floor is unchanged"
    );
}

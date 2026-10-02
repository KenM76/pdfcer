//! Typing a character an `Identity-H` CIDFontType2 subset outlines but the
//! page never shows (decision 172, route A, composite shape).
//!
//! `cid-shaped-subset.pdf`: CIDs 2..4 (A B C) shown; 5 (D) mapped and
//! outlined; 6 (E) mapped but empty; 8 (U+0394) and 9 (U+0416) outlined but
//! absent from `/ToUnicode`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{self, EditOptions, EditRequest};
use pdfcer_core::text_extract::cmap::ToUnicodeCMap;
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::font::embedded_glyphs::EmbeddedProgramGlyphs;
use pdfcer_render::{RenderOptions, render_page_with};

fn variant(name: &str) -> Document {
    let path = format!(
        "{}/../../fixtures/synthetic/text/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    Document::from_bytes(std::fs::read(path).expect("run tools/gen-cid-subset-fixture.py")).unwrap()
}

fn base() -> Document {
    variant("cid-shaped-subset.pdf")
}

/// `base()` with `from` replaced by the equally long `to`.
fn patched(from: &[u8], to: &[u8]) -> Document {
    assert_eq!(from.len(), to.len());
    let mut bytes = base().bytes().to_vec();
    let at = bytes
        .windows(from.len())
        .position(|w| w == from)
        .expect("patch site");
    bytes[at..at + to.len()].copy_from_slice(to);
    Document::from_bytes(bytes).unwrap()
}

fn opts() -> EditOptions {
    EditOptions::default().with_embedded_glyphs(&EmbeddedProgramGlyphs)
}

fn edit(doc: &Document, replace: &str) -> Result<text_edit::EditOutcome, String> {
    text_edit::edit_text(doc, &EditRequest::find_replace(0, "ABC", replace), &opts())
        .map_err(|e| e.to_string())
}

fn page_text(bytes: &[u8], index: usize) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    pdfcer_core::text_extract::extract_page(&doc, &pages[index], index, &Default::default())
        .unwrap()
        .sourced_text()
}

/// The newest revision of object `id`, as text.
fn newest(bytes: &[u8], id: u32) -> String {
    let s = String::from_utf8_lossy(bytes);
    let at = s.rfind(&format!("\n{id} 0 obj")).expect("object");
    let end = at + s[at..].find("endobj").unwrap();
    s[at..end].split_whitespace().collect::<Vec<_>>().join(" ")
}

fn saved_map(bytes: &[u8]) -> ToUnicodeCMap {
    let s = String::from_utf8_lossy(bytes);
    let body = &s[s.rfind("\n9 0 obj").unwrap()..];
    let start = body.find("stream").unwrap() + 6;
    let end = body.find("endstream").unwrap();
    ToUnicodeCMap::parse(&body.as_bytes()[start..end])
}

fn disclosed(out: &text_edit::EditOutcome, needle: &str) -> bool {
    out.report.disclosures.iter().any(|d| d.contains(needle))
}

#[test]
fn a_mapped_unshown_cid_gains_its_width() {
    let out = edit(&base(), "ABD").expect("D is outlined");
    // 1343 / 2048 em, in thousandths.
    assert!(
        newest(&out.bytes, 6).contains("/W [2 [667 600 722] 5 [656]]"),
        "{}",
        newest(&out.bytes, 6)
    );
    assert!(page_text(&out.bytes, 0).contains("ABD"));
    assert!(
        disclosed(&out, "typed as CID 5") && disclosed(&out, "the CID /ToUnicode already gave it"),
        "{:?}",
        out.report.disclosures
    );
    assert!(
        !out.bytes[base().bytes().len()..]
            .windows(8)
            .any(|w| w == b"\n9 0 obj")
    );
}

#[test]
fn an_unmapped_glyph_gets_its_glyph_id_as_cid_and_a_map_entry() {
    let out = edit(&base(), "AB\u{394}").expect("Delta is outlined");
    assert!(newest(&out.bytes, 6).contains("8 [612]"));
    assert_eq!(saved_map(&out.bytes).lookup(8).as_deref(), Some("\u{394}"));
    assert!(page_text(&out.bytes, 0).contains("AB\u{394}"));
    assert!(disclosed(&out, "found through the program's cmap"));
    assert!(
        newest(&out.bytes, 5) == newest(base().bytes(), 5),
        "the Type0 dictionary is untouched"
    );
}

#[test]
fn the_new_cids_draw_and_page_two_is_unchanged() {
    let before = base();
    let out = edit(&before, "D\u{394}\u{416}").unwrap();
    let doc = Document::from_bytes(out.bytes).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let r = render_page_with(&doc, &pages[0], 1.0, &RenderOptions::default()).unwrap();
    assert_eq!(r.diagnostics.glyphs_notdef, 0);
    assert_eq!(r.diagnostics.glyphs_substituted, 0);
    let render = |d: &Document| {
        let p = page_tree::pages(d).unwrap().remove(1);
        render_page_with(d, &p, 1.0, &RenderOptions::default())
            .unwrap()
            .pixmap
            .data()
            .to_vec()
    };
    assert!(render(&doc) == render(&before), "page 2 shares /F0");
}

#[test]
fn an_empty_slot_and_an_absent_glyph_are_refused_with_the_reason() {
    let err = edit(&base(), "ABE").unwrap_err();
    assert!(
        err.contains("the embedded program has no outline for it"),
        "{err}"
    );
    let err = edit(&base(), "AB\u{3A9}").unwrap_err();
    assert!(
        err.contains("could not be given an unused code")
            && err.contains("the embedded program has no outline for it"),
        "{err}"
    );
}

#[test]
fn a_shared_descendant_refuses_a_width_in_the_edit_and_the_query() {
    let doc = variant("cid-shaped-subset-shared-descendant.pdf");
    let err = edit(&doc, "ABD").unwrap_err();
    assert!(err.contains("descendant CIDFont may be shared"), "{err}");
    let rep = EditSession::new(doc)
        .run_repertoire_with(0, "ABC", None, &opts())
        .unwrap();
    assert!(!rep.accepts('D') && !rep.accepts('\u{394}'));
}

#[test]
fn a_cid_w_already_lists_at_another_width_is_refused() {
    let doc = patched(b"/W [2 [667 600 722]]", b"/W [2 3 667 5 5 999]");
    let err = edit(&doc, "ABD").unwrap_err();
    assert!(err.contains("/W already gives CID 5 width 999"), "{err}");
}

#[test]
fn a_cid_shown_without_a_map_entry_is_not_mapped() {
    let doc = patched(b"<0002> Tj", b"<0008> Tj");
    let err = edit(&doc, "AB\u{394}").unwrap_err();
    assert!(
        err.contains("CID 8 is already shown elsewhere without a /ToUnicode entry"),
        "{err}"
    );
}

#[test]
fn a_cid_shown_at_the_default_width_is_not_widened() {
    let doc = patched(b"<0002> Tj", b"<0005> Tj");
    let err = edit(&doc, "ABD").unwrap_err();
    assert!(
        err.contains("code 5 is already shown elsewhere in the document with a different width"),
        "{err}"
    );
}

#[test]
fn a_cid_the_map_gives_another_character_is_not_reused() {
    let doc = patched(b"<0006> <0045>", b"<0008> <0045>");
    let err = edit(&doc, "AB\u{394}").unwrap_err();
    assert!(err.contains("code 8 already reads as \"E\""), "{err}");
}

#[test]
fn the_repertoire_accepts_exactly_what_the_edit_adds() {
    let session = EditSession::new(base());
    let rep = session
        .run_repertoire_with(0, "ABC", None, &opts())
        .unwrap();
    for ch in ['D', '\u{394}', '\u{416}'] {
        assert!(rep.accepts(ch), "{ch}");
    }
    assert!(!rep.accepts('E') && !rep.accepts('\u{3A9}'));
    let strict = session.run_repertoire(0, "ABC", None).unwrap();
    assert!(!strict.accepts('D') && !strict.accepts('\u{394}'));
}

#[test]
fn without_a_reader_the_refusal_is_unchanged() {
    let err = text_edit::edit_text(
        &base(),
        &EditRequest::find_replace(0, "ABC", "AB\u{394}"),
        &EditOptions::default(),
    )
    .unwrap_err()
    .to_string();
    assert!(!err.contains("unused code"), "{err}");
}

#[test]
fn a_session_extension_undoes_to_the_base_bytes() {
    let doc = base();
    let before = doc.bytes().to_vec();
    let mut session = EditSession::new(doc);
    session
        .edit_text(&EditRequest::find_replace(0, "ABC", "AB\u{394}"), &opts())
        .unwrap();
    let saved = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0;
    assert!(newest(&saved, 6).contains("8 [612]"));
    session.undo().expect("one undoable command");
    let reverted = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0;
    assert_eq!(reverted, before, "undo nets to nothing");
}

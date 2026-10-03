//! Typing a character a TrueType subset outlines but its encoding has no
//! code for (decision 172, route A, `/Differences` allocation).
//!
//! `word-shaped-subset.pdf`'s program outlines U+0394 and U+0416, which
//! `WinAnsiEncoding` cannot address; the edit names an unused code after each.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{self, EditOptions, EditRequest};
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::font::embedded_glyphs::EmbeddedProgramGlyphs;
use pdfcer_render::{RenderOptions, render_page_with};

fn variant(name: &str) -> Document {
    let path = format!(
        "{}/../../fixtures/synthetic/text/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    Document::from_bytes(std::fs::read(path).expect("run tools/gen-word-subset-fixture.py"))
        .unwrap()
}

fn base() -> Document {
    variant("word-shaped-subset.pdf")
}

/// Route A alone: route B (`same_program_route.rs`) would set what its
/// guards refuse.
fn opts() -> EditOptions {
    EditOptions::default()
        .with_embedded_glyphs(&EmbeddedProgramGlyphs)
        .with_cid_font_program(text_edit::CidFontProgram::Off)
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

/// The newest `5 0 obj` (the font dictionary), as text.
fn saved_font(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let at = s.rfind("5 0 obj").expect("font object");
    let end = at + s[at..].find("endobj").unwrap();
    s[at..end].to_owned()
}

fn saved_width(font: &str, code: usize) -> i64 {
    let first: usize = number_after(font, "/FirstChar");
    let widths = &font[font.find("/Widths").unwrap()..];
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

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn a_character_the_encoding_cannot_address_gets_an_unused_code() {
    let out = edit(&base(), "AB\u{394}").expect("Delta is outlined");
    let font = saved_font(&out.bytes);
    assert!(
        squash(&font).contains("/Differences [127 /uni0394]"),
        "{font}"
    );
    assert!(squash(&font).contains("/BaseEncoding /WinAnsiEncoding"));
    // 1253 / 2048 em, in thousandths.
    assert_eq!(saved_width(&font, 127), 612);
    assert!(page_text(&out.bytes, 0).contains("AB\u{394}"));
    assert!(
        out.report
            .disclosures
            .iter()
            .any(|d| d.contains("unused code 127 was named /uni0394")),
        "{:?}",
        out.report.disclosures
    );
}

#[test]
fn the_allocated_code_draws_the_glyph_and_page_two_is_unchanged() {
    let before = base();
    let out = edit(&before, "AB\u{394}\u{416}").unwrap();
    let doc = Document::from_bytes(out.bytes).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let r = render_page_with(&doc, &pages[0], 1.0, &RenderOptions::default()).unwrap();
    assert_eq!(r.diagnostics.glyphs_notdef, 0, "both codes reach a glyph");
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
fn two_characters_take_the_first_two_free_codes() {
    let out = edit(&base(), "A\u{394}\u{416}").unwrap();
    let font = squash(&saved_font(&out.bytes));
    // 128 is the euro sign, so 129 is the next undefined WinAnsi code.
    assert!(
        font.contains("/Differences [127 /uni0394 129 /uni0416]"),
        "{font}"
    );
    assert!(page_text(&out.bytes, 0).contains("A\u{394}\u{416}"));
}

#[test]
fn a_code_already_in_differences_is_not_reused() {
    let out = edit(&variant("word-shaped-subset-differences.pdf"), "AB\u{394}").unwrap();
    let font = squash(&saved_font(&out.bytes));
    assert!(
        font.contains("/Differences [127 /uni2126 129 /uni0394]"),
        "{font}"
    );
}

#[test]
fn a_tounicode_map_gains_the_allocated_code() {
    let out = edit(&variant("word-shaped-subset-tounicode.pdf"), "AB\u{394}").unwrap();
    assert!(page_text(&out.bytes, 0).contains("AB\u{394}"));
    let s = String::from_utf8_lossy(&out.bytes);
    let at = s.rfind("10 0 obj").expect("map object");
    let body = &s[at..];
    let start = body.find("stream").unwrap() + 6;
    let end = body.find("endstream").unwrap();
    let map = pdfcer_core::text_extract::cmap::ToUnicodeCMap::parse(&body.as_bytes()[start..end]);
    assert_eq!(map.lookup(0x7F).as_deref(), Some("\u{394}"));
}

#[test]
fn a_code_the_map_already_defines_is_not_reused() {
    let mut bytes = variant("word-shaped-subset-tounicode.pdf").bytes().to_vec();
    let at = bytes
        .windows(16)
        .position(|w| w == b"<41> <43> <0041>")
        .expect("range");
    bytes[at..at + 16].copy_from_slice(b"<7F> <7F> <0058>");
    let out = edit(&Document::from_bytes(bytes).unwrap(), "AB\u{394}").unwrap();
    let font = squash(&saved_font(&out.bytes));
    assert!(font.contains("/Differences [129 /uni0394]"), "{font}");
}

/// The new code's width is already right, so only the encoding changes.
#[test]
fn an_allocation_needing_no_width_is_still_written() {
    let mut bytes = base().bytes().to_vec();
    let from: &[u8] = b"/CapHeight 716 /StemV 80";
    let at = bytes
        .windows(24)
        .position(|w| w == from)
        .expect("descriptor");
    bytes[at..at + 24].copy_from_slice(b"/MissingWidth 612 /S 80 ");
    let out = edit(&Document::from_bytes(bytes).unwrap(), "AB\u{394}").unwrap();
    let font = squash(&saved_font(&out.bytes));
    assert!(font.contains("/Differences [127 /uni0394]"), "{font}");
    assert!(font.contains("/LastChar 67"), "no width written: {font}");
}

#[test]
fn a_shared_map_refuses_allocation_in_the_edit_and_the_query() {
    let doc = variant("word-shaped-subset-shared-tounicode.pdf");
    let err = edit(&doc, "AB\u{394}").unwrap_err();
    assert!(err.contains("may be shared with another font"), "{err}");
    let rep = EditSession::new(doc)
        .run_repertoire_with(0, "ABC", None, &opts())
        .unwrap();
    assert!(!rep.accepts('\u{394}'));
}

#[test]
fn the_repertoire_accepts_exactly_what_allocation_adds() {
    let session = EditSession::new(base());
    let rep = session
        .run_repertoire_with(0, "ABC", None, &opts())
        .unwrap();
    assert!(rep.accepts('\u{394}') && rep.accepts('\u{416}'));
    assert!(!rep.accepts('\u{3A9}'), "Omega is not in the program");
    let strict = session.run_repertoire(0, "ABC", None).unwrap();
    assert!(!strict.accepts('\u{394}'), "no reader, no allocation");
}

#[test]
fn without_a_reader_the_refusal_is_unchanged() {
    let doc = base();
    let err = text_edit::edit_text(
        &doc,
        &EditRequest::find_replace(0, "ABC", "AB\u{394}"),
        &EditOptions::default(),
    )
    .unwrap_err()
    .to_string();
    assert!(!err.contains("unused code"), "{err}");
}

#[test]
fn a_character_the_program_lacks_is_refused_with_the_reason() {
    let err = edit(&base(), "AB\u{3A9}").unwrap_err();
    assert!(
        err.contains("could not be given an unused code")
            && err.contains("the embedded program has no outline for it"),
        "{err}"
    );
}

#[test]
fn a_session_allocation_undoes_to_the_base_bytes() {
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
    assert!(squash(&saved_font(&saved)).contains("127 /uni0394"));
    session.undo().expect("one undoable command");
    let reverted = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0;
    assert_eq!(reverted, before, "undo nets to nothing");
}

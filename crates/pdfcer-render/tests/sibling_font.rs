//! Setting a character the run's font cannot carry in a same-face sibling
//! font resource on the page (decision 174). `sibling-font.pdf`: `/F0` is a
//! one-glyph `Identity-H` subset showing "AA"; `/F1` is a WinAnsi TrueType
//! subset of the same face showing "ABC". `-other-face` renames `/F1`'s face.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{self, EditOptions, EditRequest, FollowerDisposition};
use pdfcer_render::{RenderOptions, render_page_with};

fn variant(name: &str) -> Document {
    let path = format!(
        "{}/../../fixtures/synthetic/text/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    Document::from_bytes(std::fs::read(path).expect("run tools/gen-sibling-font-fixture.py"))
        .unwrap()
}

fn on() -> EditOptions {
    EditOptions::default().with_sibling_fonts(true)
}

fn edit_find(
    doc: &Document,
    find: &str,
    replace: &str,
    opts: &EditOptions,
) -> Result<text_edit::EditOutcome, String> {
    text_edit::edit_text(doc, &EditRequest::find_replace(0, find, replace), opts)
        .map_err(|e| e.to_string())
}

fn edit(
    doc: &Document,
    replace: &str,
    opts: &EditOptions,
) -> Result<text_edit::EditOutcome, String> {
    edit_find(doc, "AA", replace, opts)
}

fn page_text(bytes: &[u8]) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    pdfcer_core::text_extract::extract_page(&doc, &pages[0], 0, &Default::default())
        .unwrap()
        .sourced_text()
}

/// The page's content stream (object 4, unfiltered), newest revision,
/// whitespace-normalised.
fn content(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let at = s.rfind("\n4 0 obj").expect("content object");
    let end = at + s[at..].find("endobj").unwrap();
    s[at..end].split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first show operator's byte span, the way a shell obtains one.
fn first_operator_span(doc: &Document) -> pdfcer_core::span::ByteSpan {
    let pages = page_tree::pages(doc).unwrap();
    let opts = pdfcer_core::text_extract::ExtractOptions::default().with_provenance(true);
    let page = pdfcer_core::text_extract::extract_page(doc, &pages[0], 0, &opts).unwrap();
    page.runs
        .iter()
        .flat_map(|r| r.glyphs.iter())
        .find_map(|g| g.provenance.as_ref().map(|p| p.operator_span))
        .unwrap()
}

#[test]
fn without_the_option_the_run_refuses() {
    assert!(edit(&variant("sibling-font.pdf"), "AB", &EditOptions::default()).is_err());
}

#[test]
fn the_replacement_switches_to_the_sibling_and_back() {
    let base = variant("sibling-font.pdf");
    let out = edit(&base, "AB", &on()).unwrap();
    let c = content(&out.bytes);
    assert!(
        c.contains("/F0 24 Tf 72 600 Td /F1 24 Tf (AB) Tj /F0 24 Tf ET"),
        "the whole match moves to the sibling, then the run's font is restored: {c}"
    );
    let text = page_text(&out.bytes);
    assert!(text.contains("AB"), "{text}");
    assert!(
        out.report
            .disclosures
            .iter()
            .any(|d| d.contains("SIBBBB+pdfceSib") && d.contains("/F1")),
        "{:?}",
        out.report.disclosures
    );
    let appended = String::from_utf8_lossy(&out.bytes[base.bytes().len()..]);
    assert!(
        !appended.contains("/Type /Font"),
        "no font object is rewritten"
    );
}

#[test]
fn text_beside_the_match_stays_in_the_runs_font() {
    let out = edit_find(&variant("sibling-font.pdf"), "A", "B", &on()).unwrap();
    let c = content(&out.bytes);
    assert!(
        c.contains("72 600 Td /F1 24 Tf (B) Tj /F0 24 Tf (\\000\\002) Tj ET"),
        "the second A is still shown by /F0: {c}"
    );
    assert!(page_text(&out.bytes).contains("BA"));
}

#[test]
fn a_pinned_follower_gets_its_compensation_after_the_restore() {
    let opts = on().with_disposition(FollowerDisposition::Pin);
    let out = edit(&variant("sibling-font.pdf"), "AB", &opts).unwrap();
    let c = content(&out.bytes);
    // A is 667 wide, B 600: the restored font carries the 67-unit difference.
    assert!(
        c.contains("/F1 24 Tf (AB) Tj /F0 24 Tf [-67] TJ ET"),
        "the pin follows the restore: {c}"
    );
}

#[test]
fn a_font_of_another_face_is_not_a_sibling() {
    assert!(edit(&variant("sibling-font-other-face.pdf"), "AB", &on()).is_err());
}

#[test]
fn the_edited_page_renders() {
    let out = edit(&variant("sibling-font.pdf"), "AB", &on()).unwrap();
    let doc = Document::from_bytes(out.bytes).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    assert!(render_page_with(&doc, &pages[0], 1.0, &RenderOptions::default()).is_ok());
}

#[test]
fn the_preview_lays_out_the_replacement_in_the_sibling() {
    use pdfcer_render::edit_preview::preview_outlines;
    let session = EditSession::new(variant("sibling-font.pdf"));
    let preview = session
        .edit_text_preview(&EditRequest::find_replace(0, "AA", "AB"), &on())
        .unwrap();
    assert_eq!(preview.base_font, "SIBBBB+pdfceSib");
    assert_eq!(preview.font_resource, b"F1");
    let out = preview_outlines(
        &session.view(),
        &preview,
        &pdfcer_render::FontEnvironment::bundled(),
    );
    assert!(out.glyphs.iter().all(Option::is_some), "B has an outline");
}

#[test]
fn the_repertoire_adds_what_a_sibling_carries() {
    let session = EditSession::new(variant("sibling-font.pdf"));
    let strict = session.run_repertoire(0, "AA", None).unwrap();
    assert_eq!(strict.accepted.iter().collect::<String>(), "A");
    let wide = session.run_repertoire_with(0, "AA", None, &on()).unwrap();
    assert_eq!(wide.accepted.iter().collect::<String>(), "ABC");
    assert_eq!(
        wide.base_font, "SIBAAA+pdfceSib",
        "the run's own font is reported"
    );
    let pinned = session
        .run_repertoire_with(
            0,
            "",
            Some(first_operator_span(&variant("sibling-font.pdf"))),
            &on(),
        )
        .unwrap();
    assert_eq!(
        pinned.accepted.iter().collect::<String>(),
        "ABC",
        "the caret's query (empty find, pinned operator) sees the sibling too"
    );
    let other = EditSession::new(variant("sibling-font-other-face.pdf"));
    let none = other.run_repertoire_with(0, "AA", None, &on()).unwrap();
    assert_eq!(none.accepted.iter().collect::<String>(), "A");
}

//! A glyph only the program's `post` table names (ISO 32000-2 §9.6.6.4's last
//! resort) is typeable, and the edit says it was an inference.
//!
//! The `-post-names` fixtures' program has outlines for `D` and U+0394 but no
//! `(3,1)` cmap entry for either; `post` names them `D` and `uni0394`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{self, EditOptions, EditRequest};
use pdfcer_render::font::embedded_glyphs::EmbeddedProgramGlyphs;
use pdfcer_render::{RenderOptions, render_page_with};

fn variant(name: &str) -> Document {
    let path = format!(
        "{}/../../fixtures/synthetic/text/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    Document::from_bytes(std::fs::read(path).expect("run the subset fixture generators")).unwrap()
}

fn simple() -> Document {
    variant("word-shaped-subset-post-names.pdf")
}

fn composite() -> Document {
    variant("cid-shaped-subset-post-names.pdf")
}

fn opts() -> EditOptions {
    EditOptions::default().with_embedded_glyphs(&EmbeddedProgramGlyphs)
}

fn edit(doc: &Document, replace: &str) -> text_edit::EditOutcome {
    text_edit::edit_text(doc, &EditRequest::find_replace(0, "ABC", replace), &opts())
        .unwrap_or_else(|e| panic!("{e}"))
}

fn page_text(bytes: &[u8]) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    pdfcer_core::text_extract::extract_page(&doc, &pages[0], 0, &Default::default())
        .unwrap()
        .sourced_text()
}

fn notdef(bytes: &[u8]) -> usize {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    render_page_with(&doc, &pages[0], 1.0, &RenderOptions::default())
        .unwrap()
        .diagnostics
        .glyphs_notdef
}

fn inferred(out: &text_edit::EditOutcome, name: &str) -> bool {
    out.report.disclosures.iter().any(|d| {
        d.contains(&format!(
            "found by the program's glyph name /{name}, an inference"
        ))
    })
}

#[test]
fn a_code_whose_name_only_post_reaches_is_typed_and_labelled() {
    let out = edit(&simple(), "ABD");
    assert!(page_text(&out.bytes).contains("ABD"));
    assert_eq!(notdef(&out.bytes), 0);
    assert!(inferred(&out, "D"), "{:?}", out.report.disclosures);
}

#[test]
fn an_allocated_code_is_named_with_the_post_name() {
    let out = edit(&simple(), "AB\u{394}");
    assert!(page_text(&out.bytes).contains("AB\u{394}"));
    assert_eq!(notdef(&out.bytes), 0, "a viewer's post lookup reaches it");
    assert!(
        String::from_utf8_lossy(&out.bytes).contains("/uni0394"),
        "the /Differences name is the program's own"
    );
    assert!(inferred(&out, "uni0394"), "{:?}", out.report.disclosures);
}

#[test]
fn a_cmap_match_is_not_labelled_an_inference() {
    let out = edit(&variant("word-shaped-subset.pdf"), "ABD");
    assert!(
        !out.report
            .disclosures
            .iter()
            .any(|d| d.contains("inference")),
        "{:?}",
        out.report.disclosures
    );
}

#[test]
fn a_composite_cid_found_by_name_gets_a_map_entry() {
    let out = edit(&composite(), "AB\u{394}");
    assert!(page_text(&out.bytes).contains("AB\u{394}"));
    assert_eq!(notdef(&out.bytes), 0);
    assert!(inferred(&out, "uni0394"), "{:?}", out.report.disclosures);
}

#[test]
fn a_composite_cid_the_map_names_is_not_an_inference() {
    let out = edit(&composite(), "ABD");
    assert!(
        !out.report
            .disclosures
            .iter()
            .any(|d| d.contains("inference")),
        "{:?}",
        out.report.disclosures
    );
}

#[test]
fn the_repertoire_offers_post_named_glyphs_only_with_the_reader() {
    for doc in [simple(), composite()] {
        let session = EditSession::new(doc);
        let rep = session
            .run_repertoire_with(0, "ABC", None, &opts())
            .unwrap();
        assert!(rep.accepts('D') && rep.accepts('\u{394}'));
        assert!(!rep.accepts('E'));
        let strict = session.run_repertoire(0, "ABC", None).unwrap();
        assert!(!strict.accepts('\u{394}'));
    }
}

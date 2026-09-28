//! `/RC` rich text and `/DS` default style on the annotation read model
//! (ISO 32000-1 §12.5.6.2 Table 170, §12.7.3.4, §7.9.3).

use pdfcer_core::annot::{Annotation, RichText, page_annotations, rich_text_in};
use pdfcer_core::document::Document;
use pdfcer_core::page_tree::pages;
use pdfcer_core::view::StreamSource;
use std::path::Path;

fn load(name: &str) -> Document {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/annot")
        .join(name);
    Document::load(&path).expect("load fixture")
}

fn first_annot(doc: &Document) -> Annotation {
    let page = pages(doc).expect("pages").remove(0);
    page_annotations(doc, page.id).remove(0)
}

#[test]
fn an_inline_rc_is_read_and_differs_from_contents() {
    let doc = load("rich-text-square.pdf");
    let a = first_annot(&doc);
    assert_eq!(a.contents.as_deref(), Some("the plain words"));
    assert_eq!(
        a.rich_contents,
        Some(RichText::Inline(
            "<?xml version=\"1.0\"?><body><p>THE RICH WORDS</p></body>".to_owned()
        ))
    );
    assert_eq!(a.default_style, None);
}

#[test]
fn ds_is_read_outside_free_text() {
    let doc = load("rich-text-square-stray-ds.pdf");
    let a = first_annot(&doc);
    assert_eq!(a.subtype, b"Square");
    assert_eq!(a.default_style.as_deref(), Some("font: 12pt Helvetica"));
}

#[test]
fn ds_is_read_on_free_text() {
    let doc = load("rich-text-freetext.pdf");
    let a = first_annot(&doc);
    assert_eq!(a.default_style.as_deref(), Some("font: 12pt Helvetica"));
    assert!(matches!(a.rich_contents, Some(RichText::Inline(_))));
}

#[test]
fn a_stream_rc_is_decoded_through_its_filter_and_text_encoding() {
    let doc = load("rich-text-stream.pdf");
    let a = first_annot(&doc);
    let rich = a.rich_contents.clone().expect("RC present");
    assert!(matches!(rich, RichText::Stream(id) if id.num == 6));
    let text = rich_text_in(&doc, StreamSource::Contiguous(doc.bytes()), &rich);
    assert_eq!(
        text.as_deref(),
        Some("<?xml version=\"1.0\"?><body><p>STREAMED RICH WORDS \u{e9}</p></body>")
    );
}

#[test]
fn an_annotation_without_rc_or_ds_reports_none() {
    let doc = load("no-ap-circle.pdf");
    let a = first_annot(&doc);
    assert_eq!(a.rich_contents, None);
    assert_eq!(a.default_style, None);
}

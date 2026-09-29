//! A stream that is both an annotation's `/AP` `/N` and page content must not
//! be rewritten in place when the annotation's appearance is rebuilt: the
//! rebuild allocates a fresh stream and the page's own drawing survives
//! byte-identical (ARCHITECTURE §5). Covers a page's `/Contents` and its own
//! `/Resources` `/XObject`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot_author::{Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, ResizeOptions, ResizedAppearance};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

const PAGE: &str = "/Type /Page /Parent 2 0 R /MediaBox [0 0 400 400]";

fn base_pdf() -> Vec<u8> {
    let content = b"0 0 m 10 10 l S";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!("<< {PAGE} /Resources << >> /Contents 4 0 R >>"),
        format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            String::from_utf8_lossy(content)
        ),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// The last number following `key` in `bytes`.
fn last_number_after(bytes: &[u8], key: &str) -> usize {
    let text = String::from_utf8_lossy(bytes);
    let at = text.rfind(key).expect("key present") + key.len();
    text[at..]
        .trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|n| n.parse().ok())
        .expect("a number follows")
}

fn ap_of(doc: &Document, annot: ObjId) -> ObjId {
    let Object::Dict(d) = &doc.get(annot).expect("annot").value else {
        panic!("annot is a dict");
    };
    let Some(Object::Dict(ap)) = d.get(b"AP").map(|o| doc.view().resolve(o).clone()) else {
        panic!("annot has /AP");
    };
    ap.get(b"N")
        .and_then(Object::as_reference)
        .expect("/N is indirect")
}

fn raw_stream(doc: &Document, id: ObjId) -> Vec<u8> {
    let Object::Stream(s) = &doc.get(id).expect("stream").value else {
        panic!("{id} is a stream");
    };
    s.data_span.slice(doc.bytes()).expect("in buffer").to_vec()
}

/// Author a square, then append a hand-written revision in which page 3
/// reaches the square's appearance stream through `page_tail`.
fn aliased(page_tail: impl Fn(ObjId) -> String) -> (EditSession, ObjId, ObjId) {
    let mut s = EditSession::new(Document::from_bytes(base_pdf()).expect("base parses"));
    let annot = s
        .add_markup(
            0,
            &MarkupSpec::Square {
                rect: Rect::from_corners(100.0, 100.0, 200.0, 160.0),
                border: Some(Color::Gray(0.0)),
                interior: None,
                border_width: 3.0,
                border_effect: None,
            },
        )
        .expect("author the square");
    let (mut bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let ap = ap_of(
        &Document::from_bytes(bytes.clone()).expect("reparse"),
        annot,
    );

    let size = last_number_after(&bytes, "/Size");
    let prev = last_number_after(&bytes, "startxref");
    let page_at = bytes.len();
    bytes.extend_from_slice(
        format!(
            "3 0 obj\n<< {PAGE} /Annots [{} 0 R] {} >>\nendobj\n",
            annot.num,
            page_tail(ap)
        )
        .as_bytes(),
    );
    let xref_at = bytes.len();
    bytes.extend_from_slice(
        format!(
            "xref\n3 1\n{page_at:010} 00000 n \ntrailer\n<< /Size {size} /Root 1 0 R /Prev {prev} >>\nstartxref\n{xref_at}\n%%EOF\n"
        )
        .as_bytes(),
    );
    let s = EditSession::new(Document::from_bytes(bytes).expect("aliased file parses"));
    (s, annot, ap)
}

fn resize_keeps_page_drawing(s: &mut EditSession, annot: ObjId, ap: ObjId) {
    let before = raw_stream(s.document(), ap);
    let out = s
        .resize_annotation(annot, (100.0, 100.0), 2.0, 2.0, &ResizeOptions::default())
        .expect("pdfcer's own square resizes");
    assert_eq!(out.appearance, ResizedAppearance::Rebuilt);

    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let doc = Document::from_bytes(bytes).expect("reparse");
    assert_ne!(
        ap_of(&doc, annot),
        ap,
        "the rebuilt appearance must go to a fresh stream"
    );
    assert_eq!(
        raw_stream(&doc, ap),
        before,
        "the stream the page draws must be byte-identical"
    );
}

#[test]
fn an_appearance_that_is_the_page_contents_is_copied_not_overwritten() {
    let (mut s, annot, ap) = aliased(|ap| format!("/Resources << >> /Contents {} 0 R", ap.num));
    resize_keeps_page_drawing(&mut s, annot, ap);
}

#[test]
fn an_appearance_in_a_contents_array_is_copied_not_overwritten() {
    let (mut s, annot, ap) =
        aliased(|ap| format!("/Resources << >> /Contents [4 0 R {} 0 R]", ap.num));
    resize_keeps_page_drawing(&mut s, annot, ap);
}

#[test]
fn an_appearance_that_is_a_page_xobject_is_copied_not_overwritten() {
    let (mut s, annot, ap) = aliased(|ap| {
        format!(
            "/Resources << /XObject << /Fm0 {} 0 R >> >> /Contents 4 0 R",
            ap.num
        )
    });
    resize_keeps_page_drawing(&mut s, annot, ap);
}

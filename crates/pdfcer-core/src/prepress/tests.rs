#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use crate::document::Document;
use crate::optimize::tests::{build, id};
use crate::space_audit::tests::stream;

const OP: &str = "<< /Type /ExtGState /OP true >>";

/// 1 catalog, 2 pages, 3 page with `page_extra` drawing `content` (4);
/// `extra` appended from 5.
fn doc(page_extra: &str, content: &[u8], extra: &[&str]) -> Document {
    let mut bodies = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R {page_extra} >>"
        )
        .into_bytes(),
        stream("", content),
    ];
    bodies.extend(extra.iter().map(|e| e.as_bytes().to_vec()));
    Document::from_bytes(build(&bodies)).unwrap()
}

fn form(res: &str, content: &str) -> String {
    String::from_utf8(stream(
        &format!("/Type /XObject /Subtype /Form /BBox [0 0 9 9] {res}"),
        content.as_bytes(),
    ))
    .unwrap()
}

fn found(d: &Document) -> OverprintUse {
    overprint_use(&d.view())
}

#[test]
fn a_page_gs_with_op_true_asks_and_names_the_page() {
    let d = doc(
        "/Resources << /ExtGState << /G 5 0 R >> >>",
        b"/G gs 0 0 9 9 re f",
        &[OP],
    );
    let f = found(&d);
    assert_eq!(f.first, Some(id(3)));
    assert!(f.asks() && f.is_complete());
}

#[test]
fn lowercase_op_alone_asks() {
    let d = doc(
        "/Resources << /ExtGState << /G 5 0 R >> >>",
        b"/G gs",
        &["<< /op true >>"],
    );
    assert!(found(&d).asks());
}

#[test]
fn a_listed_but_never_invoked_gs_or_a_false_flag_does_not_ask() {
    let d = doc(
        "/Resources << /ExtGState << /G 5 0 R /N 6 0 R >> >>",
        b"/N gs 0 0 9 9 re f",
        &[OP, "<< /OP false /op false >>"],
    );
    let f = found(&d);
    assert!(!f.asks() && f.is_complete(), "{f:?}");
}

#[test]
fn a_form_inheriting_the_page_resources_asks_and_names_the_form() {
    let d = doc(
        "/Resources << /ExtGState << /G 5 0 R >> /XObject << /Fm 6 0 R >> >>",
        b"/Fm Do",
        &[OP, &form("", "/G gs")],
    );
    assert_eq!(found(&d).first, Some(id(6)));
}

#[test]
fn an_appearance_state_asks() {
    let annot = "<< /Type /Annot /Subtype /Widget /Rect [0 0 9 9] /AP << /N << /On 6 0 R /Off 7 0 R >> >> >>";
    let on = form("/Resources << /ExtGState << /G 8 0 R >> >>", "/G gs");
    let d = doc("/Annots [5 0 R]", b"", &[annot, &form("", ""), &on, OP]);
    assert_eq!(found(&d).first, Some(id(7)));
}

#[test]
fn a_type3_glyph_procedure_asks() {
    let font = "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] \
        /CharProcs << /a 6 0 R >> /Encoding << /Differences [97 /a] >> /FirstChar 97 /LastChar 97 \
        /Widths [1] /Resources << /ExtGState << /G 7 0 R >> >> >>";
    let proc_ = String::from_utf8(stream("", b"/G gs 0 0 1 1 re f")).unwrap();
    let d = doc(
        "/Resources << /Font << /T 5 0 R >> >>",
        b"BT /T 9 Tf (a) Tj ET",
        &[font, &proc_, OP],
    );
    assert_eq!(found(&d).first, Some(id(6)));
}

#[test]
fn a_shading_patterns_own_graphics_state_asks() {
    let pattern = "<< /PatternType 2 /Shading << /ShadingType 2 /ColorSpace /DeviceGray \
        /Coords [0 0 1 0] /Function << /FunctionType 2 /Domain [0 1] /N 1 >> >> /ExtGState 6 0 R >>";
    let d = doc(
        "/Resources << /Pattern << /P 5 0 R >> >>",
        b"/Pattern cs /P scn 0 0 9 9 re f",
        &[pattern, OP],
    );
    assert_eq!(found(&d).first, Some(id(3)));
}

#[test]
fn a_tiling_pattern_and_a_soft_mask_group_are_walked() {
    let tiling = String::from_utf8(stream(
        "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 9 9] /XStep 9 /YStep 9 \
         /Resources << /ExtGState << /G 7 0 R >> >>",
        b"/G gs",
    ))
    .unwrap();
    let d = doc(
        "/Resources << /Pattern << /P 5 0 R >> >>",
        b"/Pattern cs /P scn",
        &[&tiling, "<< >>", OP],
    );
    assert_eq!(found(&d).first, Some(id(5)));

    let mask = "<< /Type /ExtGState /SMask << /S /Luminosity /G 6 0 R >> >>";
    let group = form(
        "/Group << /S /Transparency >> /Resources << /ExtGState << /G 7 0 R >> >>",
        "/G gs",
    );
    let d = doc(
        "/Resources << /ExtGState << /M 5 0 R >> >>",
        b"/M gs",
        &[mask, &group, OP],
    );
    assert_eq!(found(&d).first, Some(id(6)));
}

#[test]
fn an_unreadable_stream_makes_a_no_incomplete() {
    let broken = "<< /Type /XObject /Subtype /Form /BBox [0 0 9 9] /Filter /FlateDecode /Length 3 >>\nstream\nabc\nendstream";
    let d = doc(
        "/Resources << /XObject << /Fm 5 0 R >> >>",
        b"/Fm Do",
        &[broken],
    );
    let f = found(&d);
    assert!(!f.asks() && !f.is_complete(), "{f:?}");
}

#[test]
fn a_text_fault_does_not_spoil_the_answer() {
    let d = doc("", b"BT (no font) Tj ET", &[]);
    let f = found(&d);
    assert!(!f.asks() && f.is_complete(), "{f:?}");
}

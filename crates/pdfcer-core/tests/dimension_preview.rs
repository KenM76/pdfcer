//! `EditSession::dimension_preview`: a ce dimension drag preview is the
//! appearance the commit bakes, from `&self`, with nothing staged.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    DEFAULT_GROUP_ID, DimStandard, DimensionId, DimensionKind, GroupId, NumberFormat, ScaleState,
    Unit,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::vector::{AxisConstraint, Point};
use pdfcer_core::writer::SaveOptions;

fn one_page_pdf() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn linear(offset: f64, text_along: f64) -> DimensionKind {
    DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(300.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset,
        text_along,
        extension_gap: [None; 2],
    }
}

fn committed_ap(s: &EditSession, annot: ObjId) -> Vec<u8> {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let Object::Dict(d) = &doc.get(annot).unwrap().value else {
        panic!("annotation is not a dictionary")
    };
    let Some(Object::Dict(ap)) = d.get(b"AP") else {
        panic!("no /AP")
    };
    let n = ap.get(b"N").and_then(Object::as_reference).unwrap();
    match &doc.get(n).unwrap().value {
        Object::Stream(st) => st.data_span.slice(doc.bytes()).unwrap().to_vec(),
        other => panic!("/N is not a stream: {other:?}"),
    }
}

/// The preview of a placement bakes the same bytes the placement commits,
/// including the operator's text override, and stages nothing.
#[test]
fn the_preview_is_the_appearance_the_placement_commits() {
    let mut s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    let (annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, linear(0.0, 0.0))
        .unwrap();
    s.set_dimension_label(id, Some("2X <DIM>")).unwrap();
    let before = committed_ap(&s, annot);

    let preview = s.dimension_preview(id, &linear(30.0, 40.0)).unwrap();
    assert_eq!(
        committed_ap(&s, annot),
        before,
        "a preview must not change the document"
    );
    assert!(preview.appearance.label.starts_with("2X "));

    s.place_dimension(id, 30.0, 40.0).unwrap();
    assert_eq!(preview.appearance.ap_content, committed_ap(&s, annot));
}

/// The label box follows `text_along`, and it sits inside `/Rect`.
#[test]
fn the_label_rect_moves_with_the_text_and_lies_inside_the_rect() {
    let mut s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    let (_, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, linear(20.0, 0.0))
        .unwrap();
    let centred = s
        .dimension_preview(id, &linear(20.0, 0.0))
        .unwrap()
        .appearance;
    let slid = s
        .dimension_preview(id, &linear(20.0, 50.0))
        .unwrap()
        .appearance;

    let (c, m) = (centred.label_rect(), slid.label_rect());
    assert!((m.llx - c.llx - 50.0).abs() < 1e-9 && (m.lly - c.lly).abs() < 1e-9);
    // Centred on the dimension line's midpoint (x = 200).
    assert!(c.llx < 200.0 && c.urx > 200.0 && c.lly < 220.0 && c.ury > 220.0);
    let r = centred.rect;
    assert!(c.llx >= r.llx && c.urx <= r.urx && c.lly >= r.lly && c.ury <= r.ury);
}

#[test]
fn previewing_an_unknown_ce_dimension_is_refused_by_name() {
    let s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    assert!(matches!(
        s.dimension_preview(DimensionId(7), &linear(0.0, 0.0)),
        Err(EditError::DimensionNotFound { id: 7 })
    ));
}

/// A preview for a ce dimension that does not exist yet bakes the bytes the
/// placing `add_dimension` commits, in a group whose scale, format and
/// standard all differ from the factory defaults, and stages nothing.
#[test]
fn a_new_ce_dimension_previews_as_the_placement_commits_it() {
    let mut s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    let g = s.add_dimension_group("Plan", Unit::Inch).unwrap();
    s.set_group_scale(
        g,
        ScaleState::Calibrated { scale: 0.05 },
        NumberFormat::decimal(Unit::Inch, 3),
    )
    .unwrap();
    s.set_group_standard(g, DimStandard::Iso).unwrap();
    let kind = linear(25.0, 30.0);

    let before = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let preview = s.new_dimension_preview(g, &kind).unwrap().appearance;
    assert_eq!(
        s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0,
        before,
        "a preview must not change the document"
    );
    // 200 pt at 0.05 in/pt, three places, ISO's decimal comma: the group's
    // scale, format and standard all reached it.
    assert_eq!(preview.label, "10,000 in");

    let default = s.new_dimension_preview(DEFAULT_GROUP_ID, &kind).unwrap();
    assert_ne!(default.appearance.ap_content, preview.ap_content);

    let (annot, _) = s.add_dimension(0, g, kind).unwrap();
    assert_eq!(preview.ap_content, committed_ap(&s, annot));
}

#[test]
fn previewing_into_an_unknown_group_is_refused_by_name() {
    let s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    assert!(matches!(
        s.new_dimension_preview(GroupId(9), &linear(0.0, 0.0)),
        Err(EditError::DimensionGroupNotFound { id: 9 })
    ));
}

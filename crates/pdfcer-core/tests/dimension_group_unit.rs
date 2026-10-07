//! A dimension group's unit is display, not calibration (pdfcer-gui G146):
//! changing it, by `set_group_unit` or by a per-ce-dimension unit override,
//! shows the same real length in the new unit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    DEFAULT_GROUP_ID, DimensionKind, GroupId, NumberFormat, ScaleState, StyleOverrides, Unit,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::object::Object;
use pdfcer_core::vector::{AxisConstraint, Point};
use pdfcer_core::writer::SaveOptions;

fn minimal_pdf() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
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

/// A session whose one 100 pt ce dimension reads `1000.00 mm`.
fn placed() -> (EditSession, pdfcer_core::dimension::DimensionId) {
    let mut s = EditSession::new(Document::from_bytes(minimal_pdf()).unwrap());
    s.set_group_scale(
        DEFAULT_GROUP_ID,
        ScaleState::Calibrated { scale: 10.0 },
        NumberFormat::decimal(Unit::Millimeter, 2),
    )
    .unwrap();
    let kind = DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(200.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    };
    let (_, id) = s.add_dimension(0, DEFAULT_GROUP_ID, kind).unwrap();
    (s, id)
}

/// The saved `/AP /N` bytes of the one ce dimension.
fn baked(session: &EditSession) -> String {
    let bytes = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0;
    let doc = Document::from_bytes(bytes).unwrap();
    let annot_id = session.dimension_model().dimensions()[0].annot.unwrap();
    let Object::Dict(annot) = &doc.get(annot_id).unwrap().value else {
        panic!("the annotation is a dict");
    };
    let ap_id = annot
        .get(b"AP")
        .and_then(Object::as_dict)
        .and_then(|ap| ap.get(b"N"))
        .and_then(Object::as_reference)
        .unwrap();
    let Some(Object::Stream(ap)) = doc.get(ap_id).map(|io| &io.value) else {
        panic!("the /AP /N is a stream");
    };
    String::from_utf8_lossy(ap.data_span.slice(doc.bytes()).unwrap()).into_owned()
}

#[test]
fn a_group_unit_change_keeps_the_real_length() {
    let (mut s, _) = placed();
    assert!(baked(&s).contains("1000.00 mm"), "{}", baked(&s));
    assert_eq!(
        s.set_group_unit(DEFAULT_GROUP_ID, Unit::DecimalFeet)
            .unwrap(),
        1
    );
    assert!(baked(&s).contains("3.28 ft"), "{}", baked(&s));
    s.undo().unwrap();
    assert!(
        baked(&s).contains("1000.00 mm"),
        "undo restores the mm reading"
    );
}

#[test]
fn a_member_unit_override_keeps_the_real_length() {
    let (mut s, id) = placed();
    let over = StyleOverrides {
        unit: Some(Unit::DecimalFeet),
        ..StyleOverrides::default()
    };
    s.set_dimension_style(id, over).unwrap();
    assert!(baked(&s).contains("3.28 ft"), "{}", baked(&s));
}

#[test]
fn an_unknown_group_is_refused() {
    let (mut s, _) = placed();
    assert!(matches!(
        s.set_group_unit(GroupId(99), Unit::Inch),
        Err(EditError::DimensionGroupNotFound { id: 99 })
    ));
}

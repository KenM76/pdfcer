//! Area ce dimensions: a closed perimeter with `area: true` measures the
//! enclosed area, written as `/Polygon` + `/IT /PolygonDimension` with a
//! `/Measure /A` consistent with `/X` (ISO 32000-1 §12.9 Table 262).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    DEFAULT_GROUP_ID, DimensionId, DimensionKind, NumberFormat, ScaleState, Unit,
    deserialize_model, serialize_model, sidecar_version,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, decode_text_string};
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::vector::{Point, polygon_area};
use pdfcer_core::writer::SaveOptions;

/// Catalog(1) → pages(2) → page(3), 400 × 400.
fn minimal_pdf() -> Vec<u8> {
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

/// The acceptance square: 72 pt a side, counter-clockwise from (100, 100).
fn square() -> Vec<Point> {
    vec![
        Point::new(100.0, 100.0),
        Point::new(172.0, 100.0),
        Point::new(172.0, 172.0),
        Point::new(100.0, 172.0),
    ]
}

fn area_kind(points: Vec<Point>) -> DimensionKind {
    DimensionKind::Perimeter {
        points,
        closed: true,
        area: true,
        offset: 0.0,
        text_along: 0.0,
    }
}

/// A session whose default group is 1 pt = 0.05 m, two decimal places.
fn calibrated() -> (Vec<u8>, EditSession) {
    let bytes = minimal_pdf();
    let mut s = EditSession::new(Document::from_bytes(bytes.clone()).unwrap());
    s.set_group_scale(
        DEFAULT_GROUP_ID,
        ScaleState::Calibrated { scale: 0.05 },
        NumberFormat::decimal(Unit::Meter, 2),
    )
    .unwrap();
    (bytes, s)
}

fn save(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

/// The saved annotation dict for `annot`, and the saved document.
fn saved_annot(s: &EditSession, annot: ObjId) -> (Document, Dict) {
    let doc = Document::from_bytes(save(s)).unwrap();
    let Object::Dict(d) = doc.get(annot).unwrap().value.clone() else {
        panic!("the annotation is a dict");
    };
    (doc, d)
}

/// The raw `/AP /N` stream bytes of `annot`.
fn ap_bytes(doc: &Document, annot: &Dict) -> Vec<u8> {
    let ap_id = annot
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"N")
        .and_then(Object::as_reference)
        .unwrap();
    let Some(Object::Stream(ap)) = doc.get(ap_id).map(|io| &io.value) else {
        panic!("/AP /N is a stream");
    };
    ap.data_span.slice(doc.bytes()).unwrap().to_vec()
}

fn name(d: &Dict, key: &[u8]) -> Vec<u8> {
    d.get(key).unwrap().as_name().unwrap().as_bytes().to_vec()
}

/// The first `/C` of a NumberFormat array key of `measure`.
fn first_c(measure: &Dict, key: &[u8]) -> f64 {
    let arr = measure.get(key).unwrap().as_array().unwrap();
    arr[0]
        .as_dict()
        .unwrap()
        .get(b"C")
        .unwrap()
        .as_number()
        .unwrap()
}

#[test]
fn polygon_area_of_the_square_is_5184_in_either_winding() {
    let ccw = square();
    let mut cw = square();
    cw.reverse();
    assert_eq!(polygon_area(&ccw), 5184.0);
    assert_eq!(polygon_area(&cw), 5184.0);
}

#[test]
fn polygon_area_of_fewer_than_three_points_is_zero() {
    assert_eq!(polygon_area(&square()[..2]), 0.0);
}

#[test]
fn the_square_writes_a_polygon_dimension_with_four_vertices() {
    let (_, mut s) = calibrated();
    let (annot, _) = s
        .add_dimension(0, DEFAULT_GROUP_ID, area_kind(square()))
        .unwrap();
    let (_, d) = saved_annot(&s, annot);
    assert_eq!(name(&d, b"Subtype"), b"Polygon");
    assert_eq!(name(&d, b"IT"), b"PolygonDimension");
    let vertices = d.get(b"Vertices").unwrap().as_array().unwrap();
    assert_eq!(vertices.len(), 8, "4 vertices, flat x/y pairs");
}

#[test]
fn the_label_reads_12_96_square_metres_in_the_appearance_and_the_contents() {
    let (_, mut s) = calibrated();
    let (annot, _) = s
        .add_dimension(0, DEFAULT_GROUP_ID, area_kind(square()))
        .unwrap();
    let (doc, d) = saved_annot(&s, annot);
    let Some(Object::String(contents)) = d.get(b"Contents") else {
        panic!("/Contents is a string");
    };
    assert_eq!(decode_text_string(contents).text, "12.96 m\u{b2}");
    // WinAnsi 0xB2 is twosuperior: the glyph exists in the label font, so it
    // is not tofu. Accept the byte raw or as the `\262` octal escape.
    let ap = ap_bytes(&doc, &d);
    let raw = [b"12.96 m".as_slice(), &[0xB2]].concat();
    let escaped = b"12.96 m\\262".to_vec();
    let has = |needle: &[u8]| ap.windows(needle.len()).any(|w| w == needle);
    assert!(
        has(&raw) || has(&escaped),
        "the baked /AP must draw `12.96 m²`: {}",
        String::from_utf8_lossy(&ap)
    );
}

#[test]
fn the_measure_area_factor_is_consistent_with_x() {
    let (_, mut s) = calibrated();
    let (annot, _) = s
        .add_dimension(0, DEFAULT_GROUP_ID, area_kind(square()))
        .unwrap();
    let (_, d) = saved_annot(&s, annot);
    let measure = d.get(b"Measure").unwrap().as_dict().unwrap();
    let x = first_c(measure, b"X");
    let a = first_c(measure, b"A");
    // Table 262: /A converts X's units SQUARED, so pt² → m² is X.C² × A.C.
    let area = 5184.0 * x * x * a;
    assert!((area - 12.96).abs() < 1e-9, "reader-computed area {area}");
    let a0 = measure.get(b"A").unwrap().as_array().unwrap()[0]
        .as_dict()
        .unwrap()
        .clone();
    let Some(Object::String(u)) = a0.get(b"U") else {
        panic!("/A /U is a string");
    };
    assert_eq!(decode_text_string(u).text, "m\u{b2}");
}

#[test]
fn fewer_than_three_vertices_is_refused_by_name_before_anything_is_staged() {
    let (_, mut s) = calibrated();
    let depth = s.undo_depth();
    let err = s
        .add_dimension(0, DEFAULT_GROUP_ID, area_kind(square()[..2].to_vec()))
        .unwrap_err();
    assert!(
        matches!(err, EditError::AreaNeedsThreeVertices { vertices: 2 }),
        "{err:?}"
    );
    assert_eq!(s.undo_depth(), depth, "nothing was staged");
    assert!(s.dimension_model().dimensions().is_empty());
}

#[test]
fn an_area_over_an_open_path_is_refused_by_name() {
    let (_, mut s) = calibrated();
    let depth = s.undo_depth();
    let kind = DimensionKind::Perimeter {
        points: square(),
        closed: false,
        area: true,
        offset: 0.0,
        text_along: 0.0,
    };
    let err = s.add_dimension(0, DEFAULT_GROUP_ID, kind).unwrap_err();
    assert!(matches!(err, EditError::AreaNeedsClosedOutline), "{err:?}");
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn undo_removes_the_area_ce_dimension_in_one_step() {
    let (_, mut s) = calibrated();
    let before = save(&s);
    let depth = s.undo_depth();
    s.add_dimension(0, DEFAULT_GROUP_ID, area_kind(square()))
        .unwrap();
    assert_eq!(s.undo_depth(), depth + 1, "one undo entry");
    s.undo().unwrap();
    assert!(s.dimension_model().dimensions().is_empty());
    assert_eq!(save(&s), before, "one undo restores the pre-add bytes");
}

#[test]
fn an_incremental_save_keeps_every_original_byte() {
    let (original, mut s) = calibrated();
    s.add_dimension(0, DEFAULT_GROUP_ID, area_kind(square()))
        .unwrap();
    let out = save(&s);
    assert!(out.starts_with(&original), "original bytes verbatim");
    assert!(out.len() > original.len());
}

#[test]
fn a_closed_perimeter_switches_to_its_area_and_back_in_one_step_each() {
    let (_, mut s) = calibrated();
    let mut kind = area_kind(square());
    if let DimensionKind::Perimeter { area, .. } = &mut kind {
        *area = false;
    }
    let (annot, id) = s.add_dimension(0, DEFAULT_GROUP_ID, kind).unwrap();
    let contents = |s: &EditSession| {
        let (_, d) = saved_annot(s, annot);
        let Some(Object::String(c)) = d.get(b"Contents") else {
            panic!("/Contents");
        };
        decode_text_string(c).text
    };
    assert_eq!(contents(&s), "14.40 m", "4 × 72 pt × 0.05");
    let depth = s.undo_depth();
    s.set_dimension_area(id, true).unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
    assert_eq!(contents(&s), "12.96 m\u{b2}");
    s.undo().unwrap();
    assert_eq!(contents(&s), "14.40 m");
}

#[test]
fn switching_to_area_is_refused_for_a_ce_dimension_without_vertices_or_an_open_path() {
    let (_, mut s) = calibrated();
    let linear = DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(300.0, 200.0),
        constraint: pdfcer_core::vector::AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    };
    let (_, lin) = s.add_dimension(0, DEFAULT_GROUP_ID, linear).unwrap();
    let open = DimensionKind::Perimeter {
        points: square(),
        closed: false,
        area: false,
        offset: 0.0,
        text_along: 0.0,
    };
    let (_, path) = s.add_dimension(0, DEFAULT_GROUP_ID, open).unwrap();
    let depth = s.undo_depth();
    assert!(matches!(
        s.set_dimension_area(lin, true).unwrap_err(),
        EditError::DimensionHasNoVertices { .. }
    ));
    assert!(matches!(
        s.set_dimension_area(path, true).unwrap_err(),
        EditError::AreaNeedsClosedOutline
    ));
    assert!(matches!(
        s.set_dimension_area(DimensionId(4242), true).unwrap_err(),
        EditError::DimensionNotFound { .. }
    ));
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn the_area_flag_survives_the_sidecar_and_raises_its_version() {
    let (_, mut s) = calibrated();
    s.add_dimension(0, DEFAULT_GROUP_ID, area_kind(square()))
        .unwrap();
    let model = s.dimension_model();
    let written = serialize_model(&model);
    assert_eq!(sidecar_version(&written), Some(5));
    let back = deserialize_model(&written).unwrap();
    assert!(back.dimensions()[0].kind.is_area());
    // And through a real save and reopen.
    let reopened = EditSession::new(Document::from_bytes(save(&s)).unwrap());
    assert!(reopened.dimension_model().dimensions()[0].kind.is_area());
}

#[test]
fn a_document_without_an_area_keeps_its_older_sidecar_version() {
    let (_, mut s) = calibrated();
    let mut kind = area_kind(square());
    if let DimensionKind::Perimeter { area, .. } = &mut kind {
        *area = false;
    }
    s.add_dimension(0, DEFAULT_GROUP_ID, kind).unwrap();
    assert_eq!(
        sidecar_version(&serialize_model(&s.dimension_model())),
        Some(3)
    );
}

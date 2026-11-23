//! Snap targets on every kind of page object (pdfcer-gui request G324).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::dimension::{DEFAULT_GROUP_ID, DimensionKind, FitCircle, resolve_style};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::object::ObjId;
use pdfcer_core::vector::{
    AxisConstraint, Point, SnapCandidate, SnapConfig, SnapKind, SnapSource, VectorObject,
    snap_candidates_with_markups,
};
use std::collections::BTreeSet;
use std::path::Path;

const OCG: ObjId = ObjId::new(7, 0);
const STAMP: ObjId = ObjId::new(8, 0);
const LINK: ObjId = ObjId::new(9, 0);
const HIGHLIGHT: ObjId = ObjId::new(10, 0);
const WIDGET: ObjId = ObjId::new(11, 0);
const LAYERED_STAMP: ObjId = ObjId::new(12, 0);
const NOTE: ObjId = ObjId::new(13, 0);

/// A 30-degree rotated image at (300,300), Helvetica "Hello" at (100,600),
/// an image on layer 7 at (500,500), and annotations 8 to 13.
const CONTENT: &str = "q 43.30127 25 -25 43.30127 300 300 cm /Im1 Do Q \
BT /F1 12 Tf 100 600 Td (Hello) Tj ET \
/OC /L1 BDC q 10 0 0 10 500 500 cm /Im1 Do Q EMC";

fn fixture() -> EditSession {
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
          /Resources << /XObject << /Im1 5 0 R >> /Font << /F1 6 0 R >> \
          /Properties << /L1 7 0 R >> >> \
          /Annots [8 0 R 9 0 R 10 0 R 11 0 R 12 0 R 13 0 R] >>"
            .to_vec(),
        stream("", CONTENT.as_bytes()),
        stream(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 \
             /ColorSpace /DeviceGray /BitsPerComponent 8",
            b"A",
        ),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /OCG /Name (Hidden) >>".to_vec(),
        b"<< /Type /Annot /Subtype /Stamp /Rect [400 600 480 640] /Name /Approved >>".to_vec(),
        b"<< /Type /Annot /Subtype /Link /Rect [50 50 150 70] /Border [0 0 0] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Highlight /Rect [200 700 260 715] \
          /QuadPoints [200 715 260 715 200 700 260 700] >>"
            .to_vec(),
        b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /Rect [100 200 200 230] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Stamp /Rect [500 100 540 140] /OC 7 0 R >>".to_vec(),
        b"<< /Type /Annot /Subtype /Text /Rect [20 750 40 770] >>".to_vec(),
    ];
    EditSession::new(Document::from_bytes(build(&objects)).unwrap())
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

fn build(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    let size = objects.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// Every candidate at `(x, y)`, tolerance 3, with `hidden` switched off.
fn at_with(s: &mut EditSession, x: f64, y: f64, hidden: &BTreeSet<ObjId>) -> Vec<SnapCandidate> {
    let model = s.page_objects(0).unwrap();
    let all = s.page_snap_geometry(0, Some(&model), hidden).unwrap();
    let cfg = SnapConfig::new(3.0).with_axes(false);
    snap_candidates_with_markups(Point::new(x, y), &cfg, Some(&model), &all)
}

fn at(s: &mut EditSession, x: f64, y: f64) -> Vec<SnapCandidate> {
    at_with(s, x, y, &BTreeSet::new())
}

fn has(c: &[SnapCandidate], kind: SnapKind, p: (f64, f64), source: SnapSource) -> bool {
    c.iter().any(|c| {
        c.kind == kind
            && (c.point.x - p.0).abs() < 1e-3
            && (c.point.y - p.1).abs() < 1e-3
            && c.source == source
    })
}

fn has_annot(c: &[SnapCandidate], kind: SnapKind, p: (f64, f64), id: ObjId) -> bool {
    c.iter().any(|c| {
        c.kind == kind
            && (c.point.x - p.0).abs() < 1e-3
            && (c.point.y - p.1).abs() < 1e-3
            && c.source_annotation == Some(id)
    })
}

#[test]
fn a_rotated_image_snaps_to_its_real_corners_and_centre() {
    let mut s = fixture();
    // (1,0) maps to (343.30127, 325): a corner of the rotated quad, not of its
    // axis-aligned box.
    let c = at(&mut s, 343.0, 325.5);
    assert!(
        has(&c, SnapKind::Node, (343.30127, 325.0), SnapSource::Image),
        "{c:?}"
    );
    let img = c.iter().find(|c| c.source == SnapSource::Image).unwrap();
    assert_eq!(img.source_object, Some(0));
    assert_eq!(img.source_annotation, None);
    // (1,1) maps to (318.30127, 368.30127).
    let top = at(&mut s, 318.0, 368.0);
    assert!(has(
        &top,
        SnapKind::Node,
        (318.30127, 368.30127),
        SnapSource::Image
    ));
    // Centre: (0.5,0.5) maps to (309.15064, 334.15064).
    let mid = at(&mut s, 309.0, 334.0);
    assert!(has(
        &mid,
        SnapKind::Center,
        (309.15064, 334.15064),
        SnapSource::Image
    ));
    // The axis-aligned box's corner (275, 300) is not a target.
    assert!(
        !at(&mut s, 275.0, 300.0)
            .iter()
            .any(|c| c.source == SnapSource::Image && c.kind == SnapKind::Node)
    );
}

#[test]
fn a_text_run_snaps_to_its_box_corner_baseline_ends_and_centre() {
    let mut s = fixture();
    let model = s.page_objects(0).unwrap();
    let run = model
        .objects
        .iter()
        .find_map(|o| match o {
            VectorObject::Text(t) => t.runs.first().copied(),
            _ => None,
        })
        .expect("a text run");
    let b = run.bounds;
    // Box corner.
    let c = at(&mut s, b.max.x - 0.5, b.max.y - 0.5);
    assert!(
        has(&c, SnapKind::Node, (b.max.x, b.max.y), SnapSource::Text),
        "{c:?}"
    );
    // Baseline start is the pen position; the end is "Hello"'s Helvetica
    // advance, 2278/1000 x 12, further along.
    let start = at(&mut s, 100.5, 600.5);
    assert!(has(
        &start,
        SnapKind::Endpoint,
        (100.0, 600.0),
        SnapSource::Text
    ));
    let end = at(&mut s, 127.0, 600.5);
    assert!(
        has(&end, SnapKind::Endpoint, (127.336, 600.0), SnapSource::Text),
        "{end:?}"
    );
    let centre = b.min.midpoint(b.max);
    let mid = at(&mut s, centre.x, centre.y + 0.5);
    assert!(has(
        &mid,
        SnapKind::Center,
        (centre.x, centre.y),
        SnapSource::Text
    ));
}

#[test]
fn a_widget_snaps_to_its_centre_and_corners_as_a_form_field() {
    let mut s = fixture();
    let c = at(&mut s, 150.5, 215.5);
    assert!(
        has(&c, SnapKind::Center, (150.0, 215.0), SnapSource::FormField),
        "{c:?}"
    );
    assert!(has_annot(&c, SnapKind::Center, (150.0, 215.0), WIDGET));
    let corner = at(&mut s, 199.0, 229.0);
    assert!(has(
        &corner,
        SnapKind::Node,
        (200.0, 230.0),
        SnapSource::FormField
    ));
}

#[test]
fn a_stamp_a_link_a_note_and_a_highlight_quad_are_targets() {
    let mut s = fixture();
    let stamp = at(&mut s, 401.0, 601.0);
    assert!(has(
        &stamp,
        SnapKind::Node,
        (400.0, 600.0),
        SnapSource::Annotation
    ));
    assert!(has_annot(&stamp, SnapKind::Node, (400.0, 600.0), STAMP));
    let link = at(&mut s, 100.0, 69.0);
    assert!(has(
        &link,
        SnapKind::Midpoint,
        (100.0, 70.0),
        SnapSource::Annotation
    ));
    assert!(has_annot(&link, SnapKind::Midpoint, (100.0, 70.0), LINK));
    let note = at(&mut s, 30.0, 760.5);
    assert!(has_annot(&note, SnapKind::Center, (30.0, 760.0), NOTE));
    // The quad is read in Z order; its corners and centre are targets.
    let quad = at(&mut s, 259.0, 701.0);
    assert!(
        has(
            &quad,
            SnapKind::Node,
            (260.0, 700.0),
            SnapSource::TextMarkup
        ),
        "{quad:?}"
    );
    assert!(has_annot(&quad, SnapKind::Node, (260.0, 700.0), HIGHLIGHT));
    let centre = at(&mut s, 230.0, 707.0);
    assert!(has(
        &centre,
        SnapKind::Center,
        (230.0, 707.5),
        SnapSource::TextMarkup
    ));
}

#[test]
fn objects_on_a_hidden_layer_give_nothing() {
    let mut s = fixture();
    let shown = BTreeSet::new();
    let hidden: BTreeSet<ObjId> = [OCG].into();
    // The layered stamp.
    assert!(has_annot(
        &at_with(&mut s, 501.0, 101.0, &shown),
        SnapKind::Node,
        (500.0, 100.0),
        LAYERED_STAMP
    ));
    assert!(at_with(&mut s, 501.0, 101.0, &hidden).is_empty());
    // The layered image at (500,500)-(510,510).
    let on = at_with(&mut s, 510.5, 510.5, &shown);
    assert!(has(&on, SnapKind::Node, (510.0, 510.0), SnapSource::Image));
    let off = at_with(&mut s, 510.5, 510.5, &hidden);
    assert!(
        !off.iter().any(|c| c.source == SnapSource::Image),
        "{off:?}"
    );
}

fn hello() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

#[test]
fn a_linear_ce_dimension_snaps_to_its_extension_line_and_dimension_line_ends() {
    let mut s = hello();
    let kind = DimensionKind::Linear {
        a: Point::new(100.0, 100.0),
        b: Point::new(200.0, 100.0),
        constraint: AxisConstraint::Horizontal,
        offset: 40.0,
        text_along: 0.5,
        extension_gap: [None; 2],
    };
    s.add_dimension(0, DEFAULT_GROUP_ID, kind.clone()).unwrap();
    let dims = s.dimension_model();
    let record = &dims.dimensions()[0];
    let style = resolve_style(dims.group(record.group).unwrap(), &record.style);
    let [ext_a, _] = kind.extension_segments(style).unwrap();
    let (gap_end, far_end) = ext_a.expect("an extension line");
    let (dim_a, _, _, _) = kind.linear_geometry().unwrap();

    for p in [gap_end, far_end, dim_a] {
        let c = at(&mut s, p.x + 0.4, p.y + 0.4);
        assert!(
            has(&c, SnapKind::Endpoint, (p.x, p.y), SnapSource::CeDimension),
            "{p:?} {c:?}"
        );
        assert!(c.iter().any(|c| c.source_annotation == record.annot));
    }
    // Still left out of the markup-only reader.
    assert!(
        s.markup_snap_geometry(0, &BTreeSet::new())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_circular_ce_dimension_snaps_to_its_centre() {
    let mut s = hello();
    let kind = DimensionKind::Circular {
        fit: FitCircle {
            center: Point::new(300.0, 300.0),
            radius: 50.0,
            residual: 0.0,
        },
        show_diameter: true,
        leader_angle: 45.0,
        text_distance: None,
    };
    s.add_dimension(0, DEFAULT_GROUP_ID, kind).unwrap();
    let c = at(&mut s, 301.0, 299.0);
    assert!(
        has(
            &c,
            SnapKind::Center,
            (300.0, 300.0),
            SnapSource::CeDimension
        ),
        "{c:?}"
    );
}

//! `place_dimension` on a circular ce dimension (`Pass 370.0`): `offset` is the
//! text distance past the rim, `text_along` the leader angle in degrees.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{DEFAULT_GROUP_ID, DimensionId, DimensionKind, FitCircle};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::vector::Point;
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

/// Radius 50 about (200, 200).
fn circle(show_diameter: bool) -> DimensionKind {
    DimensionKind::Circular {
        fit: FitCircle {
            center: Point::new(200.0, 200.0),
            radius: 50.0,
            residual: 0.0,
        },
        show_diameter,
        leader_angle: 0.0,
        text_distance: None,
    }
}

fn setup(show_diameter: bool) -> (EditSession, ObjId, DimensionId) {
    let mut s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    let (annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, circle(show_diameter))
        .unwrap();
    (s, annot, id)
}

fn kind(s: &EditSession, id: DimensionId) -> DimensionKind {
    s.dimension_model().dimension(id).unwrap().kind.clone()
}

/// The saved annotation's `/L` leader and its measured caption.
fn saved_leader(bytes: Vec<u8>, annot: ObjId) -> (Vec<f64>, String) {
    let doc = Document::from_bytes(bytes).unwrap();
    let Object::Dict(d) = &doc.get(annot).unwrap().value else {
        panic!("annotation is not a dictionary")
    };
    let l = d
        .get(b"L")
        .and_then(Object::as_array)
        .map(|a| a.iter().filter_map(Object::as_number).collect())
        .unwrap();
    let Some(Object::String(c)) = d.get(b"Contents") else {
        panic!("no /Contents")
    };
    (l, String::from_utf8_lossy(c).into_owned())
}

fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

/// An unplaced circle draws exactly as before this Pass: centre to the rim
/// at +x, the text at the leader's midpoint.
#[test]
fn an_unplaced_radius_keeps_its_centre_to_rim_leader() {
    let (s, annot, id) = setup(false);
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let (l, _) = saved_leader(bytes, annot);
    assert!(close(&l, &[200.0, 200.0, 250.0, 200.0]), "{l:?}");
    let anchor = kind(&s, id).label_anchor().unwrap();
    assert!((anchor.x - 225.0).abs() < 1e-9 && (anchor.y - 200.0).abs() < 1e-9);
}

/// Placed at 90 degrees, 20 pt past the rim: the leader runs from the rim to
/// the text, straight up, and the measured value does not change.
#[test]
fn placing_outside_swings_the_leader_and_keeps_the_value() {
    let (mut s, annot, id) = setup(false);
    let before = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let (_, caption0) = saved_leader(before, annot);

    s.place_dimension(id, 20.0, 90.0).unwrap();
    let k = kind(&s, id);
    let DimensionKind::Circular {
        leader_angle,
        text_distance,
        ..
    } = k
    else {
        panic!("not circular")
    };
    assert!((leader_angle - 90.0).abs() < 1e-9);
    assert_eq!(text_distance, Some(20.0));

    let after = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let (l, caption1) = saved_leader(after.clone(), annot);
    assert!(close(&l, &[200.0, 250.0, 200.0, 270.0]), "{l:?}");
    assert_eq!(caption0, caption1, "placement must not re-measure");

    // The drag resolves back to the same pair, so a live preview and the
    // commit agree.
    let anchor = k.label_anchor().unwrap();
    let (off, ang) = k.placement_from_point(anchor).unwrap();
    assert!((off - 20.0).abs() < 1e-9 && (ang - 90.0).abs() < 1e-9);

    // The sidecar carries it across a reopen.
    let reopened = EditSession::new(Document::from_bytes(after).unwrap());
    assert_eq!(kind(&reopened, id), k);
}

/// A diameter placed inside crosses the circle rim to rim along the leader
/// angle, and a text distance past the opposite rim is clamped to it.
#[test]
fn an_inside_diameter_crosses_the_centre_and_is_clamped() {
    let (mut s, annot, id) = setup(true);
    s.place_dimension(id, -1000.0, 180.0).unwrap();
    assert_eq!(kind(&s, id).circular_text_distance(), Some(-100.0));
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let (l, _) = saved_leader(bytes, annot);
    assert!(close(&l, &[250.0, 200.0, 150.0, 200.0]), "{l:?}");

    // Switching to radius keeps the angle and clamps the text to the centre.
    s.set_dimension_display(id, false).unwrap();
    let k = kind(&s, id);
    assert!(
        matches!(k, DimensionKind::Circular { leader_angle, .. } if (leader_angle - 180.0).abs() < 1e-9)
    );
    assert_eq!(k.circular_text_distance(), Some(-50.0));
}

/// One undo step restores the unplaced circle.
#[test]
fn undo_restores_the_unplaced_circle() {
    let (mut s, _annot, id) = setup(false);
    let before = kind(&s, id);
    s.place_dimension(id, 30.0, 45.0).unwrap();
    assert_ne!(kind(&s, id), before);
    s.undo().unwrap();
    assert_eq!(kind(&s, id), before);
}

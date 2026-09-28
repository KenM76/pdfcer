//! `EditSession::set_dimension_extension_gap`: a per-end extension-line gap
//! on a linear ce dimension (`Pass 369.0`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    DEFAULT_GROUP_ID, DimensionEnd, DimensionId, DimensionKind, resolve_style,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
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

/// Where each extension line of `id` is drawn, from the session's model.
fn segments(s: &EditSession, id: DimensionId) -> [Option<(Point, Point)>; 2] {
    let model = s.dimension_model();
    let rec = model.dimension(id).unwrap();
    let style = resolve_style(model.group(rec.group).unwrap(), &rec.style);
    rec.kind.extension_segments(style).unwrap()
}

fn gaps(s: &EditSession, id: DimensionId) -> [Option<f64>; 2] {
    match s.dimension_model().dimension(id).unwrap().kind {
        DimensionKind::Linear { extension_gap, .. } => extension_gap,
        _ => panic!("not linear"),
    }
}

/// Points at y=200, dimension line stood off 50 above: each extension line
/// runs up from the point. Setting end A's gap moves only A's start.
#[test]
fn a_gap_moves_only_its_own_ends_extension_start() {
    let mut s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    let (annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, linear(50.0, 0.0))
        .unwrap();
    let before_ap = committed_ap(&s, annot);
    let [a0, b0] = segments(&s, id);
    let (a0, b0) = (a0.unwrap(), b0.unwrap());

    s.set_dimension_extension_gap(id, DimensionEnd::A, Some(20.0))
        .unwrap();
    let [a1, b1] = segments(&s, id);
    let (a1, b1) = (a1.unwrap(), b1.unwrap());
    assert!(
        (a1.0.x - 100.0).abs() < 1e-9 && (a1.0.y - 220.0).abs() < 1e-9,
        "{a1:?}"
    );
    assert_eq!(a1.1, a0.1, "the overshoot end does not move");
    assert_eq!(b1, b0, "end B is untouched");
    assert_ne!(
        committed_ap(&s, annot),
        before_ap,
        "the appearance re-bakes"
    );

    // The appearance is the baker's, so the preview of the stored kind
    // matches the commit byte for byte.
    let kind = s.dimension_model().dimension(id).unwrap().kind.clone();
    let preview = s.dimension_preview(id, &kind).unwrap();
    assert_eq!(preview.appearance.ap_content, committed_ap(&s, annot));

    assert_eq!(
        s.undo(),
        Some(CommandKind::SetDimensionExtensionGap {
            end: DimensionEnd::A,
            cleared: false
        })
    );
    assert_eq!(gaps(&s, id), [None, None]);
    assert_eq!(committed_ap(&s, annot), before_ap);
}

/// The gap survives a placement drag and a save/reopen; clearing it
/// restores the standard's appearance exactly.
#[test]
fn a_gap_survives_placement_and_the_sidecar_and_clears_exactly() {
    let mut s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    let (annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, linear(50.0, 0.0))
        .unwrap();
    s.place_dimension(id, 60.0, 10.0).unwrap();
    let standard_ap = committed_ap(&s, annot);

    s.set_dimension_extension_gap(id, DimensionEnd::B, Some(12.5))
        .unwrap();
    s.place_dimension(id, 50.0, 10.0).unwrap();
    s.place_dimension(id, 60.0, 10.0).unwrap();
    assert_eq!(gaps(&s, id), [None, Some(12.5)]);

    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let mut reopened = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert_eq!(gaps(&reopened, id), [None, Some(12.5)]);

    reopened
        .set_dimension_extension_gap(id, DimensionEnd::B, None)
        .unwrap();
    assert_eq!(gaps(&reopened, id), [None, None]);
    assert_eq!(committed_ap(&reopened, annot), standard_ap);
}

/// Refusals: negative, non-finite, one leaving no line to draw, a
/// non-linear ce dimension, an unknown id. None of them changes anything.
#[test]
fn an_undrawable_gap_or_a_non_linear_target_is_refused() {
    let mut s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    let (annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, linear(50.0, 0.0))
        .unwrap();
    let before = committed_ap(&s, annot);

    // ANSI: reach = 50 - 3 (overshoot) = 47.
    for bad in [-1.0, f64::NAN, 47.0, 60.0] {
        match s.set_dimension_extension_gap(id, DimensionEnd::A, Some(bad)) {
            Err(EditError::ExtensionGapOutOfRange { reach, .. }) => {
                assert!((reach - 47.0).abs() < 1e-9, "{reach}");
            }
            other => panic!("gap {bad}: {other:?}"),
        }
    }
    s.set_dimension_extension_gap(id, DimensionEnd::A, Some(46.9))
        .unwrap();
    assert!(segments(&s, id)[0].is_some(), "an accepted gap is drawn");
    s.undo();
    assert_eq!(committed_ap(&s, annot), before);

    let (_, angular) = s
        .add_dimension(
            0,
            DEFAULT_GROUP_ID,
            DimensionKind::Angular {
                apex: Point::new(50.0, 50.0),
                dir_a: Point::new(1.0, 0.0),
                dir_b: Point::new(0.0, 1.0),
                radius: 40.0,
                text_along: 0.0,
            },
        )
        .unwrap();
    assert!(matches!(
        s.set_dimension_extension_gap(angular, DimensionEnd::A, Some(1.0)),
        Err(EditError::NoExtensionLines { .. })
    ));
    assert!(matches!(
        s.set_dimension_extension_gap(DimensionId(999), DimensionEnd::A, None),
        Err(EditError::DimensionNotFound { id: 999 })
    ));
}

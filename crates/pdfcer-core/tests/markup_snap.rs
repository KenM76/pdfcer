//! Snap targets from markup annotations (pdfcer-gui request G281).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot::AnnotFlags;
use pdfcer_core::annot_author::{Color, LineEnding, MarkupSpec};
use pdfcer_core::dimension::{DEFAULT_GROUP_ID, DimensionKind};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::object::ObjId;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vector::{
    AxisConstraint, Point, SnapCandidate, SnapConfig, SnapKind, snap_candidates,
    snap_candidates_with_markups,
};
use std::collections::BTreeSet;
use std::path::Path;

fn session() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn square(width: f64) -> MarkupSpec {
    MarkupSpec::Square {
        rect: Rect {
            llx: 300.0,
            lly: 400.0,
            urx: 400.0,
            ury: 460.0,
        },
        border: Some(Color::Gray(0.0)),
        interior: None,
        border_width: width,
        border_effect: None,
    }
}

fn line(start: (f64, f64), end: (f64, f64)) -> MarkupSpec {
    MarkupSpec::Line {
        start,
        end,
        color: Color::Gray(0.0),
        width: 1.0,
        endings: (LineEnding::None, LineEnding::None),
        interior: None,
    }
}

/// Markup-only candidates at `(x, y)`, tolerance 3.
fn at(s: &EditSession, x: f64, y: f64) -> Vec<SnapCandidate> {
    let markups = s.markup_snap_geometry(0, &BTreeSet::new()).unwrap();
    snap_candidates_with_markups(Point::new(x, y), &SnapConfig::new(3.0), None, &markups)
}

fn has(c: &[SnapCandidate], kind: SnapKind, x: f64, y: f64, id: ObjId) -> bool {
    c.iter().any(|c| {
        c.kind == kind
            && (c.point.x - x).abs() < 1e-6
            && (c.point.y - y).abs() < 1e-6
            && c.source_annotation == Some(id)
            && c.source_object.is_none()
    })
}

#[test]
fn a_square_gives_corners_edge_middles_centre_and_edges_on_its_stroke_centreline() {
    let mut s = session();
    let id = s.add_markup(0, &square(4.0)).unwrap();
    // Border width 4: the centreline is 2 inside /Rect.
    assert!(has(&at(&s, 301.0, 401.0), SnapKind::Node, 302.0, 402.0, id));
    assert!(has(
        &at(&s, 350.0, 403.0),
        SnapKind::Midpoint,
        350.0,
        402.0,
        id
    ));
    assert!(has(
        &at(&s, 351.0, 431.0),
        SnapKind::Center,
        350.0,
        430.0,
        id
    ));
    let edge = at(&s, 330.0, 459.0);
    assert!(has(&edge, SnapKind::SegmentCenterline, 330.0, 458.0, id));
    // The /Rect corner itself is 2.83 away from the centreline corner: found.
    // A point 6 outside the box is not.
    assert!(at(&s, 294.0, 394.0).is_empty());
}

#[test]
fn a_circle_gives_its_centre_axis_ends_and_a_point_on_the_ellipse() {
    let mut s = session();
    let id = s
        .add_markup(
            0,
            &MarkupSpec::Circle {
                rect: Rect {
                    llx: 100.0,
                    lly: 100.0,
                    urx: 202.0,
                    ury: 162.0,
                },
                border: Some(Color::Gray(0.0)),
                interior: None,
                border_width: 2.0,
            },
        )
        .unwrap();
    // Centreline ellipse: centre (151,131), radii 50 x 30.
    assert!(has(
        &at(&s, 151.0, 131.0),
        SnapKind::Center,
        151.0,
        131.0,
        id
    ));
    assert!(has(&at(&s, 200.0, 131.5), SnapKind::Node, 201.0, 131.0, id));
    assert!(has(&at(&s, 151.0, 160.0), SnapKind::Node, 151.0, 161.0, id));
    let on = at(&s, 187.0, 152.0);
    let edge = on
        .iter()
        .find(|c| c.kind == SnapKind::SegmentCenterline)
        .expect("a point on the ellipse");
    let (nx, ny) = ((edge.point.x - 151.0) / 50.0, (edge.point.y - 131.0) / 30.0);
    assert!(
        (nx.hypot(ny) - 1.0).abs() < 0.01,
        "on the ellipse: {edge:?}"
    );
    assert_eq!(
        on.iter()
            .filter(|c| c.kind == SnapKind::SegmentCenterline)
            .count(),
        1
    );
}

#[test]
fn a_line_gives_endpoints_and_its_middle_and_a_polyline_gives_nodes() {
    let mut s = session();
    let l = s
        .add_markup(0, &line((50.0, 600.0), (150.0, 600.0)))
        .unwrap();
    assert!(has(
        &at(&s, 51.0, 601.0),
        SnapKind::Endpoint,
        50.0,
        600.0,
        l
    ));
    assert!(has(
        &at(&s, 100.0, 601.0),
        SnapKind::Midpoint,
        100.0,
        600.0,
        l
    ));

    let p = s
        .add_markup(
            0,
            &MarkupSpec::PolyLine {
                vertices: vec![(300.0, 600.0), (340.0, 640.0), (380.0, 600.0)],
                color: Color::Gray(0.0),
                width: 1.0,
                endings: (LineEnding::None, LineEnding::None),
                interior: None,
            },
        )
        .unwrap();
    assert!(has(&at(&s, 340.0, 641.0), SnapKind::Node, 340.0, 640.0, p));
    assert!(has(
        &at(&s, 300.0, 601.0),
        SnapKind::Endpoint,
        300.0,
        600.0,
        p
    ));
    // Open: no closing edge from the last vertex back to the first.
    assert!(at(&s, 340.0, 600.0).is_empty());
}

#[test]
fn a_polygon_closes_and_an_ink_stroke_gives_only_its_ends_and_a_point_on_it() {
    let mut s = session();
    let poly = s
        .add_markup(
            0,
            &MarkupSpec::Polygon {
                vertices: vec![(300.0, 600.0), (340.0, 640.0), (380.0, 600.0)],
                border: Some(Color::Gray(0.0)),
                interior: None,
                width: 1.0,
            },
        )
        .unwrap();
    assert!(has(
        &at(&s, 340.0, 601.0),
        SnapKind::Midpoint,
        340.0,
        600.0,
        poly
    ));
    assert!(has(
        &at(&s, 300.0, 601.0),
        SnapKind::Node,
        300.0,
        600.0,
        poly
    ));

    let ink = s
        .add_markup(
            0,
            &MarkupSpec::Ink {
                strokes: vec![vec![(60.0, 300.0), (80.0, 300.0), (100.0, 300.0)]],
                color: Color::Gray(0.0),
                width: 1.0,
            },
        )
        .unwrap();
    assert!(has(
        &at(&s, 60.0, 301.0),
        SnapKind::Endpoint,
        60.0,
        300.0,
        ink
    ));
    let mid = at(&s, 80.0, 301.0);
    assert!(has(&mid, SnapKind::SegmentCenterline, 80.0, 300.0, ink));
    assert!(mid.iter().all(|c| c.kind == SnapKind::SegmentCenterline));
}

#[test]
fn hidden_excluded_and_layer_hidden_markups_and_ce_dimensions_give_nothing() {
    let mut s = session();
    let id = s.add_markup(0, &square(0.0)).unwrap();
    assert!(!at(&s, 300.0, 400.0).is_empty());

    // Named in `exclude`.
    let graph = s.graph();
    let page = s.pages().unwrap()[0].id;
    let none = BTreeSet::new();
    let exclude: BTreeSet<ObjId> = [id].into();
    assert!(pdfcer_core::vector::markup_snap_geometry(&graph, page, &none, &exclude).is_empty());
    assert_eq!(
        pdfcer_core::vector::markup_snap_geometry(&graph, page, &none, &none).len(),
        1
    );

    // /F Hidden.
    s.set_annotation_flags(id, AnnotFlags(AnnotFlags::HIDDEN | 4))
        .unwrap();
    assert!(at(&s, 300.0, 400.0).is_empty());

    // A ce dimension is a /Line annotation and is left out.
    let mut s = session();
    s.add_dimension(
        0,
        DEFAULT_GROUP_ID,
        DimensionKind::Linear {
            a: Point::new(100.0, 100.0),
            b: Point::new(200.0, 100.0),
            constraint: AxisConstraint::Horizontal,
            offset: 0.0,
            text_along: 0.0,
            extension_gap: [None; 2],
        },
    )
    .unwrap();
    assert!(s.markup_snap_geometry(0, &none).unwrap().is_empty());
    assert!(s.markup_snap_geometry(9, &none).is_err());
}

#[test]
fn page_paths_and_markups_answer_one_query_in_one_order() {
    let mut s = session();
    let id = s
        .add_markup(0, &line((72.0, 500.0), (172.0, 500.0)))
        .unwrap();
    let markups = s.markup_snap_geometry(0, &BTreeSet::new()).unwrap();
    let model = s.page_objects(0).unwrap();
    let cfg = SnapConfig::new(3.0);
    let q = Point::new(73.0, 501.0);
    let both = snap_candidates_with_markups(q, &cfg, Some(&model), &markups);
    assert_eq!(both[0].kind, SnapKind::Endpoint);
    assert_eq!(both[0].source_annotation, Some(id));
    // With no markups the answer is `snap_candidates`' own.
    assert_eq!(
        snap_candidates_with_markups(q, &cfg, Some(&model), &[]),
        snap_candidates(q, &cfg, &model)
    );
    // A bad query gives nothing rather than panicking.
    let nan = Point::new(f64::NAN, 0.0);
    assert!(snap_candidates_with_markups(nan, &cfg, None, &markups).is_empty());
}

#[test]
fn a_markup_on_a_hidden_layer_gives_nothing() {
    let mut s = session();
    let id = s.add_markup(0, &square(0.0)).unwrap();
    let layer = s
        .add_layer("Notes", &pdfcer_core::edit::LayerEdit::new())
        .unwrap();
    s.set_annotation_layer(id, Some(layer)).unwrap();
    assert_eq!(
        s.markup_snap_geometry(0, &BTreeSet::new()).unwrap().len(),
        1
    );
    let hidden: BTreeSet<ObjId> = [layer].into();
    assert!(s.markup_snap_geometry(0, &hidden).unwrap().is_empty());
}

/// Entries `add_markup` does not write: `/RD`, `/CL`, `/LL`, a cloudy
/// border, a popup, NoView, and an annotation written directly in `/Annots`.
#[test]
fn rd_callout_leader_cloud_popup_noview_and_direct_entries_are_read_as_drawn() {
    let bodies: Vec<String> = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [4 0 R 5 0 R 6 0 R \
         7 0 R 8 0 R << /Type /Annot /Subtype /Line /Rect [50 740 150 760] \
         /L [50 750 150 750] >>] >>",
        "<< /Type /Annot /Subtype /FreeText /Rect [100 100 300 200] /RD [50 10 0 0] \
         /BS << /W 0 >> /CL [20 20 60 60 150 150] /DA (/Helv 10 Tf) /Contents (x) >>",
        "<< /Type /Annot /Subtype /Line /Rect [390 380 510 420] /L [400 400 500 400] \
         /LL 10 >>",
        "<< /Type /Annot /Subtype /Popup /Rect [10 600 60 650] >>",
        "<< /Type /Annot /Subtype /Square /Rect [400 100 500 200] /BS << /W 4 >> \
         /BE << /S /C /I 1 >> >>",
        "<< /Type /Annot /Subtype /Line /Rect [50 690 150 710] /L [50 700 150 700] /F 32 >>",
    ]
    .iter()
    .map(|b| (*b).to_string())
    .collect();
    let doc = Document::from_bytes(crate::block_layout::build_pdf(&bodies)).unwrap();
    let s = EditSession::new(doc);
    let (free, leader, cloud) = (ObjId::new(4, 0), ObjId::new(5, 0), ObjId::new(7, 0));
    assert_eq!(
        s.markup_snap_geometry(0, &BTreeSet::new()).unwrap().len(),
        3
    );

    // /RD is left, top, right, bottom: the box is [150 100 300 190].
    assert!(has(
        &at(&s, 150.0, 101.0),
        SnapKind::Node,
        150.0,
        100.0,
        free
    ));
    assert!(has(
        &at(&s, 300.0, 191.0),
        SnapKind::Node,
        300.0,
        190.0,
        free
    ));
    assert!(at(&s, 100.0, 200.0).is_empty());
    assert!(has(
        &at(&s, 20.0, 21.0),
        SnapKind::Endpoint,
        20.0,
        20.0,
        free
    ));
    assert!(has(&at(&s, 60.0, 61.0), SnapKind::Node, 60.0, 60.0, free));

    // Leader lines: the /L points, and nothing along /L.
    assert!(has(
        &at(&s, 400.0, 401.0),
        SnapKind::Endpoint,
        400.0,
        400.0,
        leader
    ));
    assert!(at(&s, 450.0, 400.0).is_empty());

    // Cloudy: the box itself, not inset by half the border width.
    assert!(has(
        &at(&s, 400.0, 101.0),
        SnapKind::Node,
        400.0,
        100.0,
        cloud
    ));

    // Popup, NoView and the direct entry.
    assert!(at(&s, 10.0, 600.0).is_empty());
    assert!(at(&s, 50.0, 700.0).is_empty());
    assert!(at(&s, 50.0, 750.0).is_empty());
}

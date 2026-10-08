//! Insert a node, and convert a node's or a segment's kind (G154): the
//! planners' exact bytes and shape preservation, and the session verbs on the
//! page and inside a scaled form.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::vector::decompose::Segment;
use pdfcer_core::vector::{
    Matrix, NoXObjects, NodeKind, PathObject, PlannedEdit, Point, SegmentKind, VectorEditError,
    VectorObject, decompose, plan_convert_node, plan_convert_segment, plan_insert_node,
};

fn path_of(content: &[u8]) -> (ContentStream, PathObject) {
    let cs = ContentStream::parse(content.to_vec()).unwrap();
    let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
    let Some(VectorObject::Path(p)) = model.objects.first() else {
        panic!("not a path: {}", String::from_utf8_lossy(content));
    };
    let p = p.clone();
    (cs, p)
}

fn text(plan: &PlannedEdit) -> String {
    String::from_utf8(plan.content.clone()).unwrap()
}

fn insert(content: &[u8], node: usize, t: f64) -> Result<PlannedEdit, VectorEditError> {
    let (cs, p) = path_of(content);
    plan_insert_node(&cs, &p, node, t)
}

fn node(content: &[u8], node: usize, kind: NodeKind) -> Result<PlannedEdit, VectorEditError> {
    let (cs, p) = path_of(content);
    plan_convert_node(&cs, &p, node, kind)
}

fn segment(content: &[u8], node: usize, kind: SegmentKind) -> Result<PlannedEdit, VectorEditError> {
    let (cs, p) = path_of(content);
    plan_convert_segment(&cs, &p, node, kind)
}

fn anchors(content: &[u8]) -> Vec<Point> {
    let (_, p) = path_of(content);
    p.subpaths.iter().flat_map(|s| s.anchors()).collect()
}

fn near(a: Point, x: f64, y: f64) -> bool {
    (a.x - x).abs() < 1e-6 && (a.y - y).abs() < 1e-6
}

fn cubic_at(p0: Point, c1: Point, c2: Point, p3: Point, t: f64) -> Point {
    let u = 1.0 - t;
    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    Point::new(
        w[0] * p0.x + w[1] * c1.x + w[2] * c2.x + w[3] * p3.x,
        w[0] * p0.y + w[1] * c1.y + w[2] * c2.y + w[3] * p3.y,
    )
}

/// The cubics of the first subpath, as (start, c1, c2, end).
fn cubics(content: &[u8]) -> Vec<(Point, Point, Point, Point)> {
    let (_, p) = path_of(content);
    let sp = &p.subpaths[0];
    let mut from = sp.start;
    let mut out = Vec::new();
    for seg in &sp.segments {
        if let Segment::Cubic { c1, c2, to } = *seg {
            out.push((from, c1, c2, to));
        }
        from = seg.end();
    }
    out
}

// --- insert ---------------------------------------------------------------

#[test]
fn inserting_on_a_line_splits_it_and_leaves_neighbours_verbatim() {
    let plan = insert(b"0 0 m 1.50 0 l 3.000 0 l S", 1, 0.5).unwrap();
    assert_eq!(text(&plan), "0 0 m 1.50 0 l 2.25 0 l 3.000 0 l S");
    assert!(plan.disclosures.is_empty());
}

#[test]
fn inserting_on_a_curve_keeps_its_shape() {
    let before = b"0 0 m 0 10 10 10 10 0 c S";
    let plan = insert(before, 0, 0.5).unwrap();
    let after = plan.content.clone();
    let pts = anchors(&after);
    assert_eq!(pts.len(), 3);
    assert!(near(pts[1], 5.0, 7.5), "{:?}", pts[1]);
    let (a, b) = (cubics(before), cubics(&after));
    assert_eq!(b.len(), 2, "{}", text(&plan));
    let (p0, c1, c2, p3) = a[0];
    for i in 1..10 {
        let s = f64::from(i) / 10.0;
        let (q0, d1, d2, q3) = b[0];
        let want = cubic_at(p0, c1, c2, p3, s / 2.0);
        let got = cubic_at(q0, d1, d2, q3, s);
        assert!(near(got, want.x, want.y), "first half at {s}");
        let (q0, d1, d2, q3) = b[1];
        let want = cubic_at(p0, c1, c2, p3, 0.5 + s / 2.0);
        let got = cubic_at(q0, d1, d2, q3, s);
        assert!(near(got, want.x, want.y), "second half at {s}");
    }
}

#[test]
fn inserting_on_a_closing_edge_adds_a_line_before_h() {
    let plan = insert(b"0 0 m 10 0 l 10 10 l h S", 2, 0.5).unwrap();
    assert_eq!(text(&plan), "0 0 m 10 0 l 10 10 l 5 5 l h S");
}

#[test]
fn inserting_on_a_rectangle_rewrites_it_and_says_so() {
    let plan = insert(b"0 0 10 10 re f", 0, 0.5).unwrap();
    assert_eq!(text(&plan), "0 0 m 5 0 l 10 0 l 10 10 l 0 10 l h f");
    assert_eq!(plan.disclosures.len(), 1);
    assert!(plan.disclosures[0].contains("rectangle"));
}

#[test]
fn inserting_maps_through_the_objects_matrix() {
    let plan = insert(b"2 0 0 2 0 0 cm 0 0 m 10 0 l S", 0, 0.5).unwrap();
    assert_eq!(text(&plan), "2 0 0 2 0 0 cm 0 0 m 5 0 l 10 0 l S");
}

#[test]
fn insert_refusals_name_the_problem() {
    for t in [0.0, 1.0, -0.5, f64::NAN, f64::INFINITY] {
        assert_eq!(
            insert(b"0 0 m 10 0 l S", 0, t).unwrap_err(),
            VectorEditError::InvalidSegmentParameter,
            "t = {t}"
        );
    }
    assert_eq!(
        insert(b"0 0 m 10 0 l S", 1, 0.5).unwrap_err(),
        VectorEditError::NoSegmentHere { index: 1 }
    );
    assert_eq!(
        insert(b"0 0 m 10 0 l S", 2, 0.5).unwrap_err(),
        VectorEditError::NodeOutOfRange { index: 2, count: 2 }
    );
}

#[test]
fn inserting_in_a_second_subpath_addresses_object_scoped_indices() {
    let plan = insert(b"0 0 m 10 0 l 0 5 m 10 5 l S", 2, 0.5).unwrap();
    assert_eq!(text(&plan), "0 0 m 10 0 l 0 5 m 5 5 l 10 5 l S");
}

// --- convert segment --------------------------------------------------------

#[test]
fn a_curve_becomes_a_line() {
    let plan = segment(b"0 0 m 0 10 10 10 10 0 c S", 0, SegmentKind::Line).unwrap();
    assert_eq!(text(&plan), "0 0 m 10 0 l S");
}

#[test]
fn converting_to_the_same_kind_changes_nothing() {
    let src = b"0 0 m 10 0 l S";
    let plan = segment(src, 0, SegmentKind::Line).unwrap();
    assert_eq!(plan.content, src);
    assert_eq!(plan.operators_touched, 0);
}

#[test]
fn reshaping_a_clipping_path_is_disclosed() {
    let plan = segment(b"0 0 m 10 0 l 10 10 l h W n", 0, SegmentKind::Curve).unwrap();
    assert!(plan.disclosures.iter().any(|d| d.contains("clipping")));
}

// --- convert node -----------------------------------------------------------

#[test]
fn smooth_between_two_lines_makes_collinear_curves() {
    let plan = node(b"0 0 m 10 10 l 20 0 l S", 1, NodeKind::Smooth).unwrap();
    assert_eq!(plan.disclosures.len(), 1, "{:?}", plan.disclosures);
    let c = cubics(&plan.content);
    assert_eq!(c.len(), 2, "{}", text(&plan));
    let (into, out) = (c[0].2, c[1].1);
    assert!(near(c[0].3, 10.0, 10.0), "the node itself does not move");
    assert!((into.y - 10.0).abs() < 1e-6 && (out.y - 10.0).abs() < 1e-6);
    assert!(into.x < 10.0 && out.x > 10.0);
}

#[test]
fn smooth_beside_a_line_aligns_the_curve_and_keeps_the_line() {
    let plan = node(b"0 0 m 10 0 l 10 10 20 10 20 0 c S", 1, NodeKind::Smooth).unwrap();
    assert_eq!(text(&plan), "0 0 m 10 0 l 20 0 20 10 20 0 c S");
    assert_eq!(plan.operators_touched, 1);
    assert!(plan.disclosures.is_empty());
}

#[test]
fn symmetric_equalises_the_handles() {
    let plan = node(
        b"0 0 m 0 10 6 0 10 0 c 18 0 20 10 20 0 c S",
        1,
        NodeKind::Symmetric,
    )
    .unwrap();
    let c = cubics(&plan.content);
    let at = c[0].3;
    let (a, b) = (c[0].2, c[1].1);
    let (la, lb) = (a.distance(at), b.distance(at));
    assert!(
        (la - 6.0).abs() < 1e-6 && (lb - 6.0).abs() < 1e-6,
        "{la} {lb}"
    );
    assert!((a.y - at.y).abs() < 1e-6 && (b.y - at.y).abs() < 1e-6);
}

#[test]
fn corner_between_lines_changes_nothing() {
    let src = b"0 0 m 10 10 l 20 0 l S";
    let plan = node(src, 1, NodeKind::Corner).unwrap();
    assert_eq!(plan.content, src);
    assert_eq!(plan.operators_touched, 0);
}

#[test]
fn smooth_at_an_open_end_is_refused() {
    assert_eq!(
        node(b"0 0 m 10 10 l 20 0 l S", 0, NodeKind::Smooth).unwrap_err(),
        VectorEditError::NodeHasOneSide { index: 0 }
    );
    assert!(node(b"0 0 m 10 10 l 20 0 l S", 2, NodeKind::Corner).is_ok());
}

#[test]
fn smoothing_the_first_node_of_a_closed_path_curves_the_closing_edge() {
    let src = b"0 0 m 10 0 l 10 10 l h S";
    let plan = node(src, 0, NodeKind::Smooth).unwrap();
    let out = text(&plan);
    assert!(out.ends_with(" 0 0 c h S"), "{out}");
    let pts = anchors(&plan.content);
    assert_eq!(pts.len(), 4, "the closing curve ends on the first node");
    assert!(near(pts[0], 0.0, 0.0) && near(pts[3], 0.0, 0.0));
    let c = cubics(&plan.content);
    let (into, out_h) = (c.last().unwrap().2, c[0].1);
    let cross = into.x * out_h.y - into.y * out_h.x;
    assert!(cross.abs() < 1e-6, "handles through the node are collinear");
}

// --- session verbs ----------------------------------------------------------

fn page_session(content: &str) -> EditSession {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>".to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    EditSession::new(Document::from_bytes(buf).unwrap())
}

fn page_anchors(s: &mut EditSession, object: usize) -> Vec<Point> {
    match &s.page_objects(0).unwrap().objects[object] {
        VectorObject::Path(p) => p.subpaths.iter().flat_map(|sp| sp.anchors()).collect(),
        other => panic!("not a path: {other:?}"),
    }
}

#[test]
fn the_page_verbs_edit_one_object_and_undo_as_one_entry() {
    let mut s = page_session("0 0 m 40 0 l S 0 10 m 40 10 l S");
    s.insert_node(0, 1, 0, 0.25).unwrap();
    let pts = page_anchors(&mut s, 1);
    assert_eq!(pts.len(), 3);
    assert!(near(pts[1], 10.0, 10.0), "{:?}", pts[1]);
    assert_eq!(
        page_anchors(&mut s, 0).len(),
        2,
        "the other object is untouched"
    );
    assert_eq!(s.undo(), Some(CommandKind::InsertNode));
    assert_eq!(page_anchors(&mut s, 1).len(), 2);

    s.convert_segment(0, 0, 0, SegmentKind::Curve).unwrap();
    assert_eq!(s.undo(), Some(CommandKind::ConvertSegment));
    s.convert_node(0, 0, 1, NodeKind::Corner).unwrap();
    assert_eq!(s.undo(), Some(CommandKind::ConvertNode));
}

#[test]
fn a_refused_page_verb_changes_nothing() {
    let mut s = page_session("0 0 m 40 0 l S");
    let err = s.insert_node(0, 0, 1, 0.5).unwrap_err();
    assert!(
        matches!(
            err,
            EditError::VectorEdit(VectorEditError::NoSegmentHere { index: 1 })
        ),
        "{err:?}"
    );
    assert_eq!(s.undo(), None);
}

fn form_session() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms-xobject/scaled-form-placement.pdf");
    EditSession::new(Document::load(&path).unwrap())
}

fn leaf_anchors(s: &mut EditSession, leaf: usize) -> Vec<Point> {
    match &s.page_objects(0).unwrap().leaves[leaf].object {
        VectorObject::Path(p) => p
            .page_subpaths()
            .iter()
            .flat_map(|sp| sp.anchors())
            .collect(),
        other => panic!("not a path: {other:?}"),
    }
}

/// The form is placed at `2 0 0 2 40 30 cm`, so a planner fed form space
/// instead of page space would put the new node somewhere else.
#[test]
fn inserting_inside_a_scaled_form_lands_on_the_drawn_edge() {
    let mut s = form_session();
    let before = leaf_anchors(&mut s, 0);
    assert!(near(before[0], 60.0, 50.0) && near(before[1], 100.0, 50.0));
    let outcome = s.insert_node_in_form(0, 0, 0, 0.5).unwrap();
    assert!(
        !outcome.disclosures.is_empty(),
        "the rectangle rewrite is disclosed"
    );
    let after = leaf_anchors(&mut s, 0);
    assert_eq!(after.len(), before.len() + 1);
    assert!(near(after[1], 80.0, 50.0), "{:?}", after[1]);
    assert_eq!(s.undo(), Some(CommandKind::InsertNode));
}

#[test]
fn the_form_convert_verbs_reach_the_form() {
    let mut s = form_session();
    s.convert_segment_in_form(0, 0, 0, SegmentKind::Curve)
        .unwrap();
    let after = leaf_anchors(&mut s, 0);
    assert!(near(after[1], 100.0, 50.0), "the corner stays put");
    assert_eq!(s.undo(), Some(CommandKind::ConvertSegment));
    s.convert_node_in_form(0, 0, 1, NodeKind::Smooth).unwrap();
    assert_eq!(s.undo(), Some(CommandKind::ConvertNode));
    assert!(matches!(
        s.insert_node_in_form(0, 9, 0, 0.5).unwrap_err(),
        EditError::FormLeafOutOfRange { .. }
    ));
}

//! Continue an open path from either end, and open a closed path at a node:
//! the planners' exact bytes, their refusals, and the session verbs on the
//! page and inside a scaled form.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::vector::{
    Matrix, NoXObjects, PathObject, PlannedEdit, Point, Segment, VectorEditError, VectorObject,
    decompose, plan_extend_path, plan_open_subpath,
};
use pdfcer_core::writer::SaveOptions;

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

fn line(x: f64, y: f64) -> Segment {
    Segment::Line {
        to: Point::new(x, y),
    }
}

fn curve(c1: (f64, f64), c2: (f64, f64), to: (f64, f64)) -> Segment {
    Segment::Cubic {
        c1: Point::new(c1.0, c1.1),
        c2: Point::new(c2.0, c2.1),
        to: Point::new(to.0, to.1),
    }
}

fn extend(content: &[u8], node: usize, more: &[Segment]) -> Result<PlannedEdit, VectorEditError> {
    let (cs, p) = path_of(content);
    plan_extend_path(&cs, &p, node, more)
}

fn open(content: &[u8], node: usize) -> Result<PlannedEdit, VectorEditError> {
    let (cs, p) = path_of(content);
    plan_open_subpath(&cs, &p, node)
}

/// Every subpath of the first object, as (closed, anchors).
fn shape(content: &[u8]) -> Vec<(bool, Vec<Point>)> {
    let (_, p) = path_of(content);
    p.subpaths
        .iter()
        .map(|s| (s.closed, s.anchors().collect()))
        .collect()
}

fn near(a: Point, x: f64, y: f64) -> bool {
    (a.x - x).abs() < 1e-6 && (a.y - y).abs() < 1e-6
}

// --- extend ---------------------------------------------------------------

#[test]
fn segments_added_at_the_end_follow_the_paths_own_and_touch_nothing_else() {
    let src = b"q 1 0 0 RG 0 0 m 10.50 0 l S Q 5 5 m 6 6 l S";
    let plan = extend(
        src,
        1,
        &[
            line(10.0, 5.0),
            curve((11.0, 6.0), (12.0, 7.0), (13.0, 8.0)),
        ],
    )
    .unwrap();
    assert_eq!(
        text(&plan),
        "q 1 0 0 RG 0 0 m 10.50 0 l 10 5 l 11 6 12 7 13 8 c S Q 5 5 m 6 6 l S"
    );
    assert_eq!(plan.operators_touched, 1);
    assert!(plan.disclosures.is_empty());
}

#[test]
fn segments_added_at_the_start_move_the_m_and_run_back_to_the_old_first_node() {
    let plan = extend(
        b"0 0 m 10 0 l S",
        0,
        &[line(0.0, 5.0), curve((1.0, 6.0), (2.0, 7.0), (3.0, 8.0))],
    )
    .unwrap();
    // Drawn outward the curve leaves (0,5) by handle (1,6); written backward
    // it leaves (3,8) by handle (2,7).
    assert_eq!(text(&plan), "3 8 m 2 7 1 6 0 5 c 0 0 l 10 0 l S");
    let after = shape(&plan.content);
    assert_eq!(after[0].1.len(), 4);
    assert!(near(after[0].1[0], 3.0, 8.0) && near(after[0].1[2], 0.0, 0.0));
}

#[test]
fn only_the_named_subpath_is_extended() {
    let plan = extend(b"0 0 m 10 0 l 20 0 m 30 0 l S", 1, &[line(10.0, 9.0)]).unwrap();
    assert_eq!(text(&plan), "0 0 m 10 0 l 10 9 l 20 0 m 30 0 l S");
    let plan = extend(b"0 0 m 10 0 l 20 0 m 30 0 l S", 2, &[line(20.0, 9.0)]).unwrap();
    assert_eq!(text(&plan), "0 0 m 10 0 l 20 9 m 20 0 l 30 0 l S");
}

#[test]
fn new_points_are_given_in_page_space_and_written_in_the_paths_own() {
    let plan = extend(
        b"q 2 0 0 2 10 10 cm 0 0 m 5 0 l S Q",
        1,
        &[line(30.0, 40.0)],
    )
    .unwrap();
    assert_eq!(text(&plan), "q 2 0 0 2 10 10 cm 0 0 m 5 0 l 10 15 l S Q");
}

#[test]
fn a_closed_path_has_no_end_to_continue_from() {
    let more = [line(1.0, 1.0)];
    for src in [
        b"0 0 m 10 0 l 10 10 l h S".as_slice(),
        b"0 0 10 10 re S",
        b"0 0 m 10 0 l 10 10 l s",
    ] {
        assert_eq!(
            extend(src, 0, &more).unwrap_err(),
            VectorEditError::SubpathClosed { index: 0 },
            "{}",
            String::from_utf8_lossy(src)
        );
    }
}

#[test]
fn extending_is_refused_by_name() {
    let src = b"0 0 m 10 0 l 20 0 l S";
    assert_eq!(
        extend(src, 1, &[line(1.0, 1.0)]).unwrap_err(),
        VectorEditError::NotAnEndNode { index: 1 }
    );
    assert_eq!(
        extend(src, 2, &[]).unwrap_err(),
        VectorEditError::NoSegmentsGiven
    );
    assert_eq!(
        extend(src, 2, &[line(f64::NAN, 0.0)]).unwrap_err(),
        VectorEditError::NonFinitePoint
    );
    assert!(matches!(
        extend(src, 9, &[line(1.0, 1.0)]).unwrap_err(),
        VectorEditError::NodeOutOfRange { index: 9, .. }
    ));
}

#[test]
fn extending_a_clipping_path_is_disclosed() {
    let plan = extend(b"0 0 m 10 0 l 10 10 l W n", 2, &[line(0.0, 10.0)]).unwrap();
    assert_eq!(plan.disclosures.len(), 1);
}

// --- open -----------------------------------------------------------------

#[test]
fn opening_at_the_first_node_rewrites_only_the_h() {
    let plan = open(b"0 0 m 10.50 0 l 10 10 l h S", 0).unwrap();
    assert_eq!(text(&plan), "0 0 m 10.50 0 l 10 10 l 0 0 l S");
    assert_eq!(plan.operators_touched, 1);
}

#[test]
fn opening_elsewhere_starts_and_ends_the_path_at_that_node() {
    let src = b"0 0 m 10 0 l 10 10 l h S";
    let plan = open(src, 2).unwrap();
    assert_eq!(text(&plan), "10 10 m 0 0 l 10 0 l 10 10 l S");
    let after = shape(&plan.content);
    assert!(!after[0].0);
    assert_eq!(after[0].1.len(), shape(src)[0].1.len() + 1);
}

#[test]
fn curves_keep_their_handles_when_the_path_is_rotated() {
    let plan = open(b"0 0 m 0 5 5 10 10 10 c 20 10 l h f", 1).unwrap();
    assert_eq!(text(&plan), "10 10 m 20 10 l 0 0 l 0 5 5 10 10 10 c f");
}

#[test]
fn a_path_whose_last_node_sits_on_its_first_gains_no_node() {
    let src = b"0 0 m 10 0 l 10 10 l 0 0 l h S";
    let plan = open(src, 0).unwrap();
    assert_eq!(text(&plan), "0 0 m 10 0 l 10 10 l 0 0 l  S");
    let plan = open(src, 1).unwrap();
    assert_eq!(text(&plan), "10 0 m 10 10 l 0 0 l 10 0 l S");
    assert_eq!(shape(&plan.content)[0].1.len(), shape(src)[0].1.len());
}

#[test]
fn a_rectangle_is_rewritten_as_lines_and_that_is_disclosed() {
    let plan = open(b"0 0 10 20 re S", 1).unwrap();
    assert_eq!(text(&plan), "10 0 m 10 20 l 0 20 l 0 0 l 10 0 l S");
    assert_eq!(plan.disclosures.len(), 1);
}

#[test]
fn a_painting_operator_that_would_close_the_path_again_is_replaced() {
    for (closing, open_form) in [("s", "S"), ("b", "B"), ("b*", "B*")] {
        let src = format!("0 0 m 10 0 l 10 10 l {closing}");
        let plan = open(src.as_bytes(), 0).unwrap();
        assert_eq!(
            text(&plan),
            format!("0 0 m 10 0 l 10 10 l 0 0 l {open_form}")
        );
        assert!(!shape(&plan.content)[0].0, "{closing}");
        let src = format!("0 0 m 10 0 l 10 10 l h {closing}");
        let plan = open(src.as_bytes(), 1).unwrap();
        assert_eq!(
            text(&plan),
            format!("10 0 m 10 10 l 0 0 l 10 0 l {open_form}")
        );
    }
}

#[test]
fn only_the_last_subpath_is_the_painting_operators_to_close() {
    let plan = open(b"0 0 m 10 0 l 10 10 l h 20 0 m 30 0 l 30 10 l s", 0).unwrap();
    assert_eq!(
        text(&plan),
        "0 0 m 10 0 l 10 10 l 0 0 l 20 0 m 30 0 l 30 10 l s"
    );
    let after = shape(&plan.content);
    assert!(!after[0].0 && after[1].0);
}

#[test]
fn a_subpath_that_relied_on_the_h_for_its_start_keeps_that_start() {
    let src = b"0 0 m 10 0 l 10 10 l h 5 5 l S";
    let before = shape(src);
    let plan = open(src, 1).unwrap();
    assert_eq!(text(&plan), "10 0 m 10 10 l 0 0 l 10 0 l 0 0 m 5 5 l S");
    let after = shape(&plan.content);
    assert_eq!(after[1].1, before[1].1);
}

#[test]
fn opening_is_refused_by_name() {
    assert_eq!(
        open(b"0 0 m 10 0 l S", 1).unwrap_err(),
        VectorEditError::SubpathAlreadyOpen { index: 1 }
    );
    assert_eq!(
        open(b"0 0 m 10 0 l 10 10 l h 5 5 l 9 9 l h S", 3).unwrap_err(),
        VectorEditError::SubpathStartsImplicitly { index: 3 }
    );
}

#[test]
fn an_opened_path_can_then_be_continued() {
    let plan = open(b"0 0 m 10 0 l 10 10 l h S", 0).unwrap();
    let again = extend(&plan.content, 3, &[line(-5.0, 0.0)]).unwrap();
    assert_eq!(text(&again), "0 0 m 10 0 l 10 10 l 0 0 l -5 0 l S");
}

// --- session ----------------------------------------------------------------

fn page_bytes(content: &str) -> Vec<u8> {
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
    buf
}

fn session(bytes: Vec<u8>) -> EditSession {
    EditSession::new(Document::from_bytes(bytes).unwrap())
}

fn saved(s: &mut EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

fn page_anchors(s: &mut EditSession, object: usize) -> Vec<Point> {
    match &s.page_objects(0).unwrap().objects[object] {
        VectorObject::Path(p) => p.subpaths.iter().flat_map(|sp| sp.anchors()).collect(),
        other => panic!("not a path: {other:?}"),
    }
}

#[test]
fn the_page_verbs_edit_one_object_survive_a_save_and_undo_to_the_original_bytes() {
    let bytes = page_bytes("0 0 m 40 0 l S 0 10 m 40 10 l 40 30 l h S");
    let untouched = saved(&mut session(bytes.clone()));
    let mut s = session(bytes);

    s.extend_path(0, 0, 1, &[line(40.0, 5.0)]).unwrap();
    s.open_subpath(0, 1, 1).unwrap();
    let mut reopened = session(saved(&mut s));
    let first = page_anchors(&mut reopened, 0);
    assert_eq!(first.len(), 3);
    assert!(near(first[2], 40.0, 5.0), "{:?}", first[2]);
    let second = page_anchors(&mut reopened, 1);
    assert_eq!(second.len(), 4);
    assert!(near(second[0], 40.0, 10.0) && near(second[3], 40.0, 10.0));

    assert_eq!(s.undo(), Some(CommandKind::OpenSubpath));
    assert_eq!(s.undo(), Some(CommandKind::ExtendPath));
    assert_eq!(saved(&mut s), untouched);
}

#[test]
fn a_refused_page_verb_changes_nothing() {
    let mut s = session(page_bytes("0 0 m 40 0 l 40 40 l h S"));
    let err = s.extend_path(0, 0, 0, &[line(1.0, 1.0)]).unwrap_err();
    assert!(
        matches!(
            err,
            EditError::VectorEdit(VectorEditError::SubpathClosed { index: 0 })
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
fn the_form_verbs_reach_the_form_and_take_page_space_points() {
    let mut s = form_session();
    let before = leaf_anchors(&mut s, 0);
    assert!(matches!(
        s.extend_path_in_form(0, 0, 0, &[line(1.0, 1.0)])
            .unwrap_err(),
        EditError::VectorEdit(VectorEditError::SubpathClosed { .. })
    ));
    let outcome = s.open_subpath_in_form(0, 0, 0).unwrap();
    assert!(
        !outcome.disclosures.is_empty(),
        "the rectangle rewrite is disclosed"
    );
    let opened = leaf_anchors(&mut s, 0);
    assert_eq!(opened.len(), before.len() + 1);
    assert!(near(opened[4], before[0].x, before[0].y));

    s.extend_path_in_form(0, 0, 4, &[line(50.0, 44.0)]).unwrap();
    let extended = leaf_anchors(&mut s, 0);
    assert_eq!(extended.len(), opened.len() + 1);
    assert!(near(extended[5], 50.0, 44.0), "{:?}", extended[5]);

    assert_eq!(s.undo(), Some(CommandKind::ExtendPath));
    assert_eq!(s.undo(), Some(CommandKind::OpenSubpath));
    assert_eq!(leaf_anchors(&mut s, 0), before);
    assert!(matches!(
        s.open_subpath_in_form(0, 9, 0).unwrap_err(),
        EditError::FormLeafOutOfRange { .. }
    ));
}

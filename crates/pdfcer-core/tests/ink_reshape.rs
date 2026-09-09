//! # An `/Ink` stroke had readable vertices and no way to edit them
//!
//! ## The operator, verbatim
//!
//! > *"Also the draw a line that follows the pointer tool — I can't edit the
//! > nodes that make it."*
//!
//! That is the freehand tool, which authors an `/Ink` annotation.
//!
//! ## What was true before `Pass 278.0`
//!
//! `Annotation::ink_list` was **readable**, and `reshape_annotation` /
//! `move_annotation_vertex` / `insert_annotation_vertex` /
//! `remove_annotation_vertex` all **refused `/Ink` by name** — with a reason
//! argued from Acrobat: *"an `/InkList` stroke is a recorded pen trace, and
//! Acrobat has never offered per-point ink editing at any version"*.
//!
//! The consuming shell asked the right question about that: **is this a
//! decision or a not-yet, because from here they look identical?**
//!
//! ## The answer
//!
//! It was a decision, and it is overturned. **Parity with Acrobat is this
//! project's floor, not its ceiling.** Every other markup the operator can
//! draw — polygon, polyline, line, cloud — he can nudge; the one he draws
//! fastest and least precisely was the one he could not.
//!
//! ## Why new verbs instead of widening `VertexEdit`
//!
//! `/InkList` (§12.5.6.13, Table 182) is an array **of** arrays, so a point
//! needs a `(stroke, point)` address and `VertexEdit` carries one index. It is
//! also a **relaxation** of a refusal, and decision 144's corollary is that a
//! relaxation gets a new name while only a tightening may be added in place:
//! tightening turns silent wrong answers into refusals, relaxing turns
//! refusals into silent answers.
//!
//! ## Three objections the requester raised, and what measurement says
//!
//! 1. *"`/InkList` is a list of lists"* — correct, and the reason for the new
//!    address.
//! 2. *"a freehand stroke has hundreds of points, and per-point anchors on a
//!    400-point stroke are unusable as a UI"* — correct, and the reason
//!    `ReplaceStroke` and `MoveStroke` exist beside the point verbs. What a
//!    shell draws is its business; the engine offers both grains.
//! 3. *"the `/AP` is a smoothed curve, so moving one point moves a length of
//!    curve on both sides"* — **not true of pdfcer.** `annot_author::ink`
//!    emits `m`/`l` — a polyline. §12.5.6.13 leaves the join
//!    "implementation-dependent", so both readings conform, and a shell that
//!    previews with a polyline is exactly right rather than approximately
//!    right. `a_point_move_changes_exactly_two_segments` is the measurement.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot_author::{Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, InkEdit, InkEditKind};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::vector::Point;
use pdfcer_core::writer::SaveOptions;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/annot/demo-annotated.pdf")
}

/// Two strokes: a three-point one and a two-point one — the second sitting
/// exactly at the point floor, so a remove on it must be refused while the
/// same remove on the first succeeds.
fn ink() -> MarkupSpec {
    MarkupSpec::Ink {
        strokes: vec![
            vec![(10.0, 10.0), (20.0, 30.0), (25.0, 35.0)],
            vec![(40.0, 40.0), (50.0, 60.0)],
        ],
        color: Color::Gray(0.0),
        width: 2.0,
    }
}

fn with_ink() -> (EditSession, ObjId) {
    let mut s = EditSession::new(Document::load(&fixture()).expect("fixture parses"));
    let id = s.add_markup(0, &ink()).expect("author the ink");
    (s, id)
}

/// Save incrementally and re-parse — the assertions are about the bytes, not
/// about the session's opinion of them.
fn reload(s: &EditSession) -> Document {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    Document::from_bytes(bytes).expect("re-parse")
}

/// The `/InkList` as it is in the saved file: a list of point lists.
fn ink_list(doc: &Document, id: ObjId) -> Vec<Vec<(f64, f64)>> {
    let Object::Dict(d) = &doc.get(id).expect("annotation present").value else {
        panic!("not a dict")
    };
    let view = doc.view();
    let Object::Array(strokes) = view.resolve(d.get(b"InkList").expect("/InkList")) else {
        panic!("/InkList is not an array")
    };
    strokes
        .iter()
        .map(|s| {
            let Object::Array(nums) = view.resolve(s) else {
                panic!("a stroke is not an array")
            };
            nums.chunks_exact(2)
                .map(|c| {
                    (
                        view.resolve(&c[0]).as_number().expect("x"),
                        view.resolve(&c[1]).as_number().expect("y"),
                    )
                })
                .collect()
        })
        .collect()
}

/// The `/AP` `/N` stream of the annotation, from the saved bytes.
fn ap_text(doc: &Document, id: ObjId) -> String {
    let Object::Dict(d) = &doc.get(id).expect("present").value else {
        panic!("not a dict")
    };
    let view = doc.view();
    let Some(Object::Dict(ap)) = d.get(b"AP").map(|o| view.resolve(o)) else {
        panic!("no /AP")
    };
    let Object::Stream(st) = view.resolve(ap.get(b"N").expect("/N")) else {
        panic!("/N is not a stream")
    };
    String::from_utf8_lossy(st.data_span.slice(doc.bytes()).expect("bytes")).into_owned()
}

fn rect(doc: &Document, id: ObjId) -> Vec<f64> {
    let Object::Dict(d) = &doc.get(id).expect("present").value else {
        panic!("not a dict")
    };
    let view = doc.view();
    let Object::Array(r) = view.resolve(d.get(b"Rect").expect("/Rect")) else {
        panic!("/Rect is not an array")
    };
    r.iter()
        .map(|o| view.resolve(o).as_number().expect("number"))
        .collect()
}

// ---------------------------------------------------------------- the verbs

/// ★★★ THE OPERATOR'S ASK: a node of a freehand stroke moves.
#[test]
fn a_point_of_a_stroke_moves() {
    let (mut s, id) = with_ink();
    let out = s.move_ink_point(id, 0, 1, 5.0, -7.0).expect("move it");
    assert_eq!(out.forecast.edit, InkEditKind::PointMoved);
    assert_eq!(out.forecast.stroke, 0);

    let doc = reload(&s);
    let strokes = ink_list(&doc, id);
    assert_eq!(strokes[0][1], (25.0, 23.0), "the dragged point moved");
    assert_eq!(
        strokes[0][0],
        (10.0, 10.0),
        "its neighbour did not move with it"
    );
    assert_eq!(
        strokes[1],
        vec![(40.0, 40.0), (50.0, 60.0)],
        "so did stroke 1 stay put"
    );
}

/// The second index space is real: the same point index in a different stroke
/// is a different point.
///
/// ★ Without this, every assertion in this file could be satisfied by an
/// implementation that ignored the stroke index and always edited stroke 0.
#[test]
fn the_stroke_index_selects_which_stroke() {
    let (mut s, id) = with_ink();
    s.move_ink_point(id, 1, 0, 100.0, 100.0).expect("move it");
    let doc = reload(&s);
    let strokes = ink_list(&doc, id);
    assert_eq!(strokes[1][0], (140.0, 140.0), "stroke 1's point 0 moved");
    assert_eq!(
        strokes[0][0],
        (10.0, 10.0),
        "stroke 0's point 0 is a DIFFERENT point and must not have moved"
    );
}

#[test]
fn a_point_is_inserted_after_the_named_one() {
    let (mut s, id) = with_ink();
    let out = s
        .insert_ink_point(id, 0, 0, Point { x: 15.0, y: 20.0 })
        .expect("insert");
    assert_eq!(out.forecast.stroke_points_before, 3);
    assert_eq!(out.forecast.stroke_points_after, 4);
    let doc = reload(&s);
    assert_eq!(
        ink_list(&doc, id)[0],
        vec![(10.0, 10.0), (15.0, 20.0), (20.0, 30.0), (25.0, 35.0)],
        "the new point sits between the named one and its successor"
    );
}

/// Appending past the end extends the stroke — "keep drawing where I
/// stopped". An ink stroke is open, so unlike a `/Polygon` there is no
/// closing segment for this to split.
#[test]
fn inserting_after_the_last_point_extends_the_stroke() {
    let (mut s, id) = with_ink();
    s.insert_ink_point(id, 0, 2, Point { x: 60.0, y: 80.0 })
        .expect("extend");
    let doc = reload(&s);
    assert_eq!(*ink_list(&doc, id)[0].last().expect("points"), (60.0, 80.0));
}

#[test]
fn a_point_is_removed() {
    let (mut s, id) = with_ink();
    let out = s.remove_ink_point(id, 0, 1).expect("remove");
    assert_eq!(out.forecast.points_before, 5);
    assert_eq!(out.forecast.points_after, 4);
    let doc = reload(&s);
    assert_eq!(ink_list(&doc, id)[0], vec![(10.0, 10.0), (25.0, 35.0)]);
}

/// The slice the requester actually asked for: replace one stroke wholesale,
/// so a shell can offer "reshape this stroke" without pretending 400 anchors
/// are a control.
#[test]
fn a_whole_stroke_is_replaced_and_the_others_are_not() {
    let (mut s, id) = with_ink();
    let out = s
        .replace_ink_stroke(id, 0, vec![(1.0, 1.0), (2.0, 2.0), (3.0, 3.0), (4.0, 4.0)])
        .expect("replace");
    assert_eq!(out.forecast.edit, InkEditKind::StrokeReplaced);
    assert_eq!(out.forecast.stroke_points_after, 4);
    let doc = reload(&s);
    let strokes = ink_list(&doc, id);
    assert_eq!(strokes[0].len(), 4);
    assert_eq!(
        strokes[1],
        vec![(40.0, 40.0), (50.0, 60.0)],
        "the untouched stroke is untouched"
    );
}

#[test]
fn a_whole_stroke_translates() {
    let (mut s, id) = with_ink();
    s.move_ink_stroke(id, 1, 10.0, 10.0).expect("move stroke");
    let doc = reload(&s);
    let strokes = ink_list(&doc, id);
    assert_eq!(strokes[1], vec![(50.0, 50.0), (60.0, 70.0)]);
    assert_eq!(
        strokes[0][0],
        (10.0, 10.0),
        "move_ink_stroke moves ONE stroke; move_annotation is the verb for all of them"
    );
}

/// ★★ REMOVES STROKE **0**, DELIBERATELY, AND THE FIRST VERSION REMOVED
/// STROKE 1.
///
/// With the LAST stroke removed, `next.get(stroke_index)` is `None` and the
/// naive report answers `0` by accident — so a sabotage that dropped the
/// "the stroke is gone, not empty" branch **stayed green**. Removing the
/// FIRST stroke makes the second slide into index 0, where the naive report
/// would name a two-point length belonging to a stroke the operator never
/// touched: a plausible number about the wrong subject.
#[test]
fn a_whole_stroke_is_removed() {
    let (mut s, id) = with_ink();
    let out = s.remove_ink_stroke(id, 0).expect("remove stroke");
    assert_eq!(out.forecast.strokes_before, 2);
    assert_eq!(out.forecast.strokes_after, 1);
    assert_eq!(out.forecast.stroke_points_before, 3);
    assert_eq!(
        out.forecast.stroke_points_after, 0,
        "the stroke is GONE, not empty -- and stroke 1 (two points) has just slid \
         into index 0, so a report reading that index would say 2"
    );
    let doc = reload(&s);
    let strokes = ink_list(&doc, id);
    assert_eq!(strokes.len(), 1);
    assert_eq!(
        strokes[0],
        vec![(40.0, 40.0), (50.0, 60.0)],
        "the SURVIVING stroke is the one that was not named"
    );
}

// ------------------------------------------------------------- the refusals

#[test]
fn a_stroke_index_past_the_end_is_refused_by_its_own_name() {
    let (mut s, id) = with_ink();
    let err = s.move_ink_point(id, 7, 0, 1.0, 1.0).expect_err("refused");
    let EditError::InkStrokeIndexOutOfRange { index, count, .. } = err else {
        panic!("expected the STROKE range refusal, got {err:?}");
    };
    assert_eq!((index, count), (7, 2));
}

/// ★ The two index spaces must be told apart. A single "index out of range"
/// would send a caller to audit the wrong list.
#[test]
fn a_point_index_past_the_end_names_the_point_space_and_the_stroke() {
    let (mut s, id) = with_ink();
    let err = s.move_ink_point(id, 1, 9, 1.0, 1.0).expect_err("refused");
    let EditError::InkPointIndexOutOfRange {
        stroke,
        index,
        count,
        ..
    } = err
    else {
        panic!("expected the POINT range refusal, got {err:?}");
    };
    assert_eq!(
        (stroke, index, count),
        (1, 9, 2),
        "the stroke existed; the point did not, and the message must say which is which"
    );
}

/// A one-point stroke is not a path (§12.5.6.13), and the refusal names the
/// verb that does what the caller probably meant.
#[test]
fn removing_the_second_of_two_points_is_refused_at_the_floor() {
    let (mut s, id) = with_ink();
    let err = s.remove_ink_point(id, 1, 0).expect_err("floor");
    let EditError::InkStrokeWouldBreachPointFloor { stroke, count, .. } = &err else {
        panic!("expected the floor refusal, got {err:?}");
    };
    assert_eq!((*stroke, *count), (1, 2));
    assert!(
        err.to_string().contains("RemoveStroke"),
        "the refusal must name the verb that removes the whole stroke: {err}"
    );
}

/// ★ The same remove, one stroke over, SUCCEEDS. Without this the floor test
/// would pass against an implementation that refused every point removal.
#[test]
fn the_same_remove_on_a_three_point_stroke_succeeds() {
    let (mut s, id) = with_ink();
    s.remove_ink_point(id, 0, 0)
        .expect("three points can spare one");
}

#[test]
fn removing_the_last_usable_stroke_is_refused() {
    let (mut s, id) = with_ink();
    s.remove_ink_stroke(id, 1)
        .expect("the first removal is fine");
    let err = s.remove_ink_stroke(id, 0).expect_err("not the last one");
    assert!(
        matches!(err, EditError::InkWouldBeEmpty { .. }),
        "got {err:?}"
    );
    assert!(
        err.to_string().contains("delete_annotation"),
        "an /Ink with nothing to draw is not the same act as deleting the comment, \
         and the refusal must say where that act lives: {err}"
    );
}

/// A hand-written one-page PDF whose `/Ink` carries a drawable two-point
/// stroke **and** a degenerate one-point stroke.
///
/// ★ This fixture exists to make one comment in `apply_ink_edit` a
/// measurement rather than an assertion. `RemoveStroke` counts what would
/// REMAIN and be drawable, not `strokes.len() > 1`; the two answers differ on
/// exactly this file, which no verb of pdfcer's can author.
fn pdf_with_a_degenerate_ink_stroke() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> /Annots [4 0 R] >>",
        "<< /Type /Annot /Subtype /Ink /Rect [5 5 45 45] \
         /InkList [[10 10 20 20] [30 30]] /C [0 0 0] >>",
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

/// ★★ `strokes.len() > 1` WOULD HAVE PASSED HERE, and left an `/Ink` that
/// draws nothing.
///
/// Two strokes are present, so the naive count says "one will remain". Only
/// one of them is a path; removing it leaves a single one-point stroke, which
/// pdfcer's renderer draws as nothing and other readers draw as a dot or as
/// nothing. The refusal counts what would remain **and be drawable**.
#[test]
fn removing_the_only_drawable_stroke_is_refused_even_when_another_survives() {
    let mut s = EditSession::new(
        Document::from_bytes(pdf_with_a_degenerate_ink_stroke()).expect("fixture parses"),
    );
    let id = ObjId::new(4, 0);
    let err = s
        .remove_ink_stroke(id, 0)
        .expect_err("nothing drawable would remain");
    assert!(
        matches!(err, EditError::InkWouldBeEmpty { .. }),
        "a second stroke exists but is not a path; got {err:?}"
    );
    // And the degenerate one can still be removed, because the drawable one
    // survives it — the refusal is about the RESULT, not about the count.
    s.remove_ink_stroke(id, 1)
        .expect("removing the degenerate stroke leaves a drawable one");
}

/// Replacing a stroke with a single point is the same degenerate result as
/// removing points down to one, and is refused by the same name so a caller
/// does not have to learn two.
#[test]
fn replacing_a_stroke_with_one_point_is_refused_at_the_same_floor() {
    let (mut s, id) = with_ink();
    let err = s
        .replace_ink_stroke(id, 0, vec![(1.0, 1.0)])
        .expect_err("one point is not a path");
    assert!(
        matches!(err, EditError::InkStrokeWouldBreachPointFloor { .. }),
        "got {err:?}"
    );
}

#[test]
fn a_non_finite_result_is_refused_rather_than_written() {
    let (mut s, id) = with_ink();
    let err = s
        .move_ink_point(id, 0, 0, f64::INFINITY, 0.0)
        .expect_err("not placeable");
    assert!(
        matches!(err, EditError::AnnotationVertexNotPlaceable { .. }),
        "got {err:?}"
    );
}

/// The ink verbs on a `/Polygon` say so, and name where to go.
#[test]
fn an_ink_verb_on_a_polygon_is_refused_by_name() {
    let mut s = EditSession::new(Document::load(&fixture()).expect("fixture parses"));
    let id = s
        .add_markup(
            0,
            &MarkupSpec::Polygon {
                vertices: vec![(10.0, 10.0), (50.0, 10.0), (30.0, 40.0)],
                border: Some(Color::Gray(0.0)),
                interior: None,
                width: 1.0,
            },
        )
        .expect("author a polygon");
    let err = s.move_ink_point(id, 0, 0, 1.0, 1.0).expect_err("not ink");
    let EditError::InkVerbOnNonInk { subtype, .. } = &err else {
        panic!("expected InkVerbOnNonInk, got {err:?}");
    };
    assert_eq!(subtype, "Polygon");
    assert!(
        err.to_string().contains("reshape_annotation"),
        "name the verb that DOES address a polygon: {err}"
    );
}

/// And the reverse direction: `reshape_annotation` still refuses `/Ink`, and
/// its refusal is now a signpost rather than a limitation.
#[test]
fn reshape_annotation_still_refuses_ink_and_names_the_new_verbs() {
    let (mut s, id) = with_ink();
    let err = s
        .move_annotation_vertex(id, 0, 1.0, 1.0)
        .expect_err("refused");
    let msg = err.to_string();
    assert!(
        msg.contains("reshape_ink") || msg.contains("move_ink_point"),
        "the old refusal named Acrobat and a limitation; the new one must name the verb: {msg}"
    );
    assert!(
        !msg.contains("Acrobat has never offered"),
        "parity is the floor, not the ceiling, and this sentence is superseded: {msg}"
    );
}

// ------------------------------------------------- the geometry consequences

/// ★★ THE OBJECTION THAT TURNED OUT TO BE FALSE FOR pdfcer.
///
/// The requester expected the `/AP` to be a smoothed curve, so that moving one
/// point would move "a length of curve on both sides of it" and a shell's
/// preview would be approximate. `annot_author::ink` emits `m` then `l` — a
/// polyline — so a point drag moves exactly the two segments either side, and
/// a polyline preview is exact.
///
/// §12.5.6.13 leaves the join "implementation-dependent" ("straight lines or
/// curves"), so this is a conforming choice rather than a shortcut, and it is
/// worth pinning: a future change to spline the strokes would silently make
/// every shell's preview wrong.
#[test]
fn a_point_move_changes_exactly_two_segments() {
    let (mut s, id) = with_ink();
    let before = ap_text(&reload(&s), id);
    assert_eq!(
        before.matches(" c\n").count(),
        0,
        "pdfcer draws an /InkList as a POLYLINE; a curve operator here means this \
         test is measuring a different renderer"
    );
    let segments_before = before.matches(" l\n").count();

    s.move_ink_point(id, 0, 1, 5.0, 5.0).expect("move");
    let after = ap_text(&reload(&s), id);
    assert_eq!(
        after.matches(" l\n").count(),
        segments_before,
        "moving a point changes where two segments GO, not how many there are"
    );
}

/// The `/Rect` is derived, not preserved: a point dragged outside the old box
/// must not be clipped by §12.5.5's placement.
#[test]
fn the_rect_grows_to_contain_a_point_dragged_outside_it() {
    let (mut s, id) = with_ink();
    let before = rect(&reload(&s), id);
    let out = s.move_ink_point(id, 0, 0, -30.0, 0.0).expect("drag left");
    let after = rect(&reload(&s), id);
    assert!(
        after[0] < before[0],
        "the box must follow the stroke: before {before:?}, after {after:?}"
    );
    assert!(
        (out.forecast.rect_after.llx - after[0]).abs() < 1e-6,
        "the report's rect_after must be the one that was written"
    );
}

// ------------------------------------------------------------- the preview

/// The preview answers the same question the verb does, through the same code.
#[test]
fn the_preview_agrees_with_the_verb_and_stages_nothing() {
    let (mut s, id) = with_ink();
    let edit = InkEdit::MovePoint {
        stroke: 0,
        point: 1,
        dx: 5.0,
        dy: -7.0,
    };
    let forecast = s.reshape_ink_preview(id, &edit).expect("preview");
    let before = ink_list(&reload(&s), id);

    let done = s.reshape_ink(id, &edit, None).expect("do it");
    assert_eq!(
        forecast, done.forecast,
        "a preview that can disagree with its verb is a control that enables the wrong thing"
    );
    assert_ne!(
        before,
        ink_list(&reload(&s), id),
        "and the preview must not have been what changed it"
    );
}

/// A preview of a refused edit refuses identically.
#[test]
fn the_preview_refuses_what_the_verb_refuses() {
    let (s, id) = with_ink();
    let edit = InkEdit::RemovePoint {
        stroke: 1,
        point: 0,
    };
    assert!(
        matches!(
            s.reshape_ink_preview(id, &edit),
            Err(EditError::InkStrokeWouldBreachPointFloor { .. })
        ),
        "the preview is what a shell greys a menu item on"
    );
}

/// ★ The disclosure a shell can act on BEFORE the drag: was this artwork ours?
///
/// pdfcer authored this one, so re-baking is lossless and
/// `appearance_was_pdfces` says so. On a stroke another producer drew — and
/// smoothed — the same edit straightens it, which is exactly the thing an
/// operator should be told once rather than discover.
#[test]
fn the_preview_says_whether_the_artwork_is_ours() {
    let (s, id) = with_ink();
    let f = s
        .reshape_ink_preview(
            id,
            &InkEdit::MovePoint {
                stroke: 0,
                point: 0,
                dx: 1.0,
                dy: 1.0,
            },
        )
        .expect("preview");
    assert!(
        f.appearance_was_pdfces,
        "pdfcer drew this ink in this session; claiming otherwise is the false-provenance \
         defect that cost a sibling verb a whole Pass"
    );
}

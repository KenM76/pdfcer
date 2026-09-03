//! Moving a curve's **shaping handles** — the operation that changes a
//! curve's shape rather than the points it passes through.
//!
//! # Why this is a separate capability from node dragging
//!
//! `plan_move_node` moves ON-CURVE anchors. Every point it can move is a point
//! the curve goes through, so with only that, a curve's *curvature* could not
//! be edited at all: you could drag the ends of an arc anywhere on the page
//! and never change how it bows between them.
//!
//! # The part that is easy to get wrong
//!
//! Two of the three cubic spellings omit a control point (§8.5.2.1, Table 59):
//!
//! | operator | operands | first control | second control |
//! |---|---|---|---|
//! | `c` | `x1 y1 x2 y2 x3 y3` | `(x1,y1)` | `(x2,y2)` |
//! | `v` | `x2 y2 x3 y3` | **the current point** | `(x2,y2)` |
//! | `y` | `x1 y1 x3 y3` | `(x1,y1)` | **the endpoint** |
//!
//! So `v` and `y` each have one handle whose entire definition is "equal to
//! that other point". It cannot both stay implicit and move, and the operator
//! has to be re-spelled as `c`. The failure mode is a promotion that puts the
//! operands in the wrong order — which still parses, still renders, and draws
//! a different curve. Every test here that touches `v`/`y` therefore checks
//! the resulting CONTROL POINTS through the decomposition, not the bytes.
//!
//! `v` and `y` are also, per the spec RAG, "the single most-confused pair in
//! the operator set", which is a good reason not to trust a reading of them
//! that only one test agrees with.

use pdfcer_core::content::ContentStream;
use pdfcer_core::vector::edit::{Handle, plan_move_handle};
use pdfcer_core::vector::{
    Matrix, NoXObjects, PathObject, Point, Segment, VectorEditError, VectorObject, decompose,
};

fn parse(bytes: &[u8]) -> ContentStream {
    ContentStream::parse(bytes.to_vec()).expect("fixture parses")
}

fn only_path(cs: &ContentStream) -> PathObject {
    let model = decompose(cs, Matrix::IDENTITY, &NoXObjects);
    model
        .objects
        .iter()
        .find_map(|o| match o {
            VectorObject::Path(p) => Some(p.clone()),
            _ => None,
        })
        .expect("one path object")
}

/// The `(c1, c2, end)` of the first cubic segment of the first subpath.
///
/// Read through the decomposition, which resolves `v`/`y`'s implicit control
/// points into explicit ones — so a promotion that reordered operands shows up
/// here as different control points, while a byte assertion would just be
/// comparing my own mistake to itself.
fn first_cubic(cs: &ContentStream) -> (Point, Point, Point) {
    let path = only_path(cs);
    for sp in &path.subpaths {
        for seg in &sp.segments {
            if let Segment::Cubic { c1, c2, to } = seg {
                return (*c1, *c2, *to);
            }
        }
    }
    panic!("no cubic segment");
}

// ---------------------------------------------------------------------------
// `c` — both handles explicit
// ---------------------------------------------------------------------------

/// **The capability.** Both handles of a `c` move, and the on-curve points
/// stay exactly put.
///
/// The "node does not move" half is the point of the whole operation: a handle
/// drag that also dragged its node would be a worse node drag, not a new
/// capability.
#[test]
fn both_handles_of_a_cubic_move_without_moving_the_curve_through_points() {
    // A bump from (0,0) to (70,0), handles up at y=40.
    let cs = parse(b"0 0 m 10 40 60 40 70 0 c S");
    assert_eq!(
        first_cubic(&cs),
        (
            Point::new(10.0, 40.0),
            Point::new(60.0, 40.0),
            Point::new(70.0, 0.0)
        )
    );

    // Node 0 is the `m` at (0,0); the curve LEAVES it, so that is c1.
    let out = plan_move_handle(
        &cs,
        &only_path(&cs),
        0,
        Handle::Outgoing,
        Point::new(5.0, 90.0),
    )
    .expect("the outgoing handle of a cubic is draggable");
    let after = ContentStream::parse(out.content).expect("re-parses");
    assert_eq!(
        first_cubic(&after),
        (
            Point::new(5.0, 90.0),
            Point::new(60.0, 40.0),
            Point::new(70.0, 0.0)
        ),
        "only the first control point may change"
    );

    // Node 1 is the `c`'s endpoint (70,0); the curve ARRIVES there, so c2.
    let out = plan_move_handle(
        &cs,
        &only_path(&cs),
        1,
        Handle::Incoming,
        Point::new(65.0, 95.0),
    )
    .expect("the incoming handle of a cubic is draggable");
    let after = ContentStream::parse(out.content).expect("re-parses");
    assert_eq!(
        first_cubic(&after),
        (
            Point::new(10.0, 40.0),
            Point::new(65.0, 95.0),
            Point::new(70.0, 0.0)
        ),
        "only the second control point may change"
    );
    assert!(
        out.disclosures.is_empty(),
        "a `c` states both handles already, so nothing changed form"
    );
}

/// Incoming and outgoing at the same node address DIFFERENT control points.
///
/// The guard against a planner that ignores `handle` and always rewrites one
/// pair — which would pass any test that only ever asks for one side.
#[test]
fn the_two_handles_of_one_segment_are_not_the_same_point() {
    let cs = parse(b"0 0 m 10 40 60 40 70 0 c S");
    let target = Point::new(33.0, 44.0);

    // The `c` segment runs from node 0 to node 1: node 0's OUTGOING handle and
    // node 1's INCOMING handle both live in this one operator, and must be its
    // two different control points.
    let a = plan_move_handle(&cs, &only_path(&cs), 0, Handle::Outgoing, target).expect("c1");
    let b = plan_move_handle(&cs, &only_path(&cs), 1, Handle::Incoming, target).expect("c2");
    assert_ne!(
        a.content, b.content,
        "the two sides of a node must not resolve to the same control point"
    );

    let (c1, _, _) = first_cubic(&ContentStream::parse(a.content).expect("re-parses"));
    assert_eq!(c1, target, "outgoing is the FIRST control point");
    let (_, c2, _) = first_cubic(&ContentStream::parse(b.content).expect("re-parses"));
    assert_eq!(c2, target, "incoming is the SECOND control point");
}

// ---------------------------------------------------------------------------
// `v` and `y` — one handle implicit, so the operator is re-spelled
// ---------------------------------------------------------------------------

/// **`v`'s implicit first control point can be dragged**, by promoting the
/// segment to `c`.
///
/// `x2 y2 x3 y3 v` puts its first control point AT the current point. Dragging
/// it means it is no longer the current point, so the short spelling can no
/// longer express the curve.
///
/// Checked through the decomposition: a promotion that emitted the operands in
/// the wrong order parses and renders perfectly well and simply draws a
/// different curve.
#[test]
fn dragging_the_implicit_handle_of_a_v_promotes_it_to_a_cubic() {
    // `v` from (0,0): c1 = (0,0) implicitly, c2 = (60,40), end = (70,0).
    let cs = parse(b"0 0 m 60 40 70 0 v S");
    assert_eq!(
        first_cubic(&cs),
        (
            Point::new(0.0, 0.0),
            Point::new(60.0, 40.0),
            Point::new(70.0, 0.0)
        ),
        "`v`'s first control point is the current point"
    );

    let out = plan_move_handle(
        &cs,
        &only_path(&cs),
        0,
        Handle::Outgoing,
        Point::new(5.0, 50.0),
    )
    .expect("`v`'s implicit handle is draggable via promotion");
    let after = ContentStream::parse(out.content).expect("re-parses");
    assert_eq!(
        first_cubic(&after),
        (
            Point::new(5.0, 50.0),
            Point::new(60.0, 40.0),
            Point::new(70.0, 0.0)
        ),
        "only the promoted control point changes; c2 and the endpoint hold"
    );
    assert_eq!(out.disclosures.len(), 1, "the re-spelling is disclosed");
    assert!(out.disclosures[0].contains("identically"));
}

/// **`y`'s implicit second control point** — the mirror case.
///
/// `x1 y1 x3 y3 y` puts its second control point AT the endpoint. Included
/// separately rather than trusting symmetry with `v`, because `v` and `y` are
/// the most-confused pair in the operator set and a planner that mixed them up
/// would satisfy the `v` test alone.
#[test]
fn dragging_the_implicit_handle_of_a_y_promotes_it_to_a_cubic() {
    // `y` from (0,0): c1 = (10,40), c2 = end = (70,0).
    let cs = parse(b"0 0 m 10 40 70 0 y S");
    assert_eq!(
        first_cubic(&cs),
        (
            Point::new(10.0, 40.0),
            Point::new(70.0, 0.0),
            Point::new(70.0, 0.0)
        ),
        "`y`'s second control point is the endpoint"
    );

    // The curve ARRIVES at node 1 (the endpoint), so its incoming handle is c2.
    let out = plan_move_handle(
        &cs,
        &only_path(&cs),
        1,
        Handle::Incoming,
        Point::new(65.0, 30.0),
    )
    .expect("`y`'s implicit handle is draggable via promotion");
    let after = ContentStream::parse(out.content).expect("re-parses");
    assert_eq!(
        first_cubic(&after),
        (
            Point::new(10.0, 40.0),
            Point::new(65.0, 30.0),
            Point::new(70.0, 0.0)
        ),
        "c1 and the endpoint hold; only the promoted c2 changes"
    );
    assert_eq!(out.disclosures.len(), 1);
}

/// `v`'s EXPLICIT handle needs no promotion, and says nothing.
///
/// The other half of the promotion logic: only the implicit side re-spells.
/// A planner that promoted unconditionally would pass every test above.
#[test]
fn the_explicit_handle_of_a_v_is_rewritten_in_place() {
    let cs = parse(b"0 0 m 60 40 70 0 v S");
    let out = plan_move_handle(
        &cs,
        &only_path(&cs),
        1,
        Handle::Incoming,
        Point::new(55.0, 20.0),
    )
    .expect("`v`'s explicit second control point is a plain rewrite");
    assert_eq!(
        out.content, b"0 0 m 55 20 70 0 v S",
        "the operator keeps its short spelling"
    );
    assert!(
        out.disclosures.is_empty(),
        "nothing changed form, so nothing is owed"
    );
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// **A straight segment has no handle, and is not silently curved.**
///
/// Turning a line into a curve is a different operation with a different name.
/// Inferring it from a drag on a handle that was never drawn would be exactly
/// the silent reinterpretation rule 4 forbids.
#[test]
fn a_straight_segment_has_no_handle_and_refuses_by_name() {
    let cs = parse(b"0 0 m 70 0 l S");
    let err = plan_move_handle(
        &cs,
        &only_path(&cs),
        0,
        Handle::Outgoing,
        Point::new(5.0, 50.0),
    )
    .expect_err("a straight line has no curve handle");
    assert!(
        matches!(err, VectorEditError::NoHandleHere { index: 0, .. }),
        "got {err:?}"
    );
    // The refusal has to say what it will not do on its own, or the operator
    // reads it as "this is broken" rather than "ask for the conversion".
    let msg = err.to_string();
    assert!(
        msg.contains("straight"),
        "the refusal must name the obstruction: {msg}"
    );
    assert!(
        !msg.contains("  "),
        "collapsed continuation whitespace: {msg}"
    );
}

/// **A handle drag never reaches across a subpath boundary.**
///
/// Anchor indices are object-scoped and run straight across subpaths, so the
/// "next anchor" after a subpath's last node is the NEXT SUBPATH's first
/// node. Dragging the outgoing handle of a subpath's final node must not
/// reshape a different subpath's first segment — an edit somewhere the
/// operator was not looking, which would look entirely correct in any test
/// using a single-subpath fixture.
///
/// # What actually enforces this, which is not what it looks like
///
/// `plan_move_handle` filters out a next-anchor that opens a subpath, and this
/// test was written believing that filter was what it exercised. It is not:
/// deleting the filter leaves this test passing. Every subpath-opening anchor
/// carries `m`, `re`, or (after an `h`-reopen) no keyword at all, and the
/// keyword match refuses all three before the filter is ever consulted.
///
/// Recorded rather than quietly corrected because the consequence outlives the
/// detail: the KEYWORD MATCH is the load-bearing guard here. Weakening it on
/// the belief that the `is_start` filter is a backstop would open exactly the
/// cross-subpath edit this test is named for, and this test would not notice.
/// Found by deleting the filter and re-running — the same differential check
/// the rest of this Pass used.
#[test]
fn the_outgoing_handle_of_a_subpaths_last_node_does_not_reach_the_next_subpath() {
    // Two subpaths; the second opens with `m` and then curves.
    let cs = parse(b"0 0 m 70 0 l 100 0 m 110 40 160 40 170 0 c S");
    let path = only_path(&cs);
    assert_eq!(path.subpaths.len(), 2, "fixture has two subpaths");

    // Node 1 is the first subpath's last node. Its "next" anchor is node 2 —
    // the second subpath's `m` — whose segment must be out of reach.
    let err = plan_move_handle(&cs, &path, 1, Handle::Outgoing, Point::new(5.0, 50.0))
        .expect_err("must not reach into the next subpath");
    assert!(
        matches!(err, VectorEditError::NoHandleHere { index: 1, .. }),
        "got {err:?}"
    );

    // And the curve in the second subpath is untouched by that attempt.
    assert_eq!(
        first_cubic(&cs),
        (
            Point::new(110.0, 40.0),
            Point::new(160.0, 40.0),
            Point::new(170.0, 0.0)
        )
    );
}

/// A subpath's FIRST node has no incoming segment, so no incoming handle.
#[test]
fn the_first_node_of_a_subpath_has_no_incoming_handle() {
    let cs = parse(b"0 0 m 10 40 60 40 70 0 c S");
    let err = plan_move_handle(
        &cs,
        &only_path(&cs),
        0,
        Handle::Incoming,
        Point::new(5.0, 50.0),
    )
    .expect_err("nothing arrives at a subpath's start");
    assert!(
        matches!(err, VectorEditError::NoHandleHere { .. }),
        "got {err:?}"
    );
}

/// An out-of-range node is reported as out of range, not as a missing handle.
///
/// Two different problems with two different next actions: one means "that
/// point does not exist", the other "that point exists and is a corner".
#[test]
fn an_out_of_range_node_is_named_as_such() {
    let cs = parse(b"0 0 m 10 40 60 40 70 0 c S");
    let err = plan_move_handle(
        &cs,
        &only_path(&cs),
        99,
        Handle::Outgoing,
        Point::new(0.0, 0.0),
    )
    .expect_err("must refuse");
    assert!(
        matches!(
            err,
            VectorEditError::NodeOutOfRange {
                index: 99,
                count: 2
            }
        ),
        "got {err:?}"
    );
}

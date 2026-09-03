//! Editing anchors whose coordinates are written **nowhere** in the file.
//!
//! # The shared defect these tests cover
//!
//! Two anchor kinds carry no operand of their own, and both were refused for
//! that reason until Pass 30.0:
//!
//! - a corner of an `re` rectangle — `re` states an origin and a *size*, so
//!   three of the four corners appear in no operand at all, and the fourth
//!   (`x y`) cannot move without dragging the other three with it;
//! - the reused start of a subpath reopened after `h` — §8.5.2.1 says a
//!   segment following a close begins a new subpath at the closed subpath's
//!   start point, which the file never names.
//!
//! Both are now handled by **materializing the missing operand**: expanding
//! `re` to the equivalent `m`/`l`/`l`/`l`/`h` the spec itself states
//! (Table 59), and inserting the `m` the file omitted. The tests below are
//! organized around what can go wrong with that, not around the happy path.
//!
//! # What these tests are really guarding
//!
//! **1. That the corner that moved is the corner that was asked for.** The
//! anchor index has to mean the same thing before the expansion (where it
//! indexes `rect_corners`) and after (where it indexes the emitted `m`/`l`
//! sequence). Those are two orderings derived in two places; nothing in the
//! type system ties them together. So the corner is checked by DECOMPOSING
//! the result and comparing points — a byte comparison would agree with
//! itself if both orderings were wrong in the same way.
//!
//! **2. That the expansion does not silently change the picture.** `re`
//! appends a *closed* subpath. Emitting the four segments without the
//! trailing `h` leaves it open, which is invisible on a fill and clearly
//! wrong on a stroke: an open subpath takes two line caps where the closed one
//! takes a corner join. This is the single most likely way to write this
//! function and have every geometric assertion still pass.
//!
//! **3. That the anchor count and order survive.** A front end holds a node
//! index across the drag it just performed (that is what makes a drag feel
//! continuous). If the expansion changed the count, the second drag of a
//! gesture would move a different point than the first.
//!
//! **4. That inserting an `m` does not disturb the subpath before it.** The
//! insertion sits between a `h` and the segment that inherited from it, and
//! the risk is that it is written *before* the `h` instead — which would
//! leave the earlier subpath unclosed and move its end.

use pdfcer_core::content::ContentStream;
use pdfcer_core::vector::edit::{anchor_count, plan_move_node, plan_move_subpath};
use pdfcer_core::vector::{
    Matrix, NoXObjects, PathObject, Point, Segment, VectorObject, decompose,
};

/// Decompose a content stream and hand back its single path object.
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

/// Every anchor of the object, in decomposition order — the space
/// `plan_move_node`'s `node_index` addresses.
fn anchors(cs: &ContentStream) -> Vec<Point> {
    only_path(cs)
        .subpaths
        .iter()
        .flat_map(|s| s.anchors().collect::<Vec<_>>())
        .collect()
}

fn parse(bytes: &[u8]) -> ContentStream {
    ContentStream::parse(bytes.to_vec()).expect("fixture parses")
}

// ---------------------------------------------------------------------------
// `re` rectangle corners
// ---------------------------------------------------------------------------

/// **The capability, checked geometrically.** Each of the four corners moves
/// on its own, and the other three stay exactly put.
///
/// Run over all four indices deliberately: a single-corner test passes just as
/// happily when the corner ordering is rotated or reversed, and a rotation is
/// the most natural mistake to make between `rect_corners` order and emission
/// order.
#[test]
fn every_rectangle_corner_moves_alone_and_the_others_stay() {
    // Corners, in rect_corners order: (10,10) (90,10) (90,50) (10,50).
    let original = [
        Point::new(10.0, 10.0),
        Point::new(90.0, 10.0),
        Point::new(90.0, 50.0),
        Point::new(10.0, 50.0),
    ];
    for corner in 0..4 {
        let cs = parse(b"10 10 80 40 re S");
        assert_eq!(anchors(&cs), original, "fixture corner order");

        let target = Point::new(-7.0, -8.0);
        let plan = plan_move_node(&cs, &only_path(&cs), corner, target)
            .unwrap_or_else(|e| panic!("corner {corner} must be draggable: {e}"));
        let after = ContentStream::parse(plan.content).expect("re-parses");

        let mut expected = original;
        expected[corner] = target;
        assert_eq!(
            anchors(&after),
            expected,
            "dragging corner {corner} moved the wrong point(s)"
        );
    }
}

/// **The trailing `h` is load-bearing.** The expanded rectangle must stay a
/// CLOSED subpath.
///
/// Dropping it is invisible under a fill and visibly wrong under a stroke —
/// two line caps instead of a corner join — and every geometric assertion in
/// this file would still pass without it. Asserted through the decomposition's
/// `closed` flag rather than by looking for the byte, so it is a claim about
/// the shape rather than about the spelling.
#[test]
fn the_expanded_rectangle_is_still_a_closed_subpath() {
    let cs = parse(b"10 10 80 40 re S");
    assert!(
        only_path(&cs).subpaths[0].closed,
        "the `re` fixture is closed"
    );

    let plan = plan_move_node(&cs, &only_path(&cs), 2, Point::new(120.0, 60.0)).expect("draggable");
    let after = ContentStream::parse(plan.content).expect("re-parses");
    let path = only_path(&after);
    assert_eq!(path.subpaths.len(), 1, "still one subpath");
    assert!(
        path.subpaths[0].closed,
        "the expansion must keep the subpath closed, or a stroked box gains \
         two line caps where it had a corner join"
    );
    // Four corners means three segments plus the implicit closing one.
    assert_eq!(path.subpaths[0].segments.len(), 3);
    assert!(
        path.subpaths[0]
            .segments
            .iter()
            .all(|s| matches!(s, Segment::Line { .. })),
        "a rectangle expands to straight lines only"
    );
}

/// The anchor count and ordering survive the expansion, so a front end can
/// hold a node index across a drag.
#[test]
fn the_expansion_preserves_the_anchor_count() {
    let cs = parse(b"10 10 80 40 re S");
    let before = anchor_count(&cs, &only_path(&cs));
    assert_eq!(before, 4);

    let plan = plan_move_node(&cs, &only_path(&cs), 1, Point::new(95.0, 15.0)).expect("draggable");
    let after = ContentStream::parse(plan.content).expect("re-parses");
    assert_eq!(
        anchor_count(&after, &only_path(&after)),
        before,
        "a drag must not renumber the object's nodes under the operator's cursor"
    );
}

/// The rewrite is disclosed: the shape survives, the *form* does not, and
/// dragging back will not restore it (rule 4 applied to representation).
#[test]
fn expanding_a_rectangle_is_disclosed_to_the_operator() {
    let cs = parse(b"10 10 80 40 re S");
    let plan = plan_move_node(&cs, &only_path(&cs), 0, Point::new(0.0, 0.0)).expect("draggable");
    assert_eq!(plan.disclosures.len(), 1, "exactly one thing to say");
    let msg = &plan.disclosures[0];
    // It must name the consequence, not merely announce that something
    // happened — an operator told only "the shape was rewritten" cannot tell
    // whether their drawing changed.
    assert!(
        msg.contains("identically"),
        "the disclosure must say the drawing is unchanged: {msg}"
    );
    assert!(
        msg.contains("back"),
        "and that dragging back will not restore the original form: {msg}"
    );
    // R1's collapsed-`\`-continuation defect, shipped repeatedly in this
    // project.
    assert!(
        !msg.contains("  "),
        "collapsed continuation whitespace: {msg}"
    );
}

/// An ordinary (non-rectangle) node drag discloses NOTHING.
///
/// The guard against disclosure inflation: a message attached to every edit is
/// a message the operator stops reading, which costs exactly the cases where
/// it mattered.
#[test]
fn an_ordinary_node_drag_says_nothing() {
    let cs = parse(b"10 20 m 100 200 l S");
    let plan =
        plan_move_node(&cs, &only_path(&cs), 1, Point::new(120.0, 250.0)).expect("draggable");
    assert_eq!(plan.content, b"10 20 m 120 250 l S");
    assert!(
        plan.disclosures.is_empty(),
        "an in-place operand rewrite changes no form and owes no disclosure"
    );
}

// ---------------------------------------------------------------------------
// The implicit reused start (`h` then a segment with no `m`)
// ---------------------------------------------------------------------------

/// **The capability.** The inherited start point moves, and only it.
///
/// `0 0 m 10 0 l h 20 20 l 30 30 l S` — the second `l` run has no `m`, so its
/// subpath starts at (0,0), inherited from the closed subpath before it.
#[test]
fn the_inherited_start_of_a_reopened_subpath_can_be_dragged() {
    let cs = parse(b"0 0 m 10 0 l h 20 20 l 30 30 l S");
    assert_eq!(
        anchors(&cs),
        vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            // The implicit start — inherited, named nowhere in the bytes.
            Point::new(0.0, 0.0),
            Point::new(20.0, 20.0),
            Point::new(30.0, 30.0),
        ]
    );

    let plan = plan_move_node(&cs, &only_path(&cs), 2, Point::new(5.0, 7.0))
        .expect("the inherited start is draggable");
    let after = ContentStream::parse(plan.content).expect("re-parses");
    assert_eq!(
        anchors(&after),
        vec![
            // The FIRST subpath is untouched — this is the assertion that
            // catches an `m` inserted before the `h` instead of after it,
            // which would leave that subpath open and move its end.
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(5.0, 7.0),
            Point::new(20.0, 20.0),
            Point::new(30.0, 30.0),
        ]
    );
}

/// The first subpath keeps its `h`, and therefore its closure.
///
/// Separate from the point check above because a `closed` flag and a set of
/// coordinates are different claims: an `m` inserted just before the `h`
/// would preserve every point above and still leave the first subpath open.
#[test]
fn materializing_the_start_leaves_the_previous_subpath_closed() {
    let cs = parse(b"0 0 m 10 0 l h 20 20 l 30 30 l S");
    assert!(only_path(&cs).subpaths[0].closed);

    let plan = plan_move_node(&cs, &only_path(&cs), 2, Point::new(5.0, 7.0)).expect("draggable");
    let after = ContentStream::parse(plan.content).expect("re-parses");
    let path = only_path(&after);
    assert!(
        path.subpaths[0].closed,
        "the `h` must still close the subpath before the inserted `m`"
    );
    assert!(
        !path.subpaths[1].starts_implicitly,
        "the second subpath now names its own start"
    );
}

/// **Moving** a whole implicitly-started subpath works too — the same
/// materialization, in `plan_move_subpath`.
///
/// The failure this catches is specific: translating only the operands that
/// exist moves every point of the subpath EXCEPT its inherited start, which
/// shears the shape instead of moving it. So the test checks that the start
/// moved by the same delta as the rest, not merely that the call succeeded.
#[test]
fn an_implicitly_started_subpath_can_be_moved_as_a_whole() {
    let cs = parse(b"0 0 m 10 0 l h 20 20 l 30 30 l S");
    let plan = plan_move_subpath(&cs, &only_path(&cs), 1, 100.0, 200.0)
        .expect("an implicitly-started subpath is movable");
    let after = ContentStream::parse(plan.content).expect("re-parses");
    assert_eq!(
        anchors(&after),
        vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            // start (0,0) + delta — the point that would have stayed behind
            Point::new(100.0, 200.0),
            Point::new(120.0, 220.0),
            Point::new(130.0, 230.0),
        ],
        "every point of the subpath must shift by the same delta, including \
         the start that had no operand"
    );
    assert_eq!(plan.disclosures.len(), 1);
}

/// The `re`-then-segment form of the same inheritance.
///
/// `re` also leaves `needs_move` set (§8.5.2.1: it appends a *complete*
/// subpath), so a following segment inherits the rectangle's origin exactly as
/// it would after `h`. Included because the two paths through the walker are
/// separate and only one of them is exercised by the `h` fixtures.
#[test]
fn a_segment_after_a_rectangle_inherits_its_origin_and_is_draggable() {
    let cs = parse(b"10 10 80 40 re 50 50 l S");
    let before = anchors(&cs);
    assert_eq!(
        before.get(4),
        Some(&Point::new(10.0, 10.0)),
        "the segment after `re` starts at the rectangle's origin"
    );

    let plan = plan_move_node(&cs, &only_path(&cs), 4, Point::new(1.0, 2.0))
        .expect("an origin inherited from `re` is draggable");
    let after = ContentStream::parse(plan.content).expect("re-parses");
    let got = anchors(&after);
    assert_eq!(got.get(4), Some(&Point::new(1.0, 2.0)));
    assert_eq!(
        got.get(..4),
        before.get(..4),
        "the rectangle itself must not move"
    );
}

// ---------------------------------------------------------------------------
// Clipping paths: moved with a disclosure, not silently
// ---------------------------------------------------------------------------

/// **A clip's geometry is editable, and says that the effect lands
/// elsewhere.**
///
/// A clipping path draws nothing; it decides which OTHER content is visible.
/// Moving one therefore changes the page somewhere the operator is not
/// looking — the same condition that makes subpath-DELETE refuse outright
/// (`subpath_delete.rs`). The difference is intent: resizing a crop region is
/// a real task, while "delete part of a clip" has no reading worth guessing,
/// so this discloses where that refuses.
///
/// # Why this test exists at all
///
/// It was found by running the new rectangle-corner drag against a real file
/// instead of a fixture. Clips are almost always `re` rectangles (§8.5.4's
/// canonical `re W n`), so before Pass 30.0 their corners were unreachable —
/// refused as un-draggable — and making them draggable silently removed that
/// accidental protection. The first closed 4-anchor object on the first page
/// of the first real PDF tried was a full-page clip.
#[test]
fn dragging_a_clipping_paths_corner_discloses_that_it_clips() {
    // §8.5.4's canonical page-clip idiom.
    let cs = parse(b"0 0 500 700 re W n");
    let plan = plan_move_node(&cs, &only_path(&cs), 2, Point::new(300.0, 400.0))
        .expect("a clip's geometry is editable");

    let all = plan.disclosures.join(" ");
    assert!(
        all.contains("clipping region"),
        "the operator must be told the shape controls other content: {all}"
    );
    assert!(
        all.contains("rectangle"),
        "and that the rectangle had to be rewritten: {all}"
    );
    assert_eq!(
        plan.disclosures.len(),
        2,
        "two independent facts, both owed: {:?}",
        plan.disclosures
    );
}

/// A clip is disclosed on a whole-object move too, where no form change
/// happens — so the clip note cannot be riding on the rectangle expansion.
#[test]
fn moving_a_clipping_path_as_a_whole_discloses_it() {
    let cs = parse(b"0 0 500 700 re W n");
    let plan = pdfcer_core::vector::edit::plan_move(&cs, &only_path(&cs), 10.0, 10.0)
        .expect("a clip can be moved");
    assert_eq!(plan.disclosures.len(), 1);
    assert!(plan.disclosures[0].contains("clipping region"));
}

/// **And an ordinary painted path says nothing.**
///
/// `is_clipping_path` reads the operators rather than the paint style, because
/// a bare `n` is invisible without clipping anything. A guard keyed on
/// invisibility would fire here and teach the operator that the tool warns at
/// random.
#[test]
fn a_bare_n_path_that_clips_nothing_is_not_disclosed_as_a_clip() {
    let cs = parse(b"0 0 500 700 re n");
    let plan =
        plan_move_node(&cs, &only_path(&cs), 2, Point::new(300.0, 400.0)).expect("draggable");
    let all = plan.disclosures.join(" ");
    assert!(
        !all.contains("clipping region"),
        "a path that paints nothing is not a clip: {all}"
    );
}

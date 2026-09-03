//! Deleting ONE subpath of a many-subpath path object, and the refusals that
//! keep that safe.
//!
//! # Why the operation exists
//!
//! A CAD producer emits an entire drawing view as one path object — measured
//! on a real SolidWorks export, 1194 subpaths and 6681 anchors in a single
//! stroked path covering a 550×500 pt isometric view. `delete_object` can only
//! remove the whole view. An operator asking to delete "this line" means one
//! of its subpaths.
//!
//! # What these tests are really guarding
//!
//! Deleting by index is only safe if the index means the same thing to the
//! geometry (what the operator clicked) and to the operator bytes (what gets
//! removed). Nothing in the type system enforces that agreement — the two are
//! derived by separate walks over the same stream. So the interesting tests
//! here are not "does it delete" but:
//!
//! - the removed subpath is the one that was ASKED for, checked by geometry
//!   afterwards rather than by byte comparison (byte comparison would pass if
//!   both walks were wrong in the same way);
//! - every other subpath survives byte-verbatim;
//! - a structure the byte walk does not model exactly is REFUSED rather than
//!   approximated.

use pdfcer_core::content::ContentStream;
use pdfcer_core::vector::edit::plan_delete_subpath;
use pdfcer_core::vector::{
    Matrix, NoXObjects, PathObject, VectorEditError, VectorObject, decompose,
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

/// The x-coordinate of each subpath's first anchor, in order — a cheap
/// fingerprint for "which lines are still here".
fn first_xs(cs: &ContentStream) -> Vec<f64> {
    only_path(cs)
        .subpaths
        .iter()
        .filter_map(|s| s.anchors().next().map(|p| p.x))
        .collect()
}

fn three_lines() -> ContentStream {
    // Three separate lines painted by ONE `S`: one object, three subpaths.
    // First anchors at x = 0, 100, 200 so each is identifiable afterwards.
    ContentStream::parse(b"0 0 m 10 0 l 100 5 m 110 5 l 200 9 m 210 9 l S".to_vec())
        .expect("fixture parses")
}

/// **The operation.** The subpath asked for goes; the others stay.
///
/// Identified by geometry after the edit, not by comparing bytes to an
/// expected string: a byte comparison would also pass if the geometric walk
/// and the byte walk were wrong in the SAME way, which is precisely the
/// failure this operation is exposed to.
#[test]
fn deleting_the_middle_subpath_removes_that_one_and_keeps_the_others() {
    let cs = three_lines();
    assert_eq!(first_xs(&cs), vec![0.0, 100.0, 200.0]);

    let plan = plan_delete_subpath(&cs, &only_path(&cs), 1).expect("middle subpath is deletable");
    let after = ContentStream::parse(plan.content).expect("the edited stream re-parses");
    assert_eq!(
        first_xs(&after),
        vec![0.0, 200.0],
        "the subpath at x=100 must be the one that went"
    );
}

/// Each index really names a different line — the guard against an off-by-one
/// that a single-index test cannot see.
#[test]
fn every_index_deletes_its_own_subpath() {
    for (index, expected) in [
        (0usize, vec![100.0, 200.0]),
        (1, vec![0.0, 200.0]),
        (2, vec![0.0, 100.0]),
    ] {
        let cs = three_lines();
        let plan = plan_delete_subpath(&cs, &only_path(&cs), index).expect("deletable");
        let after = ContentStream::parse(plan.content).expect("re-parses");
        assert_eq!(
            first_xs(&after),
            expected,
            "deleting index {index} removed the wrong subpath"
        );
    }
}

/// The surviving operators keep their EXACT bytes (R46 / §5.7).
///
/// The minimal-diff invariant applies within the stream too: nothing may be
/// renumbered, reformatted or re-spaced merely because a neighbour was
/// removed.
#[test]
fn the_surviving_operators_are_byte_verbatim() {
    let cs = three_lines();
    let plan = plan_delete_subpath(&cs, &only_path(&cs), 1).expect("deletable");
    assert_eq!(
        plan.content, b"0 0 m 10 0 l 200 9 m 210 9 l S",
        "surviving operators must be untouched, and the gap left by the removed \
         subpath must not widen"
    );
}

/// Deleting the only subpath deletes the object.
///
/// A painting operator with no path is not a smaller object; it is
/// meaningless. Asserted through the decomposition — the page must contain no
/// path object at all afterwards.
#[test]
fn deleting_the_last_subpath_removes_the_whole_object() {
    let cs = ContentStream::parse(b"q 1 w 0 0 m 10 0 l S Q".to_vec()).expect("parses");
    let plan = plan_delete_subpath(&cs, &only_path(&cs), 0).expect("deletable");
    let after = ContentStream::parse(plan.content).expect("re-parses");
    let model = decompose(&after, Matrix::IDENTITY, &NoXObjects);
    assert!(
        !model
            .objects
            .iter()
            .any(|o| matches!(o, VectorObject::Path(_))),
        "no path object may survive; got {:?}",
        model.objects.len()
    );
}

/// **A clipping path is refused by name.**
///
/// Removing one subpath of a `W n` clip changes which OTHER content shows
/// through — an edit whose visible effect is somewhere the operator was not
/// looking. Rule 4 (fuzzy, never sneaky) forbids it.
#[test]
fn a_clipping_path_is_refused_rather_than_silently_changing_what_is_visible() {
    let cs = ContentStream::parse(b"0 0 m 10 0 l 100 5 m 110 5 l W n".to_vec()).expect("parses");
    let err = plan_delete_subpath(&cs, &only_path(&cs), 0)
        .expect_err("a clip path must not be part-deleted");
    assert!(
        matches!(err, VectorEditError::ClippingPath),
        "expected the clipping-path refusal, got {err:?}"
    );
    // The message must say WHY, not merely that it declined — an operator told
    // only "no" goes looking for a workaround.
    let msg = err.to_string();
    assert!(
        msg.contains("visible"),
        "the refusal must explain the consequence it is preventing: {msg}"
    );
}

/// **The refusal is now PRECISE, not conservative** (Pass 28.0).
///
/// `h` closes a subpath; a following segment operator reopens at the closed
/// subpath's start point (§8.5.2.1) with no `m` of its own — so its start is
/// INHERITED, carried by no operand. Excising the subpath BEFORE it changes
/// where it begins: a byte-minimal edit that passes `--verify-undo` and every
/// content-identity check, and still moves a line the operator never touched.
///
/// This test previously asserted `SubpathStructureMismatch` — a refusal of the
/// WHOLE OBJECT whenever an implicit reopen appeared anywhere in it, which came
/// from re-deriving the subpaths in a second walk and giving up when the counts
/// disagreed. Now that each subpath carries the token range the decomposition
/// recorded, only the one genuinely unsafe deletion is refused, and the rest of
/// the object stays editable.
#[test]
fn deleting_before_an_implicitly_reopened_subpath_is_refused_by_name() {
    // `m … l h l …`: the trailing `l` starts a subpath with no `m`.
    let cs = ContentStream::parse(b"0 0 m 10 0 l h 20 20 l 30 30 l S".to_vec()).expect("parses");
    let path = only_path(&cs);
    assert!(
        path.subpaths.len() >= 2,
        "the fixture must actually produce an implicit subpath, or this tests nothing: got {} subpath(s)",
        path.subpaths.len()
    );
    assert!(
        path.subpaths[1].starts_implicitly,
        "the second subpath must be the implicit one"
    );

    let err = plan_delete_subpath(&cs, &path, 0).expect_err("must refuse");
    match err {
        VectorEditError::DeleteWouldMoveNextSubpath { index } => assert_eq!(index, 0),
        other => panic!("expected DeleteWouldMoveNextSubpath, got {other:?}"),
    }
    // The message has to name the CONSEQUENCE, not just decline.
    assert!(
        err.to_string().contains("move"),
        "the refusal must say what it is preventing: {err}"
    );
}

/// **And the implicit subpath itself IS deletable** — the capability the old
/// conservative guard denied.
///
/// Removing its own segments moves nothing: it has no successor inheriting
/// from it here, and its own start was inherited rather than written, so there
/// is no operand to orphan. Under the previous guard this whole object was
/// undeletable; that is the cost the count-based approach was quietly paying.
#[test]
fn the_implicitly_reopened_subpath_itself_can_be_deleted() {
    let cs = ContentStream::parse(b"0 0 m 10 0 l h 20 20 l 30 30 l S".to_vec()).expect("parses");
    let plan = plan_delete_subpath(&cs, &only_path(&cs), 1).expect("the implicit subpath deletes");
    let after = ContentStream::parse(plan.content).expect("re-parses");
    assert_eq!(
        first_xs(&after),
        vec![0.0],
        "only the first subpath survives"
    );
}

/// An out-of-range index refuses and names the real count.
#[test]
fn an_out_of_range_index_refuses_with_the_true_count() {
    let cs = three_lines();
    let err = plan_delete_subpath(&cs, &only_path(&cs), 9).expect_err("must refuse");
    match err {
        VectorEditError::SubpathOutOfRange { index, count } => {
            assert_eq!(index, 9);
            assert_eq!(count, 3);
        }
        other => panic!("expected SubpathOutOfRange, got {other:?}"),
    }
}

/// `re` rectangles are whole subpaths in one operator, and delete cleanly.
#[test]
fn a_rectangle_subpath_deletes_as_one_operator() {
    let cs = ContentStream::parse(b"0 0 10 10 re 50 50 10 10 re S".to_vec()).expect("parses");
    assert_eq!(only_path(&cs).subpaths.len(), 2);
    let plan = plan_delete_subpath(&cs, &only_path(&cs), 0).expect("deletable");
    assert_eq!(plan.content, b"50 50 10 10 re S");
}

/// **The structure guard must not OVER-refuse.**
///
/// `an_implicitly_reopened_subpath_makes_the_whole_edit_refuse` proves the
/// guard fires. This proves it is narrow: `h` is perfectly legitimate when the
/// next subpath opens with its own `m`, and a guard that refused every path
/// containing `h` would make delete unavailable on most real drawings while
/// looking correct in the refusal test.
///
/// Same discipline as differentially testing an exclusion added to a checker —
/// "it refuses the bad case" and "it permits the good case" are two different
/// claims, and only testing the first is how a guard quietly becomes a wall.
#[test]
fn a_closed_subpath_followed_by_an_explicit_move_still_deletes() {
    let cs = ContentStream::parse(b"0 0 m 10 0 l h 50 50 m 60 60 l S".to_vec()).expect("parses");
    let path = only_path(&cs);
    assert_eq!(
        path.subpaths.len(),
        2,
        "an `h` followed by an explicit `m` is two ordinary subpaths"
    );
    let plan = plan_delete_subpath(&cs, &path, 0).expect("this structure IS deletable");
    let after = ContentStream::parse(plan.content).expect("re-parses");
    assert_eq!(
        first_xs(&after),
        vec![50.0],
        "the closed first subpath goes, the second survives"
    );
}

/// A rectangle mixed with a hand-drawn subpath deletes by index correctly.
#[test]
fn a_rectangle_and_a_drawn_subpath_index_independently() {
    let cs = ContentStream::parse(b"0 0 10 10 re 50 50 m 60 60 l S".to_vec()).expect("parses");
    assert_eq!(only_path(&cs).subpaths.len(), 2);
    let plan = plan_delete_subpath(&cs, &only_path(&cs), 0).expect("deletable");
    assert_eq!(
        plan.content, b"50 50 m 60 60 l S",
        "removing the `re` must leave the drawn subpath byte-verbatim"
    );
}

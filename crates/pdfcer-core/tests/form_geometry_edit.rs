//! **Editing geometry INSIDE a form XObject** (`Pass 188.0`).
//!
//! ## The gap this closes, and why it was invisible on one operator's files
//!
//! `PageObjects::leaves` has been readable since form recursion landed — a
//! shell can see, name, hit-test and describe every object inside a form, and
//! walk `containment` to say *"inside Title block"*. What it could not do is
//! **edit** any of it: every geometry verb is addressed by an index into
//! `PageObjects::objects`, and a leaf deliberately has no such index. Leaves
//! are kept out of that list precisely so that eleven page-surgery call sites
//! cannot apply a form-relative token range to the page's stream and corrupt
//! it silently.
//!
//! So the reach existed and the write did not. `pdfcer-gui` measured the cost on
//! real drawings:
//!
//! | fixture | page objects | forms | leaves |
//! |---|---|---|---|
//! | print-conformance composite | 28 | 4 | **242** |
//! | `ncored-benchmark-cad-drawing.pdf` | 129,758 | 1 | **10,256** |
//! | a 36-sheet SolidWorks set | 5,903 | 0 | 0 |
//!
//! On two of three fixtures almost nothing visible was node-editable — and on
//! the operator's own drawings everything was. **That asymmetry is why this
//! was never reported as a defect**, and it is worth remembering as a shape:
//! a capability gap that happens to miss the person most likely to notice it.
//!
//! ## ★★ The load-bearing property, and the fixture built to falsify it
//!
//! A leaf's geometry is **page space**; its bytes are in the form's stream,
//! which is **form space**. The planners convert a page-space target into
//! stream space by inverting the object's `ctm`, and that `ctm` is page-space
//! only if the form's stream is decomposed **starting from the matrix that
//! placed it**.
//!
//! Every pre-existing form fixture in this repository places its form with a
//! **pure translation**, and a translation is exactly the transform under
//! which the wrong conversion still looks plausible: a 10 pt drag moves 10 pt
//! either way, just from the wrong origin. `scaled-form-placement.pdf` exists
//! for this Pass and places its form at `2 0 0 2 40 30 cm`, so a decomposition
//! started from the identity moves a node by **twice** the requested distance.
//! `moving_a_node_inside_a_scaled_form_lands_where_asked` is the assertion
//! that would go red, and no fixture without a scale could carry it.
//!
//! ## The write semantics are decision 076's, carried over unchanged
//!
//! A form has ONE set of bytes and §8.10.1 explicitly allows it to be drawn
//! many times. So an edit inside one changes every place it appears, and pdfcer
//! cannot prevent that structurally. Decision 076 already ruled this for text
//! editing inside a form: **edit in place, disclose, with `unshare_form` as
//! the option rather than the precondition.** `Pass 188.0` carries it to
//! geometry. `an_edit_inside_a_shared_form_changes_every_invocation` is that
//! ruling stated as a test rather than as a paragraph.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::vector::{Bounds, Point, VectorObject};
use std::path::{Path, PathBuf};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

/// Every leaf's page-space bounding box, in leaf order.
///
/// Page space is the point: the whole question is whether an edit expressed in
/// the coordinates the operator clicked in lands where they clicked.
fn leaf_boxes(s: &mut EditSession, page_index: usize) -> Vec<Bounds> {
    s.page_objects(page_index)
        .unwrap()
        .leaves
        .iter()
        .map(|l| match &l.object {
            VectorObject::Path(p) => p.page_bbox,
            VectorObject::Text(t) => t.page_bbox,
            VectorObject::Image(i) => i.page_bbox,
        })
        .collect()
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn box_is(b: Bounds, llx: f64, lly: f64, urx: f64, ury: f64) -> bool {
    approx(b.min.x, llx) && approx(b.min.y, lly) && approx(b.max.x, urx) && approx(b.max.y, ury)
}

// -------------------------------------------------------------------------
// What the decomposer now records
// -------------------------------------------------------------------------

/// The two fields the surgery needs, asserted directly, because everything
/// below is a consequence of them being right.
#[test]
fn a_leaf_records_its_placement_and_its_in_form_index() {
    let mut s = session("forms-xobject/scaled-form-placement.pdf");
    let model = s.page_objects(0).unwrap();
    assert_eq!(model.leaves.len(), 1, "one path inside one form");
    let leaf = &model.leaves[0];
    assert_eq!(leaf.form_object_index, 0);
    // `2 0 0 2 40 30 cm` — the placement, not the identity.
    assert!(approx(leaf.placement.a, 2.0) && approx(leaf.placement.d, 2.0));
    assert!(approx(leaf.placement.e, 40.0) && approx(leaf.placement.f, 30.0));
}

/// ★ A nested form's leaf carries the COMPOSED placement, not the inner `cm`
/// alone. `nested-forms.pdf` puts Fm1 at (50, 50) inside Fm0 at the identity,
/// and Fm1 draws `20 20 30 30 re` — so the leaf is at page (70, 70).
#[test]
fn a_nested_leaf_carries_the_composed_placement() {
    let mut s = session("forms-xobject/nested-forms.pdf");
    let model = s.page_objects(0).unwrap();
    let leaf = &model.leaves[0];
    assert_eq!(
        leaf.containment.len(),
        2,
        "outermost first, ending with the direct parent"
    );
    assert!(approx(leaf.placement.e, 50.0) && approx(leaf.placement.f, 50.0));
    let boxes = leaf_boxes(&mut s, 0);
    assert!(box_is(boxes[0], 70.0, 70.0, 100.0, 100.0), "{:?}", boxes[0]);
}

/// `is_editable` was a hard `false` with a note saying it existed so the
/// answer would have somewhere to change. It changed — and it now means "this
/// leaf is a path", not "nothing in a form can be edited".
#[test]
fn is_editable_now_answers_the_question_it_names() {
    let mut s = session("forms-xobject/scaled-form-placement.pdf");
    let model = s.page_objects(0).unwrap();
    assert!(model.leaves[0].is_editable());
}

// -------------------------------------------------------------------------
// ★★ The placement matrix is load-bearing
// -------------------------------------------------------------------------

/// The assertion the scaled fixture exists for.
///
/// The form draws `10 10 20 20 re` in form space; at `2 0 0 2 40 30` that is
/// page `[60, 50, 100, 90]`. The lower-left node is dragged **outside** the
/// object's current box, to page `(40, 20)`.
///
/// # ★ Why the target is outside the box, and the assertion that was wrong
///
/// The first cut of this test dragged the corner to `(70, 60)` — inward — and
/// asserted the object's bounding box became `[70, 60, …]`. It failed, and the
/// feature was correct: moving ONE corner of a rectangle inward changes
/// nothing about the box, because the two adjacent corners still hold `x = 60`
/// and `y = 50`. The expectation was naive, not the code.
///
/// Recorded rather than silently corrected, because the correction is the
/// useful part: **a bounding box is a lossy instrument for a node move**, and
/// an inward drag is exactly the case where it reports nothing. Dragging
/// outward makes the box carry the answer, and the subpath's own start point
/// is asserted underneath it in form-space coordinates so the test does not
/// depend on the box at all.
///
/// # What a wrong matrix would produce
///
/// Page `(40, 20)` is form `(0, -5)` through the inverse of `2 0 0 2 40 30`.
/// A decomposition started from `Matrix::IDENTITY` would instead set the node
/// to form `(40, 20)` and paint it at page `(120, 70)` — inside the old box,
/// so the object's page box would come back `[60, 50, 180, 90]`, unchanged.
/// The two answers differ, which is what no translation-only fixture can say.
#[test]
fn moving_a_node_inside_a_scaled_form_lands_where_asked() {
    let mut s = session("forms-xobject/scaled-form-placement.pdf");
    let before = leaf_boxes(&mut s, 0);
    assert!(
        box_is(before[0], 60.0, 50.0, 180.0, 90.0),
        "{:?}",
        before[0]
    );

    let out = s
        .move_node_in_form(0, 0, 0, Point { x: 40.0, y: 20.0 })
        .unwrap();
    assert_eq!(out.invocations, 1, "this form is drawn once");

    let after = leaf_boxes(&mut s, 0);
    assert!(
        box_is(after[0], 40.0, 20.0, 180.0, 90.0),
        "the moved corner must be at the page point that was asked for; an identity-decomposed \
         form would leave this at [60, 50, 180, 90]. Got {:?}",
        after[0]
    );

    // And the same fact stated in the form's OWN space, independent of any
    // bounding box: page (40, 20) is form (0, -5).
    let model = s.page_objects(0).unwrap();
    let VectorObject::Path(path) = &model.leaves[0].object else {
        panic!("a path");
    };
    assert!(
        approx(path.subpaths[0].start.x, 0.0) && approx(path.subpaths[0].start.y, -5.0),
        "form-space start; got {:?}",
        path.subpaths[0].start
    );
    assert!(
        approx(path.subpaths[1].start.x, 50.0) && approx(path.subpaths[1].start.y, 10.0),
        "the sibling subpath must be untouched; got {:?}",
        path.subpaths[1].start
    );
}

/// The planner's own disclosures reach the caller through the form path, not
/// just through the page path (rule 4).
///
/// Moving one corner of a `re` turns a rectangle into a four-sided shape that
/// is no longer a box, and pdfcer says so. That sentence is generated deep
/// inside the planner; this asserts the form-scoped verb carries it out.
#[test]
fn a_planner_disclosure_survives_the_form_path() {
    let mut s = session("forms-xobject/scaled-form-placement.pdf");
    let out = s
        .move_node_in_form(0, 0, 0, Point { x: 40.0, y: 20.0 })
        .unwrap();
    assert!(
        out.disclosures
            .iter()
            .any(|d| d.contains("stored as a rectangle")),
        "the rectangle-rewrite disclosure must reach the caller: {:?}",
        out.disclosures
    );
}

/// The same property through the subpath verb, and with the second subpath as
/// a control: "the subpath moved" and "the whole object moved" are different
/// outcomes and a one-subpath fixture cannot tell them apart.
#[test]
fn moving_one_subpath_inside_a_form_leaves_its_sibling_alone() {
    let mut s = session("forms-xobject/scaled-form-placement.pdf");
    // Page-space delta of 20 pt right; the form is at 2x, so 10 pt in form
    // space. The FIRST subpath is page [60,50,100,90], the second is
    // [140,50,180,90] — together the object's box is [60,50,180,90].
    s.move_subpath_in_form(0, 0, 0, 20.0, 0.0).unwrap();
    let after = leaf_boxes(&mut s, 0);
    assert!(
        box_is(after[0], 80.0, 50.0, 180.0, 90.0),
        "only the first subpath should have moved right; got {:?}",
        after[0]
    );
}

// -------------------------------------------------------------------------
// ★★★ Decision 076's ruling, stated as a measurement
// -------------------------------------------------------------------------

/// A form invoked twice has one set of bytes. Editing through one invocation
/// changes both, and the outcome says so rather than leaving it to be
/// discovered.
#[test]
fn an_edit_inside_a_shared_form_changes_every_invocation() {
    let mut s = session("forms-xobject/shared-form-twice.pdf");
    let before = leaf_boxes(&mut s, 0);
    assert_eq!(before.len(), 2, "one form, two placements, two leaves");
    assert!(box_is(before[0], 10.0, 10.0, 40.0, 40.0), "{:?}", before[0]);
    assert!(
        box_is(before[1], 120.0, 120.0, 150.0, 150.0),
        "{:?}",
        before[1]
    );

    let out = s.move_subpath_in_form(0, 0, 0, 5.0, 0.0).unwrap();

    assert_eq!(out.invocations, 2, "the form is drawn twice");
    assert_eq!(out.pages, 1, "both on one page");
    // ★ The reach is STRUCTURED DATA and deliberately not prose. An earlier
    // cut pushed a sentence about it onto `disclosures` as well, and the CLI
    // then printed the reach twice — once from the sentence and once from its
    // own better-worded line naming `unshare-form` as the CLI spells it. Two
    // renderings of one fact, and a shell could only suppress either by
    // matching on a string.
    //
    // So this asserts the pair, and asserts the ABSENCE of the sentence,
    // because "we stopped duplicating it" is the property that would silently
    // regress the moment somebody adds a helpful `push`.
    assert!(
        !out.disclosures.iter().any(|d| d.contains("time(s)")),
        "the reach belongs in `invocations`/`pages`, not in prose: {:?}",
        out.disclosures
    );

    let after = leaf_boxes(&mut s, 0);
    assert!(
        box_is(after[0], 15.0, 10.0, 45.0, 40.0),
        "the invocation that was edited moved; got {:?}",
        after[0]
    );
    assert!(
        box_is(after[1], 125.0, 120.0, 155.0, 150.0),
        "★ and so did the other one — this is decision 076, not a bug; got {:?}",
        after[1]
    );
}

/// The reach counts PAGES separately from invocations, because "you changed
/// every sheet" and "you changed one sheet forty times over" are different
/// sentences to put in front of an operator.
#[test]
fn the_reach_counts_pages_separately_from_invocations() {
    let mut s = session("forms-xobject/shared-across-two-pages.pdf");
    let out = s.move_subpath_in_form(0, 0, 0, 1.0, 0.0).unwrap();
    assert_eq!(out.pages, 2, "one form, one Do per page, two pages");
    assert!(out.invocations >= 2);
}

// -------------------------------------------------------------------------
// The refusals
// -------------------------------------------------------------------------

/// A leaf index past the end names its own list, not the page's — a shell told
/// "object 4 of 28" by a verb it asked about leaf 4 of 242 would look for the
/// wrong bug.
#[test]
fn a_leaf_index_out_of_range_is_refused_by_name() {
    let mut s = session("forms-xobject/shared-form-twice.pdf");
    let err = s
        .move_node_in_form(0, 99, 0, Point { x: 0.0, y: 0.0 })
        .expect_err("leaf 99 does not exist");
    assert!(
        matches!(
            err,
            EditError::FormLeafOutOfRange {
                index: 99,
                count: 2
            }
        ),
        "got {err:?}"
    );
}

/// ★ A selection spanning two INVOCATIONS of one form is refused, and this is
/// the subtle half.
///
/// The obvious guard is "same form object?", and it is not enough: two
/// invocations produce leaves that name the same form and carry different
/// placements, and their `form_object_index` values COLLIDE — leaf 0 of the
/// first and leaf 1 of the second are the same bytes. Accepting that selection
/// would ask to move one object twice, through two different matrices.
#[test]
fn a_selection_spanning_two_invocations_is_refused() {
    let mut s = session("forms-xobject/shared-form-twice.pdf");
    let err = s
        .move_objects_in_form(0, &[0, 1], 5.0, 0.0)
        .expect_err("leaves 0 and 1 are two invocations of one form");
    assert!(
        matches!(err, EditError::FormLeafSelectionSpansForms { .. }),
        "got {err:?}"
    );
}

/// Nothing was written by that refusal — rule 4 applies to a form edit exactly
/// as it does to a page edit.
#[test]
fn a_refused_form_edit_leaves_the_session_untouched() {
    let mut s = session("forms-xobject/shared-form-twice.pdf");
    let before = leaf_boxes(&mut s, 0);
    let _ = s.move_objects_in_form(0, &[0, 1], 5.0, 0.0);
    let _ = s.move_node_in_form(0, 99, 0, Point { x: 0.0, y: 0.0 });
    assert_eq!(before, leaf_boxes(&mut s, 0));
    assert_eq!(s.undo_depth(), 0, "a refusal must record no command");
}

// -------------------------------------------------------------------------
// Undo, and the accumulate property
// -------------------------------------------------------------------------

/// One gesture, one undo entry, and undo restores the form's bytes — including
/// at the other invocation, which is where a half-undo would show.
#[test]
fn a_form_edit_is_one_undo_entry_and_undo_restores_both_invocations() {
    let mut s = session("forms-xobject/shared-form-twice.pdf");
    let before = leaf_boxes(&mut s, 0);
    s.move_subpath_in_form(0, 0, 0, 5.0, 0.0).unwrap();
    assert_ne!(before, leaf_boxes(&mut s, 0));
    assert!(s.undo().is_some());
    assert_eq!(
        before,
        leaf_boxes(&mut s, 0),
        "undo must restore every invocation, not just the edited one"
    );
}

/// A second edit to the same form composes on top of the first, exactly as a
/// second edit to a page's content does. This needs the session view; a
/// base read would re-splice the original bytes and silently discard edit one.
#[test]
fn two_edits_to_one_form_accumulate() {
    let mut s = session("forms-xobject/scaled-form-placement.pdf");
    s.move_subpath_in_form(0, 0, 0, 20.0, 0.0).unwrap();
    s.move_subpath_in_form(0, 0, 0, 20.0, 0.0).unwrap();
    let after = leaf_boxes(&mut s, 0);
    assert!(
        box_is(after[0], 100.0, 50.0, 180.0, 90.0),
        "two 20 pt moves must total 40 pt, not 20; got {:?}",
        after[0]
    );
}

/// Deleting inside a form removes the object from every invocation — which is
/// the case most worth a disclosure before the gesture, and the one this test
/// exists to make concrete.
#[test]
fn deleting_inside_a_shared_form_empties_every_invocation() {
    let mut s = session("forms-xobject/shared-form-twice.pdf");
    assert_eq!(leaf_boxes(&mut s, 0).len(), 2);
    let out = s.delete_objects_in_form(0, &[0]).unwrap();
    assert_eq!(out.invocations, 2);
    assert!(
        leaf_boxes(&mut s, 0).is_empty(),
        "one delete, both invocations emptied"
    );
}

/// ★★★ AN EDIT INSIDE A FORM MUST MOVE `page_content_generation`
/// (`Pass 197.0`).
///
/// # Reported, measured, by the consuming shell
///
/// `page_content_generation` exists so a front end can assert continuously
/// that its object model still agrees with the crate's, by comparing two
/// `u64`s instead of decomposing twice. The shell tested it one dependency
/// class at a time -- page content, annotations, in-form content -- and found
/// the third silent: `move_node_in_form` rewrote an invocation, the verb
/// returned `Ok` with `invocations > 0`, and the number did not move.
///
/// # Why that is corruption and not a cache miss
///
/// `PageObjects` addresses content **by index**. A shell that keys its
/// decomposition cache on this number serves the pre-edit model after every
/// in-form edit, and the next drag is applied to whatever object now sits at
/// that index. Both sides return `Ok`; the index is in range on both. Nothing
/// can notice.
///
/// # The shape of the defect, which is the reusable part
///
/// `Pass 188.0` found exactly this against the INTERNAL memo and fixed it
/// there, by keeping the descended-form spans beside the cache key -- they
/// cannot go inside it, because which forms a page reaches is an OUTPUT of the
/// walk. `page_content_generation` publishes the key ALONE, so it was left
/// behind: the crate's own staleness test became strictly stronger than the
/// one it hands to callers.
///
/// **A fix applied to one route is not a fix to the other**, and the second
/// route here was the one with a consumer on the end of it.
#[test]
fn an_edit_inside_a_form_moves_the_page_generation() {
    let mut s = session("forms-xobject/scaled-form-placement.pdf");
    let before = s.page_content_generation(0).unwrap();
    assert_eq!(
        before,
        s.page_content_generation(0).unwrap(),
        "the generation must be stable across a repeat call, or the rest of this proves nothing"
    );

    let out = s
        .move_node_in_form(0, 0, 0, Point { x: 40.0, y: 20.0 })
        .unwrap();
    assert!(
        out.invocations > 0,
        "the verb must actually have rewritten an invocation, or there is nothing to detect"
    );

    assert_ne!(
        before,
        s.page_content_generation(0).unwrap(),
        "an edit INSIDE a form XObject left the page generation unchanged. A shell keying its \
         decomposition cache on this number now serves a pre-edit model, and PageObjects \
         addresses content by INDEX -- so the next edit lands on the wrong object and returns Ok"
    );
}

/// ★ The CONTROL: the generation must still hold still when nothing changed.
///
/// Without this, "it moves after a form edit" is equally satisfied by a number
/// that moves on every call -- which would be useless as an agreement check and
/// would make every shell re-decompose every frame.
#[test]
fn the_page_generation_still_holds_still_when_nothing_is_edited() {
    let mut s = session("forms-xobject/scaled-form-placement.pdf");
    let a = s.page_content_generation(0).unwrap();
    let _ = s.page_objects(0).unwrap();
    let b = s.page_content_generation(0).unwrap();
    assert_eq!(
        a, b,
        "decomposing the page is not an edit and must not move the generation"
    );
}

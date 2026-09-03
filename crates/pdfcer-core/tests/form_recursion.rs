//! # Descending into form XObjects — the objects a page-sized wrapper was hiding
//!
//! Integration test for `decompose_page`'s form recursion and
//! [`FormLeaf`](pdfcer_core::vector::decompose::FormLeaf).
//!
//! # What was wrong, and it reached the operator
//!
//! `decompose_page` emitted a form XObject as **one opaque object** bounded by
//! its `/BBox` and never entered it. On a page whose visible body is wrapped in
//! a form — what SolidWorks emits per orthographic view, and what a great many
//! print files emit per panel — the form is an object in paint order **above**
//! everything drawn before it, and the hit test answers every click, anywhere,
//! with the form.
//!
//! His report, relayed from the GUI project: *"when I click on one of the
//! objects all I get is the page selected."* He was selecting a real object. It
//! was a form. Measured on one print-conformance page: **sixteen** ~20 × 20 pt
//! forms, one per blend-mode cell, each swallowing every click aimed at the
//! swatch inside it.
//!
//! ## ★ Why nothing caught it
//!
//! **No committed fixture had a form XObject at all.** Every vector fixture in
//! this repository draws straight onto the page, so the entire form branch of
//! the walk was exercised only by a stub resolver that returns a shape and no
//! content. `fixtures/synthetic/forms-xobject/` exists to end that.
//!
//! # The properties asserted
//!
//! | Property | Asserted by |
//! |---|---|
//! | a page-sized form yields the objects inside it | `a_page_sized_form_yields_the_objects_inside_it` |
//! | leaves are in page space, so one hit test serves both lists | (same) |
//! | nesting produces a containment path, and an intermediate form is not a leaf | `a_nested_form_reports_its_whole_containment_path` |
//! | a form invoking itself terminates, and says it did | `a_self_referential_form_terminates_and_is_counted` |
//! | a form invoked twice contributes twice, in two places | `a_form_invoked_twice_contributes_its_contents_twice` |
//! | the flat list does **not** move | `the_flat_object_list_is_unchanged_by_recursion` |
//! | a leaf names the form's stream, and `is_editable` answers about the OBJECT | `a_leaf_names_its_own_stream_and_reports_its_editability` |
//!
//! ## ★★ The last two are the load-bearing ones, and not for obvious reasons
//!
//! `the_flat_object_list_is_unchanged_by_recursion` guards a **safety**
//! property, not a compatibility one. Eleven call sites in `edit.rs` resolve a
//! paint-order index and apply content-stream surgery **to the page's stream**.
//! A leaf's token range indexes the *form's* stream — a different buffer. If a
//! leaf ever appears in `objects`, those verbs will apply a form-relative range
//! to the page and corrupt it silently, because the range is in bounds. Keeping
//! the lists separate is what makes those eleven sites correct by construction
//! rather than by a guard somebody must remember to add to each.
//!
//! `a_leaf_names_its_own_stream_and_reports_its_editability` is the same fact
//! from the caller's side, in the vocabulary the shell already uses for text.
//!
//! ## ★ `is_editable` changed meaning in `Pass 188.0`, and the change is narrow
//!
//! It used to be a hard `false` — *"editing through the recursion is not
//! built"*. It is built: the geometry verbs have form-scoped twins addressed by
//! a leaf index (`crates/pdfcer-core/tests/form_geometry_edit.rs`). So the
//! predicate now answers about **the object**: `true` for a path, `false` for
//! anything with no node, handle or subpath to drag.
//!
//! **The safety property above is untouched and must stay untouched.** Leaves
//! are still absent from `PageObjects::objects`; the form verbs reach them
//! through `PageObjects::leaves` and write to the *form's* stream, never the
//! page's. `the_flat_object_list_is_unchanged_by_recursion` is what guards
//! that, and it guards it exactly as hard now as it did before — arguably
//! harder, because there is now a second family of verbs that must not confuse
//! the two lists.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::text_extract::ContentStreamRef;
use pdfcer_core::vector::decompose::{ImageSource, PageObjects};
use pdfcer_core::vector::linepick::pick_line_in_page;
use pdfcer_core::vector::{
    Bounds, FormMarquee, HitTarget, MarqueeMode, Matrix, Point, VectorObject, decompose_page,
    hit_test_point, hit_test_point_deep, hit_test_rect_deep,
};

fn model(name: &str) -> PageObjects {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms-xobject")
        .join(name);
    let doc = Document::load(Path::new(&path)).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree");
    decompose_page(&doc.view(), &pages[0], Matrix::IDENTITY).expect("decomposes")
}

/// How many objects in the flat list are form XObjects.
fn form_count(m: &PageObjects) -> usize {
    m.objects
        .iter()
        .filter(|o| matches!(o, VectorObject::Image(i) if i.source == ImageSource::Form))
        .count()
}

/// ★ THE HEADLINE. One page-sized form, three squares inside it.
///
/// Before the recursion the page offered exactly one selectable thing — the
/// wrapper — and it covered the whole sheet.
#[test]
fn a_page_sized_form_yields_the_objects_inside_it() {
    let m = model("page-sized-form.pdf");

    assert_eq!(m.objects.len(), 1, "the flat list is the wrapper alone");
    assert_eq!(form_count(&m), 1);
    assert_eq!(
        m.leaves.len(),
        3,
        "★ the three squares inside the form must be reachable; \
         without the recursion this is 0 and every click hits the wrapper"
    );

    // Page space, not form space -- so a caller can hit-test the flat list and
    // the leaf list against ONE point without transforming anything. The
    // squares are at (10,10), (80,80) and (150,150) in the form, and the form
    // is placed at the origin, so those are their page coordinates too.
    let mut origins: Vec<(i64, i64)> = m
        .leaves
        .iter()
        .map(|l| {
            let b = l.object.page_bbox();
            (b.min.x.round() as i64, b.min.y.round() as i64)
        })
        .collect();
    origins.sort_unstable();
    assert_eq!(origins, vec![(10, 10), (80, 80), (150, 150)]);

    for leaf in &m.leaves {
        assert_eq!(leaf.containment.len(), 1, "one enclosing form");
    }
    assert_eq!(m.diagnostics.form_cycles, 0);
    assert_eq!(m.diagnostics.form_depth_overflows, 0);
}

/// Nesting: form A holds form B holds one square.
///
/// Two things are asserted and the second is the easy one to forget: the
/// containment path has BOTH forms in it, outermost first, and the
/// **intermediate form is not itself a leaf**. Emitting it as one would put a
/// second large hit target into the very list built to stop the first one
/// winning every click.
#[test]
fn a_nested_form_reports_its_whole_containment_path() {
    let m = model("nested-forms.pdf");

    assert_eq!(m.objects.len(), 1);
    assert_eq!(m.leaves.len(), 1, "only the square is a leaf");
    let leaf = &m.leaves[0];
    assert_eq!(
        leaf.containment.len(),
        2,
        "outer form then inner form, outermost first"
    );
    assert_ne!(
        leaf.containment[0], leaf.containment[1],
        "two distinct forms"
    );

    // The square is at (20,20) inside the inner form, which the outer form
    // places at +(50,50). Geometry composing through two levels is the thing
    // a single-level test could not have caught.
    let b = leaf.object.page_bbox();
    assert_eq!((b.min.x.round() as i64, b.min.y.round() as i64), (70, 70));
}

/// ★★ A form that invokes ITSELF terminates, and the walk says it did.
///
/// ISO 32000-1 §8.10.1 does not forbid this and nothing makes the file
/// invalid — it is simply unbounded to a naive walker. A decomposer that hangs
/// has a defect; one that refuses the whole page has a different defect. The
/// right answer is to descend once, notice the repeat, stop, and **count it**,
/// because a silently truncated list presented as "everything on the page" is
/// the failure this project cares most about.
///
/// The guard is keyed on the form's **object number**: the same stream is
/// reachable under different resource names, so a name-keyed guard would miss
/// the cycle entirely.
#[test]
fn a_self_referential_form_terminates_and_is_counted() {
    let m = model("self-referential-form.pdf");

    assert_eq!(m.leaves.len(), 1, "the square inside, once");
    assert_eq!(
        m.diagnostics.form_cycles, 1,
        "★ the repeat must be COUNTED, not silently dropped -- an incomplete \
         list presented as complete is worse than a refusal"
    );
    assert_eq!(
        m.diagnostics.form_depth_overflows, 0,
        "the cycle guard caught it, not the depth bound"
    );
}

/// A form invoked twice contributes its contents twice, in two places, naming
/// the same form both times.
///
/// ★ Worth pinning because it looks like a bug and is not: it is what the page
/// actually draws. It is also exactly the situation that makes editing a leaf
/// inside a shared form change **every** invocation — `ARCHITECTURE.md` §12
/// decision 076, which rules that edit-in-place is the default and that
/// copy-on-write is a separate verb.
#[test]
fn a_form_invoked_twice_contributes_its_contents_twice() {
    let m = model("shared-form-twice.pdf");

    assert_eq!(form_count(&m), 2, "two invocations, two flat objects");
    assert_eq!(m.leaves.len(), 2);
    assert_eq!(
        m.leaves[0].parent(),
        m.leaves[1].parent(),
        "both name the SAME form -- that is what 'shared' means"
    );

    let mut origins: Vec<(i64, i64)> = m
        .leaves
        .iter()
        .map(|l| {
            let b = l.object.page_bbox();
            (b.min.x.round() as i64, b.min.y.round() as i64)
        })
        .collect();
    origins.sort_unstable();
    assert_eq!(
        origins,
        vec![(10, 10), (120, 120)],
        "the same contents, drawn in two different places"
    );
}

/// ★★★ THE SAFETY PROPERTY. Recursion must not put leaves into `objects`.
///
/// Eleven call sites in `edit.rs` resolve a paint-order index and apply
/// content-stream surgery **to the page's stream**. A leaf's token range
/// indexes the **form's** stream, a different buffer. A leaf in `objects` would
/// be handed to those verbs and corrupt the page silently, because the range is
/// in bounds.
///
/// Asserted as "the flat list contains only what the page's own stream drew",
/// which is the property those eleven sites depend on.
#[test]
fn the_flat_object_list_is_unchanged_by_recursion() {
    for name in [
        "page-sized-form.pdf",
        "nested-forms.pdf",
        "shared-form-twice.pdf",
    ] {
        let m = model(name);
        assert_eq!(
            m.objects.len(),
            form_count(&m),
            "{name}: every flat object is drawn by the PAGE's stream -- here \
             they are all `Do`s. A leaf appearing in this list is a corruption \
             hazard, not a cosmetic issue"
        );
        assert!(!m.leaves.is_empty(), "{name}: the leaves went somewhere");
    }
}

/// A leaf names its own content stream, and reports whether the OBJECT can be
/// edited.
///
/// Deliberately the same vocabulary `text_extract` uses for a `TextRun` inside
/// a form, so a form-interior path and a form-interior text run describe
/// themselves identically. A shell reconciles both in one selection; two
/// vocabularies for one fact would be its problem and our fault.
///
/// # ★ What each half now asserts, because they came apart in `Pass 188.0`
///
/// - **`stream()` is a fact about the BUFFER** and has not changed: the leaf's
///   token range indexes the form's bytes, not the page's. That is the safety
///   property, and every form-scoped verb writes to the form accordingly.
/// - **`is_editable()` is a fact about the OBJECT** and has changed. It was a
///   hard `false` meaning *"editing through the recursion is not built"*; it is
///   built, so it now means *"this leaf is a path, so there is something to
///   drag"*.
///
/// The two are asserted separately here precisely so that a future change to
/// one cannot be mistaken for a change to the other.
#[test]
fn a_leaf_names_its_own_stream_and_reports_its_editability() {
    let m = model("page-sized-form.pdf");
    let leaf = &m.leaves[0];

    let parent = leaf.parent().expect("a leaf always has an enclosing form");
    assert_eq!(
        leaf.stream(),
        ContentStreamRef::Form { object: parent.num },
        "the leaf's token range indexes the FORM's buffer, not the page's"
    );
    assert!(
        !leaf.stream().is_page(),
        "a leaf is never in the page's own stream"
    );
    assert_eq!(
        leaf.is_editable(),
        matches!(leaf.object, VectorObject::Path(_)),
        "since Pass 188.0 this answers about the object, not about whether the \
         feature exists: a path inside a form is editable through the \
         form-scoped verbs, and anything with no node to drag is not"
    );
}

/// ★★★ THE OPERATOR'S CLICK. Before: the wrapper. After: the square.
///
/// This is the whole feature in one assertion. `hit_test_point` treats a form
/// as its bounding box, so on a page-sized form it answers with the form no
/// matter where you click — *"all I get is the page selected."*
/// `hit_test_point_deep` excludes the form and answers with what is drawn
/// inside it.
#[test]
fn a_click_inside_a_page_sized_form_now_finds_the_object_not_the_wrapper() {
    let m = model("page-sized-form.pdf");
    let inside_the_first_square = Point::new(30.0, 30.0);

    // The old answer: object 0, which is the page-sized form.
    assert_eq!(
        hit_test_point(&m, inside_the_first_square, 1.0),
        Some(0),
        "the shallow test still answers with the wrapper -- unchanged on \
         purpose, because eleven editing verbs index that list"
    );

    // The new answer: a leaf, and specifically the square under the point.
    let deep = hit_test_point_deep(&m, inside_the_first_square, 1.0);
    let Some(HitTarget::Leaf(i)) = deep.first().copied() else {
        panic!("expected a leaf under the point, got {deep:?}");
    };
    let b = m.leaves[i].object.page_bbox();
    assert_eq!(
        (b.min.x.round() as i64, b.min.y.round() as i64),
        (10, 10),
        "the square the click was actually on"
    );

    // ★ And the form itself is NOT in the candidate list at all. Its `/BBox` is
    // an extent declaration (§8.10.1), not a statement about coverage.
    assert!(
        !deep.iter().any(|t| matches!(t, HitTarget::Object(_))),
        "a form must not answer a first click; it is still reachable through \
         the leaf's containment path, which is a deliberate second act"
    );
}

/// A click on empty space inside the form's bbox hits nothing.
///
/// The counterpart to the test above, and the one that proves the exclusion is
/// real rather than the leaf merely winning a race: the form's bbox covers this
/// point, so if forms were still candidates this would return the form.
#[test]
fn a_click_on_empty_space_inside_a_form_hits_nothing() {
    let m = model("page-sized-form.pdf");
    // (60,60) is inside the 200x200 form and inside none of its three squares.
    let empty = Point::new(60.0, 60.0);

    assert_eq!(
        hit_test_point(&m, empty, 1.0),
        Some(0),
        "the shallow test answers with the form, because its bbox covers this"
    );
    assert!(
        hit_test_point_deep(&m, empty, 1.0).is_empty(),
        "★ nothing is drawn here, so nothing should be selectable here"
    );
}

/// Leaves and page objects are ONE paint order, interleaved on
/// `paint_order` — not two lists concatenated.
///
/// `shared-form-twice.pdf` places the same form at (10,10) and (120,120), so
/// each invocation's leaf must be found under its own invocation's point and
/// must name that invocation's position. A concatenation would still pass a
/// single-point test; two points at two invocations is what distinguishes it.
#[test]
fn a_leaf_is_found_under_its_own_invocation() {
    let m = model("shared-form-twice.pdf");

    for (point, expected_origin) in [
        (Point::new(20.0, 20.0), (10_i64, 10_i64)),
        (Point::new(130.0, 130.0), (120, 120)),
    ] {
        let deep = hit_test_point_deep(&m, point, 1.0);
        let Some(HitTarget::Leaf(i)) = deep.first().copied() else {
            panic!("expected a leaf at {point:?}, got {deep:?}");
        };
        let b = m.leaves[i].object.page_bbox();
        assert_eq!(
            (b.min.x.round() as i64, b.min.y.round() as i64),
            expected_origin,
            "the leaf under {point:?} must belong to the invocation drawn there"
        );
    }
}

// ===========================================================================
// Pass 138.0 — the OTHER two gestures that could not see inside a form
// ===========================================================================
//
// `hit_test_point_deep` fixed the CLICK. Two sibling queries were left behind,
// and both were reported by the consuming shell within a day of it adopting
// the click fix — which is why they are noted here rather than only in a
// commit message.
//
// ★ Neither was a regression. Both were blind from the day they shipped, and
// both were INVISIBLE while selection was equally blind, because an operator
// meets the page-sized form long before they meet the marquee or the measure
// tool. Fixing one gesture is what promoted the others from "not reached yet"
// to "the next wall", and this project has now produced that shape three times
// in three days (images/shadings, analytic/mesh, click/marquee+pick).

/// ★★★ A MARQUEE MUST SELECT WHAT A CLICK SELECTS.
///
/// Two gestures that both mean "select this", disagreeing about what is
/// selectable, is an inconsistency an operator meets in the first minute. The
/// shell could not ship the click fix without this one and wrote its own
/// version rather than wait — then reported the workaround instead of keeping
/// it, because a duplicated enclosure predicate *"will drift silently: our
/// copy will keep compiling and keep returning something plausible."*
#[test]
fn a_marquee_reaches_inside_a_form() {
    let m = model("page-sized-form.pdf");
    // The three squares are at (10,10), (80,80) and (150,150), 40 pt each, so
    // this rectangle fully encloses the first two and misses the third.
    let region = Bounds {
        min: Point::new(0.0, 0.0),
        max: Point::new(130.0, 130.0),
    };

    let picked = hit_test_rect_deep(&m, region, MarqueeMode::Enclosed, FormMarquee::Exclude);
    assert_eq!(
        picked,
        vec![HitTarget::Leaf(0), HitTarget::Leaf(1)],
        "★ two squares inside the form must be selected, and the form itself \
         must not be. Before Pass 138.0 this was EMPTY — hit_test_rect filters \
         model.objects only, and the only page object here is the wrapper"
    );
}

/// The default excludes the form; `Include` is the other shipped answer.
///
/// `R206`: two defensible readings, so both ship and one is the default. The
/// shell asked which and said it could not justify a preference — *"a marquee
/// that fully encloses a form's box has arguably named the form on purpose…
/// We think that is right and we are not sure."*
///
/// ★ The tie-breaker is not which reading is more principled. It is that a
/// click can NEVER yield a form, so if a marquee can, the operator acquires by
/// one gesture — and not the other — a selection every edit verb then refuses.
/// A capability reachable only by accident is a trap, not a feature.
#[test]
fn including_forms_is_available_and_is_not_the_default() {
    let m = model("page-sized-form.pdf");
    // Encloses the whole page, so the wrapper form qualifies too.
    let all = Bounds {
        min: Point::new(-10.0, -10.0),
        max: Point::new(210.0, 210.0),
    };

    let default = hit_test_rect_deep(&m, all, MarqueeMode::Enclosed, FormMarquee::Exclude);
    assert_eq!(
        default,
        vec![HitTarget::Leaf(0), HitTarget::Leaf(1), HitTarget::Leaf(2)],
        "the default answer is the three squares and NOT the wrapper"
    );

    let with_form = hit_test_rect_deep(&m, all, MarqueeMode::Enclosed, FormMarquee::Include);
    assert!(
        with_form.contains(&HitTarget::Object(0)),
        "FormMarquee::Include must offer the wrapper as an operand"
    );
    assert_eq!(
        with_form.len(),
        4,
        "★ Include adds the container TO the leaves — it does not return the \
         container alone. That is the difference from the old shallow \
         hit_test_rect, and it is the half a caller migrating will not expect"
    );
}

/// The marquee's order is paint order, and the point query's is its reverse.
///
/// Asserted rather than left to the doc comment because the two functions look
/// like siblings and a reader will assume they agree. They deliberately do
/// not: a point query answers "which one?" and wants the winner first; a
/// marquee answers "which ones?" and a caller drawing handles or re-emitting
/// wants paint order. Guessing which order a `Vec` is in is a bug; reversing
/// at a call site is one line.
#[test]
fn the_marquee_returns_paint_order_and_the_point_query_returns_its_reverse() {
    let m = model("page-sized-form.pdf");
    let all = Bounds {
        min: Point::new(-10.0, -10.0),
        max: Point::new(210.0, 210.0),
    };
    let marquee = hit_test_rect_deep(&m, all, MarqueeMode::Enclosed, FormMarquee::Exclude);
    assert_eq!(marquee.first(), Some(&HitTarget::Leaf(0)));
    assert_eq!(marquee.last(), Some(&HitTarget::Leaf(2)));

    // The same objects under a point that hits only the last one; the point
    // query's contract is topmost-first.
    let deep = hit_test_point_deep(&m, Point::new(170.0, 170.0), 1.0);
    assert_eq!(deep.first(), Some(&HitTarget::Leaf(2)));
}

/// ★★★ THE MEASURE TOOL WAS INERT, NOT DEGRADED, ON A WRAPPED DRAWING.
///
/// `pick_line_in_page` offered only `PageObjects::objects` to the picker, and
/// a form is not a line, so a page whose drawing lives inside a form had
/// **nothing pickable at all**. That is what most CAD exports look like —
/// measured on the operator's own drawing: 129,758 page objects, one form, and
/// 10,256 leaves, every one of them a candidate line and every one invisible.
///
/// A square's edges are straight, so each square here contributes four
/// pickable segments.
#[test]
fn the_line_picker_reaches_inside_a_form() {
    let m = model("page-sized-form.pdf");
    // The bottom edge of the first square: y = 10, x from 10 to 50.
    let picked = pick_line_in_page(&m, Point::new(30.0, 10.0), 1.0)
        .expect("★ a straight edge inside the form must be pickable — this was None before");

    assert_eq!(
        picked.target,
        HitTarget::Leaf(0),
        "and the result must say WHICH LIST it came from"
    );
    assert_eq!(
        picked.page_object_index(),
        None,
        "the migration helper must refuse to hand a leaf ordinal to a caller \
         expecting a page index — those index different content streams, and a \
         number that is in range and wrong is the failure mode this Option \
         exists to prevent"
    );
    // Horizontal, 40 pt long.
    assert!((picked.length() - 40.0).abs() < 1e-6, "{picked:?}");
    assert!((picked.start.y - 10.0).abs() < 1e-6);
    assert!((picked.end.y - 10.0).abs() < 1e-6);
}

/// Whatever is picked must be the NEAREST straight segment, from either list.
///
/// The fix appends a second list to the search; this pins that it appends
/// rather than pre-empts. Without it, a change that searched leaves first and
/// short-circuited would pass every other test in this file while making the
/// pick prefer form contents over the page's own geometry — a wrong answer
/// that looks like a working feature.
#[test]
fn the_pick_is_the_nearest_segment_from_either_list() {
    let m = model("nested-forms.pdf");
    // The single leaf is a 30 pt square at (70,70) — the placement matrices of
    // the two enclosing forms move it there from the (20,20) written in the
    // innermost stream, which is exactly why the point is read off the model
    // rather than off the generator.
    let point = Point::new(85.0, 71.0);
    let Some(picked) = pick_line_in_page(&m, point, 6.0) else {
        panic!("nested-forms.pdf holds a rectangle inside two forms; expected a pick");
    };
    let d = distance_to_segment_2d(point, picked.start, picked.end);

    for leaf in &m.leaves {
        if let VectorObject::Path(p) = &leaf.object {
            for sp in p.page_subpaths() {
                let mut from = sp.start;
                for seg in &sp.segments {
                    let to = seg.end();
                    let other = distance_to_segment_2d(point, from, to);
                    assert!(
                        other >= d - 1e-9,
                        "a nearer segment exists ({other}) than the one picked ({d})"
                    );
                    from = to;
                }
            }
        }
    }
}

/// Local copy of the distance the picker uses internally.
///
/// Deliberately re-derived rather than imported: the assertion above is that
/// `pick_line_in_page` returns the nearest segment, and checking that with the
/// same private function it used would only prove the function agrees with
/// itself.
fn distance_to_segment_2d(p: Point, a: Point, b: Point) -> f64 {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let len_sq = vx.mul_add(vx, vy * vy);
    if len_sq <= f64::EPSILON {
        return (p.x - a.x).hypot(p.y - a.y);
    }
    let t = ((((p.x - a.x) * vx) + ((p.y - a.y) * vy)) / len_sq).clamp(0.0, 1.0);
    (p.x - vx.mul_add(t, a.x)).hypot(p.y - vy.mul_add(t, a.y))
}

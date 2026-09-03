//! **Cut** — copy-and-remove as ONE undo entry, for annotations, mixed
//! selections and form fields (`Pass 168.0`).
//!
//! ## What these tests are really checking
//!
//! Before this Pass, exactly one class of thing in pdfcer had all three of
//! cut, copy and paste: page content objects. Copy had three entry points
//! (`copy_objects`, `copy_annotations`, `copy_selection`) and cut had one —
//! the objects-only one. An annotation could be copied and pasted and never
//! cut; a form field could be copied and pasted and never cut.
//!
//! So the assertions here are about **two invariants that are easy to state
//! and easy to violate silently**:
//!
//! 1. **One gesture is ONE undo entry** (`R179`/`R49`). A cut that produced
//!    N undo entries would give the operator their objects back a third at a
//!    time, and they would find out by pressing undo — not by any test
//!    failing.
//! 2. **The copy half runs first, always.** A selection that cannot be
//!    carried is refused with **nothing deleted**. Reversed, a cut whose copy
//!    failed takes the objects away with nothing on the clipboard, which is
//!    the one outcome no paste can recover from.
//!
//! ## Why the two-annotations-on-one-page test is the load-bearing one
//!
//! `undo` walks a command's writes **forward**, applying each `before`. That
//! is correct only while an object appears at most once per command. Two
//! annotations on one page both rewrite that page's `/Annots`, so a naive
//! concatenation of their two commands puts the page object in twice — and
//! undoing forward would restore the ORIGINAL `/Annots` and then re-apply the
//! INTERMEDIATE one, leaving the document with one annotation still missing
//! and no further undo to reach it.
//!
//! That failure produces a **plausible** document. Nothing errors, nothing
//! looks corrupt, and the count is off by one. It is exactly the shape a test
//! has to be written for deliberately, because no gate can see it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot_author::{Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vector::Matrix;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(name)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

fn annot_count(s: &EditSession) -> usize {
    let slots = s.page_slots().expect("page slots");
    let page = slots.first().expect("a page").id;
    pdfcer_core::annot::page_annotations(&s.graph(), page).len()
}

/// A square markup at a given offset, so a test can author two distinct ones.
fn square(at: f64) -> MarkupSpec {
    MarkupSpec::Square {
        rect: Rect {
            llx: at,
            lly: at,
            urx: at + 40.0,
            ury: at + 40.0,
        },
        border: Some(Color::Rgb(1.0, 0.0, 0.0)),
        interior: None,
        border_width: 1.0,
        border_effect: None,
    }
}

// ---------------------------------------------------------------------------
// The load-bearing one
// ---------------------------------------------------------------------------

/// Cutting two annotations from ONE page is one undo entry, and undoing it
/// brings back **both**.
///
/// Both deletions rewrite the same page's `/Annots`, so this is precisely the
/// case where folding two commands together goes wrong if the duplicate
/// object is not collapsed — and it goes wrong by leaving one annotation
/// missing, which is a document that looks fine.
#[test]
fn cutting_two_annotations_from_one_page_is_one_undo_entry_and_undo_restores_both() {
    let mut s = session("hello.pdf");
    s.add_markup(0, &square(20.0)).expect("first markup");
    s.add_markup(0, &square(90.0)).expect("second markup");
    assert_eq!(annot_count(&s), 2, "precondition: two annotations");
    let depth_before = s.undo_depth();

    let clip = s.cut_annotations(0, &[0, 1]).expect("cut");
    assert_eq!(clip.annotation_count(), 2, "both are on the clipboard");
    assert_eq!(annot_count(&s), 0, "and both are off the page");
    assert_eq!(
        s.undo_depth(),
        depth_before + 1,
        "ONE undo entry for one gesture -- two deletions were folded, not stacked",
    );

    s.undo().expect("one press");
    assert_eq!(
        annot_count(&s),
        2,
        "ONE press of undo brings back BOTH. If the fold had not collapsed the \
         page's two /Annots writes, the forward-applied `before` values would \
         have left exactly one annotation missing -- a document that looks \
         fine and is wrong by one.",
    );
    assert_eq!(s.undo_depth(), depth_before);
}

/// And redo puts them both back out again — the fold has to be reversible in
/// both directions, not only undoable.
#[test]
fn redoing_a_folded_cut_removes_both_again() {
    let mut s = session("hello.pdf");
    s.add_markup(0, &square(20.0)).expect("first");
    s.add_markup(0, &square(90.0)).expect("second");
    s.cut_annotations(0, &[0, 1]).expect("cut");
    s.undo().expect("undo");
    assert_eq!(annot_count(&s), 2);
    s.redo().expect("redo");
    assert_eq!(
        annot_count(&s),
        0,
        "redo replays the folded command's `after` values, all of them",
    );
}

// ---------------------------------------------------------------------------
// Copy first, always
// ---------------------------------------------------------------------------

/// An annotation the clipboard cannot carry refuses the cut, with **nothing
/// deleted**.
///
/// Copy leaves it in place and says so. Cut would be a deletion wearing a
/// clipboard's clothes: the operator's next gesture is a paste that refuses,
/// and by then the only copy is gone.
#[test]
fn cutting_an_annotation_the_clipboard_cannot_carry_is_refused_with_nothing_deleted() {
    let mut s = session("annot/demo-annotated.pdf");
    let before = annot_count(&s);
    assert!(before > 0, "precondition");

    // Index 3 is a /Popup — refused BY POLICY, not for want of a model: a
    // pop-up is not an independent annotation (§12.5.6.14), it belongs to the
    // comment that opens it and travels with it.
    //
    // ★ This test used to use index 0, a /Stamp. `Pass 170.0` made stamps
    // carryable (they travel as their own dictionary), so the example had to
    // change — which is the point of the Pass, not a weakening of the test.
    let err = s.cut_annotations(0, &[3]).expect_err("must refuse");
    assert!(
        matches!(err, EditError::CutWouldNotSurvive { ref subtype } if subtype == "Popup"),
        "got {err:?}",
    );
    assert_eq!(
        annot_count(&s),
        before,
        "nothing was deleted -- the refusal comes BEFORE any removal",
    );

    // A copy of the same thing is fine, and loses nothing.
    let clip = s.copy_annotations(0, &[3]).expect("copy is allowed");
    assert_eq!(clip.annotation_count(), 1);
    assert_eq!(annot_count(&s), before, "and the original is still there");
}

/// One unsupported annotation refuses the whole selection, not the supported
/// remainder.
///
/// `R168`: a verb offered on an N-target selection acts on the whole
/// selection or refuses — never a silent subset. Cutting two of three and
/// leaving the third would be the silent subset.
#[test]
fn one_uncarryable_annotation_refuses_the_whole_selection() {
    let mut s = session("annot/demo-annotated.pdf");
    let before = annot_count(&s);
    // Index 2 is a /Circle (carryable); index 3 is a /Popup (refused).
    let err = s.cut_annotations(0, &[2, 3]).expect_err("must refuse");
    assert!(
        matches!(err, EditError::CutWouldNotSurvive { .. }),
        "got {err:?}"
    );
    assert_eq!(
        annot_count(&s),
        before,
        "the carryable one was NOT cut either",
    );
}

// ---------------------------------------------------------------------------
// Mixed selections
// ---------------------------------------------------------------------------

/// Content objects and annotations cut together are still one undo entry.
#[test]
fn cutting_content_and_annotations_together_is_one_undo_entry() {
    let mut s = session("hello.pdf");
    s.add_markup(0, &square(20.0)).expect("markup");
    // `hello.pdf` has four content objects; index 3 is the last, so it is
    // out of range once one has been cut -- which is how this test measures
    // that the content half happened at all without a decomposition API.
    assert!(
        s.copy_objects(0, &[3]).is_ok(),
        "precondition: the page has four content objects",
    );
    let depth_before = s.undo_depth();

    let clip = s.cut_selection(0, &[0], &[0]).expect("cut");
    assert_eq!(clip.len(), 1, "one content object on the clipboard");
    assert_eq!(clip.annotation_count(), 1, "and one annotation");
    assert_eq!(annot_count(&s), 0);
    assert!(
        s.copy_objects(0, &[3]).is_err(),
        "and the page is one content object shorter",
    );
    assert_eq!(
        s.undo_depth(),
        depth_before + 1,
        "the content delete and the annotation delete folded into one entry",
    );

    s.undo().expect("one press");
    assert_eq!(annot_count(&s), 1, "the annotation is back");
    assert!(
        s.copy_objects(0, &[3]).is_ok(),
        "and so is the content object -- one press, both halves",
    );
}

/// A cut clip pastes back, which is what makes it a cut rather than a delete.
#[test]
fn what_a_cut_carried_can_be_pasted_back() {
    let mut s = session("hello.pdf");
    s.add_markup(0, &square(20.0)).expect("markup");
    let clip = s.cut_annotations(0, &[0]).expect("cut");
    assert_eq!(annot_count(&s), 0);
    s.paste_objects(0, &clip, Matrix::translate(120.0, 0.0))
        .expect("paste");
    assert_eq!(
        annot_count(&s),
        1,
        "the annotation the cut carried came back on the page",
    );
}

// ---------------------------------------------------------------------------
// Form fields
// ---------------------------------------------------------------------------

#[test]
fn cutting_a_field_carries_it_and_removes_it_as_one_undo_entry() {
    let mut s = session("forms/rich-field-form.pdf");
    let depth_before = s.undo_depth();

    let cut = s.cut_field("TitleBlock.Revision").expect("cut");
    assert_eq!(cut.clip.source_name(), "TitleBlock.Revision");
    assert_eq!(
        cut.clip.carried_font(),
        Some(b"TB".as_slice()),
        "the clip is the full one -- cut is copy plus delete, not a lesser copy",
    );
    assert!(cut.deletion.field_removed);
    assert_eq!(
        cut.deletion.emptied_parents, 1,
        "the grouping node it left childless was pruned, and the cut says so",
    );
    assert!(
        forms::parse_acroform(&s.graph())
            .expect("form")
            .fields
            .is_empty(),
        "the field is gone",
    );
    assert_eq!(s.undo_depth(), depth_before + 1, "ONE undo entry");

    s.undo().expect("one press");
    assert_eq!(
        forms::parse_acroform(&s.graph())
            .expect("form")
            .fields
            .len(),
        1,
        "one press of undo puts the whole field back",
    );
}

/// A cut field pastes into another document — the point of cutting it.
#[test]
fn a_cut_field_pastes_into_another_document() {
    use pdfcer_core::formclip::{FieldPastePolicy, PasteTooltip};
    let mut source = session("forms/rich-field-form.pdf");
    let cut = source.cut_field("TitleBlock.Revision").expect("cut");

    let mut destination = session("hello.pdf");
    destination
        .paste_field(
            &cut.clip,
            0,
            Rect {
                llx: 40.0,
                lly: 40.0,
                urx: 200.0,
                ury: 64.0,
            },
            &FieldPastePolicy::NewField {
                name: "Moved".to_owned(),
                tooltip: PasteTooltip::Carry,
                copy_value: false,
                copy_actions: false,
            },
        )
        .expect("paste");
    let form = forms::parse_acroform(&destination.graph()).expect("form");
    assert_eq!(form.fields.len(), 1);
    assert_eq!(
        form.fields.first().map(|f| f.fully_qualified_name.as_str()),
        Some("Moved"),
    );
}

/// A SIGNED signature field refuses the cut, with nothing deleted.
///
/// This matters more than refusing the copy: a cut that carried nothing would
/// have **deleted a signature** and left the operator holding an empty
/// clipboard.
#[test]
fn cutting_a_signed_signature_field_is_refused_with_nothing_deleted() {
    let mut s = session("signature/signed-full-coverage.pdf");
    let err = s.cut_field("Approval").expect_err("must refuse");
    assert!(
        matches!(err, EditError::SignedFieldNotCopyable { .. }),
        "got {err:?}",
    );
    assert_eq!(
        forms::parse_acroform(&s.graph())
            .expect("form")
            .fields
            .len(),
        1,
        "the signature is still there",
    );
    assert_eq!(s.undo_depth(), 0, "and nothing was committed");
}

// ---------------------------------------------------------------------------
// The two defects this Pass found
// ---------------------------------------------------------------------------

/// Removing a calculated field prunes it from `/AcroForm /CO`.
///
/// §12.7.2 Table 218 makes `/CO` "an array of indirect references to field
/// dictionaries with calculation actions". Leaving a reference behind is not
/// corruption — §7.3.10 resolves a dangling reference to null — but it is a
/// lie the file tells about itself, and it is COUNTED: before this fix,
/// `list-fields` on a document whose only field had just been deleted
/// reported `fields=0 calc_order=1`.
#[test]
fn removing_a_calculated_field_prunes_the_calculation_order() {
    let mut s = session("forms/rich-field-form.pdf");
    let before = forms::parse_acroform(&s.graph()).expect("form");
    assert_eq!(
        before.calc_order.len(),
        1,
        "precondition: the fixture's field is in /CO",
    );

    s.delete_field("TitleBlock.Revision").expect("delete");
    let after = forms::parse_acroform(&s.graph()).expect("form");
    assert!(after.fields.is_empty());
    assert!(
        after.calc_order.is_empty(),
        "the calculation order must not name a field that is gone",
    );

    s.undo().expect("undo");
    assert_eq!(
        forms::parse_acroform(&s.graph())
            .expect("form")
            .calc_order
            .len(),
        1,
        "and the prune undoes with the deletion it was part of",
    );
}

/// The paste refusal names the copied subtype's OWN reason.
///
/// Both paste sites used to carry one hardcoded sentence about **widgets** —
/// `/AcroForm` registration, field names, calculation order — printed
/// whatever had been copied. Copying a `/Link` was answered with an
/// explanation of form-field renaming. The prose was accurate about a
/// different object, it was duplicated verbatim in two sites, and it was
/// well-formed enough that nothing could notice.
///
/// ★ Uses a `/Popup`, and used to use a `/Stamp`. `Pass 170.0` made stamps
/// carryable, so the example had to move to something still refused — which
/// is the Pass working, not the test weakening.
#[test]
fn the_paste_refusal_explains_the_subtype_that_was_actually_copied() {
    let mut s = session("annot/demo-annotated.pdf");
    let clip = s.copy_annotations(0, &[3]).expect("copy the /Popup");
    let outcome = s
        .paste_objects(0, &clip, Matrix::IDENTITY)
        .expect("paste places nothing but reports");
    let said = outcome.disclosures.join(" ");
    assert!(said.contains("/Popup"), "it names the subtype: {said:?}");
    assert!(
        !said.contains("AcroForm"),
        "and it does NOT explain form-field registration for a pop-up: {said:?}",
    );

    // The preview must say the same thing as the verb -- they are two sites
    // and used to hold two copies of one string.
    let preview = s
        .paste_preview(0, &clip, Matrix::IDENTITY)
        .expect("preview");
    assert_eq!(
        preview.disclosures, outcome.disclosures,
        "the preview and the verb must not drift",
    );
}

/// A widget still gets the widget reason — and it now points at the verb that
/// exists for it.
#[test]
fn a_copied_widget_is_pointed_at_the_field_clipboard() {
    let mut s = session("forms/rich-field-form.pdf");
    let clip = s.copy_annotations(0, &[0]).expect("copy the widget");
    let outcome = s.paste_objects(0, &clip, Matrix::IDENTITY).expect("paste");
    let said = outcome.disclosures.join(" ");
    assert!(said.contains("/Widget"), "{said:?}");
    assert!(
        said.contains("copy-field"),
        "the refusal points at the verb that CAN do this, which has existed \
         since Pass 167.0: {said:?}",
    );
}

// ---------------------------------------------------------------------------
// Guards
// ---------------------------------------------------------------------------

/// An out-of-range annotation index refuses before anything is deleted.
#[test]
fn an_out_of_range_index_refuses_the_cut() {
    let mut s = session("annot/demo-annotated.pdf");
    let before = annot_count(&s);
    assert!(s.cut_annotations(0, &[99]).is_err());
    assert_eq!(annot_count(&s), before);
}

/// An empty selection is a no-op, not an error, and commits nothing.
#[test]
fn an_empty_selection_cuts_nothing_and_commits_nothing() {
    let mut s = session("annot/demo-annotated.pdf");
    let before = annot_count(&s);
    let clip = s.cut_selection(0, &[], &[]).expect("an empty cut is legal");
    assert_eq!(clip.len(), 0);
    assert_eq!(clip.annotation_count(), 0);
    assert_eq!(annot_count(&s), before);
    assert_eq!(s.undo_depth(), 0, "nothing reached the undo stack");
}

/// The `/CO` prune leaves a document that never had one alone.
#[test]
fn a_document_with_no_calculation_order_is_not_given_one() {
    let mut s = session("forms/demo-form.pdf");
    s.delete_field("FullName").expect("delete");
    let graph = s.graph();
    let catalog = graph.catalog_dict().expect("catalog");
    let acroform = graph
        .resolve(catalog.get(b"AcroForm").expect("/AcroForm"))
        .as_dict()
        .expect("dict");
    assert!(
        acroform.get(b"CO").is_none(),
        "a /CO must not be invented for a document that had none -- writing a \
         key that changes nothing is a minimal-diff violation",
    );
}

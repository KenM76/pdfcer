//! # `unshare_form` — giving one page a private copy of a shared form XObject
//!
//! # ★★★ WHY THIS TEST FILE EXISTS AT ALL
//!
//! `ARCHITECTURE.md` §12 decision 076 ruled that editing content inside a
//! shared form XObject is **edit-in-place, disclosed**, and argued its `R206`
//! compliance — *two defensible behaviours ship as two options with a chosen
//! default* — on the premise that **"both are shipped"**.
//!
//! **That premise was false.** `Pass 119.1` was filed the same day and never
//! built. So the decision certified its own compliance with a standing rule
//! using a fact that was not true, and for a week the operator had the default
//! and **no option at all** — which is the state `R206` exists to prevent.
//!
//! It surfaced only because a consuming project asked to be ruled on what
//! happens when you move an object inside a shared form, and the reply was one
//! sentence from citing an escape hatch that had never existed.
//!
//! # The acceptance criterion, quoted from the Pass
//!
//! > *"an unshare followed by an edit changes exactly one page (verified by
//! > render), every other invocation site byte-identical."*
//!
//! That is unstatable on a one-page document and unfalsifiable on one whose
//! pages invoke different forms, which is why
//! `fixtures/synthetic/forms-xobject/shared-across-two-pages.pdf` exists: two
//! pages, **one** shared form object, nothing else shared.
//!
//! # The properties asserted
//!
//! | Property | Asserted by |
//! |---|---|
//! | the page gets its own copy, and the other page does not | `unsharing_re_points_only_the_named_page` |
//! | an INHERITED `/Resources` is privatised first, or the re-point leaks | `an_inherited_resources_dict_is_privatised_before_re_pointing` |
//! | undo puts the page back to sharing | `undo_restores_the_sharing` |
//! | a nested invocation is refused **by name**, not silently mishandled | `a_form_reached_only_through_another_form_is_refused` |
//! | a form that is not on the page is a different refusal | `a_form_that_is_not_on_the_page_is_refused_differently` |
//!
//! ## ★★ The inheritance test is the one that would fail a naive implementation
//!
//! A page with no `/Resources` of its own uses an ancestor's (§7.7.3.4). Re-
//! pointing the `/XObject` name *in place* there would re-point it for **every
//! page under that ancestor** — the exact leak this verb exists to prevent,
//! producing a "private" copy that is still shared. It looks like it works on
//! any fixture whose pages carry their own resources, which is most of them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};

fn session(name: &str) -> EditSession {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms-xobject")
        .join(name);
    let doc = Document::load(Path::new(&path)).expect("fixture loads");
    EditSession::new(doc)
}

/// Which object `page_index`'s `/XObject` `/Fm0` resolves to, read through the
/// session's own view so it reflects uncommitted edits.
fn fm0_of(session: &EditSession, page_index: usize) -> ObjId {
    let pages = session.pages().expect("page tree");
    let page = &pages[page_index];
    let view = session.view();
    let xobjects = page
        .resources
        .get(b"XObject")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
        .cloned()
        .expect("the page has an /XObject subdictionary");
    xobjects
        .get(b"Fm0")
        .and_then(Object::as_reference)
        .expect("/Fm0 is an indirect reference")
}

/// ★ THE ACCEPTANCE CRITERION. One page moves; the other does not.
#[test]
fn unsharing_re_points_only_the_named_page() {
    let mut s = session("shared-across-two-pages.pdf");

    let before_0 = fm0_of(&s, 0);
    let before_1 = fm0_of(&s, 1);
    assert_eq!(
        before_0, before_1,
        "the fixture must start SHARED, or this test proves nothing"
    );

    let report = s.unshare_form(0, before_0).expect("unshare succeeds");
    assert_eq!(report.original, before_0);
    assert_ne!(report.copy, before_0, "a copy is a different object");
    assert_eq!(report.references_moved, 1);

    let after_0 = fm0_of(&s, 0);
    let after_1 = fm0_of(&s, 1);
    assert_eq!(after_0, report.copy, "page 0 now names its own copy");
    assert_eq!(
        after_1, before_1,
        "★ page 1 must be untouched -- if this moves, 'private' copy is a \
         misnomer and every other invocation site changed too"
    );
    assert_ne!(after_0, after_1, "the two pages no longer share");

    // The copy resolves to a real form stream, not a dangling reference.
    let view = s.view();
    assert!(
        matches!(view.resolved(report.copy), Object::Stream(_)),
        "the copy must be a usable stream object"
    );
}

/// ★★ An INHERITED `/Resources` is privatised before the re-point.
///
/// Neither page in this fixture carries `/Resources`; the `Pages` node does.
/// Re-pointing the name in place would re-point it for both pages — a "private"
/// copy that is still shared, which is worse than refusing.
#[test]
fn an_inherited_resources_dict_is_privatised_before_re_pointing() {
    let mut s = session("inherited-resources-shared-form.pdf");

    let before_0 = fm0_of(&s, 0);
    assert_eq!(before_0, fm0_of(&s, 1), "shared to begin with");

    let report = s.unshare_form(0, before_0).expect("unshare succeeds");

    assert_eq!(fm0_of(&s, 0), report.copy);
    assert_eq!(
        fm0_of(&s, 1),
        before_0,
        "★ page 1 still names the ORIGINAL. If it names the copy, the \
         re-point was written into the INHERITED dictionary and leaked onto \
         every page under that ancestor"
    );

    // And page 0 now carries resources of its own, which is what made the
    // re-point safe rather than a lucky ordering.
    let pages = s.pages().expect("page tree");
    let page0 = s
        .value(pages[0].id)
        .and_then(Object::as_dict)
        .cloned()
        .expect("page 0 is a dictionary");
    assert!(
        page0.contains_key(b"Resources"),
        "page 0 must have been given its own /Resources"
    );
}

/// Undo puts the page back to sharing.
#[test]
fn undo_restores_the_sharing() {
    let mut s = session("shared-across-two-pages.pdf");
    let before = fm0_of(&s, 0);

    s.unshare_form(0, before).expect("unshare succeeds");
    assert_ne!(fm0_of(&s, 0), before);

    assert_eq!(s.undo(), Some(CommandKind::UnshareForm));
    assert_eq!(
        fm0_of(&s, 0),
        before,
        "undo must put the page back to naming the shared form"
    );
    assert!(
        s.dirty_set().is_empty(),
        "undo must restore the byte-identical original"
    );
}

/// ★★ A form reached only from INSIDE another form is refused, by name.
///
/// Re-binding a nested invocation means editing the **parent** form, which may
/// itself be shared — so the blast radius would depend on the document's
/// nesting structure. Decision 076's own decisive reason forbids that, and the
/// refusal is the honest answer rather than a silent partial success.
#[test]
fn a_form_reached_only_through_another_form_is_refused() {
    let mut s = session("nested-forms.pdf");

    // The INNER form: named by the outer form's resources, never by the page's.
    let pages = s.pages().expect("page tree");
    let model = pdfcer_core::vector::decompose_page(
        &s.view(),
        &pages[0],
        pdfcer_core::vector::Matrix::IDENTITY,
    )
    .expect("decomposes");
    let inner = *model.leaves[0]
        .containment
        .last()
        .expect("the leaf is inside a form");

    let err = s
        .unshare_form(0, inner)
        .expect_err("a nested invocation must be refused");
    assert!(
        matches!(err, EditError::FormNestedInAnotherForm { .. }),
        "expected FormNestedInAnotherForm, got {err:?}"
    );
    assert!(
        s.dirty_set().is_empty(),
        "a refused unshare must change nothing"
    );
}

/// A form that is not on the page at all is a *different* refusal.
///
/// The two lead a caller to different next actions — "unshare the outer form
/// instead" versus "you have the wrong page or the wrong object" — so
/// collapsing them into one error would lose the only information that helps.
#[test]
fn a_form_that_is_not_on_the_page_is_refused_differently() {
    let mut s = session("shared-across-two-pages.pdf");
    let absent = ObjId::new(9999, 0);

    let err = s
        .unshare_form(0, absent)
        .expect_err("an absent form must be refused");
    assert!(
        matches!(err, EditError::FormNotOnPage { .. }),
        "expected FormNotOnPage, got {err:?}"
    );
    assert!(s.dirty_set().is_empty());
}

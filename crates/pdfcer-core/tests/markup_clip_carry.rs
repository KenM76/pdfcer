//! # A copied markup comes back as the one that was copied
//!
//! ## Four properties the clipboard silently dropped
//!
//! `ClipAnnotation::Markup` carried a `MarkupSpec` and nothing else. The spec
//! describes the **shape**; four things an operator can see live *beside* it
//! as author-time options and were therefore lost on every copy-paste:
//!
//! | key | what was lost |
//! |---|---|
//! | `/BS` `/S` + `/D` | a dashed revision cloud came back **solid** |
//! | `/CA` | a 50 %-opacity highlight came back **opaque** |
//! | `/Contents` | a comment came back **blank** |
//! | `/T` | and **unsigned** |
//!
//! **Nothing disclosed any of it**, because from the paste's point of view it
//! was authoring a fresh mark and there was nothing to report.
//!
//! ★ The dash is the sharpest of the four. `docs/FEATURES.md` enumerated the
//! four appearance-regeneration routes that used to solidify a dash —
//! *"restyle, resize, reshape, author"* — and read as though the class were
//! closed. **Copy-paste was a fifth route and was not in the list**, which is
//! the shape `R245` names: a fix applied to some of a family and not the
//! rest.
//!
//! ## Why the properties are still not IN the spec
//!
//! Keeping them out is right: the spec is also what `reshape_annotation` and
//! `set_markup_style` rebuild from, and neither of those should be able to
//! change an author's name. So they travel as a sibling `MarkupCarry`, and
//! the paste applies them through the **same `MarkupOptions` authoring uses**
//! — one code path, so a pasted mark and a freshly-authored one cannot
//! disagree about how a dash or an opacity is written.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::{Annotation, page_annotations};
use pdfcer_core::annot_author::{BorderDash, Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, MarkupNote, MarkupOptions};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::vector::Matrix;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session() -> EditSession {
    EditSession::new(
        Document::load(&fixture("annot/demo-annotated.pdf")).expect("load the fixture"),
    )
}

/// A dashed, half-opaque square carrying a note and an author.
fn author_rich_square(s: &mut EditSession) -> ObjId {
    let spec = MarkupSpec::Square {
        rect: pdfcer_core::page_tree::Rect {
            llx: 40.0,
            lly: 40.0,
            urx: 140.0,
            ury: 100.0,
        },
        border: Some(Color::Gray(0.0)),
        interior: None,
        border_width: 2.0,
        border_effect: None,
    };
    let opts = MarkupOptions {
        dash: BorderDash::new(vec![4.0, 2.0]),
        opacity: Some(0.5),
        note: Some(MarkupNote::new("a note that must survive").by("Ken")),
    };
    s.add_markup_with(0, &spec, &opts).expect("author")
}

/// The index into page 0's `/Annots` of the annotation with this id.
///
/// `copy_annotations` addresses annotations by their **position in
/// `/Annots`**, not by object id — a different address space from the one
/// `add_markup_with` hands back, and the reason this helper exists rather
/// than the id being passed straight through.
fn index_of(s: &EditSession, id: ObjId) -> usize {
    annots(s)
        .iter()
        .position(|a| a.id == Some(id))
        .expect("the annotation must be on page 0")
}

/// The `/BS` `/D` array on an annotation, if any.
fn dash_of(s: &EditSession, id: ObjId) -> Option<Vec<f64>> {
    let g = s.graph();
    let d = g.resolved(id).as_dict().cloned()?;
    let Some(Object::Dict(bs)) = d.get(b"BS").map(|o| g.resolve(o).clone()) else {
        return None;
    };
    let arr = bs.get(b"D").map(|o| g.resolve(o).clone())?;
    Some(
        arr.as_array()?
            .iter()
            .filter_map(Object::as_number)
            .collect(),
    )
}

/// The annotations on page 0, newest last.
fn annots(s: &EditSession) -> Vec<Annotation> {
    let slots = s.page_slots().expect("slots");
    page_annotations(&s.graph(), slots[0].id)
}

/// ★★ All four properties survive a copy and paste.
///
/// Asserted on the PASTED annotation read back out of the session, not on the
/// clip — a clip that carries a value and a paste that drops it would pass
/// any test that only inspected the clipboard.
#[test]
fn a_pasted_markup_keeps_its_dash_opacity_note_and_author() {
    let mut s = session();
    let src = author_rich_square(&mut s);
    let before = annots(&s).len();

    let at = index_of(&s, src);
    let clip = s.copy_annotations(0, &[at]).expect("copy");
    s.paste_objects(0, &clip, Matrix::translate(200.0, 0.0))
        .expect("paste onto the same page, offset");

    let after = annots(&s);
    assert_eq!(after.len(), before + 1, "the paste must add one annotation");
    let pasted = after.last().expect("the pasted one");
    let pid = pasted.id.expect("an id");
    assert_ne!(pid, src, "the paste must be a NEW object, not the source");

    assert_eq!(
        dash_of(&s, pid),
        Some(vec![4.0, 2.0]),
        "the dash must survive — a dashed revision cloud coming back solid is \
         the defect this test exists for, and copy-paste was the fifth \
         regeneration route to have it"
    );
    assert_eq!(
        pasted.constant_alpha,
        Some(0.5),
        "the opacity must survive; an opaque paste of a 50% mark is a visible \
         change nothing disclosed"
    );
    assert_eq!(
        pasted.contents.as_deref(),
        Some("a note that must survive"),
        "the note text must survive"
    );
    assert_eq!(
        pasted.title.as_deref(),
        Some("Ken"),
        "the author must survive — a comment that arrives unsigned has lost \
         the thing that makes it a comment"
    );
}

/// The source is untouched by the copy.
///
/// Cheap, and it guards the shape where a "carry" implementation moves a
/// property instead of copying it.
#[test]
fn copying_does_not_disturb_the_source() {
    let mut s = session();
    let src = author_rich_square(&mut s);
    let at = index_of(&s, src);
    let _ = s.copy_annotations(0, &[at]).expect("copy");

    assert_eq!(dash_of(&s, src), Some(vec![4.0, 2.0]));
    let a = annots(&s);
    let source = a.iter().find(|x| x.id == Some(src)).expect("the source");
    assert_eq!(source.constant_alpha, Some(0.5));
    assert_eq!(source.title.as_deref(), Some("Ken"));
}

/// A mark with NONE of the four pastes cleanly and gains none of them.
///
/// `None` in the carry means *the source did not have one*, not "use a
/// default" — so a plain square must not arrive carrying an empty note or an
/// invented opacity. Without this, an implementation that defaulted every
/// absent field would pass the round-trip test above and quietly add keys to
/// every plain mark anybody copied.
#[test]
fn a_plain_markup_gains_nothing_it_did_not_have() {
    let mut s = session();
    let spec = MarkupSpec::Square {
        rect: pdfcer_core::page_tree::Rect {
            llx: 40.0,
            lly: 40.0,
            urx: 90.0,
            ury: 70.0,
        },
        border: Some(Color::Gray(0.0)),
        interior: None,
        border_width: 1.0,
        border_effect: None,
    };
    let src = s.add_markup(0, &spec).expect("author a plain square");

    let at = index_of(&s, src);
    let clip = s.copy_annotations(0, &[at]).expect("copy");
    s.paste_objects(0, &clip, Matrix::translate(150.0, 0.0))
        .expect("paste");

    let pasted = annots(&s).pop().expect("the pasted one");
    let pid = pasted.id.expect("an id");
    assert_eq!(
        dash_of(&s, pid),
        None,
        "no dash was copied, so none is written"
    );
    assert_eq!(pasted.constant_alpha, None, "no /CA invented");
    assert_eq!(pasted.contents, None, "no empty note invented");
    assert_eq!(pasted.title, None, "no author invented");
}

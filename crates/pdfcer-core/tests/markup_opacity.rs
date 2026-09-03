//! # `Pass 81.1` — authoring a markup annotation AT an opacity
//!
//! ## The defect, which is an undo defect and not an ergonomic one
//!
//! `/CA` (ISO 32000-1 §12.5.2 Table 164 — the constant alpha with which a
//! markup annotation is composited onto the page) was reachable only through
//! [`EditSession::set_markup_style`], a **restyle** verb, which by definition
//! acts on an annotation that already exists. So a shell placing a
//! 40 %-opaque highlight had to author it opaque and then restyle it:
//!
//! ```text
//! let id = session.add_markup(page, &spec)?;         // opaque
//! session.set_markup_style(id, &MarkupStyle {        // now translucent
//!     opacity: Some(StyleEdit::Set(0.4)),
//!     ..Default::default()
//! })?;
//! ```
//!
//! Two verbs — and **two undo entries**. An operator who draws a translucent
//! highlight and presses Ctrl+Z once gets an **opaque highlight**: a state
//! they never asked for and could not have created any other way. That reads
//! as a bug in undo, and it is one.
//!
//! The consuming shell reported it under decision 058 rather than coalescing
//! the pair on its own side. That is what let it be fixed where it belongs; a
//! shell-side coalesce would have worked and left every other consumer with
//! the same defect.
//!
//! ## Both authoring routes, in one Pass
//!
//! Table 164 is the **markup annotation** entry list, and `FreeText`, `Text`
//! (sticky note) and `Stamp` are markup annotations exactly as `Square` and
//! `Highlight` are. pdfcer has two authoring verbs — `add_markup` for the
//! geometric subtypes and `add_text_annotation` for the text-bearing ones —
//! and both got the option in the same Pass.
//!
//! The requesting shell asked about geometric markup only. **That is not the
//! same question as "which annotations does `/CA` apply to"**, and the
//! standard answers the second one. Shipping one route would have reproduced
//! the failure this project hit three times in three days: fixing one route
//! makes the others look broken, and the shell discovers the asymmetry by
//! pressing a control that works on a rectangle and not on a sticky note.

use pdfcer_core::annot_author::{Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, MarkupOptions};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(name)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

fn square() -> MarkupSpec {
    MarkupSpec::Square {
        rect: Rect {
            llx: 10.0,
            lly: 10.0,
            urx: 60.0,
            ury: 40.0,
        },
        border: Some(Color::Gray(0.0)),
        interior: None,
        border_width: 1.0,
        border_effect: None,
    }
}

/// The `/CA` value on the annotation `id` names, read back off the session's
/// own graph — the same view a viewer would resolve.
fn constant_alpha(s: &EditSession, id: pdfcer_core::object::ObjId) -> Option<f64> {
    let graph = s.graph();
    match graph.value(id)? {
        Object::Dict(d) => d.get(b"CA").and_then(Object::as_number),
        _ => None,
    }
}

/// The headline: one verb, one `/CA`, **one undo entry**.
#[test]
fn authoring_at_an_opacity_is_one_command() {
    let mut s = session("annot/no-ap-circle.pdf");
    let before = s.undo_depth();
    let id = s
        .add_markup_with(
            0,
            &square(),
            &MarkupOptions {
                opacity: Some(0.4),
                ..Default::default()
            },
        )
        .expect("authoring a translucent square");

    assert_eq!(constant_alpha(&s, id), Some(0.4));
    assert_eq!(
        s.undo_depth() - before,
        1,
        "ONE undoable command. The whole point of this Pass is that the \
         author-then-restyle pair left an OPAQUE annotation on the page after \
         a single Ctrl+Z — a state the operator never asked for"
    );

    // And undoing it once removes the annotation entirely, rather than
    // leaving the opaque intermediate the old two-verb sequence did.
    s.undo().expect("undo");
    assert!(
        s.graph().value(id).is_none(),
        "one undo must remove the annotation, not reveal an opaque one"
    );
}

/// `add_markup` is `add_markup_with(.., &MarkupOptions::default())`.
///
/// Pinned so the delegation cannot be "simplified" into two bodies that then
/// drift — which is `R92`'s failure mode, and is how `/CA` came to be
/// reachable from the restyle verb and not the author verb in the first
/// place.
#[test]
fn the_plain_verb_is_the_options_verb_with_defaults() {
    let mut a = session("annot/no-ap-circle.pdf");
    let mut b = session("annot/no-ap-circle.pdf");
    let ia = a.add_markup(0, &square()).expect("plain");
    let ib = b
        .add_markup_with(0, &square(), &MarkupOptions::default())
        .expect("with defaults");
    assert_eq!(ia, ib, "same object number, same allocation order");
    assert_eq!(constant_alpha(&a, ia), None);
    assert_eq!(constant_alpha(&b, ib), None);
}

/// **`None` omits the key; it does not write `1.0`.**
///
/// Table 164's default is 1.0, so writing it explicitly adds a key that
/// changes nothing — and makes a pdfcer-authored opaque annotation textually
/// distinguishable from every other producer's, for no gain. Asserted
/// because "write the default explicitly, it is clearer" is a plausible
/// later edit that would silently change every authored annotation's bytes.
#[test]
fn an_absent_opacity_writes_no_key_at_all() {
    let mut s = session("annot/no-ap-circle.pdf");
    let id = s
        .add_markup_with(
            0,
            &square(),
            &MarkupOptions {
                opacity: None,
                ..Default::default()
            },
        )
        .expect("authoring without an opacity");
    let graph = s.graph();
    let Some(Object::Dict(d)) = graph.value(id) else {
        panic!("the annotation is a dictionary");
    };
    assert!(
        d.get(b"CA").is_none(),
        "the key must be ABSENT, not present with the standard's default"
    );
}

/// Fully opaque is still writable **explicitly**, and is not the same bytes
/// as absent.
///
/// The two render identically, which is exactly why this needs pinning: a
/// caller round-tripping `Some(1.0)` through pdfcer must get its key back.
/// Collapsing `Some(1.0)` to `None` "because it renders the same" would be
/// pdfcer deciding what the caller meant.
#[test]
fn an_explicit_one_is_written_and_is_not_the_same_as_absent() {
    let mut s = session("annot/no-ap-circle.pdf");
    let id = s
        .add_markup_with(
            0,
            &square(),
            &MarkupOptions {
                opacity: Some(1.0),
                ..Default::default()
            },
        )
        .expect("authoring fully opaque, explicitly");
    assert_eq!(constant_alpha(&s, id), Some(1.0));
}

/// The two boundary values are accepted, not refused off-by-one.
#[test]
fn zero_and_one_are_both_in_range() {
    for alpha in [0.0, 1.0] {
        let mut s = session("annot/no-ap-circle.pdf");
        let id = s
            .add_markup_with(
                0,
                &square(),
                &MarkupOptions {
                    opacity: Some(alpha),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|e| panic!("alpha {alpha} must be accepted: {e}"));
        assert_eq!(constant_alpha(&s, id), Some(alpha));
    }
}

/// **Out of range is REFUSED, not clamped — and nothing is authored.**
///
/// The opposite of what [`MarkupStyle::opacity`] does, and the asymmetry is
/// the point. A *restyle* corrects a value that already exists on an
/// annotation the operator can see, so clamping keeps that annotation
/// renderable and visibly changes it. An *author* call with alpha 4.0 is a
/// caller bug: clamping would put a fully opaque annotation on the page while
/// returning `Ok`, and the shell would report success for a gesture whose
/// whole point was the transparency.
///
/// The "nothing was authored" half is asserted separately from the error,
/// because a refusal that had already allocated objects or pushed a command
/// would leave the session dirty and the next undo would do something
/// unrelated.
#[test]
fn an_out_of_range_opacity_is_refused_with_nothing_authored() {
    for bad in [4.0, -0.5, f64::NAN, f64::INFINITY] {
        let mut s = session("annot/no-ap-circle.pdf");
        let depth = s.undo_depth();
        let err = s
            .add_markup_with(
                0,
                &square(),
                &MarkupOptions {
                    opacity: Some(bad),
                    ..Default::default()
                },
            )
            .expect_err("out of range must refuse");
        assert!(
            matches!(err, EditError::MarkupOpacityOutOfRange { .. }),
            "refused BY NAME, not folded into a generic error: {err}"
        );
        assert_eq!(
            s.undo_depth(),
            depth,
            "a refusal must leave the session untouched — no allocation, no \
             command pushed, nothing for a later undo to trip over"
        );
    }
}

/// **The text-bearing route carries it too**, and this is the falsifier for
/// the "one route only" version of this Pass.
///
/// `Text` (a sticky note) is a markup annotation, so Table 164's `/CA`
/// applies to it exactly as it does to a `Square`. Without this test the Pass
/// could have shipped covering geometric markup alone and looked complete.
#[test]
fn a_sticky_note_is_a_markup_annotation_and_takes_an_opacity() {
    use pdfcer_core::annot_author::{StickyIcon, TextAnnotSpec};

    let mut s = session("annot/no-ap-circle.pdf");
    let spec = TextAnnotSpec::Sticky {
        rect: Rect {
            llx: 100.0,
            lly: 100.0,
            urx: 120.0,
            ury: 120.0,
        },
        icon: StickyIcon::Note,
        contents: "hello".to_owned(),
        color: Color::Rgb(1.0, 1.0, 0.0),
        open: false,
    };
    let id = s
        .add_text_annotation_with(
            0,
            &spec,
            &MarkupOptions {
                opacity: Some(0.25),
                ..Default::default()
            },
        )
        .expect("a sticky note at 25%");
    assert_eq!(
        constant_alpha(&s, id),
        Some(0.25),
        "ISO 32000-1 §12.5.2 Table 164 is the MARKUP ANNOTATION entry list; \
         a sticky note is a markup annotation"
    );
}

/// The text route refuses out of range on the same terms as the geometric
/// one.
///
/// Asserted rather than assumed: two verbs validating the same value is two
/// places the rule can drift, and "the other one already checks it" is how a
/// second route ends up not checking at all.
#[test]
fn the_text_route_refuses_out_of_range_identically() {
    use pdfcer_core::annot_author::{StickyIcon, TextAnnotSpec};

    let mut s = session("annot/no-ap-circle.pdf");
    let spec = TextAnnotSpec::Sticky {
        rect: Rect {
            llx: 100.0,
            lly: 100.0,
            urx: 120.0,
            ury: 120.0,
        },
        icon: StickyIcon::Note,
        contents: "hello".to_owned(),
        color: Color::Rgb(1.0, 1.0, 0.0),
        open: false,
    };
    let err = s
        .add_text_annotation_with(
            0,
            &spec,
            &MarkupOptions {
                opacity: Some(-1.0),
                ..Default::default()
            },
        )
        .expect_err("out of range must refuse on this route too");
    assert!(matches!(err, EditError::MarkupOpacityOutOfRange { .. }));
}

/// **`/CA` goes on the dictionary, never into the appearance stream.**
///
/// The load-bearing placement claim, and the one a later "optimisation" would
/// most plausibly break by folding the alpha into the appearance's own `gs`.
/// §12.5.2 makes `/CA` composite the *annotation* onto the page, and pdfcer's
/// generated appearances deliberately leave their graphics-state alpha at
/// 1.0 — so if the alpha were also written into the stream, a 0.5 authored
/// here and a 0.5 restyled later would render at **0.25** while every
/// dictionary in the file said 0.5.
///
/// # Why this asserts on `/ExtGState` rather than on the stream's operators
///
/// Constant alpha is not expressible in a content stream directly. §11.6.4.4
/// Table 58 puts `ca`/`CA` in an **`/ExtGState` dictionary**, reached by a
/// `gs` operator that names an entry in the resource dictionary's
/// `/ExtGState`. So an appearance whose resources carry no `/ExtGState` at
/// all **cannot** set an alpha, whatever its operators say — which makes this
/// a stronger assertion than grepping the decoded bytes for `gs`, and one
/// that does not depend on how the stream happens to be spelled.
#[test]
fn the_appearance_stream_can_carry_no_alpha_of_its_own() {
    let mut s = session("annot/no-ap-circle.pdf");
    let id = s
        .add_markup_with(
            0,
            &square(),
            &MarkupOptions {
                opacity: Some(0.4),
                ..Default::default()
            },
        )
        .expect("authoring");

    let graph = s.graph();
    let Some(Object::Dict(annot)) = graph.value(id) else {
        panic!("the annotation is a dictionary");
    };
    let Some(Object::Dict(ap)) = annot.get(b"AP") else {
        panic!("R44: pdfcer always authors an /AP");
    };
    let Some(Object::Reference(n)) = ap.get(b"N") else {
        panic!("/AP /N is an indirect reference");
    };
    let Some(Object::Stream(stream)) = graph.value(*n) else {
        panic!("the appearance is a stream");
    };
    match stream.dict.get(b"Resources") {
        None => {}
        Some(Object::Dict(res)) => assert!(
            res.get(b"ExtGState").is_none(),
            "an /ExtGState in the appearance's resources is the ONLY way its \
             operators could set an alpha, and that alpha would COMPOUND with \
             /CA — rendering at alpha squared while every dictionary in the \
             file reported the single value"
        ),
        Some(other) => panic!("/Resources must be a dictionary, got {other:?}"),
    }
}

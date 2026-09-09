//! # A sticky note's refusal was correct and its sentence was false
//!
//! ## The report
//!
//! From the consuming shell, against `00ddbb1` — the commit that fixed the
//! same false sentence on `/FreeText` and left it standing on `/Text`:
//!
//! | authored by | subtype | before `00ddbb1` | after |
//! |---|---|---|---|
//! | `add_markup` | `/Square` | ✅ | ✅ |
//! | `add_text_annotation` | `/FreeText` | ❌ refused | ✅ |
//! | `add_text_annotation` | `/Text` | ❌ refused | ❌ **still refused** |
//!
//! > *"We are not asking for `/Text` to become resizable. We are asking for it
//! > to decline as **what it is**."*
//!
//! ## What was wrong, precisely
//!
//! The refusal said *"pdfcer did not draw it, so pdfcer will not redraw it"*
//! about a marker pdfcer had drawn **seconds earlier, in the same session**.
//! The policy was right; the *sentence* named provenance, and the true fact is
//! about the **kind**.
//!
//! §12.5.6.4: a `/Text` annotation "shall behave as if the `NoZoom` and
//! `NoRotate` flags were set". §12.5.3: `NoZoom` means "do not scale the
//! annotation's appearance to match the magnification of the page", with the
//! position taken from the **upper-left corner** of `/Rect`. A conforming
//! reader therefore reads `/Rect` as an anchor, not as a size — so there is
//! nothing for a scale factor to act on.
//!
//! ## ★ Why the wrong sentence cost more than a wrong sentence
//!
//! The reporting shell had offered **eight resize grips** on a sticky for the
//! life of the feature. Nobody questioned them, because the refusal read as a
//! fact about the *file* ("some other producer drew this") rather than about
//! the *kind* ("this thing does not have a size"). Their words: *"a refusal
//! that had said 'a sticky's marker is a fixed size' would have been read as a
//! design fact and fixed on our side months ago."*
//!
//! ## The class, not the instance
//!
//! The guard tests the subtype rule **OR** the `NoZoom` flag, because they are
//! the same statement about the same rectangle. A guard that knew only
//! `/Text` would refuse a `NoZoom` sticky while happily resizing a `NoZoom`
//! stamp into a rectangle no conforming reader honours — `R245`'s shape, a
//! guard present on one route and absent on its twin.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::AnnotFlags;
use pdfcer_core::annot_author::{Color, MarkupSpec, StickyIcon, TextAnnotSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, ResizeOptions};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::ObjId;
use pdfcer_core::page_tree::Rect;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/annot/demo-annotated.pdf")
}

fn session() -> EditSession {
    EditSession::new(Document::load(&fixture()).expect("fixture parses"))
}

fn rect() -> Rect {
    Rect {
        llx: 40.0,
        lly: 40.0,
        urx: 240.0,
        ury: 160.0,
    }
}

/// A `/Text` sticky note authored by pdfcer, in this session, just now.
fn author_sticky(s: &mut EditSession) -> ObjId {
    s.add_text_annotation(
        0,
        &TextAnnotSpec::Sticky {
            rect: rect(),
            icon: StickyIcon::Note,
            contents: "A NOTE".to_owned(),
            color: Color::Rgb(1.0, 0.9, 0.2),
            open: false,
        },
    )
    .expect("author a sticky note")
}

/// A `/Square` authored by the markup verb, same session.
fn author_square(s: &mut EditSession) -> ObjId {
    s.add_markup(
        0,
        &MarkupSpec::Square {
            rect: rect(),
            border: Some(Color::Gray(0.0)),
            interior: None,
            border_width: 1.0,
            border_effect: None,
        },
    )
    .expect("author a square")
}

/// ★ THE CONTROL. A blanket resize breakage would make every refusal
/// assertion in this file pass for the wrong reason.
#[test]
fn a_square_without_nozoom_still_resizes() {
    let mut s = session();
    let id = author_square(&mut s);
    s.resize_annotation(id, (40.0, 40.0), 1.5, 1.5, &ResizeOptions::new())
        .expect("an ordinary markup still resizes");
}

/// ★ THE SECOND CONTROL: `00ddbb1`'s fix must survive this one.
///
/// `/FreeText` is the sibling subtype from the same authoring verb, and it is
/// **not** fixed-size — it has no `NoZoom` rule and its box genuinely is its
/// size. A guard that refused the whole `add_text_annotation` family would
/// look correct against every other test here.
#[test]
fn a_free_text_still_resizes() {
    let mut s = session();
    let id = s
        .add_text_annotation(
            0,
            &TextAnnotSpec::FreeText {
                rect: rect(),
                text: "RESIZE ME".to_owned(),
                font: pdfcer_core::fontdata::Std14::Helvetica,
                font_size: 12.0,
                color: pdfcer_core::vartext::TextColor::Gray(0.0),
                quadding: pdfcer_core::vartext::Quadding::Left,
                multiline: false,
                border: None,
                border_width: 0.0,
            },
        )
        .expect("author a text box");
    s.resize_annotation(id, (40.0, 40.0), 1.5, 1.5, &ResizeOptions::new())
        .expect("a /FreeText is not a fixed-size marker");
}

/// ★★★ THE DEFECT: the refusal is right, and it must say why.
#[test]
fn a_sticky_refuses_as_a_fixed_size_marker() {
    let mut s = session();
    let id = author_sticky(&mut s);
    let err = s
        .resize_annotation(id, (40.0, 40.0), 1.5, 1.5, &ResizeOptions::new())
        .expect_err("a sticky has no size to scale");
    assert!(
        matches!(err, EditError::ResizeFixedSizeMarker { .. }),
        "expected the fixed-size-marker refusal, got {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("fixed-size marker") && msg.contains("12.5.6.4"),
        "the refusal must state the KIND rule and cite it: {msg}"
    );
    assert!(
        msg.contains("move_annotation"),
        "a refusal that names no remedy is where this project's discoverability \
         defects come from: {msg}"
    );
}

/// ★★ THE FALSE CLAIM, pinned by its own words.
///
/// This is the assertion the report was actually about. The refusal must no
/// longer say pdfcer did not draw an appearance pdfcer drew seconds ago — and
/// asserting on the *absence* of that phrase is what stops a future
/// "simplification" from routing `/Text` back through the provenance test.
#[test]
fn the_refusal_no_longer_claims_pdfcer_did_not_draw_it() {
    let mut s = session();
    let id = author_sticky(&mut s);
    let msg = s
        .resize_annotation(id, (40.0, 40.0), 1.5, 1.5, &ResizeOptions::new())
        .expect_err("refused")
        .to_string();
    assert!(
        !msg.contains("pdfcer did not draw it"),
        "pdfcer drew this marker in this session: {msg}"
    );
}

/// There is no override, and that is deliberate.
///
/// `allow_appearance_distortion` means *"I accept a distorted appearance"*,
/// and a conforming reader does not distort this one — it **ignores** the new
/// size. Taking that consent would be taking consent for something that will
/// not happen.
#[test]
fn allow_appearance_distortion_does_not_unlock_a_sticky() {
    let mut s = session();
    let id = author_sticky(&mut s);
    let opts = ResizeOptions::new().with_allow_appearance_distortion(true);
    let err = s
        .resize_annotation(id, (40.0, 40.0), 1.5, 0.8, &opts)
        .expect_err("still refused with the distortion flag set");
    assert!(
        matches!(err, EditError::ResizeFixedSizeMarker { .. }),
        "got {err:?}"
    );
}

/// The CLASS half: the `NoZoom` flag alone is enough, on any subtype.
///
/// Without this, the guard would be a `/Text` special case, and a `NoZoom`
/// `/Square` — which a conforming reader draws unscaled for exactly the same
/// reason — would be resized into a rectangle nothing honours.
#[test]
fn a_nozoom_square_refuses_for_the_flag_reason() {
    let mut s = session();
    let id = author_square(&mut s);
    s.set_annotation_flags(id, AnnotFlags(AnnotFlags::PRINT | AnnotFlags::NO_ZOOM))
        .expect("set NoZoom");
    let err = s
        .resize_annotation(id, (40.0, 40.0), 1.5, 1.5, &ResizeOptions::new())
        .expect_err("NoZoom means the appearance is not scaled");
    let EditError::ResizeFixedSizeMarker { subtype, why } = &err else {
        panic!("expected the fixed-size-marker refusal, got {err:?}");
    };
    assert_eq!(subtype, "Square", "the message must name what was selected");
    assert!(
        why.contains("/F sets NoZoom"),
        "a /Square reaches this by the FLAG, and the two reasons are not \
         interchangeable — a caller can clear a flag and cannot change a \
         subtype's rule: {why}"
    );
}

/// The same annotation resizes once the flag is cleared.
///
/// ★ This is what makes the previous test a measurement of the flag rather
/// than of the fixture. Without it, a guard that refused every `/Square`
/// would pass both.
#[test]
fn clearing_nozoom_makes_the_same_square_resizable_again() {
    let mut s = session();
    let id = author_square(&mut s);
    s.set_annotation_flags(id, AnnotFlags(AnnotFlags::PRINT | AnnotFlags::NO_ZOOM))
        .expect("set NoZoom");
    assert!(
        s.resize_annotation(id, (40.0, 40.0), 1.5, 1.5, &ResizeOptions::new())
            .is_err(),
        "the flag must be what refuses"
    );
    s.set_annotation_flags(id, AnnotFlags(AnnotFlags::PRINT))
        .expect("clear NoZoom");
    s.resize_annotation(id, (40.0, 40.0), 1.5, 1.5, &ResizeOptions::new())
        .expect("with the flag cleared there is a size to scale again");
}

/// A refusal writes nothing — measured on the case where that is not free.
///
/// ★★ THE FIRST VERSION OF THIS TEST WAS VACUOUS, AND THE SABOTAGE IS WHAT
/// SAID SO. It used the **sticky**, and stayed green with the guard moved to
/// after the write: a sticky never reaches the write anyway, because the
/// provenance refusal it used to get also returns early. The test was
/// measuring *"some refusal returns before the write"* while its name claimed
/// *"this guard returns before the write"* — the fourth cause of a surviving
/// sabotage this project has recorded (an alternate, also-correct path
/// supplying the same answer).
///
/// The `NoZoom` `/Square` is the case with something to lose: pdfcer drew it,
/// so without the guard it resizes and stages a scaled `/Rect`. Moving the
/// guard after the write leaves that rectangle behind, and only this fixture
/// can see it.
#[test]
fn a_refused_resize_leaves_the_rect_untouched() {
    let mut s = session();
    let id = author_square(&mut s);
    s.set_annotation_flags(id, AnnotFlags(AnnotFlags::PRINT | AnnotFlags::NO_ZOOM))
        .expect("set NoZoom");
    let before = annot_rect(&s, id);
    let _ = s.resize_annotation(id, (40.0, 40.0), 2.0, 2.0, &ResizeOptions::new());
    assert_eq!(
        before,
        annot_rect(&s, id),
        "the guard must return BEFORE the /Rect is staged — this annotation would otherwise have been resized"
    );
}

/// The `/Rect` of an annotation as the session currently holds it.
fn annot_rect(s: &EditSession, id: ObjId) -> Vec<f64> {
    use pdfcer_core::object::Object;
    let g = s.graph();
    let Some(Object::Dict(d)) = s.value(id) else {
        panic!("not a dict")
    };
    let Object::Array(r) = g.resolve(d.get(b"Rect").expect("/Rect")) else {
        panic!("/Rect is not an array")
    };
    r.iter()
        .map(|o| match g.resolve(o) {
            Object::Real(v) => *v,
            Object::Integer(v) => *v as f64,
            other => panic!("not a number: {other:?}"),
        })
        .collect()
}

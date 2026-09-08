//! # An annotation's blend mode is the file's, not pdfcer's
//!
//! ## The key, and why it looked safe to delete
//!
//! `/BM` on an **annotation dictionary** — as opposed to inside a graphics
//! state — is the blend mode a conformant reader uses when compositing that
//! annotation's appearance onto the page. It changes pixels.
//!
//! ★ **ISO 32000-2 AS PRINTED says the opposite.** Its §12.5.2 paragraph
//! lists `BM` among the keys a reader *"shall ignore … when rendering the
//! appearance dictionary"*, alongside `C`, `IC`, `Border`, `BS`, `CA` and the
//! rest — company that makes it read as pure metadata. **The PDF Association
//! errata (issue #56, closed 2021-07-09, label `ISO approved`) REMOVED `BM`
//! from that list**, while adding `MK` to it. See
//! `iso32000__s__12.5.6.19.md` §4.3 in the spec corpus.
//!
//! So the printed standard and the standard-as-corrected disagree about this
//! exact key, and only one of the two readings is operative. Reading the
//! printed list and concluding "ignored, therefore safe to normalise away" is
//! a defensible mistake that produces a silent rendering change.
//!
//! ## What was actually happening
//!
//! Two regeneration routes had drifted, and **neither looked wrong on its own
//! reading**:
//!
//! | route | what it did to a foreign `/BM` |
//! |---|---|
//! | resize | merged authored keys over the file's dict, never cleared `/BM` — survived **by accident** |
//! | restyle / reshape | listed `/BM` among the keys cleared before merging — **deleted outright** |
//!
//! Deletion was silent: no `DroppedProperty` names `/BM`, so an operator who
//! set Darken in Acrobat and then changed the colour in pdfcer got Normal
//! back and was told nothing.
//!
//! ## The fixture is `/Darken` on purpose
//!
//! pdfcer authors exactly one blend mode: `/Multiply`, and only on a
//! `/Highlight`. A fixture carrying `/Multiply` would stay green under an
//! implementation that "preserved" `/BM` by **re-deriving pdfcer's own
//! default** rather than by keeping the file's value. `/Darken` is a value no
//! pdfcer path can produce, so surviving it can only mean it was carried.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::page_annotations;
use pdfcer_core::annot_author::{Color, MarkupSpec, Quad, TextMarkupKind};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, MarkupStyle, ResizeOptions, StyleEdit, VertexEdit};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::ObjId;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

/// A session over the `/Darken` square, and that square's object id.
fn darken_square() -> (EditSession, ObjId) {
    let s = EditSession::new(
        Document::load(&fixture("annot/blend-mode-square.pdf")).expect("load the fixture"),
    );
    let id = page_annotations(&s.graph(), s.page_slots().expect("slots")[0].id)
        .first()
        .and_then(|a| a.id)
        .expect("the fixture's one annotation");
    (s, id)
}

/// The `/BM` name on an annotation, as a string, or `None` if the key is gone.
fn blend_mode(s: &EditSession, id: ObjId) -> Option<String> {
    let g = s.graph();
    let d = g.resolved(id).as_dict().cloned()?;
    let v = d.get(b"BM").map(|o| g.resolve(o).clone())?;
    Some(String::from_utf8_lossy(v.as_name()?.as_bytes()).into_owned())
}

/// ★ The fixture says what this test thinks it says.
///
/// Cheap, and it is the assertion that stops every other assertion here from
/// becoming vacuous if the generator is ever edited: a fixture that lost its
/// `/BM` would make "the blend mode survived" trivially unfalsifiable, since
/// `None == None` after a route that deletes it.
#[test]
fn the_fixture_carries_a_blend_mode_pdfcer_cannot_author() {
    let (s, id) = darken_square();
    assert_eq!(
        blend_mode(&s, id).as_deref(),
        Some("Darken"),
        "the fixture must arrive carrying /BM /Darken, or every other \
         assertion in this file is testing nothing"
    );
}

/// ★★ Restyling keeps it. This is the route that deleted it.
#[test]
fn a_restyle_keeps_the_files_blend_mode() {
    let (mut s, id) = darken_square();
    let style = MarkupStyle {
        stroke: Some(StyleEdit::Set(Color::Rgb(1.0, 0.0, 0.0))),
        ..Default::default()
    };
    let change = s.set_markup_style(id, &style).expect("restyle");
    assert_eq!(
        blend_mode(&s, id).as_deref(),
        Some("Darken"),
        "a colour change must not retype the blend mode; /BM was on the \
         cleared-keys list and this is the assertion that took it off"
    );
    assert!(
        !format!("{:?}", change.dropped).contains("Blend"),
        "nothing is disclosed as dropped, because nothing was dropped -- \
         rule 4 cuts both ways and a false loss report devalues the real ones"
    );
}

/// ★★ Reshaping keeps it — the same regeneration body, reached by a different
/// verb, which is exactly how a family member gets missed.
///
/// On the `/Polygon` fixture rather than the `/Square` one, because a square
/// **cannot reach this verb at all**: reshape edits a vertex list and a
/// rectangle has none. A test that tried to prove the reshape route on the
/// square would have refused, and a refusal is not a pass.
#[test]
fn a_reshape_keeps_the_files_blend_mode() {
    let mut s = EditSession::new(
        Document::load(&fixture("annot/blend-mode-polygon.pdf")).expect("load the fixture"),
    );
    let id = page_annotations(&s.graph(), s.page_slots().expect("slots")[0].id)
        .first()
        .and_then(|a| a.id)
        .expect("the fixture's one annotation");
    assert_eq!(
        blend_mode(&s, id).as_deref(),
        Some("Darken"),
        "the polygon fixture must carry it too"
    );

    s.reshape_annotation(
        id,
        VertexEdit::Move {
            index: 1,
            dx: 20.0,
            dy: 15.0,
        },
        None,
    )
    .expect("move a vertex");
    assert_eq!(blend_mode(&s, id).as_deref(), Some("Darken"));
}

/// Resizing keeps it. This route was already correct — by accident rather
/// than by decision, which is why it is pinned rather than assumed.
///
/// `scale_stroke_width` is ON here, and that is not incidental: the fixture
/// carries `/BS /W 2`, and with the default (off) this verb **refuses**
/// outright — `ResizeAppearanceNotRebuildable`, because the placement matrix
/// would scale a drawn stroke the operator did not ask to scale. A refusal is
/// not a pass, so a version of this test that shrugged at the error would have
/// proved nothing about `/BM` at all.
#[test]
fn a_resize_keeps_the_files_blend_mode() {
    let (mut s, id) = darken_square();
    let opts = ResizeOptions::new().with_scale_stroke_width(true);
    s.resize_annotation(id, (100.0, 100.0), 1.5, 1.5, &opts)
        .expect("resize");
    assert_eq!(blend_mode(&s, id).as_deref(), Some("Darken"));
}

/// ★★★ A `/Highlight` that already had `/Darken` is **not retyped** to
/// pdfcer's `/Multiply`.
///
/// This is the only test in this file that proves the *preservation rule* as
/// opposed to the *deletion bug*, and it exists because a sabotage run said so.
///
/// `/Highlight` is the one subtype pdfcer authors `/BM` on. Everywhere else the
/// authored dictionary simply has no `/BM`, so merely not deleting the key is
/// the entire fix and the merge order never matters. Here the two collide: the
/// file says `Darken`, pdfcer's authoring default says `Multiply`, and one of
/// them has to win.
///
/// **The file wins.** An operator who set Darken in Acrobat and then changed
/// the colour in pdfcer did not ask for Multiply, and a restyle is not an
/// invitation to reassert defaults over decisions the file already records.
///
/// Without this test, removing the preservation call left all four other tests
/// green — the guard was a null mutation against every fixture that existed,
/// which is one of the three reasons a sabotage survives and the only one that
/// means the guard was never doing anything.
#[test]
fn a_restyle_does_not_retype_a_highlights_own_blend_mode() {
    let mut s = EditSession::new(
        Document::load(&fixture("annot/blend-mode-highlight.pdf")).expect("load the fixture"),
    );
    let id = page_annotations(&s.graph(), s.page_slots().expect("slots")[0].id)
        .first()
        .and_then(|a| a.id)
        .expect("the fixture's one annotation");
    assert_eq!(
        blend_mode(&s, id).as_deref(),
        Some("Darken"),
        "a highlight carrying a blend mode pdfcer would never author is the \
         whole point of this fixture"
    );

    let style = MarkupStyle {
        stroke: Some(StyleEdit::Set(Color::Rgb(0.0, 1.0, 0.0))),
        ..Default::default()
    };
    s.set_markup_style(id, &style).expect("restyle");
    assert_eq!(
        blend_mode(&s, id).as_deref(),
        Some("Darken"),
        "the file's blend mode must beat pdfcer's authoring default; getting \
         Multiply here means the authored dict overwrote a decision the \
         document already recorded"
    );
}

/// ★★★ And a **resize** does not retype it either.
///
/// The twin of the test above, through the other regeneration route, and it
/// exists for exactly the same reason: sabotaging the resize route's
/// preservation call left every other test green, because the only test that
/// went through resize used a `/Square` — a subtype whose authored dictionary
/// has no `/BM` to collide with.
///
/// Two routes, two collisions, two assertions. Verifying the class once and
/// assuming the other member behaves is how the family got uneven in the
/// first place.
#[test]
fn a_resize_does_not_retype_a_highlights_own_blend_mode() {
    let mut s = EditSession::new(
        Document::load(&fixture("annot/blend-mode-highlight.pdf")).expect("load the fixture"),
    );
    let id = page_annotations(&s.graph(), s.page_slots().expect("slots")[0].id)
        .first()
        .and_then(|a| a.id)
        .expect("the fixture's one annotation");

    s.resize_annotation(
        id,
        (100.0, 100.0),
        1.25,
        1.25,
        &ResizeOptions::new().with_scale_stroke_width(true),
    )
    .expect("resize the highlight");
    assert_eq!(
        blend_mode(&s, id).as_deref(),
        Some("Darken"),
        "the resize route merges the authored dictionary too, so it can \
         retype /BM in exactly the way the restyle route could"
    );
}

/// A freshly-authored `/Highlight` still gets pdfcer's own `/Multiply`.
///
/// The preservation rule is *the file's value wins*, not *never write one*.
/// Without this, a fix that simply stopped authoring `/BM` would satisfy
/// every test above while making overlapping highlights darken each other.
#[test]
fn a_new_highlight_still_gets_multiply() {
    let mut s = EditSession::new(
        Document::load(&fixture("annot/demo-annotated.pdf")).expect("load the fixture"),
    );
    let spec = MarkupSpec::TextMarkup {
        kind: TextMarkupKind::Highlight,
        quads: vec![Quad {
            ul: (40.0, 100.0),
            ur: (140.0, 100.0),
            ll: (40.0, 80.0),
            lr: (140.0, 80.0),
        }],
        color: Color::Rgb(1.0, 1.0, 0.0),
    };
    let id = s.add_markup(0, &spec).expect("author a highlight");
    assert_eq!(
        blend_mode(&s, id).as_deref(),
        Some("Multiply"),
        "pdfcer's own default for a highlight it is creating from nothing"
    );

    // And a restyle of that highlight keeps it -- here the file's value and
    // pdfcer's default happen to agree, which is why this cannot stand in
    // for the /Darken cases above.
    let style = MarkupStyle {
        stroke: Some(StyleEdit::Set(Color::Rgb(0.0, 1.0, 1.0))),
        ..Default::default()
    };
    s.set_markup_style(id, &style).expect("restyle");
    assert_eq!(blend_mode(&s, id).as_deref(), Some("Multiply"));
}

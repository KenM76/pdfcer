//! # `EditSession::resize_annotation` — the half of a transform that a
//! translation does not prepare you for (`Pass 151.0`)
//!
//! ## Why this file exists separately from `annot_move.rs`
//!
//! `move_annotation` and `resize_annotation` look like siblings and are not.
//! §12.5.5 computes the placement matrix **A** by mapping the appearance's
//! `BBox`×`Matrix` bounds onto `/Rect`, which means:
//!
//! * under a **translation**, **A** is a pure translation and the `/AP` is
//!   carried untouched — correct, free, and the reason `annot_move.rs` asserts
//!   the appearance stream is byte-identical afterwards;
//! * under a **scale**, **A** becomes a scale applied *after* stroking, so the
//!   drawn stroke scales **whatever `/BS` `/W` says**, and under a non-uniform
//!   scale it is anisotropic — which no scalar stroke width can express,
//!   because PDF has one `w` operand and not one per axis.
//!
//! Inkscape hit the identical thing in SVG (Launchpad #1335376, closed
//! **Invalid** — declared correct spec behaviour rather than a defect) and its
//! shipped response is to silently produce the distorted stroke (ux#339).
//! pdfcer's Inkscape-parity RAG records that pdfcer should be *better than* the
//! parity reference here. These tests are what "better" is measured against.
//!
//! ## The four claims under test
//!
//! 1. **Geometry and `/Rect` scale together**, about the caller's anchor, so
//!    an annotation cannot render in one place and be reconstructed in
//!    another. Same claim `annot_move.rs` makes about a translation, and it
//!    fails the same way if only half is written.
//! 2. **The options are options.** `ResizeOptions::default()` is exactly the
//!    behaviour `pdfcer-gui` specified — stroke width does not travel — and each
//!    flag is independently observable in the saved bytes.
//! 3. **`/RD` scales and `/BS` `/W` does not, by default**, which looks
//!    inconsistent and is not: the discriminator is *is this a length in the
//!    space being transformed?* An inset is; a line weight is a drafting
//!    convention.
//! 4. **A foreign appearance is not redrawn**, and the refusal distinguishes
//!    the case that is genuinely unsatisfiable from the case that is exactly
//!    satisfiable by carrying it.
//!
//! Fixture provenance: `fixtures/synthetic/annot/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::page_annotations;
use pdfcer_core::annot_author::{Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, ResizeOptions, ResizedAppearance};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

/// Save incrementally and re-parse — every assertion is about the bytes, not
/// about the in-memory session that produced them.
fn reload(s: &EditSession) -> Document {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    Document::from_bytes(bytes).expect("re-parse")
}

fn dict_of(doc: &Document, id: ObjId) -> Dict {
    match &doc.get(id).expect("object present").value {
        Object::Dict(d) => d.clone(),
        other => panic!("object {id} is not a dictionary: {other:?}"),
    }
}

fn nums(doc: &Document, d: &Dict, key: &[u8]) -> Vec<f64> {
    match d.get(key).map(|o| doc.view().resolve(o).clone()) {
        Some(Object::Array(items)) => items
            .iter()
            .map(|o| doc.view().resolve(o).as_number().expect("numeric"))
            .collect(),
        other => panic!(
            "key {} not a numeric array: {other:?}",
            String::from_utf8_lossy(key)
        ),
    }
}

/// `/BS` `/W` as saved, or `None` when the annotation has no `/BS` dictionary.
fn bs_width(doc: &Document, d: &Dict) -> Option<f64> {
    match d.get(b"BS").map(|o| doc.view().resolve(o).clone()) {
        Some(Object::Dict(bs)) => bs
            .get(b"W")
            .map(|o| doc.view().resolve(o).clone())
            .and_then(|o| o.as_number()),
        _ => None,
    }
}

fn with_markup(spec: &MarkupSpec) -> (EditSession, ObjId) {
    let mut s = session("annot/demo-annotated.pdf");
    let id = s.add_markup(0, spec).expect("author the markup");
    (s, id)
}

/// A square whose `/BS` `/W` is a value no default could produce by accident,
/// so a test that sees 3.0 knows it saw *this* number and not a fallback.
fn square() -> MarkupSpec {
    MarkupSpec::Square {
        rect: Rect::from_corners(100.0, 100.0, 200.0, 160.0),
        border: Some(Color::Gray(0.0)),
        interior: None,
        border_width: 3.0,
        border_effect: None,
    }
}

/// The `/RD`-carrying fixture, whose `/AP` was written by the generator and is
/// therefore **genuinely foreign** to pdfcer — the property three tests below
/// depend on. `/BS` `/W` is 3.0 and `/RD` is `[2 4 2 4]`.
fn rd_fixture() -> (EditSession, ObjId) {
    let s = session("annot/rect-differences-square.pdf");
    let slots = s.page_slots().expect("page slots");
    let id = page_annotations(&s.graph(), slots[0].id)
        .first()
        .and_then(|a| a.id)
        .expect("the square has an object identity");
    (s, id)
}

/// The anchor every test scales about: the square's lower-left corner. Chosen
/// because it makes the arithmetic checkable by hand — a point ON the anchor
/// must not move at all, which is a property no scaling bug preserves.
const ANCHOR: (f64, f64) = (100.0, 100.0);

// ---------------------------------------------------------------------------
// 1. THE ONE THAT MATTERS — geometry and /Rect scale about the same anchor
// ---------------------------------------------------------------------------

/// A `/Polygon` carries `/Vertices`, which pdfcer's own rendering never
/// consults once an `/AP` exists. That is exactly what makes it the right
/// subject: if only `/Rect` scaled, this annotation would draw at the new size
/// here and be reconstructed at the old size by any tool that regenerates from
/// `/Vertices`. The failure is invisible in pdfcer and visible in Acrobat.
#[test]
fn geometry_and_rect_scale_about_the_same_anchor() {
    let spec = MarkupSpec::Polygon {
        vertices: vec![(100.0, 100.0), (160.0, 100.0), (130.0, 150.0)],
        border: Some(Color::Gray(0.0)),
        interior: None,
        width: 1.0,
    };
    let (mut s, id) = with_markup(&spec);

    let out = s
        .resize_annotation(id, ANCHOR, 2.0, 2.0, &ResizeOptions::default())
        .expect("polygon resizes");
    assert!(
        out.geometry_keys_scaled.iter().any(|k| k == "Vertices"),
        "the disclosure must name /Vertices, got {:?}",
        out.geometry_keys_scaled
    );

    let after = reload(&s);
    let d = dict_of(&after, id);
    let verts = nums(&after, &d, b"Vertices");
    let rect = nums(&after, &d, b"Rect");

    // The vertex sitting ON the anchor is the sharpest assertion available:
    // it must be unmoved, which is false for a bug that scales about the
    // origin, about the rect centre, or that forgets the anchor entirely.
    assert!(
        (verts[0] - 100.0).abs() < 1e-6 && (verts[1] - 100.0).abs() < 1e-6,
        "the vertex on the anchor moved: {verts:?}"
    );
    assert!(
        (verts[2] - 220.0).abs() < 1e-6,
        "x=160 should map to 100+(160-100)*2 = 220, got {}",
        verts[2]
    );
    assert!(
        (verts[5] - 200.0).abs() < 1e-6,
        "y=150 should map to 100+(150-100)*2 = 200, got {}",
        verts[5]
    );
    // And the box agrees with the geometry it contains.
    assert!(
        rect[0] <= verts[0] + 1e-6 && rect[2] >= verts[2] - 1e-6,
        "/Rect {rect:?} does not contain the scaled vertices {verts:?}"
    );
}

// ---------------------------------------------------------------------------
// 2. THE DEFAULT IS THE CONSUMING PROJECT'S ANSWER, EXACTLY
// ---------------------------------------------------------------------------

/// `pdfcer-gui` specified "stroke width does not scale" and argued it from CAD
/// drafting standards. `ResizeOptions::default()` is that answer byte for
/// byte, and this test is what stops a later Pass from quietly flipping it.
#[test]
fn default_options_leave_the_stroke_width_alone() {
    let (mut s, id) = with_markup(&square());
    let out = s
        .resize_annotation(id, ANCHOR, 3.0, 3.0, &ResizeOptions::default())
        .expect("square resizes");

    assert_eq!(
        out.stroke_width, None,
        "the outcome must report that the width was NOT touched"
    );
    let after = reload(&s);
    assert_eq!(
        bs_width(&after, &dict_of(&after, id)),
        Some(3.0),
        "a 3× resize must leave a 3.0 pt border at 3.0 pt by default"
    );
}

/// The toggle Ken ruled must exist (2026-08-28). Same fixture, same factors,
/// one flag — and the observable difference is in the saved bytes, not merely
/// in the returned struct.
#[test]
fn the_toggle_scales_the_stroke_width() {
    let (mut s, id) = with_markup(&square());
    let opts = ResizeOptions::new().with_scale_stroke_width(true);
    let out = s
        .resize_annotation(id, ANCHOR, 3.0, 3.0, &opts)
        .expect("square resizes");

    assert_eq!(
        out.stroke_width,
        Some((3.0, 9.0)),
        "the outcome must disclose both the before and the after"
    );
    let after = reload(&s);
    assert_eq!(
        bs_width(&after, &dict_of(&after, id)),
        Some(9.0),
        "3.0 pt scaled 3× is 9.0 pt in the saved file"
    );
}

/// A single `/BS` `/W` scalar has no exact answer under a non-uniform scale.
/// pdfcer picks the geometric mean — the area-preserving choice — and the point
/// of this test is that the choice is **disclosed** in the outcome rather than
/// applied silently. √(4×1) = 2, so a 3.0 pt border becomes 6.0 pt.
#[test]
fn non_uniform_stroke_scaling_uses_the_disclosed_geometric_mean() {
    let (mut s, id) = with_markup(&square());
    let opts = ResizeOptions::new().with_scale_stroke_width(true);
    let out = s
        .resize_annotation(id, ANCHOR, 4.0, 1.0, &opts)
        .expect("square resizes non-uniformly");

    let (before, after_w) = out.stroke_width.expect("width was scaled");
    assert!((before - 3.0).abs() < 1e-9);
    assert!(
        (after_w - 6.0).abs() < 1e-9,
        "geometric mean of 4 and 1 is 2, so 3.0 pt should become 6.0 pt, got {after_w}"
    );
}

// ---------------------------------------------------------------------------
// 3. /RD SCALES AND /BS /W DOES NOT — the asymmetry, asserted side by side
// ---------------------------------------------------------------------------

/// Both defaults in one test, deliberately, because separately they read as an
/// inconsistency and together they are the discriminator: **is the property a
/// length in the space being transformed?** `/RD` is an inset — a length — so
/// it scales. A line weight is a drafting convention, so it does not.
#[test]
fn rect_differences_scale_while_the_border_width_does_not() {
    // A FIXTURE rather than a staged edit: `/RD` has no authoring verb, and a
    // test that invents one measures the invention. Table 175 orders it
    // [left, top, right, bottom], so 0 and 2 are horizontal and 1 and 3
    // vertical — an ordering a "scale every slot by sx" bug gets wrong in
    // exactly two of four places, which is why the fixture's values are not
    // all equal.
    let (mut s, id) = rd_fixture();

    let out = s
        .resize_annotation(
            id,
            ANCHOR,
            3.0,
            5.0,
            &ResizeOptions::new().with_allow_appearance_distortion(true),
        )
        .expect("square resizes");
    assert_eq!(
        out.rect_differences_scaled,
        Some(true),
        "/RD was present and must be reported as scaled"
    );

    let after = reload(&s);
    let d = dict_of(&after, id);
    let rd = nums(&after, &d, b"RD");
    assert!(
        (rd[0] - 6.0).abs() < 1e-6 && (rd[2] - 6.0).abs() < 1e-6,
        "left/right insets scale by sx=3: expected 6.0, got {rd:?}"
    );
    assert!(
        (rd[1] - 20.0).abs() < 1e-6 && (rd[3] - 20.0).abs() < 1e-6,
        "top/bottom insets scale by sy=5: expected 20.0, got {rd:?}"
    );
    // The other half of the asymmetry, in the same saved bytes.
    assert_eq!(
        bs_width(&after, &d),
        Some(3.0),
        "the border width must be untouched in the very same save"
    );
}

/// The opt-out. Spelled as an opt-OUT so that `default()` is correct for both
/// fields, which is the only reason the two flags read in opposite directions.
#[test]
fn keep_rect_differences_opts_out() {
    let (mut s, id) = rd_fixture();
    let opts = ResizeOptions::new()
        .with_keep_rect_differences(true)
        .with_allow_appearance_distortion(true);
    let out = s
        .resize_annotation(id, ANCHOR, 3.0, 5.0, &opts)
        .expect("square resizes");
    assert_eq!(out.rect_differences_scaled, Some(false));

    let after = reload(&s);
    let rd = nums(&after, &dict_of(&after, id), b"RD");
    assert_eq!(rd, vec![2.0, 4.0, 2.0, 4.0], "/RD must be untouched");
}

// ---------------------------------------------------------------------------
// 4. REFUSALS — by name, before anything is written
// ---------------------------------------------------------------------------

/// Zero collapses the `/Rect` to a degenerate box, which §12.5.5 treats as a
/// negative appearance result and draws as nothing. "My annotation vanished"
/// is a far worse diagnostic than a named refusal.
#[test]
fn a_zero_or_non_finite_factor_is_refused_by_name() {
    for (sx, sy, axis) in [
        (0.0, 2.0, "sx"),
        (2.0, 0.0, "sy"),
        (f64::NAN, 2.0, "sx"),
        (2.0, f64::INFINITY, "sy"),
    ] {
        let (mut s, id) = with_markup(&square());
        match s.resize_annotation(id, ANCHOR, sx, sy, &ResizeOptions::default()) {
            Err(EditError::ResizeFactorInvalid { axis: got, .. }) => assert_eq!(got, axis),
            other => panic!("sx={sx} sy={sy} should be refused, got {other:?}"),
        }
    }
}

/// A negative factor is a **mirror**, not an error — `Rect::from_corners`
/// normalises, which is what keeps §12.5.2's "lower-left then upper-right"
/// ordering true rather than writing an inverted rectangle the spec forbids.
#[test]
fn a_negative_factor_mirrors_and_normalises_the_rect() {
    let (mut s, id) = with_markup(&square());
    s.resize_annotation(id, ANCHOR, -1.0, 1.0, &ResizeOptions::default())
        .expect("a mirror is a legitimate resize");

    let after = reload(&s);
    let rect = nums(&after, &dict_of(&after, id), b"Rect");
    assert!(
        rect[0] < rect[2] && rect[1] < rect[3],
        "/Rect must stay normalised after a mirror: {rect:?}"
    );
    // x=200 mirrored about x=100 is x=0, which becomes the new lower-left.
    assert!(
        (rect[0] - 0.0).abs() < 1e-6,
        "expected the mirrored edge at x=0, got {rect:?}"
    );
}

/// A widget belongs to a field, and `edit_widget` rebuilds its appearance into
/// the new box as part of the same command — so this verb refuses and names
/// the one that does more, rather than doing half the job.
#[test]
fn a_widget_is_refused_and_the_message_names_the_right_verb() {
    let mut s = session("forms/demo-form.pdf");
    let slots = s.page_slots().expect("page slots");
    let id = page_annotations(&s.graph(), slots[0].id)
        .iter()
        .find_map(|a| a.id)
        .expect("a widget with an object identity");

    match s.resize_annotation(id, ANCHOR, 2.0, 2.0, &ResizeOptions::default()) {
        Err(EditError::AnnotationMoveWrongVerb { use_instead, .. }) => {
            assert!(
                use_instead.starts_with("edit_widget"),
                "the message must name the verb that does more, got {use_instead}"
            );
        }
        other => panic!("a widget should be refused, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 5. THE APPEARANCE GATE — the part a translation never has to think about
// ---------------------------------------------------------------------------

/// An appearance pdfcer drew is pdfcer's to redraw, so both stroke-width states
/// are exactly satisfiable and no option has to be waived.
#[test]
fn a_pdfcer_authored_appearance_is_rebuilt() {
    let (mut s, id) = with_markup(&square());
    let out = s
        .resize_annotation(id, ANCHOR, 2.0, 3.0, &ResizeOptions::default())
        .expect("pdfcer redraws its own artwork");
    assert_eq!(out.appearance, ResizedAppearance::Rebuilt);
}

/// ★ The test that stops the verb from quietly replacing another producer's
/// artwork. The gate is a BYTE comparison against a rebuild of the unmodified
/// spec — not `spec_from_dict(..).is_ok()`, which succeeds for an
/// Acrobat-drawn square too and would have made this whole refusal
/// unreachable.
///
/// The sabotage that proves the test is real: change the gate to `is_ok()` and
/// this case starts returning `Rebuilt` instead of refusing.
#[test]
fn a_foreign_appearance_is_not_redrawn_and_the_refusal_says_why() {
    // The fixture's `/AP` is a black fill written by the generator, NOT by
    // `build_appearance`. Its dictionary parses perfectly — `/Rect`, `/C`,
    // `/BS` all read — so a `spec_from_dict(..).is_ok()` gate would call it
    // rebuildable and silently replace it. That is the whole point of this
    // test, and the reason it uses a real foreign appearance rather than one
    // this test manufactured.
    let (mut s, id) = rd_fixture();

    match s.resize_annotation(id, ANCHOR, 2.0, 3.0, &ResizeOptions::default()) {
        Err(EditError::ResizeAppearanceNotRebuildable { uniform, why, .. }) => {
            assert!(!uniform, "4:1 is not uniform");
            assert!(
                why.contains("non-uniform"),
                "the message must name the reason, got {why}"
            );
        }
        other => panic!("a foreign /AP under a non-uniform scale must refuse, got {other:?}"),
    }
}

/// ★★ And the case that must NOT refuse, which is the whole reason the gate is
/// three branches rather than two. A uniform scale with `scale_stroke_width`
/// on is satisfied **exactly** by §12.5.5's matrix — it scales the drawn
/// stroke by precisely the requested factor. Refusing here would refuse a
/// resize that comes out right.
#[test]
fn a_foreign_appearance_carries_exactly_when_uniform_and_stroke_scaling_is_on() {
    let (mut s, id) = rd_fixture();
    let opts = ResizeOptions::new().with_scale_stroke_width(true);
    let out = s
        .resize_annotation(id, ANCHOR, 2.0, 2.0, &opts)
        .expect("no flag should be needed for the exact case");
    assert_eq!(out.appearance, ResizedAppearance::CarriedUniform);
    assert_eq!(
        out.stroke_width,
        Some((3.0, 6.0)),
        "the dictionary's width must agree with what the matrix will draw"
    );
}

/// Taking the distortion knowingly. The outcome names it `CarriedDistorted`
/// rather than a boolean beside `CarriedUniform`, so a shell can say "the
/// border is now oval" without recomputing which case it was in.
#[test]
fn allow_appearance_distortion_proceeds_and_names_the_distortion() {
    let (mut s, id) = rd_fixture();
    let opts = ResizeOptions::new().with_allow_appearance_distortion(true);
    let out = s
        .resize_annotation(id, ANCHOR, 4.0, 1.0, &opts)
        .expect("the distortion was accepted knowingly");
    assert_eq!(out.appearance, ResizedAppearance::CarriedDistorted);
}

// ---------------------------------------------------------------------------
// 6. UNDO — the command log, not a special case
// ---------------------------------------------------------------------------

/// A resize is one command, so one undo restores every key it touched — the
/// `/Rect`, the geometry, the `/RD`, the `/BS` `/W` and the appearance stream.
/// Asserted on the SAVED bytes, because "the session forgot" and "the writer
/// emitted it anyway" are indistinguishable in memory.
#[test]
fn one_undo_restores_every_key_the_resize_touched() {
    let (mut s, id) = with_markup(&square());
    let before = reload(&s);
    let before_d = dict_of(&before, id);
    let before_rect = nums(&before, &before_d, b"Rect");

    let opts = ResizeOptions::new().with_scale_stroke_width(true);
    s.resize_annotation(id, ANCHOR, 2.5, 2.5, &opts)
        .expect("resize");
    assert!(
        s.undo().is_some(),
        "the resize must be one undoable command"
    );

    let after = reload(&s);
    let after_d = dict_of(&after, id);
    assert_eq!(nums(&after, &after_d, b"Rect"), before_rect);
    assert_eq!(bs_width(&after, &after_d), Some(3.0));
}

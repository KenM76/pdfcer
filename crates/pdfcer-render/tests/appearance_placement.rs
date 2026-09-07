//! # `appearance_placement` — where an annotation's artwork actually lands
//! (`Pass 155.2`)
//!
//! ## Why a renderer exports this at all
//!
//! It looks like a paint internal and is not. It answers *"where is this
//! object?"*, and three things a **shell** draws depend on the answer: a
//! selection outline, a rotate grip, and a hit test. All three are otherwise
//! computed from `/Rect`, which ISO 32000-1 §12.5.2 requires to be
//! **upright** — so on a turned annotation every one of them is wrong. The
//! operator hit exactly that on 2026-09-07: *"the box outlined when an object
//! is selected should be in the same angled orientation as the object."*
//!
//! `pdfcer-gui` reported (decision 058) that reaching the answer without this
//! function meant re-implementing §12.5.5 inside the shell — including this
//! crate's `MIN_BOX_EXTENT` degeneracy floor copied by value and its `/AS`
//! single-entry rule copied out of a doc comment. Four copies of a normative
//! algorithm, in a crate that does not model appearance-state
//! subdictionaries. Exporting the one implementation deletes all four.
//!
//! ## What is pinned here
//!
//! 1. **An unrotated appearance places on `/Rect` exactly** — the corners
//!    ARE the rectangle, which is what makes the fixture's own placement
//!    assertable without a render.
//! 2. **A `/Matrix` rotation produces a genuinely turned quadrilateral**:
//!    its edge angle equals the rotation, its side lengths are preserved,
//!    and its upright bound is `/Rect` (§12.5.5 step (c) fits exactly).
//! 3. **The degenerate `/BBox` is refused, not fabricated** — the case where
//!    step (b) would divide by zero and the standard specifies no handling.
//! 4. **An annotation with no appearance is refused**, so a caller cannot
//!    mistake "no artwork" for "artwork at the origin".
//!
//! Fixture provenance: `fixtures/synthetic/annot/PROVENANCE.md`. Every one of
//! these files was authored byte by byte for `Pass 6.0`'s placement claims,
//! which is exactly the contract this function exposes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::page_annotations;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::annot::appearance_placement;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/annot")
        .join(rel)
}

/// The first annotation of a single-page fixture, with its placement.
fn placed(rel: &str) -> (pdfcer_core::annot::Annotation, Option<[(f64, f64); 4]>) {
    let doc = Document::load(&fixture(rel)).expect("load fixture");
    let page = page_tree::pages(&doc).expect("pages").remove(0);
    let view = doc.view();
    let annot = page_annotations(&view, page.id)
        .into_iter()
        .next()
        .expect("an annotation");
    let quad = appearance_placement(&view, &annot);
    (annot, quad)
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// `placement-identity.pdf`: `/BBox [0 0 20 20]`, identity `/Matrix`,
/// `/Rect [40 30 60 50]`. One-to-one into a non-origin rectangle, so the
/// placed corners are the rectangle's own corners — the sharpest available
/// assertion, with no tolerance to argue about.
#[test]
fn an_unrotated_appearance_places_exactly_on_its_rect() {
    let (annot, quad) = placed("placement-identity.pdf");
    let quad = quad.expect("a placement");
    let r = annot.rect.expect("/Rect");

    for (got, want) in quad.iter().zip([
        (r.llx, r.lly),
        (r.urx, r.lly),
        (r.urx, r.ury),
        (r.llx, r.ury),
    ]) {
        assert!(
            dist(*got, want) < 1e-4,
            "expected the /BBox corners to land on /Rect's own corners; got \
             {quad:?} against rect {r:?}"
        );
    }
}

/// `placement-bbox-larger.pdf`: `/BBox [0 0 80 80]` into `/Rect [10 10 30
/// 30]`. §12.5.5 step (b) scales the appearance DOWN to fit, and the placed
/// corners must follow it down — a function that returned raw `/BBox`
/// corners, or raw `/Matrix`-mapped ones, would pass the identity test above
/// and fail here.
#[test]
fn an_oversized_bbox_is_scaled_down_onto_the_rect() {
    let (annot, quad) = placed("placement-bbox-larger.pdf");
    let quad = quad.expect("a placement");
    let r = annot.rect.expect("/Rect");

    let width = dist(quad[0], quad[1]);
    assert!(
        (width - (r.urx - r.llx)).abs() < 1e-4,
        "the 80-unit /BBox edge must have been scaled to the 20-unit /Rect \
         edge; got {width:.4}"
    );
    for corner in quad {
        assert!(
            corner.0 >= r.llx - 1e-4
                && corner.0 <= r.urx + 1e-4
                && corner.1 >= r.lly - 1e-4
                && corner.1 <= r.ury + 1e-4,
            "every placed corner must lie inside /Rect: {corner:?} against \
             {r:?}"
        );
    }
}

/// ★ The whole point of the function: a rotating `/Matrix` produces a
/// **quadrilateral of arbitrary orientation**, not a rectangle.
///
/// `placement-matrix-rotate.pdf` carries a rotating `/Matrix`, so:
///
/// * adjacent edges are perpendicular and the opposite pair is equal — it is
///   still a rectangle in shape, just not an upright one;
/// * **the bearing of its first edge equals the angle the annotation itself
///   reports** through `Pass 155.2`'s reader — the cross-check that makes the
///   two halves of that Pass agree, and the assertion a shell's rotate grip
///   actually depends on;
/// * its upright bound is `/Rect` exactly, because step (c) fits it there.
///
/// ★ An earlier draft asserted instead that *"at least one edge is genuinely
/// off-axis"*, and it failed — **because the expectation was wrong, not the
/// code.** This fixture's `/Matrix` is a **quarter** turn, and a quarter turn
/// is axis-aligned by definition. The bearing check below is what that
/// assertion was reaching for and states it correctly at any angle.
#[test]
fn a_rotating_matrix_produces_a_turned_quadrilateral_bounded_by_rect() {
    let (annot, quad) = placed("placement-matrix-rotate.pdf");
    let quad = quad.expect("a placement");
    let r = annot.rect.expect("/Rect");

    let e0 = (quad[1].0 - quad[0].0, quad[1].1 - quad[0].1);
    let e1 = (quad[2].0 - quad[1].0, quad[2].1 - quad[1].1);
    let dot = e0.0 * e1.0 + e0.1 * e1.1;
    let len0 = e0.0.hypot(e0.1);
    let len1 = e1.0.hypot(e1.1);
    assert!(
        dot.abs() < 1e-3 * len0 * len1,
        "adjacent edges must stay perpendicular under a rotation; dot {dot}"
    );
    assert!(
        (dist(quad[0], quad[1]) - dist(quad[3], quad[2])).abs() < 1e-4,
        "opposite edges must stay equal"
    );
    let bearing = e0.1.atan2(e0.0).to_degrees();
    let reported = annot
        .appearance_rotation_degrees()
        .expect("Pass 155.2's reader must decompose this /Matrix");
    assert!(
        (bearing - reported).abs() < 1e-3,
        "the placed edge runs at {bearing:.4} degrees while the annotation \
         reports {reported:.4}. These two must agree exactly: a shell reads \
         the angle from one and draws the outline from the other, and a \
         divergence would put the grip somewhere the artwork is not."
    );
    assert!(
        reported.abs() > 1.0,
        "this fixture is supposed to CARRY a rotation -- if it reports ~0 the \
         /Matrix was ignored and the checks above are vacuous"
    );

    let (mut lo_x, mut lo_y) = (f64::INFINITY, f64::INFINITY);
    let (mut hi_x, mut hi_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (x, y) in quad {
        lo_x = lo_x.min(x);
        lo_y = lo_y.min(y);
        hi_x = hi_x.max(x);
        hi_y = hi_y.max(y);
    }
    for (got, want) in [(lo_x, r.llx), (lo_y, r.lly), (hi_x, r.urx), (hi_y, r.ury)] {
        assert!(
            (got - want).abs() < 1e-3,
            "the upright bound of the placed quad must be /Rect exactly \
             (§12.5.5 step c fits it there); got {got:.4} want {want:.4}"
        );
    }
}

/// A collapsed `/BBox` makes step (b)'s fit matrix singular, and §12.5.5
/// specifies no handling. The paint path refuses to place and counts it; this
/// refuses too, rather than returning corners it would have had to invent.
#[test]
fn a_degenerate_bbox_is_refused_rather_than_fabricated() {
    let (_, quad) = placed("placement-degenerate-bbox.pdf");
    assert!(quad.is_none(), "expected a refusal, got {quad:?}");
}

/// No appearance stream, no placement. A caller must not be able to mistake
/// *"this annotation paints nothing"* for *"its artwork is at the origin"* —
/// which is what a `[(0,0); 4]` default would have meant.
#[test]
fn an_annotation_with_no_appearance_has_no_placement() {
    let (annot, quad) = placed("no-ap-circle.pdf");
    assert!(quad.is_none(), "expected a refusal, got {quad:?}");
    assert!(
        annot.rect.is_some(),
        "the refusal must be about the APPEARANCE, not about a missing \
         /Rect -- this fixture has one, so a passing test that read the rect \
         as absent would be measuring the wrong thing"
    );
}

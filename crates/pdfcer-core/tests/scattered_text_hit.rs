//! Regression: a text object's gaps are not part of it.
//!
//! # The defect this pins
//!
//! `TextObject::page_bbox` is one rectangle enclosing an entire `BT`…`ET`.
//! Nothing obliges a producer to keep the show operators inside one close
//! together — and a CAD exporter typically does the opposite, emitting every
//! label on a drawing from a single text object. The enclosing rectangle then
//! spans the whole sheet while the ink covers almost none of it.
//!
//! Hit-testing that rectangle made such an object the front-most hit for
//! **every** click on the page. Measured on a real SolidWorks export before
//! the fix: one text object holding every dimension label, `page_bbox` =
//! 23,14 → 1564,1216 (the entire drawing), 237 runs, painted at index 5871 of
//! 5903. At a point over a real line it beat **57** genuine objects beneath
//! it; at a point over empty space it was the *only* hit.
//!
//! The operator experienced both halves: clicking a visible line selected
//! nothing useful, and clicking empty space drew "a box that doesn't seem to
//! correspond to anything" — which was that page-spanning rectangle being
//! outlined.
//!
//! # Why the fixture is synthetic
//!
//! The export that revealed this is proprietary work product and is not in
//! this repository (`docs/LEGAL.md` §5). Only the *structure* it exposed is
//! reproduced, from first principles: one `BT`…`ET`, two short runs at
//! opposite corners, plus a mid-page rule. Two `Td`s are enough — the defect
//! never needed 5,903 objects, it needed a gap.

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::vector::Point;
use pdfcer_core::vector::{Matrix, decompose_page, hit_test_point_all};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text/scattered-text-one-object.pdf")
}

fn objects() -> pdfcer_core::vector::PageObjects {
    let bytes = std::fs::read(fixture())
        .unwrap_or_else(|e| panic!("missing fixture {}: {e}", fixture().display()));
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let pages = page_tree::pages(&doc).expect("pages");
    let page = pages.first().expect("one page");
    decompose_page(&doc.view(), page, Matrix::IDENTITY).expect("page decomposes")
}

/// The shape of the bug: the enclosing rectangle really does span the page.
///
/// Asserted so the fixture cannot silently stop reproducing the condition.
/// If a future change made these two runs land close together, every
/// assertion below would pass while testing nothing.
#[test]
fn the_fixture_still_has_a_page_spanning_text_object() {
    let m = objects();
    let text = m
        .objects
        .iter()
        .find_map(|o| match o {
            pdfcer_core::vector::VectorObject::Text(t) => Some(t),
            _ => None,
        })
        .expect("the fixture has one text object");

    let w = text.page_bbox.max.x - text.page_bbox.min.x;
    let h = text.page_bbox.max.y - text.page_bbox.min.y;
    assert!(
        w > 400.0 && h > 600.0,
        "the enclosing box must still span most of the page, or this fixture \
         no longer reproduces the defect: {w} x {h}"
    );
    assert!(
        text.runs.len() >= 2,
        "the two runs must be retained separately — with one run (or none) \
         there is no gap to test"
    );
}

/// **The defect.** A point in the empty middle must hit nothing.
///
/// Before the fix this returned the text object, because the point is inside
/// the enclosing rectangle even though it is nowhere near any glyph.
#[test]
fn empty_space_between_runs_hits_nothing() {
    let m = objects();
    let hits = hit_test_point_all(&m, Point::new(306.0, 300.0), 3.0);
    assert!(
        hits.is_empty(),
        "a click in empty space must not hit the text object whose enclosing \
         box merely surrounds it; got {hits:?}"
    );
}

/// **The true positive that must survive.** Real ink in the gap wins.
///
/// Before the fix the text object was ordinal 0 here and the line was
/// ordinal 1 — so clicking the visible line selected the invisible text.
/// This is the assertion that makes the fix a fix rather than a mute.
#[test]
fn a_line_in_the_gap_is_the_front_most_hit() {
    let m = objects();
    let hits = hit_test_point_all(&m, Point::new(306.0, 396.0), 3.0);
    let first = hits.first().copied().expect("the rule must be hit");
    assert!(
        matches!(
            m.objects.get(first),
            Some(pdfcer_core::vector::VectorObject::Path(_))
        ),
        "the visible line must be the front-most hit, not the text object \
         that happens to enclose it; hits = {hits:?}"
    );
}

/// **The other true positive.** Text is still selectable ON the text.
///
/// A fix that made text unhittable would satisfy both assertions above and
/// be strictly worse than the bug. This is the control that forbids it.
#[test]
fn a_click_on_actual_glyphs_still_hits_the_text() {
    let m = objects();
    let hits = hit_test_point_all(&m, Point::new(80.0, 742.0), 3.0);
    assert!(
        hits.iter().any(|i| matches!(
            m.objects.get(*i),
            Some(pdfcer_core::vector::VectorObject::Text(_))
        )),
        "text must still be hittable where it is actually drawn; got {hits:?}"
    );
}

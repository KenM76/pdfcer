//! A spot colorant painted inside — or beneath — a transparency group on an
//! ink page keeps its plane through the group's merge (`Pass 239.0`).
//!
//! # The defect these pin
//!
//! A transparency group renders into a CHILD colorant buffer and is merged
//! back into its parent. Until `Pass 239.0` that merge copied the spot planes
//! **by index**: the child's plane 0 landed in the parent's plane 0 whatever
//! colorant either held, or — when the parent had allocated fewer planes than
//! the child — was dropped by `set_pixel`'s surplus-dropping zip. Silently.
//! A knockout group's initial backdrop, and a non-isolated group's, carried
//! no spot planes at all. So a spot painted inside any buffered group on an
//! ink page either changed colorant or vanished, and a spot beneath a
//! knockout group was gone before the group's first element.
//!
//! That is an Illustrator file with a Pantone swatch at 50 % opacity.
//!
//! # The oracle
//!
//! The same paint drawn twice on one page — directly, and through the
//! construct under test — must agree. No reference render, no remembered
//! colour. Fixtures: `tools/gen-group-spot-fixtures.py`.

use std::path::PathBuf;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::RenderedPage;

fn render(name: &str) -> RenderedPage {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/transparency")
        .join(name);
    let doc =
        Document::from_bytes(std::fs::read(&path).expect("fixture file")).expect("fixture parses");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page(&doc, &pages[0], 2.0).expect("renders")
}

/// Mean RGB over a box well inside one of the two 80×60 pt rectangles.
fn patch(page: &RenderedPage, x0: f64, x1: f64) -> (f64, f64, f64) {
    let w = f64::from(page.pixmap.width());
    let h = f64::from(page.pixmap.height());
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (ax, bx, ay, by) = (
        (w * x0) as u32,
        (w * x1) as u32,
        (h * 0.35) as u32,
        (h * 0.65) as u32,
    );
    let (mut r, mut g, mut b, mut n) = (0.0, 0.0, 0.0, 0.0);
    for y in ay..by {
        for x in ax..bx {
            let p = page.pixmap.pixel(x, y).expect("in bounds");
            r += f64::from(p.red());
            g += f64::from(p.green());
            b += f64::from(p.blue());
            n += 1.0;
        }
    }
    (r / n, g / n, b / n)
}

fn direct_and_grouped(page: &RenderedPage) -> ((f64, f64, f64), (f64, f64, f64)) {
    (patch(page, 0.15, 0.35), patch(page, 0.65, 0.85))
}

fn mean_abs(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    ((a.0 - b.0).abs() + (a.1 - b.1).abs() + (a.2 - b.2).abs()) / 3.0
}

/// The colour is the spot's, not paper: a merge that dropped the plane
/// leaves the grouped box WHITE, and the agreement test alone would then be
/// comparing green to white — which fails, but this names the failure.
fn is_green(c: (f64, f64, f64)) -> bool {
    c.1 > c.0 + 20.0 && c.1 > c.2 + 20.0
}

/// An isolated group's child buffer starts with an empty roster, so its
/// spot is plane 0 there and — before the name-mapped merge — landed in
/// nothing on a parent that had no plane 0.
#[test]
fn a_spot_fill_inside_an_isolated_group_survives_the_merge() {
    let page = render("spot_in_isolated_group_vs_fill.pdf");
    let (direct, grouped) = direct_and_grouped(&page);
    assert!(
        is_green(direct),
        "the direct fill is the control: {direct:?}"
    );
    assert!(
        is_green(grouped),
        "the spot painted inside the group vanished at the merge: {grouped:?}"
    );
    let d = mean_abs(direct, grouped);
    assert!(
        d <= 1.5,
        "direct {direct:?} vs inside an isolated group {grouped:?}, mean |diff| {d:.2}"
    );
}

/// The non-isolated route composites through `composite_non_isolated`,
/// which built its source with an empty spot array — the spot vanished
/// there whatever the merge did.
#[test]
fn a_spot_fill_inside_a_nonisolated_group_survives_the_merge() {
    let page = render("spot_in_nonisolated_group_vs_fill.pdf");
    let (direct, grouped) = direct_and_grouped(&page);
    let d = mean_abs(direct, grouped);
    assert!(
        d <= 2.0,
        "direct {direct:?} vs inside a non-isolated group at 50% {grouped:?}, mean \
         |diff| {d:.2}: the group's spot did not come back through the backdrop \
         removal"
    );
}

/// A knockout group composites every element against the group's INITIAL
/// backdrop. That backdrop carried the four process planes and no spot, so
/// a spot beneath the group was gone before its first element painted.
#[test]
fn a_spot_beneath_a_knockout_group_is_still_there() {
    let page = render("spot_under_knockout_group_vs_direct.pdf");
    let (direct, grouped) = direct_and_grouped(&page);
    // Half the spot survives (a 50 % opaque process paint knocks half of it
    // out, §11.7.3), measured (132, 166, 153) both ways — a teal, so only the
    // red-to-green gap is a safe witness of "the spot is still there"; a
    // white box would be (255, 255, 255) and a full knockout neutral.
    assert!(
        grouped.1 > grouped.0 + 20.0,
        "the spot beneath the knockout group was knocked out by its initial \
         backdrop, not by any element: {grouped:?}"
    );
    let d = mean_abs(direct, grouped);
    assert!(
        d <= 2.0,
        "a 50% K over the spot directly {direct:?} vs as a knockout group's one \
         element {grouped:?}, mean |diff| {d:.2}"
    );
}

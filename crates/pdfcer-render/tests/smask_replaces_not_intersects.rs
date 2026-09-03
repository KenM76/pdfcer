//! **A new `gs /SMask` REPLACES the mask in force; it does not intersect with
//! it** (`Pass 192.0`).
//!
//! # The clause
//!
//! ISO 32000-1 Table 58, the `/SMask` row, verbatim:
//!
//! > "Although the current soft mask is sometimes referred to as a *soft clip*,
//! > altering it with the `gs` operator **completely replaces** the old value
//! > with the new one, rather than intersecting the two as is done with the
//! > current clipping path parameter."
//!
//! §11.6.4.3 says the same from the other direction: *"at most one mask input
//! shall be provided to any PDF compositing operation"* — there is no
//! arithmetic slot for a second mask.
//!
//! # The defect this pins
//!
//! pdfcer folds a soft mask into the clip by multiplication. That is a sound way
//! to apply ONE mask and a wrong way to apply two: a second `gs /SMask` with no
//! intervening `q`/`Q` used not to lift the first one out, so the clip became
//! `mask₁ × mask₂`.
//!
//! ★ **Why that is expensive rather than merely wrong.** A bevel-and-emboss
//! effect is a highlight and a shadow whose masks are **complementary**
//! gradients. Their product is ≈ 0, so the second layer paints under no
//! coverage at all and vanishes entirely — while the first, painted while only
//! its own mask was in force, renders perfectly. *"The first masked layer works
//! and the second is missing"* is precisely how it was reported.
//!
//! Measured on the real document that surfaced it: the missing layer's paint
//! coverage was **0 pixels**, and became 2,425 with the fix; the affected cell's
//! error against that page's own baked reference fell from **29.70 to 2.95**,
//! with the shadow region moving from luminance 101.3 to 58.5 against a
//! reference of 58.8.
//!
//! # Why this test uses hard halves and not gradients
//!
//! The real case is two gradients, which would make this a tolerance test. Two
//! **disjoint halves** make it a colour test: left must be red, right must be
//! blue, and before the fix the right half is untouched white because
//! `left × right = 0` everywhere. An assertion that cannot be argued with is
//! worth more here than one that resembles the original document.
//!
//! # ★ The control is not optional
//!
//! `smask-single-layer-control.pdf` has ONE masked layer and must render
//! identically. Without it, "the second layer now paints" is equally consistent
//! with a change that broke masking altogether — and the fix touches the code
//! path every single-mask document in the corpus uses.
//!
//! Fixtures from `tools/gen-smask-replace-fixtures.py`; wholly synthetic
//! (`LEGAL.md` §5 category (a)).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/transparency")
        .join(name)
}

/// Render page 1 at 1:1 and return the raw RGBA bytes plus the row width.
///
/// The page is rendered white-backed and fully opaque, so the bytes can be read
/// directly without un-premultiplying -- the same idiom the sibling tests in
/// this directory use.
fn render(name: &str) -> (Vec<u8>, usize) {
    let doc = Document::load(&fixture(name)).expect("fixture must load");
    let pages = pdfcer_core::page_tree::pages(&doc).expect("page tree");
    let out = pdfcer_render::render_page(&doc, &pages[0], 1.0).expect("render must succeed");
    let w = out.pixmap.width() as usize;
    (out.pixmap.data().to_vec(), w)
}

/// `(r, g, b)` at a device pixel.
fn rgb(px: &(Vec<u8>, usize), x: usize, y: usize) -> (u8, u8, u8) {
    let (data, w) = px;
    let i = (y * w + x) * 4;
    (data[i], data[i + 1], data[i + 2])
}

/// ★★★ THE ASSERTION. Two soft-masked fills in one q-level: BOTH must paint,
/// each under its OWN mask.
///
/// The second layer is the one that vanished. Its half is asserted first,
/// because a failure there is the defect and a failure on the first half would
/// mean something else entirely broke.
#[test]
fn a_second_soft_mask_replaces_the_first_rather_than_multiplying_with_it() {
    let p = render("smask-replaces-not-intersects.pdf");

    let right = rgb(&p, 150, 100);
    assert_eq!(
        right,
        (0, 0, 255),
        "the SECOND soft-masked layer did not paint. Its mask covers the right \
         half, but if a new /SMask INTERSECTS the one already in force instead \
         of replacing it (Table 58), the clip becomes left-mask x right-mask = 0 \
         and this half stays white. Got {right:?}"
    );

    let left = rgb(&p, 50, 100);
    assert_eq!(
        left,
        (255, 0, 0),
        "the FIRST soft-masked layer must still paint under its own mask; got {left:?}"
    );
}

/// ★ Each mask must apply to its OWN layer only.
///
/// Separate from the test above because "both halves are painted" is also
/// satisfied by an implementation that dropped masking entirely and painted
/// both full-width rectangles — in which case the LAST one drawn would cover
/// everything. Asserting that the left half is red rather than blue is what
/// rules that out.
#[test]
fn each_layer_is_confined_to_its_own_mask() {
    let p = render("smask-replaces-not-intersects.pdf");
    let left = rgb(&p, 50, 100);
    assert_ne!(
        left,
        (0, 0, 255),
        "the left half is BLUE, so the second fill painted across the whole page: \
         its mask was not applied at all. Both-halves-painted is not enough; each \
         must be confined to its own mask"
    );
}

/// ★ THE CONTROL. One masked layer must be unaffected.
///
/// The fix changes the code path every single-mask document uses, and this is
/// the assertion that says so out loud rather than assuming it.
#[test]
fn a_single_masked_layer_is_unaffected() {
    let p = render("smask-single-layer-control.pdf");
    assert_eq!(
        rgb(&p, 50, 100),
        (255, 0, 0),
        "the single masked layer must paint inside its mask"
    );
    assert_eq!(
        rgb(&p, 150, 100),
        (255, 255, 255),
        "and must NOT paint outside it — a fix that made masks stop applying \
         would pass the two-layer test and fail here"
    );
}

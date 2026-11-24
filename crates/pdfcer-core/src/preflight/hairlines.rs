//! Hairlines: strokes thinner than a minimum once the CTM is applied.
//!
//! The line width is a user-space length (ISO 32000-2 §8.4.3.2), so a stroke
//! is measured through the CTM that paints it. The pen is a circle of the line
//! width in user space; the CTM maps it to an ellipse, and the ellipse's
//! **minor axis** is the width measured here, because that is the thinnest the
//! stroke prints in any direction. Width 0 is "the thinnest line that can be
//! rendered at device resolution" (§8.4.3.2), so it is always a hairline,
//! whatever the minimum.
//!
//! Widths are in points of default user space, the device-independent stand
//! in for device space: the page's `/UserUnit` and the output device's own
//! scale are not applied.
//!
//! Covered: stroked paths on the page and inside form XObjects. Not covered:
//! strokes inside Type 3 glyph procedures, tiling-pattern cells and annotation
//! appearances, and stroked text; [`HAIRLINES_NOT_COVERED`] says so in one sentence.

use crate::page_tree::{Page, Rect};
use crate::vector::{self, Matrix, PageObjects, VectorObject};
use crate::view::DocumentView;

use super::FindingTarget;

/// What [`hairlines`] does not measure, as a sentence for a report.
pub const HAIRLINES_NOT_COVERED: &str = "Strokes inside Type 3 glyphs, tiling-pattern cells and annotation \
     appearances, and stroked text, are not measured or changed.";

/// One stroke thinner than the minimum.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Hairline {
    /// [`FindingTarget::PageObject`] or [`FindingTarget::FormLeaf`].
    pub target: FindingTarget,
    /// The stroke's line width in its own user space (the `w` value).
    pub line_width: f64,
    /// Points of default user space per user-space unit along the CTM's
    /// thinnest axis: its smallest singular value.
    pub axis_scale: f64,
    /// The printed width in points: `line_width * axis_scale`.
    pub width_pt: f64,
    /// True for line width 0.
    pub zero_width: bool,
    /// The stroke's extent in page space.
    pub rect: Rect,
}

impl Hairline {
    /// The user-space line width that prints `min_width_pt` points along the
    /// thinnest axis.
    #[must_use]
    pub fn width_for(&self, min_width_pt: f64) -> f64 {
        min_width_pt / self.axis_scale
    }
}

/// Every stroke on `page` that prints thinner than `min_width_pt` points, or
/// has width 0, in [`vector::decompose_page`] order: page objects, then form
/// leaves. A path under a CTM that collapses an axis paints no area and is
/// not reported.
///
/// # Errors
///
/// The page content could not be decoded.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::{page_tree, preflight};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../../fixtures/synthetic/hello.pdf").to_vec(),
/// )?;
/// let pages = page_tree::pages(&doc)?;
/// let thin = preflight::hairlines(&doc.view(), &pages[0], 0.25)?;
/// assert!(thin.iter().all(|h| h.zero_width || h.width_pt < 0.25));
/// # Ok(())
/// # }
/// ```
pub fn hairlines(
    view: &DocumentView<'_>,
    page: &Page,
    min_width_pt: f64,
) -> Result<Vec<Hairline>, crate::content::ContentError> {
    let objs = vector::decompose_page(view, page, Matrix::IDENTITY)?;
    Ok(hairlines_in(&objs, min_width_pt))
}

/// [`hairlines`] over an already decomposed page.
pub(crate) fn hairlines_in(objs: &PageObjects, min_width_pt: f64) -> Vec<Hairline> {
    let top = objs
        .objects
        .iter()
        .enumerate()
        .map(|(i, o)| (FindingTarget::PageObject(i), o));
    let leaves = objs
        .leaves
        .iter()
        .enumerate()
        .map(|(i, l)| (FindingTarget::FormLeaf(i), &l.object));
    top.chain(leaves)
        .filter_map(|(target, o)| measure(target, o, min_width_pt))
        .collect()
}

/// `o` as a hairline, when it is a stroked path below the minimum.
fn measure(target: FindingTarget, o: &VectorObject, min_width_pt: f64) -> Option<Hairline> {
    let VectorObject::Path(p) = o else {
        return None;
    };
    if !p.style.stroke {
        return None;
    }
    let axis_scale = min_axis_scale(p.ctm);
    if axis_scale <= 0.0 || !axis_scale.is_finite() {
        return None;
    }
    let width_pt = p.line_width * axis_scale;
    let zero_width = p.line_width <= 0.0;
    // The relative tolerance keeps a stroke written at exactly the minimum
    // from re-reporting through floating-point rounding.
    let thin = width_pt < min_width_pt * (1.0 - 1e-9);
    (zero_width || thin).then(|| Hairline {
        target,
        line_width: p.line_width,
        axis_scale,
        width_pt,
        zero_width,
        rect: super::checks::rect(p.page_bbox),
    })
}

/// The smallest singular value of the CTM's linear part.
///
/// For `M = [a b; c d]`, `s_max * s_min = |det M|` and
/// `s_max^2 + s_min^2 = a^2 + b^2 + c^2 + d^2`; `s_min` is the smaller root.
pub(crate) fn min_axis_scale(m: Matrix) -> f64 {
    let sum = m.a * m.a + m.b * m.b + m.c * m.c + m.d * m.d;
    let det = (m.a * m.d - m.b * m.c).abs();
    let disc = (sum * sum - 4.0 * det * det).max(0.0).sqrt();
    // `2 det^2 / (sum + disc)` is the small root without cancellation.
    if sum + disc <= 0.0 {
        0.0
    } else {
        (2.0 * det * det / (sum + disc)).sqrt()
    }
}

#[cfg(test)]
#[path = "hairlines_tests.rs"]
pub(crate) mod tests;

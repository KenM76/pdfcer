//! # Centerline derivation ("line center, not thickness" — decision 011 §2.1)
//!
//! Dimensions snap to a path's **geometry** (its centerline), never to a
//! stroke's edges. For a **stroked** path that is automatic — the stroke
//! straddles the path ±w/2, so the path nodes already *are* the centerline
//! and the snapping engine needs nothing special. The hard case is a line
//! an author drew as a **thin filled rectangle** (or a filled 4-point
//! quad): its geometry is the *outline* of a bar, and its centerline — the
//! midline the operator wants to measure to — must be **derived**.
//!
//! This module derives that midline: for a filled 4-anchor quad whose
//! long:short edge aspect ratio exceeds [`CENTERLINE_ASPECT_THRESHOLD`], it
//! connects the **midpoints of the two short edges**.
//!
//! ## Fuzzy, never sneaky (rule 4)
//!
//! The derivation is a **fuzzy inference** — a genuinely rectangular filled
//! region (a table cell, a filled swatch) can look exactly like a thick
//! line. So a [`CenterlineCandidate`] is exactly that: a **candidate** the
//! GUI shows highlighted, with a "centerline derived from filled shape"
//! disclosure, that the operator confirms or overrides. This module
//! **never** commits a centerline and **never** mutates the object; it only
//! reports candidates and their count (decision 011 "Count/flag every
//! derivation"). The aspect-ratio threshold plus mandatory confirmation is
//! the Z3 (false-positive) mitigation from decision 011 Appendix A.

use super::decompose::{PageObjects, PathObject, VectorObject};
use super::geometry::Point;

/// The long:short edge aspect ratio a filled quad must exceed for its
/// midline to be *offered* as a centerline candidate (decision 011 §2.1:
/// "long:short over ~8:1"). Below this, a filled quad is treated as a
/// genuine rectangle, not a drawn line, and no candidate is produced.
pub const CENTERLINE_ASPECT_THRESHOLD: f64 = 8.0;

/// A derived centerline offered for operator confirmation (module docs).
///
/// The endpoints are in **page space** (the same frame the object's
/// [`PathObject::page_subpaths`] and the hit-test use), so the GUI can draw
/// the highlighted candidate directly and the snapping engine can snap to
/// it once confirmed. Nothing here is applied automatically.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CenterlineCandidate {
    /// Index into [`PageObjects::objects`] of the filled quad this midline
    /// was derived from — the object the disclosure names and the operator
    /// confirms against.
    pub object_index: usize,
    /// One end of the derived midline (midpoint of one short edge), page
    /// space.
    pub start: Point,
    /// The other end (midpoint of the opposite short edge), page space.
    pub end: Point,
    /// The measured long:short edge aspect ratio (≥
    /// [`CENTERLINE_ASPECT_THRESHOLD`]) — surfaced so the operator sees how
    /// line-like the shape is.
    pub aspect_ratio: f64,
    /// Length of the derived midline (≈ the long edge), page space — the
    /// value a linear dimension along the centerline would measure before
    /// scaling.
    pub length: f64,
}

/// Every centerline candidate on a page: one per filled quad whose aspect
/// ratio clears the threshold, in object (paint) order.
///
/// The count (`.len()`) is the "flag every derivation" signal decision 011
/// asks for; the GUI presents each as a confirmable hint.
#[must_use]
pub fn page_candidates(model: &PageObjects) -> Vec<CenterlineCandidate> {
    model
        .objects
        .iter()
        .enumerate()
        .filter_map(|(i, obj)| match obj {
            VectorObject::Path(p) => derive_from_path(i, p),
            _ => None,
        })
        .collect()
}

/// Derive a centerline candidate from one path object, or `None` if it is
/// not a filled quad line-like enough to offer.
///
/// Requirements (all must hold):
/// 1. the object is **filled** (a stroked-only quad is a drawn box outline,
///    already centerline-snappable via its edges);
/// 2. it is exactly one **closed 4-anchor quad** ([`PathObject::is_quad`]);
/// 3. its page-space long:short aspect ratio ≥
///    [`CENTERLINE_ASPECT_THRESHOLD`].
#[must_use]
pub fn derive_from_path(index: usize, path: &PathObject) -> Option<CenterlineCandidate> {
    if path.style.fill.is_none() || !path.is_quad() {
        return None;
    }
    let page = path.page_subpaths();
    let sp = page.first()?;
    // Anchors of a closed 4-anchor quad: P0 (start) + three segment ends.
    let anchors: Vec<Point> = sp.anchors().filter(|p| p.is_finite()).collect();
    let [p0, p1, p2, p3] = <[Point; 4]>::try_from(anchors).ok()?;

    // The four edges, as (midpoint, length). Opposite edges pair up:
    // (e0,e2) are one pair of sides, (e1,e3) the other.
    let e0 = edge(p0, p1);
    let e1 = edge(p1, p2);
    let e2 = edge(p2, p3);
    let e3 = edge(p3, p0);

    // The SHORT edges are the pair with the smaller length; the midline
    // joins their midpoints. Compare one representative from each pair.
    let (short_a, short_b, short_len, long_len) = if e0.1 < e1.1 {
        // e0/e2 are the short pair (the caps); e1/e3 are the long sides.
        (e0.0, e2.0, mean(e0.1, e2.1), mean(e1.1, e3.1))
    } else {
        // e1/e3 are the short pair.
        (e1.0, e3.0, mean(e1.1, e3.1), mean(e0.1, e2.1))
    };

    if !short_len.is_finite() || short_len <= f64::EPSILON || !long_len.is_finite() {
        return None; // degenerate (zero-area or non-finite) quad
    }
    let aspect_ratio = long_len / short_len;
    if aspect_ratio < CENTERLINE_ASPECT_THRESHOLD {
        return None;
    }
    Some(CenterlineCandidate {
        object_index: index,
        start: short_a,
        end: short_b,
        aspect_ratio,
        length: short_a.distance(short_b),
    })
}

/// An edge as `(midpoint, length)`.
fn edge(a: Point, b: Point) -> (Point, f64) {
    (a.midpoint(b), a.distance(b))
}

/// The arithmetic mean of two lengths (the representative length of an
/// opposite-edge pair).
fn mean(a: f64, b: f64) -> f64 {
    (a + b) / 2.0
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::content::ContentStream;
    use crate::vector::decompose::{NoXObjects, decompose};
    use crate::vector::geometry::Matrix;

    fn model(src: &[u8]) -> PageObjects {
        let cs = ContentStream::parse(src.to_vec()).unwrap();
        decompose(&cs, Matrix::IDENTITY, &NoXObjects)
    }

    #[test]
    fn a_thin_horizontal_filled_bar_yields_a_horizontal_midline() {
        // 200 wide, 4 tall (aspect 50) filled rectangle at (10, 100).
        let m = model(b"10 100 200 4 re f");
        let cands = page_candidates(&m);
        assert_eq!(cands.len(), 1);
        let c = cands[0];
        assert_eq!(c.object_index, 0);
        // midline runs along y = 102 (center of the 100..104 band)
        assert!((c.start.y - 102.0).abs() < 1e-6);
        assert!((c.end.y - 102.0).abs() < 1e-6);
        // from x=10 to x=210 (either order)
        let xs = [c.start.x, c.end.x];
        assert!(xs.contains(&10.0) && xs.contains(&210.0));
        assert!((c.aspect_ratio - 50.0).abs() < 1e-6);
        assert!((c.length - 200.0).abs() < 1e-6);
    }

    #[test]
    fn a_thin_vertical_filled_bar_yields_a_vertical_midline() {
        // 4 wide, 200 tall (aspect 50).
        let m = model(b"100 10 4 200 re f");
        let c = page_candidates(&m)[0];
        assert!((c.start.x - 102.0).abs() < 1e-6);
        assert!((c.end.x - 102.0).abs() < 1e-6);
        assert!((c.length - 200.0).abs() < 1e-6);
    }

    #[test]
    fn a_square_fill_is_not_offered_a_centerline() {
        // aspect 1 — a genuine rectangle, no candidate (Z3 false-positive
        // guard).
        let m = model(b"0 0 100 100 re f");
        assert!(page_candidates(&m).is_empty());
    }

    #[test]
    fn a_below_threshold_bar_is_not_offered() {
        // aspect 4 (< 8) — treated as a rectangle, not a line.
        let m = model(b"0 0 40 10 re f");
        assert!(page_candidates(&m).is_empty());
    }

    #[test]
    fn a_stroked_only_thin_quad_is_not_a_filled_line() {
        // A thin quad that is STROKED, not filled: its edges are already
        // the centerline, so no derivation is offered.
        let m = model(b"0 0 200 4 re S");
        assert!(page_candidates(&m).is_empty());
    }

    #[test]
    fn a_rotated_thin_filled_quad_derives_a_rotated_midline() {
        // A thin bar drawn as an explicit 4-point quad under a 90° CTM:
        // the midline is derived in page space (rotation-correct).
        // Bar 100 long x 2 thick, rotated 90°: page-space midline is
        // vertical. Draw in user space then rotate via cm.
        let m = model(b"0 1 0 -1 0 0 cm 0 0 m 100 0 l 100 2 l 0 2 l h f");
        let cands = page_candidates(&m);
        assert_eq!(cands.len(), 1);
        // Under [0 1 0 -1 0 0] the x-axis maps to the y-axis, so the long
        // dimension is vertical in page space.
        let c = cands[0];
        assert!(
            (c.start.x - c.end.x).abs() < 1e-6,
            "midline is vertical in page space"
        );
        assert!((c.length - 100.0).abs() < 1e-6);
    }
}

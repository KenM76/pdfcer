//! Shrinks a search hit's quad before it becomes a `/Redact` mark.
//!
//! A search quad starts exactly where the previous glyph's advance ends, and
//! redaction apply counts a touching box as a hit (its over-redaction bias),
//! so an exact quad also removed the character on each side of the match.

use crate::annot_author::Quad;

/// `quad` with both ends pulled in along its writing direction by 1% of its
/// height, capped at a quarter of its length. The matched glyphs still
/// overlap it; their neighbours no longer touch it.
pub(crate) fn inset_ends(quad: Quad) -> Quad {
    let (ax, ay) = (quad.ur.0 - quad.ul.0, quad.ur.1 - quad.ul.1);
    let len = ax.hypot(ay);
    let height = (quad.ul.0 - quad.ll.0).hypot(quad.ul.1 - quad.ll.1);
    if !(len.is_finite() && height.is_finite()) || len <= 0.0 {
        return quad;
    }
    let inset = (0.01 * height).min(len / 4.0);
    let (dx, dy) = (ax / len * inset, ay / len * inset);
    let fwd = |p: (f64, f64)| (p.0 + dx, p.1 + dy);
    let back = |p: (f64, f64)| (p.0 - dx, p.1 - dy);
    Quad {
        ul: fwd(quad.ul),
        ll: fwd(quad.ll),
        ur: back(quad.ur),
        lr: back(quad.lr),
    }
}

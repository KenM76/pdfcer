//! The box of the pixels an op can paint, which for a soft-masked layer is
//! tighter than its op box.
//!
//! Where an op paints nothing it leaves the page as it was, so the scene
//! there is the scene without it: an exact cut, not an approximation. A
//! soft-masked layer paints only where its mask is above zero (ISO 32000-2
//! §11.6.5.2: outside the mask group's `/BBox` the mask is the `/BC`
//! value's, zero for the usual black), so its box is its content's box cut
//! to the mask's nonzero pixels. A drop shadow drawn as a large rect under
//! an image mask thus patches only the shadow, and an overprinted paint
//! over it (§11.7.4) stays vector away from the shadow.

use pdfcer_render::writer_seam::{DeviceBounds, Op, op_box};
use tiny_skia::Mask;

/// How deep [`painted`] walks nested layers before settling for the box.
const MAX_DEPTH: usize = 64;

/// The common part of `a` and `b`, `None` when they share no area.
pub(crate) fn intersect(a: DeviceBounds, b: DeviceBounds) -> Option<DeviceBounds> {
    let r = DeviceBounds {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    };
    (r.left < r.right && r.top < r.bottom).then_some(r)
}

fn union(a: DeviceBounds, b: DeviceBounds) -> DeviceBounds {
    DeviceBounds {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}

/// The page, `w × h` device pixels.
pub(crate) fn page_box((w, h): (u32, u32)) -> DeviceBounds {
    #[allow(clippy::cast_precision_loss)]
    DeviceBounds {
        left: 0.0,
        top: 0.0,
        right: w as f32,
        bottom: h as f32,
    }
}

/// The box of the pixels `op` can paint on a `page`-sized recording,
/// within its [`op_box`]; `None` when it paints none.
pub(crate) fn painted(op: &Op, page: (u32, u32)) -> Option<DeviceBounds> {
    painted_in(op, page, 0)
}

fn painted_in(op: &Op, page: (u32, u32), depth: usize) -> Option<DeviceBounds> {
    let bx = op_box(op)?;
    if depth >= MAX_DEPTH {
        return Some(bx);
    }
    let inner = match op {
        Op::Layer { ops, mask, .. } => {
            let inner = ops
                .iter()
                .filter_map(|o| painted_in(o, page, depth + 1))
                .reduce(union)?;
            match mask {
                Some(m) => intersect(inner, support(m, page)?)?,
                None => inner,
            }
        }
        Op::Text { ops, .. } => ops
            .iter()
            .filter_map(|o| painted_in(o, page, depth + 1))
            .reduce(union)?,
        Op::Fill { .. } | Op::Stroke { .. } => bx,
    };
    intersect(inner, bx)
}

/// The box of `mask`'s nonzero pixels; the whole page when the mask is not
/// page-sized, `None` when it is zero everywhere.
fn support(mask: &Mask, page: (u32, u32)) -> Option<DeviceBounds> {
    let (w, h) = (mask.width(), mask.height());
    if (w, h) != page || w == 0 {
        return Some(page_box(page));
    }
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for (y, row) in (0u32..).zip(mask.data().chunks_exact(w as usize)) {
        let Some(first) = row.iter().position(|&v| v > 0) else {
            continue;
        };
        let last = row.iter().rposition(|&v| v > 0).unwrap_or(first);
        // Both are below `w`, a `u32`.
        #[allow(clippy::cast_possible_truncation)]
        let (first, last) = (first as u32, last as u32);
        x0 = x0.min(first);
        x1 = x1.max(last);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    #[allow(clippy::cast_precision_loss)]
    (x0 != u32::MAX).then(|| DeviceBounds {
        left: x0 as f32,
        top: y0 as f32,
        right: (x1 + 1) as f32,
        bottom: (y1 + 1) as f32,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn b(left: f32, top: f32, right: f32, bottom: f32) -> DeviceBounds {
        DeviceBounds {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn a_mask_bounds_its_nonzero_pixels() {
        let mut m = Mask::new(8, 6).unwrap();
        if let Some(v) = m.data_mut().get_mut(2 * 8 + 3) {
            *v = 9;
        }
        if let Some(v) = m.data_mut().get_mut(4 * 8 + 5) {
            *v = 1;
        }
        assert_eq!(support(&m, (8, 6)), Some(b(3.0, 2.0, 6.0, 5.0)));
        assert_eq!(support(&Mask::new(8, 6).unwrap(), (8, 6)), None);
        assert_eq!(support(&m, (9, 6)), Some(b(0.0, 0.0, 9.0, 6.0)));
    }

    #[test]
    fn boxes_meeting_at_an_edge_share_nothing() {
        assert_eq!(
            intersect(b(0.0, 0.0, 2.0, 2.0), b(2.0, 0.0, 4.0, 2.0)),
            None
        );
        assert_eq!(
            intersect(b(0.0, 0.0, 3.0, 2.0), b(2.0, 1.0, 4.0, 5.0)),
            Some(b(2.0, 1.0, 3.0, 2.0))
        );
    }
}

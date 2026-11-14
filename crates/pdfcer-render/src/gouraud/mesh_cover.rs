//! Whether a set of triangles overlaps, sampled at device pixel centres.

/// The largest device extent, in pixels, the test samples; past it the
/// triangles are taken to overlap.
const MAX_PIXELS: usize = 1 << 26;

/// True when some pixel centre lies inside two of `tris`, or the set is
/// too large to sample. A centre on an edge two triangles share belongs
/// to exactly one of them, so a partition does not overlap itself. A fold
/// narrower than a pixel is not seen, as the raster would not see it.
pub(super) fn overlaps(tris: &[[[f32; 2]; 3]]) -> bool {
    let Some((x0, y0, w, h)) = extent(tris) else {
        return false;
    };
    if w.saturating_mul(h) > MAX_PIXELS {
        return true;
    }
    let mut hit = vec![0u64; (w * h).div_ceil(64)];
    for t in tris {
        let mut double = false;
        each_centre(*t, x0, y0, w, h, |i| {
            let bit = 1u64 << (i % 64);
            if let Some(word) = hit.get_mut(i / 64) {
                double |= *word & bit != 0;
                *word |= bit;
            }
        });
        if double {
            return true;
        }
    }
    false
}

/// The pixel box holding every finite triangle: origin and size.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // floored and clamped to >= 0
fn extent(tris: &[[[f32; 2]; 3]]) -> Option<(i64, i64, usize, usize)> {
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in tris
        .iter()
        .flatten()
        .filter(|p| p[0].is_finite() && p[1].is_finite())
    {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    if lo[0] > hi[0] {
        return None;
    }
    let (x0, y0) = (lo[0].floor() as i64, lo[1].floor() as i64);
    let w = (hi[0].ceil() as i64 - x0 + 1).max(0) as usize;
    let h = (hi[1].ceil() as i64 - y0 + 1).max(0) as usize;
    Some((x0, y0, w, h))
}

/// Calls `f` with the index of each pixel whose centre `t` owns.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)] // indices are within the extent
fn each_centre(t: [[f32; 2]; 3], x0: i64, y0: i64, w: usize, h: usize, mut f: impl FnMut(usize)) {
    let [a, b, c] = t.map(|p| [f64::from(p[0]), f64::from(p[1])]);
    let area = cross(a, b, c);
    if !area.is_finite() || area == 0.0 {
        return;
    }
    // Anticlockwise, so a shared edge runs opposite ways in its two triangles.
    let [a, b, c] = if area > 0.0 { [a, b, c] } else { [a, c, b] };
    let edges = [(a, b), (b, c), (c, a)];
    let lo = [a[0].min(b[0]).min(c[0]), a[1].min(b[1]).min(c[1])];
    let hi = [a[0].max(b[0]).max(c[0]), a[1].max(b[1]).max(c[1])];
    let (px0, px1) = ((lo[0] - 0.5).ceil() as i64, (hi[0] - 0.5).floor() as i64);
    let (py0, py1) = ((lo[1] - 0.5).ceil() as i64, (hi[1] - 0.5).floor() as i64);
    for y in py0.max(y0)..=py1.min(y0 + h as i64 - 1) {
        for x in px0.max(x0)..=px1.min(x0 + w as i64 - 1) {
            let p = [x as f64 + 0.5, y as f64 + 0.5];
            if edges.iter().all(|&(u, v)| inside(u, v, p)) {
                f((y - y0) as usize * w + (x - x0) as usize);
            }
        }
    }
}

/// `p` is left of `u → v`, or on it with the edge owning its points.
fn inside(u: [f64; 2], v: [f64; 2], p: [f64; 2]) -> bool {
    let e = cross(u, v, p);
    let (dx, dy) = (v[0] - u[0], v[1] - u[1]);
    e > 0.0 || (e == 0.0 && (dy > 0.0 || (dy == 0.0 && dx > 0.0)))
}

fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_split_square_does_not_overlap_itself() {
        // Every edge runs through pixel centres, so ownership decides each.
        let a = [[0.5, 0.5], [8.5, 0.5], [8.5, 8.5]];
        let b = [[0.5, 0.5], [8.5, 8.5], [0.5, 8.5]];
        assert!(!overlaps(&[a, b]));
    }

    #[test]
    fn a_fold_overlaps() {
        let a = [[0.0, 0.0], [8.0, 0.0], [0.0, 8.0]];
        let b = [[1.0, 1.0], [6.0, 1.0], [1.0, 6.0]];
        assert!(overlaps(&[a, b]));
    }

    #[test]
    fn nothing_finite_does_not_overlap() {
        assert!(!overlaps(&[[[f32::NAN; 2]; 3]]));
    }
}

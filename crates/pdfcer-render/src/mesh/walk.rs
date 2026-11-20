//! A mesh's device triangles in painting order and each one's scan setup:
//! the one walk [`super::rasterise`] fills and the GPU packer records, so
//! the two cannot disagree on which triangles paint or where the page's
//! mesh budget stops them.

use super::{MAX_TRIANGLES, Mesh, MeshData, Shade, map, patch_device_extent, subdivision_for};
use crate::page_budget::{MESH_TRIANGLE_COST, MeshMeter};

/// A triangle's scan conversion: what the pixel loop reads.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Setup {
    /// Device vertices.
    pub(crate) dev: [[f32; 2]; 3],
    /// One over twice the signed area.
    pub(crate) inv_area: f32,
    /// Per-edge barycentric floors for [`super::CRACK_MARGIN_PX`].
    pub(crate) margin: [f32; 3],
    /// Scratch pixels scanned, `(lo_x, hi_x, lo_y, hi_y)`, high exclusive.
    pub(crate) span: (i32, i32, i32, i32),
}

/// What a triangle does to the scan.
pub(crate) enum Step {
    /// Paints nothing; the walk goes on.
    Skip,
    /// Scan these pixels.
    Fill(Setup),
    /// The page's mesh budget is spent; the walk stops.
    Stop,
}

/// `dev`'s setup over a `(w, h)` scratch at device `(ox, oy)`, charging
/// `meter` as the scan would.
pub(crate) fn setup(
    dev: [[f32; 2]; 3],
    (ox, oy): (i32, i32),
    (w, h): (i32, i32),
    meter: MeshMeter<'_>,
) -> Step {
    let skip = |m: MeshMeter<'_>| {
        if m.charge(MESH_TRIANGLE_COST) {
            Step::Skip
        } else {
            Step::Stop
        }
    };
    let [[x0, y0], [x1, y1], [x2, y2]] = dev;
    if !dev.iter().flatten().all(|v| v.is_finite()) {
        return skip(meter);
    }
    // Twice the signed area; zero covers no pixel centre (MSH-N5).
    let area = (x1 - x0).mul_add(y2 - y0, -((x2 - x0) * (y1 - y0)));
    if area.abs() < f32::EPSILON {
        return skip(meter);
    }
    // A normalised barycentric weight is a normalised distance, so "up to
    // `margin` pixels outside edge `i`" is `w_i >= -margin * |edge_i| /
    // |area|`, per edge because a sliver's heights differ by magnitudes.
    let len = |ax: f32, ay: f32, bx: f32, by: f32| (bx - ax).hypot(by - ay);
    let scale = super::CRACK_MARGIN_PX / area.abs();
    let margin = [
        -scale * len(x1, y1, x2, y2),
        -scale * len(x0, y0, x2, y2),
        -scale * len(x0, y0, x1, y1),
    ];
    #[allow(clippy::cast_possible_truncation)] // device pixels
    let (min_x, max_x, min_y, max_y) = (
        x0.min(x1).min(x2).floor() as i32,
        x0.max(x1).max(x2).ceil() as i32,
        y0.min(y1).min(y2).floor() as i32,
        y0.max(y1).max(y2).ceil() as i32,
    );
    let span = (
        (min_x - ox).max(0),
        (max_x - ox + 1).min(w),
        (min_y - oy).max(0),
        (max_y - oy + 1).min(h),
    );
    if span.0 >= span.1 || span.2 >= span.3 {
        return skip(meter);
    }
    #[allow(clippy::cast_sign_loss)] // both sides positive
    let reach = ((span.1 - span.0) as u64) * ((span.3 - span.2) as u64);
    if !meter.charge(MESH_TRIANGLE_COST + reach) {
        return Step::Stop;
    }
    Step::Fill(Setup {
        dev,
        inv_area: 1.0 / area,
        margin,
        span,
    })
}

/// `mesh`'s device triangles under `to_device` in painting order, each
/// handed to `each` until it returns `false`; patches are subdivided
/// (MSH-A3) and charged to `meter` per grid point.
pub(crate) fn walk(
    mesh: &Mesh,
    to_device: tiny_skia::Transform,
    meter: MeshMeter<'_>,
    mut each: impl FnMut([[f32; 2]; 3], [Shade; 3]) -> bool,
) {
    let mut budget = MAX_TRIANGLES;
    match &mesh.data {
        MeshData::Triangles(tris) => {
            for tri in tris {
                if budget == 0 {
                    break;
                }
                budget -= 1;
                let dev = tri.xy.map(|p| map(to_device, p));
                if !each(dev, tri.shade) {
                    break;
                }
            }
        }
        MeshData::Patches(patches) => {
            let mut grid_p: Vec<[f32; 2]> = Vec::new();
            let mut grid_s: Vec<Shade> = Vec::new();
            for patch in patches {
                let n = subdivision_for(patch_device_extent(patch, to_device));
                let side = (n + 1) as usize;
                let cells = (n as usize) * (n as usize) * 2;
                // The ceiling is on the whole mesh, so a patch that does not
                // fit stops it rather than drawing coarser than it asked.
                if budget == 0 || cells > budget {
                    break;
                }
                budget -= cells;
                if !meter.charge((side * side) as u64) {
                    break;
                }
                grid_p.clear();
                grid_s.clear();
                #[allow(clippy::cast_precision_loss)] // a subdivision count
                for jv in 0..=n {
                    let v = jv as f32 / n as f32;
                    for iu in 0..=n {
                        let u = iu as f32 / n as f32;
                        grid_p.push(map(to_device, patch.at(u, v)));
                        grid_s.push(patch.shade_at(u, v));
                    }
                }
                if !cells_of(&grid_p, &grid_s, side, &mut each) {
                    break;
                }
            }
        }
    }
}

/// A grid's two triangles per cell, increasing `v` outer and `u` inner so
/// the largest-`v` point wins a fold (MSH32); `false` once `each` stops.
fn cells_of(
    grid_p: &[[f32; 2]],
    grid_s: &[Shade],
    side: usize,
    each: &mut impl FnMut([[f32; 2]; 3], [Shade; 3]) -> bool,
) -> bool {
    let rows_p = grid_p
        .chunks_exact(side)
        .zip(grid_p.chunks_exact(side).skip(1));
    let rows_s = grid_s
        .chunks_exact(side)
        .zip(grid_s.chunks_exact(side).skip(1));
    for ((p0, p1), (s0, s1)) in rows_p.zip(rows_s) {
        let cells = p0
            .windows(2)
            .zip(p1.windows(2))
            .zip(s0.windows(2).zip(s1.windows(2)));
        for ((pt, pb), (st, sb)) in cells {
            let (&[pa, pb_], &[pc, pd], &[sa, sb_], &[sc, sd]) = (pt, pb, st, sb) else {
                continue;
            };
            if !each([pa, pb_, pc], [sa, sb_, sc]) || !each([pb_, pc, pd], [sb_, sc, sd]) {
                return false;
            }
        }
    }
    true
}

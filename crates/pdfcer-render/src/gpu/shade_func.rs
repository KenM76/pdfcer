//! A type 1 (function-based) shading's record for the shader to evaluate,
//! and that evaluation in Rust for [`super::reference`]: the CPU's sampled
//! lattice, looked up and interpolated bilinearly per pixel as
//! [`crate::shading::function_based`] does.
//!
//! Record layout from its first texel, `f32`s as bit patterns: `[−4, flags,
//! 0, 0]` (flags bit 2 `/BBox`), the walked rectangle, the device-to-target
//! transform `[sx, kx, tx, ky]`, `[sy, ty, 0, 0]`, the `/BBox` `[x_min,
//! x_max, y_min, y_max]`, target-to-domain `[sx, kx, tx, ky]`, `[sy, ty,
//! cells per unit x, y]`, `/Domain` `[x_min, x_max, y_min, y_max]`, the
//! alpha table at [`EVAL_HEAD`], then per lattice node `[r, g, b, valid]`.

use std::sync::Arc;

use super::scene::{Frame, GpuRefusal};
use super::shade::{EVAL_HEAD, ShadePaint};
use super::shade_eval::{Evaluated, alpha_lanes, area_edges, bits};
use crate::display_list::shading_op::{self, ShadingOp};
use crate::shading::function_based::{GRID, GridTerms};

/// A function-based record's tag.
pub(crate) const FUNC_TAG: i32 = -4;
/// The first lattice node's texel.
const NODES: i32 = EVAL_HEAD + 64;

/// `op`'s record over `frame`, `None` when `op` is not a type 1 shading
/// with a lattice; inside, `None` where it paints nothing.
///
/// # Errors
///
/// What [`Frame::edges`] refuses.
pub(crate) fn eval(
    frame: &Frame<'_>,
    op: &ShadingOp,
) -> Option<Result<Option<Evaluated>, GpuRefusal>> {
    let grid = op.shading.grid.as_ref()?.gpu_terms();
    let size = (frame.width, frame.height);
    let Some((to_target, rect)) =
        shading_op::eval_frame(op, frame.clips, size, (frame.x0, frame.y0))
    else {
        return Some(Ok(None));
    };
    let edges = area_edges(frame, op, rect);
    let texels = texels(&grid, (to_target, op.shading.bbox), rect, op.alpha);
    Some(edges.map(|e| Some((Arc::new(ShadePaint::Evaluated(texels)), e))))
}

fn texels(
    grid: &GridTerms<'_>,
    (m, bbox): (tiny_skia::Transform, Option<[f32; 4]>),
    (l, t, r, b): (i32, i32, i32, i32),
    alpha: f32,
) -> Vec<[i32; 4]> {
    let bb = bbox.map_or([0.0; 4], |[x0, y0, x1, y1]| {
        [x0.min(x1), x0.max(x1), y0.min(y1), y0.max(y1)]
    });
    let d = grid.from_target;
    let mut out = vec![
        [FUNC_TAG, i32::from(bbox.is_some()) << 2, 0, 0],
        [l, t, r, b],
        bits([m.sx, m.kx, m.tx, m.ky]),
        bits([m.sy, m.ty, 0.0, 0.0]),
        bits(bb),
        bits([d.sx, d.kx, d.tx, d.ky]),
        bits([d.sy, d.ty, grid.scale[0], grid.scale[1]]),
        bits(grid.domain),
    ];
    out.extend(alpha_lanes(alpha));
    out.extend(grid.nodes.iter().map(|n| match n {
        Some([r, g, b]) => {
            let [r, g, b, _] = bits([*r, *g, *b, 0.0]);
            [r, g, b, 1]
        }
        None => [0; 4],
    }));
    out
}

/// What the shader's `func_eval` returns for the record at `rec` at pixel
/// `(px, py)` under clip-coverage byte `cb`.
pub(crate) fn reference(
    at: &dyn Fn(i32) -> [i32; 4],
    rec: i32,
    (px, py): (i32, i32),
    cb: i32,
) -> Option<[f32; 4]> {
    #[allow(clippy::cast_sign_loss)] // bit patterns
    let f = |t: [i32; 4]| t.map(|v| f32::from_bits(v as u32));
    let [_, flags, _, _] = at(rec);
    let [l, t, r, b] = at(rec + 1);
    if px < l || py < t || px >= r || py >= b {
        return None;
    }
    let [sx, kx, tx, ky] = f(at(rec + 2));
    let [sy, ty, _, _] = f(at(rec + 3));
    #[allow(clippy::cast_precision_loss)] // raster pixel coordinates
    let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
    let (x, y) = (fx * sx + fy * kx + tx, fx * ky + fy * sy + ty);
    let [bx0, bx1, by0, by1] = f(at(rec + 4));
    if flags & 4 != 0 && (x < bx0 || x > bx1 || y < by0 || y > by1) {
        return None;
    }
    let [dsx, dkx, dtx, dky] = f(at(rec + 5));
    let [dsy, dty, su, sv] = f(at(rec + 6));
    let (x, y) = (x * dsx + y * dkx + dtx, x * dky + y * dsy + dty);
    let [x0, x1, y0, y1] = f(at(rec + 7));
    if !(x0..=x1).contains(&x) || !(y0..=y1).contains(&y) {
        return None;
    }
    let (u, v) = ((x - x0) * su, (y - y0) * sv);
    #[allow(clippy::cast_precision_loss)]
    let last = (GRID - 1) as f32;
    #[allow(clippy::cast_possible_truncation)] // in 0..GRID-1 after the clamp
    let cell = |t: f32| t.floor().clamp(0.0, last - 1.0) as i32;
    let (i, j) = (cell(u), cell(v));
    #[allow(clippy::cast_precision_loss)]
    let (fu, fv) = (u - i as f32, v - j as f32);
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let row = GRID as i32;
    let node = |di: i32, dj: i32| {
        let n = at(rec + NODES + (j + dj) * row + i + di);
        (n[3] != 0).then(|| f(n))
    };
    let (c00, c10, c01, c11) = (node(0, 0)?, node(1, 0)?, node(0, 1)?, node(1, 1)?);
    let lane = at(rec + EVAL_HEAD + (cb >> 2));
    #[allow(clippy::cast_sign_loss)] // bit patterns; `cb & 3` is a lane
    let a = f32::from_bits(lane.get((cb & 3) as usize).copied().unwrap_or(0) as u32);
    if a.is_nan() || a <= 0.0 {
        return None;
    }
    let a = a.min(1.0);
    let ch = |k: usize| {
        let g = |c: [f32; 4]| c.get(k).copied().unwrap_or(0.0);
        let top = fu * (g(c10) - g(c00)) + g(c00);
        let bottom = fu * (g(c11) - g(c01)) + g(c01);
        (fv * (bottom - top) + top) * a * 255.0
    };
    Some([ch(0), ch(1), ch(2), a])
}

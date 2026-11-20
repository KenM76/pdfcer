//! An axial or radial shading's record for the shader to evaluate, and that
//! evaluation in Rust for [`super::reference`]; meshes and type 1 shadings
//! are handed to [`super::shade_mesh`] and [`super::shade_func`].
//!
//! Record layout from its first texel, `f32`s as bit patterns:
//! `[tag, flags, 0, 0]` (tag −1 axial, −2 radial; flags bit 0/1 extend,
//! 2 `/BBox`, 3 zero ramp span), the walked rectangle `[l, t, r, b]`, the
//! device-to-target transform `[sx, kx, tx, ky]`, `[sy, ty, ramp t0,
//! 1/span]`, the `/BBox` `[x_min, x_max, y_min, y_max]`, then
//! [`Terms::origin`], [`Terms::axis`], [`Terms::cone`]; at
//! [`EVAL_HEAD`] the alpha per clip-coverage byte (256 lanes), at
//! [`EVAL_RAMP`] per ramp sample `[r, g, b, opaque]`, `opaque` the packed
//! opaque bytes with alpha 255, zero for a hole.

use std::sync::Arc;

use tiny_skia::{FillRule, PathBuilder, Rect, Transform};

use super::scene::{Frame, GpuRefusal};
use super::shade::{EVAL_HEAD, EVAL_RAMP, ShadePaint};
use crate::display_list::shading_op::{self, ShadingOp};
use crate::shading::gpu_terms::{Kind, Terms};

/// An evaluated shading and the edges of the area it draws in each tile.
pub(super) type Evaluated = (Arc<ShadePaint>, Vec<Option<super::fill_edges::FillEdges>>);

#[allow(clippy::cast_possible_wrap)] // bit patterns
pub(super) fn bits(v: [f32; 4]) -> [i32; 4] {
    v.map(|f| f.to_bits() as i32)
}

/// `op`'s record over `frame`, `None` for a shading the shader does not
/// evaluate; inside, `None` where it paints nothing.
///
/// # Errors
///
/// What [`Frame::edges`] refuses.
pub(crate) fn eval(
    frame: &Frame<'_>,
    op: &ShadingOp,
) -> Option<Result<Option<Evaluated>, GpuRefusal>> {
    if let Some(mesh) = super::shade_mesh::eval(frame, op) {
        return Some(mesh);
    }
    if let Some(func) = super::shade_func::eval(frame, op) {
        return Some(func);
    }
    let terms = op.shading.gpu_terms()?;
    let size = (frame.width, frame.height);
    let Some((to_target, rect)) =
        shading_op::eval_frame(op, frame.clips, size, (frame.x0, frame.y0))
    else {
        return Some(Ok(None));
    };
    let edges = area_edges(frame, op, rect);
    let texels = texels(&terms, to_target, rect, op.alpha);
    Some(edges.map(|e| Some((Arc::new(ShadePaint::Evaluated(texels)), e))))
}

/// The edges of the area `op` draws over `frame`: its path, or else the
/// walked rectangle `rect`.
///
/// # Errors
///
/// What [`Frame::edges`] refuses.
pub(super) fn area_edges(
    frame: &Frame<'_>,
    op: &ShadingOp,
    rect: (i32, i32, i32, i32),
) -> Result<Vec<Option<super::fill_edges::FillEdges>>, GpuRefusal> {
    match &op.area {
        Some(a) => frame.edges(&a.path, a.rule, frame.to_region(a.ctm), a.anti_alias),
        None => {
            let (l, t, r, b) = rect;
            #[allow(clippy::cast_precision_loss)] // raster pixel coordinates
            let Some(path) =
                Rect::from_ltrb(l as f32, t as f32, r as f32, b as f32).map(PathBuilder::from_rect)
            else {
                return Ok(vec![]);
            };
            frame.edges(&path, FillRule::Winding, Transform::identity(), false)
        }
    }
}

/// `paint_region`'s `alpha * coverage` per clip byte, four lanes a texel.
pub(super) fn alpha_lanes(alpha: f32) -> impl Iterator<Item = [i32; 4]> {
    let alpha = alpha.clamp(0.0, 1.0);
    let lanes: Vec<f32> = (0..=255u8)
        .map(|c| alpha * (f32::from(c) / 255.0))
        .collect();
    (0..64).map(move |i| {
        bits(std::array::from_fn(|k| {
            lanes.get(i * 4 + k).copied().unwrap_or(0.0)
        }))
    })
}

fn texels(
    terms: &Terms,
    m: Transform,
    (l, t, r, b): (i32, i32, i32, i32),
    alpha: f32,
) -> Vec<[i32; 4]> {
    let tag = match terms.kind {
        Kind::Axial => -1,
        Kind::Radial => -2,
    };
    let (rt0, inv_span) = terms.ramp_scale;
    let flags = i32::from(terms.extend[0])
        | (i32::from(terms.extend[1]) << 1)
        | (i32::from(terms.bbox.is_some()) << 2)
        | (i32::from(inv_span.is_none()) << 3);
    let mut out = Vec::with_capacity(EVAL_RAMP as usize + terms.samples.len());
    out.push([tag, flags, 0, 0]);
    out.push([l, t, r, b]);
    out.push(bits([m.sx, m.kx, m.tx, m.ky]));
    out.push(bits([m.sy, m.ty, rt0, inv_span.unwrap_or(0.0)]));
    out.push(bits(terms.bbox.unwrap_or_default()));
    out.push(bits(terms.origin));
    out.push(bits(terms.axis));
    out.push(bits(terms.cone));
    out.extend(alpha_lanes(alpha));
    for s in &terms.samples {
        out.push(match s {
            Some((rgb, o)) => {
                let [r, g, b] = bits([rgb[0], rgb[1], rgb[2], 0.0])[..3]
                    .try_into()
                    .unwrap_or([0; 3]);
                [r, g, b, i32::from_le_bytes([o[0], o[1], o[2], 255])]
            }
            None => [0; 4],
        });
    }
    out
}

/// `d`'s correctly rounded square root from `r`, an estimate within a few
/// units in the last place, as the shader computes it: the root `R` of
/// mantissa `D` is right when `R² − R < D ≤ R² + R` (at the bottom of a
/// binade, `R² − R/2`), compared exactly in 24-bit limbs.
pub(crate) fn sqrt_rn(d: f32, mut r: f32) -> f32 {
    if d.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return d.max(0.0).sqrt();
    }
    let db = d.to_bits();
    let ed = (db >> 23) as i32;
    if ed == 0 {
        return r;
    }
    let md = (db & 0x7F_FFFF) | 0x80_0000;
    for _ in 0..4 {
        let rb = r.to_bits();
        let er = (rb >> 23) as i32;
        let rm = (rb & 0x7F_FFFF) | 0x80_0000;
        let k = ed + 150 - 2 * er;
        // Past the limbs `R` is a binade off: too small above, too big below.
        if k > 24 {
            r = f32::from_bits(rb + 1);
            continue;
        }
        if k < 0 {
            r = f32::from_bits(rb - 1);
            continue;
        }
        let dd = limbs(md >> (24 - k), (md & ((1 << (24 - k)) - 1)) << k);
        let q = square(rm);
        let upper = q + u64::from(rm);
        let lower = q - u64::from(if rm == 0x80_0000 { rm >> 1 } else { rm });
        if dd > upper {
            r = f32::from_bits(rb + 1);
        } else if dd <= lower {
            r = f32::from_bits(rb - 1);
        } else {
            return r;
        }
    }
    r
}

fn limbs(hi: u32, lo: u32) -> u64 {
    (u64::from(hi) << 24) + u64::from(lo)
}

/// `r²` as the shader forms it, in 12-bit halves.
fn square(r: u32) -> u64 {
    let (h, l) = (r >> 12, r & 4095);
    let m = 2 * h * l;
    let lo = l * l + ((m & 4095) << 12);
    let hi = h * h + (m >> 12) + (lo >> 24);
    limbs(hi, lo & 0xFF_FFFF)
}

/// What the shader's `shade_eval` returns for the record at `rec` at pixel
/// `(px, py)` under clip-coverage byte `cb`: the source terms and alpha
/// [`super::shade::over`] composites, `None` where it paints nothing.
pub(crate) fn reference(
    at: &dyn Fn(i32) -> [i32; 4],
    rec: i32,
    (px, py): (i32, i32),
    cb: i32,
) -> Option<[f32; 4]> {
    #[allow(clippy::cast_sign_loss)] // bit patterns
    let f = |t: [i32; 4]| t.map(|v| f32::from_bits(v as u32));
    let [tag, flags, _, _] = at(rec);
    if tag == super::shade_mesh::MESH_TAG {
        return super::shade_mesh::reference(at, rec, (px, py), cb);
    }
    if tag == super::shade_func::FUNC_TAG {
        return super::shade_func::reference(at, rec, (px, py), cb);
    }
    let [l, t, r, b] = at(rec + 1);
    if px < l || py < t || px >= r || py >= b {
        return None;
    }
    let [sx, kx, tx, ky] = f(at(rec + 2));
    let [sy, ty, rt0, inv_span] = f(at(rec + 3));
    #[allow(clippy::cast_precision_loss)] // raster pixel coordinates
    let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
    let x = fx * sx + fy * kx + tx;
    let y = fx * ky + fy * sy + ty;
    let [bx0, bx1, by0, by1] = f(at(rec + 4));
    if flags & 4 != 0 && (x < bx0 || x > bx1 || y < by0 || y > by1) {
        return None;
    }
    let ext = [flags & 1 != 0, flags & 2 != 0];
    let t = param(
        tag,
        (f(at(rec + 5)), f(at(rec + 6)), f(at(rec + 7))),
        ext,
        (x, y),
    )?;
    let frac = if flags & 8 != 0 {
        0.0
    } else {
        ((t - rt0) * inv_span).clamp(0.0, 1.0)
    };
    #[allow(clippy::cast_possible_truncation)] // in 0..=255
    let idx = (frac * 255.0).round() as i32;
    let s = at(rec + EVAL_RAMP + idx);
    if s[3] == 0 {
        return None;
    }
    let lane = at(rec + EVAL_HEAD + (cb >> 2));
    #[allow(clippy::cast_sign_loss)] // bit patterns; `cb & 3` is a lane
    let a = f32::from_bits(lane.get((cb & 3) as usize).copied().unwrap_or(0) as u32);
    if a.is_nan() || a <= 0.0 {
        return None;
    }
    if a >= 1.0 {
        let o = s[3].to_le_bytes();
        return Some([f32::from(o[0]), f32::from(o[1]), f32::from(o[2]), 1.0]);
    }
    let c = f(s);
    Some([c[0] * a * 255.0, c[1] * a * 255.0, c[2] * a * 255.0, a])
}

/// `t` at target point `(x, y)` from [`Terms::origin`], [`Terms::axis`] and
/// [`Terms::cone`], as `axial_param` and `radial_param` compute it.
fn param(
    tag: i32,
    ([x0, y0, t0, t1], axis, [_, a4, inv2a, rtd]): ([f32; 4], [f32; 4], [f32; 4]),
    ext: [bool; 2],
    (x, y): (f32, f32),
) -> Option<f32> {
    if tag == -1 {
        let [dx, dy, inv, td] = axis;
        let xp = (dx * (x - x0) + dy * (y - y0)) * inv;
        return if xp < 0.0 {
            ext[0].then_some(t0)
        } else if xp > 1.0 {
            ext[1].then_some(t1)
        } else {
            Some(td * xp + t0)
        };
    }
    let [dx, dy, dr, r0] = axis;
    let (px, py) = (x - x0, y - y0);
    let b = -2.0 * (px * dx + py * dy + r0 * dr);
    let c = px * px + py * py - r0 * r0;
    let disc = b * b - a4 * c;
    if disc < 0.0 {
        return None;
    }
    let root = sqrt_rn(disc, disc.sqrt());
    let (s1, s2) = ((-b + root) * inv2a, (-b - root) * inv2a);
    let (hi, lo) = if s1 >= s2 { (s1, s2) } else { (s2, s1) };
    let ok = |s: f32| !(s < 0.0 && !ext[0]) && !(s > 1.0 && !ext[1]) && dr * s + r0 >= 0.0;
    let s = if ok(hi) {
        hi
    } else if ok(lo) {
        lo
    } else {
        return None;
    };
    Some(rtd * s.clamp(0.0, 1.0) + t0)
}

#[cfg(test)]
mod tests {
    use super::sqrt_rn;

    #[test]
    fn the_shaders_square_root_corrects_a_guess_a_few_units_out() {
        let mut x: u32 = 0x1234_5678;
        for i in 0..200_000u32 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            // Positive normals, half of them at the bottom of a binade.
            let mut bits = (x % 0x7F00_0000).max(0x0080_0000);
            if i % 2 == 0 {
                bits &= !0x7F_FFFF;
            }
            let d = f32::from_bits(bits);
            let want = d.sqrt();
            for off in [-3i32, -1, 0, 1, 2, 3] {
                let guess = f32::from_bits(want.to_bits().wrapping_add_signed(off));
                assert_eq!(sqrt_rn(d, guess).to_bits(), want.to_bits(), "{d:e} {off}");
            }
            // Just below a power of four, where the root rounds down a binade.
            let below = f32::from_bits(bits.wrapping_sub(1));
            if below.is_normal() {
                assert_eq!(
                    sqrt_rn(below, below.sqrt().next_up()),
                    below.sqrt(),
                    "{below:e}"
                );
            }
        }
    }
}

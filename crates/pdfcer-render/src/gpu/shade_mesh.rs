//! A mesh shading's record for the shader to evaluate, and that evaluation
//! in Rust for [`super::reference`]: [`crate::mesh::paint`]'s triangles,
//! set up and metered at packing time by the walk the CPU fills, binned
//! into 16-pixel tiles.
//!
//! A pixel's colour is the last triangle in painting order that covers it
//! exactly and resolves, else the first whose crack margin covers it: what
//! the CPU's opaque overwrites into its scratch leave behind.
//!
//! Record layout from its first texel, `f32`s as bit patterns: `[−3, flags,
//! tiles per row, 0]` (flags bit 0 parametric, 2 `/BBox`, 3 zero ramp
//! span), the walked rectangle, the device-to-target transform `[sx, kx,
//! tx, ky]`, `[sy, ty, ramp t0, 1/span]`, the `/BBox` `[x_min, x_max,
//! y_min, y_max]`, `[tiles, lists, triangles, ramp]` offsets from the
//! record, two unused texels, the alpha table at [`EVAL_HEAD`]; per tile
//! `[first, count, 0, 0]` into the lists' lanes; per triangle
//! `[x0, y0, x1, y1]`, `[x2, y2, 1/area, margin0]`, `[margin1, margin2, 0,
//! 0]`, its device span `[lo_x, hi_x, lo_y, hi_y]` and per vertex `[r, g,
//! b, 0]` or `[t, 0, 0, 0]`; per ramp sample its packed opaque bytes.

use std::sync::Arc;

use super::scene::{Frame, GpuRefusal};
use super::shade::{EVAL_HEAD, ShadePaint};
use super::shade_eval::{Evaluated, alpha_lanes, area_edges, bits};
use crate::display_list::shading_op::{self, ShadingOp};
use crate::mesh::walk::{self, Step};
use crate::mesh::{Shade, bary};
use crate::page_budget::{MeshMeter, PageBudget};

/// A mesh record's tag.
pub(crate) const MESH_TAG: i32 = -3;
const TILE: i32 = 16;
const TRI: i32 = 7;

/// One triangle as recorded.
struct Tri {
    set: walk::Setup,
    shade: [[f32; 3]; 3],
}

/// `op`'s record over `frame`, `None` when `op` is not a mesh the shader
/// evaluates; inside, `None` where it paints nothing.
///
/// # Errors
///
/// What [`Frame::edges`] refuses.
pub(crate) fn eval(
    frame: &Frame<'_>,
    op: &ShadingOp,
) -> Option<Result<Option<Evaluated>, GpuRefusal>> {
    let mesh = op.shading.mesh.as_ref()?;
    let size = (frame.width, frame.height);
    let Some((to_target, rect)) =
        shading_op::eval_frame(op, frame.clips, size, (frame.x0, frame.y0))
    else {
        return Some(Ok(None));
    };
    let to_device = to_target.invert()?;
    let parametric = mesh_is_parametric(mesh)?;
    let ramp = op.shading.ramp.as_ref();
    if parametric && ramp.is_none() {
        return Some(Ok(None));
    }
    let (l, t, r, b) = rect;
    let budget = PageBudget::default();
    let meter = MeshMeter::new(&budget, op.canvas_pixels);
    let mut tris = Vec::new();
    let mut unsupported = false;
    walk::walk(mesh, to_device, meter, |dev, shade| {
        match walk::setup(dev, (l, t), (r - l, b - t), meter) {
            Step::Skip => true,
            Step::Stop => false,
            Step::Fill(mut set) => {
                let (lx, hx, ly, hy) = set.span;
                set.span = (lx + l, hx + l, ly + t, hy + t);
                match triple(shade, parametric) {
                    Some(shade) => tris.push(Tri { set, shade }),
                    None => unsupported = true,
                }
                !unsupported
            }
        }
    });
    if unsupported {
        return None;
    }
    if tris.is_empty() {
        return Some(Ok(None));
    }
    let edges = area_edges(frame, op, rect);
    let ramp = parametric.then_some(ramp).flatten();
    let texels = texels(&tris, rect, (to_target, op.shading.bbox), ramp, op.alpha);
    Some(edges.map(|e| Some((Arc::new(ShadePaint::Evaluated(texels)), e))))
}

/// Whether the mesh's vertices are parametric; `None` for no triangles'
/// worth of a kind the shader takes.
fn mesh_is_parametric(mesh: &crate::mesh::Mesh) -> Option<bool> {
    let first = match &mesh.data {
        crate::mesh::MeshData::Triangles(t) => t.first().map(|t| t.shade[0]),
        crate::mesh::MeshData::Patches(p) => p.first().map(|p| p.shade_at(0.0, 0.0)),
    }?;
    Some(matches!(first, Shade::Parametric(_)))
}

/// A triangle's vertex terms as [`crate::mesh`]'s `interpolate` weighs
/// them: three of one kind interpolate, a mixed triple paints its first
/// vertex flat; `None` where the kind is not the mesh's.
fn triple(shade: [Shade; 3], parametric: bool) -> Option<[[f32; 3]; 3]> {
    let terms = |s: Shade| match s {
        Shade::Rgb(c) => (0, [c.r, c.g, c.b]),
        Shade::Ink { rgb: c, .. } => (1, [c.r, c.g, c.b]),
        Shade::Spot { rgb: c, .. } => (2, [c.r, c.g, c.b]),
        Shade::Parametric(t) => (3, [t, 0.0, 0.0]),
    };
    let [(ka, a), (kb, b), (kc, c)] = shade.map(terms);
    if (ka == 3) != parametric {
        return None;
    }
    Some(if ka == kb && kb == kc {
        [a, b, c]
    } else {
        [a; 3]
    })
}

fn texels(
    tris: &[Tri],
    (l, t, r, b): (i32, i32, i32, i32),
    (m, bbox): (tiny_skia::Transform, Option<[f32; 4]>),
    ramp: Option<&crate::shading::ColorRamp>,
    alpha: f32,
) -> Vec<[i32; 4]> {
    let (cols, rows) = ((r - l + TILE - 1) / TILE, (b - t + TILE - 1) / TILE);
    let (lists, counts) = bin(tris, (l, t), (cols, rows));
    let parametric = ramp.is_some();
    let (rt0, inv_span) = ramp.map_or((0.0, Some(0.0)), crate::shading::gpu_terms::ramp_scale);
    let flags = i32::from(parametric)
        | (i32::from(bbox.is_some()) << 2)
        | (i32::from(inv_span.is_none()) << 3);
    let ntiles = cols * rows;
    let tiles_at = EVAL_HEAD + 64;
    let lists_at = tiles_at + ntiles;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)] // texel counts
    let tris_at = lists_at + lists.len().div_ceil(4) as i32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let ramp_at = tris_at + TRI * tris.len() as i32;
    let bb = bbox.map_or([0.0; 4], |[x0, y0, x1, y1]| {
        [x0.min(x1), x0.max(x1), y0.min(y1), y0.max(y1)]
    });
    let mut out = vec![
        [MESH_TAG, flags, cols, 0],
        [l, t, r, b],
        bits([m.sx, m.kx, m.tx, m.ky]),
        bits([m.sy, m.ty, rt0, inv_span.unwrap_or(0.0)]),
        bits(bb),
        [tiles_at, lists_at, tris_at, ramp_at],
        [0; 4],
        [0; 4],
    ];
    out.extend(alpha_lanes(alpha));
    let mut first = 0;
    for &n in &counts {
        out.push([first, n, 0, 0]);
        first += n;
    }
    out.extend(
        lists
            .chunks(4)
            .map(|c| std::array::from_fn(|k| c.get(k).copied().unwrap_or(0))),
    );
    for tri in tris {
        let s = &tri.set;
        let [[x0, y0], [x1, y1], [x2, y2]] = s.dev;
        let [m0, m1, m2] = s.margin;
        out.push(bits([x0, y0, x1, y1]));
        out.push(bits([x2, y2, s.inv_area, m0]));
        out.push(bits([m1, m2, 0.0, 0.0]));
        out.push([s.span.0, s.span.1, s.span.2, s.span.3]);
        for v in tri.shade {
            out.push(bits([v[0], v[1], v[2], 0.0]));
        }
    }
    if let Some(ramp) = ramp {
        for s in crate::shading::gpu_terms::samples(ramp) {
            out.push(match s {
                Some((_, o)) => [i32::from_le_bytes([o[0], o[1], o[2], 255]), 0, 0, 0],
                None => [0; 4],
            });
        }
    }
    out
}

/// Each tile's triangle indices in painting order, concatenated, and each
/// tile's count.
fn bin(tris: &[Tri], (l, t): (i32, i32), (cols, rows): (i32, i32)) -> (Vec<i32>, Vec<i32>) {
    let tiles = |s: &walk::Setup| {
        let (lx, hx, ly, hy) = s.span;
        let (c0, c1) = ((lx - l) / TILE, ((hx - l - 1) / TILE).min(cols - 1));
        let (r0, r1) = ((ly - t) / TILE, ((hy - t - 1) / TILE).min(rows - 1));
        (r0..=r1).flat_map(move |row| (c0..=c1).map(move |c| row * cols + c))
    };
    #[allow(clippy::cast_sign_loss)] // tile indices are non-negative
    let mut counts = vec![0i32; (cols * rows) as usize];
    for tri in tris {
        for k in tiles(&tri.set) {
            if let Some(c) = counts.get_mut(k as usize) {
                *c += 1;
            }
        }
    }
    let mut next: Vec<i32> = counts
        .iter()
        .scan(0, |acc, &n| {
            let at = *acc;
            *acc += n;
            Some(at)
        })
        .collect();
    let mut lists = vec![0i32; counts.iter().sum::<i32>().max(0) as usize];
    for (i, tri) in tris.iter().enumerate() {
        for k in tiles(&tri.set) {
            if let Some(at) = next.get_mut(k as usize) {
                if let Some(slot) = lists.get_mut(*at as usize) {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                    {
                        *slot = i as i32;
                    }
                }
                *at += 1;
            }
        }
    }
    (lists, counts)
}

/// What the shader's `mesh_eval` returns for the record at `rec` at pixel
/// `(px, py)` under clip-coverage byte `cb`, as [`super::shade_eval::reference`].
pub(crate) fn reference(
    at: &dyn Fn(i32) -> [i32; 4],
    rec: i32,
    (px, py): (i32, i32),
    cb: i32,
) -> Option<[f32; 4]> {
    #[allow(clippy::cast_sign_loss)] // bit patterns
    let f = |t: [i32; 4]| t.map(|v| f32::from_bits(v as u32));
    let [_, flags, cols, _] = at(rec);
    let [l, t, r, b] = at(rec + 1);
    if px < l || py < t || px >= r || py >= b {
        return None;
    }
    #[allow(clippy::cast_precision_loss)] // raster pixel coordinates
    let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
    if flags & 4 != 0 {
        let [sx, kx, tx, ky] = f(at(rec + 2));
        let [sy, ty, _, _] = f(at(rec + 3));
        let (x, y) = (cx * sx + cy * kx + tx, cx * ky + cy * sy + ty);
        let [bx0, bx1, by0, by1] = f(at(rec + 4));
        if x < bx0 || x > bx1 || y < by0 || y > by1 {
            return None;
        }
    }
    let [tiles_at, lists_at, tris_at, ramp_at] = at(rec + 5);
    let [first, n, _, _] = at(rec + tiles_at + ((py - t) / TILE) * cols + (px - l) / TILE);
    let lane = |e: i32| at(e >> 2).get((e & 3) as usize).copied().unwrap_or(0);
    let mut colour = None;
    for i in (first..first + n).rev() {
        let tri = rec + tris_at + TRI * lane(((rec + lists_at) << 2) + i);
        let [lx, hx, ly, hy] = at(tri + 3);
        if px < lx || px >= hx || py < ly || py >= hy {
            continue;
        }
        let [x0, y0, x1, y1] = f(at(tri));
        let [x2, y2, inv_area, m0] = f(at(tri + 1));
        let [m1, m2, _, _] = f(at(tri + 2));
        let w1 = ((cx - x0) * (y2 - y0) - (cy - y0) * (x2 - x0)) * inv_area;
        let w2 = ((x1 - x0) * (cy - y0) - (y1 - y0) * (cx - x0)) * inv_area;
        let w0 = 1.0 - w1 - w2;
        let exact = !(w0 < 0.0 || w1 < 0.0 || w2 < 0.0);
        if !exact && !(w0 >= m0 && w1 >= m1 && w2 >= m2) {
            continue;
        }
        let ([a0, a1, a2, _], [b0, b1, b2, _], [c0, c1, c2, _]) =
            (f(at(tri + 4)), f(at(tri + 5)), f(at(tri + 6)));
        let w = [w0, w1, w2];
        let rgb = [
            bary(w, a0, b0, c0),
            bary(w, a1, b1, c1),
            bary(w, a2, b2, c2),
        ];
        let Some(c) = resolve(at, rec, flags, ramp_at, rgb) else {
            continue;
        };
        colour = Some(c);
        if exact {
            break;
        }
    }
    let c = colour?;
    let lane_a = at(rec + EVAL_HEAD + (cb >> 2));
    #[allow(clippy::cast_sign_loss)] // bit patterns; `cb & 3` is a lane
    let a = f32::from_bits(lane_a.get((cb & 3) as usize).copied().unwrap_or(0) as u32);
    if a.is_nan() || a <= 0.0 {
        return None;
    }
    Some([c[0] * a, c[1] * a, c[2] * a, a])
}

/// An interpolated vertex value's opaque bytes, `None` for a ramp hole.
fn resolve(
    at: &dyn Fn(i32) -> [i32; 4],
    rec: i32,
    flags: i32,
    ramp_at: i32,
    v: [f32; 3],
) -> Option<[f32; 3]> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    if flags & 1 == 0 {
        return Some(v.map(|c| f32::from(byte(c))));
    }
    #[allow(clippy::cast_sign_loss)] // bit patterns
    let [_, _, _, inv_span] = at(rec + 3).map(|b| f32::from_bits(b as u32));
    #[allow(clippy::cast_sign_loss)]
    let rt0 = f32::from_bits(at(rec + 3)[2] as u32);
    let frac = if flags & 8 != 0 {
        0.0
    } else {
        ((v[0] - rt0) * inv_span).clamp(0.0, 1.0)
    };
    #[allow(clippy::cast_possible_truncation)] // in 0..=255
    let idx = (frac * 255.0).round() as i32;
    let s = at(rec + ramp_at + idx)[0];
    (s != 0).then(|| {
        let o = s.to_le_bytes();
        [f32::from(o[0]), f32::from(o[1]), f32::from(o[2])]
    })
}

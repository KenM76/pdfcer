//! Shadings on the graphics card. An axial or radial shading is evaluated
//! there per pixel from [`crate::shading::gpu_terms::Terms`], with the CPU
//! painter's operations in its order; any other shading's CPU painter runs
//! at packing time into a [`Recorder`], which keeps each pixel's source
//! terms and alpha for the shader to composite with
//! [`crate::shading::target::over_pixel`]'s arithmetic.
//!
//! A shading reads no backdrop, so recording its sources is exact: the
//! card's pixels equal the CPU replay's wherever its composite does.

use std::sync::Arc;

use tiny_skia::{FillRule, PathBuilder, PremultipliedColorU8, Rect, Transform};

use super::fill_edges::FillEdges;
use super::scene::{Frame, GpuRefusal};
use crate::display_list::shading_op::{self, ShadingOp};
use crate::shading::target::ShadeTarget;

/// An op texel's colour lane for a shading paint.
pub(crate) const SHADING_PAINT: i32 = 0x00FF_FFFC;

/// A shading as the shader draws it.
#[derive(Debug)]
pub(crate) enum ShadePaint {
    /// Per pixel of its rectangle, `[p_r, p_g, p_b, a]` as `f32` bit
    /// patterns, `a` zero where nothing was painted.
    Recorded {
        x0: i32,
        y0: i32,
        width: i32,
        texels: Vec<[i32; 4]>,
    },
    /// An [`eval`] record.
    Evaluated(Vec<[i32; 4]>),
}

/// An evaluated record's texels before its alpha table.
pub(crate) const EVAL_HEAD: i32 = 8;
/// An evaluated record's ramp, after the alpha table's 64 texels.
pub(crate) const EVAL_RAMP: i32 = EVAL_HEAD + 64;

struct Recorder {
    size: (u32, u32),
    rect: (i32, i32, i32, i32),
    texels: Vec<[i32; 4]>,
    painted: bool,
}

impl Recorder {
    fn slot(&mut self, idx: usize) -> Option<&mut [i32; 4]> {
        let w = self.size.0 as usize;
        let (x, y) = (i32::try_from(idx % w).ok()?, i32::try_from(idx / w).ok()?);
        let (l, t, r, b) = self.rect;
        if !(l..r).contains(&x) || !(t..b).contains(&y) {
            return None;
        }
        let at = usize::try_from((y - t) * (r - l) + (x - l)).ok()?;
        self.texels.get_mut(at)
    }

    fn put(&mut self, idx: usize, v: [f32; 4]) -> bool {
        let Some(slot) = self.slot(idx) else {
            return false;
        };
        #[allow(clippy::cast_possible_wrap)] // bit patterns
        let bits = v.map(|f| f.to_bits() as i32);
        *slot = bits;
        self.painted = true;
        true
    }
}

impl ShadeTarget for Recorder {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    // With `a` one the composite is `round(p)`, so an opaque colour's
    // bytes as terms reproduce it exactly.
    fn opaque(&mut self, idx: usize, c: PremultipliedColorU8) -> bool {
        let v = [c.red(), c.green(), c.blue()].map(f32::from);
        self.put(idx, [v[0], v[1], v[2], 1.0])
    }

    fn over(&mut self, idx: usize, p: [f32; 3], a: f32) -> bool {
        if a <= 0.0 {
            return false;
        }
        self.put(idx, [p[0], p[1], p[2], a.min(1.0)])
    }
}

/// A recorded shading and its rectangle's edges in each tile.
type Recorded = (Arc<ShadePaint>, Vec<Option<FillEdges>>);

/// `op` recorded over `frame`, and the edges of the rectangle it draws in
/// each tile; `None` when it paints nothing there.
///
/// # Errors
///
/// What [`Frame::edges`] refuses.
pub(crate) fn record(frame: &Frame<'_>, op: &ShadingOp) -> Result<Option<Recorded>, GpuRefusal> {
    let size = (frame.width, frame.height);
    let Some(rect) = shading_op::walked(op, size, (frame.x0, frame.y0)) else {
        return Ok(None);
    };
    let (l, t, r, b) = rect;
    let n = usize::try_from((r - l) * (b - t)).unwrap_or(0);
    let mut rec = Recorder {
        size,
        rect,
        texels: vec![[0; 4]; n],
        painted: false,
    };
    shading_op::paint_target(op, &mut rec, frame.clips, size, (frame.x0, frame.y0));
    if !rec.painted {
        return Ok(None);
    }
    #[allow(clippy::cast_precision_loss)] // raster pixel coordinates
    let Some(path) =
        Rect::from_ltrb(l as f32, t as f32, r as f32, b as f32).map(PathBuilder::from_rect)
    else {
        return Ok(None);
    };
    let edges = frame.edges(&path, FillRule::Winding, Transform::identity(), false)?;
    let paint = ShadePaint::Recorded {
        x0: l,
        y0: t,
        width: r - l,
        texels: rec.texels,
    };
    Ok(Some((Arc::new(paint), edges)))
}

/// The shading section from texel `base`: once per distinct paint, a
/// recorded paint's record `[pixels, x0, y0, width]` then its pixels, or an
/// evaluated paint's [`eval`] record.
/// Returns each paint's record texel (`-1` for none) and the section.
pub(crate) fn sections(
    paints: &[Option<&Arc<ShadePaint>>],
    base: usize,
) -> (Vec<i32>, Vec<[i32; 4]>) {
    let mut out = Vec::new();
    let mut placed: std::collections::HashMap<*const ShadePaint, i32> = Default::default();
    let mut rec_of = Vec::with_capacity(paints.len());
    for p in paints {
        let Some(p) = p else {
            rec_of.push(-1);
            continue;
        };
        let at = *placed.entry(Arc::as_ptr(p)).or_insert_with(|| {
            let rec = (base + out.len()) as i32;
            match &***p {
                ShadePaint::Recorded {
                    x0,
                    y0,
                    width,
                    texels,
                } => {
                    out.push([rec + 1, *x0, *y0, *width]);
                    out.extend_from_slice(texels);
                }
                ShadePaint::Evaluated(texels) => out.extend_from_slice(texels),
            }
            rec
        });
        rec_of.push(at);
    }
    (rec_of, out)
}

fn byte(v: f32) -> i32 {
    // `f32::round` then a clamp: half away from zero, and only
    // non-negative values reach here unclamped.
    let f = v.floor();
    let r = if v - f >= 0.5 { f + 1.0 } else { f };
    #[allow(clippy::cast_possible_truncation)] // clamped to a byte
    let r = r.clamp(0.0, 255.0) as i32;
    r
}

/// The shader's composite of record texel `s` over `d`, as
/// [`crate::shading::target::over_pixel`]; `d` unchanged where unpainted.
pub(crate) fn over(s: [i32; 4], d: [i32; 4]) -> [i32; 4] {
    #[allow(clippy::cast_sign_loss)] // bit patterns
    let f = s.map(|v| f32::from_bits(v as u32));
    let a = f[3];
    if a.is_nan() || a <= 0.0 {
        return d;
    }
    let inv = 1.0 - a;
    #[allow(clippy::cast_precision_loss)] // bytes
    let df = d.map(|v| v as f32);
    let oa = byte(a * 255.0 + df[3] * inv);
    let ch = |p: f32, d: f32| byte(p + d * inv).min(oa);
    [ch(f[0], df[0]), ch(f[1], df[1]), ch(f[2], df[2]), oa]
}

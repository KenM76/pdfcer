//! Image paints on the graphics card: `tiny-skia` 0.11's pattern shader
//! (seed, inverse transform, nearest or bilinear gather under `Pad`) and
//! its source-over blend, as the high-precision pipeline computes them,
//! one `f32` rounding at a time. [`super::glsl`] repeats each step with
//! the products kept from fusing.

use std::sync::Arc;

use tiny_skia::{FilterQuality, IntRect, Pixmap, Transform};

use super::scene::{Frame, GpuRefusal};

/// `load_8888`'s factor, `1 / 255` as `f32` (bits `0x3b80_8081`).
pub(crate) const FACTOR: f32 = 1.0 / 255.0;

/// The colour an image paint's op texel carries: channels above alpha, a
/// premultiplied colour no solid paint has.
pub(crate) const IMAGE_PAINT: i32 = 0x00FF_FFFF;

/// Sample coordinates past which `floor` through a 32-bit truncation (the
/// CPU's) and the shader's `floor` could part.
const MAX_COORD: f64 = 16_777_216.0;

/// An image paint's sampler in one `DrawTiler` tile.
#[derive(Clone)]
pub(crate) struct ImagePaint {
    pub texels: Arc<Pixmap>,
    pub bilinear: bool,
    /// Tile pixels to image pixels: `[sx, ky, kx, sy, tx, ty]`.
    pub inverse: [f32; 6],
}

/// The sampler of an image painted under `ts` in each of
/// [`Frame::draw_tiles`]; `None` where the pattern cannot be inverted and
/// so draws nothing. As `fill_path` does, the shader is moved to each tile
/// and back in turn, so a later tile sees every earlier round trip.
///
/// # Errors
///
/// [`GpuRefusal::Image`] for a bicubic sampler, or an inverse that is not
/// finite or has a coefficient below `1e-30`.
pub(crate) fn per_tile(
    frame: &Frame<'_>,
    texels: &Arc<Pixmap>,
    quality: FilterQuality,
    image_to_user: Transform,
    ts: Transform,
) -> Result<Vec<Option<ImagePaint>>, GpuRefusal> {
    let tiles = frame.draw_tiles();
    // `fill_path_fit`'s `post_translate(0, -0)` is the identity, which
    // `post_concat` returns unchanged.
    let mut t = image_to_user.post_concat(ts);
    let one = |t: Transform| sampler(texels, quality, t);
    if let [_] = tiles.as_slice() {
        return Ok(vec![one(t)?]);
    }
    let mut out = Vec::with_capacity(tiles.len());
    for &(x, y, _, _) in &tiles {
        let (dx, dy) = (x as f32, y as f32);
        t = t.post_concat(Transform::from_translate(-dx, -dy));
        out.push(one(t)?);
        t = t.post_concat(Transform::from_translate(dx, dy));
    }
    Ok(out)
}

/// `Pattern::push_stages`' choices for the shader transform `t`.
fn sampler(
    texels: &Arc<Pixmap>,
    quality: FilterQuality,
    t: Transform,
) -> Result<Option<ImagePaint>, GpuRefusal> {
    let Some(inv) = t.invert() else {
        return Ok(None);
    };
    let coefficients = [inv.sx, inv.ky, inv.kx, inv.sy, inv.tx, inv.ty];
    // A coefficient this small could leave subnormal products, which a
    // graphics card may flush to zero.
    let tiny = |v: f32| v != 0.0 && v.abs() < 1e-30;
    if !inv.is_finite() || coefficients.into_iter().any(tiny) {
        return Err(GpuRefusal::Image);
    }
    let plain = inv.is_identity() || inv.is_translate();
    let bilinear = match quality {
        _ if plain => false,
        FilterQuality::Nearest => false,
        FilterQuality::Bilinear => true,
        FilterQuality::Bicubic => return Err(GpuRefusal::Image),
    };
    Ok(Some(ImagePaint {
        texels: Arc::clone(texels),
        bilinear,
        inverse: coefficients,
    }))
}

impl ImagePaint {
    /// Whether every pixel centre of `area`, tile pixels, samples within
    /// [`MAX_COORD`]; a nearest gather clamps first, so it always does.
    pub(crate) fn in_range(&self, area: IntRect) -> bool {
        if !self.bilinear {
            return true;
        }
        let [sx, ky, kx, sy, tx, ty] = self.inverse.map(f64::from);
        let xs = [f64::from(area.x()), f64::from(area.right())];
        let ys = [f64::from(area.y()), f64::from(area.bottom())];
        xs.iter().all(|x| {
            ys.iter().all(|y| {
                (x * sx + y * kx + tx).abs() <= MAX_COORD
                    && (x * ky + y * sy + ty).abs() <= MAX_COORD
            })
        })
    }
}

/// `floor` as `f32x8` computes it: truncate, step down where that rose.
fn floor(v: f32) -> f32 {
    // In range: `in_range` bounds every coordinate.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let t = (v as i32) as f32;
    if t > v { t - 1.0 } else { t }
}

/// `gather_ix`: `(x, y)` clamped to the image as SSE `max`/`min` clamp,
/// truncated, as a pixel index.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)] // clamped to the image
fn gather(x: f32, y: f32, (w, h): (i32, i32)) -> i32 {
    let limit = |n: i32| f32::from_bits((n as f32).to_bits() - 1);
    let clamp = |v: f32, l: f32| {
        let v = if v > 0.0 { v } else { 0.0 };
        if v < l { v } else { l }
    };
    (clamp(y, limit(h)) as i32) * w + clamp(x, limit(w)) as i32
}

/// `load_8888` of one packed `RGBA8` pixel.
fn load(p: i32) -> [f32; 4] {
    p.to_le_bytes().map(|b| f32::from(b) * FACTOR)
}

/// The pattern's premultiplied colour at tile pixel (`lx`, `ly`), image
/// pixel `i` being `fetch(i)` of a `w` × `h` image.
pub(crate) fn sample(
    fetch: impl Fn(i32) -> i32,
    size: (i32, i32),
    bilinear: bool,
    [sx, ky, kx, sy, tx, ty]: [f32; 6],
    (lx, ly): (i32, i32),
) -> [f32; 4] {
    #[allow(clippy::cast_precision_loss)] // a pixel index
    let (r, g) = (lx as f32 + 0.5, ly as f32 + 0.5);
    let (x, y) = (r * sx + (g * kx + tx), r * ky + (g * sy + ty));
    if !bilinear {
        return load(fetch(gather(x, y, size)));
    }
    let (fx, fy) = (x + 0.5 - floor(x + 0.5), y + 0.5 - floor(y + 0.5));
    let mut acc = [0.0f32; 4];
    let mut yy = y + -0.5;
    for wy in [1.0 - fy, fy] {
        let mut xx = x + -0.5;
        for wx in [1.0 - fx, fx] {
            let s = load(fetch(gather(xx, yy, size)));
            let w = wx * wy;
            for (a, v) in acc.iter_mut().zip(s) {
                *a += w * v;
            }
            xx += 1.0;
        }
        yy += 1.0;
    }
    acc
}

/// `store_8888`'s rounding of one channel.
#[allow(clippy::cast_possible_truncation)] // in 0..=255
fn unnorm(v: f32) -> i32 {
    let v = if v > 0.0 { v } else { 0.0 };
    let v = if v < 1.0 { v } else { 1.0 };
    (v * 255.0).round_ties_even() as i32
}

/// `s` drawn source-over onto the bytes `d`, scaled by `mask` (a clip's
/// [`quotients`] entry) if clipped and by coverage `cov`, non-zero.
pub(crate) fn over(d: [i32; 4], mut s: [f32; 4], mask: Option<f32>, cov: i32) -> [i32; 4] {
    #[allow(clippy::cast_precision_loss)] // bytes
    let byte = |v: i32| v as f32;
    if let Some(c) = mask {
        s = s.map(|v| v * c);
    }
    if cov < 255 {
        let c = byte(cov) * FACTOR;
        s = s.map(|v| v * c);
    }
    let keep = 1.0 - s[3];
    let mut out = [0; 4];
    for ((o, dv), sv) in out.iter_mut().zip(d).zip(s) {
        *o = unnorm(byte(dv) * FACTOR * keep + sv);
    }
    out
}

/// `k / 255` as `f32` for every byte `k`, `mask_u8`'s division, as bits.
pub(crate) fn quotients() -> [i32; 256] {
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)] // 0..256
    std::array::from_fn(|k| ((k as f32) / 255.0).to_bits() as i32)
}

/// Texels per image record.
pub(crate) const RECORD: usize = 3;

/// The image section at texel `base` for paints' samplers `samplers`: per
/// image paint a record (`[pixels, width, height, bilinear]`, then its
/// inverse's bits `[sx, ky, kx, sy]`, `[tx, ty, 0, 0]`), the
/// [`quotients`] table (64 texels), then each distinct image's
/// premultiplied pixels, one `RGBA8` value a lane. Returns each paint's
/// record texel (-1 for a solid), the texels, and the table's texel (0
/// with no image).
pub(crate) fn sections(
    samplers: &[Option<&ImagePaint>],
    base: usize,
) -> (Vec<i32>, Vec<[i32; 4]>, i32) {
    let count = samplers.iter().flatten().count();
    if count == 0 {
        return (vec![-1; samplers.len()], Vec::new(), 0);
    }
    let div_base = base + RECORD * count;
    let mut pixels: Vec<[i32; 4]> = Vec::new();
    let mut placed: std::collections::HashMap<*const Pixmap, i32> = Default::default();
    let mut rec_of = Vec::with_capacity(samplers.len());
    let mut out = Vec::with_capacity(RECORD * count + 64);
    for s in samplers {
        let Some(img) = s else {
            rec_of.push(-1);
            continue;
        };
        rec_of.push((base + out.len()) as i32);
        let at = *placed.entry(Arc::as_ptr(&img.texels)).or_insert_with(|| {
            let at = (div_base + 64 + pixels.len()) as i32;
            pixels.extend(img.texels.data().chunks(16).map(|c| {
                std::array::from_fn(|k| {
                    let px = c.get(k * 4..k * 4 + 4).unwrap_or_default();
                    i32::from_le_bytes(std::array::from_fn(|j| px.get(j).copied().unwrap_or(0)))
                })
            }));
            at
        });
        #[allow(clippy::cast_possible_wrap)] // bit patterns; sizes below `MAX_PIXMAP_EDGE`
        let bits = img.inverse.map(|v| v.to_bits() as i32);
        #[allow(clippy::cast_possible_wrap)]
        let (w, h) = (img.texels.width() as i32, img.texels.height() as i32);
        out.push([at, w, h, i32::from(img.bilinear)]);
        out.push([bits[0], bits[1], bits[2], bits[3]]);
        out.push([bits[4], bits[5], 0, 0]);
    }
    out.extend(
        quotients()
            .chunks(4)
            .map(|c| std::array::from_fn(|k| c.get(k).copied().unwrap_or(0))),
    );
    out.extend(pixels);
    (rec_of, out, div_base as i32)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_zero_mask_leaves_every_byte() {
        for b in 0..=255 {
            assert_eq!(super::over([b; 4], [0.0; 4], Some(1.0), 255), [b; 4]);
        }
    }
}

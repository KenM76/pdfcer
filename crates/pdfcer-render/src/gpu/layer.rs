//! Transparency groups on the graphics card. An `Op::Layer` becomes a
//! begin marker, its members, and an end marker, each listed in every
//! raster tile a member reaches (a group leaves pixels no member paints
//! untouched). Begin saves the pixel and starts from transparent; end
//! composites the group's pixel onto the saved one as `draw_pixmap` does:
//! the nearest gather, the opacity scale and the blend in `tiny-skia`
//! 0.11's high-precision pipeline, one `f32` rounding at a time.

use super::image::{self, FACTOR};
use super::scene::GpuRefusal;
use crate::canvas::LayerPaint;

/// The colour lane of a begin marker's op texel (a premultiplied colour no
/// paint has).
pub(crate) const LAYER_BEGIN: i32 = 0x00FF_FFFE;
/// The colour lane of an end marker's op texel, which carries the group's
/// opacity bits in its header lane and blend code in its clip lane.
pub(crate) const LAYER_END: i32 = 0x00FF_FFFD;
/// Group nesting the shader keeps saved pixels for.
pub(crate) const MAX_LAYER_DEPTH: usize = 8;

/// The op texel of `paint`'s end marker.
///
/// # Errors
///
/// [`GpuRefusal::Transparency`] for a non-separable or high-precision-only
/// blend mode, or an opacity that is not a number.
pub(crate) fn end_marker(paint: &LayerPaint) -> Result<[i32; 4], GpuRefusal> {
    if paint.nonseparable.is_some() || paint.opacity.is_nan() {
        return Err(GpuRefusal::Transparency);
    }
    let code = super::blend::code(paint.blend)?;
    #[allow(clippy::cast_possible_wrap)] // a bit pattern
    let bits = paint.opacity.clamp(0.0, 1.0).to_bits() as i32;
    Ok([bits, code, LAYER_END, 0])
}

fn inv(v: f32) -> f32 {
    1.0 - v
}

/// `code`'s high-precision blend of channel `s` over `d`, alphas `sa`, `da`.
fn channel(code: i32, (s, sa): (f32, f32), (d, da): (f32, f32)) -> f32 {
    let light = |c: bool| {
        let v = if c {
            let p = s * d;
            p + p
        } else {
            let p = (da - d) * (sa - s);
            sa * da - (p + p)
        };
        s * inv(da) + d * inv(sa) + v
    };
    match code {
        1 => s * inv(da) + d * inv(sa) + s * d,
        2 => s + d - s * d,
        3 => light(d + d <= da),
        4 => s + d - (s * da).max(d * sa),
        5 => s + d - (s * da).min(d * sa),
        6 => light(s + s <= sa),
        7 => {
            let m = (s * da).min(d * sa);
            s + d - (m + m)
        }
        _ => {
            let p = s * d;
            s + d - (p + p)
        }
    }
}

/// The group pixel `inner` (bytes) composited onto `d` (bytes) with
/// opacity `opacity` and blend `code`.
pub(crate) fn composite(d: [i32; 4], inner: [i32; 4], opacity: f32, code: i32) -> [i32; 4] {
    #[allow(clippy::cast_precision_loss)] // bytes
    let load = |v: i32| v as f32 * FACTOR;
    let s = inner.map(|v| load(v) * opacity);
    if code == 0 {
        return image::over(d, s, None, 255);
    }
    let dv = d.map(load);
    let (sa, da) = (s[3], dv[3]);
    let mut r = [0.0f32; 4];
    for ((o, sv), d) in r.iter_mut().zip(s).zip(dv).take(3) {
        *o = channel(code, (sv, sa), (d, da));
    }
    r[3] = match code {
        1 | 2 => channel(code, (sa, sa), (da, da)),
        _ => da * inv(sa) + sa,
    };
    r.map(image::unnorm)
}

/// The pixels saved under the groups open at one pixel.
#[derive(Default)]
pub(crate) struct Stack {
    saved: [[i32; 4]; MAX_LAYER_DEPTH],
    depth: usize,
}

impl Stack {
    /// Applies op texel `[h, clip, color]` to pixel `d` if it is a marker;
    /// whether it was.
    pub(crate) fn apply(&mut self, d: &mut [i32; 4], [h, clip, color]: [i32; 3]) -> bool {
        match color {
            LAYER_BEGIN => {
                if let Some(s) = self.saved.get_mut(self.depth) {
                    *s = *d;
                }
                self.depth += 1;
                *d = [0; 4];
            }
            LAYER_END => {
                self.depth = self.depth.saturating_sub(1);
                let under = self.saved.get(self.depth).copied().unwrap_or_default();
                #[allow(clippy::cast_sign_loss)] // a bit pattern
                let opacity = f32::from_bits(h as u32);
                *d = composite(under, *d, opacity, clip);
            }
            _ => return false,
        }
        true
    }
}

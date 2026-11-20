//! Where a shading painter's pixels go: a pixmap, composited there, or a
//! record of each pixel's source for the graphics card to composite
//! (`crate::gpu::shade`).
//!
//! Every painter hands a pixel over as source terms `p` and alpha `a`, and
//! [`over_pixel`] is the one composite both destinations use, so the card's
//! copy of it has a single formula to match.

use tiny_skia::{Pixmap, PremultipliedColorU8};

/// A shading painter's destination; pixel `idx` is row-major over
/// [`Self::size`]. A painter writes each pixel at most once.
pub(crate) trait ShadeTarget {
    /// Width and height, pixels.
    fn size(&self) -> (u32, u32);
    /// Replace pixel `idx` with opaque `c`. Whether it wrote.
    fn opaque(&mut self, idx: usize, c: PremultipliedColorU8) -> bool;
    /// Composite source terms `p` at alpha `a` over pixel `idx`
    /// ([`over_pixel`]). Whether it wrote.
    fn over(&mut self, idx: usize, p: [f32; 3], a: f32) -> bool;
}

/// Source-over in premultiplied bytes: each channel `p + d·(1 − a)`, alpha
/// `a·255 + d_a·(1 − a)`, every product and sum a separately rounded `f32`,
/// then rounded half away from zero. `p` is a channel's `src·a·255` for a
/// unit source, `src·a` for a byte one.
///
/// Each channel is clamped to the alpha: channels and alpha round
/// independently, a channel can land one above it, and `from_rgba`
/// rejects that.
pub(crate) fn over_pixel(
    dst: PremultipliedColorU8,
    p: [f32; 3],
    a: f32,
) -> Option<PremultipliedColorU8> {
    let a = a.min(1.0);
    let inv = 1.0 - a;
    let out_a = byte(a * 255.0 + f32::from(dst.alpha()) * inv);
    let mix = |p: f32, d: u8| byte(p + f32::from(d) * inv).min(out_a);
    PremultipliedColorU8::from_rgba(
        mix(p[0], dst.red()),
        mix(p[1], dst.green()),
        mix(p[2], dst.blue()),
        out_a,
    )
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to a byte
fn byte(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

impl ShadeTarget for Pixmap {
    fn size(&self) -> (u32, u32) {
        (self.width(), self.height())
    }

    fn opaque(&mut self, idx: usize, c: PremultipliedColorU8) -> bool {
        self.pixels_mut().get_mut(idx).map(|s| *s = c).is_some()
    }

    fn over(&mut self, idx: usize, p: [f32; 3], a: f32) -> bool {
        if a <= 0.0 {
            return false;
        }
        let Some(slot) = self.pixels_mut().get_mut(idx) else {
            return false;
        };
        let Some(c) = over_pixel(*slot, p, a) else {
            return false;
        };
        *slot = c;
        true
    }
}

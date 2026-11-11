//! `EMR_ALPHABLEND`: every bitmap the EMF writer embeds.

use tiny_skia::Pixmap;

use super::{Writer, i32le, rectl, u32le};

/// A pixel box: `(x0, y0, width, height)`.
pub(super) type PxBox = (u32, u32, u32, u32);

/// A destination in logical units: `(x, y, width, height)`.
pub(super) type LuBox = (i32, i32, i32, i32);

impl Writer<'_> {
    /// Crop `scratch` to its painted box and write it as `EMR_ALPHABLEND`
    /// (premultiplied BGRA, top-down, `AC_SRC_OVER | AC_SRC_ALPHA`).
    pub(super) fn alpha_blend(&mut self, scratch: &Pixmap) {
        let w = scratch.width();
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for (i, px) in scratch.pixels().iter().enumerate() {
            if px.alpha() == 0 {
                continue;
            }
            #[allow(clippy::cast_possible_truncation)]
            let (x, y) = ((i as u32) % w, (i as u32) / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        if x0 == u32::MAX {
            return;
        }
        let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
        #[allow(clippy::cast_precision_loss)]
        let dest = (
            self.lu(x0 as f32),
            self.lu(y0 as f32),
            self.lu(bw as f32).max(1),
            self.lu(bh as f32).max(1),
        );
        self.bitmap(scratch, (x0, y0, bw, bh), dest);
    }

    /// `SETSTRETCHBLTMODE HALFTONE` and `SETBRUSHORGEX`, once, before the
    /// first bitmap.
    pub(super) fn raster_prologue(&mut self) {
        if !self.raster_prologue_done {
            self.record(0x15, &u32le(4)); // SETSTRETCHBLTMODE HALFTONE
            self.record(0x0D, &[i32le(0), i32le(0)].concat()); // SETBRUSHORGEX
            self.raster_prologue_done = true;
        }
    }

    /// Write `src`'s pixels in `area` as one `EMR_ALPHABLEND` onto `dest`.
    pub(super) fn bitmap(&mut self, src: &Pixmap, area: PxBox, dest: LuBox) {
        let (x0, y0, bw, bh) = area;
        let (dx, dy, dw, dh) = dest;
        self.raster_prologue();
        self.outcome.rasters_embedded += 1;

        let w = src.width();
        let mut bits = Vec::with_capacity((bw * bh * 4) as usize);
        for row in 0..bh {
            let s0 = ((y0 + row) * w + x0) as usize;
            for px in src.pixels().get(s0..s0 + bw as usize).unwrap_or_default() {
                bits.extend_from_slice(&[px.blue(), px.green(), px.red(), px.alpha()]);
            }
        }
        let mut body = Vec::with_capacity(100 + 40 + bits.len());
        body.extend_from_slice(&rectl(dx, dy, dx + dw - 1, dy + dh - 1)); // Bounds
        body.extend_from_slice(&i32le(dx));
        body.extend_from_slice(&i32le(dy));
        body.extend_from_slice(&i32le(dw));
        body.extend_from_slice(&i32le(dh));
        body.extend_from_slice(&[0x00, 0x00, 0xFF, 0x01]); // BLENDFUNCTION: AC_SRC_OVER, 0, 255, AC_SRC_ALPHA
        body.extend_from_slice(&i32le(0)); // xSrc
        body.extend_from_slice(&i32le(0)); // ySrc
        for f in [1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0] {
            body.extend_from_slice(&f.to_le_bytes()); // XformSrc identity
        }
        body.extend_from_slice(&u32le(0)); // BkColorSrc
        body.extend_from_slice(&u32le(0)); // UsageSrc DIB_RGB_COLORS
        body.extend_from_slice(&u32le(108)); // offBmiSrc
        body.extend_from_slice(&u32le(40)); // cbBmiSrc
        body.extend_from_slice(&u32le(148)); // offBitsSrc
        #[allow(clippy::cast_possible_truncation)]
        body.extend_from_slice(&u32le(bits.len() as u32)); // cbBitsSrc
        #[allow(clippy::cast_possible_wrap)]
        body.extend_from_slice(&i32le(bw as i32)); // cxSrc
        #[allow(clippy::cast_possible_wrap)]
        body.extend_from_slice(&i32le(bh as i32)); // cySrc
        debug_assert_eq!(body.len(), 100);
        body.extend_from_slice(&bitmap_info_header(bw, bh));
        body.extend_from_slice(&bits);
        self.record(0x72, &body);
    }
}

/// `BitmapInfoHeader`: 32 bpp `BI_RGB`, TOP-DOWN (negative height).
fn bitmap_info_header(bw: u32, bh: u32) -> Vec<u8> {
    let mut h = Vec::with_capacity(40);
    h.extend_from_slice(&u32le(40));
    #[allow(clippy::cast_possible_wrap)]
    h.extend_from_slice(&i32le(bw as i32));
    #[allow(clippy::cast_possible_wrap)]
    h.extend_from_slice(&i32le(-(bh as i32)));
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&32u16.to_le_bytes());
    h.extend_from_slice(&u32le(0)); // BI_RGB
    h.extend_from_slice(&u32le(0)); // ImageSize
    h.extend_from_slice(&i32le(0));
    h.extend_from_slice(&i32le(0));
    h.extend_from_slice(&u32le(0));
    h.extend_from_slice(&u32le(0));
    h
}

//! An `image` value: its inline base64 content, or the `href` it names in
//! [`XfaImages`], drawn into its region per `aspect` (XFA 3.3 image p.750;
//! slice `adobe_ext__xfa_appearance_drawing.md` §5.4). The format is sniffed
//! from the bytes; `contentType` is not trusted.
//!
//! Drawn as an inline image (ISO 32000-1 §8.9.7) because the painted page
//! owns no streams. An inline image has no `/SMask` or `/Mask`, so an image
//! with transparency is composited onto white first and noted.

use std::borrow::Cow;
use std::fmt::Write as _;

use crate::filters::flate;
use crate::image_import::jpeg_encode::source_samples;
use crate::image_import::{self, ImportColorSpace, ImportFilter, ImportedImage, flate_encode};
use crate::xfa::images::decode_base64;

use super::{
    ImageOverflow, ImagePlacement, LayoutItem, LayoutRect, PaintNoteKind, Painter, XfaNode,
};

/// `holder`'s `value/image`, when its value is an image.
pub(super) fn value_image(holder: &XfaNode) -> Option<&XfaNode> {
    holder.child("value")?.child("image")
}

/// The drawn size of an image of natural size `(nw, nh)` in a `w` by `h`
/// region under `aspect` (p.750; `fit` is the default).
fn fitted(aspect: Option<&str>, (nw, nh): (f64, f64), w: f64, h: f64) -> (f64, f64) {
    match aspect.map(str::trim) {
        Some("none") => (w, h),
        Some("actual") => (nw, nh),
        Some("width") => (w, nh * w / nw),
        Some("height") => (nw * h / nh, h),
        _ => {
            let s = (w / nw).min(h / nh);
            (nw * s, nh * s)
        }
    }
}

/// The horizontal and vertical share of the slack an image sits after,
/// from `para`'s `hAlign`/`vAlign`.
fn para_factors(para: Option<&XfaNode>) -> (f64, f64) {
    let h = match para.and_then(|p| p.attr("hAlign")).map(str::trim) {
        Some("center") => 0.5,
        Some("right") => 1.0,
        _ => 0.0,
    };
    let v = match para.and_then(|p| p.attr("vAlign")).map(str::trim) {
        Some("middle") => 0.5,
        Some("bottom") => 1.0,
        _ => 0.0,
    };
    (h, v)
}

/// Image samples ready for `BI`: their filter and colour space.
struct Raster {
    width: u32,
    height: u32,
    bpc: u8,
    space: ImportColorSpace,
    filter: ImportFilter,
    data: Vec<u8>,
}

impl Painter<'_> {
    /// Draw `node`'s image in `r`, placed per `para` when the options say
    /// so and clipped to `r` unless they say not; with no area in `r`, at
    /// its natural size.
    pub(super) fn image(
        &mut self,
        item: &LayoutItem,
        node: &XfaNode,
        para: Option<&XfaNode>,
        r: LayoutRect,
    ) {
        let Some(bytes) = self.image_bytes(node) else {
            if node.attr("href").is_some_and(|h| !h.is_empty()) || !node.text.trim().is_empty() {
                self.note(item, PaintNoteKind::Image);
            }
            return;
        };
        let Some(img) = self.import(&bytes) else {
            self.note(item, PaintNoteKind::Image);
            return;
        };
        let natural = img.natural_size_pt();
        let sized = r.w > 0.0 && r.h > 0.0;
        let (dw, dh) = if sized {
            fitted(node.attr("aspect"), natural, r.w, r.h)
        } else {
            natural
        };
        let (fx, fy) = match self.opts.image_placement {
            ImagePlacement::Para => para_factors(para),
            _ => (0.0, 0.0),
        };
        let (x, y) = if sized {
            (r.x + (r.w - dw) * fx, r.y + (r.h - dh) * fy)
        } else {
            (r.x, r.y)
        };
        let orientation = img.orientation.unit_square_matrix();
        let (raster, flattened) = match raster(img) {
            Some(v) => v,
            None => {
                self.note(item, PaintNoteKind::Image);
                return;
            }
        };
        if flattened {
            self.note(item, PaintNoteKind::ImageTransparency);
        }
        self.b.save_state();
        if sized && self.opts.image_overflow == ImageOverflow::Crop {
            self.b.rect(r.x, self.height - r.y - r.h, r.w, r.h);
            self.b.clip_nonzero();
            self.b.paint(crate::writer::content::Paint::NoPaint);
        }
        self.b
            .concat_matrix(dw, 0.0, 0.0, dh, x, self.height - y - dh);
        if orientation != [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] {
            let [a, b, c, d, e, f] = orientation;
            self.b.concat_matrix(a, b, c, d, e, f);
        }
        self.b.append_raw(&inline_image(&raster));
        self.b.restore_state();
    }

    /// The natural size of `node`'s image in points, when it resolves.
    pub(super) fn image_extent(&self, node: &XfaNode) -> Option<(f64, f64)> {
        let bytes = self.image_bytes(node)?;
        self.import(&bytes).map(|i| i.natural_size_pt())
    }

    /// `bytes` decoded, with the assumed resolution when they record none.
    fn import(&self, bytes: &[u8]) -> Option<ImportedImage> {
        let mut img = image_import::import(bytes).ok()?;
        if img.dpi.is_none() {
            let dpi = f64::from(if self.opts.image_dpi == 0 {
                72
            } else {
                self.opts.image_dpi
            });
            img.dpi = Some((dpi, dpi));
        }
        Some(img)
    }

    /// The `href` target held in the file, else the inline content (p.750:
    /// `href` takes priority).
    fn image_bytes<'n>(&'n self, node: &'n XfaNode) -> Option<Cow<'n, [u8]>> {
        if let Some(found) = node.attr("href").and_then(|h| self.images.get(h)) {
            return Some(Cow::Borrowed(found));
        }
        let text = node.text.trim();
        if text.is_empty() {
            return None;
        }
        decode_base64(text.as_bytes()).map(Cow::Owned)
    }
}

/// `img` as `BI` samples, and whether its transparency was flattened onto
/// white (an inline image carries no mask, §8.9.7 Table 93).
fn raster(img: ImportedImage) -> Option<(Raster, bool)> {
    if img.soft_mask.is_none() && img.color_key_mask.is_none() {
        let raster = Raster {
            width: img.width,
            height: img.height,
            bpc: img.bits_per_component,
            space: img.color_space,
            filter: img.filter,
            data: img.data,
        };
        return Some((raster, false));
    }
    let s = source_samples(&img).ok()?;
    let n = usize::from(s.components);
    let pixels = s.width as usize * s.height as usize;
    let alpha = alpha(&img, &s.samples, n, pixels)?;
    let cmyk = s.space == ImportColorSpace::DeviceCmyk;
    let mut out = s.samples;
    for (px, &a) in out.chunks_exact_mut(n).zip(&alpha) {
        for c in px {
            let v = u32::from(*c) * u32::from(a);
            let bg = if cmyk { 0 } else { 255 * (255 - u32::from(a)) };
            *c = u8::try_from((v + bg + 127) / 255).unwrap_or(u8::MAX);
        }
    }
    let raster = Raster {
        width: s.width,
        height: s.height,
        bpc: 8,
        space: s.space,
        filter: ImportFilter::Flate,
        data: flate_encode(&out).ok()?,
    };
    Some((raster, true))
}

/// One 8-bit opacity per pixel: the soft mask's, else 0 where an 8-bit
/// direct-colour sample matches the colour key and 255 elsewhere (any other
/// key is left opaque).
fn alpha(img: &ImportedImage, samples: &[u8], n: usize, pixels: usize) -> Option<Vec<u8>> {
    if let Some(mask) = &img.soft_mask {
        let raw = flate::decode(&mask.data, None).ok()?;
        let step = if mask.bits_per_component == 16 { 2 } else { 1 };
        let a: Vec<u8> = raw.iter().step_by(step).copied().take(pixels).collect();
        return (a.len() == pixels).then_some(a);
    }
    let key = img.color_key_mask.as_deref().unwrap_or_default();
    let keyed = img.bits_per_component == 8
        && !matches!(img.color_space, ImportColorSpace::Indexed { .. })
        && key.len() == 2 * n;
    Some(
        samples
            .chunks_exact(n)
            .map(|px| {
                let hit = keyed
                    && px.iter().enumerate().all(|(i, &v)| {
                        let v = i64::from(v);
                        key.get(2 * i).is_some_and(|lo| v >= *lo)
                            && key.get(2 * i + 1).is_some_and(|hi| v <= *hi)
                    });
                if hit { 0 } else { 255 }
            })
            .collect(),
    )
}

/// The `BI … ID … EI` operator for `r`, its data ASCII-hex encoded so no
/// byte run inside it can read as `EI` (§8.9.7).
fn inline_image(r: &Raster) -> Vec<u8> {
    let mut d = format!("\nBI /W {} /H {} /BPC {} /CS ", r.width, r.height, r.bpc);
    match &r.space {
        ImportColorSpace::DeviceRgb => d.push_str("/RGB"),
        ImportColorSpace::DeviceCmyk => d.push_str("/CMYK"),
        ImportColorSpace::Indexed { hival, lookup } => {
            let _ = write!(d, "[/I /RGB {hival} <");
            lookup.iter().for_each(|b| {
                let _ = write!(d, "{b:02X}");
            });
            d.push_str(">]");
        }
        _ => d.push_str("/G"),
    }
    match r.filter {
        ImportFilter::DctDecode => d.push_str(" /F [/AHx /DCT]"),
        ImportFilter::FlatePngPredictor {
            colors,
            bits_per_component,
            columns,
        } => {
            let _ = write!(
                d,
                " /F [/AHx /Fl] /DP [null << /Predictor 15 /Colors {colors} \
                 /BitsPerComponent {bits_per_component} /Columns {columns} >>]"
            );
        }
        _ => d.push_str(" /F [/AHx /Fl]"),
    }
    d.push_str(" ID ");
    let mut out = d.into_bytes();
    out.reserve(r.data.len() * 2 + 6);
    let hex = |n: u8| if n < 10 { b'0' + n } else { b'A' + n - 10 };
    for &b in &r.data {
        out.push(hex(b >> 4));
        out.push(hex(b & 0xF));
    }
    out.extend_from_slice(b">\nEI\n");
    out
}

#[cfg(test)]
mod tests;

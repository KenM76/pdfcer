//! Scan-image side of the editable-text OCR output ([`super::editable`]):
//! where each image is drawn, measuring a word's background and ink, erasing
//! its glyph pixels, and re-encoding the image.
//!
//! Only images drawn by the page's own content (not inside a form XObject)
//! are considered, with the CTM tracked through `q`/`Q`/`cm` (ISO 32000-1
//! §8.4.4). An image's unit square maps to its samples with sample row 0 at
//! unit-square `v = 1` (§8.9.5).

use crate::content::{ContentStream, ContentTokenKind};
use crate::graph::ObjectGraph;
use crate::image_codec::Codec;
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::{Page, Rect};
use crate::redact_image::Decoded;
use crate::vector::geometry::{Matrix, Point};
use crate::view::DocumentView;

use super::editable::{EditableImageCodec, SuspectReason};

/// One `Do` of an image XObject in the page's own content.
#[derive(Debug, Clone)]
pub(super) struct Placement {
    /// The `/XObject` resource name the `Do` used.
    pub(super) name: Vec<u8>,
    pub(super) id: ObjId,
    pub(super) ctm: Matrix,
}

/// Every image drawn by the page's own content stream(s), in paint order.
pub(super) fn placements(view: &DocumentView<'_>, page: &Page) -> Vec<Placement> {
    let Ok(stream) = ContentStream::from_page(view, page) else {
        return Vec::new();
    };
    let xobjects = page
        .resources
        .get(b"XObject")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
        .cloned()
        .unwrap_or_default();
    let mut ctm = Matrix::IDENTITY;
    let mut stack = Vec::new();
    let mut out = Vec::new();
    for op in stream.operations() {
        let nums = || -> Vec<f64> {
            op.operands
                .iter()
                .filter_map(|t| match &t.kind {
                    ContentTokenKind::Operand(o) => o.as_number(),
                    _ => None,
                })
                .collect()
        };
        match op.operator_name(&stream.buf) {
            Some(b"q") => stack.push(ctm),
            Some(b"Q") => ctm = stack.pop().unwrap_or(Matrix::IDENTITY),
            Some(b"cm") => {
                if let [a, b, c, d, e, f] = nums().as_slice() {
                    ctm = Matrix::new(*a, *b, *c, *d, *e, *f).post_concat(ctm);
                }
            }
            Some(b"Do") => {
                let name = match op.operands.first().map(|t| &t.kind) {
                    Some(ContentTokenKind::Operand(Object::Name(n))) => n.as_bytes().to_vec(),
                    _ => continue,
                };
                let Some(id) = xobjects.get(&name[..]).and_then(Object::as_reference) else {
                    continue;
                };
                if let Object::Stream(s) = view.resolved(id)
                    && matches!(s.dict.get(b"Subtype"), Some(Object::Name(n)) if n.as_bytes() == b"Image")
                {
                    out.push(Placement { name, id, ctm });
                }
            }
            _ => {}
        }
    }
    out
}

/// A pixel rectangle, half-open: `x0..x1`, `y0..y1`, row 0 at the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PixRect {
    pub(super) x0: u32,
    pub(super) y0: u32,
    pub(super) x1: u32,
    pub(super) y1: u32,
}

impl PixRect {
    fn height(self) -> u32 {
        self.y1 - self.y0
    }

    /// `self` grown by `by` on every side, clipped to `w` × `h`.
    fn grown(self, by: u32, w: u32, h: u32) -> Self {
        Self {
            x0: self.x0.saturating_sub(by),
            y0: self.y0.saturating_sub(by),
            x1: self.x1.saturating_add(by).min(w),
            y1: self.y1.saturating_add(by).min(h),
        }
    }

    fn contains(self, x: u32, y: u32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

/// The pixels of a `w` × `h` image drawn at `ctm` that word box `rect`
/// covers, or `None` when the box is not inside the image (half a pixel of
/// slack) or covers less than one pixel.
pub(super) fn word_pixels(ctm: Matrix, w: u32, h: u32, rect: Rect) -> Option<PixRect> {
    let inv = ctm.inverse()?;
    let (fw, fh) = (f64::from(w), f64::from(h));
    let corners = [
        (rect.llx, rect.lly),
        (rect.urx, rect.lly),
        (rect.llx, rect.ury),
        (rect.urx, rect.ury),
    ];
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in corners {
        let p = inv.map_point(Point::new(x, y));
        let (col, row) = (p.x * fw, (1.0 - p.y) * fh);
        x0 = x0.min(col);
        x1 = x1.max(col);
        y0 = y0.min(row);
        y1 = y1.max(row);
    }
    let slack = 0.5;
    if !(x0 >= -slack && y0 >= -slack && x1 <= fw + slack && y1 <= fh + slack) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to the image above
    let px = |v: f64, max: u32| v.round().clamp(0.0, f64::from(max)) as u32;
    let r = PixRect {
        x0: px(x0, w),
        y0: px(y0, h),
        x1: px(x1, w),
        y1: px(y1, h),
    };
    (r.x1 > r.x0 && r.y1 > r.y0).then_some(r)
}

/// An image's samples unpacked to one byte per component (a 1-bit image's
/// samples become 0 or 255), ready to measure and erase.
#[derive(Debug, Clone)]
pub(super) struct Raster {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) components: u32,
    one_bit: bool,
    data: Vec<u8>,
}

/// Why an image cannot be rewritten, or `None` when it can: 8-bit samples,
/// or 1-bit grey, in a device, calibrated or ICC space whose component
/// count matches, with no `/Decode`, `/ImageMask` or codestream alpha.
pub(super) fn unsupported(view: &DocumentView<'_>, dict: &Dict, d: &Decoded) -> bool {
    let flag = |k: &[u8]| {
        matches!(
            dict.get(k).map(|o| view.resolve(o)),
            Some(Object::Boolean(true))
        )
    };
    let space_n = dict
        .get(b"ColorSpace")
        .map(|o| view.resolve(o))
        .and_then(|cs| space_components(view, cs));
    flag(b"ImageMask")
        || dict.get(b"Decode").is_some()
        || d.embedded_alpha.is_some()
        || d.colorspace_substituted
        || space_n != Some(d.components)
        || !(d.bpc == 8 || (d.bpc == 1 && d.components == 1))
}

/// Components of a device, calibrated or ICC colour space; `None` for any
/// other family, whose sample values a device colour operator cannot carry.
fn space_components(view: &DocumentView<'_>, cs: &Object) -> Option<u32> {
    let (family, param) = match cs {
        Object::Name(n) => (n.as_bytes(), None),
        Object::Array(a) => (a.first()?.as_name()?.as_bytes(), a.get(1)),
        _ => return None,
    };
    match family {
        b"DeviceGray" | b"G" | b"CalGray" => Some(1),
        b"DeviceRGB" | b"RGB" | b"CalRGB" => Some(3),
        b"DeviceCMYK" | b"CMYK" => Some(4),
        b"ICCBased" => {
            let Object::Stream(s) = view.resolve(param?) else {
                return None;
            };
            let n = s.dict.get(b"N").map(|o| view.resolve(o))?.as_int()?;
            u32::try_from(n).ok().filter(|n| matches!(n, 1 | 3 | 4))
        }
        _ => None,
    }
}

impl Raster {
    /// One sample per byte: 1-bit images are unpacked to 0/255 so erasure
    /// and measurement work on one representation.
    pub(super) fn from_decoded(d: &Decoded) -> Self {
        let one_bit = d.bpc == 1;
        let data = if one_bit {
            let stride = (d.width as usize).div_ceil(8);
            let mut out = Vec::with_capacity(d.width as usize * d.height as usize);
            for row in d.samples.chunks(stride).take(d.height as usize) {
                for x in 0..d.width as usize {
                    let bit = row.get(x / 8).is_some_and(|b| b & (0x80 >> (x % 8)) != 0);
                    out.push(if bit { 255 } else { 0 });
                }
            }
            out
        } else {
            d.samples.clone()
        };
        Self {
            width: d.width,
            height: d.height,
            components: d.components,
            one_bit,
            data,
        }
    }

    fn at(&self, x: u32, y: u32) -> &[u8] {
        let n = self.components as usize;
        let i = (y as usize * self.width as usize + x as usize) * n;
        self.data.get(i..i + n).unwrap_or(&[])
    }

    fn set(&mut self, x: u32, y: u32, v: &[u8]) {
        let n = self.components as usize;
        let i = (y as usize * self.width as usize + x as usize) * n;
        if let Some(px) = self.data.get_mut(i..i + n) {
            px.copy_from_slice(v);
        }
    }

    /// The samples back in the source layout (1-bit re-packed).
    fn samples(&self) -> Vec<u8> {
        if !self.one_bit {
            return self.data.clone();
        }
        let stride = (self.width as usize).div_ceil(8);
        let mut out = vec![0_u8; stride * self.height as usize];
        for (i, &v) in self.data.iter().enumerate() {
            if v >= 128 {
                let (y, x) = (i / self.width as usize, i % self.width as usize);
                if let Some(b) = out.get_mut(y * stride + x / 8) {
                    *b |= 0x80 >> (x % 8);
                }
            }
        }
        out
    }
}

/// A word's measured background and ink.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Measure {
    pub(super) rect: PixRect,
    pub(super) background: Vec<u8>,
    /// The ink colour, one value per component in `0..=255`.
    pub(super) ink: Vec<u8>,
    /// The bounds of the word's ink, glyph overhang included.
    pub(super) ink_box: PixRect,
    /// The word's ink pixels: every pixel connected to ink inside the box.
    mask: Vec<(u32, u32)>,
}

/// Largest per-component difference between two samples.
fn dist(a: &[u8], b: &[u8]) -> u32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| u32::from(x.abs_diff(*y)))
        .max()
        .unwrap_or(0)
}

/// Per-component median of `pixels` (each `n` components).
fn median(pixels: &[&[u8]], n: usize) -> Vec<u8> {
    (0..n)
        .map(|c| {
            let mut v: Vec<u8> = pixels.iter().filter_map(|p| p.get(c).copied()).collect();
            v.sort_unstable();
            v.get(v.len() / 2).copied().unwrap_or(0)
        })
        .collect()
}

/// Below this largest difference from the background a box holds no ink.
const MIN_INK_CONTRAST: u32 = 48;
/// A background whose 75th-percentile difference from its median exceeds
/// this is a picture or a pattern, not paper.
const MAX_BACKGROUND_SPREAD: u32 = 40;

/// Measure the word at `rect`: the median of a ring around it is the
/// background, the median of its strongest-contrast pixels is the ink.
pub(super) fn measure(r: &Raster, rect: PixRect) -> Result<Measure, SuspectReason> {
    let n = r.components as usize;
    let pad = (rect.height() / 4).max(2);
    let outer = rect.grown(pad, r.width, r.height);
    let inner = rect.grown(1, r.width, r.height);
    let mut ring = Vec::new();
    for y in outer.y0..outer.y1 {
        for x in outer.x0..outer.x1 {
            if !inner.contains(x, y) {
                ring.push(r.at(x, y));
            }
        }
    }
    if ring.len() < 8 {
        return Err(SuspectReason::BusyBackground);
    }
    let background = median(&ring, n);
    let mut spread: Vec<u32> = ring.iter().map(|p| dist(p, &background)).collect();
    spread.sort_unstable();
    let p75 = spread.get(spread.len() * 3 / 4).copied().unwrap_or(0);
    if p75 > MAX_BACKGROUND_SPREAD {
        return Err(SuspectReason::BusyBackground);
    }
    let mut boxed = Vec::new();
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            let p = r.at(x, y);
            boxed.push((dist(p, &background), p));
        }
    }
    let dmax = boxed.iter().map(|(d, _)| *d).max().unwrap_or(0);
    if dmax < MIN_INK_CONTRAST {
        return Err(SuspectReason::NoInk);
    }
    let core: Vec<&[u8]> = boxed
        .iter()
        .filter(|(d, _)| *d * 4 >= dmax * 3)
        .map(|(_, p)| *p)
        .collect();
    let mask = ink_mask(r, rect, &background, (p75 + 8).max(dmax / 5));
    let reach = rect.height() / 4;
    // Half contrast is the glyph's edge under a symmetric blur; the mask's
    // looser threshold also takes the fringe, which would inflate the size.
    let ink_box = mask
        .iter()
        .filter(|&&(x, _)| x + reach >= rect.x0 && x < rect.x1 + reach)
        .filter(|&&(x, y)| dist(r.at(x, y), &background) * 2 >= dmax)
        .fold(None, |b: Option<PixRect>, &(x, y)| {
            Some(b.map_or(
                PixRect {
                    x0: x,
                    y0: y,
                    x1: x + 1,
                    y1: y + 1,
                },
                |b| PixRect {
                    x0: b.x0.min(x),
                    y0: b.y0.min(y),
                    x1: b.x1.max(x + 1),
                    y1: b.y1.max(y + 1),
                },
            ))
        })
        .unwrap_or(rect);
    Ok(Measure {
        rect,
        ink: median(&core, n),
        ink_box,
        mask,
        background,
    })
}

/// Every pixel differing from `background` by more than `above` that is
/// 8-connected to such a pixel inside `rect`, searched within the box grown
/// by a quarter of its height sideways and half of it up and down. A glyph
/// the engine's box clips is erased whole; a neighbouring word, separated by
/// paper, is not reached.
fn ink_mask(r: &Raster, rect: PixRect, background: &[u8], above: u32) -> Vec<(u32, u32)> {
    let h = rect.height();
    let zone = PixRect {
        x0: rect.x0.saturating_sub(h / 4),
        y0: rect.y0.saturating_sub(h / 2),
        x1: rect.x1.saturating_add(h / 4).min(r.width),
        y1: rect.y1.saturating_add(h / 2).min(r.height),
    };
    let zw = (zone.x1 - zone.x0) as usize;
    let at = |x: u32, y: u32| (y - zone.y0) as usize * zw + (x - zone.x0) as usize;
    let mut seen = vec![false; zw * (zone.y1 - zone.y0) as usize];
    let is_ink = |x: u32, y: u32| dist(r.at(x, y), background) > above;
    let mut stack = Vec::new();
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            if is_ink(x, y)
                && let Some(s) = seen.get_mut(at(x, y))
            {
                *s = true;
                stack.push((x, y));
            }
        }
    }
    let mut mask = Vec::new();
    while let Some((x, y)) = stack.pop() {
        mask.push((x, y));
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let (Some(nx), Some(ny)) = (x.checked_add_signed(dx), y.checked_add_signed(dy))
                else {
                    continue;
                };
                if !zone.contains(nx, ny) || !is_ink(nx, ny) {
                    continue;
                }
                if let Some(s) = seen.get_mut(at(nx, ny))
                    && !*s
                {
                    *s = true;
                    stack.push((nx, ny));
                }
            }
        }
    }
    mask
}

/// Paint `m`'s ink mask, dilated by one pixel for the antialiased edge, with
/// the measured background, except inside `protect` (the boxes of words
/// staying in the image).
pub(super) fn erase(r: &mut Raster, m: &Measure, protect: &[PixRect]) {
    for &(x, y) in &m.mask {
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (-1, 0), (0, -1)] {
            let (Some(px), Some(py)) = (x.checked_add_signed(dx), y.checked_add_signed(dy)) else {
                continue;
            };
            if px < r.width && py < r.height && !protect.iter().any(|p| p.contains(px, py)) {
                r.set(px, py, &m.background);
            }
        }
    }
}

/// The page-space bounds of pixels `p` of a `w` × `h` image drawn at `ctm`;
/// the inverse of [`word_pixels`].
pub(super) fn page_rect(ctm: Matrix, w: u32, h: u32, p: PixRect) -> Option<Rect> {
    if w == 0 || h == 0 {
        return None;
    }
    let (fw, fh) = (f64::from(w), f64::from(h));
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (col, row) in [(p.x0, p.y0), (p.x1, p.y0), (p.x0, p.y1), (p.x1, p.y1)] {
        let q = ctm.map_point(Point::new(f64::from(col) / fw, 1.0 - f64::from(row) / fh));
        x0 = x0.min(q.x);
        x1 = x1.max(q.x);
        y0 = y0.min(q.y);
        y1 = y1.max(q.y);
    }
    Some(Rect::from_corners(x0, y0, x1, y1))
}

/// A re-encoded image: its dictionary (no `/Length`) and stream bytes.
#[derive(Debug, Clone)]
pub(super) struct Encoded {
    pub(super) dict: Dict,
    pub(super) data: Vec<u8>,
    pub(super) filter: &'static str,
}

/// JPEG quality for a JPEG source re-encoded under
/// [`EditableImageCodec::MatchSource`].
const JPEG_QUALITY: u8 = 90;

/// Re-encode `r` with `original`'s dictionary, per `codec`: a JPEG source of
/// one or three components stays JPEG under `MatchSource`; everything else
/// becomes Flate (§7.4.4). `None` when the encoder fails.
pub(super) fn encode(
    original: &Dict,
    r: &Raster,
    source: Option<Codec>,
    codec: EditableImageCodec,
) -> Option<Encoded> {
    let mut dict = original.clone();
    for key in [
        &b"Filter"[..],
        b"F",
        b"DecodeParms",
        b"DP",
        b"Length",
        b"SMaskInData",
    ] {
        dict.remove(key);
    }
    dict.insert(Name::from(b"Width"), Object::Integer(i64::from(r.width)));
    dict.insert(Name::from(b"Height"), Object::Integer(i64::from(r.height)));
    let bpc = if r.one_bit { 1 } else { 8 };
    dict.insert(Name::from(b"BitsPerComponent"), Object::Integer(bpc));
    let samples = r.samples();
    let jpeg = (codec == EditableImageCodec::MatchSource
        && source == Some(Codec::Dct)
        && !r.one_bit
        && matches!(r.components, 1 | 3))
    .then(|| {
        let opts = crate::jpeg_write::JpegOptions {
            quality: JPEG_QUALITY,
            chroma: crate::jpeg_write::Chroma::Auto,
            progressive: false,
            optimize_huffman: true,
        };
        #[allow(clippy::cast_possible_truncation)] // 1 or 3, matched above
        let comps = r.components as u8;
        crate::jpeg_write::encode(&samples, r.width, r.height, comps, opts).ok()
    })
    .flatten();
    let (data, filter): (Vec<u8>, &'static str) = match jpeg {
        Some(bytes) => (bytes, "DCTDecode"),
        None => (
            crate::image_import::flate_encode(&samples).ok()?,
            "FlateDecode",
        ),
    };
    dict.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(filter.as_bytes())),
    );
    Some(Encoded { dict, data, filter })
}

/// The `/Filter` an image dictionary names, `/`-joined for a chain, or
/// `"none"`.
pub(super) fn filter_name(view: &DocumentView<'_>, dict: &Dict) -> String {
    match dict.get(b"Filter").map(|o| view.resolve(o)) {
        Some(Object::Name(n)) => String::from_utf8_lossy(n.as_bytes()).into_owned(),
        Some(Object::Array(a)) => a
            .iter()
            .filter_map(Object::as_name)
            .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned())
            .collect::<Vec<_>>()
            .join("/"),
        _ => "none".to_owned(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn grey(w: u32, h: u32, fill: u8) -> Raster {
        Raster {
            width: w,
            height: h,
            components: 1,
            one_bit: false,
            data: vec![fill; (w * h) as usize],
        }
    }

    #[test]
    fn a_dark_bar_on_paper_measures_its_ink_and_erases_to_paper() {
        let mut r = grey(40, 20, 240);
        for y in 6..14 {
            for x in 10..30 {
                r.set(x, y, &[30]);
            }
        }
        let rect = PixRect {
            x0: 10,
            y0: 6,
            x1: 30,
            y1: 14,
        };
        let m = measure(&r, rect).unwrap();
        assert_eq!(m.background, vec![240]);
        assert_eq!(m.ink, vec![30]);
        erase(&mut r, &m, &[]);
        assert!(r.data.iter().all(|&v| v == 240));
    }

    #[test]
    fn blank_paper_is_no_ink_and_noise_is_a_busy_background() {
        let r = grey(40, 20, 240);
        let rect = PixRect {
            x0: 10,
            y0: 6,
            x1: 30,
            y1: 14,
        };
        assert_eq!(measure(&r, rect), Err(SuspectReason::NoInk));
        let mut noisy = grey(40, 20, 0);
        for (i, v) in noisy.data.iter_mut().enumerate() {
            *v = if i % 2 == 0 { 0 } else { 255 };
        }
        assert_eq!(measure(&noisy, rect), Err(SuspectReason::BusyBackground));
    }

    #[test]
    fn one_bit_samples_survive_unpack_and_repack() {
        let d = Decoded {
            samples: vec![0b1010_0000, 0b0101_0000],
            width: 4,
            height: 2,
            components: 1,
            bpc: 1,
            codec: None,
            color_model: crate::image_codec::CodecColorModel::Gray,
            icc_profile: None,
            embedded_alpha: None,
            colorspace_substituted: false,
            preblended_alpha_dropped: false,
            paper: true,
        };
        let r = Raster::from_decoded(&d);
        assert_eq!(r.data, vec![255, 0, 255, 0, 0, 255, 0, 255]);
        assert_eq!(r.samples(), d.samples);
    }

    #[test]
    fn a_word_outside_the_image_has_no_pixels() {
        let ctm = Matrix::new(100.0, 0.0, 0.0, 50.0, 0.0, 0.0);
        let inside = word_pixels(ctm, 200, 100, Rect::from_corners(10.0, 40.0, 30.0, 45.0));
        assert_eq!(
            inside,
            Some(PixRect {
                x0: 20,
                y0: 10,
                x1: 60,
                y1: 20
            })
        );
        let outside = word_pixels(ctm, 200, 100, Rect::from_corners(90.0, 40.0, 130.0, 45.0));
        assert_eq!(outside, None);
    }
}

//! Point hit-testing for raster images: the image's own placed parallelogram
//! (the unit square under its CTM, ISO 32000-1 §8.9.4), then — given an
//! [`ImageAlpha`] — whether its mask leaves the nearest sample painted
//! (§8.9.6.2 stencil, §8.9.6.3 explicit, §8.9.6.4 colour key, §11.6.5.3 soft).
//!
//! A form XObject keeps its bounding-box test: a form has no samples, and its
//! `/BBox` is not stored on [`ImageObject`].

use std::cell::RefCell;
use std::collections::HashMap;

use super::decompose::{ImageObject, ImageSource};
use super::geometry::Point;
use crate::image_codec::{self, CodedImage};
use crate::image_import::row_bytes;
use crate::object::{Dict, ObjId, Object};
use crate::view::DocumentView;

/// Answers whether a placed image is transparent at a point.
///
/// Passed to [`hit_test_point_with`](super::hit::hit_test_point_with) and
/// its siblings. [`NoImageAlpha`] treats every image as opaque;
/// [`DocumentImageAlpha`] decodes masks from the document.
pub trait ImageAlpha {
    /// Whether the sample of `image` nearest the unit-square point `(u, v)`
    /// (image space flipped to y-up: `(0, 0)` is the lower-left corner on the
    /// page) is **fully** transparent. `false` whenever that cannot be
    /// decided, so a decode failure never makes an image unclickable.
    fn is_clear_at(&self, image: &ImageObject, u: f64, v: f64) -> bool;
}

/// Every image is opaque everywhere: the geometry-only hit test.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoImageAlpha;

impl ImageAlpha for NoImageAlpha {
    fn is_clear_at(&self, _image: &ImageObject, _u: f64, _v: f64) -> bool {
        false
    }
}

/// Mask-aware [`ImageAlpha`] over a document, memoizing each image's decoded
/// coverage by object (keep one alive across clicks on the same view).
///
/// Mask precedence follows §8.9.5 Table 89 and §11.6.5.3: `/SMask` overrides
/// `/Mask`; a JPX image with `/SMaskInData 1` uses its embedded alpha; an
/// `/ImageMask true` image is its own stencil. A soft-mask sample is clear
/// when its decoded value is 0; a stencil sample is clear when its decoded
/// value is 1 (§8.9.6.2: decoded 0 paints, `/Decode [1 0]` reverses the
/// raw meaning). A colour-key sample is clear when every raw, pre-`/Decode`
/// component lies in its inclusive range.
///
/// An inline image (no object to decode) and an image whose mask cannot be
/// decoded are treated as opaque.
#[derive(Debug)]
pub struct DocumentImageAlpha<'a> {
    view: &'a DocumentView<'a>,
    cache: RefCell<HashMap<ObjId, Option<Coverage>>>,
}

impl<'a> DocumentImageAlpha<'a> {
    /// A resolver over `view` (a session view sees images added this session).
    #[must_use]
    pub fn new(view: &'a DocumentView<'a>) -> Self {
        Self {
            view,
            cache: RefCell::new(HashMap::new()),
        }
    }
}

impl ImageAlpha for DocumentImageAlpha<'_> {
    fn is_clear_at(&self, image: &ImageObject, u: f64, v: f64) -> bool {
        let Some(id) = image.xobject else {
            return false;
        };
        let mut cache = self.cache.borrow_mut();
        let coverage = cache
            .entry(id)
            .or_insert_with(|| image_coverage(self.view, id));
        coverage.as_ref().is_some_and(|c| c.is_clear_at(u, v))
    }
}

/// Whether `point` hits `image` within `tolerance` (module docs).
pub(super) fn image_hit(
    image: &ImageObject,
    point: Point,
    tolerance: f64,
    alpha: &dyn ImageAlpha,
    edge_distance: impl Fn(Point, Point, Point) -> f64,
) -> bool {
    if image.source == ImageSource::Form {
        return image.page_bbox.inflate(tolerance).contains(point);
    }
    let unit = image.ctm.inverse().map(|inv| inv.map_point(point));
    let inside = unit.is_some_and(|p| (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y));
    if !inside {
        let c = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
            .map(|(x, y)| image.ctm.map_point(Point::new(x, y)));
        let near = c
            .iter()
            .zip(c.iter().cycle().skip(1))
            .any(|(a, b)| edge_distance(point, *a, *b) <= tolerance);
        if !near {
            return false;
        }
    }
    unit.is_none_or(|p| !alpha.is_clear_at(image, p.x.clamp(0.0, 1.0), p.y.clamp(0.0, 1.0)))
}

/// The largest mask grid decoded into a [`Coverage`] (one byte per sample,
/// ARCHITECTURE.md §10.1); a larger image is treated as opaque.
const MAX_COVERAGE_PIXELS: u64 = 1 << 26;

/// A mask's clear/painted state per sample, row 0 at the top (§8.9.3).
#[derive(Debug)]
struct Coverage {
    width: u32,
    height: u32,
    clear: Vec<bool>,
}

impl Coverage {
    fn is_clear_at(&self, u: f64, v: f64) -> bool {
        let col = nearest(u, self.width);
        let row = nearest(1.0 - v, self.height);
        let i = row.saturating_mul(self.width as usize).saturating_add(col);
        self.clear.get(i).copied().unwrap_or(false)
    }
}

/// The sample index nearest unit coordinate `t` across `n` samples.
fn nearest(t: f64, n: u32) -> usize {
    let last = n.saturating_sub(1);
    // Saturating float→int cast: NaN maps to 0, out-of-range clamps.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = (t * f64::from(n)).floor().clamp(0.0, f64::from(last)) as u32;
    i as usize
}

/// The image's coverage, or `None` when it has no mask or cannot be decoded.
fn image_coverage(view: &DocumentView<'_>, id: ObjId) -> Option<Coverage> {
    let Object::Stream(stream) = view.graph().resolved(id) else {
        return None;
    };
    let dict = &stream.dict;
    let get = |key: &[u8]| dict.get(key).map(|o| view.graph().resolve(o));
    if let Some(Object::Stream(sm)) = get(b"SMask") {
        let mask = Samples::decode(view, &sm.dict, view.slice(sm.data_span)?, false)?;
        return Some(mask.coverage(|s| s.decoded(0) <= 0.0));
    }
    let base = || Samples::decode(view, dict, view.slice(stream.data_span)?, false);
    if get(b"SMaskInData").and_then(Object::as_int).unwrap_or(0) == 1 {
        let image = base()?;
        let alpha = image.coded.embedded_alpha.as_ref()?;
        let clear = (0..image.width as usize * image.height as usize)
            .map(|i| alpha.get(i).is_some_and(|a| *a == 0))
            .collect();
        return Some(image.with_clear(clear));
    }
    if matches!(get(b"ImageMask"), Some(Object::Boolean(true))) {
        return Some(base()?.coverage(|s| s.decoded(0) > 0.5));
    }
    match get(b"Mask") {
        Some(Object::Stream(m)) => {
            let mask = Samples::decode(view, &m.dict, view.slice(m.data_span)?, true)?;
            Some(mask.coverage(|s| s.decoded(0) > 0.5))
        }
        Some(Object::Array(ranges)) => {
            let ranges: Vec<i64> = ranges
                .iter()
                .map(|o| view.graph().resolve(o).as_int())
                .collect::<Option<_>>()?;
            let image = base()?;
            let n = ranges.len() / 2;
            (n == image.components as usize && n > 0).then(|| {
                image.coverage(|s| {
                    (0..n).all(|c| {
                        let raw = i64::from(s.raw(c));
                        let lo = ranges.get(2 * c).copied().unwrap_or(1);
                        let hi = ranges.get(2 * c + 1).copied().unwrap_or(0);
                        lo <= raw && raw <= hi
                    })
                })
            })
        }
        _ => None,
    }
}

/// Decoded, still `/Decode`-free samples plus the geometry describing them.
struct Samples {
    coded: CodedImage,
    width: u32,
    height: u32,
    components: u32,
    bpc: u32,
    decode: Option<(f64, f64)>,
}

/// One pixel of a [`Samples`] grid.
struct Pixel<'s> {
    samples: &'s Samples,
    x: u32,
    y: u32,
}

impl Samples {
    /// Decode an image or mask stream. `is_mask` defaults a missing
    /// `/BitsPerComponent` and component count to the stencil shape.
    fn decode(view: &DocumentView<'_>, dict: &Dict, raw: &[u8], is_mask: bool) -> Option<Self> {
        let coded = image_codec::decode_image_view(view, dict, raw, false).ok()?;
        let int = |k: &[u8]| {
            dict.get(k)
                .map(|o| view.graph().resolve(o))
                .and_then(Object::as_int)
                .and_then(|v| u32::try_from(v).ok())
        };
        let stencil = is_mask || matches!(dict.get(b"ImageMask"), Some(Object::Boolean(true)));
        let from_codec = coded.codec.is_some() && coded.width > 0 && coded.height > 0;
        let (width, height) = if from_codec {
            (coded.width, coded.height)
        } else {
            (int(b"Width")?, int(b"Height")?)
        };
        let bpc = if from_codec && coded.bits_per_component > 0 {
            u32::from(coded.bits_per_component)
        } else if stencil {
            1
        } else {
            int(b"BitsPerComponent")?
        };
        let components = match coded.components {
            0 if stencil => 1,
            0 => colour_components(view, dict)?,
            n => u32::from(n),
        };
        let pixels = u64::from(width) * u64::from(height);
        if pixels == 0 || pixels > MAX_COVERAGE_PIXELS || !matches!(bpc, 1 | 2 | 4 | 8 | 16) {
            return None;
        }
        let decode = match dict.get(b"Decode").map(|o| view.graph().resolve(o)) {
            Some(Object::Array(a)) => {
                let num = |i: usize| {
                    a.get(i)
                        .map(|o| view.graph().resolve(o))
                        .and_then(Object::as_number)
                };
                Some((num(0)?, num(1)?))
            }
            _ => None,
        };
        Some(Self {
            coded,
            width,
            height,
            components,
            bpc,
            decode,
        })
    }

    fn coverage(&self, clear: impl Fn(&Pixel<'_>) -> bool) -> Coverage {
        let mut out = Vec::with_capacity(self.width as usize * self.height as usize);
        for y in 0..self.height {
            for x in 0..self.width {
                out.push(clear(&Pixel {
                    samples: self,
                    x,
                    y,
                }));
            }
        }
        Coverage {
            width: self.width,
            height: self.height,
            clear: out,
        }
    }

    fn with_clear(&self, clear: Vec<bool>) -> Coverage {
        Coverage {
            width: self.width,
            height: self.height,
            clear,
        }
    }
}

impl Pixel<'_> {
    /// Raw sample of component `c`; short data reads as 0.
    fn raw(&self, c: usize) -> u32 {
        let s = self.samples;
        let stride = row_bytes(s.width, s.components, s.bpc);
        let bit = (self.x as usize * s.components as usize + c) * s.bpc as usize;
        let at = self.y as usize * stride + bit / 8;
        let byte = |i: usize| u32::from(s.coded.samples.get(i).copied().unwrap_or(0));
        match s.bpc {
            16 => (byte(at) << 8) | byte(at + 1),
            8 => byte(at),
            b => (byte(at) >> (8 - b - (bit % 8) as u32)) & ((1 << b) - 1),
        }
    }

    /// Component `c` mapped through `/Decode` (§8.9.5.2; default `[0 1]`).
    fn decoded(&self, c: usize) -> f64 {
        let max = f64::from((1u32 << self.samples.bpc) - 1);
        let (lo, hi) = self.samples.decode.unwrap_or((0.0, 1.0));
        lo + f64::from(self.raw(c)) * (hi - lo) / max
    }
}

/// Components of an image XObject's `/ColorSpace` (§8.6), for byte-filter-only
/// streams whose codec declared none.
fn colour_components(view: &DocumentView<'_>, dict: &Dict) -> Option<u32> {
    let g = view.graph();
    let space = g.resolve(dict.get(b"ColorSpace")?);
    let family = match space {
        Object::Name(n) => n.as_bytes(),
        Object::Array(a) => match a.first().map(|o| g.resolve(o)) {
            Some(Object::Name(n)) => n.as_bytes(),
            _ => return None,
        },
        _ => return None,
    };
    let param = |i: usize| match space {
        Object::Array(a) => a.get(i).map(|o| g.resolve(o)),
        _ => None,
    };
    match family {
        b"DeviceGray" | b"CalGray" | b"Indexed" | b"Separation" | b"G" | b"I" => Some(1),
        b"DeviceRGB" | b"CalRGB" | b"Lab" | b"RGB" => Some(3),
        b"DeviceCMYK" | b"CMYK" => Some(4),
        b"ICCBased" => match param(1) {
            Some(Object::Stream(s)) => s
                .dict
                .get(b"N")
                .and_then(Object::as_int)
                .and_then(|n| u32::try_from(n).ok()),
            _ => None,
        },
        b"DeviceN" => match param(1) {
            Some(Object::Array(names)) => u32::try_from(names.len()).ok(),
            _ => None,
        },
        _ => None,
    }
}

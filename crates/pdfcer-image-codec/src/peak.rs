//! Peak-memory prediction for image decodes, and the limits a prediction
//! is checked against (decision 227).
//!
//! A predictor reads only headers: the geometry an image declares, never
//! its entropy-coded data. It answers "how many heap bytes will the decode
//! hold at its worst moment", so a caller can refuse an image that does
//! not fit its [`DecodeLimits::memory_budget`] *before* a dependency
//! allocates infallibly. Each predictor is pinned by measurement: the
//! `peak_memory` integration test counts the real peak with a counting
//! allocator and requires `measured <= predicted <= 1.5 * measured` on
//! every path, so a dependency upgrade that changes buffer sizing fails
//! that test rather than silently undercounting.
//!
//! Nothing in this module refuses anything yet; the budget goes live in a
//! later Pass and the existing ceiling constants stay in force until then.

pub(crate) mod dct;
mod jbig2;
#[cfg(feature = "jpx")]
mod jpx;

use pdfcer_model::filters::{self, predictor};
use pdfcer_model::graph::ObjectGraph;
use pdfcer_model::object::{Dict, Object};
use pdfcer_model::view::DocumentView;

use crate::Codec;

/// The memory a caller allows one image decode, plus an optional operator
/// pixel cap (decision 227 sources (a) and (c)).
///
/// The budget is supplied by the shell, normally half the machine's
/// physical memory; the engine never queries the machine itself, so a
/// result is reproducible from its inputs.
///
/// # Examples
///
/// ```
/// use pdfcer_image_codec::peak::DecodeLimits;
///
/// let limits = DecodeLimits::for_physical_memory(16 << 30)
///     .with_max_pixels(Some(100_000_000))
///     .with_downstream_bytes_per_pixel(4);
/// assert_eq!(limits.memory_budget, 8 << 30);
/// assert_eq!(limits.max_pixels, Some(100_000_000));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct DecodeLimits {
    /// Heap bytes one decode may hold at its peak, including the samples it
    /// returns and `downstream_bytes_per_pixel` for every pixel.
    pub memory_budget: u64,
    /// Operator strict mode: the most pixels an image may declare. `None`
    /// (the default) means memory alone decides.
    pub max_pixels: Option<u64>,
    /// Bytes per pixel the caller allocates after the decode (an RGBA
    /// pixmap is 4, each spot plane 1), added to the codec's own peak.
    pub downstream_bytes_per_pixel: u64,
}

impl DecodeLimits {
    /// The budget used when no shell supplies one: 4 GiB on 64-bit
    /// targets, 1 GiB on 32-bit targets including `wasm32`.
    pub const FALLBACK_MEMORY_BUDGET: u64 = if cfg!(target_pointer_width = "64") {
        4 << 30
    } else {
        1 << 30
    };

    /// A budget of `memory_budget` bytes, no pixel cap, nothing downstream.
    #[must_use]
    pub const fn new(memory_budget: u64) -> Self {
        Self {
            memory_budget,
            max_pixels: None,
            downstream_bytes_per_pixel: 0,
        }
    }

    /// The default budget for a machine with `physical_bytes` of RAM: half
    /// of it, so a decode never competes for the whole machine.
    #[must_use]
    pub const fn for_physical_memory(physical_bytes: u64) -> Self {
        Self::new(physical_bytes / 2)
    }

    /// Sets or clears the operator pixel cap.
    #[must_use]
    pub const fn with_max_pixels(mut self, max_pixels: Option<u64>) -> Self {
        self.max_pixels = max_pixels;
        self
    }

    /// Sets the bytes per pixel the caller allocates after the decode.
    #[must_use]
    pub const fn with_downstream_bytes_per_pixel(mut self, bytes: u64) -> Self {
        self.downstream_bytes_per_pixel = bytes;
        self
    }
}

impl Default for DecodeLimits {
    /// [`DecodeLimits::new`] over [`DecodeLimits::FALLBACK_MEMORY_BUDGET`].
    fn default() -> Self {
        Self::new(Self::FALLBACK_MEMORY_BUDGET)
    }
}

/// Allocator bookkeeping and small tables every decode holds regardless of
/// geometry (Huffman tables, segment lists, the inflater's state), measured
/// at up to 110 KiB across the paths in the `peak_memory` test.
const FIXED_OVERHEAD: u64 = 128 << 10;

/// The decoded-image record an unfiltered decode adds to its sample copy.
const RECORD_OVERHEAD: u64 = 1 << 10;

/// The inflater's output chunk, mirroring `pdfcer_model::filters::CHUNK`.
/// A drift is caught by the `peak_memory` test, not silently.
const FLATE_CHUNK: u64 = 64 << 10;

/// Predicted peak heap bytes of [`crate::decode_image_view`] on this image
/// stream: the copy of `raw` the filter chain starts from, every filter
/// stage's buffers and the codec's working set, including the samples it
/// returns.
///
/// Not included: `w * h * downstream_bytes_per_pixel`, which the caller
/// adds from its [`DecodeLimits`].
///
/// Returns `None` when the peak cannot be bounded from headers: a filter
/// chain other than none or a single `FlateDecode` before the image data,
/// any filter in front of a codec (its input length is only known after
/// decoding), a JBIG2 stream with a symbol dictionary or an unknown page
/// height, JPEG XL, or headers too damaged to read. A `None` decode is
/// bounded by the fallible-allocation backstop alone.
///
/// The prediction for an unfiltered or Flate image assumes the stream
/// decodes to the length its geometry declares; data past that length is
/// not counted.
#[must_use]
pub fn predict_peak(doc: &DocumentView<'_>, dict: &Dict, raw: &[u8]) -> Option<u64> {
    let names = filters::filter_names(dict).ok()?;
    let input = len(raw);
    match names.as_slice() {
        [] => Some(input.saturating_add(RECORD_OVERHEAD)),
        [only] => match Codec::from_name(only) {
            Some(codec) => {
                codec_working_bytes(doc, codec, raw, dict).map(|w| w.saturating_add(input))
            }
            None if matches!(only.as_slice(), b"FlateDecode" | b"Fl") => {
                flate_peak(doc, dict, input)
            }
            None => None,
        },
        _ => None,
    }
}

/// Predicted peak heap bytes a terminal `codec` holds while decoding
/// `data`, the fully filter-decoded image data, *above* `data` itself:
/// working buffers plus the returned samples.
///
/// `dict` is the image dictionary; its `/DecodeParms` supply CCITT
/// geometry and `/JBIG2Globals`. Returns `None` where [`predict_peak`]
/// lists, for the codec part.
#[must_use]
pub fn codec_working_bytes(
    doc: &DocumentView<'_>,
    codec: Codec,
    data: &[u8],
    dict: &Dict,
) -> Option<u64> {
    let parms = crate::codec_parms(dict, filters::filter_names(dict).ok()?.len());
    let working = match codec {
        Codec::Dct => dct::working_bytes(data)?,
        Codec::Ccitt => ccitt_working_bytes(doc, parms, dict)?,
        Codec::Jbig2 => {
            let globals = crate::jbig2::globals(doc, parms).ok()?;
            jbig2::working_bytes(data, globals.as_deref())?
        }
        #[cfg(feature = "jpx")]
        Codec::Jpx => jpx::working_bytes(data)?,
        #[cfg(not(feature = "jpx"))]
        Codec::Jpx => 0,
        Codec::Jxl => return None,
    };
    Some(working.saturating_add(FIXED_OVERHEAD))
}

/// `FlateDecode` alone in front of uncoded samples (ISO 32000-1 7.4.4).
///
/// The inflated bytes grow by doubling from one `FLATE_CHUNK`, so at the
/// last growth the old buffer (C/2) and the new one (C) are both live
/// beside the compressed copy. A PNG predictor then writes the unfiltered
/// rows into a fresh buffer while the inflated one is still held; a TIFF
/// predictor works in place.
fn flate_peak(doc: &DocumentView<'_>, dict: &Dict, input: u64) -> Option<u64> {
    let (width, height) = (
        dim(doc, dict, b"Width", b"W")?,
        dim(doc, dict, b"Height", b"H")?,
    );
    let row = width
        .saturating_mul(sample_components(doc, dict)?)
        .saturating_mul(bits_per_component(doc, dict)?)
        .div_ceil(8);
    let samples = row.saturating_mul(height);
    let params = predictor::Params::from_dict(crate::codec_parms(dict, 1)).ok()?;
    let png = params.filter(|p| p.predictor >= 10);
    let inflated = match png {
        Some(p) => {
            let bits = u64::from(p.columns) * u64::from(p.colors) * u64::from(p.bits_per_component);
            bits.div_ceil(8).saturating_add(1).saturating_mul(height)
        }
        None => samples,
    };
    let cap = drain_capacity(inflated);
    let inflating = input
        .saturating_add(FLATE_CHUNK)
        .saturating_add(cap / 2)
        .saturating_add(cap);
    let unfiltering = match png {
        Some(_) => input.saturating_add(cap).saturating_add(inflated),
        None => 0,
    };
    Some(inflating.max(unfiltering).saturating_add(FIXED_OVERHEAD))
}

/// Capacity of the inflate output buffer once it holds `n` bytes: the
/// first pull fills one chunk, then each growth doubles.
fn drain_capacity(n: u64) -> u64 {
    let mut cap = FLATE_CHUNK;
    while cap < n {
        cap = cap.saturating_mul(2);
    }
    cap
}

/// CCITT (T.4/T.6): two coding lines of `Columns` changing elements, and
/// the packed output growing byte by byte to the declared rows.
fn ccitt_working_bytes(doc: &DocumentView<'_>, parms: Option<&Dict>, dict: &Dict) -> Option<u64> {
    let (columns, rows) = crate::ccitt::declared_geometry(doc, parms, dict);
    let columns = u64::try_from(columns).ok().filter(|&c| c > 0)?;
    let packed = columns.div_ceil(8).saturating_mul(u64::from(rows?));
    let lines = columns.saturating_add(2).saturating_mul(16);
    Some(byte_pushed_peak(packed).saturating_add(lines))
}

/// Peak of a `Vec<u8>` filled one byte at a time to `n` bytes: capacity
/// doubles from 8, and the growth that reaches capacity C holds C/2 + C.
pub(crate) fn byte_pushed_peak(n: u64) -> u64 {
    let cap = n.max(8).checked_next_power_of_two().unwrap_or(u64::MAX);
    cap.saturating_add(cap / 2)
}

/// Colour components per sample of an uncoded image (ISO 32000-1 8.6,
/// 8.9.5): 1 for an image mask. `None` for a colour space that is a
/// resource name or a `/Pattern`, which an image cannot use.
fn sample_components(doc: &DocumentView<'_>, dict: &Dict) -> Option<u64> {
    if is_image_mask(doc, dict) {
        return Some(1);
    }
    let space = dict.get(b"ColorSpace").or_else(|| dict.get(b"CS"))?;
    colour_space_components(doc, doc.resolve(space))
}

fn colour_space_components(doc: &DocumentView<'_>, space: &Object) -> Option<u64> {
    let (family, rest) = match space {
        Object::Name(n) => (n.as_bytes(), &[][..]),
        Object::Array(items) => {
            let (first, rest) = items.split_first()?;
            (doc.resolve(first).as_name()?.as_bytes(), rest)
        }
        _ => return None,
    };
    match family {
        b"DeviceGray" | b"G" | b"CalGray" | b"Indexed" | b"I" | b"Separation" => Some(1),
        b"DeviceRGB" | b"RGB" | b"CalRGB" | b"Lab" => Some(3),
        b"DeviceCMYK" | b"CMYK" => Some(4),
        b"DeviceN" => Some(len(doc.resolve(rest.first()?).as_array()?)),
        b"ICCBased" => match doc.resolve(rest.first()?) {
            Object::Stream(s) => s.dict.get(b"N").and_then(Object::as_int),
            _ => None,
        }
        .and_then(|n| u64::try_from(n).ok()),
        _ => None,
    }
}

fn bits_per_component(doc: &DocumentView<'_>, dict: &Dict) -> Option<u64> {
    if is_image_mask(doc, dict) {
        return Some(1);
    }
    dim(doc, dict, b"BitsPerComponent", b"BPC")
}

fn is_image_mask(doc: &DocumentView<'_>, dict: &Dict) -> bool {
    let entry = dict.get(b"ImageMask").or_else(|| dict.get(b"IM"));
    matches!(entry.map(|o| doc.resolve(o)), Some(Object::Boolean(true)))
}

/// A non-negative integer entry under its full or inline-abbreviated key.
fn dim(doc: &DocumentView<'_>, dict: &Dict, key: &[u8], short: &[u8]) -> Option<u64> {
    let entry = dict.get(key).or_else(|| dict.get(short))?;
    u64::try_from(doc.resolve(entry).as_int()?).ok()
}

/// A slice length as `u64`; lossless on every supported target.
fn len<T>(s: &[T]) -> u64 {
    u64::try_from(s.len()).unwrap_or(u64::MAX)
}

/// Big-endian unsigned field of `N` bytes at `at`.
fn be<const N: usize>(d: &[u8], at: usize) -> Option<u64> {
    let bytes: &[u8; N] = d.get(at..at.checked_add(N)?)?.try_into().ok()?;
    Some(bytes.iter().fold(0, |v, &b| (v << 8) | u64::from(b)))
}

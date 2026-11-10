//! A rendered page as a JPEG XL file (ISO/IEC 18181), `Pass 696.0`.
//!
//! Encoding is `jixel` (a pure-Rust port of libjxl-tiny) on one thread;
//! every file is decoded back with `jxl-oxide` before it is returned, so a
//! lossless encode that does not reproduce the pixmap exactly is an error,
//! never a written file. A lossy encode reports its measured error.

use tiny_skia::Pixmap;

use crate::export::{ExportError, Rgb, flatten_over};

/// How [`encode_jxl`] encodes.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct JxlOptions {
    /// `None` (the default) encodes losslessly. `Some(d)` encodes lossy at
    /// Butteraugli distance `d`, clamped to `0.1..=25.0`; 1.0 is visually
    /// lossless, larger is smaller and coarser.
    pub distance: Option<f32>,
    /// `Some(colour)` flattens the pixmap over `colour` and writes no alpha;
    /// `None` keeps the pixmap's alpha.
    pub background: Option<Rgb>,
}

impl Default for JxlOptions {
    fn default() -> Self {
        Self {
            distance: None,
            background: Some(Rgb::WHITE),
        }
    }
}

impl JxlOptions {
    /// Lossy at Butteraugli `distance` (see [`JxlOptions::distance`]).
    #[must_use]
    pub fn with_distance(mut self, distance: f32) -> Self {
        self.distance = Some(distance);
        self
    }

    /// Flatten over `background`, or keep alpha with `None`.
    #[must_use]
    pub fn with_background(mut self, background: Option<Rgb>) -> Self {
        self.background = background;
        self
    }
}

/// A verified JPEG XL file and what the decode-back measured.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct JxlEncoded {
    /// The file's bytes (a bare codestream, `FF 0A`).
    pub bytes: Vec<u8>,
    /// The largest per-sample difference between the decoded file and the
    /// input, 0–255. Always 0 for a lossless encode.
    pub max_error: u8,
    /// Whether the file carries an alpha channel.
    pub has_alpha: bool,
}

/// The Butteraugli distance range `jixel` accepts for lossy encoding.
const DISTANCE_RANGE: std::ops::RangeInclusive<f32> = 0.1..=25.0;

/// Encode `pixmap` as JPEG XL, then decode it back and check it.
///
/// # Errors
///
/// [`ExportError::EmptyRaster`] for a zero dimension;
/// [`ExportError::Encoder`] when the encoder fails, when the decode-back
/// fails, or when a lossless file does not reproduce the input exactly.
///
/// # Example
///
/// ```
/// use pdfcer_render::export::{encode_jxl, JxlOptions};
/// use pdfcer_render::tiny_skia::Pixmap;
///
/// let mut pixmap = Pixmap::new(8, 8).unwrap();
/// pixmap.fill(pdfcer_render::tiny_skia::Color::from_rgba8(0, 128, 255, 255));
/// let jxl = encode_jxl(&pixmap, &JxlOptions::default()).unwrap();
/// assert_eq!(&jxl.bytes[..2], &[0xFF, 0x0A]);
/// assert_eq!(jxl.max_error, 0);
/// ```
pub fn encode_jxl(pixmap: &Pixmap, options: &JxlOptions) -> Result<JxlEncoded, ExportError> {
    let (width, height) = (pixmap.width(), pixmap.height());
    if width == 0 || height == 0 {
        return Err(ExportError::EmptyRaster { width, height });
    }
    let (samples, has_alpha) = straight_samples(pixmap, options.background);
    let mut config = jixel::EncodeConfig::default().with_num_threads(1);
    config = match options.distance {
        None => config.with_lossless(true),
        Some(d) => config.with_distance(d.clamp(*DISTANCE_RANGE.start(), *DISTANCE_RANGE.end())),
    };
    let (w, h) = (width as usize, height as usize);
    let encoded = if has_alpha {
        jixel::encode_image_with_alpha(&samples, w, h, &config)
    } else {
        jixel::encode_image(&samples, w, h, &config)
    }
    .map_err(|e| encoder_error(format!("{e:?}")))?;
    let max_error = verify(&encoded, &samples, width, height, has_alpha)?;
    if options.distance.is_none() && max_error != 0 {
        return Err(encoder_error(format!(
            "the lossless file decodes {max_error} level(s) away from the page; nothing written"
        )));
    }
    Ok(JxlEncoded {
        bytes: encoded,
        max_error,
        has_alpha,
    })
}

fn encoder_error(detail: String) -> ExportError {
    ExportError::Encoder {
        format: "JPEG XL",
        detail,
    }
}

/// Interleaved straight-alpha RGBA (`background` `None`) or flattened RGB.
fn straight_samples(pixmap: &Pixmap, background: Option<Rgb>) -> (Vec<u8>, bool) {
    match background {
        Some(bg) => {
            let flat = flatten_over(pixmap, bg);
            let mut rgb = Vec::with_capacity(flat.pixels().len() * 3);
            for px in flat.pixels() {
                rgb.extend_from_slice(&[px.red(), px.green(), px.blue()]);
            }
            (rgb, false)
        }
        None => {
            let mut rgba = Vec::with_capacity(pixmap.pixels().len() * 4);
            for px in pixmap.pixels() {
                let c = px.demultiply();
                rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
            }
            (rgba, true)
        }
    }
}

/// Decode `encoded` and return the largest per-sample difference from
/// `samples`. Fully transparent pixels compare alpha only: their colour is
/// undefined and the encoder may rewrite it.
fn verify(
    encoded: &[u8],
    samples: &[u8],
    width: u32,
    height: u32,
    has_alpha: bool,
) -> Result<u8, ExportError> {
    let back = pdfcer_core::image_codec::jxl::decode_pixels(encoded)
        .map_err(|e| encoder_error(format!("the file does not decode back: {e}")))?;
    if back.width != width || back.height != height || back.colour_channels != 3 {
        return Err(encoder_error(format!(
            "the file decodes as {}x{} with {} colour channel(s), not {width}x{height} RGB",
            back.width, back.height, back.colour_channels
        )));
    }
    let alpha = match (has_alpha, &back.alpha) {
        (true, Some(a)) => Some(a.as_slice()),
        (false, None) => None,
        _ => {
            return Err(encoder_error(
                "the file's alpha channel is not the page's".into(),
            ));
        }
    };
    let stride = if has_alpha { 4 } else { 3 };
    let mut max = 0u8;
    for (i, (src, rgb)) in samples
        .chunks_exact(stride)
        .zip(back.colour.as_chunks::<3>().0)
        .enumerate()
    {
        if let Some(a) = alpha {
            let a_back = a.get(i).copied().unwrap_or(0);
            let a_src = src.get(3).copied().unwrap_or(255);
            max = max.max(a_src.abs_diff(a_back));
            if a_src == 0 {
                continue;
            }
        }
        for (s, d) in src.iter().take(3).zip(rgb) {
            max = max.max(s.abs_diff(*d));
        }
    }
    Ok(max)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tiny_skia::{Color, Paint, PathBuilder, Transform};

    fn page() -> Pixmap {
        let mut p = Pixmap::new(64, 40).unwrap();
        p.fill(Color::WHITE);
        let mut paint = Paint::default();
        paint.set_color_rgba8(200, 20, 40, 255);
        let path = PathBuilder::from_circle(30.0, 20.0, 12.0).unwrap();
        p.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
        p
    }

    #[test]
    fn lossless_round_trips_exactly() {
        let out = encode_jxl(&page(), &JxlOptions::default()).unwrap();
        assert_eq!(out.max_error, 0);
        assert!(!out.has_alpha);
    }

    #[test]
    fn lossless_keeps_alpha_when_asked() {
        let mut p = Pixmap::new(16, 16).unwrap();
        p.fill(Color::from_rgba8(10, 200, 30, 128));
        let out = encode_jxl(&p, &JxlOptions::default().with_background(None)).unwrap();
        assert!(out.has_alpha);
        assert_eq!(out.max_error, 0);
    }

    /// Photo-like content: a gradient with hashed noise. On flat line art
    /// lossless modular can beat lossy, so size is only compared here.
    fn photo() -> Pixmap {
        let mut p = Pixmap::new(192, 128).unwrap();
        for (i, px) in p.pixels_mut().iter_mut().enumerate() {
            let (x, y) = ((i % 192) as u32, (i / 192) as u32);
            let n = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) >> 27;
            let c = |v: u32| u8::try_from((v + n).min(255)).unwrap();
            *px = tiny_skia::ColorU8::from_rgba(c(x), c(y * 2), c(128), 255).premultiply();
        }
        p
    }

    #[test]
    fn lossy_is_smaller_and_reports_its_error() {
        let lossless = encode_jxl(&photo(), &JxlOptions::default()).unwrap();
        let lossy = encode_jxl(&photo(), &JxlOptions::default().with_distance(4.0)).unwrap();
        assert!(
            lossy.bytes.len() < lossless.bytes.len(),
            "{} vs {}",
            lossy.bytes.len(),
            lossless.bytes.len()
        );
        assert!(
            lossy.max_error > 0 && lossy.max_error < 128,
            "{}",
            lossy.max_error
        );
    }
}

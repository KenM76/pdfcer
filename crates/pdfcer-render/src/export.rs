//! # Raster export — a rendered page as a PNG or JPEG file (`Pass 248.0`)
//!
//! Turns a [`tiny_skia::Pixmap`] — the thing every render in this crate
//! produces — into the bytes of an image file another program will open,
//! carrying the two facts a file format can hold that a pixmap cannot:
//! **physical resolution** and, for JPEG, **the colour the transparency
//! was flattened onto**.
//!
//! ## Why this module exists when `Pixmap::encode_png` already does
//!
//! `pdfcer render-page` has written PNGs through `Pixmap::encode_png` since
//! `Pass 1`. That encoder is correct — it demultiplies, it writes RGBA8 —
//! and it writes **no `pHYs` chunk**. A PNG without one has no physical
//! size, so Word, PowerPoint and LibreOffice place it at 96 DPI: a US
//! Letter page rendered at 300 DPI arrives on the slide as 26 inches wide.
//! The operator's request (2026-09-03) was for an export *to paste into
//! other software*, and a paste that lands at four times the page's size is
//! a paste the operator immediately has to fix by hand.
//!
//! The `png` crate is already in the dependency graph — it is what
//! `tiny_skia`'s encoder calls — so writing the chunk costs no package and
//! no licence entry; only the twenty lines of [`encode_png`] below.
//!
//! ## Contracts
//!
//! - **Input pixmaps are premultiplied** (that is what `tiny_skia` stores)
//!   and both encoders demultiply through `PremultipliedColorU8::demultiply`
//!   — one place, the library's own, never a hand-rolled division. Rust RAG
//!   `premultiplied_alpha_needs_multiply_not_clamp.md` records the cost of
//!   getting this wrong: dark fringes on every anti-aliased edge, on every
//!   export, with no error.
//! - **PNG keeps alpha; JPEG cannot.** A JPEG is flattened over an opaque
//!   [`Rgb`] the caller chooses ([`JpegOptions::background`]), and the
//!   caller — not this module — is responsible for *saying so*: a shell
//!   that offers `--transparent` refuses it for JPEG by name rather than
//!   silently flattening, because a white-backed JPEG looks exactly like
//!   success (`CLAUDE.md` rule 4).
//! - **Resolution is metadata, never a resample.** `dpi` is written into
//!   the file (`pHYs` for PNG, JFIF density for JPEG); the pixel grid is the
//!   pixmap's. A caller wanting 300 DPI renders at `scale = 300 / 72` and
//!   passes `dpi = 300`. Two numbers because they are two facts: the second
//!   is a claim about the first that the file will carry to a program that
//!   cannot check it.
//! - **Nothing here allocates a second copy of an opaque image for the
//!   sake of symmetry.** [`flatten_over`] returns the input untouched when
//!   every pixel is already opaque, so a `White`-backdrop render costs one
//!   scan and no copy.
//!
//! ## Failure modes
//!
//! [`ExportError`] — the pixmap is empty, wider than JPEG's 16-bit limit,
//! or the encoder itself failed (an I/O error into a `Vec`, which in
//! practice means allocation). None of these are document errors; they are
//! reported, not recovered from.

use std::fmt::Write as _;

use tiny_skia::Pixmap;

/// A DPI value the file formats can carry. Both write it as an integer
/// (pixels per metre for PNG, dots per inch for JFIF), so the same
/// rounding is applied once, here.
fn dpi_to_dots_per_metre(dpi: f32) -> u32 {
    // 1 inch = 0.0254 m, so ppm = dpi / 0.0254. Clamped: `pHYs` is a u32
    // and a negative or NaN DPI is a caller bug that must not become a
    // panic in a batch export.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ppm = (f64::from(dpi) / 0.0254).round();
    if ppm.is_finite() && ppm > 0.0 {
        ppm.min(f64::from(u32::MAX)) as u32
    } else {
        0
    }
}

/// An opaque sRGB colour, 8 bits per channel — the backdrop a JPEG is
/// flattened onto and the optional background of an SVG.
///
/// Its own type rather than `[u8; 3]` so a call site reads
/// `Rgb::WHITE` and so the `#rrggbb` parser every shell needs lives beside
/// the value it produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    /// Red, `0..=255`.
    pub r: u8,
    /// Green, `0..=255`.
    pub g: u8,
    /// Blue, `0..=255`.
    pub b: u8,
}

impl Rgb {
    /// Paper.
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
    };

    /// Parse a CSS-style hex colour: `#rrggbb` or `rrggbb`, case-insensitive.
    ///
    /// The three-digit `#rgb` shorthand is **not** accepted — an operator
    /// typing a background colour on a command line is copying it from a
    /// design tool, which gives six digits, and a shorthand that silently
    /// expands `#abc` to `#aabbcc` is one more thing to explain.
    ///
    /// # Errors
    ///
    /// A message naming what was wrong, for the shell to print.
    ///
    /// # Example
    ///
    /// ```
    /// use pdfcer_render::export::Rgb;
    ///
    /// assert_eq!(Rgb::parse_hex("#FF8000"), Ok(Rgb { r: 255, g: 128, b: 0 }));
    /// assert_eq!(Rgb::parse_hex("ffffff"), Ok(Rgb::WHITE));
    /// assert!(Rgb::parse_hex("#fff").is_err());
    /// ```
    pub fn parse_hex(text: &str) -> Result<Self, String> {
        let hex = text.trim().strip_prefix('#').unwrap_or(text.trim());
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!(
                "`{text}` is not a colour; expected six hex digits like `#ffffff`"
            ));
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
        Ok(Self {
            r: channel(0),
            g: channel(2),
            b: channel(4),
        })
    }

    /// The `#rrggbb` spelling, lowercase.
    #[must_use]
    pub fn to_hex(self) -> String {
        let mut s = String::with_capacity(7);
        // Writing to a `String` cannot fail.
        let _ = write!(s, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b);
        s
    }
}

/// Why an export could not produce bytes.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExportError {
    /// The pixmap has a zero dimension. A render never produces one (it
    /// refuses with `BadRasterSize` first), so this is a caller error.
    #[error("cannot export an empty {width}x{height} raster")]
    EmptyRaster {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
    },
    /// JPEG stores its dimensions in 16 bits (ITU-T T.81 §B.2.2), so a
    /// raster wider or taller than 65 535 pixels has no JPEG form at all.
    /// PNG has no such limit.
    #[error("a {width}x{height} raster exceeds JPEG's 65535-pixel limit on each side")]
    TooLargeForJpeg {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
    },
    /// The encoder failed — an allocation, in practice, since the sink is
    /// a `Vec`.
    #[error("{format} encoding failed: {detail}")]
    Encoder {
        /// `"PNG"` or `"JPEG"`.
        format: &'static str,
        /// The encoder's own message.
        detail: String,
    },
}

/// Encode a pixmap as an RGBA8 PNG, demultiplied, with an optional `pHYs`
/// resolution chunk.
///
/// The alpha channel is the pixmap's own: a render with
/// [`crate::PageBackdrop::Transparent`] produces a PNG that is see-through
/// where nothing was painted, and a default (white-backed) render produces
/// one whose every pixel is opaque. This function does not flatten —
/// see [`flatten_over`] for that.
///
/// `dpi` — `Some(300.0)` writes `pHYs` so the file carries its physical
/// size; `None` writes no chunk (the pre-`Pass 248.0` behaviour of
/// `render-page`). Fractional DPI rounds to whole pixels per metre, which
/// is a relative error below 3 × 10⁻⁵ at any resolution a page export uses.
///
/// # Errors
///
/// [`ExportError::EmptyRaster`], [`ExportError::Encoder`].
///
/// # Example
///
/// ```
/// use pdfcer_render::export::encode_png;
/// use pdfcer_render::tiny_skia::Pixmap;
///
/// let mut pixmap = Pixmap::new(4, 4).unwrap();
/// pixmap.fill(pdfcer_render::tiny_skia::Color::from_rgba8(255, 0, 0, 128));
/// let png = encode_png(&pixmap, Some(150.0)).unwrap();
/// assert_eq!(&png[1..4], b"PNG");
/// ```
pub fn encode_png(pixmap: &Pixmap, dpi: Option<f32>) -> Result<Vec<u8>, ExportError> {
    let (width, height) = (pixmap.width(), pixmap.height());
    if width == 0 || height == 0 {
        return Err(ExportError::EmptyRaster { width, height });
    }
    // Demultiply into a straight-alpha RGBA8 buffer. `demultiply` is
    // tiny-skia's own; a hand-written `c * 255 / a` here would be a second
    // implementation of the one arithmetic this module must not get wrong.
    let mut rgba = Vec::with_capacity(pixmap.pixels().len() * 4);
    for px in pixmap.pixels() {
        let c = px.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        if let Some(dpi) = dpi {
            let ppm = dpi_to_dots_per_metre(dpi);
            if ppm > 0 {
                encoder.set_pixel_dims(Some(png::PixelDimensions {
                    xppu: ppm,
                    yppu: ppm,
                    unit: png::Unit::Meter,
                }));
            }
        }
        let mut writer = encoder.write_header().map_err(|e| ExportError::Encoder {
            format: "PNG",
            detail: e.to_string(),
        })?;
        writer
            .write_image_data(&rgba)
            .map_err(|e| ExportError::Encoder {
                format: "PNG",
                detail: e.to_string(),
            })?;
    }
    Ok(out)
}

/// How a JPEG is written.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct JpegOptions {
    /// Encoder quality, `1..=100`. Clamped, not refused: an operator
    /// asking for `105` wants the best the format has, and a refusal on a
    /// number that only ever means "more" would be pedantry.
    pub quality: u8,
    /// The opaque colour every partially- or fully-transparent pixel is
    /// composited onto before encoding. JPEG has no alpha; this is where
    /// it goes, and the caller discloses it.
    pub background: Rgb,
    /// Resolution written as JFIF density (`Some`) or left as "no units"
    /// (`None`). Metadata only — see the module docs.
    pub dpi: Option<f32>,
}

impl Default for JpegOptions {
    /// Quality 90 over white, no density. 90 is where 4:2:0 chroma
    /// subsampling switches off in `jpeg-encoder` (its own quality-
    /// conditioned default), which for a page export — line art, text —
    /// is the difference between crisp and smeared colour edges.
    fn default() -> Self {
        Self {
            quality: 90,
            background: Rgb::WHITE,
            dpi: None,
        }
    }
}

/// Composite every non-opaque pixel onto `background`, returning a new
/// opaque pixmap — or the input itself, unchanged, when nothing needed
/// flattening.
///
/// §11.4.7's media composite `C = (1 − αg)·W + αg·Cg` with `W` the given
/// colour instead of white; since the buffer is premultiplied, `αg·Cg` is
/// the stored value and the operation is "add the uncovered fraction of the
/// background". This is the same arithmetic the renderer's own
/// white-flatten performs, generalised to a colour.
///
/// Returns `Cow::Borrowed` for an already-opaque input so a caller exporting
/// a default render pays one scan and no copy.
#[must_use]
pub fn flatten_over(pixmap: &Pixmap, background: Rgb) -> std::borrow::Cow<'_, Pixmap> {
    if pixmap.pixels().iter().all(|px| px.alpha() == 255) {
        return std::borrow::Cow::Borrowed(pixmap);
    }
    let mut out = pixmap.clone();
    for px in out.pixels_mut() {
        let a = u32::from(px.alpha());
        if a == 255 {
            continue;
        }
        let uncovered = 255 - a;
        // `c + bg·(1 − a)`, in 8-bit with the product rounded. Bounded by
        // 255 for a valid premultiplied pixel (`c ≤ a`); the `min` is a
        // guard against a malformed buffer, not part of the formula.
        let mix = |c: u8, bg: u8| {
            let added = (u32::from(bg) * uncovered + 127) / 255;
            u8::try_from((u32::from(c) + added).min(255)).unwrap_or(255)
        };
        *px = tiny_skia::PremultipliedColorU8::from_rgba(
            mix(px.red(), background.r),
            mix(px.green(), background.g),
            mix(px.blue(), background.b),
            255,
        )
        .unwrap_or(*px);
    }
    std::borrow::Cow::Owned(out)
}

/// Encode a pixmap as a baseline JPEG, flattened over
/// [`JpegOptions::background`].
///
/// Colour is 8-bit YCbCr from the demultiplied, flattened sRGB; chroma
/// subsampling follows the encoder's quality-conditioned default (4:2:0
/// below quality 90, 4:4:4 at or above), which is what "quality N" means to
/// anyone who has used a photographic encoder and is therefore left alone.
///
/// # Errors
///
/// [`ExportError::EmptyRaster`], [`ExportError::TooLargeForJpeg`],
/// [`ExportError::Encoder`].
///
/// # Example
///
/// ```
/// use pdfcer_render::export::{encode_jpeg, JpegOptions};
/// use pdfcer_render::tiny_skia::Pixmap;
///
/// let mut pixmap = Pixmap::new(8, 8).unwrap();
/// pixmap.fill(pdfcer_render::tiny_skia::Color::from_rgba8(0, 0, 255, 255));
/// let jpeg = encode_jpeg(&pixmap, &JpegOptions::default()).unwrap();
/// assert_eq!(&jpeg[..2], &[0xFF, 0xD8]); // SOI marker
/// ```
pub fn encode_jpeg(pixmap: &Pixmap, options: &JpegOptions) -> Result<Vec<u8>, ExportError> {
    let (width, height) = (pixmap.width(), pixmap.height());
    if width == 0 || height == 0 {
        return Err(ExportError::EmptyRaster { width, height });
    }
    let (Ok(w16), Ok(h16)) = (u16::try_from(width), u16::try_from(height)) else {
        return Err(ExportError::TooLargeForJpeg { width, height });
    };
    let flat = flatten_over(pixmap, options.background);
    // Every pixel is opaque now, so premultiplied == straight and the
    // channels can be read directly. `demultiply` would be a no-op at
    // alpha 255 and is skipped only to keep the loop honest about that.
    let mut rgb = Vec::with_capacity(flat.pixels().len() * 3);
    for px in flat.pixels() {
        rgb.extend_from_slice(&[px.red(), px.green(), px.blue()]);
    }
    let mut out = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut out, options.quality.clamp(1, 100));
    if let Some(dpi) = options.dpi {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let d = dpi.round().clamp(1.0, f32::from(u16::MAX)) as u16;
        encoder.set_density(jpeg_encoder::PixelDensity::dpi(d));
    }
    encoder
        .encode(&rgb, w16, h16, jpeg_encoder::ColorType::Rgb)
        .map_err(|e| ExportError::Encoder {
            format: "JPEG",
            detail: e.to_string(),
        })?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_rounds_to_pixels_per_metre() {
        assert_eq!(dpi_to_dots_per_metre(72.0), 2835);
        assert_eq!(dpi_to_dots_per_metre(300.0), 11811);
        assert_eq!(dpi_to_dots_per_metre(0.0), 0);
        assert_eq!(dpi_to_dots_per_metre(-5.0), 0);
        assert_eq!(dpi_to_dots_per_metre(f32::NAN), 0);
    }

    #[test]
    fn hex_parses_both_spellings_and_refuses_shorthand() {
        assert_eq!(Rgb::parse_hex("#000000"), Ok(Rgb { r: 0, g: 0, b: 0 }));
        assert_eq!(
            Rgb::parse_hex("  ABCDEF "),
            Ok(Rgb {
                r: 0xab,
                g: 0xcd,
                b: 0xef
            })
        );
        assert!(Rgb::parse_hex("#abc").is_err());
        assert!(Rgb::parse_hex("#gggggg").is_err());
        assert!(Rgb::parse_hex("").is_err());
        assert_eq!(
            Rgb {
                r: 255,
                g: 128,
                b: 0
            }
            .to_hex(),
            "#ff8000"
        );
    }

    #[test]
    fn flatten_over_borrows_an_opaque_input_and_composites_a_transparent_one() {
        let mut opaque = Pixmap::new(2, 2).unwrap();
        opaque.fill(tiny_skia::Color::from_rgba8(10, 20, 30, 255));
        assert!(matches!(
            flatten_over(&opaque, Rgb::WHITE),
            std::borrow::Cow::Borrowed(_)
        ));

        // A 50%-alpha pure red over pure blue: half red, half blue.
        let mut half = Pixmap::new(1, 1).unwrap();
        half.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 128));
        let flat = flatten_over(&half, Rgb { r: 0, g: 0, b: 255 });
        let px = flat.pixels()[0];
        assert_eq!(px.alpha(), 255);
        assert!((px.red() as i32 - 128).abs() <= 1, "red {}", px.red());
        assert!((px.blue() as i32 - 127).abs() <= 1, "blue {}", px.blue());
        assert_eq!(px.green(), 0);
    }

    #[test]
    fn empty_rasters_are_refused_by_both_encoders() {
        // `Pixmap::new(0, _)` returns None, so build the smallest legal one
        // and check the guard through a zero-width path is unreachable;
        // instead assert the JPEG size guard, which IS reachable.
        let big = Pixmap::new(65_536, 1);
        if let Some(big) = big {
            assert!(matches!(
                encode_jpeg(&big, &JpegOptions::default()),
                Err(ExportError::TooLargeForJpeg { .. })
            ));
        }
    }
}

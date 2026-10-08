//! PaddleOCR-VL work that needs no model runtime: image resizing and
//! patching, prompt assembly and line placement. Ungated and pure.
//!
//! The constants and `smart_resize` follow the model's own image processor
//! (`image_processing_paddleocr_vl.py` and `processor_config.json` in the
//! PaddlePaddle/PaddleOCR-VL repository): factor 28 = patch 14 x merge 2,
//! pixel bounds 112,896..=1,003,520, PIL bicubic resampling, rescale 1/255,
//! mean and std 0.5 per channel, patches in row-major grid order.

use super::RecognizedWord;
use super::vl_tokenizer::VlTokenizer;
use crate::page_tree::Rect;

/// Side of one vision patch, in resized pixels.
pub const PATCH: usize = 14;
/// Patches merged per side into one image token.
pub const MERGE: usize = 2;
/// Resized sides are multiples of this (`PATCH * MERGE`).
pub const FACTOR: usize = PATCH * MERGE;
/// Fewest resized pixels.
pub const MIN_PIXELS: usize = 112_896;
/// Most resized pixels.
pub const MAX_PIXELS: usize = 1_003_520;
/// Most input pixels accepted before resizing.
pub const MAX_INPUT_PIXELS: usize = 1 << 26;
/// Largest long-side / short-side ratio the model's processor accepts.
pub const MAX_ASPECT: f64 = 200.0;
/// Grey level below which a pixel counts as ink for cropping and placement.
pub const INK_BELOW: u8 = 160;

/// Text before the image placeholders.
pub const PROMPT_PREFIX: &str = "<|begin_of_sentence|>User: <|IMAGE_START|>";
/// The image placeholder token's text.
pub const IMAGE_PLACEHOLDER: &str = "<|IMAGE_PLACEHOLDER|>";
/// Text after the image placeholders: the model's plain-OCR task prompt.
pub const PROMPT_SUFFIX: &str = "<|IMAGE_END|>OCR:\nAssistant:\n";

/// What the model is asked to read a region as. The prompts are the
/// PaddleOCR-VL 1.5 model card's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum VlTask {
    /// Plain text, one line per output line.
    #[default]
    Ocr,
    /// A table, answered in OTSL (`super::otsl`).
    Table,
    /// A formula, answered in LaTeX.
    Formula,
    /// A chart, answered as a Markdown-style data table.
    Chart,
    /// A seal or stamp, answered as its text.
    Seal,
}

impl VlTask {
    /// The task's prompt text, e.g. `"Table Recognition:"`.
    #[must_use]
    pub fn prompt_text(self) -> &'static str {
        match self {
            Self::Ocr => "OCR:",
            Self::Table => "Table Recognition:",
            Self::Formula => "Formula Recognition:",
            Self::Chart => "Chart Recognition:",
            Self::Seal => "Seal Recognition:",
        }
    }
}
/// The end-of-sequence token's text.
pub const END_OF_SEQUENCE: &str = "</s>";

/// Why an image or prompt could not be prepared.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PrepError {
    /// The image has a zero side.
    #[error("the image is empty")]
    Empty,
    /// The pixel buffer length is not `width * height`.
    #[error("pixel buffer is {got} bytes; a {width}x{height} grey image needs {want}")]
    BufferLength {
        /// Bytes supplied.
        got: usize,
        /// Bytes required.
        want: usize,
        /// Image width.
        width: u32,
        /// Image height.
        height: u32,
    },
    /// More than [`MAX_INPUT_PIXELS`].
    #[error("the image has {0} pixels; the limit is {MAX_INPUT_PIXELS}")]
    TooLarge(usize),
    /// Long side over short side exceeds [`MAX_ASPECT`].
    #[error("the image's aspect ratio exceeds {MAX_ASPECT}:1")]
    Aspect,
    /// The tokenizer lacks a token the prompt needs.
    #[error("the tokenizer has no `{0}` token")]
    MissingToken(&'static str),
    /// The tokenizer refused the prompt text.
    #[error("the tokenizer refused the prompt: {0}")]
    Tokenizer(String),
}

/// A greyscale image, row-major, one byte per pixel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grey {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// `width * height` bytes.
    pub pixels: Vec<u8>,
}

/// Check a caller's buffer and wrap it.
///
/// # Errors
///
/// [`PrepError::Empty`], [`PrepError::TooLarge`] or
/// [`PrepError::BufferLength`].
pub fn grey(width: u32, height: u32, pixels: &[u8]) -> Result<Grey, PrepError> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 {
        return Err(PrepError::Empty);
    }
    let want = w.checked_mul(h).ok_or(PrepError::TooLarge(usize::MAX))?;
    if want > MAX_INPUT_PIXELS {
        return Err(PrepError::TooLarge(want));
    }
    if pixels.len() != want {
        return Err(PrepError::BufferLength {
            got: pixels.len(),
            want,
            width,
            height,
        });
    }
    Ok(Grey {
        width: w,
        height: h,
        pixels: pixels.to_vec(),
    })
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)] // bounded by MAX_INPUT_PIXELS, far inside f64's exact range
fn round_to(v: f64, f: impl Fn(f64) -> f64) -> usize {
    (f(v / FACTOR as f64) as usize).max(1) * FACTOR
}

/// The resized `(height, width)` the model sees, as its processor computes
/// it (Python rounding: ties to even).
///
/// # Errors
///
/// [`PrepError::Empty`] or [`PrepError::Aspect`].
#[allow(clippy::cast_precision_loss)] // sides are bounded by MAX_INPUT_PIXELS
pub fn smart_resize(height: usize, width: usize) -> Result<(usize, usize), PrepError> {
    if height == 0 || width == 0 {
        return Err(PrepError::Empty);
    }
    let (mut h, mut w) = (height as f64, width as f64);
    let f = FACTOR as f64;
    if h < f {
        w = (w * f / h).round_ties_even();
        h = f;
    }
    if w < f {
        h = (h * f / w).round_ties_even();
        w = f;
    }
    if h.max(w) / h.min(w) > MAX_ASPECT {
        return Err(PrepError::Aspect);
    }
    let (mut hb, mut wb) = (
        round_to(h, f64::round_ties_even),
        round_to(w, f64::round_ties_even),
    );
    if hb * wb > MAX_PIXELS {
        let beta = (h * w / MAX_PIXELS as f64).sqrt();
        (hb, wb) = (
            round_to(h / beta, f64::floor),
            round_to(w / beta, f64::floor),
        );
    } else if hb * wb < MIN_PIXELS {
        let beta = (MIN_PIXELS as f64 / (h * w)).sqrt();
        (hb, wb) = (round_to(h * beta, f64::ceil), round_to(w * beta, f64::ceil));
    }
    Ok((hb, wb))
}

/// PIL's bicubic kernel (a = -0.5).
fn cubic(x: f64) -> f64 {
    let a = -0.5;
    let x = x.abs();
    if x < 1.0 {
        ((a + 2.0) * x - (a + 3.0)) * x * x + 1.0
    } else if x < 2.0 {
        (((x - 5.0) * x + 8.0) * x - 4.0) * a
    } else {
        0.0
    }
}

/// Per output index: the first input index and the normalised weights, as
/// PIL's `precompute_coeffs` builds them (support widens when shrinking).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)] // sizes bounded by MAX_INPUT_PIXELS
fn coeffs(in_len: usize, out_len: usize) -> Vec<(usize, Vec<f64>)> {
    let scale = in_len as f64 / out_len as f64;
    let fscale = scale.max(1.0);
    let support = 2.0 * fscale;
    (0..out_len)
        .map(|i| {
            let center = (i as f64 + 0.5) * scale;
            let lo = (center - support + 0.5).floor().max(0.0) as usize;
            let hi = ((center + support + 0.5).floor() as usize).min(in_len);
            let mut w: Vec<f64> = (lo..hi)
                .map(|j| cubic((j as f64 - center + 0.5) / fscale))
                .collect();
            let sum: f64 = w.iter().sum();
            if sum != 0.0 {
                w.iter_mut().for_each(|v| *v /= sum);
            }
            (lo, w)
        })
        .collect()
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 0..=255
fn to_u8(v: f64) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

/// One separable pass along rows (`horizontal`) or columns, PIL-style with a
/// clamp to `u8` after the pass.
fn resample_pass(img: &Grey, out_len: usize, horizontal: bool) -> Grey {
    let (in_len, lines) = if horizontal {
        (img.width, img.height)
    } else {
        (img.height, img.width)
    };
    let cs = coeffs(in_len, out_len);
    let (ow, oh) = if horizontal {
        (out_len, img.height)
    } else {
        (img.width, out_len)
    };
    let mut out = vec![0u8; ow * oh];
    for line in 0..lines {
        for (o, (lo, ws)) in cs.iter().enumerate() {
            let v: f64 = ws
                .iter()
                .enumerate()
                .map(|(k, w)| {
                    let (x, y) = if horizontal {
                        (lo + k, line)
                    } else {
                        (line, lo + k)
                    };
                    w * f64::from(img.pixels.get(y * img.width + x).copied().unwrap_or(0))
                })
                .sum();
            let (x, y) = if horizontal { (o, line) } else { (line, o) };
            if let Some(p) = out.get_mut(y * ow + x) {
                *p = to_u8(v);
            }
        }
    }
    Grey {
        width: ow,
        height: oh,
        pixels: out,
    }
}

/// Bicubic resize to `width x height`, horizontal pass first as PIL does.
#[must_use]
pub fn resize(img: &Grey, width: usize, height: usize) -> Grey {
    let mut cur = img.clone();
    if width != cur.width {
        cur = resample_pass(&cur, width, true);
    }
    if height != cur.height {
        cur = resample_pass(&cur, height, false);
    }
    cur
}

/// The vision encoder's `pixel_values`: `[gh * gw, 3, 14, 14]` flattened,
/// grey replicated to three channels, normalised to -1..=1, plus
/// `(gh, gw)`. `img` sides must be multiples of [`FACTOR`].
#[must_use]
pub fn patchify(img: &Grey) -> (Vec<f32>, usize, usize) {
    let (gh, gw) = (img.height / PATCH, img.width / PATCH);
    let per = 3 * PATCH * PATCH;
    let mut out = vec![0f32; gh * gw * per];
    for (i, &p) in img.pixels.iter().enumerate() {
        let (y, x) = (i / img.width, i % img.width);
        let v = (f32::from(p) / 255.0 - 0.5) / 0.5;
        let base = ((y / PATCH) * gw + x / PATCH) * per + (y % PATCH) * PATCH + x % PATCH;
        for c in 0..3 {
            if let Some(slot) = out.get_mut(base + c * PATCH * PATCH) {
                *slot = v;
            }
        }
    }
    (out, gh, gw)
}

/// Image tokens the encoder emits for a `gh x gw` patch grid.
#[must_use]
pub fn image_tokens(gh: usize, gw: usize) -> usize {
    gh * gw / (MERGE * MERGE)
}

/// The decoder prompt: token ids, and where the image tokens start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// Prefix, `n` placeholders, suffix.
    pub ids: Vec<u32>,
    /// Index of the first placeholder.
    pub image_at: usize,
    /// End-of-sequence token id.
    pub eos: u32,
}

/// Assemble the OCR prompt around `n_image` placeholders:
/// [`prompt_for`] with [`VlTask::Ocr`].
///
/// # Errors
///
/// As [`prompt_for`].
pub fn prompt(tok: &VlTokenizer, n_image: usize) -> Result<Prompt, PrepError> {
    prompt_for(tok, n_image, VlTask::Ocr)
}

/// Assemble `task`'s prompt around `n_image` placeholders.
///
/// # Errors
///
/// [`PrepError::MissingToken`] when the tokenizer lacks one of the special
/// tokens (the prompt would silently tokenise as plain text);
/// [`PrepError::Tokenizer`] if encoding fails.
pub fn prompt_for(tok: &VlTokenizer, n_image: usize, task: VlTask) -> Result<Prompt, PrepError> {
    for name in ["<|begin_of_sentence|>", "<|IMAGE_START|>", "<|IMAGE_END|>"] {
        tok.token_id(name).ok_or(PrepError::MissingToken(name))?;
    }
    let image = tok
        .token_id(IMAGE_PLACEHOLDER)
        .ok_or(PrepError::MissingToken(IMAGE_PLACEHOLDER))?;
    let eos = tok
        .token_id(END_OF_SEQUENCE)
        .ok_or(PrepError::MissingToken(END_OF_SEQUENCE))?;
    let enc = |t: &str| {
        tok.encode(t)
            .map_err(|e| PrepError::Tokenizer(e.to_string()))
    };
    let mut ids = enc(PROMPT_PREFIX)?;
    let image_at = ids.len();
    ids.extend(std::iter::repeat_n(image, n_image));
    ids.extend(enc(&format!(
        "<|IMAGE_END|>{}\nAssistant:\n",
        task.prompt_text()
    ))?);
    Ok(Prompt { ids, image_at, eos })
}

/// Inclusive-exclusive pixel box `(x0, y0, x1, y1)`.
pub type PixelBox = (usize, usize, usize, usize);

/// The bounding box of ink pixels, or `None` for a blank image.
#[must_use]
pub fn ink_bbox(img: &Grey) -> Option<PixelBox> {
    let mut b: Option<PixelBox> = None;
    for (i, &p) in img.pixels.iter().enumerate() {
        if p < INK_BELOW {
            let (y, x) = (i / img.width, i % img.width);
            b = Some(match b {
                None => (x, y, x + 1, y + 1),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
            });
        }
    }
    b
}

/// Copy out `bx`, widened by `margin` and clamped to the image.
#[must_use]
pub fn crop(img: &Grey, bx: PixelBox, margin: usize) -> (Grey, PixelBox) {
    let x0 = bx.0.saturating_sub(margin);
    let y0 = bx.1.saturating_sub(margin);
    let x1 = (bx.2 + margin).min(img.width);
    let y1 = (bx.3 + margin).min(img.height);
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    let mut pixels = Vec::with_capacity(w * h);
    for y in y0..y1 {
        let row = img.pixels.get(y * img.width + x0..y * img.width + x1);
        pixels.extend_from_slice(row.unwrap_or(&[]));
    }
    (
        Grey {
            width: w,
            height: h,
            pixels,
        },
        (x0, y0, x1, y1),
    )
}

/// How decoded lines were given positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LinePlacement {
    /// One line per band of inked rows: the counts matched.
    Bands,
    /// The counts differed, so the lines divide the ink box evenly.
    Even,
    /// Nothing was read.
    None,
}

/// Maximal runs of rows inside `bx` holding ink, as `(y0, y1, x0, x1)`.
fn ink_bands(img: &Grey, bx: PixelBox) -> Vec<PixelBox> {
    let mut bands: Vec<PixelBox> = Vec::new();
    let mut open: Option<(usize, usize, usize)> = None;
    for y in bx.1..=bx.3 {
        let row = img
            .pixels
            .get(y * img.width + bx.0..y * img.width + bx.2)
            .unwrap_or(&[]);
        let first = row.iter().position(|&p| p < INK_BELOW);
        let last = row.iter().rposition(|&p| p < INK_BELOW);
        match (first, last, open) {
            (Some(a), Some(b), None) => open = Some((y, bx.0 + a, bx.0 + b + 1)),
            (Some(a), Some(b), Some((y0, x0, x1))) => {
                open = Some((y0, x0.min(bx.0 + a), x1.max(bx.0 + b + 1)));
            }
            (_, _, Some((y0, x0, x1))) => {
                bands.push((x0, y0, x1, y));
                open = None;
            }
            _ => {}
        }
    }
    bands
}

/// Place each non-empty line of `text` in image pixels (y down): on the ink
/// bands of `bx` when there is one band per line, else on even slices of
/// `bx`. Every line carries `confidence`.
#[must_use]
#[allow(clippy::cast_precision_loss)] // pixel coordinates
pub fn place_lines(
    img: &Grey,
    bx: PixelBox,
    text: &str,
    confidence: Option<f32>,
) -> (Vec<RecognizedWord>, LinePlacement) {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return (Vec::new(), LinePlacement::None);
    }
    let bands = ink_bands(img, bx);
    let (boxes, how) = if bands.len() == lines.len() {
        (bands, LinePlacement::Bands)
    } else {
        let n = lines.len();
        let h = (bx.3 - bx.1) as f64 / n as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let at = |i: usize| bx.1 + (h * i as f64).round() as usize;
        // More lines than ink rows gives one-row slices; keep them in `bx`.
        let last = bx.3.saturating_sub(1).max(bx.1);
        let even = (0..n).map(|i| {
            let y0 = at(i).min(last);
            (bx.0, y0, bx.2, at(i + 1).max(y0 + 1).min(bx.3.max(y0 + 1)))
        });
        (even.collect(), LinePlacement::Even)
    };
    let words = lines
        .iter()
        .zip(boxes)
        .map(|(l, (x0, y0, x1, y1))| RecognizedWord {
            text: (*l).to_owned(),
            rect: Rect::from_corners(x0 as f64, y0 as f64, x1 as f64, y1 as f64),
            confidence,
        })
        .collect();
    (words, how)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn smart_resize_matches_the_reference_processor() {
        // Values from the model's own `smart_resize` (Python).
        assert_eq!(smart_resize(100, 300).unwrap(), (196, 588));
        assert_eq!(smart_resize(3300, 2550).unwrap(), (1120, 868));
        assert_eq!(smart_resize(10, 1000).unwrap(), (56, 3360));
        assert_eq!(smart_resize(28, 5000).unwrap(), (28, 5012));
        assert_eq!(smart_resize(4000, 3000).unwrap(), (1148, 840));
        assert_eq!(smart_resize(7, 7).unwrap(), (336, 336));
        assert_eq!(smart_resize(42, 42).unwrap(), (336, 336));
        // 350 / 28 = 12.5: Python rounds the tie to even.
        assert_eq!(smart_resize(350, 700).unwrap(), (336, 700));
        assert_eq!(smart_resize(700, 350).unwrap(), (700, 336));
        assert_eq!(smart_resize(1, 300), Err(PrepError::Aspect));
        assert_eq!(smart_resize(0, 5), Err(PrepError::Empty));
        for (h, w) in [(28, 5000), (7, 7), (4000, 3000), (999, 1)] {
            if let Ok((rh, rw)) = smart_resize(h, w) {
                assert_eq!((rh % FACTOR, rw % FACTOR), (0, 0));
                assert!(rh * rw <= MAX_PIXELS && rh * rw >= MIN_PIXELS, "{h}x{w}");
            }
        }
    }

    #[test]
    fn grey_checks_its_buffer() {
        assert!(matches!(
            grey(2, 2, &[0; 3]),
            Err(PrepError::BufferLength { want: 4, .. })
        ));
        assert_eq!(grey(0, 2, &[]), Err(PrepError::Empty));
        assert_eq!(
            grey(1 << 14, 1 << 13, &[]),
            Err(PrepError::TooLarge(1 << 27))
        );
    }

    #[test]
    fn bicubic_resize_matches_pil() {
        // Values from Pillow's `Image.resize(.., Image.BICUBIC)` on mode "L".
        let img = grey(4, 1, &[0, 255, 0, 255]).unwrap();
        assert_eq!(resize(&img, 2, 1).pixels, [107, 148]);
        let px = [
            0, 50, 100, 200, 255, 10, 20, 30, 40, 50, 255, 0, 255, 0, 128,
        ];
        let img5 = grey(5, 3, &px).unwrap();
        assert_eq!(
            resize(&img5, 7, 4).pixels,
            [
                0, 26, 65, 104, 180, 241, 255, 0, 13, 36, 40, 86, 115, 121, 101, 44, 31, 109, 36,
                30, 68, 255, 99, 52, 255, 61, 39, 139
            ]
        );
        assert_eq!(resize(&img5, 2, 2).pixels, [30, 140, 101, 72]);
        let flat = grey(3, 3, &[77; 9]).unwrap();
        assert!(
            resize(&flat, 7, 5).pixels.iter().all(|&p| p == 77),
            "flat stays flat"
        );
        assert_eq!(resize(&img, 4, 1), img, "same size is the identity");
    }

    #[test]
    fn patches_are_grid_major_with_replicated_channels() {
        let mut px = vec![255u8; 28 * 28];
        px[14 * 28 + 15] = 0; // patch (row 1, col 1), offset (0, 1)
        let (v, gh, gw) = patchify(&grey(28, 28, &px).unwrap());
        assert_eq!((gh, gw, v.len()), (2, 2, 4 * 3 * 196));
        let base = 3 * 196 * 3 + 1;
        for c in 0..3 {
            assert_eq!(v[base + c * 196], -1.0);
        }
        assert_eq!(v.iter().filter(|&&x| x < 0.0).count(), 3);
        assert_eq!(v[0], 1.0);
        assert_eq!(image_tokens(gh, gw), 1);
    }

    fn tokenizer() -> VlTokenizer {
        let json = r#"{"added_tokens": [
            {"id": 2, "content": "</s>", "special": true},
            {"id": 10, "content": "<|begin_of_sentence|>", "special": true},
            {"id": 11, "content": "<|IMAGE_START|>", "special": true},
            {"id": 12, "content": "<|IMAGE_END|>", "special": true},
            {"id": 13, "content": "<|IMAGE_PLACEHOLDER|>", "special": true}],
          "normalizer": null, "pre_tokenizer": null, "decoder": null,
          "model": {"type": "BPE", "byte_fallback": true, "vocab": {
            "<0x0A>": 20, "<0x20>": 21, "<0x3A>": 22, "U": 30, "s": 31, "e": 32,
            "r": 33, "O": 34, "C": 35, "R": 36, "A": 37, "i": 38, "t": 39,
            "a": 40, "n": 41, ":": 42, " ": 43}, "merges": []}}"#;
        VlTokenizer::from_json_bytes(json.as_bytes()).unwrap()
    }

    #[test]
    fn prompt_wraps_the_placeholders() {
        let p = prompt(&tokenizer(), 3).unwrap();
        assert_eq!(p.ids[..2], [10, 30]);
        assert_eq!(p.image_at, 8, "<bos> U s e r : space <IMAGE_START>");
        assert_eq!(p.ids[p.image_at - 1], 11);
        assert_eq!(p.ids[p.image_at..p.image_at + 4], [13, 13, 13, 12]);
        assert_eq!(p.ids.last(), Some(&20));
        assert_eq!(p.eos, 2);
    }

    #[test]
    fn prompt_refuses_a_tokenizer_without_the_special_tokens() {
        let json = r#"{"model": {"type": "BPE", "vocab": {"a": 0}, "merges": []}}"#;
        let tok = VlTokenizer::from_json_bytes(json.as_bytes()).unwrap();
        assert_eq!(
            prompt(&tok, 1),
            Err(PrepError::MissingToken("<|begin_of_sentence|>"))
        );
    }

    /// Two dark text rows in a white 20x20 image.
    fn two_lines() -> Grey {
        let mut px = vec![255u8; 400];
        for x in 3..15 {
            px[4 * 20 + x] = 0;
            px[5 * 20 + x] = 0;
        }
        for x in 5..10 {
            px[12 * 20 + x] = 0;
        }
        grey(20, 20, &px).unwrap()
    }

    #[test]
    fn ink_box_and_crop() {
        let img = two_lines();
        let bx = ink_bbox(&img).unwrap();
        assert_eq!(bx, (3, 4, 15, 13));
        let (c, at) = crop(&img, bx, 2);
        assert_eq!(at, (1, 2, 17, 15));
        assert_eq!((c.width, c.height, c.pixels.len()), (16, 13, 208));
        assert_eq!(ink_bbox(&grey(2, 2, &[255; 4]).unwrap()), None);
    }

    #[test]
    fn lines_land_on_bands_when_the_counts_match() {
        let img = two_lines();
        let bx = ink_bbox(&img).unwrap();
        let (w, how) = place_lines(&img, bx, "first\n\n second \n", Some(0.9));
        assert_eq!(how, LinePlacement::Bands);
        assert_eq!(w.len(), 2);
        assert_eq!(
            (w[0].text.as_str(), w[1].text.as_str()),
            ("first", "second")
        );
        assert_eq!(w[0].rect, Rect::from_corners(3.0, 4.0, 15.0, 6.0));
        assert_eq!(w[1].rect, Rect::from_corners(5.0, 12.0, 10.0, 13.0));
        assert_eq!(w[1].confidence, Some(0.9));
    }

    #[test]
    fn lines_split_the_box_evenly_otherwise() {
        let img = two_lines();
        let bx = ink_bbox(&img).unwrap();
        let (w, how) = place_lines(&img, bx, "a\nb\nc", None);
        assert_eq!(how, LinePlacement::Even);
        assert_eq!(w[0].rect, Rect::from_corners(3.0, 4.0, 15.0, 7.0));
        assert_eq!(w[2].rect, Rect::from_corners(3.0, 10.0, 15.0, 13.0));
        assert_eq!(place_lines(&img, bx, " \n", None).1, LinePlacement::None);
    }

    #[test]
    fn more_lines_than_ink_rows_stay_inside_the_image() {
        let img = grey(4, 2, &[255, 255, 255, 255, 0, 0, 0, 0]).unwrap();
        let bx = ink_bbox(&img).unwrap();
        let (w, how) = place_lines(&img, bx, "a\nb\nc", None);
        assert_eq!(how, LinePlacement::Even);
        for l in &w {
            assert!(l.rect.lly >= 1.0 && l.rect.ury <= 2.0, "{:?}", l.rect);
        }
    }
}

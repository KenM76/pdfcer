//! PP-OCR (PaddleOCR) pre- and post-processing: the pure half of the
//! [`engine_paddle`](super::engine_paddle) recogniser.
//!
//! Always compiled, with no model runtime, so the fuzz harness (which builds
//! `pdfcer-core` without default features) reaches every branch that reads
//! model output. The ONNX models are operator-supplied, so their outputs are
//! untrusted: every function here checks shapes and tolerates NaN.
//!
//! Mirrors RapidOCR's defaults (`rapidocr_onnxruntime` 1.4 `config.yaml`):
//! detection normalises to `[-1, 1]`, resizes the short side up to 736 and
//! the long side down to 2000 in multiples of 32, thresholds the probability
//! map at 0.3, dilates 2×2, keeps boxes scoring ≥ 0.5 and unclips by 1.6;
//! recognition takes 48-pixel-high crops padded to at least 320 wide and
//! greedy-decodes CTC with blank at class 0 and a space appended after the
//! dictionary. One departure: boxes are **axis-aligned** bounding boxes of
//! the thresholded components rather than `minAreaRect` rotated boxes, so a
//! line skewed by more than a few degrees is cropped with some of its
//! neighbours.

use crate::ocr::RecognizedWord;
use crate::page_tree::Rect;

/// Probability above which a detection-map pixel is text.
const THRESH: f32 = 0.3;
/// Mean probability a box needs to be kept.
const BOX_THRESH: f32 = 0.5;
/// How far a kept box is grown: `area × ratio / perimeter` on every side.
const UNCLIP_RATIO: f32 = 1.6;
/// Components examined per page; the rest are ignored.
const MAX_CANDIDATES: usize = 1000;
/// A box whose short side is below this (map pixels) is noise.
const MIN_SIZE: f32 = 3.0;
/// Detection input: the short side is scaled up to this…
const DET_MIN_SIDE: f32 = 736.0;
/// …unless that makes the long side exceed this.
const DET_MAX_SIDE: f32 = 2000.0;
/// Recognition input height.
pub const REC_HEIGHT: u32 = 48;
/// Recognition input width is at least this (padded).
const REC_MIN_WIDTH: u32 = 320;
/// Recognition input width ceiling; wider crops are squeezed to it.
const REC_MAX_WIDTH: u32 = 3200;
/// A recognised line whose mean character probability is below this is
/// dropped as noise (RapidOCR's `text_score`).
pub const LINE_SCORE_MIN: f32 = 0.5;

/// A shape or dictionary mismatch between the models and what was supplied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PostError {
    /// A tensor's element count does not match its stated shape.
    #[error("model output has {actual} values where its shape needs {expected}")]
    Shape {
        /// Elements the shape implies.
        expected: usize,
        /// Elements present.
        actual: usize,
    },
    /// The recogniser's class count fits neither `dictionary + 2` (blank and
    /// space) nor `dictionary + 1` (blank only).
    #[error(
        "the recognition model has {classes} classes but the dictionary has {dictionary} entries (expected {dictionary} + 2, or + 1 without a space class) — the dictionary does not belong to this model"
    )]
    Classes {
        /// Classes in the model output.
        classes: usize,
        /// Entries in the dictionary.
        dictionary: usize,
    },
}

/// A detected text line in image pixels, y-down; `x1`/`y1` exclusive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBox {
    /// Left edge.
    pub x0: u32,
    /// Top edge.
    pub y0: u32,
    /// Right edge.
    pub x1: u32,
    /// Bottom edge.
    pub y1: u32,
    /// Mean detection probability inside the box.
    pub score: f32,
}

/// A model input: CHW `f32` data for a batch of one.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelInput {
    /// `3 × height × width` values.
    pub data: Vec<f32>,
    /// Input width.
    pub width: u32,
    /// Input height.
    pub height: u32,
    /// Columns holding the image; the rest is zero padding.
    pub content_width: u32,
}

/// One greedy-CTC character with the time step it was read at.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedChar {
    /// The dictionary entry (usually one character), or `" "`.
    pub text: String,
    /// Output time step.
    pub step: usize,
    /// The class probability.
    pub prob: f32,
}

/// Bilinear resize with OpenCV's `INTER_LINEAR` pixel-centre convention.
fn resize(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<f32> {
    let axis = |dst: u32, s: u32, d: u32| -> (usize, usize, f32) {
        let scale = s as f32 / d as f32;
        let f = (dst as f32 + 0.5) * scale - 0.5;
        let last = s.saturating_sub(1) as usize;
        if f <= 0.0 {
            return (0, 0, 0.0);
        }
        let i = f.floor() as usize;
        if i >= last {
            return (last, last, 0.0);
        }
        (i, i + 1, f - i as f32)
    };
    let cols: Vec<_> = (0..dw).map(|x| axis(x, sw, dw)).collect();
    let px = |x: usize, y: usize| f32::from(src.get(y * sw as usize + x).copied().unwrap_or(0));
    let mut out = Vec::with_capacity(dw as usize * dh as usize);
    for y in 0..dh {
        let (y0, y1, fy) = axis(y, sh, dh);
        for &(x0, x1, fx) in &cols {
            let top = px(x0, y0) * (1.0 - fx) + px(x1, y0) * fx;
            let bottom = px(x0, y1) * (1.0 - fx) + px(x1, y1) * fx;
            out.push(top * (1.0 - fy) + bottom * fy);
        }
    }
    out
}

/// Grey `0..=255` to `[-1, 1]`, replicated into three channel planes.
fn to_chw(plane: &[f32]) -> Vec<f32> {
    let norm: Vec<f32> = plane.iter().map(|v| v / 127.5 - 1.0).collect();
    let mut data = Vec::with_capacity(norm.len() * 3);
    for _ in 0..3 {
        data.extend_from_slice(&norm);
    }
    data
}

/// The detection model's input for an 8-bit greyscale page image.
///
/// `pixels` must hold `width × height` bytes; a short buffer reads as black.
#[must_use]
pub fn det_input(width: u32, height: u32, pixels: &[u8]) -> ModelInput {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let mut ratio = if w.min(h) < DET_MIN_SIDE {
        DET_MIN_SIDE / w.min(h)
    } else {
        1.0
    };
    if w.max(h) * ratio > DET_MAX_SIDE {
        ratio = DET_MAX_SIDE / w.max(h);
    }
    let to32 = |v: f32| (((v * ratio).trunc() / 32.0).round() as u32 * 32).max(32);
    let (dw, dh) = (to32(w), to32(h));
    ModelInput {
        data: to_chw(&resize(pixels, width, height, dw, dh)),
        width: dw,
        height: dh,
        content_width: dw,
    }
}

/// Text boxes from a detection probability map of `map_w × map_h`, scaled to
/// a `dest_w × dest_h` image.
///
/// # Errors
///
/// [`PostError::Shape`] when `prob` is not `map_w × map_h` long.
pub fn db_boxes(
    prob: &[f32],
    map_w: usize,
    map_h: usize,
    dest_w: u32,
    dest_h: u32,
) -> Result<Vec<TextBox>, PostError> {
    let expected = map_w.saturating_mul(map_h);
    if prob.len() != expected {
        return Err(PostError::Shape {
            expected,
            actual: prob.len(),
        });
    }
    let raw: Vec<bool> = prob.iter().map(|&p| p > THRESH).collect();
    let on = |x: usize, y: usize| raw.get(y * map_w + x).copied().unwrap_or(false);
    // 2×2 dilation, OpenCV anchor (1, 1): a pixel is set when it or its
    // left, upper or upper-left neighbour is.
    let mut bitmap = vec![false; expected];
    for y in 0..map_h {
        for x in 0..map_w {
            let set = on(x, y)
                || (x > 0 && on(x - 1, y))
                || (y > 0 && on(x, y - 1))
                || (x > 0 && y > 0 && on(x - 1, y - 1));
            if let Some(b) = bitmap.get_mut(y * map_w + x) {
                *b = set;
            }
        }
    }

    let mut seen = vec![false; expected];
    let mut stack = Vec::new();
    let mut boxes = Vec::new();
    let mut candidates = 0;
    for start in 0..expected {
        if candidates >= MAX_CANDIDATES {
            break;
        }
        if !bitmap.get(start).copied().unwrap_or(false) || seen.get(start).copied().unwrap_or(true)
        {
            continue;
        }
        candidates += 1;
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (usize::MAX, usize::MAX, 0, 0);
        stack.push(start);
        if let Some(s) = seen.get_mut(start) {
            *s = true;
        }
        while let Some(i) = stack.pop() {
            let (x, y) = (i % map_w, i / map_w);
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            for (dx, dy) in [
                (-1, -1),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ] {
                let (nx, ny) = (x as isize + dx, y as isize + dy);
                if nx < 0 || ny < 0 || nx as usize >= map_w || ny as usize >= map_h {
                    continue;
                }
                let n = ny as usize * map_w + nx as usize;
                if bitmap.get(n).copied().unwrap_or(false) && !seen.get(n).copied().unwrap_or(true)
                {
                    if let Some(s) = seen.get_mut(n) {
                        *s = true;
                    }
                    stack.push(n);
                }
            }
        }
        if let Some(b) = component_box(
            prob,
            map_w,
            map_h,
            [min_x, min_y, max_x, max_y],
            dest_w,
            dest_h,
        ) {
            boxes.push(b);
        }
    }
    Ok(boxes)
}

/// Score, filter, unclip and scale one component's pixel extent.
fn component_box(
    prob: &[f32],
    map_w: usize,
    map_h: usize,
    [min_x, min_y, max_x, max_y]: [usize; 4],
    dest_w: u32,
    dest_h: u32,
) -> Option<TextBox> {
    // Contour points are pixel centres, so a one-pixel blob has size zero.
    let (w, h) = ((max_x - min_x) as f32, (max_y - min_y) as f32);
    if w.min(h) < MIN_SIZE {
        return None;
    }
    let mut sum = 0.0_f32;
    for y in min_y..=max_y {
        let row = prob.get(y * map_w + min_x..=y * map_w + max_x)?;
        sum += row.iter().sum::<f32>();
    }
    let score = sum / ((max_x - min_x + 1) * (max_y - min_y + 1)) as f32;
    // A NaN score is dropped too.
    if score.is_nan() || score < BOX_THRESH {
        return None;
    }
    let d = w * h * UNCLIP_RATIO / (2.0 * (w + h));
    if (w + 2.0 * d).min(h + 2.0 * d) < MIN_SIZE + 2.0 {
        return None;
    }
    let sx = dest_w as f32 / map_w as f32;
    let sy = dest_h as f32 / map_h as f32;
    let clip = |v: f32, max: u32| (v.round().max(0.0) as u32).min(max);
    let b = TextBox {
        x0: clip((min_x as f32 - d) * sx, dest_w),
        y0: clip((min_y as f32 - d) * sy, dest_h),
        x1: clip((max_x as f32 + d) * sx, dest_w),
        y1: clip((max_y as f32 + d) * sy, dest_h),
        score,
    };
    (b.x1 > b.x0 && b.y1 > b.y0).then_some(b)
}

/// Sort boxes top-to-bottom, then left-to-right within 10 pixels of one
/// another vertically (PaddleOCR's `sorted_boxes`).
pub fn sort_reading_order(boxes: &mut [TextBox]) {
    boxes.sort_by_key(|b| (b.y0, b.x0));
    for i in 0..boxes.len().saturating_sub(1) {
        for j in (0..=i).rev() {
            let (Some(a), Some(b)) = (boxes.get(j), boxes.get(j + 1)) else {
                break;
            };
            if a.y0.abs_diff(b.y0) < 10 && b.x0 < a.x0 {
                boxes.swap(j, j + 1);
            } else {
                break;
            }
        }
    }
}

/// Copy `b` out of a `width`-wide greyscale image. Returns the crop and its
/// size; a crop that would be empty is `None`.
#[must_use]
pub fn crop(pixels: &[u8], width: u32, height: u32, b: &TextBox) -> Option<(Vec<u8>, u32, u32)> {
    let (x0, x1) = (b.x0.min(width), b.x1.min(width));
    let (y0, y1) = (b.y0.min(height), b.y1.min(height));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let mut out = Vec::with_capacity(((x1 - x0) * (y1 - y0)) as usize);
    for y in y0..y1 {
        let start = (y * width + x0) as usize;
        out.extend_from_slice(pixels.get(start..start + (x1 - x0) as usize)?);
    }
    Some((out, x1 - x0, y1 - y0))
}

/// Rotate a greyscale image 90° counter-clockwise (NumPy's `rot90`), for a
/// crop at least 1.5 times taller than wide.
#[must_use]
pub fn rotate_ccw(src: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let mut out = Vec::with_capacity((w * h) as usize);
    for i in 0..w {
        for j in 0..h {
            out.push(
                src.get((j * w + (w - 1 - i)) as usize)
                    .copied()
                    .unwrap_or(0),
            );
        }
    }
    (out, h, w)
}

/// The recognition model's input for one greyscale line crop.
#[must_use]
pub fn rec_input(crop: &[u8], cw: u32, ch: u32) -> ModelInput {
    let ratio = cw.max(1) as f32 / ch.max(1) as f32;
    let width = ((REC_HEIGHT as f32 * ratio) as u32).clamp(REC_MIN_WIDTH, REC_MAX_WIDTH);
    let content = ((REC_HEIGHT as f32 * ratio).ceil() as u32).clamp(1, width);
    let plane = resize(crop, cw.max(1), ch.max(1), content, REC_HEIGHT);
    let mut padded = vec![127.5_f32; (width * REC_HEIGHT) as usize];
    for (row, src) in padded
        .chunks_mut(width as usize)
        .zip(plane.chunks(content as usize))
    {
        if let Some(dst) = row.get_mut(..src.len()) {
            dst.copy_from_slice(src);
        }
    }
    ModelInput {
        data: to_chw(&padded),
        width,
        height: REC_HEIGHT,
        content_width: content,
    }
}

/// The dictionary entries in a `dict.txt` or an embedded `character` list:
/// one entry per line.
#[must_use]
pub fn parse_dictionary(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

/// Whether a recogniser with `classes` outputs has a trailing space class for
/// a dictionary of `dictionary` entries.
///
/// # Errors
///
/// [`PostError::Classes`] when the counts do not fit.
pub fn space_class(dictionary: usize, classes: usize) -> Result<bool, PostError> {
    if classes == dictionary.saturating_add(2) {
        Ok(true)
    } else if classes == dictionary.saturating_add(1) {
        Ok(false)
    } else {
        Err(PostError::Classes {
            classes,
            dictionary,
        })
    }
}

/// Greedy CTC decode of `steps × classes` probabilities: best class per step,
/// repeats collapsed, blanks (class 0) dropped.
///
/// # Errors
///
/// [`PostError::Shape`] or [`PostError::Classes`].
pub fn ctc_decode(
    probs: &[f32],
    steps: usize,
    classes: usize,
    dictionary: &[String],
) -> Result<Vec<DecodedChar>, PostError> {
    let expected = steps.saturating_mul(classes);
    if probs.len() != expected {
        return Err(PostError::Shape {
            expected,
            actual: probs.len(),
        });
    }
    let space = space_class(dictionary.len(), classes)?;
    let mut out = Vec::new();
    let mut prev = None;
    for (step, row) in probs.chunks(classes.max(1)).enumerate() {
        let mut best = (0, f32::NEG_INFINITY);
        for (i, &p) in row.iter().enumerate() {
            if p > best.1 {
                best = (i, p);
            }
        }
        let (idx, prob) = best;
        if idx != 0 && prev != Some(idx) {
            let text = match dictionary.get(idx - 1) {
                Some(t) => t.clone(),
                None if space => " ".to_owned(),
                None => String::new(),
            };
            out.push(DecodedChar { text, step, prob });
        }
        prev = Some(idx);
    }
    Ok(out)
}

/// Mean probability of a decoded line, `0.0` when empty.
#[must_use]
pub fn line_score(chars: &[DecodedChar]) -> f32 {
    if chars.is_empty() {
        return 0.0;
    }
    chars.iter().map(|c| c.prob).sum::<f32>() / chars.len() as f32
}

fn confidence(probs: &[f32]) -> f32 {
    let m = probs.iter().sum::<f32>() / probs.len().max(1) as f32;
    if m.is_finite() {
        m.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Split a decoded line into words at spaces, placing each word from the time
/// steps it was read at. `input` is the recognition input the line came
/// from, `crop` the box it was cut from.
#[must_use]
pub fn words_from_line(
    chars: &[DecodedChar],
    steps: usize,
    input: &ModelInput,
    crop: &TextBox,
) -> Vec<RecognizedWord> {
    let crop_w = crop.x1.saturating_sub(crop.x0) as f32;
    let per_step = input.width as f32 / steps.max(1) as f32;
    let scale = crop_w / input.content_width.max(1) as f32;
    let x_at = |step: usize| crop.x0 as f32 + (step as f32 * per_step * scale).clamp(0.0, crop_w);
    let mut words = Vec::new();
    let mut current: Option<(usize, usize, String, Vec<f32>)> = None;
    let mut flush = |w: Option<(usize, usize, String, Vec<f32>)>| {
        if let Some((first, last, text, probs)) = w {
            let (x0, x1) = (x_at(first), x_at(last + 1));
            words.push(RecognizedWord {
                text,
                rect: Rect::from_corners(
                    f64::from(x0),
                    f64::from(crop.y0),
                    f64::from(x1.max(x0 + 1.0)),
                    f64::from(crop.y1),
                ),
                confidence: Some(confidence(&probs)),
            });
        }
    };
    for c in chars {
        if c.text.trim().is_empty() {
            flush(current.take());
            continue;
        }
        let w = current.get_or_insert_with(|| (c.step, c.step, String::new(), Vec::new()));
        w.1 = c.step;
        w.2.push_str(&c.text);
        w.3.push(c.prob);
    }
    flush(current);
    words
}

/// A whole decoded line as one word covering `crop` — for a rotated crop,
/// whose time steps do not map onto the page's x axis.
#[must_use]
pub fn line_as_word(chars: &[DecodedChar], crop: &TextBox) -> Option<RecognizedWord> {
    let text: String = chars.iter().map(|c| c.text.as_str()).collect();
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let probs: Vec<f32> = chars.iter().map(|c| c.prob).collect();
    Some(RecognizedWord {
        text: text.to_owned(),
        rect: Rect::from_corners(
            f64::from(crop.x0),
            f64::from(crop.y0),
            f64::from(crop.x1),
            f64::from(crop.y1),
        ),
        confidence: Some(confidence(&probs)),
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn dict(s: &str) -> Vec<String> {
        s.chars().map(String::from).collect()
    }

    /// One-hot rows: `seq` lists the winning class per step.
    fn onehot(seq: &[usize], classes: usize) -> Vec<f32> {
        let mut v = vec![0.0; seq.len() * classes];
        for (t, &c) in seq.iter().enumerate() {
            v[t * classes + c] = 0.9;
        }
        v
    }

    #[test]
    fn ctc_collapses_repeats_drops_blanks_and_maps_the_space_class() {
        let d = dict("ab");
        // classes: 0 blank, 1 'a', 2 'b', 3 space
        let p = onehot(&[1, 1, 0, 1, 3, 2, 2, 0], 4);
        let out = ctc_decode(&p, 8, 4, &d).unwrap();
        let text: String = out.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(text, "aa b");
        assert_eq!(out.iter().map(|c| c.step).collect::<Vec<_>>(), [0, 3, 4, 5]);
    }

    #[test]
    fn a_dictionary_that_does_not_fit_the_model_is_named() {
        assert_eq!(space_class(6623, 6625), Ok(true));
        assert_eq!(space_class(95, 96), Ok(false));
        assert_eq!(
            space_class(10, 20),
            Err(PostError::Classes {
                classes: 20,
                dictionary: 10
            })
        );
        assert!(matches!(
            ctc_decode(&[0.0; 5], 2, 3, &dict("a")),
            Err(PostError::Shape {
                expected: 6,
                actual: 5
            })
        ));
    }

    #[test]
    fn a_solid_block_becomes_one_unclipped_box_scaled_to_the_image() {
        let (w, h) = (40, 20);
        let mut prob = vec![0.0; w * h];
        for y in 5..15 {
            for x in 10..30 {
                prob[y * w + x] = 0.9;
            }
        }
        let boxes = db_boxes(&prob, w, h, 80, 40).unwrap();
        assert_eq!(boxes.len(), 1);
        let b = boxes[0];
        // Dilation adds one pixel right/down; unclip grows every side.
        assert!(b.x0 < 20 && b.x1 > 60 && b.y0 < 10 && b.y1 > 30, "{b:?}");
        assert!(
            b.score > 0.7,
            "the dilated rim lowers the mean: {}",
            b.score
        );
    }

    #[test]
    fn noise_and_nan_produce_no_boxes() {
        let (w, h) = (10, 10);
        let mut prob = vec![f32::NAN; w * h];
        prob[55] = 0.9; // a single pixel: below MIN_SIZE
        assert!(db_boxes(&prob, w, h, 10, 10).unwrap().is_empty());
        assert!(db_boxes(&prob, w, h + 1, 10, 10).is_err());
    }

    #[test]
    fn reading_order_puts_a_slightly_higher_right_box_after_its_left_neighbour() {
        let b = |x0, y0| TextBox {
            x0,
            y0,
            x1: x0 + 5,
            y1: y0 + 5,
            score: 1.0,
        };
        let mut v = vec![b(100, 50), b(10, 55), b(10, 10)];
        sort_reading_order(&mut v);
        assert_eq!(
            v.iter().map(|b| (b.x0, b.y0)).collect::<Vec<_>>(),
            [(10, 10), (10, 55), (100, 50)]
        );
    }

    #[test]
    fn words_are_placed_from_their_time_steps() {
        let d = dict("ab");
        let p = onehot(&[1, 0, 3, 0, 2, 0, 0, 0], 4);
        let chars = ctc_decode(&p, 8, 4, &d).unwrap();
        let input = ModelInput {
            data: Vec::new(),
            width: 320,
            height: 48,
            content_width: 320,
        };
        let crop = TextBox {
            x0: 100,
            y0: 10,
            x1: 420,
            y1: 58,
            score: 1.0,
        };
        let words = words_from_line(&chars, 8, &input, &crop);
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "a");
        assert_eq!(words[1].text, "b");
        assert!(
            (words[0].rect.llx - 100.0).abs() < 1e-6 && (words[0].rect.urx - 140.0).abs() < 1e-6
        );
        assert!((words[1].rect.llx - 260.0).abs() < 1e-6);
        assert_eq!(words[0].confidence, Some(0.9));
    }

    #[test]
    fn inputs_have_the_shapes_the_models_expect() {
        let det = det_input(100, 50, &[255; 5000]);
        assert_eq!((det.width % 32, det.height % 32), (0, 0));
        assert_eq!(det.data.len(), (3 * det.width * det.height) as usize);
        assert!(det.data.iter().all(|v| (v - 1.0).abs() < 1e-6));

        let rec = rec_input(&[0; 200], 20, 10);
        assert_eq!((rec.width, rec.height, rec.content_width), (320, 48, 96));
        assert_eq!(rec.data.len(), (3 * 320 * 48) as usize);
        assert!((rec.data[0] + 1.0).abs() < 1e-6, "ink is -1");
        assert!(rec.data[200].abs() < 1e-6, "padding is 0");
    }

    #[test]
    fn crop_and_rotate_keep_the_pixels() {
        let img: Vec<u8> = (0..12).collect(); // 4 x 3
        let b = TextBox {
            x0: 1,
            y0: 1,
            x1: 3,
            y1: 3,
            score: 1.0,
        };
        let (c, w, h) = crop(&img, 4, 3, &b).unwrap();
        assert_eq!((c.as_slice(), w, h), (&[5, 6, 9, 10][..], 2, 2));
        let (r, rw, rh) = rotate_ccw(&[1, 2, 3, 4, 5, 6], 3, 2);
        assert_eq!((r.as_slice(), rw, rh), (&[3, 6, 2, 5, 1, 4][..], 2, 3));
    }
}

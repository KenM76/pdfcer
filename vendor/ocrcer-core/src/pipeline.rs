//! `Engine`: the public entry point, wiring binarize → deskew → label →
//! lines → words → segmentation lattice → match → decode → confidence.
//!
//! # Contract
//!
//! [`Engine::from_bytes`] loads a `.ocrw` recogniser. Every guard it can fail
//! is in `crate::Error`, and a mismatched feature version or charset is
//! refused rather than read, because a mismatched pair does not fail — it
//! answers wrongly.
//!
//! [`Engine::recognize`] is total on a well-formed image: it returns the words
//! it found, possibly none. It never returns an error for a blank or
//! unreadable page, because "no text here" is an answer and not a fault.
//!
//! [`Engine::recognize_lines`] is the same work with the line grouping kept,
//! for a caller that needs reading order or a line-level confidence.
//!
//! # Coordinates
//!
//! Boxes are in the coordinates of the image that was handed in, y-down. When
//! the page was deskewed, the reported x is the deskewed x and the y is
//! mapped back through the shear, so a box still sits on the ink the caller
//! can see. The shear is vertical only, so x needs no correction at all.
//!
//! # What confidence means here
//!
//! A character's confidence is the match margin — the distance to the best
//! rival of a *different* class over the distance to the winner — pushed
//! through the authored calibration curve. A word's is the geometric mean
//! over its characters, and a line's the geometric mean over its words
//! weighted by character count. Two classes that match equally well report a
//! low confidence even when both matched well, which is the thing a reviewer
//! needs told (`CLAUDE.md` rule 5).

use crate::confidence;
use crate::decode::viterbi::{self, Cand, Hyp, Tables, WordLattice};
use crate::image::{binarize, components, deskew};
use crate::layout::{lines, segment, underline, words};
use crate::ocrw::Model;
use crate::Error;

/// A recognised word, in image pixel coordinates, y-down.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub rect: Rect,
    /// 0.0..=1.0, geometric mean over `chars`.
    pub confidence: f32,
    pub chars: Vec<CharBox>,
}

/// A single recognised character within a `Word`.
#[derive(Debug, Clone, PartialEq)]
pub struct CharBox {
    pub ch: char,
    pub rect: Rect,
    pub confidence: f32,
}

/// A recognised line of text: the words on it, left to right.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub words: Vec<Word>,
    pub rect: Rect,
    /// The line's baseline, in the same image coordinates as `rect`. Measured
    /// from the histogram of component bottom edges, not assumed from the box:
    /// a line with no descender sits on the bottom of its box and a line with
    /// one does not.
    pub baseline: f32,
    /// The line's x-height in pixels, the scale the baseline-relative features
    /// are expressed in. May have been inherited from the page when the line's
    /// own estimate was implausibly small; see `ARCHITECTURE.md` section 4.
    pub x_height: f32,
    /// Geometric mean over `words`, weighted by character count.
    pub confidence: f32,
    /// Index of the band this line's fragment was cut from, per
    /// `ARCHITECTURE.md` section 11 ("The shipped fixed-pitch rule is a net
    /// gain..."). Lines sharing a band index are fragments of one visual row
    /// that `column_gap_heights` split apart; joining them with a space and a
    /// single trailing newline is what a page-text renderer needs to say a
    /// band is one row rather than a run of unrelated lines. A band the cut
    /// left whole, or the cut disabled, is one line with this index to
    /// itself.
    pub band: usize,
}

/// An axis-aligned box in image pixel coordinates, y-down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The OCR engine, loaded from a `.ocrw` model file.
pub struct Engine {
    model: Model,
    cal: confidence::Calibration,
}

impl Engine {
    /// Loads a recogniser.
    pub fn from_bytes(model: &[u8]) -> Result<Self, Error> {
        let model = Model::load(model)?;
        // The calibration's shape is authored; only its language-model floor
        // is exposed as a threshold, so the file can soften how much a
        // disagreeing decoder is allowed to lower a confidence.
        let cal =
            confidence::Calibration { lm_floor: model.params.confidence.lm_floor, ..confidence::AUTHORED };
        Ok(Engine { model, cal })
    }

    /// The loaded model, for a caller that wants to report what it is running.
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// The parameter block this engine reads with.
    pub fn params(&self) -> &crate::params::Params {
        &self.model.params
    }

    /// Overrides one named threshold, returning whether the name was known.
    ///
    /// For the tuning harness only. `ARCHITECTURE.md` section 5 names the
    /// decoder weights as the tuning surface and `model/params.tsv` labels
    /// each one authored, measured or guess; this is how a guess gets swept
    /// into a measurement without writing a model file per point. Nothing on
    /// a reading path calls it — a page is read with the parameters its
    /// model file carries, and no others.
    pub fn set_param(&mut self, name: &str, v: f32) -> bool {
        self.model.params.set_f32(name, v)
            || (v >= 0.0 && self.model.params.set_u32(name, v as u32))
    }

    /// Overrides the matcher's per-dimension weights.
    ///
    /// The `.ocrw` `feature_weights` table is optional and absent from every
    /// file written so far, so a loaded model weights all 107 dimensions at
    /// one. This is how a candidate weighting is measured end to end before
    /// it is authored into the writer. Weights must be finite and
    /// non-negative, the same guard the loader applies; anything else is
    /// refused and the loaded weights stand.
    pub fn set_feature_weights(&mut self, w: &[f32; crate::feature::FEATURE_DIMS]) -> bool {
        if w.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return false;
        }
        self.model.weights = *w;
        true
    }

    /// Recognises a page, returning its words in reading order.
    pub fn recognize(&self, img: crate::Gray<'_>) -> Result<Vec<Word>, Error> {
        Ok(self.recognize_lines(img)?.into_iter().flat_map(|l| l.words).collect())
    }

    /// Recognises a page from a raw 8-bit greyscale buffer.
    ///
    /// `width`/`height` are pixels; `pixels` is row-major, top-down, one byte
    /// per pixel, `width * height` bytes long — the layout every external OCR
    /// consumer takes, `pdfcer`'s `OcrEngine::recognize` included. This is
    /// [`Engine::recognize`] with the [`crate::Gray`] borrow built for the
    /// caller, so an embedder never has to name that type itself; it performs
    /// no work of its own and is not a second implementation of anything
    /// (`CLAUDE.md` rule 4).
    ///
    /// # Coordinates
    ///
    /// [`Word::rect`] and each [`CharBox::rect`] are in the coordinates of
    /// this image, y-down, unrotated. This crate never converts to a
    /// page-space or a y-up convention — see the pipeline module's own
    /// contract doc.
    ///
    /// # Confidence
    ///
    /// [`Word::confidence`] is the calibrated match-margin score from
    /// `confidence.rs` (`CLAUDE.md` rule 5), already a geometric mean over
    /// the word's characters — never a raw distance.
    ///
    /// # Errors
    ///
    /// [`Error::BadTable`] if `pixels.len() != width * height`. Never errors
    /// on a blank or unreadable page; "no text here" returns `Ok(vec![])`.
    pub fn recognize_bytes(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<Word>, Error> {
        self.recognize(crate::Gray { width, height, data: pixels })
    }

    /// Recognises a page, keeping the line grouping.
    pub fn recognize_lines(&self, img: crate::Gray<'_>) -> Result<Vec<Line>, Error> {
        let p = &self.model.params;
        if img.data.len() != img.width as usize * img.height as usize {
            return Err(Error::BadTable {
                name: "image".into(),
                why: "data length must equal width*height",
            });
        }
        if img.width == 0 || img.height == 0 {
            return Ok(Vec::new());
        }

        // Stage timing (docs/measurements/2026-09-24_dense_page_speed.md):
        // everything through word-splitting is one "binarize/layout" bucket,
        // stopped where the per-word segmentation loop below starts its own.
        let prof_t = crate::prof::start();

        // 1. Binarize, then estimate skew on the mask rather than the
        //    grayscale: the estimator counts ink, and ink is what the mask is.
        let mask = binarize::binarize_with(&img, &p.binarize());
        let slope = deskew::estimate_with(&mask, img.width, img.height, f64::from(p.deskew.max_slope));
        let page = deskew::correct_with(&img, slope, f64::from(p.deskew.min_corrected_slope));

        // 2. Re-binarize the corrected page. Deskew resamples, so the mask
        //    taken before it no longer describes these pixels — and a mask
        //    that is one interpolation generation out of date is exactly the
        //    kind of near-miss that shows up as an occasional wrong glyph
        //    rather than as an obvious failure.
        let gray = page.gray();
        let mut mask = binarize::binarize_with(&gray, &p.binarize());

        // 3. Lines, then words within each line. Line params are needed
        //    ahead of labelling because the underline-strip pass (disabled
        //    by default; `ARCHITECTURE.md` section 11, 2026-09-23) erases
        //    rule pixels out of over-wide components before the labels a
        //    caller sees are ever produced -- a component this build reports
        //    already has its rule gone, not merely a component that would be
        //    rejected whole.
        let line_p = p.lines();
        let (labels, comps) = if line_p.underline_strip {
            let stripped = underline::strip_underlines(&mut mask, page.width, page.height, &line_p);
            (stripped.labels, stripped.components)
        } else {
            let (labels, count) =
                components::label(&mask, page.width, page.height, components::Connectivity::Eight);
            let comps = components::components(&labels, page.width, page.height, count);
            (labels, comps)
        };
        if comps.is_empty() {
            return Ok(Vec::new());
        }

        let word_p = p.words();
        let seg_p = p.segment();
        let tables = Tables {
            bigrams: self.model.bigrams.as_ref(),
            lexicon: self.model.lexicon.as_ref(),
            confusions: self.model.confusions.as_ref(),
        };
        let bands = lines::group_with_bands(&comps, page.width, page.height, &line_p);
        prof_t.stop(&crate::prof::COUNTERS.binarize_layout_ns);

        let mut out = Vec::new();
        for (band, group) in bands.into_iter().enumerate() {
            // Space thresholds for every fragment of this band at once: a
            // band the column cut split needs its fragments' gaps pooled,
            // per `ARCHITECTURE.md` section 11 ("The column cut's precision
            // collapse..."), which a fragment split alone cannot do.
            let split_t = crate::prof::start();
            let spans_by_line = words::split_band_with(&group, &comps, &word_p);
            split_t.stop(&crate::prof::COUNTERS.binarize_layout_ns);
            for (line, spans) in group.iter().zip(spans_by_line) {
                let mut got: Vec<Word> = Vec::new();
                for span in spans {
                    // Slant is measured once per word, ahead of segmentation,
                    // per `ARCHITECTURE.md` section 11 (2026-09-24 decision):
                    // gating off skips the measurement entirely, so an upright
                    // page pays nothing beyond this one branch. `slanted` is
                    // the same verdict, carried on to `read_word` so the
                    // decoder can credit `decode.char_bonus_slanted` instead
                    // of `decode.char_bonus` (the 2026-09-24 follow-up,
                    // "next a slanted-word bonus") -- a word never measured
                    // (gating off) is treated as not slanted, the same
                    // behaviour-neutral choice `italic_ok = true` makes for
                    // gating itself.
                    let slant_t = crate::prof::start();
                    let slanted = if p.layout.italic_gating != 0 {
                        let mut member_labels: Vec<u32> =
                            span.members.iter().map(|&i| comps[i].label).collect();
                        member_labels.sort_unstable();
                        member_labels.dedup();
                        crate::layout::slant::estimate(
                            &labels,
                            page.width,
                            span.x0,
                            span.x1,
                            span.y0,
                            span.y1,
                            &member_labels,
                            line.baseline,
                            &p.slant(),
                        )
                        .slanted
                    } else {
                        false
                    };
                    let italic_ok = p.layout.italic_gating == 0 || slanted;
                    slant_t.stop(&crate::prof::COUNTERS.binarize_layout_ns);

                    let seg_t = crate::prof::start();
                    let lat =
                        segment::build_with(&span, &comps, &labels, page.width, line, &seg_p);
                    seg_t.stop(&crate::prof::COUNTERS.segment_ns);
                    if crate::prof::enabled() {
                        crate::prof::add(&crate::prof::COUNTERS.words, 1);
                        crate::prof::add(&crate::prof::COUNTERS.edges, lat.edges.len() as u64);
                    }
                    let Some(w) = self.read_word(
                        &lat,
                        line,
                        &labels,
                        page.width,
                        &tables,
                        italic_ok,
                        slanted,
                    )
                    else {
                        continue;
                    };
                    if !w.text.is_empty() {
                        got.push(w);
                    }
                }
                if got.is_empty() {
                    continue;
                }
                let weighted: Vec<(f32, u32)> =
                    got.iter().map(|w| (w.confidence, w.chars.len() as u32)).collect();
                let rect = Rect {
                    x: line.x0,
                    y: unshear(line.y0, line.x0, slope, page.height),
                    width: line.width(),
                    height: line.height(),
                };
                let baseline =
                    unshear(line.baseline.round().max(0.0) as u32, line.x0, slope, page.height);
                out.push(Line {
                    words: got,
                    rect,
                    baseline: baseline as f32,
                    x_height: line.x_height,
                    confidence: confidence::line(&weighted),
                    band,
                });
            }
        }
        Ok(out)
    }

    /// Matches every edge of one word's lattice and decodes it.
    ///
    /// `slanted` is the same slant-estimator verdict `italic_ok` is built
    /// from, carried separately because the two feed different decisions:
    /// `italic_ok` gates which prototypes the matcher may consider, while
    /// `slanted` tells the decoder which per-character bonus to credit
    /// (`decode::viterbi::decode_word`'s own `slanted` argument).
    fn read_word(
        &self,
        lat: &segment::Lattice,
        line: &lines::TextLine,
        labels: &[u32],
        page_width: u32,
        tables: &Tables<'_>,
        italic_ok: bool,
        slanted: bool,
    ) -> Option<Word> {
        let p = &self.model.params;
        let k = p.matching.top_k.max(1) as usize;
        let mut hyps: Vec<Hyp> = Vec::with_capacity(lat.edges.len());
        let mut boxes: Vec<(u32, u32, u32, u32)> = Vec::with_capacity(lat.edges.len());

        for e in &lat.edges {
            let Some(g) = segment::crop(lat, labels, page_width, e) else {
                continue;
            };
            if g.width == 0 || g.height == 0 {
                continue;
            }
            let extract_t = crate::prof::start();
            let raw = crate::feature::extract(&g.input(line));
            extract_t.stop(&crate::prof::COUNTERS.extract_ns);
            let match_t = crate::prof::start();
            let matched = crate::r#match::nearest(&self.model, &raw, k, italic_ok);
            match_t.stop(&crate::prof::COUNTERS.match_ns);
            if crate::prof::enabled() {
                crate::prof::add(&crate::prof::COUNTERS.match_calls, 1);
            }
            let Some(m) = matched else {
                continue;
            };
            let ratio = m.ratio();
            let cands: Vec<Cand> = m
                .best
                .iter()
                .map(|c| Cand { class: c.class, distance: c.distance, ratio })
                .collect();
            if cands.is_empty() {
                continue;
            }
            hyps.push(Hyp {
                from: e.from,
                to: e.to,
                x0: e.x0,
                x1: e.x1,
                kind: e.kind,
                aspect: g.width as f32 / g.height as f32,
                cands,
            });
            boxes.push((g.x, g.y, g.width, g.height));
        }
        if hyps.is_empty() {
            return None;
        }

        // The box of the edge a decoded character came from. Matching on the
        // cut positions rather than carrying an index through the decoder
        // keeps the decoder free of image types; the pair is unique because
        // no two edges of a lattice share both cuts.
        let cuts: Vec<(u32, u32)> = hyps.iter().map(|h| (h.x0, h.x1)).collect();
        let lookup = |x0: u32, x1: u32| -> Option<(u32, u32, u32, u32)> {
            cuts.iter().position(|c| *c == (x0, x1)).map(|i| boxes[i])
        };

        let lattice = WordLattice { nodes: lat.positions.len(), edges: hyps };
        let decode_t = crate::prof::start();
        let decoded = viterbi::decode_word(&lattice, &self.model.class_info, tables, &p.decode, slanted);
        decode_t.stop(&crate::prof::COUNTERS.decode_ns);
        let decoded = decoded?;

        let mut text = String::new();
        let mut chars = Vec::with_capacity(decoded.chars.len());
        let mut scores = Vec::with_capacity(decoded.chars.len());
        for c in &decoded.chars {
            let Some(ch) = self.model.char_of(c.class) else {
                continue;
            };
            text.push(ch);
            let conf = confidence::character(&self.cal, c.ratio);
            scores.push(conf);
            let (x, y, w, h) = lookup(c.x0, c.x1).unwrap_or((c.x0, line.y0, c.x1 - c.x0, line.height()));
            chars.push(CharBox { ch, rect: Rect { x, y, width: w, height: h }, confidence: conf });
        }
        if chars.is_empty() {
            return None;
        }
        let x0 = chars.iter().map(|c| c.rect.x).min().unwrap_or(0);
        let x1 = chars.iter().map(|c| c.rect.x + c.rect.width).max().unwrap_or(0);
        let y0 = chars.iter().map(|c| c.rect.y).min().unwrap_or(0);
        let y1 = chars.iter().map(|c| c.rect.y + c.rect.height).max().unwrap_or(0);
        Some(Word {
            text,
            rect: Rect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 },
            confidence: confidence::word(&scores),
            chars,
        })
    }
}

/// Maps a y in the deskewed page back to the input image's y.
///
/// `deskew::correct` shears vertically by `slope` about the page centre, so
/// undoing it is one multiply. x is untouched by the shear and needs nothing.
fn unshear(y: u32, x: u32, slope: f64, height: u32) -> u32 {
    if slope == 0.0 {
        return y;
    }
    let dy = (x as f64 - 0.0) * slope;
    let back = y as f64 + dy;
    back.clamp(0.0, f64::from(height.saturating_sub(1))) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_that_is_not_a_model_is_refused() {
        assert_eq!(Engine::from_bytes(b"not an ocrw file").err(), Some(Error::BadMagic));
    }

    #[test]
    fn an_unsheared_page_maps_y_to_itself() {
        assert_eq!(unshear(17, 300, 0.0, 1000), 17);
    }

    #[test]
    fn a_sheared_page_maps_y_back_and_stays_on_the_page() {
        // A positive slope pushed ink up on the right; undoing it pushes back
        // down, and nothing may leave the page.
        assert!(unshear(17, 300, 0.02, 1000) > 17);
        assert_eq!(unshear(17, 300_000, 0.02, 1000), 999);
    }
}

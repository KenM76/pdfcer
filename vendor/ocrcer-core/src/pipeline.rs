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
use crate::decode::viterbi::{self, Cand, ClassInfo, Hyp, Tables, WordLattice};
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

/// One decoded character's router inputs, in Viterbi's committed-path order
/// within its word — the exact numbers `read_word`'s relabel pass computes,
/// captured before any `route.*` threshold decides anything with them.
///
/// Exists for the chunk 15c fit harness (`ARCHITECTURE.md` §11, 2026-09-28,
/// "chunk 15c pre-registered"), so a grid search over
/// `route.matcher_margin`/`route.net_prob`/`route.max_junk`/
/// `route.same_category` can replay the router's own three-line decision at
/// every grid point from numbers computed **once** per page, rather than
/// re-running segmentation, matching and the network forward pass per point
/// (`CLAUDE.md` rule 4 — the same "replay from cached numbers, never a
/// second implementation of the decision" precedent `route_fit.rs` set in
/// chunk 15b, extended from aligned crops to whole decoded words).
/// `net` is `Some` only when [`Engine::recognize_lines_route_probe`]'s
/// caller configured `route.matcher_margin` high enough (the grid's own
/// maximum, so every point the grid could query is covered) that this
/// glyph's matcher confidence fell below it and a network query actually
/// ran; a glyph the matcher was already confident about at that ceiling
/// carries `None` and no grid point below the ceiling can ever query it
/// either, so nothing is lost.
#[derive(Debug, Clone, Copy)]
pub struct RouteProbe {
    pub matcher_class: u16,
    pub matcher_conf: f32,
    pub net: Option<NetProbe>,
}

/// The network's own numbers for one queried glyph, all read directly off
/// its log-softmax output (`crate::nn::Nn::forward`) with no second
/// computation: `prob` is `nn_candidates(..., k=1, scale=1.0)`'s top
/// candidate's `(-distance).exp()`, exactly what `route.net_prob`
/// thresholds against; `junk_prob` is `log_probs[net.junk_index].exp()`,
/// exactly what `route.max_junk` thresholds against.
#[derive(Debug, Clone, Copy)]
pub struct NetProbe {
    pub class: u16,
    pub prob: f32,
    pub junk_prob: f32,
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
    /// `Some(reason)` when `match.classifier` is `1` or `3` but no network is
    /// loaded, so every word was read with prototype scoring alone instead of
    /// what the model file asked for. Computed once at load rather than per
    /// word: the fact is about the model, not about any one page, and a
    /// per-call counter behind `OCRCER_PROFILE=1` would let this go unnoticed
    /// on an ordinary run (`CLAUDE.md` rule 5 — a promise about confidence
    /// extends to a promise about which scorer produced it).
    classifier_fallback: Option<&'static str>,
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
        let classifier_fallback = match (model.params.matching.classifier, model.nn.is_some()) {
            (1, false) => {
                Some("match.classifier=1 requested but no nn table is loaded; scoring with prototypes")
            }
            (3, false) => {
                Some("match.classifier=3 requested but no nn table is loaded; scoring with prototypes")
            }
            _ => None,
        };
        Ok(Engine { model, cal, classifier_fallback })
    }

    /// The loaded model, for a caller that wants to report what it is running.
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// `Some(reason)` when this engine is scoring with prototypes despite the
    /// model asking for the network (`match.classifier` `1` or `3` with no
    /// `nn` table loaded). `None` otherwise — including when `classifier ==
    /// 0`, where there is nothing to fall back from.
    pub fn classifier_fallback(&self) -> Option<&'static str> {
        self.classifier_fallback
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
        Ok(self.recognize_lines_impl(img)?.into_iter().map(|(l, _)| l).collect())
    }

    /// [`Engine::recognize_lines`], plus each word's [`RouteProbe`]s.
    ///
    /// Bench-only (chunk 15c, `ARCHITECTURE.md` §11, 2026-09-28): the
    /// production reading path never calls this — `recognize_lines` above
    /// discards exactly the same probes this returns, so the two cannot
    /// disagree about anything but which of a `read_word` call's two return
    /// values the caller kept (`CLAUDE.md` rule 4). The per-word probe
    /// vectors are aligned index-for-index with the returned `Line`'s
    /// `words`, each inner vector aligned with that word's `chars`.
    pub fn recognize_lines_route_probe(
        &self,
        img: crate::Gray<'_>,
    ) -> Result<Vec<(Line, Vec<Vec<RouteProbe>>)>, Error> {
        self.recognize_lines_impl(img)
    }

    fn recognize_lines_impl(&self, img: crate::Gray<'_>) -> Result<Vec<(Line, Vec<Vec<RouteProbe>>)>, Error> {
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
                let mut got: Vec<(Word, Vec<RouteProbe>)> = Vec::new();
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
                    let (w, probe) = self.read_word(
                        &lat,
                        line,
                        &labels,
                        page.width,
                        &tables,
                        italic_ok,
                        slanted,
                    );
                    let Some(w) = w else {
                        continue;
                    };
                    if !w.text.is_empty() {
                        got.push((w, probe));
                    }
                }
                if got.is_empty() {
                    continue;
                }
                let weighted: Vec<(f32, u32)> =
                    got.iter().map(|(w, _)| (w.confidence, w.chars.len() as u32)).collect();
                let rect = Rect {
                    x: line.x0,
                    y: unshear(line.y0, line.x0, slope, page.height),
                    width: line.width(),
                    height: line.height(),
                };
                let baseline =
                    unshear(line.baseline.round().max(0.0) as u32, line.x0, slope, page.height);
                let (words, probes): (Vec<Word>, Vec<Vec<RouteProbe>>) = got.into_iter().unzip();
                out.push((
                    Line {
                        words,
                        rect,
                        baseline: baseline as f32,
                        x_height: line.x_height,
                        confidence: confidence::line(&weighted),
                        band,
                    },
                    probes,
                ));
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
    ) -> (Option<Word>, Vec<RouteProbe>) {
        let p = &self.model.params;
        let k = p.matching.top_k.max(1) as usize;
        let mut hyps: Vec<Hyp> = Vec::with_capacity(lat.edges.len());
        let mut boxes: Vec<(u32, u32, u32, u32)> = Vec::with_capacity(lat.edges.len());
        // Only ever populated under `route_on` below, so a mode-0/1 run pays
        // nothing for it: the router relabels a glyph the matcher has
        // already committed to, and doing that without a second segmentation
        // implementation (`CLAUDE.md` rule 4) means re-running the same
        // extractor on the same crop after decode rather than during it.
        let mut glyphs: Vec<Option<segment::Glyph>> = Vec::new();

        // `use_nn` is decided once, outside the loop, from facts the loop
        // itself cannot change (the loaded model, not any one edge). This is
        // what makes `classifier == 0` byte-identical to every fixture that
        // predates this field by construction rather than by testing alone:
        // whenever it is false, every edge below runs the exact prototype
        // path this function has always run, untouched. The router
        // (`classifier == 3`) is deliberately absent from this condition: it
        // must always build its lattice from the prototype matcher, exactly
        // as `classifier == 0` does, and only relabels after Viterbi has
        // already fixed the segmentation (`ARCHITECTURE.md` §11, 2026-09-27).
        let use_nn = p.matching.classifier == 1 && self.model.nn.is_some();
        let route_on = p.matching.classifier == 3 && self.model.nn.is_some();

        for e in &lat.edges {
            let Some(g) = segment::crop(lat, labels, page_width, e) else {
                continue;
            };
            if g.width == 0 || g.height == 0 {
                continue;
            }
            let cands: Vec<Cand> = if use_nn {
                let extract_t = crate::prof::start();
                let (raw, grid) = crate::feature::extract_with_grid(&g.input(line));
                extract_t.stop(&crate::prof::COUNTERS.extract_ns);
                // The same normalisation code the matcher uses (`CLAUDE.md`
                // rule 4): there is no second normalisation path for the
                // network's input.
                let normalised = self.model.standardise(&raw);
                let match_t = crate::prof::start();
                // `use_nn` is only ever true when `self.model.nn` is
                // `Some(..)`.
                let net = self.model.nn.as_ref().expect("use_nn implies a loaded network");
                let forward = net.forward(&grid, &normalised);
                match_t.stop(&crate::prof::COUNTERS.match_ns);
                if crate::prof::enabled() {
                    crate::prof::add(&crate::prof::COUNTERS.match_calls, 1);
                }
                let Ok(log_probs) = forward else {
                    continue;
                };
                nn_candidates(net, &log_probs, k, p.nn.scale)
            } else {
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
                m.best.iter().map(|c| Cand { class: c.class, distance: c.distance, ratio }).collect()
            };
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
            if route_on {
                glyphs.push(Some(g));
            }
        }
        if hyps.is_empty() {
            return (None, Vec::new());
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
        let Some(decoded) = decoded else {
            return (None, Vec::new());
        };

        // The router's relabel pass: evaluated only on the glyphs Viterbi has
        // already committed to, on the fixed path above — never on a
        // partial or merged lattice edge, which is where every earlier
        // fused/network-only attempt lost (`ARCHITECTURE.md` §11,
        // 2026-09-27). `class`, when it differs from `c.class`, is a
        // relabel; `conf_override` is the network's own calibrated
        // confidence for it, computed before the word-level agreement term
        // below, exactly the two-stage shape `confidence::adjust` expects.
        let lookup_glyph = |x0: u32, x1: u32| -> Option<&segment::Glyph> {
            cuts.iter().position(|c| *c == (x0, x1)).and_then(|i| glyphs.get(i)).and_then(|g| g.as_ref())
        };
        let mut relabel: Vec<(u16, Option<f32>)> = Vec::with_capacity(decoded.chars.len());
        // Captured alongside `relabel` for `Engine::recognize_lines_route_probe`
        // (chunk 15c, `ARCHITECTURE.md` §11, 2026-09-28): the raw matcher and
        // net readings behind every relabel decision, so the fit harness can
        // replay `route.*` thresholds in memory instead of re-running
        // segmentation, matching and decode once per grid point
        // (`CLAUDE.md` rule 4 — the pipeline that produces these numbers is
        // still written exactly once). Empty whenever `route_on` is false, so
        // classifier mode 0 pays nothing for this and cannot diverge from it.
        let mut probe: Vec<RouteProbe> = Vec::with_capacity(if route_on { decoded.chars.len() } else { 0 });
        if route_on {
            // Hoisted so `category_flip_vetoed` (shared with the chunk 15c
            // fit harness) reads the word's original classes without
            // reallocating per glyph.
            let orig_classes: Vec<u16> = decoded.chars.iter().map(|c| c.class).collect();
            for (i, c) in decoded.chars.iter().enumerate() {
                let mut class = c.class;
                let mut conf_override = None;
                let matcher_conf = confidence::character(&self.cal, c.ratio);
                let mut net_probe: Option<NetProbe> = None;
                if matcher_conf < p.route.matcher_margin {
                    if let Some(g) = lookup_glyph(c.x0, c.x1) {
                        let extract_t = crate::prof::start();
                        let (raw, grid) = crate::feature::extract_with_grid(&g.input(line));
                        extract_t.stop(&crate::prof::COUNTERS.extract_ns);
                        // Same normalisation the matcher and mode 1 both use
                        // (`CLAUDE.md` rule 4).
                        let normalised = self.model.standardise(&raw);
                        // `route_on` implies `self.model.nn` is `Some(..)`.
                        let net = self.model.nn.as_ref().expect("route_on implies a loaded network");
                        let match_t = crate::prof::start();
                        let forward = net.forward(&grid, &normalised);
                        match_t.stop(&crate::prof::COUNTERS.match_ns);
                        if crate::prof::enabled() {
                            crate::prof::add(&crate::prof::COUNTERS.match_calls, 1);
                        }
                        // `k = 1, scale = 1.0`: only the top charset class's
                        // own log-probability is wanted here, junk excluded
                        // and ties to the lowest class index, reusing
                        // `nn_candidates` rather than a second sort
                        // (`CLAUDE.md` rule 4). `distance` is `-log p`
                        // un-scaled, so `(-distance).exp()` is the raw
                        // probability `route.net_prob` thresholds against.
                        if let Ok(log_probs) = forward {
                            // `route.max_junk` (chunk 15c, `ARCHITECTURE.md`
                            // §11, 2026-09-28): `forward`'s log-softmax
                            // already normalises over every output
                            // including junk, so `log_probs[junk_index]` is
                            // the crop's junk log-probability with no
                            // second computation (`CLAUDE.md` rule 4).
                            // Default `1.0` never vetoes, since a
                            // probability cannot exceed it.
                            let junk_prob = log_probs
                                .get(net.junk_index as usize)
                                .copied()
                                .unwrap_or(f32::NEG_INFINITY)
                                .exp();
                            // The top candidate is read unconditionally
                            // (unrouted by `max_junk`) so the probe carries
                            // the net's actual answer regardless of which
                            // grid point vetoes it at replay time.
                            if let Some(top) = nn_candidates(net, &log_probs, 1, 1.0).into_iter().next() {
                                let net_prob = (-top.distance).exp();
                                net_probe = Some(NetProbe { class: top.class, prob: net_prob, junk_prob });
                                if junk_prob <= p.route.max_junk
                                    && net_prob >= p.route.net_prob
                                    && top.class != c.class
                                {
                                    let vetoed = p.route.same_category == 1
                                        && category_flip_vetoed(
                                            &self.model.class_info,
                                            &orig_classes,
                                            i,
                                            top.class,
                                        );
                                    if !vetoed {
                                        class = top.class;
                                        conf_override = Some(confidence::character(&self.cal, top.ratio));
                                    }
                                }
                            }
                        }
                    }
                }
                probe.push(RouteProbe { matcher_class: c.class, matcher_conf, net: net_probe });
                relabel.push((class, conf_override));
            }
        }

        let mut text = String::new();
        let mut chars = Vec::with_capacity(decoded.chars.len());
        let mut scores = Vec::with_capacity(decoded.chars.len());
        let mut final_classes: Vec<u16> = Vec::with_capacity(decoded.chars.len());
        let mut any_relabel = false;
        for (i, c) in decoded.chars.iter().enumerate() {
            let (class, conf_override) = relabel.get(i).copied().unwrap_or((c.class, None));
            let Some(ch) = self.model.char_of(class) else {
                continue;
            };
            text.push(ch);
            final_classes.push(class);
            any_relabel |= class != c.class;
            let conf = conf_override.unwrap_or_else(|| confidence::character(&self.cal, c.ratio));
            scores.push(conf);
            let (x, y, w, h) = lookup(c.x0, c.x1).unwrap_or((c.x0, line.y0, c.x1 - c.x0, line.height()));
            chars.push(CharBox { ch, rect: Rect { x, y, width: w, height: h }, confidence: conf });
        }
        if chars.is_empty() {
            return (None, probe);
        }
        // "Words containing a relabelled glyph are re-scored by the
        // decoder's existing word terms. The path does not change."
        // (`ARCHITECTURE.md` §11, 2026-09-27.) `word_agreement` re-runs the
        // lexicon-tier test `decode_word` already computes at a word's end,
        // against the relabelled string, without touching the beam search
        // that chose the segmentation. Every character in the word is
        // adjusted, not only the relabelled one, because the lexicon term it
        // reuses is itself a whole-word quantity.
        if any_relabel {
            let agreement =
                viterbi::word_agreement(&final_classes, &self.model.class_info, tables, &p.decode);
            for (score, ch) in scores.iter_mut().zip(chars.iter_mut()) {
                *score = confidence::adjust(&self.cal, *score, agreement);
                ch.confidence = *score;
            }
        }
        let x0 = chars.iter().map(|c| c.rect.x).min().unwrap_or(0);
        let x1 = chars.iter().map(|c| c.rect.x + c.rect.width).max().unwrap_or(0);
        let y0 = chars.iter().map(|c| c.rect.y).min().unwrap_or(0);
        let y1 = chars.iter().map(|c| c.rect.y + c.rect.height).max().unwrap_or(0);
        (
            Some(Word {
                text,
                rect: Rect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 },
                confidence: confidence::word(&scores),
                chars,
            }),
            probe,
        )
    }
}

/// Turns a network forward pass's log-probabilities into up-to-`k` [`Cand`]s,
/// scored in the prototype matcher's distance units.
///
/// Junk (`net.junk_index`) is excluded from the candidate pool entirely, per
/// the 2026-09-25 junk-output amendment's item 4: junk is never emitted and
/// is never a rival class for confidence, so it must not win a lattice edge
/// and must not be counted when the top-two margin is measured. `distance`
/// is `nn.scale * (-log p(c))`; `ratio` is one value shared by every
/// returned candidate, the same shape `crate::r#match::nearest`'s single
/// `Match::ratio()` takes — it is a property of the edge's top two charset
/// classes, not of any one candidate within it.
///
/// `pub` (chunk 15b, `ARCHITECTURE.md` §11, 2026-09-27): the router's fit
/// script (`ocrcer-bench`) is a third caller, alongside the two uses inside
/// this file, and reuses this function rather than re-deriving the same
/// sort-and-margin logic outside the crate (`CLAUDE.md` rule 4).
pub fn nn_candidates(net: &crate::nn::Nn, log_probs: &[f32], k: usize, scale: f32) -> Vec<Cand> {
    let junk_index = net.junk_index as usize;
    let mut charset: Vec<(u16, f32)> = log_probs
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != junk_index)
        .map(|(i, &lp)| (i as u16, lp))
        .collect();
    // Descending by log-probability (best first); ties to the lower class
    // index, the same rule `crate::r#match` uses.
    charset.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));

    let margin = match (charset.first(), charset.get(1)) {
        (Some(&(_, top1)), Some(&(_, top2))) => top1 - top2,
        (Some(_), None) => f32::INFINITY, // one charset class stood: no rival.
        (None, _) => return Vec::new(),
    };
    let ratio = confidence::nn_ratio_from_margin(margin);

    charset
        .into_iter()
        .take(k.max(1))
        .map(|(class, lp)| Cand { class, distance: scale * -lp, ratio })
        .collect()
}

/// `route.same_category` (chunk 15c, `ARCHITECTURE.md` §11, 2026-09-28): is
/// this a digit<->letter relabel, and is every *other* decoded character in
/// the word already in the glyph's original category? A class with neither
/// flag (punctuation, etc.) is in no category and never triggers or blocks
/// the veto — the gate only speaks to digit/letter runs.
///
/// Takes the word's *original* (pre-relabel) classes rather than `&[Char]`
/// so the chunk 15c fit harness (`ocrcer-bench`) can replay this exact
/// decision from a captured [`RouteProbe`] row's `matcher_class` field,
/// without a second implementation of the category test (`CLAUDE.md` rule
/// 4) — `pub` for that one caller, the same reason `nn_candidates` is
/// `pub`.
pub fn category_flip_vetoed(class_info: &[ClassInfo], classes: &[u16], i: usize, new_class: u16) -> bool {
    fn category(ci: ClassInfo) -> Option<bool> {
        // `Some(true)`: digit. `Some(false)`: letter. `None`: neither.
        if ci.digit {
            Some(true)
        } else if ci.letter {
            Some(false)
        } else {
            None
        }
    }
    let Some(&info) = classes.get(i).and_then(|&c| class_info.get(c as usize)) else { return false };
    let Some(&new_info) = class_info.get(new_class as usize) else { return false };
    let (Some(orig_cat), Some(new_cat)) = (category(info), category(new_info)) else { return false };
    if orig_cat == new_cat {
        return false;
    }
    classes.iter().enumerate().all(|(j, &other)| {
        j == i
            || class_info.get(other as usize).and_then(|&ci| category(ci)).is_some_and(|cat| cat == orig_cat)
    })
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

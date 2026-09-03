//! OCR text layers — the "sandwich" (ISO 32000-1 §9.3.6, Table 106 mode 3).
//!
//! # What this module is, and what it deliberately is not
//!
//! This is the half of OCR that is **engine-independent**: taking words with
//! page positions and writing them into a PDF as an invisible, selectable text
//! layer over content that is left completely untouched.
//!
//! It contains **no recogniser**. [`OcrEngine`] is a trait, and the engine
//! that implements it is a separate, feature-gated decision with real licence
//! consequences (see `docs/ocr-engine-survey.md`). Splitting it this way is
//! not tidiness — the text-layer authoring is identical whichever engine wins,
//! so building it first means the engine choice can be made on its merits
//! instead of under pressure from work already committed to one API.
//!
//! # The sandwich, concretely
//!
//! A scanned page is one big image and no text. OCR reads the image and
//! produces words with bounding boxes. Those words are then drawn ON TOP of
//! the image in text rendering mode **3** — Table 106's *"neither fill nor
//! stroke text (invisible)"*.
//!
//! The result: the page looks EXACTLY as it did, because nothing visible was
//! added and nothing existing was altered, while Find, copy, and text
//! extraction all work. That is the behaviour `PRIOR_ART.md` cites OCRmyPDF
//! for, and the spec corpus names mode 3 as the mechanism by name.
//!
//! # Why the original content is never touched
//!
//! An OCR layer is **additive**. It appends a second content stream; it does
//! not rewrite, re-encode or re-compress the scan. Two reasons, and the second
//! is the one that matters:
//!
//! 1. Round-trip/minimal-diff (project rule 3) — an object pdfcer did not
//!    logically modify is re-emitted byte-identical or omitted entirely.
//! 2. **Re-encoding a scan loses evidence.** A scanned document is often the
//!    record of something — a signed contract, a survey, a drawing. Running
//!    its JPEG through a decode/re-encode cycle to "help" costs generation
//!    loss on an image the operator may need to defend the provenance of. OCR
//!    is supposed to make a document findable, not modify it.
//!
//! # Rule 4: OCR is an inference, and a large one
//!
//! Every word here is a GUESS. Project rule 4 requires that what pdfcer
//! inferred is visible before it becomes document state, and that where an
//! inference is *inherently* uncertain the uncertainty is stated rather than
//! implied.
//!
//! So [`RecognizedWord::confidence`] is `Option<f32>`, and the `None` case is
//! load-bearing rather than a convenience: some engines expose no confidence
//! at all. A shell must say *"this engine reports no per-word confidence"*
//! rather than silently presenting unscored guesses as though they had been
//! checked. An absent score and a high score must never look the same.

/// Where OCR model files come from — operator-supplied or beside the binary,
/// never downloaded. See the module's own docs for why a downloader was
/// proposed and withdrawn.
pub mod models;

/// The sandwich writer — recognised words become an invisible, selectable
/// text layer over page content that is left byte-identical.
pub mod layer;

/// The `ocrs` recogniser, behind the Cargo feature of the same name.
///
/// The ONLY engine-aware module in the OCR subsystem. Everything else here is
/// compiled unconditionally, so a build without this feature can still write a
/// text layer from words obtained some other way — it simply cannot produce
/// them itself.
#[cfg(feature = "ocrs")]
pub mod engine_ocrs;

use crate::page_tree::Rect;

/// One recognised word, positioned in PDF default user space.
///
/// # Why a WORD and not a line or a character
///
/// The unit has to match what a reader searches and selects. Per-character
/// boxes make the extractor's job harder for no gain (it would have to
/// re-derive word boundaries pdfcer already knew); per-line boxes make
/// selection coarse and make a search hit highlight a whole line.
///
/// Words also match what every candidate engine actually emits, so nothing is
/// re-derived from something else's output.
#[derive(Debug, Clone, PartialEq)]
pub struct RecognizedWord {
    /// The recognised text.
    pub text: String,
    /// Where it sits on the page, PDF default user space, y-up.
    ///
    /// Engines almost universally report y-DOWN image pixels, so the
    /// conversion is the caller's job and is deliberately not hidden here —
    /// a silent flip is the single most common way an OCR layer ends up
    /// mirrored, and it is invisible until someone selects text and finds it
    /// lands on the wrong line.
    pub rect: Rect,
    /// The engine's confidence, `0.0..=1.0`, or `None` when the engine does
    /// not report one.
    ///
    /// `None` is NOT "assume it is fine". See the module docs: a shell must
    /// disclose the absence rather than let unscored output pass as checked.
    pub confidence: Option<f32>,
}

/// Everything recognised on one page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OcrPage {
    /// The words, in reading order as the engine reported it.
    ///
    /// Order matters: it becomes the order of the `Tj` operators, and
    /// therefore the order text extraction returns. An engine that reports
    /// nonsense order produces a searchable page whose copied text is
    /// scrambled — worth checking per engine rather than assuming.
    pub words: Vec<RecognizedWord>,
    /// Whether the engine reported ANY confidence values at all.
    ///
    /// Kept at page level as well as per word so a shell can make the rule-4
    /// disclosure ONCE ("this engine reports no confidence") instead of
    /// per word, and can tell "the engine has no confidence support" apart
    /// from "the engine had nothing to say about this particular word".
    pub confidence_available: bool,
}

impl OcrPage {
    /// The mean confidence over words that reported one, or `None`.
    ///
    /// Provided so a shell can lead with a single honest number. Deliberately
    /// skips unscored words rather than treating them as zero or as one —
    /// both would be inventing data, and in opposite directions.
    #[must_use]
    pub fn mean_confidence(&self) -> Option<f32> {
        let scored: Vec<f32> = self.words.iter().filter_map(|w| w.confidence).collect();
        if scored.is_empty() {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        Some(scored.iter().sum::<f32>() / scored.len() as f32)
    }

    /// Words whose confidence is below `threshold`, for review.
    ///
    /// Words with NO confidence are **included**, because an unscored word is
    /// exactly as unverified as a low-scored one — excluding them would let
    /// an engine that reports nothing produce an empty "needs review" list and
    /// look better than one that reports honestly.
    #[must_use]
    pub fn words_needing_review(&self, threshold: f32) -> Vec<&RecognizedWord> {
        self.words
            .iter()
            .filter(|w| w.confidence.is_none_or(|c| c < threshold))
            .collect()
    }
}

/// A text recogniser.
///
/// Implemented outside this module by whichever engine is chosen, behind its
/// own Cargo feature. The trait is deliberately tiny: everything about PDF —
/// the content stream, the font, the rendering mode, the resources — belongs
/// on this side of the boundary, and an engine that had to know any of it
/// would be doing pdfcer's job.
pub trait OcrEngine {
    /// The error this engine reports.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Recognise text in an 8-bit greyscale image.
    ///
    /// `width`/`height` are pixels; `pixels` is row-major, top-down, one byte
    /// per pixel — the layout every candidate engine takes, so no conversion
    /// is imposed on the implementor.
    ///
    /// The returned rectangles are in **image pixel coordinates, y-down**.
    /// Converting them to PDF user space is [`words_to_page_space`]'s job,
    /// which keeps the flip in exactly one place instead of in every engine.
    ///
    /// # Errors
    ///
    /// Whatever the engine reports — a model that failed to load, an image it
    /// refuses, a recogniser failure.
    fn recognize(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<RecognizedWord>, Self::Error>;

    /// Whether this engine reports per-word confidence.
    ///
    /// A required method with no default, deliberately. A default of `true`
    /// would make an engine that forgot to implement it claim scores it does
    /// not have; a default of `false` would let one that HAS them silently
    /// under-report. Making it explicit costs one line and removes both.
    fn reports_confidence(&self) -> bool;
}

/// Convert engine output from image pixels (y-down) to PDF user space (y-up).
///
/// # Why this is a free function and not done inside the engine
///
/// The y-flip is the most common OCR-layer defect there is: get it wrong and
/// every word lands mirrored vertically, the page still looks perfect, and
/// nobody notices until someone selects a line and gets a different one. Doing
/// it once, here, means an engine implementor cannot get it wrong and every
/// engine is wrong or right together.
///
/// `page_rect` is the region of the page the image covers, in user space —
/// normally the full crop box for a scanned page, but not necessarily, which
/// is why it is a parameter rather than assumed.
///
/// # ★★ THIS FUNCTION ASSUMES THE PAGE IS NOT ROTATED
///
/// It applies a scale and a y-flip and nothing else, which is correct for a
/// page whose `/Rotate` is `0` (ISO 32000-1 Table 30) and **silently wrong for
/// every other value**. There is no way for it to be otherwise: the rotation
/// is not in its signature, so it cannot see one.
///
/// This matters because `pdfcer-render` **does** honour `/Rotate` — see
/// `page_device_geometry`, which swaps width and height at 90° and 270° and
/// composes a different transform for each of the four values. So a caller
/// that rasterises a rotated page and then hands the result here is combining
/// a rotation-aware rasteriser with a rotation-blind mapping, and gets an
/// invisible text layer that is transposed or inverted relative to the ink.
/// The page still looks perfect — nothing visible was added — and the defect
/// surfaces only as *"the OCR text does not line up with the image"*.
///
/// **Use [`words_to_page_space_on`] instead unless you have established the
/// page's `/Rotate` is zero.** This function is kept, unchanged, because it is
/// public API and because it is exactly right for the case it names; it now
/// says which case that is.
#[must_use]
pub fn words_to_page_space(
    words: &[RecognizedWord],
    image_width: u32,
    image_height: u32,
    page_rect: Rect,
) -> Vec<RecognizedWord> {
    if image_width == 0 || image_height == 0 {
        return Vec::new();
    }
    let sx = (page_rect.urx - page_rect.llx) / f64::from(image_width);
    let sy = (page_rect.ury - page_rect.lly) / f64::from(image_height);
    words
        .iter()
        .map(|w| {
            // The flip: an image row 0 is the TOP, a PDF y of ury is the top.
            let top = page_rect.ury - w.rect.lly * sy;
            let bottom = page_rect.ury - w.rect.ury * sy;
            RecognizedWord {
                text: w.text.clone(),
                rect: Rect::from_corners(
                    w.rect.llx.mul_add(sx, page_rect.llx),
                    bottom.min(top),
                    w.rect.urx.mul_add(sx, page_rect.llx),
                    bottom.max(top),
                ),
                confidence: w.confidence,
            }
        })
        .collect()
}

/// How the rasterised image sits on the page: the region it covers, and the
/// page's `/Rotate`.
///
/// # Why the rotation travels WITH the rectangle rather than as a loose
/// argument
///
/// Because they are only meaningful together, and separating them is how the
/// defect this type exists to prevent got in. A `Rect` alone cannot say which
/// way up the raster is; a rotation alone cannot say what it is a rotation OF.
/// A caller holding both as bare parameters can pass a crop box it read from
/// the page and a rotation it forgot to read — and that is not a hypothetical
/// shape, it is the default one, because `/Rotate` is optional and absent on
/// most pages, so code written against the common case never learns it exists.
///
/// Bundling them means the type system asks the question. [`PagePlacement`]
/// cannot be constructed without stating a rotation, and
/// [`PagePlacement::upright`] makes saying "zero" a deliberate, greppable act
/// rather than an omission.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PagePlacement {
    /// The region of the page the image covers, in **user space**.
    ///
    /// For a scanned page this is normally the whole crop box — and it must be
    /// the **crop** box, not the media box, whenever the two differ, because
    /// `pdfcer-render::page_device_geometry` rasterises the crop box. Handing
    /// this the media box of a page that has a smaller crop box scales every
    /// word by the ratio between them.
    pub rect: Rect,
    /// The page's `/Rotate`, normalised to 0, 90, 180 or 270 (Table 30).
    ///
    /// This is the value the RASTERISER used. If a caller renders with one
    /// rotation and maps with another the words land somewhere neither
    /// explains, so it is read once and passed, never re-derived.
    pub rotate: u16,
}

impl PagePlacement {
    /// A placement on an **unrotated** page.
    ///
    /// Named rather than defaulted so that "this page is upright" is something
    /// the code SAYS. A `Default` impl would let the commonest mistake —
    /// never thinking about rotation at all — look identical to having
    /// checked.
    #[must_use]
    pub fn upright(rect: Rect) -> Self {
        Self { rect, rotate: 0 }
    }

    /// A placement carrying a page's `/Rotate`, normalised.
    ///
    /// Table 30 requires a multiple of 90 and permits negatives; anything else
    /// is malformed. Rather than refuse — this is a positioning helper, not a
    /// validator, and refusing would lose an otherwise good OCR run over a
    /// stray dictionary value — a non-conforming value is normalised toward
    /// the nearest legal quarter turn by the same modular arithmetic the
    /// renderer uses, so the mapping and the raster agree whatever the file
    /// said.
    #[must_use]
    pub fn new(rect: Rect, rotate: i32) -> Self {
        let r = rotate.rem_euclid(360);
        let quarter = ((r + 45) / 90) % 4;
        Self {
            rect,
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            rotate: (quarter * 90) as u16,
        }
    }
}

/// Convert engine output from image pixels (y-down) to PDF user space (y-up),
/// **honouring the page's rotation**.
///
/// The rotation-aware sibling of [`words_to_page_space`], and the one to
/// reach for by default. On an upright page the two agree exactly, and a test
/// asserts that rather than leaving it to inspection.
///
/// # The four inverses, and why they are written out
///
/// `pdfcer-render::page_device_geometry` maps user space to device space, and
/// this function has to undo precisely that map — so the four cases below are
/// its four cases, inverted, and are kept in the same order and the same
/// notation deliberately. `s` is the rasterisation scale; the image is
/// `image_width` x `image_height` pixels; the placement's rect is
/// `(llx, lly) .. (urx, ury)`:
///
/// ```text
///   rotate    forward (render)                inverse (here)
///   0         x' = (x-llx)*s                  x = llx + x'/s
///             y' = (ury-y)*s                  y = ury - y'/s
///   90        x' = (y-lly)*s                  y = lly + x'/s
///             y' = (x-llx)*s                  x = llx + y'/s
///   180       x' = (urx-x)*s                  x = urx - x'/s
///             y' = (y-lly)*s                  y = lly + y'/s
///   270       x' = (ury-y)*s                  y = ury - x'/s
///             y' = (urx-x)*s                  x = urx - y'/s
/// ```
///
/// ★ Note that at 90 and 270 the image's WIDTH spans the page's HEIGHT. The
/// two scale factors are therefore derived from the axes the image actually
/// covers, not from a single `sx`/`sy` pair assigned by position — assigning
/// them by position is the transposition bug, and it produces a layer that is
/// the right shape on a square page and wrong on every other.
///
/// # Corners, not edges
///
/// Each word's rectangle is mapped as **two opposite corners** and then
/// re-normalised with [`Rect::from_corners`], rather than by mapping `llx`,
/// `lly`, `urx`, `ury` independently and reassembling them in place. Under an
/// odd quarter turn "lower-left" becomes "upper-left", so a component-wise
/// map produces a rectangle whose `lly` exceeds its `ury` — an inverted box
/// that many consumers will silently treat as empty, giving a text layer that
/// is present, correct, and selects nothing.
#[must_use]
pub fn words_to_page_space_on(
    words: &[RecognizedWord],
    image_width: u32,
    image_height: u32,
    placement: PagePlacement,
) -> Vec<RecognizedWord> {
    if image_width == 0 || image_height == 0 {
        return Vec::new();
    }
    let rect = placement.rect;
    let (iw, ih) = (f64::from(image_width), f64::from(image_height));
    let page_w = rect.urx - rect.llx;
    let page_h = rect.ury - rect.lly;

    // At an odd quarter turn the raster is the page transposed, so the image's
    // x axis measures the page's HEIGHT and vice versa. Deriving each scale
    // from the axis it actually spans is what makes the odd cases work on a
    // non-square page.
    let (sx, sy) = if placement.rotate == 90 || placement.rotate == 270 {
        (page_h / iw, page_w / ih)
    } else {
        (page_w / iw, page_h / ih)
    };

    // One corner mapper, applied to both corners of every word. Written once
    // so the two corners cannot be mapped by different arithmetic — which is
    // the other way a rotation fix goes wrong, and a way that looks right on
    // any word whose box happens to be square.
    let to_user = |ix: f64, iy: f64| -> (f64, f64) {
        match placement.rotate {
            90 => (rect.llx + iy * sy, rect.lly + ix * sx),
            180 => (rect.urx - ix * sx, rect.lly + iy * sy),
            270 => (rect.urx - iy * sy, rect.ury - ix * sx),
            _ => (rect.llx + ix * sx, rect.ury - iy * sy),
        }
    };

    words
        .iter()
        .map(|w| {
            let (ax, ay) = to_user(w.rect.llx, w.rect.lly);
            let (bx, by) = to_user(w.rect.urx, w.rect.ury);
            RecognizedWord {
                text: w.text.clone(),
                rect: Rect::from_corners(ax.min(bx), ay.min(by), ax.max(bx), ay.max(by)),
                confidence: w.confidence,
            }
        })
        .collect()
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

    fn word(
        text: &str,
        llx: f64,
        lly: f64,
        urx: f64,
        ury: f64,
        conf: Option<f32>,
    ) -> RecognizedWord {
        RecognizedWord {
            text: text.to_string(),
            rect: Rect::from_corners(llx, lly, urx, ury),
            confidence: conf,
        }
    }

    /// ★ The y-flip, which is the defect this module exists to make impossible.
    ///
    /// A word at the TOP of the image (small y, because image rows count down)
    /// must land at the TOP of the page (large y, because PDF counts up). Get
    /// this backwards and the page still looks perfect while every selection
    /// lands on the wrong line — the failure nobody sees until they try to use
    /// it.
    #[test]
    fn a_word_at_the_top_of_the_image_lands_at_the_top_of_the_page() {
        let page = Rect::from_corners(0.0, 0.0, 612.0, 792.0);
        // Image 1224x1584 (i.e. 2x the page), word in the top 10% of rows.
        let top_word = word("HEADING", 0.0, 0.0, 100.0, 158.0, None);
        let out = words_to_page_space(&[top_word], 1224, 1584, page);
        assert_eq!(out.len(), 1);
        assert!(
            out[0].rect.ury > 700.0,
            "a word in the image's top rows must be near the page TOP (y~792), \
             got lly={} ury={}",
            out[0].rect.lly,
            out[0].rect.ury
        );
    }

    /// And the bottom of the image lands at the bottom of the page.
    ///
    /// Both directions, because a transform that negated without offsetting
    /// would pass the top test alone.
    #[test]
    fn a_word_at_the_bottom_of_the_image_lands_at_the_bottom_of_the_page() {
        let page = Rect::from_corners(0.0, 0.0, 612.0, 792.0);
        let bottom_word = word("FOOTER", 0.0, 1426.0, 100.0, 1584.0, None);
        let out = words_to_page_space(&[bottom_word], 1224, 1584, page);
        assert!(
            out[0].rect.lly < 90.0,
            "a word in the image's bottom rows must be near the page BOTTOM, \
             got lly={} ury={}",
            out[0].rect.lly,
            out[0].rect.ury
        );
    }

    /// The converted rect is always normalised, whichever way the flip runs.
    #[test]
    fn the_converted_rect_is_never_inverted() {
        let page = Rect::from_corners(0.0, 0.0, 612.0, 792.0);
        let out = words_to_page_space(&[word("x", 10.0, 20.0, 60.0, 90.0, None)], 612, 792, page);
        assert!(out[0].rect.lly < out[0].rect.ury, "lly must be below ury");
        assert!(out[0].rect.llx < out[0].rect.urx, "llx must be left of urx");
    }

    /// An unscored word counts as needing review, exactly like a low-scored one.
    ///
    /// The alternative — skipping unscored words — would let an engine that
    /// reports no confidence produce an EMPTY needs-review list and appear
    /// more trustworthy than one that reports honestly. That is precisely
    /// backwards, and it is the kind of thing that reads as a feature.
    #[test]
    fn an_unscored_word_still_needs_review() {
        let page = OcrPage {
            words: vec![
                word("certain", 0.0, 0.0, 1.0, 1.0, Some(0.99)),
                word("doubtful", 0.0, 0.0, 1.0, 1.0, Some(0.40)),
                word("unscored", 0.0, 0.0, 1.0, 1.0, None),
            ],
            confidence_available: true,
        };
        let review = page.words_needing_review(0.8);
        let texts: Vec<&str> = review.iter().map(|w| w.text.as_str()).collect();
        assert!(texts.contains(&"doubtful"), "a low score needs review");
        assert!(
            texts.contains(&"unscored"),
            "an UNSCORED word is exactly as unverified as a low-scored one"
        );
        assert!(!texts.contains(&"certain"));
    }

    /// The mean ignores unscored words rather than inventing a value for them.
    #[test]
    fn the_mean_confidence_skips_unscored_words() {
        let page = OcrPage {
            words: vec![
                word("a", 0.0, 0.0, 1.0, 1.0, Some(0.6)),
                word("b", 0.0, 0.0, 1.0, 1.0, Some(0.8)),
                word("c", 0.0, 0.0, 1.0, 1.0, None),
            ],
            confidence_available: true,
        };
        let mean = page.mean_confidence().expect("two words are scored");
        assert!(
            (mean - 0.7).abs() < 1e-6,
            "expected the mean of the SCORED words (0.7), got {mean}"
        );
    }

    /// An engine that reports nothing yields `None`, not zero.
    ///
    /// Zero would render as "0% confident" — a specific, alarming, and false
    /// claim about text that was never scored either way.
    #[test]
    fn no_confidence_anywhere_is_none_not_zero() {
        let page = OcrPage {
            words: vec![word("a", 0.0, 0.0, 1.0, 1.0, None)],
            confidence_available: false,
        };
        assert_eq!(page.mean_confidence(), None);
    }

    /// A degenerate image size yields nothing rather than dividing by zero.
    #[test]
    fn a_zero_sized_image_yields_no_words() {
        let page = Rect::from_corners(0.0, 0.0, 612.0, 792.0);
        assert!(
            words_to_page_space(&[word("x", 0.0, 0.0, 1.0, 1.0, None)], 0, 100, page).is_empty()
        );
        assert!(
            words_to_page_space(&[word("x", 0.0, 0.0, 1.0, 1.0, None)], 100, 0, page).is_empty()
        );
    }

    // -----------------------------------------------------------------
    // `/Rotate` — the mapping the rasteriser honours and this module did not
    // -----------------------------------------------------------------

    /// On an UPRIGHT page the rotation-aware mapping and the original agree
    /// exactly.
    ///
    /// This is the test that makes `words_to_page_space_on` safe to recommend
    /// as the default. Without it, "use the new one" would be an invitation to
    /// swap a function whose behaviour on the overwhelmingly common case
    /// nobody had checked — and the common case is the one where a regression
    /// would go unnoticed longest, precisely because it is the one that works
    /// today.
    #[test]
    fn upright_placement_agrees_with_the_rotation_blind_mapping() {
        let words = vec![
            word("alpha", 10.0, 20.0, 60.0, 40.0, None),
            word("beta", 100.0, 200.0, 180.0, 230.0, Some(0.9)),
        ];
        let page = Rect::from_corners(0.0, 0.0, 612.0, 792.0);

        let old = words_to_page_space(&words, 1224, 1584, page);
        let new = words_to_page_space_on(&words, 1224, 1584, PagePlacement::upright(page));

        assert_eq!(old.len(), new.len());
        for (a, b) in old.iter().zip(new.iter()) {
            assert_eq!(a.text, b.text);
            assert_eq!(a.confidence, b.confidence);
            for (x, y) in [
                (a.rect.llx, b.rect.llx),
                (a.rect.lly, b.rect.lly),
                (a.rect.urx, b.rect.urx),
                (a.rect.ury, b.rect.ury),
            ] {
                assert!(
                    (x - y).abs() < 1e-9,
                    "upright must be bit-for-bit the old behaviour: {x} vs {y}"
                );
            }
        }
    }

    /// ★ THE ROUND TRIP, which is the only assertion here that could not pass
    /// for the wrong reason.
    ///
    /// A word is placed at a known spot in USER space. It is pushed forward
    /// through `page_device_geometry`'s own published formulas — copied into
    /// the doc comment on `words_to_page_space_on`, and reproduced here
    /// independently — to get the image pixels a rasteriser would have
    /// produced. Those pixels go back through the mapping. The result must be
    /// where it started.
    ///
    /// Asserting fixed expected numbers instead would be asserting this
    /// function's own output, which is `R215`'s blessed-screenshot failure:
    /// it would pin whatever the code did the day it was written, including a
    /// transposition, and go green forever. A round trip cannot do that,
    /// because the forward half is the RENDERER's contract and not this
    /// function's.
    #[test]
    fn every_rotation_round_trips_a_known_user_space_rectangle() {
        // A deliberately NON-SQUARE page and a NON-ORIGIN box. A square page
        // hides every transposition bug, and a box at the origin hides every
        // dropped-offset bug — the two commonest ways this arithmetic goes
        // wrong, and both invisible to the tidy case.
        let page = Rect::from_corners(20.0, 30.0, 632.0, 822.0);
        let (pw, ph) = (page.urx - page.llx, page.ury - page.lly);
        let s = 2.0_f64;

        // An asymmetric word box, so a mirrored result cannot coincide with a
        // correct one.
        let truth = Rect::from_corners(120.0, 300.0, 260.0, 340.0);

        for rotate in [0_u16, 90, 180, 270] {
            // The forward map, exactly as `page_device_geometry` documents it.
            let fwd = |x: f64, y: f64| -> (f64, f64) {
                match rotate {
                    90 => ((y - page.lly) * s, (x - page.llx) * s),
                    180 => ((page.urx - x) * s, (y - page.lly) * s),
                    270 => ((page.ury - y) * s, (page.urx - x) * s),
                    _ => ((x - page.llx) * s, (page.ury - y) * s),
                }
            };
            // …and the pixmap the renderer would have made, axes swapped on an
            // odd quarter turn exactly as `page_device_geometry` returns them.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let (iw, ih) = if rotate == 90 || rotate == 270 {
                ((ph * s) as u32, (pw * s) as u32)
            } else {
                ((pw * s) as u32, (ph * s) as u32)
            };

            let (ax, ay) = fwd(truth.llx, truth.lly);
            let (bx, by) = fwd(truth.urx, truth.ury);
            let in_image = vec![word(
                "roundtrip",
                ax.min(bx),
                ay.min(by),
                ax.max(bx),
                ay.max(by),
                None,
            )];

            let out = words_to_page_space_on(
                &in_image,
                iw,
                ih,
                PagePlacement::new(page, i32::from(rotate)),
            );
            assert_eq!(out.len(), 1);
            let r = out[0].rect;
            for (got, want, which) in [
                (r.llx, truth.llx, "llx"),
                (r.lly, truth.lly, "lly"),
                (r.urx, truth.urx, "urx"),
                (r.ury, truth.ury, "ury"),
            ] {
                assert!(
                    (got - want).abs() < 1e-6,
                    "/Rotate {rotate}: {which} came back {got}, started at {want}"
                );
            }
            assert!(
                r.ury > r.lly && r.urx > r.llx,
                "/Rotate {rotate}: inverted box"
            );
        }
    }

    /// The rotation-blind mapping is DEMONSTRABLY wrong on a rotated page.
    ///
    /// Without this, the fix has no evidence that it fixed anything: a new
    /// function that agrees with the old one everywhere would pass every test
    /// above and change nothing. This asserts the disagreement is real and
    /// large — not a rounding difference — which is what makes the round-trip
    /// test above a result rather than a tautology.
    #[test]
    fn the_rotation_blind_mapping_is_wrong_on_a_rotated_page() {
        let page = Rect::from_corners(0.0, 0.0, 612.0, 792.0);
        let s = 2.0_f64;
        // A 90-degree page: the raster is 1584 x 1224, the page 612 x 792.
        let (iw, ih) = (1584_u32, 1224_u32);
        let w = vec![word("corner", 40.0, 60.0, 140.0, 100.0, None)];

        let blind = words_to_page_space(&w, iw, ih, page);
        let aware = words_to_page_space_on(&w, iw, ih, PagePlacement::new(page, 90));

        let dx = (blind[0].rect.llx - aware[0].rect.llx).abs();
        let dy = (blind[0].rect.lly - aware[0].rect.lly).abs();
        assert!(
            dx > 1.0 || dy > 1.0,
            "if these agreed on a rotated page the fix would be a no-op: \
             blind={:?} aware={:?}",
            blind[0].rect,
            aware[0].rect
        );
        let _ = s;
    }

    /// Table 30 permits negative and over-turned values; they normalise.
    ///
    /// A page carrying `/Rotate -90` is conformant and a reader that treated
    /// it as `0` would mis-place every word on it while reporting nothing. And
    /// a NON-multiple of 90 is malformed — this normalises to the nearest
    /// quarter turn rather than refusing, because losing an entire OCR run
    /// over a stray dictionary value helps nobody, and because whatever it
    /// picks must match what the renderer picked.
    #[test]
    fn rotation_values_normalise_the_way_table_30_allows() {
        let page = Rect::from_corners(0.0, 0.0, 100.0, 200.0);
        assert_eq!(PagePlacement::new(page, -90).rotate, 270);
        assert_eq!(PagePlacement::new(page, 450).rotate, 90);
        assert_eq!(PagePlacement::new(page, 360).rotate, 0);
        assert_eq!(PagePlacement::new(page, -360).rotate, 0);
        // Malformed, nearest quarter turn.
        assert_eq!(PagePlacement::new(page, 89).rotate, 90);
        assert_eq!(PagePlacement::new(page, 46).rotate, 90);
        assert_eq!(PagePlacement::new(page, 44).rotate, 0);
        // And `upright` says zero out loud.
        assert_eq!(PagePlacement::upright(page).rotate, 0);
    }
}

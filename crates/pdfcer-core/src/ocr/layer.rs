//! The OCR "sandwich" writer — turning recognised words into an **invisible,
//! selectable text layer** over page content that is left completely untouched.
//!
//! # What this module is
//!
//! [`super`] defines the engine-independent *types* — [`RecognizedWord`],
//! [`OcrPage`], the [`OcrEngine`](super::OcrEngine) trait, and the y-flip.
//! **This module is what those types were for**: it takes an [`OcrPage`] whose
//! words are already in PDF user space and writes them into a document.
//!
//! Until this module existed, `pdfcer_core::ocr` had **zero consumers anywhere
//! in the workspace** — a trait, a coordinate conversion, and nothing that
//! produced a PDF. The parent module's own header nonetheless described
//! "writing them into a PDF as an invisible, selectable text layer", which was
//! a promise the code did not keep. Recorded here rather than quietly fixed,
//! because a module doc that describes an intention as though it were a
//! shipped behaviour is exactly the kind of claim `R151` exists to catch.
//!
//! # The mechanism, and its one spec citation
//!
//! ISO 32000-1 **§9.3.6, Table 106, mode 3**: *"Neither fill nor stroke text
//! (invisible)."* The PDF-spec corpus names this by name as the mechanism for
//! OCR text layers (`iso32000__s__9.3.md`), and notes the converse obligation
//! on the renderer — a rasteriser that does **not** honour mode 3 draws the
//! layer as visible garbage across the scan.
//!
//! So the emitted stream is, per page:
//!
//! ```text
//! q                    <- isolate: Tf/Tr/Tz are GRAPHICS state, so Q restores them
//!   BT
//!     3 Tr             <- invisible, once, for every word
//!     /OCR0 <size> Tf  <- per word: the size fitted to that word's box height
//!     <tz> Tz          <- per word: horizontal scaling fitted to its box width
//!     1 0 0 1 <x> <y> Tm
//!     (<winansi>) Tj
//!     ...              <- repeated per word
//!   ET
//! Q
//! ```
//!
//! One `BT…ET` for the whole page is correct because `Tm` is **absolute**, not
//! relative — every word sets its own text matrix outright, so word order in
//! the stream affects extraction order and nothing else.
//!
//! ## Why `q … Q`, and why it is not optional
//!
//! §8.4.2 requires `q`/`Q` to balance within a content stream, and a
//! `/Contents` **array** is defined as the concatenation of its members — so
//! an appended stream inherits whatever graphics state the preceding streams
//! left set. `Tf`, `Tr` and `Tz` are graphics-state parameters (only `Tm` and
//! `Tlm` are reset by `BT`), which means an OCR layer that set `3 Tr` without
//! wrapping would leave **every subsequent stream's text invisible**. The
//! wrapper is the same convention [`crate::text_edit::add_text`] already
//! documents at its §8.4.2 note; this module follows it deliberately rather
//! than by imitation.
//!
//! # Why the scan itself is never re-encoded
//!
//! An OCR layer is purely **additive**: one new content stream appended to
//! `/Contents`, one new font dict, one rewritten page dict. The image object is
//! not in the dirty set, so under an incremental save it is not re-emitted at
//! all (project rule 3). The second reason is the one that matters: **a scan is
//! usually the record of something** — a signed contract, a survey, a stamped
//! drawing — and pushing its JPEG through a decode/re-encode cycle to "help"
//! costs generation loss on an image whose provenance the operator may need to
//! defend. OCR makes a document findable. It does not get to modify it.
//!
//! # Geometry: how a bounding box becomes a font size and a baseline
//!
//! An engine reports an ink bounding box. A PDF viewer computes a selection
//! highlight from the font's ascent/descent times the size, positioned at the
//! baseline. So to make selection land on the ink, the glyph box has to be
//! fitted to the reported box in both axes — and the two axes are fitted by
//! **different** mechanisms, which is the part worth stating plainly:
//!
//! | axis | fitted by | why |
//! |---|---|---|
//! | vertical | the **font size** ([`HELVETICA_ASCENT_FRAC`] + [`HELVETICA_DESCENT_FRAC`]) | size is the only vertical control; there is no vertical-scaling operator short of a full `Tm` |
//! | horizontal | **`Tz`** (horizontal scaling, §9.3.4) | the size is already spent on the vertical fit, so width must come from somewhere else |
//!
//! Vertical: `size = height / (HELVETICA_ASCENT_FRAC + HELVETICA_DESCENT_FRAC)` and the baseline
//! sits at `lly + HELVETICA_DESCENT_FRAC × size`, so the glyph box's top lands at
//! `lly + height` — the reported box top — by construction.
//!
//! Horizontal: the word's natural width at that size is measured through the
//! same Standard-14 metric tables the rest of the crate uses, and
//! `Tz = 100 × target ÷ natural`. `Tz` is a **percentage** and `Th` is the
//! ratio (§9.3.4) — `100 Tz` means `Th = 1.0`, and the corpus flags treating
//! the operand as the ratio as a 100× error, so the percentage is emitted here
//! and the conversion is left where the spec puts it.
//!
//! **This is an approximation and is documented as one.** Helvetica's metrics
//! are not the scanned face's metrics; the fit is exact at the word box's
//! edges and drifts within it. That is the accepted trade in every sandwich
//! implementation, because the alternative — per-glyph positioning derived
//! from per-glyph boxes — needs data most engines do not report.
//!
//! # Rule 4 as amended by decision 059 (2026-08-13)
//!
//! **OCR is the single largest inference pdfcer makes: every word here is a
//! guess.** The amended rule 4 says what to do about that, and this module is
//! shaped by it:
//!
//! - **The result looks normal the instant the command completes.** Mode 3 is
//!   not a compromise here, it is the whole point — the page renders
//!   pixel-identically because nothing visible was added. The operator asked
//!   for exactly this: *"I want OCRed stuff to look normal when the command is
//!   executed too."* There is **no** highlighting of low-confidence words baked
//!   into the page, and there must never be: that would be a second rendering
//!   path for the same content, which is the bug class decision 059 deletes.
//! - **The disclosure is [`OcrLayerReport`], and it is off-canvas.** Mean
//!   confidence, the count needing review, the words that could not be encoded,
//!   the words that were skipped and why. A shell shows it in a panel; the CLI
//!   prints it. What rule 4 forbids is **silence**, not visibility of the text.
//! - **`confidence_available == false` is disclosed as its own fact**, never
//!   flattened into "no low-confidence words found". An engine that reports
//!   nothing must not look better than one that reports honestly — the same
//!   principle [`OcrPage::words_needing_review`](super::OcrPage::words_needing_review)
//!   already encodes by counting unscored words as needing review.
//!
//! # Where this deliberately differs from `add_text`
//!
//! [`crate::text_edit::add_text`] **refuses by name** (`R71`) when the chosen
//! face lacks a glyph for a character the operator typed. That is right for
//! typed text: the operator typed one string, and silently writing a different
//! one is the sneaky failure rule 4 names.
//!
//! **This module substitutes and discloses instead**, and the difference is
//! not laziness. OCR output is bulk machine text, hundreds of words per page,
//! that the operator never typed and cannot proofread in advance. Refusing an
//! entire page's text layer because one recognised word contains a character
//! outside WinAnsi would make the feature unusable on exactly the documents
//! that need it most — and would refuse on a *guess*, which is a strange thing
//! to hold to a stricter standard than a deliberate keystroke. The substitution
//! is counted per word and reported ([`OcrLayerReport::words_substituted`]),
//! so it is disclosed rather than silent, which is what the rule actually asks.
//!
//! **The limit this leaves is real and is named**: a Standard-14 WinAnsi face
//! cannot represent CJK, Cyrillic, Greek or Arabic at all. Recognising those
//! scripts needs an embedded composite font, which is its own slice — see
//! [`OcrLayerReport::words_substituted`] for how a caller detects that it has
//! landed in that case rather than discovering it from a page of `?`.

use crate::document::Document;
use crate::fontdata::{self, BaseEncoding, Std14};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::{self, PageTreeError};
use crate::span::ByteSpan;
use crate::text_edit::addtext::pick_font_name;
use crate::text_edit::edit::make_raw_stream;
use crate::vartext::{encode_winansi, standard14_font_dict};
use crate::writer::content::{emit_literal_string, emit_number};
use crate::writer::{DirtySet, SaveOptions, WriteError, save_incremental};

use super::OcrPage;

/// Ascent as a fraction of the font size, for the vertical fit.
///
/// Helvetica's `Ascender` is 718/1000 em (and its `CapHeight` is the same
/// value, which is why an all-caps word and a mixed-case word fit the same
/// way). Paired with [`HELVETICA_DESCENT_FRAC`] this defines the glyph box the
/// vertical fit solves against.
///
/// # ★ Why the name carries the face, and is not just `ASCENT_FRAC`
///
/// `pdfcer-core` already contains `ASCENT_FRAC` / `DESCENT_FRAC` — twice, in
/// `text_edit::addtext` and `text_edit::reflow` — holding **0.75 / 0.25**.
/// Those are the *block model's* nominal figures, deliberately shared between
/// the two so a new run's box and a reflowed line's box agree with each other.
/// These are the *real AFM metrics of one specific face*, used because this
/// module is solving a fit against a font it chose itself.
///
/// **Both are correct, and a third module adding a fourth `ASCENT_FRAC` would
/// also be correct.** That is exactly the problem: under the bare name, a grep
/// for the identifier returns the wrong constant about half the time, and the
/// two differ by 0.043 em — small enough to look like a rounding artefact
/// rather than a different quantity. The 0.558 pt residual in this module's
/// integration test is that difference, measured. Naming the face is what makes
/// the collision impossible to have by accident.
pub const HELVETICA_ASCENT_FRAC: f64 = 0.718;

/// Descent as a fraction of the font size, for the vertical fit.
///
/// Helvetica's `Descender` is −207/1000 em, taken here as a positive
/// magnitude. This is what lifts the baseline off the bottom of the reported
/// box: without it, a word with a descender would have its tail hang below the
/// ink the engine actually saw, and every selection would sit low.
pub const HELVETICA_DESCENT_FRAC: f64 = 0.207;

/// The smallest horizontal scaling emitted, as a `Tz` percentage.
///
/// A clamp floor, not a preference. It exists because a degenerate box (a word
/// box one point wide holding a ten-character word) would otherwise produce a
/// scaling near zero, and a zero-width text run is unselectable — the layer
/// would silently contain a word nobody can reach. Clamping is counted and
/// disclosed ([`OcrLayerReport::words_scale_clamped`]).
pub const MIN_TZ: f64 = 1.0;

/// The largest horizontal scaling emitted, as a `Tz` percentage.
///
/// The mirror of [`MIN_TZ`]: a one-character word inside a very wide box
/// (a common artefact when an engine merges a rule line into a word box)
/// would otherwise stretch a single glyph across the page and swallow every
/// selection near it.
pub const MAX_TZ: f64 = 10_000.0;

/// Options for building an OCR text layer.
///
/// `#[non_exhaustive]`: the layer's shape is expected to grow (an embedded
/// composite face for non-Latin scripts is the known next axis), and a struct
/// literal at a call site outside the crate would break when it does.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OcrLayerOptions {
    /// The Standard-14 face the invisible text is written in.
    ///
    /// It is never seen, so this is not an aesthetic choice — it selects the
    /// **metric table** the horizontal fit measures against, and therefore how
    /// closely a selection highlight tracks the ink. Helvetica is the default
    /// because its widths are the closest of the fourteen to the proportional
    /// sans faces most scanned business documents are set in.
    pub font: Std14,
}

impl Default for OcrLayerOptions {
    fn default() -> Self {
        Self {
            font: Std14::Helvetica,
        }
    }
}

impl OcrLayerOptions {
    /// A fresh set of options (Helvetica).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Write the layer in a different Standard-14 face.
    #[must_use]
    pub fn with_font(mut self, font: Std14) -> Self {
        self.font = font;
        self
    }
}

/// What the layer write did, and everything it inferred — the rule-4
/// disclosure, in the off-canvas form decision 059 requires.
///
/// Every field here answers a question a shell or the CLI must be able to put
/// in front of the operator **without** marking anything on the page. Nothing
/// in this struct is optional to surface: a caller that builds a layer and
/// drops the report has made pdfcer silent about a page of guesses.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct OcrLayerReport {
    /// How many words were written into the layer.
    pub words_written: usize,
    /// How many words were dropped because they could not be positioned.
    ///
    /// A word is dropped only when it is geometrically meaningless — empty
    /// text, a zero-or-negative-area box, a non-finite coordinate. It is a
    /// count rather than a silent filter because a large number here means the
    /// engine and the page geometry disagree, which is a real diagnosis and
    /// not a detail.
    pub words_skipped: usize,
    /// How many words contained at least one character with no WinAnsi code.
    ///
    /// Each such character was written as `?`. **A high count relative to
    /// [`Self::words_written`] means the page is in a script a Standard-14
    /// face cannot represent** (CJK, Cyrillic, Greek, Arabic) — the named
    /// limit from this module's header, detectable here rather than by reading
    /// a page of question marks.
    pub words_substituted: usize,
    /// How many words had their horizontal scaling clamped to
    /// [`MIN_TZ`]/[`MAX_TZ`].
    ///
    /// Usually an engine artefact (a merged rule line, a box collapsed to a
    /// sliver) rather than a pdfcer fault, which is exactly why it is reported:
    /// it is the operator's cue that a selection in that spot will not track
    /// the ink.
    pub words_scale_clamped: usize,
    /// Mean confidence across words that reported one, or `None`.
    ///
    /// `None` means *no word reported a confidence*, which is a different
    /// statement from "confidence is low" and must be presented as one.
    pub mean_confidence: Option<f32>,
    /// Whether the engine reported per-word confidence **at all**.
    ///
    /// Carried through from [`OcrPage::confidence_available`] so a caller can
    /// say *"this engine reports no per-word confidence"* rather than
    /// presenting unscored guesses as though they had been checked.
    pub confidence_available: bool,
    /// The object number of the created content stream, or 0 before saving.
    pub content_object: u32,
    /// The object number of the created font dictionary, or 0 before saving.
    pub font_object: u32,
}

impl OcrLayerReport {
    /// Human-readable disclosure lines, ready for a CLI to print or a panel to
    /// list.
    ///
    /// Built here rather than at each call site so the GUI and the CLI cannot
    /// disagree about what was disclosed — the same reason
    /// [`crate::text_edit::add_text`] carries its disclosures on the report.
    /// Deliberately says **nothing** when there is nothing to say: a report
    /// that always emits a paragraph trains the reader to skip it.
    #[must_use]
    pub fn disclosures(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "OCR text layer: {} word(s) written, invisible (text rendering \
             mode 3) — the page renders exactly as it did before.",
            self.words_written
        ));
        if self.confidence_available {
            if let Some(mean) = self.mean_confidence {
                out.push(format!(
                    "Mean recognition confidence {:.1}%. Every word is a guess; \
                     review before relying on the text.",
                    f64::from(mean) * 100.0
                ));
            }
        } else {
            out.push(
                "This engine reports NO per-word confidence, so no word here \
                 has been scored either way — that is not the same as a high \
                 score."
                    .to_owned(),
            );
        }
        if self.words_substituted > 0 {
            out.push(format!(
                "{} word(s) contained characters with no WinAnsi code and were \
                 written with '?' substitutions — a Standard-14 face cannot \
                 represent non-Latin scripts.",
                self.words_substituted
            ));
        }
        if self.words_skipped > 0 {
            out.push(format!(
                "{} word(s) were skipped: empty text or a degenerate bounding \
                 box.",
                self.words_skipped
            ));
        }
        if self.words_scale_clamped > 0 {
            out.push(format!(
                "{} word(s) had their horizontal scaling clamped; a selection \
                 there will not track the ink exactly.",
                self.words_scale_clamped
            ));
        }
        out
    }
}

/// A failure to write an OCR layer. Every variant is a named, clean outcome.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OcrLayerError {
    /// The page index is past the end of the document.
    #[error("page index {0} is out of range")]
    PageIndex(usize),
    /// The document is encrypted; this path does not re-encrypt what it writes.
    #[error("the document is encrypted — decrypt it before adding an OCR layer")]
    Encrypted,
    /// The recognised page contained no word that could be written.
    ///
    /// A named refusal rather than a zero-word success, because writing an
    /// empty content stream and a font nobody uses would grow the file and
    /// change its bytes to accomplish nothing — and would report "done" for a
    /// page where OCR in fact found nothing.
    #[error("no recognised words could be written for this page")]
    NothingToWrite,
    /// Two entries in one session run named the same page.
    ///
    /// # ★ Why this is a refusal and not a silent merge
    ///
    /// [`crate::edit::EditSession::add_ocr_layer`] plans every page against
    /// the graph as it stands **before** the command is committed — that is
    /// what lets a multi-page run be one undo entry. Two entries for one page
    /// would therefore both append to that page's *original* `/Contents`, and
    /// the second page-dictionary write would clobber the first. One layer
    /// written, one layer paid for and lost, and a report claiming both.
    ///
    /// Merging them instead would mean deciding what "both layers on one page"
    /// means, which is a question the caller is better placed to answer by
    /// merging the word lists before it calls.
    #[error("page {page_index} appears more than once in one OCR run")]
    DuplicatePage {
        /// The page index that appeared twice.
        page_index: usize,
    },
    /// The document's `/Size` is smaller than the highest object number in
    /// use, so objects exist that a writer cannot see.
    ///
    /// The [`crate::text_edit::AddTextError::HiddenObjects`] sibling, refused
    /// for the same reason and in the same position: allocating a new object
    /// number in a document whose numbering already lies is how a write lands
    /// on top of something.
    #[error("the document hides {count} object(s) behind an undersized /Size")]
    HiddenObjects {
        /// How many objects are unreachable through `/Size`.
        count: usize,
    },
    /// The page object is not a dictionary.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// The page tree could not be walked.
    #[error(transparent)]
    PageTree(#[from] PageTreeError),
    /// The document has no free object numbers left.
    #[error("no free object numbers remain")]
    ObjectNumbersExhausted,
    /// The document is certified and its enforced DocMDP forbids the change.
    ///
    /// The [`crate::text_edit::add_text`] sibling of this refusal, for the
    /// same reason and by the same machinery: writing an OCR layer creates a
    /// content stream and a font and rewrites the page dict, and §12.8.4
    /// Table 258 requires a consumer to enforce `/Perms` -> `/DocMDP`.
    /// Deliberately conservative -- every enforced certification is treated
    /// as forbidding -- because over-refusal is fail-clean and the
    /// alternative is a silently-invalidated signature.
    #[error(
        "the document is certified with DocMDP permission {permission}, which forbids adding an OCR layer"
    )]
    CertificationForbidsChange {
        /// The `/P` value from the DocMDP transform parameters (Table 254
        /// default 2 when absent).
        permission: u8,
    },
    /// The incremental save failed.
    #[error(transparent)]
    Write(#[from] WriteError),
}

/// The saved bytes plus the disclosure report.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OcrLayerOutcome {
    /// The incrementally-saved document.
    pub bytes: Vec<u8>,
    /// What was written and what was inferred.
    pub report: OcrLayerReport,
}

/// One word, resolved to the numbers the content stream actually needs.
///
/// Split out from [`build_layer_content`] so the geometry is testable without
/// parsing emitted bytes — a test that asserts on `size`/`tz`/`baseline`
/// pinpoints a fit regression, where one that greps the stream for a substring
/// only says "something changed".
#[derive(Debug, Clone, PartialEq)]
struct PlacedWord {
    codes: Vec<u8>,
    size: f64,
    tz: f64,
    x: f64,
    baseline_y: f64,
    substituted: bool,
    clamped: bool,
}

/// Measure WinAnsi `bytes` in `font` at `size`, in text-space points.
///
/// §9.4.4: advance = Σ(width/1000) × size, before `Tc`/`Tw`/`Th`. This
/// deliberately measures at `Th = 1.0` because the whole point is to then
/// *solve* for the `Th` that makes the result equal the target width.
fn natural_width(font: Std14, size: f64, bytes: &[u8]) -> f64 {
    let units: u32 = bytes
        .iter()
        .map(|&c| {
            u32::from(
                fontdata::encoding_glyph_name(BaseEncoding::WinAnsi, c)
                    .and_then(|name| fontdata::std14_width(font, name))
                    .unwrap_or(0),
            )
        })
        .sum();
    f64::from(units) / 1000.0 * size
}

/// Fit one recognised word to its box, or reject it as unplaceable.
///
/// Returns `None` for a word that cannot be positioned at all: empty text, a
/// non-finite or non-positive box, or text whose glyphs all have zero advance
/// (which would make the horizontal fit a division by zero). Each of those is
/// counted as a skip by the caller rather than being silently dropped.
fn place_word(word: &super::RecognizedWord, font: Std14) -> Option<PlacedWord> {
    if word.text.is_empty() {
        return None;
    }
    let r = word.rect;
    let (w, h) = (r.urx - r.llx, r.ury - r.lly);
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 || !r.llx.is_finite() {
        return None;
    }

    let (codes, missing) = encode_winansi(&word.text);
    if codes.is_empty() {
        return None;
    }

    // Vertical fit: solve size so the glyph box (ascent+descent) equals the
    // reported box height, then sit the baseline a descender above its bottom.
    let size = h / (HELVETICA_ASCENT_FRAC + HELVETICA_DESCENT_FRAC);
    let baseline_y = HELVETICA_DESCENT_FRAC.mul_add(size, r.lly);

    // Horizontal fit: solve Tz so the natural advance equals the box width.
    let natural = natural_width(font, size, &codes);
    if !natural.is_finite() || natural <= 0.0 {
        return None;
    }
    let raw_tz = 100.0 * w / natural;
    let tz = raw_tz.clamp(MIN_TZ, MAX_TZ);

    Some(PlacedWord {
        codes,
        size,
        tz,
        x: r.llx,
        baseline_y,
        substituted: missing > 0,
        clamped: (tz - raw_tz).abs() > f64::EPSILON,
    })
}

/// Build the invisible text-layer content stream for one page.
///
/// Pure: no document, no allocation of object numbers, no I/O. Given the words
/// and the resource name the font will be filed under, it returns the exact
/// bytes and the counts that become the report. Kept pure so the emitted
/// stream can be asserted on directly, which is the only way to catch a
/// regression in something whose entire visible effect is *nothing*.
///
/// `font_name` is the `/Resources → /Font` key (without the leading slash),
/// chosen by the caller against the page's existing names so it cannot collide.
#[must_use]
pub fn build_layer_content(
    page: &OcrPage,
    font_name: &[u8],
    opts: &OcrLayerOptions,
) -> (Vec<u8>, OcrLayerReport) {
    let mut out: Vec<u8> = Vec::new();
    let mut report = OcrLayerReport {
        words_written: 0,
        words_skipped: 0,
        words_substituted: 0,
        words_scale_clamped: 0,
        mean_confidence: page.mean_confidence(),
        confidence_available: page.confidence_available,
        content_object: 0,
        font_object: 0,
    };

    let placed: Vec<PlacedWord> = page
        .words
        .iter()
        .filter_map(|w| match place_word(w, opts.font) {
            Some(p) => Some(p),
            None => {
                report.words_skipped += 1;
                None
            }
        })
        .collect();

    if placed.is_empty() {
        return (out, report);
    }

    // `q` before `BT`: Tf/Tr/Tz are graphics state and MUST NOT leak into the
    // streams that follow this one in the /Contents array (§8.4.2).
    out.extend_from_slice(b"\nq\nBT\n3 Tr\n");

    // Emitted per word rather than hoisted: two adjacent words almost never
    // share a size, so tracking "has it changed" would save a handful of bytes
    // in exchange for a state machine that can be wrong. An OCR layer is
    // machine output; byte-thrift here buys nothing a reader will ever see.
    for p in &placed {
        out.push(b'/');
        out.extend_from_slice(font_name);
        out.push(b' ');
        emit_number(&mut out, p.size);
        out.extend_from_slice(b" Tf\n");

        emit_number(&mut out, p.tz);
        out.extend_from_slice(b" Tz\n");

        out.extend_from_slice(b"1 0 0 1 ");
        emit_number(&mut out, p.x);
        out.push(b' ');
        emit_number(&mut out, p.baseline_y);
        out.extend_from_slice(b" Tm\n");

        emit_literal_string(&mut out, &p.codes);
        out.extend_from_slice(b" Tj\n");

        report.words_written += 1;
        report.words_substituted += usize::from(p.substituted);
        report.words_scale_clamped += usize::from(p.clamped);
    }

    out.extend_from_slice(b"ET\nQ\n");
    (out, report)
}

/// Everything one page's OCR layer needs, resolved against a graph, with
/// **nothing allocated and nothing written**.
///
/// # ★★ WHY THE PLANNING IS SPLIT FROM THE WRITING
///
/// Because there are now two writers with genuinely different allocation
/// models, and the *decisions* between them must not be made twice:
///
/// - [`add_ocr_layer`] is a one-shot on an immutable [`Document`]: it takes
///   object numbers from `next_object_number()` and stages content bytes at
///   `doc.bytes().len()`.
/// - [`crate::edit::EditSession::add_ocr_layer`] is a session command: it
///   takes numbers from the session's allocator and stages bytes through the
///   session's own R45 buffer, and it does this for **several pages under one
///   undo entry**.
///
/// Everything before that fork — which font name avoids a collision, which
/// words could be placed, what the `/Resources` merge has to preserve, what
/// the content stream says — is identical, and is exactly the part that is
/// expensive to get right. This mirrors `text_edit::addtext::plan_add_text`
/// and `AddTextPrep`, deliberately and structurally, because that pair solved
/// the same problem for the same three-object append.
///
/// ★ The prep holds **no object numbers**. That is the whole point: a plan
/// that had already allocated could not be reused by a caller with a different
/// allocator, and a plan that allocated *per page* could not be collected into
/// one command.
pub(crate) struct OcrLayerPrep {
    /// The page object being modified.
    pub(crate) page_id: ObjId,
    /// The page dict as it currently stands — base, or the session overlay.
    page_dict: Dict,
    /// The page's current `/Contents` value, the append's input.
    contents_before: Option<Object>,
    /// The page's **effective** `/Resources` minus `/Font`, references intact.
    resources_base: Dict,
    /// The existing `/Font` entries the new font merges into.
    font_subdict_base: Dict,
    /// The collision-free `/Font` name for the OCR font.
    font_name: Vec<u8>,
    /// The `BT … ET` content-stream bytes for the invisible layer.
    pub(crate) content_data: Vec<u8>,
    /// The Standard-14 font dictionary object.
    pub(crate) font_dict: Object,
    /// What was written and what was inferred, minus the two object numbers
    /// the caller fills in once it has allocated them.
    pub(crate) report: OcrLayerReport,
}

impl OcrLayerPrep {
    /// The rewritten page dictionary, given the numbers the caller allocated.
    ///
    /// Takes the graph rather than re-deriving the append, for the reason
    /// `AddTextPrep::build_page_dict` states at length: `/Contents` may be a
    /// **reference to an array**, and a local helper that matched on the raw
    /// value without resolving it produced an array nested inside an array —
    /// on Qt output and on every CAD sheet. One answer, threaded in, not two
    /// correct-looking ones.
    pub(crate) fn build_page_dict<G: ObjectGraph + ?Sized>(
        &self,
        graph: &G,
        content_id: ObjId,
        font_id: ObjId,
    ) -> Dict {
        let mut new_page = self.page_dict.clone();
        new_page.insert(
            Name::from(b"Contents"),
            crate::page_tree::append_content_stream(
                graph,
                self.contents_before.as_ref(),
                content_id,
            ),
        );
        let mut font_subdict = self.font_subdict_base.clone();
        font_subdict.insert(Name(self.font_name.clone()), Object::Reference(font_id));
        let mut resources = self.resources_base.clone();
        resources.insert(Name::from(b"Font"), Object::Dict(font_subdict));
        new_page.insert(Name::from(b"Resources"), Object::Dict(resources));
        new_page
    }
}

/// Plan one page's OCR layer against `graph`, allocating nothing.
///
/// `page` must come from the same graph the caller will write through — the
/// session's overlay for a session command, the base for the one-shot — so
/// that a page edited earlier in the session contributes **its edited**
/// `/Contents` and `/Resources` to the append rather than the base revision's.
/// That divergence is the exact trap the consuming shell reported working
/// around with a refusal, and planning against the caller's own graph is what
/// removes it at the root.
///
/// The words in `ocr_page` must already be in **PDF default user space, y-up**
/// — see [`add_ocr_layer`]. Nothing here flips anything.
///
/// # Errors
///
/// [`OcrLayerError::Unsupported`] if the page object is not a dictionary, and
/// [`OcrLayerError::NothingToWrite`] if every word proved unplaceable. Both
/// happen before the caller allocates anything.
pub(crate) fn plan_ocr_layer<G: ObjectGraph + ?Sized>(
    page: &crate::page_tree::Page,
    ocr_page: &OcrPage,
    opts: &OcrLayerOptions,
    graph: &G,
) -> Result<OcrLayerPrep, OcrLayerError> {
    let page_dict = graph.resolved(page.id).as_dict().cloned().ok_or_else(|| {
        OcrLayerError::Unsupported("the page object is not a dictionary".to_owned())
    })?;
    let contents_before = page_dict.get(b"Contents").cloned();

    // The §7.7.3.4 inheritance-safe recipe, identical to add-text's: take the
    // page's EFFECTIVE resources (own-or-inherited, already resolved by the
    // page-tree walk), strip /Font, and re-add it merged. Writing an own
    // /Resources holding only the new font would shadow an inherited one and
    // silently break every other resource the page uses.
    let font_subdict_base: Dict = match page.resources.get(b"Font") {
        Some(o) => graph.resolve(o).as_dict().cloned().unwrap_or_default(),
        None => Dict::new(),
    };
    let font_name = pick_font_name(&font_subdict_base);
    let mut resources_base = page.resources.clone();
    resources_base.remove(b"Font");

    let (content_data, report) = build_layer_content(ocr_page, &font_name, opts);
    if report.words_written == 0 {
        return Err(OcrLayerError::NothingToWrite);
    }

    Ok(OcrLayerPrep {
        page_id: page.id,
        page_dict,
        contents_before,
        resources_base,
        font_subdict_base,
        font_name,
        content_data,
        font_dict: Object::Dict(standard14_font_dict(opts.font)),
        report,
    })
}

/// Add an invisible OCR text layer to `page_index` of `doc` and return the
/// incrementally-saved bytes plus the disclosure report.
///
/// The words in `ocr_page` must already be in **PDF default user space,
/// y-up** — run [`words_to_page_space`](super::words_to_page_space) on raw
/// engine output first. This function does not flip anything, deliberately: a
/// second place that could flip is a second place that could flip twice.
///
/// # What it writes
///
/// Three objects' worth of change, all additive: a new content stream
/// (appended to the page's `/Contents`), a new Standard-14 font dictionary,
/// and the rewritten page dictionary. The scanned image object is untouched
/// and, under the incremental save, not re-emitted at all (project rule 3).
///
/// # Errors
///
/// [`OcrLayerError`] — an out-of-range page, an encrypted document, a page
/// whose words all proved unplaceable ([`OcrLayerError::NothingToWrite`]), a
/// non-dictionary page object, exhausted object numbers, or a save failure.
/// Every refusal happens **before** any object is allocated.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::document::Document;
/// use pdfcer_core::ocr::{OcrPage, layer};
///
/// let doc = Document::load(std::path::Path::new("scan.pdf"))?;
/// let recognised = OcrPage::default(); // ...from an engine, in page space
/// let out = layer::add_ocr_layer(&doc, 0, &recognised, &layer::OcrLayerOptions::new())?;
/// std::fs::write("searchable.pdf", &out.bytes)?;
/// // Rule 4 (decision 059): render normally, report separately. Both.
/// for line in out.report.disclosures() {
///     eprintln!("{line}");
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn add_ocr_layer(
    doc: &Document,
    page_index: usize,
    ocr_page: &OcrPage,
    opts: &OcrLayerOptions,
) -> Result<OcrLayerOutcome, OcrLayerError> {
    if doc.trailer().contains_key(b"Encrypt") {
        return Err(OcrLayerError::Encrypted);
    }

    // §12.8.4 Table 258: a consumer "shall enforce the permissions" a
    // certification carries. This mirrors `add_text`'s
    // `refuse_if_certification_forbids` exactly -- same `census` +
    // `forbids_structural_change` machinery, same "/P absent => default 2"
    // rule (Table 254) -- and yields an `OcrLayerError` because that is this
    // path's error type. It is here rather than deeper because refusing
    // before doing any work is the difference between a clean refusal and a
    // half-built plan thrown away.
    let census = crate::signature::census(doc);
    if census.forbids_structural_change() {
        return Err(OcrLayerError::CertificationForbidsChange {
            permission: census.certification_permission.unwrap_or(2),
        });
    }

    let pages = page_tree::pages(doc)?;
    let page = pages
        .get(page_index)
        .ok_or(OcrLayerError::PageIndex(page_index))?;

    // ★ The plan is shared with `EditSession::add_ocr_layer` and allocates
    // nothing -- see `plan_ocr_layer`. What differs between the two writers is
    // only where object numbers and staged bytes come from, and that fork
    // starts on the next line.
    let prep = plan_ocr_layer(page, ocr_page, opts, doc)?;
    let mut report = prep.report.clone();

    let content_num = doc
        .next_object_number()
        .ok_or(OcrLayerError::ObjectNumbersExhausted)?;
    let font_num = content_num
        .checked_add(1)
        .ok_or(OcrLayerError::ObjectNumbersExhausted)?;
    let content_id = ObjId::new(content_num, 0);
    let font_id = ObjId::new(font_num, 0);

    let new_page = prep.build_page_dict(doc, content_id, font_id);

    // A SANCTIONED WRITER BYPASS — exception 7's shape exactly (see
    // `tools/check-bypass-paths.sh`): `add_ocr_layer(doc, ..) -> bytes`,
    // operating on a `Document` that is not in an edit session, so there is no
    // undo stack to join and nothing to disclose to a later command. It
    // refuses an encrypted document and an enforced-certified one; that second
    // refusal is what makes this exemption honest, and it was MISSING when the
    // function shipped.
    //
    // ★★★ THIS NOTE HAS BEEN WRONG ABOUT ITS OWN CALLERS THREE TIMES. The
    // wording is deliberately plain now, and the history is kept because the
    // pattern is worth more than the fact.
    //
    //   1. It once said "called by the CLI", when nothing called it.
    //   2. It was corrected to "There is NO OCR subcommand -- `grep -rn "ocr"
    //      crates/pdfcer-cli/src/main.rs` returns nothing. So this is an R151
    //      instance: a capability with no shell caller."
    //   3. That was corrected to "what still has no caller is THIS one-shot".
    //
    // **All three were false when written, and (3) was written while
    // explicitly correcting (2).** Measured 2026-08-27: `pdfcer` has an
    // `ocr` subcommand AND a `fetch-ocr-models` subcommand, "ocr" appears 71
    // times in `main.rs`, and this very function is called from
    // `main.rs:8673`. The grep quoted in (2) does not return nothing and
    // presumably never did.
    //
    // ⇒ **A claim about callers is a MEASUREMENT, and it goes stale silently
    // because nothing recompiles when it does.** Correcting such a claim by
    // reasoning about what changed — rather than by re-running the grep — is
    // how (3) happened: the author knew a new caller had appeared and inferred
    // the rest of the sentence instead of checking it. If you are about to
    // edit this paragraph, run the grep first. It takes a second.
    //
    // The exemption's warrant never depended on who calls it — a one-shot API
    // is outside a session whether a shell reaches it or not — which is
    // precisely why nobody ever had a reason to verify the sentence.
    //
    // Do not copy this marker to a new writer caller without first checking
    // the same two refusals are present.
    //
    // Stage the content bytes into the dirty set's buffer, with the new
    // stream's span in the `base.len() + local` combined coordinate system
    // (R45). The image and the original content stream are NOT in the dirty
    // set, so they are not re-emitted — round-trip, rule 3.
    let mut dirty = DirtySet::empty();
    let start = doc.bytes().len();
    let span = ByteSpan::new(start, prep.content_data.len());
    dirty.replace(content_id, make_raw_stream(span, prep.content_data.len()));
    dirty.replace(font_id, prep.font_dict.clone());
    dirty.replace(prep.page_id, Object::Dict(new_page));
    // bypass-exempt: see the note above this block. The token sits HERE, in
    // the middle of the three writer calls, because the gate's window is
    // eight lines either side of each hit and the calls span eight lines —
    // above the block it covers the first two and misses `save_incremental`.
    dirty.set_staging(prep.content_data.clone());

    let (bytes, _) = save_incremental(doc, &dirty, &SaveOptions::identity())?;

    report.content_object = content_num;
    report.font_object = font_num;
    Ok(OcrLayerOutcome { bytes, report })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use super::*;
    use crate::ocr::RecognizedWord;
    use crate::page_tree::Rect;

    fn word(text: &str, llx: f64, lly: f64, urx: f64, ury: f64) -> RecognizedWord {
        RecognizedWord {
            text: text.to_owned(),
            rect: Rect::from_corners(llx, lly, urx, ury),
            confidence: Some(0.9),
        }
    }

    fn page_of(words: Vec<RecognizedWord>) -> OcrPage {
        OcrPage {
            words,
            confidence_available: true,
        }
    }

    /// ★ The one thing this module exists to guarantee: the text is INVISIBLE.
    ///
    /// `3 Tr` must be present, and must come before any `Tj`. Without it the
    /// layer renders as visible garbage across the scan — the exact failure the
    /// spec corpus warns renderers about, produced here at the writing end
    /// instead. Asserted on the emitted bytes because there is no visual
    /// symptom to catch it any other way.
    #[test]
    fn the_layer_sets_invisible_rendering_mode_before_showing_any_text() {
        let (bytes, _) = page_of(vec![word("HELLO", 10.0, 10.0, 60.0, 22.0)])
            .pipe_build(b"OCR0", &OcrLayerOptions::new());
        let s = String::from_utf8_lossy(&bytes).to_string();
        let tr = s.find("3 Tr").expect("mode 3 must be set");
        let tj = s.find(" Tj").expect("a word must be shown");
        assert!(tr < tj, "3 Tr must precede the first Tj, got {tr} vs {tj}");
    }

    /// ★ The stream is balanced and isolated.
    ///
    /// `Tf`/`Tr`/`Tz` are graphics state, and a `/Contents` array concatenates.
    /// An unwrapped layer would leave `3 Tr` set and make every later stream's
    /// text invisible — a defect that would look like "the OCR broke my
    /// document's existing text", which is a much harder bug to trace back
    /// here than it is to prevent.
    #[test]
    fn the_stream_is_wrapped_so_invisible_mode_cannot_leak() {
        let (bytes, _) = page_of(vec![word("x", 0.0, 0.0, 10.0, 10.0)])
            .pipe_build(b"OCR0", &OcrLayerOptions::new());
        let s = String::from_utf8_lossy(&bytes).to_string();
        assert!(s.contains("q\nBT\n3 Tr"), "must open q then BT then 3 Tr");
        assert!(s.trim_end().ends_with("ET\nQ"), "must close ET then Q: {s}");
        assert_eq!(s.matches('q').count(), 1, "exactly one q");
        assert_eq!(s.matches('Q').count(), 1, "exactly one Q");
    }

    /// The vertical fit puts the glyph box top at the reported box top.
    ///
    /// Solved rather than approximated, so it is asserted exactly: baseline +
    /// ascent must equal the box's `ury`. A regression here shifts every
    /// selection highlight off the ink by a constant, which reads as "OCR is
    /// slightly wrong" and is very hard to attribute.
    #[test]
    fn the_glyph_box_top_lands_on_the_reported_box_top() {
        let p =
            place_word(&word("Ag", 10.0, 100.0, 60.0, 120.0), Std14::Helvetica).expect("placeable");
        let top = HELVETICA_ASCENT_FRAC.mul_add(p.size, p.baseline_y);
        assert!((top - 120.0).abs() < 1e-9, "glyph top {top} should be 120");
        assert!(
            p.baseline_y > 100.0,
            "the baseline sits a descender ABOVE the box bottom, got {}",
            p.baseline_y
        );
    }

    /// The horizontal fit makes the advance equal the reported box width.
    ///
    /// `Tz` is a PERCENTAGE (§9.3.4): `Th = Tz/100`. The corpus flags treating
    /// the operand as the ratio as a 100× error, so the check multiplies the
    /// natural width by `tz/100` and expects the box width back — a test that
    /// would fail loudly if the percentage/ratio confusion were ever
    /// introduced here.
    #[test]
    fn the_advance_is_scaled_to_the_reported_box_width() {
        let w = word("Invoice", 10.0, 100.0, 90.0, 112.0);
        let p = place_word(&w, Std14::Helvetica).expect("placeable");
        let natural = natural_width(Std14::Helvetica, p.size, &p.codes);
        let fitted = natural * p.tz / 100.0;
        assert!(
            (fitted - 80.0).abs() < 1e-6,
            "fitted advance {fitted} should equal the box width 80"
        );
    }

    /// A degenerate box is skipped and COUNTED, never silently dropped.
    ///
    /// Note what is NOT tested here: an inverted box built through
    /// [`Rect::from_corners`], because that constructor **normalises** its
    /// corners, so `(40,0)→(0,12)` arrives as a perfectly ordinary 40-wide
    /// rect. The first draft of this test asserted on exactly that case and
    /// failed — the test was wrong, not the guard. The genuinely inverted case
    /// needs a struct literal and gets its own test below.
    #[test]
    fn unplaceable_words_are_counted_as_skips() {
        let p = page_of(vec![
            word("good", 0.0, 0.0, 40.0, 12.0),
            word("", 0.0, 0.0, 40.0, 12.0),
            word("zero-height", 0.0, 0.0, 40.0, 0.0),
            word("zero-width", 40.0, 0.0, 40.0, 12.0),
        ]);
        let (_, report) = p.pipe_build(b"OCR0", &OcrLayerOptions::new());
        assert_eq!(report.words_written, 1);
        assert_eq!(report.words_skipped, 3, "each unplaceable word is counted");
    }

    /// An inverted or non-finite box — reachable only by building [`Rect`]
    /// through its public fields, which an engine adapter may well do — is
    /// rejected rather than producing a negative size and a `NaN` scaling.
    ///
    /// The guard is defensive by design: nothing in the crate can currently
    /// hand it such a rect, and that is precisely the state in which a guard
    /// quietly stops working and nobody notices.
    #[test]
    fn a_hand_built_inverted_or_nonfinite_box_is_rejected() {
        let inverted = RecognizedWord {
            text: "backwards".to_owned(),
            rect: Rect {
                llx: 40.0,
                lly: 12.0,
                urx: 0.0,
                ury: 0.0,
            },
            confidence: None,
        };
        assert!(place_word(&inverted, Std14::Helvetica).is_none());

        let nan = RecognizedWord {
            text: "nan".to_owned(),
            rect: Rect {
                llx: f64::NAN,
                lly: 0.0,
                urx: 40.0,
                ury: 12.0,
            },
            confidence: None,
        };
        assert!(place_word(&nan, Std14::Helvetica).is_none());
    }

    /// ★ A non-WinAnsi character is substituted and DISCLOSED, not refused.
    ///
    /// This is the deliberate divergence from `add_text`'s R71 refusal, and the
    /// test pins both halves: the layer is still written (so one stray glyph
    /// cannot cost a page its text layer), AND the substitution is counted (so
    /// it is not silent). Either half alone would be the wrong behaviour.
    #[test]
    fn a_non_winansi_word_is_substituted_and_reported_not_refused() {
        let (bytes, report) = page_of(vec![word("日本語", 0.0, 0.0, 40.0, 12.0)])
            .pipe_build(b"OCR0", &OcrLayerOptions::new());
        assert_eq!(report.words_written, 1, "the layer is still written");
        assert_eq!(report.words_substituted, 1, "and the loss is disclosed");
        assert!(!bytes.is_empty());
        let msgs = report.disclosures().join(" ");
        assert!(
            msgs.contains("no WinAnsi code"),
            "the disclosure must name the cause: {msgs}"
        );
    }

    /// An engine with no confidence says so, rather than looking clean.
    ///
    /// The failure this prevents: an engine that reports nothing produces no
    /// low-confidence warnings and therefore reads as MORE trustworthy than one
    /// that reports honestly. The disclosure must state the absence.
    #[test]
    fn an_engine_without_confidence_discloses_the_absence() {
        let page = OcrPage {
            words: vec![RecognizedWord {
                text: "word".to_owned(),
                rect: Rect::from_corners(0.0, 0.0, 40.0, 12.0),
                confidence: None,
            }],
            confidence_available: false,
        };
        let (_, report) = page.pipe_build(b"OCR0", &OcrLayerOptions::new());
        assert_eq!(report.mean_confidence, None, "None, never zero");
        let msgs = report.disclosures().join(" ");
        assert!(
            msgs.contains("NO per-word confidence"),
            "absence must be stated as its own fact: {msgs}"
        );
    }

    /// A collapsed box clamps the scaling and reports it.
    #[test]
    fn an_absurd_box_clamps_the_scaling_and_says_so() {
        let (_, report) = page_of(vec![word("wide", 0.0, 0.0, 100_000.0, 4.0)])
            .pipe_build(b"OCR0", &OcrLayerOptions::new());
        assert_eq!(report.words_scale_clamped, 1);
        assert!(
            report.disclosures().join(" ").contains("clamped"),
            "clamping is disclosed"
        );
    }

    /// An empty page emits no bytes at all, rather than an empty `q…Q`.
    ///
    /// The caller turns this into `NothingToWrite`; emitting a stream and a
    /// font for zero words would grow the file and change its bytes to
    /// accomplish nothing.
    #[test]
    fn a_page_with_no_placeable_words_emits_nothing() {
        let (bytes, report) = page_of(vec![]).pipe_build(b"OCR0", &OcrLayerOptions::new());
        assert!(bytes.is_empty());
        assert_eq!(report.words_written, 0);
    }

    /// The font resource name the caller chose is the one emitted.
    #[test]
    fn the_supplied_font_resource_name_is_used() {
        let (bytes, _) = page_of(vec![word("x", 0.0, 0.0, 10.0, 10.0)])
            .pipe_build(b"pdfceOcr7", &OcrLayerOptions::new());
        assert!(String::from_utf8_lossy(&bytes).contains("/pdfceOcr7 "));
    }

    /// Test-only sugar so each case reads as one line of intent.
    trait PipeBuild {
        fn pipe_build(&self, name: &[u8], opts: &OcrLayerOptions) -> (Vec<u8>, OcrLayerReport);
    }
    impl PipeBuild for OcrPage {
        fn pipe_build(&self, name: &[u8], opts: &OcrLayerOptions) -> (Vec<u8>, OcrLayerReport) {
            build_layer_content(self, name, opts)
        }
    }
}

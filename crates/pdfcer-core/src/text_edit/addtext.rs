//! # `addtext` — synthesize a NEW page-text run and APPEND it (Pass 16.0/16.1 / FF-D)
//!
//! This module implements decision 016 (`docs/decisions/016-ffd-add-new-page-text.md`)
//! slices **16.0** (the *add-new-text engine* plus its **point-text** insert)
//! and **16.1** (the *boxed* multi-line variant). It synthesizes a fresh
//! `BT … ET` text object at operator coordinates and **appends** it to a
//! page's content as an additional content stream, leaving every original
//! content stream **byte-identical**, using a bundled Standard-14 face that
//! needs **no embedding**.
//!
//! ## Two placement modes — point (16.0) and boxed (16.1)
//!
//! - **Point text (16.0).** [`AddTextRequest::origin`] sets one absolute `Tm`
//!   (§9.4.2) and the whole `text` is shown as a single line via one `Tj`
//!   ([`build_content`]). No wrap, no alignment — the honest single-line first
//!   cut.
//! - **Boxed text (16.1).** [`AddTextRequest::with_box`] supplies a rectangle;
//!   the run is **wrapped to the box width** through the SHIPPED 15.x greedy
//!   breaker ([`crate::linebreak::greedy_pack`], measured by the chosen face's
//!   §9.4.4 AFM advances), laid out **top-anchored** from the box top at a
//!   derived-or-specified leading, and emitted as ONE multi-line `BT…ET` using
//!   the 15.1 origin/justify recipe ([`crate::text_edit::reflow::align_origin_x`]
//!   for per-line origins, negative `TJ` numbers for justified slack — see
//!   `iso32000__ref__reflow_emission.md` §1/§3). Alignment is an **explicit
//!   operator input** (default LEFT): a fresh box has no existing glyphs to
//!   auto-detect from, so 16.1 does NOT use the 15.0 x-position auto-detect
//!   path — L/C/R/justify are all supported and simply *placed*. Both the box
//!   and the append/font/resources machinery are otherwise IDENTICAL to 16.0
//!   (same one undo-able [`CommandKind::AddText`](crate::edit::CommandKind::AddText),
//!   same byte-identical-original guarantee).
//!
//! ## Overflow — disclose and EMIT, never clip (R76, decision 016 §6/16.1)
//!
//! A boxed run whose wrapped height exceeds the box is a disclosed condition,
//! not an error: pdfcer reports *"boxed text overflows the box by N line(s)"*
//! and emits **every** line as real recoverable page content at its true
//! position. If growth additionally passes the page cropbox bottom, pdfcer also
//! reports *"grows M pt past the page"* and STILL emits the off-page lines
//! (they are recoverable content, never clipped or dropped) — the same
//! disclose-and-allow posture as the 15.x reflow engine's [`crate::text_edit::PageOverflow`].
//!
//! It is deliberately NOT the Pass-6.2 FreeText annotation path (**R78**): this
//! is genuine static page content — an *append* kin to R47's sanctioned
//! page-content edits, editable afterward by the same 14.1 surgery / 14.2
//! formatting / 15.x reflow as any other run (decision 016 §3.1/§3.4). The two
//! Acrobat "Add Text" features (Edit-PDF page content vs Fill&Sign `/FreeText`)
//! are a real, sourced naming collision with different flatten/permission/
//! removal semantics; conflating them would silently ship the wrong feature.
//!
//! ## The five objects touched, and the byte-identical guarantee
//!
//! An add-text APPEND creates/modifies EXACTLY (spec grounding
//! `iso32000__ref__page_content_append.md` §0):
//!
//! | # | Object | Action |
//! |---|---|---|
//! | 1 | the page dict | `/Contents` value single→array (append); `/Resources` (re)built inline |
//! | 2 | NEW content stream | created — holds the `q BT…ET Q` run |
//! | 3 | NEW Standard-14 font dict | created — Type1, no `/FontFile` |
//!
//! **The original content stream object(s) are NEVER in this list ⇒
//! byte-identical (R32/R46).** Only the page dict's `/Contents` *reference*
//! changes; the stream it points at is untouched. Default **incremental save**
//! (R34/R36) — the modified page dict + the two new objects are written in a
//! new update section; NOT redaction's full rewrite (R35). One undo-able
//! [`CommandKind::AddText`](crate::edit::CommandKind::AddText).
//!
//! ## `/Contents` single→array append (§7.7.3.3)
//!
//! Table 30's `/Contents` *"shall be either a single stream or an array of
//! streams … the effect shall be as if all of the streams in the array were
//! concatenated, in order"* — and *"conforming writers shall not create a
//! `Contents` array containing no elements."* So (see
//! [`crate::page_tree::append_content_stream`]):
//! `R_orig` → `[R_orig R_new]`; `[R1…Rk]` → `[R1…Rk R_new]`; absent → `R_new`.
//! The new run is appended at the END so it executes last and paints ON TOP
//! (§8.2 painter's model) — what "add text on top of the page" requires.
//!
//! ## Graphics-state isolation — the load-bearing correctness caveat (§8.4.2)
//!
//! The array concatenates into ONE logical stream and **graphics state is
//! initialized ONCE at page start, not between array elements** (Table 52). The
//! appended stream therefore *inherits* whatever state the prior stream(s)
//! left. Two consequences the emitted run ([`build_content`]) honors:
//!
//! 1. **Wrap in `q … Q`** — §8.4.2 requires q/Q to balance *"within the
//!    sequence of streams specified in a page dictionary's `Contents` array"*;
//!    the original is already self-balanced, so the appended run MUST be too,
//!    and `q…Q` also confines any state it sets (forward-proof for a later
//!    append).
//! 2. **Set every relied-on parameter explicitly** — `q`/`Q` copy/restore the
//!    *inherited* state, they do NOT reset to Table-52 initials. So the run
//!    emits `Tf` (mandatory, no default — §9.3.1), an explicit fill colour
//!    (`0 g`/`r g b rg` — black is only the *page-initial* default, §8.6), and
//!    an absolute `Tm` (REPLACES `Tm`/`Tlm`, §9.4.2). The origination sequence
//!    (§9.4.2/§9.4.3): `q BT /pdfceF1 <size> Tf <colour> 1 0 0 1 <x> <y> Tm
//!    (codes) Tj ET Q`, with one leading `\n` (0x0A) as the token separator so
//!    no token spans the array-element boundary (§7.2).
//!
//! ## `/Resources` `/Font` add + the inheritance trap (§7.8.3, §7.7.3.4)
//!
//! The run's `Tf` names a `/Font` resource resolved against the page's
//! **effective** `/Resources` (own or inherited). This module adds ONE `/Font`
//! entry with a **collision-free** name (walk the effective `/Font` subdict;
//! use `/pdfceF1`, or the first unused `/pdfceF{n}`). The trap (§7.7.3.4):
//! if the page **omits** `/Resources` it *inherits* the ancestor `/Pages`
//! node's dict, which is **SHARED by sibling pages** — mutating it would add
//! the font to unrelated pages. So this module gives THIS page its **own**
//! `/Resources` that references the same indirect sub-dictionaries as the
//! inherited one, EXCEPT `/Font`, which becomes a fresh **merged** subdict
//! (inherited fonts + the new font). The shared ancestor is never touched. The
//! same reference-not-mutate discipline covers a page whose own `/Resources`
//! (or `/Font` subdict) is an indirect object shared elsewhere. This is
//! disclosed (R73-adjacent honesty, rule 4) when it happens.
//!
//! ## The Standard-14 no-embed font dict (§9.6.2.2)
//!
//! `<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding
//! /WinAnsiEncoding /FirstChar 32 /LastChar 255 /Widths [ … ] >>` — **no
//! `/FontFile`** (**R79**: no glyph embedding; the program lives in the
//! reader's built-in std-14, §9.6.2.2). pdfcer emits the **full `/Widths`
//! form** (the spec grounding's recommendation): the std-14 special treatment
//! is *deprecated at PDF 1.5* (*"conforming writers should represent all fonts
//! using a complete font descriptor"*), and pdfcer already owns the AFM widths
//! (`fontdata::std14_width`, free) — so the run is self-contained and forward-
//! safe. `/FontDescriptor` is omitted deliberately: it is optional for a
//! std-14 font, and §9.6.2.1/Table 111 would require it to be an *indirect*
//! reference (a third object) for no metric gain here — the 3-key minimal form
//! remains a valid fallback, and `/Widths` alone carries the layout. `/Encoding
//! /WinAnsiEncoding` is written for the 12 Latin faces and **omitted for
//! `Symbol`/`ZapfDingbats`** (built-in encodings, Annex D.5/D.6 — WinAnsi would
//! yield wrong glyphs).
//!
//! ## Encoding the new string (§9.10.2 inverse — the EASY direction)
//!
//! Because pdfcer *chooses* the encoding and *controls* the font, this is the
//! simplest inverse case: each Unicode scalar → single-byte code by inverting
//! the **fixed** `/WinAnsiEncoding` (or the symbolic built-in) table, via the
//! shared [`InverseEncoding`](crate::text_edit::encoding::InverseEncoding) —
//! never by inverting `/ToUnicode` (one-way/lossy). A scalar outside the face's
//! repertoire is **refused-and-disclosed** (**R71** / F-refuse), never faked:
//! the [`AddTextError::Refused`] carries the named [`Refusal`].
//!
//! ## Tagged page — disclose untagged, never corrupt (**R73**)
//!
//! On a tagged page (`/StructTreeRoot` or `/MarkInfo /Marked true`) the new run
//! is emitted as plain **untagged** page content and pdfcer **discloses** that
//! the structure tree / reading order was not updated and no tag was created.
//! pdfcer never fabricates a mis-placed structure element (rule 4 / R73);
//! minimal StructTree insertion for new content is the deferred FF-H.
//!
//! ## GUI-core separation
//!
//! Everything here is `pdfcer-core`: it takes plain page-space coordinates and a
//! [`fontdata::Std14`] face, and writes bytes. Whether a bundled or an
//! operator-supplied face *renders* the preview is a shell concern (decision
//! 012); this module records the caller's [`FontProvenance`] choice in the
//! report but writes an identical named non-embedded dict either way
//! (`ARCHITECTURE.md` §3).

use std::collections::BTreeSet;

use crate::document::Document;
use crate::font_embed::FontEmbedPlan;
use crate::fontdata::{self, BaseEncoding, Std14};
use crate::graph::ObjectGraph;
use crate::linebreak::greedy_pack;
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::{self, Page, PageTreeError, Rect};
use crate::span::ByteSpan;
use crate::text_edit::edit::make_raw_stream;
use crate::text_edit::encoding::{InverseEncoding, Refusal};
use crate::text_edit::reflow::{BlockAlignment, align_origin_x, line_natural_width};
use crate::writer::content::{emit_literal_string, emit_number};
use crate::writer::{DirtySet, SaveOptions, WriteError, save_incremental};

/// Ascent as a fraction of the effective size — the SAME 0.75·size the 14.0
/// block model and the 15.x reflow engine use for their line boxes, so a
/// boxed add's first baseline (`box_top − ascent`) and its re-recognised block
/// box agree. Kept in lockstep with `reflow::ASCENT_FRAC`.
const ASCENT_FRAC: f64 = 0.75;
/// Descent as a fraction of the effective size (matches the block model /
/// reflow `DESCENT_FRAC`): the boxed run's bottom is `last_baseline −
/// 0.25·size`, used for the box/page overflow tests (R76).
const DESCENT_FRAC: f64 = 0.25;
/// Default leading (baseline-to-baseline) as a multiple of size when the
/// operator does not specify one. A fresh box has no existing baselines to
/// measure (unlike a reflow of existing text), so 16.1 defaults to the SAME
/// 1.2·size fallback the reflow engine uses when a single-line block offers no
/// gap — disclosed as a derived default (rule 4).
const DEFAULT_LEADING_FRAC: f64 = 1.2;
/// Fallback inter-word space width (points, as a fraction of size) when the
/// chosen face reports a zero advance for its space code — a defensive floor
/// so wrapping never divides a run into zero-width gaps. Disclosed if used.
const FALLBACK_SPACE_FRAC: f64 = 0.25;
/// Float slack for "strictly past the edge" overflow comparisons, points.
const OVERFLOW_EPS: f64 = 1e-6;

/// Where the letterforms of the new run come from, for the operator-facing
/// disclosure (decision 012 `GlyphSource`, refined by the shell).
///
/// The *written* PDF is identical for both — a non-embedded named Standard-14
/// dict (R79) — so this is a SHAPE-only, preview-fidelity distinction the
/// report discloses, never a difference in bytes on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontProvenance {
    /// A bundled pdfcer Standard-14 substitute face (the default).
    Bundled,
    /// An operator-supplied face registered via `--font-dir` (decision 012):
    /// the caller verified it covers the run and discloses its own shapes.
    Supplied,
}

impl FontProvenance {
    /// The lowercase word used in disclosures (`"bundled"` / `"supplied"`).
    const fn word(self) -> &'static str {
        match self {
            Self::Bundled => "bundled",
            Self::Supplied => "supplied",
        }
    }
}

/// The fill colour of the new run.
///
/// The default is [`Self::Black`], emitted as an explicit `0 g` (DeviceGray) —
/// because §8.4.2 forbids relying on the page-initial black default across the
/// `/Contents` array (the inherited fill may be anything). An RGB colour is
/// emitted as `r g b rg` (DeviceRGB), matching Pass 14.2's fill model.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub enum NewTextColor {
    /// DeviceGray black — `0 g`.
    #[default]
    Black,
    /// DeviceRGB — `r g b rg`, each component clamped to `[0, 1]`.
    Rgb(f64, f64, f64),
}

/// Which face a new run is written in.
///
/// # Why an enum and not two optional fields
///
/// The alternative — keep `base_font: Std14` and add `embed:
/// Option<FontEmbedPlan>` — is additive and would not have broken a single
/// call site. It also admits a state that means nothing: both set. Someone
/// then has to write down which wins, and every later reader has to find
/// that sentence. Decision 021 asked for the enum precisely so the
/// contradiction is unrepresentable, and it turned out to cost one struct
/// literal, because every other construction already goes through
/// [`AddTextRequest::with_font`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum NewTextFace {
    /// A bundled Standard-14 face, written by name with no embedded program
    /// (R79). The default, and still the right answer for Latin text: it
    /// adds no bytes to the file and needs no donor.
    Std14(Std14),
    /// A subsetted donor face, embedded as a new `/Type0` resource (FF-C,
    /// decision 021). Boxed because the plan carries the whole font program
    /// and `AddTextRequest` is passed by value in several places — an
    /// unboxed variant would make every request the size of a font.
    Embedded(Box<FontEmbedPlan>),
}

impl NewTextFace {
    /// The Standard-14 face, when this is one.
    #[must_use]
    pub fn std14(&self) -> Option<Std14> {
        match self {
            Self::Std14(f) => Some(*f),
            Self::Embedded(_) => None,
        }
    }
}

/// One add-new-text request: WHAT to add, WHERE, in WHICH face.
///
/// Construct with [`Self::new`] (Helvetica, bundled, 12 pt, black) and refine
/// with the `with_*` builders — `#[non_exhaustive]`, so a struct literal is not
/// usable out-of-crate and future fields never break callers.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AddTextRequest {
    /// 0-based page index.
    pub page_index: usize,
    /// The run's origin in default user space (§9.4.4), used as the absolute
    /// `Tm` translation (§9.4.2).
    pub origin: (f64, f64),
    /// The text to add (UTF-8; encoded to single-byte codes per the face).
    pub text: String,
    /// Which face to write (default [`Std14::Helvetica`], via
    /// [`NewTextFace::Std14`]).
    pub face: NewTextFace,
    /// The disclosed provenance of the rendering face (default
    /// [`FontProvenance::Bundled`]).
    pub provenance: FontProvenance,
    /// Font size in points (default `12.0`).
    pub size: f64,
    /// Fill colour (default [`NewTextColor::Black`]).
    pub color: NewTextColor,
    /// The wrap rectangle for the **boxed** variant (16.1), in default user
    /// space. `None` ⇒ point-text mode (16.0), where [`Self::origin`] alone
    /// places one single-line run. `Some(box)` ⇒ the text is wrapped to the
    /// box width and laid out top-anchored from the box top; [`Self::origin`]
    /// is then ignored (the box's top-left drives placement). Mutually
    /// exclusive with the point path — set via [`Self::with_box`].
    pub wrap_box: Option<Rect>,
    /// The alignment for the **boxed** variant (16.1). Default
    /// [`BlockAlignment::Left`]. Unlike the 15.0 reflow of *existing* text —
    /// which auto-detects alignment from glyph x-positions — a fresh box has
    /// no glyphs to detect, so alignment here is an EXPLICIT operator input
    /// (decision 016 §6/16.1). Ignored in point-text mode.
    pub alignment: BlockAlignment,
    /// An optional leading (baseline-to-baseline, points) for the boxed
    /// variant. `None` ⇒ the derived default `1.2·size` (disclosed). Ignored
    /// in point-text mode.
    pub leading: Option<f64>,
}

impl AddTextRequest {
    /// A request adding `text` at `origin` on `page_index`, defaulting to a
    /// bundled 12-pt black Helvetica run.
    #[must_use]
    pub fn new(page_index: usize, origin: (f64, f64), text: impl Into<String>) -> Self {
        Self {
            page_index,
            origin,
            text: text.into(),
            face: NewTextFace::Std14(Std14::Helvetica),
            provenance: FontProvenance::Bundled,
            size: 12.0,
            color: NewTextColor::Black,
            wrap_box: None,
            alignment: BlockAlignment::Left,
            leading: None,
        }
    }

    /// Switch to the **boxed** variant (16.1): wrap the text to the rectangle
    /// whose lower-left corner is `(x, y)`, width `w`, height `h` (default user
    /// space, §9.4.4). The lines are wrapped to `w` via the shipped 15.x greedy
    /// breaker and laid out top-anchored from `y + h`. Sets [`Self::origin`]
    /// to the box's top-left for the report/diagnostics; the actual per-line
    /// origins are computed by the alignment. A non-positive or non-finite `w`
    /// or `h` is later reported as [`AddTextError::InvalidBox`].
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::text_edit::{AddTextRequest, BlockAlignment};
    ///
    /// let req = AddTextRequest::new(0, (0.0, 0.0), "wrap me in a box")
    ///     .with_box(72.0, 600.0, 180.0, 120.0)
    ///     .with_alignment(BlockAlignment::Justified);
    /// assert!(req.wrap_box.is_some());
    /// assert_eq!(req.alignment, BlockAlignment::Justified);
    /// ```
    #[must_use]
    pub fn with_box(mut self, x: f64, y: f64, w: f64, h: f64) -> Self {
        self.wrap_box = Some(Rect::from_corners(x, y, x + w, y + h));
        // The origin field then reflects the box top-left (the anchor the
        // report echoes); per-line origins come from the alignment placement.
        self.origin = (x, y + h);
        self
    }

    /// Set the boxed-variant alignment (default [`BlockAlignment::Left`]).
    #[must_use]
    pub const fn with_alignment(mut self, alignment: BlockAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Set an explicit leading (baseline-to-baseline, points) for the boxed
    /// variant; `None` restores the derived `1.2·size` default.
    #[must_use]
    pub const fn with_leading(mut self, leading: Option<f64>) -> Self {
        self.leading = leading;
        self
    }

    /// Set the Standard-14 face.
    #[must_use]
    pub fn with_font(mut self, base_font: Std14) -> Self {
        self.face = NewTextFace::Std14(base_font);
        self
    }

    /// Write the run in a subsetted donor face, embedded as a new resource.
    ///
    /// The plan comes from `pdfcer_render::font::subset::plan_subset` — this
    /// crate has no font parser and deliberately never gains one (decision
    /// 021 §3.2, R21), so the caller does the parsing and hands over plain
    /// data.
    ///
    /// Embedding is never a default and never inferred: it is reached only by
    /// calling this, which is standing rule R108's "explicit, per-action
    /// operator choice" expressed in the type system rather than in a
    /// comment.
    #[must_use]
    pub fn with_embedded_face(mut self, plan: FontEmbedPlan) -> Self {
        self.face = NewTextFace::Embedded(Box::new(plan));
        self
    }

    /// Set the disclosed face provenance.
    #[must_use]
    pub fn with_provenance(mut self, provenance: FontProvenance) -> Self {
        self.provenance = provenance;
        self
    }

    /// Set the font size in points.
    #[must_use]
    pub fn with_size(mut self, size: f64) -> Self {
        self.size = size;
        self
    }

    /// Set the fill colour.
    #[must_use]
    pub fn with_color(mut self, color: NewTextColor) -> Self {
        self.color = color;
        self
    }
}

/// What the add did and what it disclosed (fuzzy-never-sneaky, rule 4).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AddTextReport {
    /// The `/BaseFont` written (one of the 14 exact §9.6.2.2 names).
    pub base_font: String,
    /// The disclosed provenance of the rendering face.
    pub provenance: FontProvenance,
    /// The collision-free `/Font` resource name assigned (e.g. `pdfceF1`).
    pub font_resource_name: String,
    /// The object number of the created content stream (0 until saved).
    pub content_object: u32,
    /// The object number of the created Standard-14 font dict (0 until saved).
    pub font_object: u32,
    /// Whether the page had to be given its OWN `/Resources` because it
    /// previously **inherited** them (the §7.7.3.4 inheritance trap) — the
    /// shared ancestor resources were NOT modified.
    pub gave_page_own_resources: bool,
    /// Whether the target page is tagged (`/StructTreeRoot` or `/MarkInfo
    /// /Marked true`) — if so the new run is untagged and that is disclosed
    /// (R73).
    pub tagged_untagged: bool,
    /// For the **boxed** variant (16.1): how many lines the text wrapped to.
    /// `None` for point-text mode (16.0 — always one implicit line).
    pub wrapped_lines: Option<usize>,
    /// For the boxed variant: how many wrapped lines fall (in whole or part)
    /// below the box bottom — the box-overflow count (R76). `0` when the text
    /// fits the box height or in point-text mode.
    pub box_overflow_lines: usize,
    /// For the boxed variant: how far the wrapped run grows past the page
    /// cropbox bottom, points (R76). `0.0` when it stays on the page or in
    /// point-text mode. The lines are STILL emitted (never clipped).
    pub page_overflow_pt: f64,
    /// The alignment placed, for the boxed variant. `None` in point-text mode.
    pub alignment: Option<BlockAlignment>,
    /// Every operator-facing disclosure, verbatim (surfaced by the UI/CLI).
    pub disclosures: Vec<String>,
}

/// The saved bytes plus the disclosure report.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AddTextOutcome {
    /// The incrementally-appended PDF bytes.
    pub bytes: Vec<u8>,
    /// The disclosure/diagnostic report.
    pub report: AddTextReport,
}

/// A failure to add text — every variant is a clean, named outcome, never a
/// crash (rule 4). A [`Self::Refused`] is the F-refuse font gate (R71) saying
/// no by name.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AddTextError {
    /// The chosen face cannot represent a character of the text (R71 /
    /// inverse-encoding gate), by name.
    #[error(transparent)]
    Refused(Refusal),
    /// A wrap box was given together with an embedded donor face.
    ///
    /// `layout_boxed` measures through the Standard-14 inverse-encoding
    /// table; giving it a second width source is its own slice (decision 021
    /// §3.4). Refused BY NAME rather than quietly falling back to point
    /// text, because silently ignoring a box the operator drew produces a
    /// result that looks deliberate and is not.
    #[error(
        "pdfcer can't wrap text to a box in an embedded font yet - that combination isn't supported in this first cut. Add the text as a single line, or choose a built-in font for the box."
    )]
    EmbeddedBoxedUnsupported,
    /// The embedded font could not be emitted as PDF objects.
    #[error(transparent)]
    Embed(#[from] crate::font_embed::FontEmbedError),
    /// The embedded plan does not cover a character of the text.
    ///
    /// Should be unreachable when the caller built the plan from the same
    /// string, and is checked anyway: the plan arrives from another crate,
    /// and "the caller passed matching inputs" is a claim, not evidence
    /// (R93). Emitting a CID the font lacks would draw `.notdef` boxes in a
    /// document that reported success.
    #[error(
        "internal: the embedded font plan has no glyph for {ch:?}, so the text could not be \
         written. This is a pdfcer bug — the plan and the text disagree."
    )]
    EmbeddedPlanIncomplete { ch: char },
    /// No page at the requested index.
    #[error("no page at index {0}")]
    PageIndex(usize),
    /// The text is empty — there is nothing to add.
    #[error("cannot add empty text — nothing to insert")]
    EmptyText,
    /// The font size is not a positive, finite number of points.
    #[error("font size {0} is not a positive, finite number of points")]
    InvalidSize(f64),
    /// The boxed variant's rectangle has a non-positive or non-finite width or
    /// height — nothing can be wrapped into it.
    #[error("the wrap box {0}x{1} is not a positive, finite rectangle")]
    InvalidBox(f64, f64),
    /// The boxed variant's text tokenised to zero words (it is entirely
    /// whitespace) — there is nothing to wrap into the box.
    #[error("the boxed text has no non-whitespace words to wrap")]
    NoWordsToWrap,
    /// The document is encrypted (out of scope for add-text).
    #[error("the document is encrypted; adding text to encrypted files is out of scope")]
    Encrypted,
    /// A **certification signature** with an enforced permissions entry
    /// forbids adding page content (§12.8.4 Table 258) — the add-text mirror
    /// of [`crate::edit::EditError::CertificationForbidsChange`], which
    /// [`crate::edit::EditSession::add_markup`] raises for the same reason.
    ///
    /// Appending a `BT…ET` run creates a content stream + font and rewrites
    /// the page dict's `/Contents`/`/Resources` — a structural page change.
    /// With the catalog's `/Perms → /DocMDP` present, Table 258 makes
    /// enforcement a `shall`, so pdfcer refuses rather than silently breaking
    /// the certification. The message is a **verbatim mirror** of
    /// `EditError::CertificationForbidsChange`'s (same wording, same §12.8.4
    /// citation, same `/P` field) — reused, not reinvented; a parity test
    /// asserts the two `Display` strings are byte-identical.
    #[error(
        "this document carries a certification signature whose permissions are enforced (ISO \
         32000-1 §12.8.4, /Perms /DocMDP, P={permission}); structural page changes are not \
         among the changes it permits, so pdfcer refuses rather than silently breaking it"
    )]
    CertificationForbidsChange {
        /// The certification's `/P` access permission (Table 254: 1–3).
        /// **2 when the transform parameters omit `/P`**, that table's
        /// documented default.
        permission: u8,
    },
    /// Creating an object would raise `/Size` and expose cross-reference
    /// entries a filtering `/Size` currently hides (§7.5.5) — refused, like
    /// [`crate::edit::EditSession::add_markup`].
    #[error(
        "creating an object would raise /Size and expose {count} hidden cross-reference \
         entr{} this file's /Size currently suppresses",
        if *count == 1 { "y" } else { "ies" }
    )]
    HiddenObjects {
        /// How many entries would be exposed.
        count: usize,
    },
    /// No object number is left to allocate.
    #[error("the document has no unused object number left")]
    ObjectNumbersExhausted,
    /// The page/target is structurally unusable for an add (e.g. the page
    /// object is not a dictionary).
    #[error("this run cannot be added: {0}")]
    Unsupported(String),
    /// The page tree could not be walked.
    #[error("page tree error: {0}")]
    PageTree(#[from] PageTreeError),
    /// The incremental save failed.
    #[error("save failed: {0}")]
    Write(#[from] WriteError),
}

/// Add a new single-line text run to `doc` and return the incrementally-saved
/// bytes plus the disclosure report — the free-function engine used by the CLI
/// and the round-trip tests.
///
/// This performs its own [`save_incremental`] with a hand-built [`DirtySet`]:
/// the two created objects (content stream + font dict) and the modified page
/// dict, with the new content bytes in the dirty set's staging buffer (the
/// `base.len() + local` combined coordinate system, R45). The original content
/// stream object is not in the set ⇒ byte-identical (R32/R46). The
/// session-integrated sibling is
/// [`EditSession::add_text`](crate::edit::EditSession::add_text), which shares
/// the [`plan_add_text`] planner so the two never drift.
///
/// # Errors
///
/// [`AddTextError`] — a named font refusal (R71), an out-of-range page, empty
/// text, an invalid size, encryption, an object-creation/`/Size` conflict, no
/// free object number, or a save failure. A refusal happens BEFORE any save.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::document::Document;
/// use pdfcer_core::text_edit::{add_text, AddTextRequest};
///
/// let doc = Document::load(std::path::Path::new("in.pdf"))?;
/// let req = AddTextRequest::new(0, (72.0, 700.0), "Hello, world").with_size(14.0);
/// let out = add_text(&doc, &req)?;
/// std::fs::write("out.pdf", &out.bytes)?;
/// for d in &out.report.disclosures {
///     eprintln!("{d}");
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn add_text(doc: &Document, req: &AddTextRequest) -> Result<AddTextOutcome, AddTextError> {
    // Guards mirror `EditSession::add_markup`, in the SAME order (encryption →
    // certification → suppressed-objects): each is a named refusal made BEFORE
    // any allocation.
    if doc.trailer().contains_key(b"Encrypt") {
        return Err(AddTextError::Encrypted);
    }
    // An enforced-DocMDP certification forbids adding page content (§12.8.4).
    refuse_if_certification_forbids(doc)?;
    let suppressed = doc.suppressed_object_count();
    if suppressed > 0 {
        return Err(AddTextError::HiddenObjects { count: suppressed });
    }

    let pages = page_tree::pages(doc)?;
    let page = pages
        .get(req.page_index)
        .ok_or(AddTextError::PageIndex(req.page_index))?;

    let prep = plan_add_text(req, page, doc)?;

    // Two fresh object numbers: content stream then font dict. Consecutive so
    // the incremental update section stays compact and deterministic.
    let content_num = doc
        .next_object_number()
        .ok_or(AddTextError::ObjectNumbersExhausted)?;
    let font_num = content_num
        .checked_add(1)
        .ok_or(AddTextError::ObjectNumbersExhausted)?;
    let content_id = ObjId::new(content_num, 0);
    let font_id = ObjId::new(font_num, 0);

    let new_page = prep.build_page_dict(doc, content_id, font_id);

    // Stage the content bytes into the dirty set's own buffer and point the
    // new stream object's span at `base.len() + local` (R45 combined source).
    let mut dirty = DirtySet::empty();
    let base_len = doc.bytes().len();
    let mut staging: Vec<u8> = Vec::new();
    let start = base_len + staging.len();
    staging.extend_from_slice(&prep.content_data);
    let span = ByteSpan::new(start, prep.content_data.len());

    dirty.replace(content_id, make_raw_stream(span, prep.content_data.len()));

    match prep.embed.as_ref() {
        // FF-C: five NEW objects instead of one font dict. The font program
        // is staged after the content stream in the same buffer, so both
        // spans are relative to the same base (R45 combined source).
        Some(plan) => {
            let prog_start = base_len + staging.len();
            staging.extend_from_slice(&plan.program);
            let prog_span = ByteSpan::new(prog_start, plan.program.len());
            let program_stream = make_font_program_stream(prog_span, plan.program.len());

            let objects = crate::font_embed::build_objects(plan, font_num, program_stream)
                .map_err(AddTextError::Embed)?;
            // The page's /Font entry must point at the /Type0 wrapper, not at
            // any of its parts. `build_objects` returns that id explicitly
            // rather than leaving the caller to assume it is the first —
            // an assumption that would still "work" until the allocation
            // order changed.
            debug_assert_eq!(objects.font_dict_id, font_id);
            for (id, obj) in objects.objects {
                dirty.replace(id, obj);
            }
        }
        None => {
            dirty.replace(font_id, prep.font_dict.clone());
        }
    }

    dirty.replace(prep.page_id, Object::Dict(new_page));
    dirty.set_staging(staging);

    let (bytes, _report) = save_incremental(doc, &dirty, &SaveOptions::identity())?;

    let mut report = prep.report;
    report.content_object = content_num;
    report.font_object = font_num;
    Ok(AddTextOutcome { bytes, report })
}

/// Refuse an add-text against a certified PDF whose enforced DocMDP forbids
/// structural page changes — the [`AddTextError`] sibling of
/// [`EditSession::check_certification`](crate::edit::EditSession) that
/// [`EditSession::add_markup`](crate::edit::EditSession::add_markup) calls
/// before it authors page content (§12.8.4 Table 258).
///
/// Adding a `BT…ET` run creates a new content stream + font and rewrites the
/// page dict's `/Contents`/`/Resources`. When the catalog carries `/Perms →
/// /DocMDP`, Table 258 says *"consumer applications shall enforce the
/// permissions"*, and Table 254's permitted-change lists contain no operation
/// pdfcer can perform — so pdfcer refuses rather than performing the edit and
/// silently invalidating the certification.
///
/// This mirrors `add_markup`'s guard EXACTLY — the same
/// [`census`](crate::signature::census) +
/// [`SignatureCensus::forbids_structural_change`](crate::signature::SignatureCensus::forbids_structural_change)
/// machinery and the same "`/P` absent ⇒ default 2" rule (Table 254) — but
/// yields an [`AddTextError`] rather than an
/// [`EditError`](crate::edit::EditError) because that is add-text's error
/// type. It is deliberately conservative and may over-refuse (every enforced
/// certification is treated as forbidding, exactly as `add_markup` documents):
/// over-refusal is fail-clean-safe. Shared by BOTH operator-reachable entry
/// points — [`EditSession::add_text`](crate::edit::EditSession::add_text) (the
/// GUI's undo-able command) and this free [`add_text`] engine (the CLI /
/// batch path) — so no operator-facing entry can add page content to an
/// enforced-certified document unguarded, and the two can never drift.
pub(crate) fn refuse_if_certification_forbids<G: ObjectGraph + ?Sized>(
    graph: &G,
) -> Result<(), AddTextError> {
    let census = crate::signature::census(graph);
    if census.forbids_structural_change() {
        return Err(AddTextError::CertificationForbidsChange {
            permission: census.certification_permission.unwrap_or(2),
        });
    }
    Ok(())
}

/// A planned add-text operation, ready to be materialized against two
/// allocated object numbers — the shared substrate of the free [`add_text`]
/// and the session-integrated
/// [`EditSession::add_text`](crate::edit::EditSession::add_text).
///
/// Everything that does NOT depend on the two object numbers is precomputed
/// here: the encoded content bytes, the Standard-14 font dict, the merged
/// resource pieces, the collision-free font name, and the report skeleton. Only
/// the page dict's two references (the appended `/Contents` element and the
/// `/Font` entry) are filled in by [`Self::build_page_dict`], so the free
/// function and the session build a byte-identical page dict from one recipe.
pub(crate) struct AddTextPrep {
    /// The page object being modified.
    pub(crate) page_id: ObjId,
    /// The page dict as it currently stands (base, or session overlay).
    page_dict: Dict,
    /// The page dict's current `/Contents` value (single→array append input).
    contents_before: Option<Object>,
    /// The page's effective `/Resources` minus `/Font` (references preserved).
    resources_base: Dict,
    /// The existing `/Font` subdict entries to merge the new font into.
    font_subdict_base: Dict,
    /// The collision-free `/Font` name (e.g. `b"pdfceF1"`).
    font_name: Vec<u8>,
    /// The `q BT…ET Q` content-stream bytes (leading `\n` included).
    pub(crate) content_data: Vec<u8>,
    /// The Standard-14 font dictionary object.
    ///
    /// Meaningless when [`Self::embed`] is `Some` — the embedded path builds
    /// five objects through `font_embed::build_objects` instead. It is left
    /// populated rather than made `Option` because every Std-14 caller would
    /// then have to unwrap something that is always present for them.
    pub(crate) font_dict: Object,
    /// The donor plan, when this run is written in an embedded face (FF-C).
    ///
    /// Its presence is what switches the save path from "one font dict" to
    /// "five new objects", which is why it lives on the prep rather than
    /// being re-derived from the request: the prep is the single recipe both
    /// the free function and the session build from, and a second read of
    /// `req.face` in the saver is a second place for the two to disagree.
    pub(crate) embed: Option<Box<FontEmbedPlan>>,
    /// The disclosure report (object numbers 0 until saved).
    pub(crate) report: AddTextReport,
}

impl AddTextPrep {
    /// Build the modified page dict from the two allocated object numbers.
    ///
    /// Sets `/Contents` via [`crate::page_tree::append_content_stream`] and
    /// `/Resources` to an inline dict that references the same sub-dictionaries
    /// as the effective resources EXCEPT for a fresh merged `/Font` subdict
    /// carrying the new font — the inheritance-safe recipe (§7.7.3.4). The
    /// original page dict's other keys are preserved (e.g. `/Annots` a prior
    /// session op added).
    pub(crate) fn build_page_dict<G: ObjectGraph + ?Sized>(
        &self,
        graph: &G,
        content_id: ObjId,
        font_id: ObjId,
    ) -> Dict {
        let mut new_page = self.page_dict.clone();

        // ★ `page_tree::append_content_stream`, not a local helper. This used
        // to call `append_contents`, a SECOND implementation of the same
        // append that lived in this file -- and it was wrong the same way the
        // first one was: it matched on the RAW `/Contents` value and wrapped a
        // reference without resolving it, so a reference to an ARRAY (Qt, and
        // every CAD sheet) produced an array nested inside an array. That is
        // R92 exactly, and the graph is threaded in here rather than the logic
        // being re-derived because ONE answer is the fix, not two correct ones.
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

/// Plan an add-text operation against any [`ObjectGraph`] view of the document
/// (the base [`Document`] for the free function; the session overlay for
/// [`EditSession::add_text`](crate::edit::EditSession::add_text)).
///
/// Encodes the text against the chosen face (refusing by name on any glyph the
/// face lacks — R71), builds the content bytes and the Standard-14 font dict,
/// resolves the page's effective resources, picks a collision-free font name,
/// and assembles the report skeleton. Reads `page.resources` (already resolved
/// own-or-inherited by the page-tree walk) and `graph.resolved(page.id)` (the
/// current page dict), so a session that already added `/Annots` to the page
/// keeps them.
///
/// # Errors
///
/// [`AddTextError`] — empty text, an invalid size, a named font refusal, or a
/// non-dictionary page object.
pub(crate) fn plan_add_text<G: ObjectGraph + ?Sized>(
    req: &AddTextRequest,
    page: &Page,
    graph: &G,
) -> Result<AddTextPrep, AddTextError> {
    if req.text.is_empty() {
        return Err(AddTextError::EmptyText);
    }
    if !req.size.is_finite() || req.size <= 0.0 {
        return Err(AddTextError::InvalidSize(req.size));
    }

    let page_dict = graph.resolved(page.id).as_dict().cloned().ok_or_else(|| {
        AddTextError::Unsupported("the page object is not a dictionary".to_owned())
    })?;
    // Own `/Resources` vs inherited: the §7.7.3.4 trap detector.
    let has_own_resources = page_dict.get(b"Resources").is_some();
    let contents_before = page_dict.get(b"Contents").cloned();

    // Effective `/Font` subdict (resolve an indirect subdict), and the base
    // resources with `/Font` stripped (re-added merged in `build_page_dict`).
    let font_subdict_base: Dict = match page.resources.get(b"Font") {
        Some(o) => graph.resolve(o).as_dict().cloned().unwrap_or_default(),
        None => Dict::new(),
    };
    let font_name = pick_font_name(&font_subdict_base);
    let mut resources_base = page.resources.clone();
    resources_base.remove(b"Font");

    // Face selection and the encode table — the SHARED setup (below) both this
    // planner and the pure `preview_wrap` use, so a boxed preview encodes and
    // measures a literal string exactly as the committed add will.
    let font = req
        .face
        .std14()
        .unwrap_or(crate::fontdata::Std14::Helvetica);
    let base_font_name = fontdata::std14_base_font_name(font);
    let (inv, enc, symbolic) = face_encoding(font);

    // Placement mode branch (decision 016 §3.2): point text (16.0) shows the
    // whole run as one line at `origin`; boxed text (16.1) wraps to the box
    // width via the shipped 15.x greedy breaker and emits N justified/aligned
    // lines. Both APPEND identically (below) — the only difference is the
    // `q BT…ET Q` body these two builders produce.
    //
    // FF-C (decision 021) adds a THIRD shape to this branch: an embedded
    // donor face is composite, so it shows 2-byte CIDs from a hex string
    // rather than single-byte codes from a literal one, and it brings five
    // new objects instead of one font dict. Boxed layout is not available
    // for it at the 21.0 floor — `layout_boxed` measures through the
    // Standard-14 inverse-encoding table, and giving it a second width
    // source is its own slice. That limit is a NAMED refusal rather than a
    // silent fallback to point text, because silently ignoring a wrap box
    // the operator drew is the rule-4 failure this project keeps refusing
    // to commit.
    let embed_plan: Option<&FontEmbedPlan> = match &req.face {
        NewTextFace::Embedded(p) => Some(p.as_ref()),
        NewTextFace::Std14(_) => None,
    };
    if embed_plan.is_some() && req.wrap_box.is_some() {
        return Err(AddTextError::EmbeddedBoxedUnsupported);
    }

    let (content_data, extra) = match req.wrap_box {
        None if embed_plan.is_some() => {
            // `expect`-free: the guard above proves it.
            let Some(plan) = embed_plan else {
                return Err(AddTextError::EmbeddedBoxedUnsupported);
            };
            let cids = cids_for(plan, &req.text)?;
            let bytes = build_content_embedded(&font_name, req.size, req.color, req.origin, &cids);
            // No extra disclosure here: the face disclosure below already
            // says this run is an embedded subset, with the same numbers.
            // Saying it twice made the CLI print two paragraphs that
            // differed only in wording, which reads as two separate facts.
            (bytes, ExtraReport::point(Vec::new()))
        }
        None => {
            let encoded = inv
                .encode_str(&req.text, &BTreeSet::new())
                .map_err(AddTextError::Refused)?;
            let bytes = build_content(&font_name, req.size, req.color, req.origin, &encoded.codes);
            (bytes, ExtraReport::point(encoded.disclosures))
        }
        Some(bx) => {
            let layout = layout_boxed(
                &req.text,
                req.size,
                req.alignment,
                req.leading,
                &inv,
                font,
                enc,
                bx,
                page.crop_box,
            )?;
            let bytes = build_content_boxed(&font_name, req.size, req.color, &layout);
            let extra = ExtraReport::boxed(&layout);
            (bytes, extra)
        }
    };
    let font_dict = build_font_dict(base_font_name, symbolic, enc, font);
    let tagged = is_tagged(graph);

    let mut disclosures = Vec::new();
    // The face disclosure must describe what this run ACTUALLY did.
    // Until FF-C there was only one answer, so it was written
    // unconditionally — and the moment `--embed-font` shipped, that line
    // went on asserting "no glyph embedding (R79)" about runs that had
    // just embedded a font. Caught by reading the CLI's own output on the
    // first real embed, not by any test: no test asserted a disclosure's
    // TEXT against the branch that produced it. R93 — a confident
    // statement is worse than none, because it stops the reader looking.
    match embed_plan {
        Some(plan) => disclosures.push(format!(
            "new run uses an EMBEDDED SUBSET of '{}' — the document now carries its own \
             glyph program for this text ({} glyph(s), {} bytes), so it renders the same \
             everywhere instead of depending on the reader's fonts (decision 021 / FF-C). \
             Nothing already in the file was rewritten (R107)",
            plan.base_name,
            plan.glyphs.len(),
            plan.program.len()
        )),
        None => disclosures.push(format!(
            "new run uses a {} Standard-14 face '{}' by name+code — no glyph embedding \
             (R79 / ISO 32000-1 §9.6.2.2); provenance is disclosed, not the document's own",
            req.provenance.word(),
            base_font_name
        )),
    }
    if !has_own_resources {
        disclosures.push(
            "this page INHERITED its /Resources from an ancestor /Pages node; pdfcer gave the \
             page its OWN /Resources (referencing the same shared sub-dictionaries) and added \
             the font there — the shared ancestor resources were NOT modified (§7.7.3.4)"
                .to_owned(),
        );
    }
    if tagged {
        disclosures.push(
            "new run added as untagged page content; the structure tree / reading order was \
             not updated — no tag created (R73)"
                .to_owned(),
        );
    }
    disclosures.extend(extra.disclosures.iter().cloned());

    let report = AddTextReport {
        // The tagged subset name for an embedded run. Reporting the
        // Std-14 default here would name a face the output does not
        // contain.
        base_font: match embed_plan {
            Some(plan) => plan.tagged_name(),
            None => base_font_name.to_owned(),
        },
        provenance: req.provenance,
        font_resource_name: String::from_utf8_lossy(&font_name).into_owned(),
        content_object: 0,
        font_object: 0,
        gave_page_own_resources: !has_own_resources,
        tagged_untagged: tagged,
        wrapped_lines: extra.wrapped_lines,
        box_overflow_lines: extra.box_overflow_lines,
        page_overflow_pt: extra.page_overflow_pt,
        alignment: extra.alignment,
        disclosures,
    };

    Ok(AddTextPrep {
        page_id: page.id,
        page_dict,
        contents_before,
        resources_base,
        font_subdict_base,
        font_name,
        content_data,
        font_dict,
        embed: match &req.face {
            NewTextFace::Embedded(p) => Some(p.clone()),
            NewTextFace::Std14(_) => None,
        },
        report,
    })
}

/// A `FontFile2` stream object: the subsetted program plus the `/Length1`
/// ISO 32000-1 §9.9 Table 127 requires for a TrueType program.
///
/// `/Length1` is the length of the UNCOMPRESSED program. pdfcer stages the
/// program uncompressed, so it equals `/Length` here — written explicitly
/// anyway, because a consumer is entitled to read `/Length1` and a future
/// compression pass that set only `/Length` would silently produce a font
/// whose declared uncompressed size was its compressed one.
fn make_font_program_stream(span: ByteSpan, len: usize) -> Object {
    let mut dict = Dict::new();
    let n = i64::try_from(len).unwrap_or(i64::MAX);
    dict.insert(Name::from(b"Length"), Object::Integer(n));
    dict.insert(Name::from(b"Length1"), Object::Integer(n));
    Object::Stream(crate::object::Stream {
        dict,
        data_span: span,
    })
}

// The `/Contents` append that used to live here (`append_contents`) is GONE —
// it was a second implementation of `page_tree::append_content_stream`, and it
// was wrong the same way that one was: it matched on the RAW `/Contents` value
// and wrapped a reference without resolving it, so a reference to an ARRAY
// produced an array nested inside an array (`Pass 111.0`). One answer, in
// `page_tree`, beside the reader that decides the same question.

/// Build the `q BT…ET Q` content bytes (§9.4.2/§9.4.3), leading `\n` included.
///
/// The exact origination sequence from the spec grounding §5 — self-`q…Q`-
/// balanced (§8.4.2), with `Tf`/fill-colour/`Tm` all set explicitly because
/// `q`/`Q` do not reset to Table-52 initials.
fn build_content(
    font_name: &[u8],
    size: f64,
    color: NewTextColor,
    origin: (f64, f64),
    codes: &[u8],
) -> Vec<u8> {
    let mut out = Vec::new();
    // Leading token separator (§7.2): no token may span the array-element
    // boundary between the prior stream and this one.
    out.push(b'\n');
    out.extend_from_slice(b"q\n");
    out.extend_from_slice(b"BT\n");
    out.push(b'/');
    out.extend_from_slice(font_name);
    out.push(b' ');
    emit_number(&mut out, size);
    out.extend_from_slice(b" Tf\n");
    match color {
        NewTextColor::Black => out.extend_from_slice(b"0 g\n"),
        NewTextColor::Rgb(r, g, b) => {
            emit_number(&mut out, r.clamp(0.0, 1.0));
            out.push(b' ');
            emit_number(&mut out, g.clamp(0.0, 1.0));
            out.push(b' ');
            emit_number(&mut out, b.clamp(0.0, 1.0));
            out.extend_from_slice(b" rg\n");
        }
    }
    // Absolute placement: `1 0 0 1 x y Tm` REPLACES Tm/Tlm (§9.4.2).
    out.extend_from_slice(b"1 0 0 1 ");
    emit_number(&mut out, origin.0);
    out.push(b' ');
    emit_number(&mut out, origin.1);
    out.extend_from_slice(b" Tm\n");
    // Show: the operand is a sequence of single-byte codes (§9.4.3), escaped
    // into a literal `( … )` string by the shared writer helper.
    emit_literal_string(&mut out, codes);
    out.extend_from_slice(b" Tj\n");
    out.extend_from_slice(b"ET\n");
    out.push(b'Q');
    out
}

/// Build the `q BT…ET Q` body for an EMBEDDED (composite) face.
///
/// The Standard-14 sibling [`build_content`] shows a literal `( … )` string of
/// single-byte codes. A `/Type0` font with `Identity-H` addresses glyphs by
/// TWO-byte CID (§9.7.6.2), so the operand is a hex string instead — writing
/// the same literal form here would silently address the wrong glyphs, and
/// would do so *plausibly*, because half the bytes would still land on real
/// CIDs.
/// Map each character of `text` to its CID in `plan`.
///
/// The plan is normally built from this exact string, so a miss "cannot
/// happen" — which is precisely why it is checked. The plan crosses a crate
/// boundary as plain data, and a caller that reused a plan for a different
/// string would otherwise emit CIDs the embedded subset has no glyphs for:
/// a page of `.notdef` boxes, reported as a successful add.
///
/// Linear scan rather than a map: a subset is small by construction (it is
/// the glyphs for one run), and building a `HashMap` per call to avoid a
/// scan of a handful of entries would cost more than it saves.
fn cids_for(plan: &FontEmbedPlan, text: &str) -> Result<Vec<u16>, AddTextError> {
    text.chars()
        .map(|ch| {
            plan.glyphs
                .iter()
                .find(|g| g.unicode == ch)
                .map(|g| g.cid)
                .ok_or(AddTextError::EmbeddedPlanIncomplete { ch })
        })
        .collect()
}

fn build_content_embedded(
    font_name: &[u8],
    size: f64,
    color: NewTextColor,
    origin: (f64, f64),
    cids: &[u16],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(b'\n');
    out.extend_from_slice(b"q\n");
    out.extend_from_slice(b"BT\n");
    out.push(b'/');
    out.extend_from_slice(font_name);
    out.push(b' ');
    emit_number(&mut out, size);
    out.extend_from_slice(b" Tf\n");
    match color {
        NewTextColor::Black => out.extend_from_slice(b"0 g\n"),
        NewTextColor::Rgb(r, g, b) => {
            emit_number(&mut out, r.clamp(0.0, 1.0));
            out.push(b' ');
            emit_number(&mut out, g.clamp(0.0, 1.0));
            out.push(b' ');
            emit_number(&mut out, b.clamp(0.0, 1.0));
            out.extend_from_slice(b" rg\n");
        }
    }
    out.extend_from_slice(b"1 0 0 1 ");
    emit_number(&mut out, origin.0);
    out.push(b' ');
    emit_number(&mut out, origin.1);
    out.extend_from_slice(b" Tm\n");
    // Hex string (§7.3.4.3), two bytes per CID, big-endian. Hex rather than
    // a literal string because a CID may contain any byte value, including
    // the ones a literal string would have to escape — and an escape the
    // writer and a reader disagree about is a glyph-level corruption that
    // renders as plausible WRONG TEXT rather than as an error.
    out.push(b'<');
    for cid in cids {
        out.extend_from_slice(format!("{cid:04X}").as_bytes());
    }
    out.extend_from_slice(b"> Tj\n");
    out.extend_from_slice(b"ET\n");
    out.push(b'Q');
    out
}

/// Build the inverse-encoding table + base encoding for a Standard-14 face —
/// the SHARED face setup used by BOTH the mutating planner ([`plan_add_text`])
/// and the pure read-only preview ([`preview_wrap`]).
///
/// Latin faces write `/WinAnsiEncoding` and encode via WinAnsi; the two
/// symbolic fonts use their built-in encoding (Annex D.5/D.6) and omit
/// `/Encoding`. Returning the inverse encoder, the base encoding, and whether
/// the face is symbolic here (rather than open-coding it in each caller) is
/// what guarantees a box-mode live preview wraps a literal string against the
/// EXACT same repertoire and code table the committed add will use — no
/// GUI-side approximation, the duplication-drift risk decision 016 §0.3 and
/// Pass 16.2 spec §4.2 both call out.
pub(crate) fn face_encoding(font: Std14) -> (InverseEncoding, BaseEncoding, bool) {
    let base_font_name = fontdata::std14_base_font_name(font);
    let symbolic = matches!(font, Std14::Symbol | Std14::ZapfDingbats);
    let enc = if symbolic {
        fontdata::std14_builtin_encoding(font)
    } else {
        BaseEncoding::WinAnsi
    };
    let table: [Option<String>; 256] = std::array::from_fn(|code| {
        u8::try_from(code)
            .ok()
            .and_then(|c| fontdata::encoding_glyph_name(enc, c))
            .map(str::to_owned)
    });
    let inv = InverseEncoding::build(base_font_name, &table);
    (inv, enc, symbolic)
}

// ===================================================================
// Boxed (16.1) multi-line layout + emission.
// ===================================================================
//
// The boxed variant reuses three shipped pieces wholesale (decision 016
// §6/16.1 — "no new wrapping logic"):
//
// 1. [`crate::linebreak::greedy_pack`] — the ONE greedy first-fit breaker the
//    15.x reflow engine and `vartext` already share.
// 2. [`line_natural_width`](crate::text_edit::reflow::line_natural_width) — the
//    same Σ-word-advance + representative-space natural-width measure the
//    reflow engine feeds that breaker.
// 3. [`align_origin_x`](crate::text_edit::reflow::align_origin_x) — the same
//    per-line origin formula (Left/Right/Center/Justified) the 15.1 surgery
//    uses (`iso32000__ref__reflow_emission.md` §3 recipe B; Justified starts
//    flush-left and gets its right flush from the §1 `TJ` slack).
//
// The ONE thing that differs from a *reflow* is the measurement input: a
// reflow measures a recognised block's OWN glyphs by their §9.4.4 provenance
// advances; a fresh boxed add has no glyphs yet, so it measures the operator's
// UTF-8 by the chosen std-14 face's AFM `/Widths` — exactly as `vartext` does
// (`fontdata::std14_width`, GUI-free). Alignment is an EXPLICIT input (default
// Left), NOT the 15.0 x-position auto-detect (a fresh box has nothing to
// detect from).

/// One laid-out line of a boxed add: the per-word source codes on the line,
/// its placement, and (for a justified full line) the slack to distribute.
///
/// A `blank` line is an empty paragraph produced by a hard `\n\n` in the
/// operator text: it consumes a baseline (so following lines drop correctly)
/// but emits no show operator.
struct LaidLine {
    /// The source codes of each word on this line (one `Vec<u8>` per word) —
    /// consumed by the emission path ([`build_content_boxed`]).
    words: Vec<Vec<u8>>,
    /// The ORIGINAL words of this line joined by a single space, for the
    /// read-only wrap preview ([`preview_wrap`]) to render as ghost text with
    /// an egui-approximate font. Empty for a blank line. Carried alongside
    /// `words` so the preview and the commit share ONE layout pass (never a
    /// second GUI-side wrap) — spec §4.2 / decision 016 §0.3.
    text: String,
    /// Natural width (Σ word advances + representative spaces), points.
    natural_width: f64,
    /// Origin x (left edge of the shown text) per the alignment, points.
    origin_x: f64,
    /// Baseline y in default user space (top-anchored: `first_baseline −
    /// i·leading` for the line's global index `i`).
    baseline_y: f64,
    /// Inter-word gaps on this line (`word count − 1`).
    gap_count: usize,
    /// For a justified, non-last, multi-word line: the slack (`wrap_width −
    /// natural_width`) to distribute across [`Self::gap_count`] gaps as
    /// negative `TJ` numbers. `None` otherwise (last line / non-justified /
    /// single word).
    justified_slack: Option<f64>,
    /// This line is the LAST of its paragraph (never justified — §4.1).
    is_last_of_para: bool,
    /// This line is a single word wider than the box (unbreakable overflow).
    is_overflowing_word: bool,
    /// A blank line (empty paragraph): advances the baseline, shows nothing.
    blank: bool,
}

impl LaidLine {
    /// A blank line for an empty paragraph.
    const fn blank() -> Self {
        Self {
            words: Vec::new(),
            text: String::new(),
            natural_width: 0.0,
            origin_x: 0.0,
            baseline_y: 0.0,
            gap_count: 0,
            justified_slack: None,
            is_last_of_para: true,
            is_overflowing_word: false,
            blank: true,
        }
    }
}

/// The computed boxed layout: the lines to emit, the space code to join words
/// with, the disclosures, and the overflow measures (R76).
struct BoxedLayout {
    /// The lines, top-to-bottom (blanks included so baselines stay correct).
    lines: Vec<LaidLine>,
    /// The face's inter-word space code (kept inside strings between words).
    space_code: u8,
    /// Named operator-facing disclosures (deduplicated).
    disclosures: Vec<String>,
    /// How many non-blank lines the text wrapped to.
    wrapped_lines: usize,
    /// How many lines fall (in whole or part) below the box bottom (R76).
    box_overflow_lines: usize,
    /// How far the run grows past the page cropbox bottom, points (R76).
    page_overflow_pt: f64,
    /// The alignment placed.
    alignment: BlockAlignment,
}

/// The subset of report fields the placement branch contributes, so
/// [`plan_add_text`] builds one [`AddTextReport`] from either mode.
struct ExtraReport {
    disclosures: Vec<String>,
    wrapped_lines: Option<usize>,
    box_overflow_lines: usize,
    page_overflow_pt: f64,
    alignment: Option<BlockAlignment>,
}

impl ExtraReport {
    /// Point-text mode (16.0): only the encode disclosures, no wrap/overflow.
    fn point(disclosures: Vec<String>) -> Self {
        Self {
            disclosures,
            wrapped_lines: None,
            box_overflow_lines: 0,
            page_overflow_pt: 0.0,
            alignment: None,
        }
    }

    /// Boxed mode (16.1): the layout's disclosures + wrap/overflow measures.
    fn boxed(layout: &BoxedLayout) -> Self {
        Self {
            disclosures: layout.disclosures.clone(),
            wrapped_lines: Some(layout.wrapped_lines),
            box_overflow_lines: layout.box_overflow_lines,
            page_overflow_pt: layout.page_overflow_pt,
            alignment: Some(layout.alignment),
        }
    }
}

/// Advance width of one code in `font` under `enc`, in text-space points at
/// `size` (§9.4.4: `width/1000 × size`, `Tc`/`Tw`/`Tz` at their defaults). A
/// code the face has no glyph for contributes 0.
fn code_advance(font: Std14, enc: BaseEncoding, code: u8, size: f64) -> f64 {
    let units = fontdata::encoding_glyph_name(enc, code)
        .and_then(|name| fontdata::std14_width(font, name))
        .unwrap_or(0);
    f64::from(units) / 1000.0 * size
}

/// Advance width of a run of codes (§9.4.4), points — the boxed measurer the
/// greedy breaker packs by (peer of `vartext::measure`, generalised to any
/// std-14 encoding so the symbolic faces also measure).
fn measure_codes(font: Std14, enc: BaseEncoding, codes: &[u8], size: f64) -> f64 {
    codes
        .iter()
        .map(|&c| code_advance(font, enc, c, size))
        .sum()
}

/// Wrap `req.text` to the box width and lay it out top-anchored (decision 016
/// §6/16.1). See the module header and the section banner above for the reuse
/// and the overflow (R76) contract.
///
/// Paragraphs split on a hard `\n`; each is wrapped independently by the
/// shared greedy breaker so its own last line is un-justified (§4.1). Words
/// split on ASCII whitespace and are encoded through the SAME inverse-encoding
/// F-refuse gate (R71) as the point path — a glyph the face lacks is refused
/// by name, before any content is built.
///
/// # Errors
///
/// - [`AddTextError::InvalidBox`] — a non-positive/non-finite width or height.
/// - [`AddTextError::Refused`] — a glyph the chosen face cannot represent (R71).
/// - [`AddTextError::NoWordsToWrap`] — the text is entirely whitespace.
#[allow(
    clippy::too_many_arguments,
    reason = "the layout inputs (text, size, alignment, leading, encoding, face, box, page) are each a distinct, irreducible parameter shared verbatim by the mutating add and the pure preview; bundling them into a struct would only move the same 9 fields elsewhere"
)]
fn layout_boxed(
    text: &str,
    size: f64,
    alignment: BlockAlignment,
    leading_opt: Option<f64>,
    inv: &InverseEncoding,
    font: Std14,
    enc: BaseEncoding,
    bx: Rect,
    page_crop: Rect,
) -> Result<BoxedLayout, AddTextError> {
    // Takes the placement inputs explicitly (not an `AddTextRequest`) so the
    // pure `preview_wrap` reuses this ONE layout pass verbatim (spec §4.2 /
    // decision 016 §0.3) — the mutating add and the live preview can never
    // wrap the same string two different ways.
    let w = bx.width();
    let h = bx.height();
    if !(w.is_finite() && w > 0.0 && h.is_finite() && h > 0.0) {
        return Err(AddTextError::InvalidBox(w, h));
    }
    let empty = BTreeSet::new();

    // Representative inter-word space: the face's own space-glyph advance
    // (fallback 0.25·size only if the face reports none — disclosed).
    let space_code = inv
        .encode_str(" ", &empty)
        .ok()
        .and_then(|r| r.codes.first().copied())
        .unwrap_or(b' ');
    let mut space_width = code_advance(font, enc, space_code, size);
    let mut space_estimated = false;
    if space_width <= 0.0 {
        space_width = FALLBACK_SPACE_FRAC * size;
        space_estimated = true;
    }

    // Leading: operator override or the derived 1.2·size default.
    let (leading, leading_estimated) = match leading_opt {
        Some(l) if l.is_finite() && l > 0.0 => (l, false),
        _ => (DEFAULT_LEADING_FRAC * size, true),
    };

    let wrap_width = w;
    let block_llx = bx.llx;
    let ascent = ASCENT_FRAC * size;
    let descent = DESCENT_FRAC * size;
    let first_baseline = bx.ury - ascent;

    // Tokenise into paragraphs + words; wrap each paragraph via the shared
    // breaker. Each word carries its source codes (re-emitted verbatim by the
    // commit path), its ORIGINAL text (rendered by the preview path), and its
    // measured width — one tokenisation feeds both consumers.
    let mut lines: Vec<LaidLine> = Vec::new();
    let mut encode_disclosures: Vec<String> = Vec::new();
    let mut any_word = false;
    for para in text.split('\n') {
        let mut words: Vec<(Vec<u8>, String, f64)> = Vec::new();
        for word in para.split_whitespace() {
            let enc_res = inv
                .encode_str(word, &empty)
                .map_err(AddTextError::Refused)?;
            for d in &enc_res.disclosures {
                if !encode_disclosures.contains(d) {
                    encode_disclosures.push(d.clone());
                }
            }
            let width = measure_codes(font, enc, &enc_res.codes, size);
            words.push((enc_res.codes, word.to_owned(), width));
        }
        if words.is_empty() {
            lines.push(LaidLine::blank());
            continue;
        }
        any_word = true;
        let widths: Vec<f64> = words.iter().map(|(_, _, wd)| *wd).collect();
        let ranges = greedy_pack(words.len(), wrap_width, |s, e| {
            line_natural_width(&widths, space_width, s, e)
        });
        let last_idx = ranges.len().saturating_sub(1);
        for (li, r) in ranges.into_iter().enumerate() {
            let natural_width = line_natural_width(&widths, space_width, r.start, r.end);
            let slice = words.get(r.clone()).unwrap_or(&[]);
            let word_codes: Vec<Vec<u8>> = slice.iter().map(|(c, _, _)| c.clone()).collect();
            let line_text = slice
                .iter()
                .map(|(_, t, _)| t.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let gap_count = word_codes.len().saturating_sub(1);
            let is_overflowing_word =
                word_codes.len() == 1 && natural_width > wrap_width + OVERFLOW_EPS;
            lines.push(LaidLine {
                words: word_codes,
                text: line_text,
                natural_width,
                origin_x: 0.0,
                baseline_y: 0.0,
                gap_count,
                justified_slack: None,
                is_last_of_para: li == last_idx,
                is_overflowing_word,
                blank: false,
            });
        }
    }
    if !any_word {
        return Err(AddTextError::NoWordsToWrap);
    }

    // Place each line: top-anchored baseline by global index, x by alignment,
    // justified slack for full non-last multi-word lines only (§3.1/§4.1).
    let n = lines.len();
    for (i, line) in lines.iter_mut().enumerate() {
        line.baseline_y = first_baseline - leading * (i as f64);
        if line.blank {
            line.origin_x = block_llx;
            continue;
        }
        line.origin_x = align_origin_x(alignment, block_llx, wrap_width, line.natural_width);
        line.justified_slack = if alignment.is_justified()
            && !line.is_last_of_para
            && line.gap_count >= 1
            && !line.is_overflowing_word
        {
            Some((wrap_width - line.natural_width).max(0.0))
        } else {
            None
        };
    }

    // Overflow (R76): box bottom, then page cropbox bottom. Everything is
    // EMITTED regardless — these are disclosures, never clips.
    let last_baseline = first_baseline - leading * ((n.saturating_sub(1)) as f64);
    let block_bottom = last_baseline - descent;
    let box_bottom = bx.lly;
    let box_overflow_lines = lines
        .iter()
        .filter(|l| !l.blank && (l.baseline_y - descent) < box_bottom - OVERFLOW_EPS)
        .count();
    let crop_bottom = page_crop.lly;
    let past_page = crop_bottom - block_bottom;
    let (page_overflow_pt, page_lines_off) = if past_page > OVERFLOW_EPS {
        let off = lines
            .iter()
            .filter(|l| !l.blank && (l.baseline_y - descent) < crop_bottom - OVERFLOW_EPS)
            .count();
        (past_page, off)
    } else {
        (0.0, 0)
    };

    let wrapped_lines = lines.iter().filter(|l| !l.blank).count();
    let mut disclosures: Vec<String> = Vec::new();
    disclosures.push(format!(
        "boxed add: wrapped to {wrapped_lines} line(s) at {wrap_width:.1}pt box width, \
         {} alignment, top-anchored from the box top at {leading:.2}pt leading{} — DERIVED \
         layout (greedy first-fit, ISO 32000-1 §9.4.4 advances via the shipped 15.x breaker; \
         §14.8 S1-S9), a reviewable add",
        alignment.as_str(),
        if leading_estimated {
            " (derived default 1.2 x size)"
        } else {
            ""
        },
    ));
    if space_estimated {
        disclosures.push(format!(
            "boxed add: inter-word space width estimated at {space_width:.2}pt (0.25 x size) — \
             the chosen face reports no advance for its space glyph"
        ));
    }
    let overflow_words = lines.iter().filter(|l| l.is_overflowing_word).count();
    if overflow_words > 0 {
        disclosures.push(format!(
            "boxed add: {overflow_words} word(s) are wider than the {wrap_width:.1}pt box width \
             and overflow their line unbroken — no hyphenation (whitespace-only breaks)"
        ));
    }
    if box_overflow_lines > 0 {
        disclosures.push(format!(
            "boxed add: the wrapped text overflows the box by {box_overflow_lines} line(s) — all \
             lines are EMITTED as real page content at their true positions, never clipped (R76)"
        ));
    }
    if page_overflow_pt > 0.0 {
        disclosures.push(format!(
            "boxed add: the wrapped run grows {page_overflow_pt:.1}pt past the page (cropbox); \
             {page_lines_off} line(s) fall off the visible page — EMITTED as recoverable content, \
             not clipped (R76)"
        ));
    }
    disclosures.extend(encode_disclosures);

    Ok(BoxedLayout {
        lines,
        space_code,
        disclosures,
        wrapped_lines,
        box_overflow_lines,
        page_overflow_pt,
        alignment,
    })
}

/// One laid-out line of a boxed wrap PREVIEW (Pass 16.2 §4.2): the literal
/// text shown on the line plus its per-line origin in default user space
/// (§9.4.4, `x` = left edge of the shown text under the chosen alignment,
/// `baseline_y` = the text baseline).
///
/// The read-only analogue of one emitted `Tm`+show line in the committed add:
/// because [`preview_wrap`] and [`add_text`]'s boxed path both run through the
/// SAME [`layout_boxed`] pass, `origin_x`/`baseline_y` here equal the `Tm`
/// operands [`add_text`] will actually write, and `text` is the words that line
/// will show — so the operator reviews exactly what the commit places (decision
/// 016 §0.3). A blank line (an empty paragraph from a hard `\n\n`) carries an
/// empty [`Self::text`] but a real [`Self::baseline_y`], so following lines
/// drop correctly; the GUI simply paints nothing for an empty string.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct WrapPreviewLine {
    /// The words shown on this line, joined by single spaces (empty = blank).
    pub text: String,
    /// The left edge of the shown text, points (default user space) — already
    /// alignment-placed (Left/Center/Right/Justified).
    pub origin_x: f64,
    /// The text baseline, points (default user space), top-anchored from the
    /// box top by the line's index times the leading.
    pub baseline_y: f64,
}

/// The result of a pure, read-only boxed wrap PREVIEW (Pass 16.2 §4.2 /
/// decision 016 §0.3) — the read-only analogue of a [`ReflowPreview`]
/// (crate::text_edit::ReflowPreview), but wrapping a literal, not-yet-committed
/// STRING into a box instead of re-planning an already-recognised block.
///
/// Produced by [`preview_wrap`] and shaped so the box-mode live-typing UI can
/// draw a ghost of the wrapped run every keystroke without hand-rolling its own
/// greedy wrap — the wrap decisions the operator reviews are the exact ones the
/// commit ([`add_text`] with a box) re-derives, because both share one
/// [`layout_boxed`] pass. Overflow (R76) is reported, never a clip: the lines
/// are all present with their true origins even when they spill the box or the
/// page, exactly as the committed add emits them.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AddTextWrapPreview {
    /// The wrapped lines, top-to-bottom (blank lines included so a following
    /// line's baseline stays faithful; a blank line's `text` is empty).
    pub lines: Vec<WrapPreviewLine>,
    /// How many non-blank lines the text wrapped to (matches
    /// [`AddTextReport::wrapped_lines`] for identical inputs).
    pub wrapped_lines: usize,
    /// How many lines fall (in whole or part) below the box bottom (R76) —
    /// matches [`AddTextReport::box_overflow_lines`].
    pub box_overflow_lines: usize,
    /// How far the run grows past the page cropbox bottom, points (R76) —
    /// matches [`AddTextReport::page_overflow_pt`]. Never a clip.
    pub page_overflow_pt: f64,
    /// The alignment placed (the explicit operator input, default Left).
    pub alignment: BlockAlignment,
    /// The layout disclosures (derived-layout, overflow, estimated-space) —
    /// the SAME strings the committed add surfaces, so the box-mode preview can
    /// show overflow warnings live (spec §4.2 / §6).
    pub disclosures: Vec<String>,
}

/// Lay out a literal string into a wrap box WITHOUT modifying anything — the
/// pure, read-only wrap preview the Pass 16.2 box-mode live-typing UI calls
/// every frame (decision 016 §0.3 / spec §4.2).
///
/// This is the read-only analogue of the mutating boxed [`add_text`]: it runs
/// the SAME [`layout_boxed`] pass (greedy first-fit breaker + AFM `/Widths`
/// measurement + top-anchored placement + alignment origins + R76 overflow),
/// then projects the placed lines into an [`AddTextWrapPreview`] instead of
/// emitting a content stream. Because the two share one pass, the per-line
/// origins and overflow returned here are **byte-for-byte** what a subsequent
/// [`add_text`] with the SAME box/text/face/size/alignment/leading will write —
/// the round-trip the GUI relies on so the ghost it draws is the run it commits
/// (no GUI-side greedy-wrap approximation, the duplication-drift risk called
/// out in decision 016 §0.3).
///
/// GUI-core separation (`ARCHITECTURE.md` §3): this takes plain page-space
/// rectangles and a [`Std14`] face and returns plain numbers/strings — no GUI
/// type, no `&Document`, no mutation — so it lives in `pdfcer-core` and the
/// shell only reads its result.
///
/// - `wrap_box` — the box in default user space (lower-left/upper-right), the
///   SAME rectangle [`AddTextRequest::with_box`] builds from `(x, y, w, h)`.
/// - `page_crop` — the target page's cropbox, needed only to compute
///   `page_overflow_pt` (R76); pass the page's `crop_box`.
/// - `font`/`size`/`alignment`/`leading` — the face metrics and layout inputs,
///   identical in meaning to the corresponding [`AddTextRequest`] fields.
///
/// # Errors
///
/// [`AddTextError`] — [`AddTextError::EmptyText`] (empty string),
/// [`AddTextError::InvalidSize`] (non-positive/non-finite size),
/// [`AddTextError::InvalidBox`] (non-positive/non-finite box),
/// [`AddTextError::Refused`] (a glyph the chosen face cannot represent, R71),
/// or [`AddTextError::NoWordsToWrap`] (whitespace-only). These mirror the
/// boxed [`add_text`]'s own reachable refusals for the same inputs, so a
/// preview that refuses predicts a commit that would refuse identically.
///
/// # Examples
///
/// ```
/// use pdfcer_core::fontdata::Std14;
/// use pdfcer_core::page_tree::Rect;
/// use pdfcer_core::text_edit::{preview_wrap, BlockAlignment};
///
/// let bx = Rect::from_corners(72.0, 600.0, 72.0 + 180.0, 600.0 + 120.0);
/// let page = Rect::from_corners(0.0, 0.0, 612.0, 792.0);
/// let preview = preview_wrap(
///     "wrap this sentence into the box",
///     bx,
///     page,
///     Std14::Helvetica,
///     12.0,
///     BlockAlignment::Left,
///     None,
/// )
/// .unwrap();
/// assert!(preview.wrapped_lines >= 1);
/// // The first line's origin is the box's own left edge under Left alignment.
/// assert!((preview.lines[0].origin_x - 72.0).abs() < 1e-9);
/// ```
pub fn preview_wrap(
    text: &str,
    wrap_box: Rect,
    page_crop: Rect,
    font: Std14,
    size: f64,
    alignment: BlockAlignment,
    leading: Option<f64>,
) -> Result<AddTextWrapPreview, AddTextError> {
    // Same guards, in the same order, as `plan_add_text` — so a preview refuses
    // exactly where the commit would (empty text, bad size), never surfacing a
    // ghost the commit could not place.
    if text.is_empty() {
        return Err(AddTextError::EmptyText);
    }
    if !size.is_finite() || size <= 0.0 {
        return Err(AddTextError::InvalidSize(size));
    }
    let (inv, enc, _symbolic) = face_encoding(font);
    let layout = layout_boxed(
        text, size, alignment, leading, &inv, font, enc, wrap_box, page_crop,
    )?;
    let lines = layout
        .lines
        .iter()
        .map(|l| WrapPreviewLine {
            text: l.text.clone(),
            origin_x: l.origin_x,
            baseline_y: l.baseline_y,
        })
        .collect();
    Ok(AddTextWrapPreview {
        lines,
        wrapped_lines: layout.wrapped_lines,
        box_overflow_lines: layout.box_overflow_lines,
        page_overflow_pt: layout.page_overflow_pt,
        alignment: layout.alignment,
        disclosures: layout.disclosures,
    })
}

/// Build the multi-line boxed `q BT…ET Q` content bytes (decision 016 §6/16.1).
///
/// Uses the **absolute-`Tm`-per-line** emission (recipe C of
/// `iso32000__ref__reflow_emission.md` §3.2): each line sets its own
/// `1 0 0 1 x y Tm`, immune to the relative-`Td` accumulation bug — and,
/// because the whole stream is NEW content, the larger-diff cost recipe C
/// carries for an *in-place* reflow does not apply here. `Tf`, the fill colour,
/// and the `q…Q` isolation are set once exactly as the point path
/// ([`build_content`]) does (§8.4.2: `q/Q` do not reset to Table-52 initials,
/// so every relied-on parameter is set explicitly). A justified full line is
/// emitted as one `[ (w SP) N … ] TJ` with the per-gap slack `N` (§9.4.3);
/// every other line is one `(…) Tj`. Blank lines (empty paragraphs) emit
/// nothing but have already consumed a baseline in [`layout_boxed`].
fn build_content_boxed(
    font_name: &[u8],
    size: f64,
    color: NewTextColor,
    layout: &BoxedLayout,
) -> Vec<u8> {
    let mut out = Vec::new();
    // Leading token separator (§7.2), q/Q isolation (§8.4.2).
    out.push(b'\n');
    out.extend_from_slice(b"q\n");
    out.extend_from_slice(b"BT\n");
    out.push(b'/');
    out.extend_from_slice(font_name);
    out.push(b' ');
    emit_number(&mut out, size);
    out.extend_from_slice(b" Tf\n");
    match color {
        NewTextColor::Black => out.extend_from_slice(b"0 g\n"),
        NewTextColor::Rgb(r, g, b) => {
            emit_number(&mut out, r.clamp(0.0, 1.0));
            out.push(b' ');
            emit_number(&mut out, g.clamp(0.0, 1.0));
            out.push(b' ');
            emit_number(&mut out, b.clamp(0.0, 1.0));
            out.extend_from_slice(b" rg\n");
        }
    }
    for line in &layout.lines {
        if line.blank {
            continue;
        }
        // Absolute placement per line (recipe C): `1 0 0 1 x y Tm`.
        out.extend_from_slice(b"1 0 0 1 ");
        emit_number(&mut out, line.origin_x);
        out.push(b' ');
        emit_number(&mut out, line.baseline_y);
        out.extend_from_slice(b" Tm\n");
        emit_boxed_line_show(&mut out, line, layout.space_code, size);
        out.push(b'\n');
    }
    out.extend_from_slice(b"ET\n");
    out.push(b'Q');
    out
}

/// Emit one boxed line's show operator: a justified full line as
/// `[ (w0 SP) N (w1 SP) N … (wlast) ] TJ` (negative `N` opens each gap,
/// §9.4.3), every other line as `(joined codes) Tj`. The code-32 space is kept
/// INSIDE the preceding string so text extraction still sees the word break
/// (§14.8.2.4). `emit_scale` is `Tfs·Th·a = size` here (identity `Tm`,
/// `Tz`=100), so the per-gap number is `−(slack/G)·1000/size` — the exact
/// sign-mirror of the 14.1 pin and the same formula as
/// [`super::reflow_apply`]'s `emit_justified_line`.
fn emit_boxed_line_show(out: &mut Vec<u8>, line: &LaidLine, space_code: u8, size: f64) {
    match line.justified_slack {
        Some(slack) if slack > 0.0 && line.gap_count >= 1 => {
            let g = line.gap_count as f64;
            let n_gap = if size.abs() > OVERFLOW_EPS {
                -(slack / g) * 1000.0 / size
            } else {
                0.0
            };
            let last = line.words.len().saturating_sub(1);
            out.push(b'[');
            for (i, w) in line.words.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                    emit_number(out, n_gap);
                    out.push(b' ');
                }
                let mut s = w.clone();
                if i != last {
                    s.push(space_code); // keep the word-break code in the string
                }
                emit_literal_string(out, &s);
            }
            out.extend_from_slice(b"] TJ");
        }
        _ => {
            let mut s: Vec<u8> = Vec::new();
            for (i, w) in line.words.iter().enumerate() {
                if i > 0 {
                    s.push(space_code);
                }
                s.extend_from_slice(w);
            }
            emit_literal_string(out, &s);
            out.extend_from_slice(b" Tj");
        }
    }
}

/// Build the minimal Standard-14 Type1 font dict (§9.6.2.2), full `/Widths`
/// form, no `/FontFile` (R79).
///
/// `/Encoding /WinAnsiEncoding` for the 12 Latin faces; omitted for
/// `Symbol`/`ZapfDingbats` (built-in encodings). `/Widths` covers codes 32..255
/// from `fontdata::std14_width` under the SAME encoding used to encode the run
/// (so codes and widths agree); a code with no glyph gets width 0 (§9.8.2
/// `MissingWidth` default).
pub(crate) fn build_font_dict(
    base_font: &str,
    symbolic: bool,
    enc: BaseEncoding,
    font: Std14,
) -> Object {
    const FIRST_CHAR: u16 = 32;
    const LAST_CHAR: u16 = 255;

    let mut d = Dict::new();
    d.insert(Name::from(b"Type"), Object::Name(Name::from(b"Font")));
    d.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Type1")));
    d.insert(
        Name::from(b"BaseFont"),
        Object::Name(Name(base_font.as_bytes().to_vec())),
    );
    if !symbolic {
        d.insert(
            Name::from(b"Encoding"),
            Object::Name(Name::from(b"WinAnsiEncoding")),
        );
    }
    d.insert(
        Name::from(b"FirstChar"),
        Object::Integer(i64::from(FIRST_CHAR)),
    );
    d.insert(
        Name::from(b"LastChar"),
        Object::Integer(i64::from(LAST_CHAR)),
    );
    let widths: Vec<Object> = (FIRST_CHAR..=LAST_CHAR)
        .map(|code| {
            let w = u8::try_from(code)
                .ok()
                .and_then(|c| fontdata::encoding_glyph_name(enc, c))
                .and_then(|name| fontdata::std14_width(font, name))
                .unwrap_or(0);
            Object::Integer(i64::from(w))
        })
        .collect();
    d.insert(Name::from(b"Widths"), Object::Array(widths));
    Object::Dict(d)
}

/// The complete standard-14 `/Font` resource dictionary for `font`, ready to
/// be written into a `/Resources` `/Font` (`Pass 162.0`).
///
/// # Why this wrapper exists rather than two calls at each site
///
/// Building the dictionary correctly needs **two** decisions that must agree:
/// whether the face is symbolic (which decides whether `/Encoding` is written
/// at all), and which [`BaseEncoding`] the `/Widths` array is indexed by. Get
/// them out of step and the file still opens — the dictionary declares one
/// encoding and its widths were measured under another, so every advance is
/// plausibly wrong and the text simply mis-spaces. Nothing errors.
///
/// [`face_encoding`] already answers both, and [`build_font_dict`] already
/// consumes them, but the pairing lived only inside `add_text`'s call site.
/// `Pass 162.0` added a **second** caller — `format_text`'s `--set-font`, which
/// may now introduce a face the page does not carry — and a second inline
/// pairing is exactly the arrangement in which the two come to disagree
/// (`R171`). One function, one convention.
///
/// The `InverseEncoding` [`face_encoding`] also returns is discarded here: this
/// caller is authoring a **resource**, not encoding a run. That is a few
/// microseconds of wasted table-building per invocation, and it is deliberately
/// preferred over a second copy of the symbolic/encoding rule.
///
/// # What it emits, and why the full form
///
/// `/Type`, `/Subtype /Type1`, `/BaseFont`, `/FirstChar 32`, `/LastChar 255`
/// and `/Widths` — plus `/Encoding /WinAnsiEncoding` for the twelve Latin
/// faces, **omitted** for `Symbol` and `ZapfDingbats`, whose built-in
/// encodings (Annex D.5/D.6) are the only correct ones.
///
/// ISO 32000-1 §9.6.2.2 permits a bare four-key dictionary for a standard-14
/// face, and PDF 1.5 **deprecates** that special treatment as a `should` —
/// *"conforming writers should represent all fonts using a complete font
/// descriptor"*, while *"conforming readers shall still provide the special
/// treatment"*. So the bare form is universally readable and the full form is
/// what the standard prefers. pdfcer emits `/Widths` because it costs nothing
/// (the metrics are already compiled in, from the APAFML Core-14 AFMs) and it
/// makes the run's spacing **self-contained** — a reader with no built-in
/// standard-14 metrics still lays it out correctly.
///
/// `/FontDescriptor` is **not** emitted. It is optional for a standard-14 face
/// with no font program, and omitting it keeps every value in this dictionary
/// **direct**, so the dictionary is complete before any object number is
/// allocated. That is what lets `format_text` hand it to its existing
/// coverage gate at PLAN time and write it at COMMIT time (`R221`: the answer
/// a caller previews and the answer the commit enforces cannot drift).
pub(crate) fn std14_resource_dict(font: Std14) -> Object {
    let (_inverse, enc, symbolic) = face_encoding(font);
    build_font_dict(fontdata::std14_base_font_name(font), symbolic, enc, font)
}

/// Pick a `/Font` resource name not already present in `existing`.
///
/// `pdfceF1`, then `pdfceF2`, … — the first unused, so the new name can never
/// shadow a font the original content stream depends on (§7.8.3: resource names
/// are local to the stream, but the page and the appended stream SHARE one
/// effective resource dict). The `pdfcer` prefix keeps it clear of the common
/// `/F{n}` producer convention.
pub(crate) fn pick_font_name(existing: &Dict) -> Vec<u8> {
    (1u32..=u32::MAX)
        .map(|n| format!("pdfceF{n}"))
        .find(|cand| existing.get(cand.as_bytes()).is_none())
        .unwrap_or_else(|| "pdfceFx".to_owned())
        .into_bytes()
}

/// Bind a new `/Font` resource under `key` into `owner_id`'s effective
/// `/Resources` `/Font`, returning **every object that must be written** and
/// whether the dictionary it landed in is shared (`Pass 162.0`).
///
/// The returned vector always begins with the font dictionary itself at
/// `font_id`, followed by whichever enclosing object had to change.
///
/// # Why this is a free function over [`ObjectGraph`] and not a method
///
/// There are **three** save paths that can introduce a font resource, and they
/// do not share a type: `EditSession::format_text` (page), the same session's
/// form twin, and the one-shot `text_edit::set_format`, which builds a
/// [`DirtySet`](crate::writer::DirtySet) against an immutable `&Document`. The
/// first version of this Pass bound the resource inside the session and shipped
/// a **half-wired feature**: the CLI, which uses the one-shot path, printed the
/// disclosure saying a resource had been added and saved a file in which it had
/// not — a content stream naming `/pdfceF1` and no `/pdfceF1` anywhere. Every
/// unit test passed, because they exercise the session.
///
/// So the rule lives in one function, over the narrowest thing all three can
/// supply: a graph that can resolve an id. `R171` — and this is the case where
/// the second implementation would have been *invisible*, since the two paths
/// produce different file bytes for the same request and only one is covered by
/// unit tests.
///
/// # ★★ Inheritance first, because getting it wrong breaks the page
///
/// §7.8.3: a page's own `/Resources` **replaces** the one it would inherit from
/// its `/Pages` ancestors — it does not merge. On a page with no `/Resources`
/// of its own, creating a direct one holding just the new font would **shadow**
/// every inherited font, image and colour space the page's existing content
/// already names. The file still parses; the page's original text stops
/// resolving its fonts.
///
/// So the object that actually holds `/Resources` is located first, walking
/// `/Parent` when `may_inherit` (pages) and not when it does not — a form
/// XObject carries its own (§8.10.1) and has no page-tree parent to inherit
/// from. Depth-guarded per `ARCHITECTURE.md` §10: `/Parent` in a damaged or
/// hostile file can cycle.
///
/// # ★★ Why a shared `/Resources` is PATCHED IN PLACE and not cloned
///
/// A page's `/Resources`, and the `/Font` sub-dictionary inside it, are very
/// often indirect objects shared by every page a producer emitted. There are
/// two ways to add an entry:
///
/// * **Patch the shared object.** Every page sharing it gains one extra
///   `/Font` entry, which is **unreferenced** on those pages — no content
///   stream there names `pdfceFn` — so nothing about how they render changes.
/// * **Clone it for this page.** The page stops sharing, which is a
///   *structural rewrite of the document performed as a side effect of a text
///   restyle*, exactly what project rule 3 forbids. It also silently
///   de-duplicates a producer's deliberate sharing and grows the file by the
///   size of the whole resource dictionary.
///
/// pdfcer patches. An unreferenced extra entry is inert; an unrequested
/// structural change is not. `shared` is returned so the caller can say so.
///
/// The write is made at the **innermost indirect level**, so the fewest objects
/// change: if `/Font` is its own object, only that object moves.
pub(crate) fn bind_font_resource<G: crate::graph::ObjectGraph + ?Sized>(
    graph: &G,
    owner_id: ObjId,
    may_inherit: bool,
    key: &[u8],
    font_id: ObjId,
    font: Dict,
) -> (Vec<(ObjId, Object)>, bool) {
    let mut out: Vec<(ObjId, Object)> = vec![(font_id, Object::Dict(font))];
    let bind = Object::Reference(font_id);

    // --- which object holds /Resources? ---
    let mut holder = owner_id;
    if may_inherit {
        let mut cursor = owner_id;
        for _ in 0..crate::outline::MAX_OUTLINE_DEPTH {
            let Some(Object::Dict(d)) = graph.value(cursor) else {
                break;
            };
            if d.get(b"Resources").is_some() {
                holder = cursor;
                break;
            }
            match d.get(b"Parent") {
                Some(Object::Reference(r)) if *r != cursor => cursor = *r,
                // Nobody in the chain has one: fall back to the owner, where
                // creating a direct `/Resources` shadows nothing.
                _ => break,
            }
        }
    }
    let owner = match graph.value(holder) {
        Some(Object::Dict(d)) => d.clone(),
        _ => Dict::new(),
    };

    // --- where does /Resources live on it? ---
    let res_ref = match owner.get(b"Resources") {
        Some(Object::Reference(r)) => Some(*r),
        _ => None,
    };
    let mut resources = match owner.get(b"Resources") {
        // A `/Resources` that does not resolve to a dictionary is treated as
        // ABSENT rather than as an error, matching `outline_root`'s handling of
        // a damaged `/Outlines`: refusing an edit because a broken file carries
        // `/Resources 7` is the worse outcome.
        Some(Object::Reference(r)) => match graph.value(*r) {
            Some(Object::Dict(d)) => d.clone(),
            _ => Dict::new(),
        },
        Some(Object::Dict(d)) => d.clone(),
        _ => Dict::new(),
    };

    // --- and /Font inside that? ---
    let font_ref = match resources.get(b"Font") {
        Some(Object::Reference(r)) => Some(*r),
        _ => None,
    };
    let mut fonts = match resources.get(b"Font") {
        Some(Object::Reference(r)) => match graph.value(*r) {
            Some(Object::Dict(d)) => d.clone(),
            _ => Dict::new(),
        },
        Some(Object::Dict(d)) => d.clone(),
        _ => Dict::new(),
    };
    fonts.insert(Name(key.to_vec()), bind);

    let shared = res_ref.is_some() || font_ref.is_some() || holder != owner_id;

    if let Some(fid) = font_ref {
        out.push((fid, Object::Dict(fonts)));
        return (out, shared);
    }
    resources.insert(Name::from(b"Font"), Object::Dict(fonts));
    if let Some(rid) = res_ref {
        out.push((rid, Object::Dict(resources)));
        return (out, shared);
    }
    let mut owner = owner;
    owner.insert(Name::from(b"Resources"), Object::Dict(resources));
    out.push((holder, Object::Dict(owner)));
    (out, shared)
}

/// Whether the document is tagged: `/StructTreeRoot` present, or `/MarkInfo
/// /Marked true` (R73 trigger).
fn is_tagged<G: ObjectGraph + ?Sized>(graph: &G) -> bool {
    let Some(catalog) = graph.catalog_dict() else {
        return false;
    };
    if catalog.get(b"StructTreeRoot").is_some() {
        return true;
    }
    match catalog.get(b"MarkInfo") {
        Some(mi) => matches!(
            graph.resolve(mi).as_dict().and_then(|d| d.get(b"Marked")),
            Some(Object::Boolean(true))
        ),
        None => false,
    }
}

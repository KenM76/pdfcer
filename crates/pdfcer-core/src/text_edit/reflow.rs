//! # `reflow` — within-block offline text reflow ENGINE (FF-A, READ-ONLY)
//!
//! Pass 15.0 of pdfcer
//! (`docs/decisions/015-ffa-within-block-offline-reflow.md` §6). The
//! **derived, read-only** first slice of within-block reflow: given one
//! recognized [`Block`], a wrap width, an alignment and a leading, it
//! computes a [`ReflowPreview`] — the new line breaks, the new per-line
//! origins, the alignment placement, the resulting new block box (with its
//! height change), and a page-overflow disclosure if the re-wrap would grow
//! the block past a supplied page cropbox. **Nothing is written.** No
//! content-stream mutation, no `EditSession` command, no save: that is Pass
//! 15.1 (the advance-preserving surgery). 15.0 computes a *preview* the
//! operator would accept or reject.
//!
//! ## Everything here is DERIVED (rule 4 / §14.8 S1-S9)
//!
//! A reflow re-decides where lines break and where each line sits — layout
//! the file never stated (§14.8 negative results S1–S9: an untagged content
//! stream defines no word/line/paragraph/reading order). So, exactly like
//! the block model it builds on ([`super::model`]), every judgement here is
//! **counted and disclosed** in [`ReflowDiagnostics`] and presented as a
//! reviewable preview, never a silent re-layout (decision 015 §3.3/§3.4,
//! standing rules R75–R77). The engine mutates nothing and borrows the
//! model immutably.
//!
//! ## How it measures (§9.4.4 — the provenance-advance measurer)
//!
//! Decision 015 §3.2 (option **VT-extend**) requires ONE greedy breaker
//! shared with [`crate::vartext`], differing only in the width measurer.
//! `vartext` measures `WinAnsi` bytes by Std14 AFM widths; the reflow
//! engine measures the block's **own glyphs by their real §9.4.4 advances**
//! — [`ExtractedGlyph::advance`](crate::text_extract::ExtractedGlyph), the
//! displacement Pass 4 already computed from the actual embedded/supplied
//! font `Widths` (plus `Tc`/`Tw`/`Tz`, §9.4.4). Because a 15.0 preview only
//! re-wraps and re-aligns the *same glyphs at the same size* (the operator
//! adjusts width/alignment/leading, never the font — decision 015 §3.4),
//! those advances are exactly the widths the re-wrap needs; no font table
//! re-measurement is required. A word's width is the sum of its glyphs'
//! advances; the representative inter-word space width is the median of the
//! block's own space-glyph advances.
//!
//! ## Tokenisation — whitespace U+0020 only, line breaks are word breaks
//!
//! The block's glyphs are read in reading order (its lines top-to-bottom,
//! each line's glyphs in content order) and split into **words** at
//! **U+0020 space glyphs only** (decision 015 §3.2 — no soft-hyphen, no
//! hyphenation, no CJK per-glyph breaking). A model line boundary is also a
//! word break: the derived line breaks are the *old* wrapping, discarded
//! here, but a word never spans two source lines in the LTR simple-font
//! corpus this Pass targets. Runs of spaces collapse; a space glyph's
//! advance feeds the representative space width. (Limitation: a word
//! boundary realised as a `DerivedWordSpace` run rather than a real space
//! glyph is not seen — disclosed; the synthetic corpus uses real space
//! glyphs. Composite/CJK/RTL and `Tw`-vs-glyph-space nuance are FF-E/FF-F.)
//!
//! ## Alignment auto-detect + preserve (§3.6 / R77 — the differentiator)
//!
//! The block's original alignment is **inferred from glyph x-positions**,
//! reusing the 14.0 line boxes: **Left** (left edges flush at the block
//! `llx`, right ragged), **Right** (right edges flush at `urx`, left
//! ragged), **Center** (per-line midpoints flush at the block centre, both
//! ragged), **Justified** (all-but-last lines flush BOTH margins, the last
//! line short). A single-line block is ambiguous → **Left**, disclosed. The
//! inference is counted and **operator-overridable**, and preserved through
//! the re-wrap by default. Acrobat has no such auto-detect (decision 015
//! §9) — re-wrapping a centred/right paragraph there risks a silent
//! left-align.
//!
//! ## Justified placement (§3.1 — computed here, emitted in 15.1)
//!
//! For a justified block, each full (non-last) line's **slack** =
//! `wrap_width − natural_line_width` is recorded as the line's positioning
//! intent ([`ReflowLine::justified_slack`]), to be distributed across its
//! inter-word gaps as `TJ` (§9.4.3) / `Tw` (§9.3.3) when 15.1 emits it. The
//! **last line of the paragraph is never stretched** (stays at the base
//! alignment); a single-word line (no gap) also falls back and is
//! disclosed. 15.0 only *computes* this placement — no operator is emitted.
//!
//! ## Vertical growth + page overflow (§3.5 / R76 — disclose, never hide)
//!
//! The block is **top-anchored**: the first baseline is fixed and the block
//! grows/shrinks **downward** as lines are added/removed. If a page cropbox
//! is supplied and the new box would fall below it, the overflow is
//! **disclosed** ([`PageOverflow`]) — the preview still computes *all*
//! lines (the off-page ones are real content), never clipping them to
//! invisible. This is a deliberate divergence from Acrobat's documented
//! silent "disappear"; 15.1 emits such content at its true off-page
//! position (recoverable), and 15.0 discloses that it would.

use core::ops::Range;

use crate::page_tree::Rect;
use crate::text_extract::PageText;

use super::model::{Block, BlockRecognitionOptions, EditableTextModel, GlyphRef};

/// Ascent as a fraction of the effective size, matching the block model's
/// own line box (`ury = baseline + 0.75·size`).
const ASCENT_FRAC: f64 = 0.75;
/// Descent as a fraction of the effective size (`lly = baseline −
/// 0.25·size`), matching the block model.
const DESCENT_FRAC: f64 = 0.25;
/// Fallback leading when the block has no baseline-gap to measure (a
/// single source line): a plausible 1.2·size. Disclosed when used.
const FALLBACK_LEADING_FRAC: f64 = 1.2;
/// Fallback inter-word space width when the block carries no space glyph to
/// measure (every source line a single word): 0.25·size. Disclosed.
const FALLBACK_SPACE_FRAC: f64 = 0.25;
/// Alignment-flush tolerance: edges within `max(2.0, 0.5·size)` points of
/// each other count as aligned. A corpus-tunable heuristic (decision 015
/// §10 revisit trigger 2), disclosed as a threshold with no spec basis.
const ALIGN_TOL_FRAC: f64 = 0.5;
/// Absolute floor for the alignment tolerance, points.
const ALIGN_TOL_MIN: f64 = 2.0;
/// Justified detection needs at least this many lines: with only two lines
/// a single "body" line cannot distinguish justified from a short-last-line
/// left paragraph, so 2-line blocks never infer justified.
const JUSTIFY_MIN_LINES: usize = 3;
/// Float slack for "strictly wider than" comparisons, points.
const EPS: f64 = 1e-6;

/// The block-recognition options every reflow consumer (the engine's own
/// tests, `pdfcer`'s `reflow` preview path, and `pdfce-gui`'s Pass 15.2
/// reflow sub-mode) recognises paragraphs with: the
/// [`BlockRecognitionOptions`] defaults with first-line-indent paragraph
/// splitting effectively disabled (`indent_ratio` pushed out of practical
/// reach). Leading-gap paragraph splitting is UNCHANGED — this only relaxes
/// the indent rule.
///
/// # Why (decision 015 §3.2 / R77, Pass 15.2 UI spec §0.3)
///
/// A right-, centre-, or justified-aligned paragraph has **ragged left edges
/// by definition**. The default recogniser's first-line-indent rule reads
/// each such ragged line as a NEW paragraph, fragmenting the whole paragraph
/// into single-line blocks — which then makes alignment auto-detection
/// ([`ReflowEngine::detect_alignment`]) see only single-line blocks (always
/// [`AlignmentSource::SingleLineDefault`], never [`AlignmentSource::Detected`]),
/// defeating R77 for precisely the alignments R77 exists to differentiate.
///
/// This is the ONE source of truth for "how reflow recognises paragraph
/// boundaries": the free-function apply path
/// ([`super::reflow_apply::plan_reflow_from_doc`]), the session path
/// ([`crate::edit::EditSession::reflow_block`], which routes through it), the
/// CLI's `inspect reflow-preview`, and the GUI's caret-block resolution all
/// call THIS function, so the block index the GUI targets means the SAME
/// block the engine previews and the surgery re-emits — the config cannot
/// drift into two independently-tuned copies (the duplication-drift risk the
/// Pass 14.3 `font_subset_stem` hoist already named, one Pass later).
///
/// # Trade-off (not a strict improvement — disclosed, never papered over)
///
/// Relaxing `indent_ratio` fixes ragged-left fragmentation but loses a
/// different real signal: a traditional flush-left, first-line-indented
/// paragraph style (no blank line between paragraphs, each new paragraph
/// signalled only by an indented first line) will, under this config, MERGE
/// what should be two paragraphs into one block. The GUI keeps BOTH
/// recognitions (the default one for its general overlay, this relaxed one
/// for reflow targeting) and discloses when they disagree (Pass 15.2 §3),
/// rather than switching the whole overlay to the relaxed config.
// `BlockRecognitionOptions` is `#[non_exhaustive]`, so a struct literal is
// unavailable outside its own module — reassign-after-default is the only way
// to construct it here, which is exactly what this lint would otherwise flag.
#[allow(clippy::field_reassign_with_default)]
#[must_use]
pub fn reflow_recognition_options() -> BlockRecognitionOptions {
    let mut o = BlockRecognitionOptions::default();
    o.indent_ratio = 1.0e6;
    o
}

/// A block's paragraph alignment — the four modes FF-A supports (decision
/// 015 §3.1). Peer of [`crate::vartext::Quadding`] but with **Justified**,
/// which `Quadding`/`/Q` (§12.7.3.3) does not carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlockAlignment {
    /// Left-justified: left edges flush at the block `llx`, right ragged.
    Left,
    /// Right-justified: right edges flush at the block `urx`, left ragged.
    Right,
    /// Centred: per-line midpoints flush at the block centre.
    Center,
    /// Justified: all-but-last lines flush both margins; last line at the
    /// base alignment (decision 015 §3.1).
    Justified,
}

impl BlockAlignment {
    /// The lowercase keyword form, for CLI output and parsing.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Center => "center",
            Self::Justified => "justified",
        }
    }

    /// Parse a CLI/user keyword into an alignment, or `None` if it is not a
    /// recognised mode. Accepts `justify` as an alias for `justified` and
    /// `centre` for `center`; case-insensitive.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "left" | "l" => Some(Self::Left),
            "right" | "r" => Some(Self::Right),
            "center" | "centre" | "c" => Some(Self::Center),
            "justified" | "justify" | "j" => Some(Self::Justified),
            _ => None,
        }
    }

    /// Whether this is the justified mode (slack distribution applies).
    #[must_use]
    pub const fn is_justified(self) -> bool {
        matches!(self, Self::Justified)
    }
}

/// How the preview's alignment was arrived at — the honesty tag on a
/// derived-or-overridden choice (rule 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AlignmentSource {
    /// Auto-detected from glyph x-positions with a clear flush signal.
    Detected,
    /// The block has a single line; alignment defaulted to Left (ambiguous,
    /// §3.6).
    SingleLineDefault,
    /// No mode's flush signal was clear; defaulted to Left and disclosed.
    AmbiguousDefault,
    /// The operator overrode the detected value (§3.4).
    Overridden,
}

/// The outcome of alignment auto-detect (§3.6), with the raggedness
/// measurements that drove it — kept so a UI/CLI can show *why* a mode was
/// picked and a corpus can tune the threshold (decision 015 §10).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct DetectedAlignment {
    /// The inferred (or overridden) alignment.
    pub alignment: BlockAlignment,
    /// How it was arrived at.
    pub source: AlignmentSource,
    /// Spread (max − min) of the lines' left edges, points. Small ⇒ left
    /// (or justified) flush.
    pub left_ragged_pt: f64,
    /// Spread of the lines' right edges, points. Small ⇒ right (or
    /// justified) flush.
    pub right_ragged_pt: f64,
    /// Spread of the lines' midpoints, points. Small ⇒ centred.
    pub mid_ragged_pt: f64,
    /// The flush tolerance used, points (`max(2.0, 0.5·size)`).
    pub tolerance_pt: f64,
}

/// One line of a [`ReflowPreview`]: which words it holds and where they go.
///
/// The origin is the **left edge of the shown text** in default user space
/// — the x a `Tm`/`TD` would set (§9.4.2) — and [`Self::baseline_y`] its
/// baseline. Nothing is emitted in 15.0; these are the numbers 15.1 will
/// turn into show operators.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ReflowLine {
    /// The half-open word-index range (into the block's tokenised words)
    /// this line holds. Tiles `0..word_count` with its siblings.
    pub words: Range<usize>,
    /// The line's text, words joined by single spaces — a DERIVED rendering
    /// for review, never a sourced accessor.
    pub text: String,
    /// Origin x (left edge of the text) in default user space, per the
    /// alignment placement.
    pub origin_x: f64,
    /// Baseline y in default user space (top-anchored: line *i* sits at
    /// `first_baseline − i·leading`).
    pub baseline_y: f64,
    /// Natural width of the line (Σ word advances + representative space
    /// widths), points — before any justified stretch.
    pub natural_width: f64,
    /// Inter-word gaps on this line (`word count − 1`); the number of gaps
    /// justified slack would be distributed across.
    pub gap_count: usize,
    /// `true` when the line is a single word wider than the wrap width — an
    /// unbreakable overflow (no hyphenation), disclosed (§3.2).
    pub is_overflowing_word: bool,
    /// For a **justified**, non-last, multi-word line: the total slack
    /// (`wrap_width − natural_width`) to distribute across [`Self::gap_count`]
    /// gaps when 15.1 emits `TJ`/`Tw`. `None` for the last line (never
    /// stretched), for non-justified alignments, and for a single-word
    /// justified line (no gap — falls back, disclosed).
    pub justified_slack: Option<f64>,
}

/// A disclosed page-overflow condition (§3.5 / R76): the re-wrap grew the
/// block below the supplied page cropbox. Computed here; **not** applied.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PageOverflow {
    /// How far the new block box falls below the cropbox bottom, points.
    pub past_bottom_pt: f64,
    /// How many preview lines have their box below the cropbox bottom.
    pub lines_outside: usize,
    /// How far the new block box extends past the cropbox RIGHT edge, points
    /// (`0.0` when it does not).
    ///
    /// # Why this axis was missing, and why its absence looked complete
    ///
    /// Only the bottom was ever checked, which reads as thorough because a
    /// re-wrap grows DOWNWARD — vertical is the axis the operation obviously
    /// threatens. The horizontal axis is threatened by something else: the
    /// WRAP WIDTH, which is auto-detected from the block's bounding box
    /// whenever the caller does not override it.
    ///
    /// That box is not a fixed property of the paragraph. An `edit-text` whose
    /// replacement was longer than the original pushes its line past the right
    /// margin and widens the box — measured on a 612 pt page, 156 pt became
    /// 930 pt — and the next reflow then wraps faithfully to a width the
    /// operator never chose, running text off the page while reporting a
    /// successful re-wrap. Both operations are individually correct; the
    /// damage lives only in their composition (R148).
    ///
    /// Disclosed rather than clamped, matching this module's existing posture
    /// for the vertical case (decision 015 §3.5, R76): pdfcer states what it
    /// derived and never silently reshapes the operator's content. Which
    /// corrective default to adopt instead is a separate open question
    /// (Pass 33.0).
    pub past_right_pt: f64,
}

/// The derived-inference counters + disclosures for one preview — the
/// fuzzy-never-sneaky report (rule 4), mirroring [`super::BlockDiagnostics`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ReflowDiagnostics {
    /// Words the block tokenised into.
    pub words: usize,
    /// Lines before the re-wrap (the block's recognised line count).
    pub lines_before: usize,
    /// Lines after the re-wrap.
    pub lines_after: usize,
    /// Words wider than the wrap width (overflowing, unbreakable).
    pub overflowing_words: usize,
    /// Representative inter-word space width used, points.
    pub space_width_pt: f64,
    /// Whether [`Self::space_width_pt`] was estimated (no space glyph in the
    /// block) rather than measured.
    pub space_width_estimated: bool,
    /// Leading (baseline-to-baseline) used, points.
    pub leading_pt: f64,
    /// Whether [`Self::leading_pt`] was estimated (single source line)
    /// rather than measured from the block's baselines.
    pub leading_estimated: bool,
    /// Named, human-readable disclosures, de-duplicated in first-seen order
    /// — the same discipline as the block model's notes.
    pub disclosures: Vec<String>,
}

impl ReflowDiagnostics {
    /// Record a disclosure, de-duplicated by exact text.
    fn disclose(&mut self, text: String) {
        if !self.disclosures.contains(&text) {
            self.disclosures.push(text);
        }
    }
}

/// A derived within-block reflow preview (decision 015 §3.4). Nothing in it
/// has been applied — it is the accept/reject artefact 15.1 turns into one
/// undo-able `ReflowBlock` command.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ReflowPreview {
    /// The alignment used, and how it was determined.
    pub alignment: DetectedAlignment,
    /// The wrap width used (the block box width unless overridden), points.
    pub wrap_width: f64,
    /// The leading used, points.
    pub leading: f64,
    /// The new lines, top-to-bottom.
    pub lines: Vec<ReflowLine>,
    /// The new block box in default user space (top-anchored; width =
    /// [`Self::wrap_width`]; height reflects the new line count).
    pub new_bbox: Rect,
    /// The block box before the re-wrap (for the height-change delta).
    pub old_bbox: Rect,
    /// Line count before the re-wrap.
    pub lines_before: usize,
    /// Line count after the re-wrap.
    pub lines_after: usize,
    /// A disclosed page-overflow condition, if a cropbox was supplied and
    /// the new box exceeds it (§3.5). `None` otherwise.
    pub overflow: Option<PageOverflow>,
    /// The derived-inference report.
    pub diagnostics: ReflowDiagnostics,
}

impl ReflowPreview {
    /// The signed change in block box height (new − old), points. Positive
    /// = the block grew taller (more lines / larger leading).
    #[must_use]
    pub fn height_delta(&self) -> f64 {
        self.new_bbox.height() - self.old_bbox.height()
    }
}

/// Operator-adjustable inputs to a preview (decision 015 §3.4): each
/// `None` field takes its default (block box width / auto-detected
/// alignment / measured leading). `page_cropbox` enables the overflow
/// disclosure (§3.5) — supply the page's cropbox to have overflow computed.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct ReflowRequest {
    /// Wrap width override; default = the block box width.
    pub wrap_width: Option<f64>,
    /// Alignment override; default = auto-detected (§3.6).
    pub alignment: Option<BlockAlignment>,
    /// Leading override; default = the block's measured baseline gap.
    pub leading: Option<f64>,
    /// The page cropbox, for overflow disclosure (§3.5). `None` = no
    /// overflow check.
    pub page_cropbox: Option<Rect>,
}

impl ReflowRequest {
    /// A request that takes every default (block box width, auto-detected
    /// alignment, measured leading, no overflow check).
    ///
    /// The struct is `#[non_exhaustive]`, so callers outside `pdfcer-core`
    /// build one through these chainable setters rather than a struct
    /// literal — which keeps adding a future input a non-breaking change.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::text_edit::{BlockAlignment, ReflowRequest};
    ///
    /// let req = ReflowRequest::new()
    ///     .with_wrap_width(180.0)
    ///     .with_alignment(BlockAlignment::Justified);
    /// assert_eq!(req.wrap_width, Some(180.0));
    /// assert_eq!(req.alignment, Some(BlockAlignment::Justified));
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the wrap width (points).
    #[must_use]
    pub const fn with_wrap_width(mut self, width: f64) -> Self {
        self.wrap_width = Some(width);
        self
    }

    /// Override the alignment (else it is auto-detected).
    #[must_use]
    pub const fn with_alignment(mut self, alignment: BlockAlignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Override the leading (points).
    #[must_use]
    pub const fn with_leading(mut self, leading: f64) -> Self {
        self.leading = Some(leading);
        self
    }

    /// Supply the page cropbox so page-bottom overflow is disclosed (§3.5).
    #[must_use]
    pub const fn with_page_cropbox(mut self, cropbox: Rect) -> Self {
        self.page_cropbox = Some(cropbox);
        self
    }

    /// Set the wrap width from an optional value (a no-op when `None`) — the
    /// shape a CLI/GUI with optional flags wants.
    #[must_use]
    pub const fn with_wrap_width_opt(mut self, width: Option<f64>) -> Self {
        self.wrap_width = width;
        self
    }

    /// Set the alignment from an optional override (a no-op when `None`).
    #[must_use]
    pub const fn with_alignment_opt(mut self, alignment: Option<BlockAlignment>) -> Self {
        self.alignment = alignment;
        self
    }

    /// Set the leading from an optional value (a no-op when `None`).
    #[must_use]
    pub const fn with_leading_opt(mut self, leading: Option<f64>) -> Self {
        self.leading = leading;
        self
    }
}

/// Why a preview could not be computed. Every variant names a condition the
/// caller can act on (C-GOOD-ERR / the crate's `thiserror` discipline).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum ReflowError {
    /// The block index is not in the model's [`EditableTextModel::blocks`].
    #[error("block index {0} is out of range ({1} block(s) recognised)")]
    BlockIndexOutOfRange(usize, usize),
    /// The block tokenised to zero words — it carries no measurable glyph
    /// (e.g. it is entirely `/ActualText` or whitespace). Nothing to wrap.
    #[error("block {0} has no measurable words to reflow")]
    EmptyBlock(usize),
    /// The requested wrap width is not a usable positive number.
    #[error("wrap width {0} is not a positive, finite width")]
    BadWidth(f64),
}

/// The within-block reflow engine (decision 015 §6, slice 15.0). Borrows a
/// recognised [`EditableTextModel`] immutably and computes read-only
/// previews over its blocks. Holds no mutable state and writes nothing.
#[derive(Debug, Clone, Copy)]
pub struct ReflowEngine<'m, 'a> {
    model: &'m EditableTextModel<'a>,
}

/// A word tokenised out of a block: the glyphs it spans and their total
/// §9.4.4 advance width, plus the decoded text (for the preview display)
/// and — for Pass 15.1's reflow-apply — the SOURCE character codes.
///
/// `pub(crate)` so [`super::reflow_apply`] (Pass 15.1) re-emits the block's
/// lines through the SAME tokenisation the 15.0 preview computed its word
/// ranges over, guaranteeing the [`ReflowLine::words`] index ranges line up
/// with the codes to show. Kept private-in-crate — never public API.
pub(crate) struct WordTok {
    pub(crate) width: f64,
    pub(crate) text: String,
    /// The word's source character codes, in content order (one byte per
    /// glyph — simple font; composite is refused before apply, R-INV-4).
    /// Carried so 15.1 re-emits the SAME bytes rather than re-encoding: a
    /// reflow only re-wraps and re-positions, it never changes the text
    /// (decision 015 §3.7, minimal-diff), so the original codes are exactly
    /// the codes to show.
    pub(crate) codes: Vec<u8>,
}

impl<'m, 'a> ReflowEngine<'m, 'a> {
    /// Build an engine over a recognised model.
    #[must_use]
    pub const fn new(model: &'m EditableTextModel<'a>) -> Self {
        Self { model }
    }

    /// Auto-detect the alignment of block `block_index` from its glyph
    /// x-positions (§3.6), without computing a full preview.
    ///
    /// # Errors
    ///
    /// [`ReflowError::BlockIndexOutOfRange`] if the index is invalid.
    pub fn detect_alignment(&self, block_index: usize) -> Result<DetectedAlignment, ReflowError> {
        let block = self.block(block_index)?;
        Ok(self.infer_alignment(block))
    }

    /// Compute a read-only reflow preview for block `block_index`
    /// (decision 015 §6, 15.0). Nothing is written.
    ///
    /// The steps: tokenise the block's glyphs into words measured by their
    /// §9.4.4 advances; pick the wrap width (`req` override or the block box
    /// width); auto-detect (or take the overridden) alignment; greedily
    /// re-break the words via the shared [`crate::linebreak::greedy_pack`];
    /// place each line by the alignment (recording justified slack for full
    /// lines); grow the block top-anchored by the leading; and, if a cropbox
    /// was supplied, disclose any page-bottom overflow. Every derived choice
    /// is counted in [`ReflowDiagnostics`].
    ///
    /// # Errors
    ///
    /// - [`ReflowError::BlockIndexOutOfRange`] — invalid index.
    /// - [`ReflowError::EmptyBlock`] — the block has no measurable words.
    /// - [`ReflowError::BadWidth`] — the wrap width is not positive/finite.
    pub fn preview(
        &self,
        block_index: usize,
        req: &ReflowRequest,
    ) -> Result<ReflowPreview, ReflowError> {
        let block = self.block(block_index)?;
        let old_bbox = block.bbox;

        // Representative effective size: the largest line size in the block
        // (the yardstick the model itself uses for its thresholds).
        let size = block
            .line_indices
            .iter()
            .filter_map(|&li| self.model.lines().get(li))
            .map(|l| f64::from(l.size))
            .fold(0.0_f64, f64::max)
            .max(1.0);

        // Tokenise words + gather inter-word space-glyph advances. The
        // space code is unused by the READ-ONLY preview (15.1's apply reads
        // it); discarded here with `_`.
        let page = self.model.sourced_view();
        let (words, space_samples, _space_code) = tokenise_block(self.model, page, block);
        if words.is_empty() {
            return Err(ReflowError::EmptyBlock(block_index));
        }

        // Representative space width: median of the block's own spaces, or a
        // disclosed 0.25·size estimate when the block has no space glyph.
        let mut diagnostics = ReflowDiagnostics {
            words: words.len(),
            lines_before: block.line_indices.len(),
            lines_after: 0,
            overflowing_words: 0,
            space_width_pt: 0.0,
            space_width_estimated: false,
            leading_pt: 0.0,
            leading_estimated: false,
            disclosures: Vec::new(),
        };
        let (space_width, space_estimated) = match median(space_samples) {
            Some(w) => (w, false),
            None => (FALLBACK_SPACE_FRAC * size, true),
        };
        diagnostics.space_width_pt = space_width;
        diagnostics.space_width_estimated = space_estimated;

        // Wrap width: override or the block box width.
        let wrap_width = req.wrap_width.unwrap_or_else(|| old_bbox.width());
        if !(wrap_width.is_finite() && wrap_width > 0.0) {
            return Err(ReflowError::BadWidth(wrap_width));
        }

        // Leading: override or the block's median baseline gap (fallback
        // 1.2·size when a single source line offers no gap to measure).
        let (leading, leading_estimated) = match req.leading {
            Some(l) if l.is_finite() && l > 0.0 => (l, false),
            _ => {
                let gaps = self.baseline_gaps(block);
                match median(gaps) {
                    Some(g) if g > 0.0 => (g, false),
                    _ => (FALLBACK_LEADING_FRAC * size, true),
                }
            }
        };
        diagnostics.leading_pt = leading;
        diagnostics.leading_estimated = leading_estimated;

        // Alignment: override (marked Overridden) or auto-detected.
        let alignment = match req.alignment {
            Some(a) => DetectedAlignment {
                alignment: a,
                source: AlignmentSource::Overridden,
                ..self.infer_alignment(block)
            },
            None => self.infer_alignment(block),
        };

        // Greedy re-break over the word advances (+ representative spaces),
        // through the ONE shared breaker (decision 015 §3.2).
        let widths: Vec<f64> = words.iter().map(|w| w.width).collect();
        let ranges = crate::linebreak::greedy_pack(widths.len(), wrap_width, |s, e| {
            line_natural_width(&widths, space_width, s, e)
        });

        // Top-anchored: first baseline fixed at the block's top baseline.
        let first_baseline = block
            .line_indices
            .iter()
            .filter_map(|&li| self.model.lines().get(li))
            .map(|l| f64::from(l.baseline_y))
            .fold(f64::NEG_INFINITY, f64::max);
        let first_baseline = if first_baseline.is_finite() {
            first_baseline
        } else {
            old_bbox.ury - ASCENT_FRAC * size
        };

        let block_llx = old_bbox.llx;
        let ascent = ASCENT_FRAC * size;
        let descent = DESCENT_FRAC * size;

        let line_count = ranges.len();
        let mut lines: Vec<ReflowLine> = Vec::with_capacity(line_count);
        let mut overflowing_words = 0usize;
        for (i, r) in ranges.into_iter().enumerate() {
            let word_count = r.end.saturating_sub(r.start);
            let natural_width = line_natural_width(&widths, space_width, r.start, r.end);
            let text = join_word_text(&words, r.clone());
            let gap_count = word_count.saturating_sub(1);
            let is_overflowing_word = word_count == 1 && natural_width > wrap_width + EPS;
            if is_overflowing_word {
                overflowing_words += 1;
            }
            let baseline_y = first_baseline - leading * (i as f64);
            let is_last = i + 1 == line_count;
            let origin_x =
                align_origin_x(alignment.alignment, block_llx, wrap_width, natural_width);
            // Justified slack: full (non-last) multi-word lines only (§3.1).
            let justified_slack = if alignment.alignment.is_justified()
                && !is_last
                && gap_count >= 1
                && !is_overflowing_word
            {
                Some((wrap_width - natural_width).max(0.0))
            } else {
                None
            };
            if alignment.alignment.is_justified() && !is_last && gap_count == 0 {
                diagnostics.disclose(
                    "reflow: a justified line has a single word (no inter-word gap) — left at \
                     the base alignment, not stretched (decision 015 §3.1)"
                        .to_string(),
                );
            }
            lines.push(ReflowLine {
                words: r,
                text,
                origin_x,
                baseline_y,
                natural_width,
                gap_count,
                is_overflowing_word,
                justified_slack,
            });
        }
        diagnostics.lines_after = lines.len();
        diagnostics.overflowing_words = overflowing_words;

        // New block box: top-anchored, width = wrap_width, height from the
        // new line count and leading.
        let last_baseline = first_baseline - leading * ((line_count.saturating_sub(1)) as f64);
        let new_bbox = Rect {
            llx: block_llx,
            lly: last_baseline - descent,
            urx: block_llx + wrap_width,
            ury: first_baseline + ascent,
        };

        // Always-on derived-layout disclosure (rule 4).
        diagnostics.disclose(
            "reflow: line breaks, per-line origins and block box are DERIVED layout the file \
             never stated (ISO 32000-1 §14.8 S1-S9) — a reviewable preview; nothing is written \
             (Pass 15.0 is READ-ONLY)"
                .to_string(),
        );
        // Alignment disclosure.
        diagnostics.disclose(alignment_disclosure(&alignment));
        if leading_estimated {
            diagnostics.disclose(format!(
                "reflow: leading estimated at {leading:.2}pt (1.2 x size) — the block has a \
                 single source line, so no baseline gap was measurable"
            ));
        }
        if space_estimated {
            diagnostics.disclose(format!(
                "reflow: inter-word space width estimated at {space_width:.2}pt (0.25 x size) — \
                 the block carries no U+0020 space glyph to measure"
            ));
        }
        if overflowing_words > 0 {
            diagnostics.disclose(format!(
                "reflow: {overflowing_words} word(s) are wider than the {wrap_width:.1}pt wrap \
                 width and overflow their line unbroken — no hyphenation (decision 015 §3.2, \
                 whitespace-only breaks)"
            ));
        }

        // Page overflow on BOTH axes (§3.5 / R76): disclosed, never applied.
        //
        // The bottom check is the obvious one — a re-wrap grows downward, so
        // that is the axis the operation plainly threatens. The right edge is
        // threatened by something else: the wrap WIDTH itself. When the caller
        // does not override it, it is measured from the block's current
        // bounding box, and a prior `edit-text` whose replacement ran past the
        // margin has already widened that box. Without this check such a
        // reflow reports a successful re-wrap while putting text off the page
        // (R148).
        let overflow = req.page_cropbox.and_then(|crop| {
            let past_bottom = (crop.lly - new_bbox.lly).max(0.0);
            let past_right = (new_bbox.urx - crop.urx).max(0.0);
            // Either axis alone is an overflow. Returning early when only the
            // bottom was clear is exactly what hid the horizontal case.
            if past_bottom <= EPS && past_right <= EPS {
                return None;
            }
            let lines_outside = lines
                .iter()
                .filter(|l| l.baseline_y - descent < crop.lly - EPS)
                .count();
            if past_bottom > EPS {
                diagnostics.disclose(format!(
                    "reflow: re-wrap grows the block {past_bottom:.1}pt past the page bottom \
                     (cropbox); {lines_outside} line(s) fall outside the visible page — \
                     DISCLOSED, not applied (decision 015 §3.5, R76)"
                ));
            }
            if past_right > EPS {
                diagnostics.disclose(format!(
                    "reflow: the wrap width puts the block {past_right:.1}pt past the page right \
                     edge (cropbox), so the re-wrapped text runs off the page. That width was \
                     measured from the block's own box, which an earlier edit may have widened \
                     past the margin — pass an explicit width to wrap to the original margin \
                     — DISCLOSED, not applied (R148, R76)"
                ));
            }
            Some(PageOverflow {
                past_bottom_pt: past_bottom,
                lines_outside,
                past_right_pt: past_right,
            })
        });

        Ok(ReflowPreview {
            alignment,
            wrap_width,
            leading,
            lines,
            new_bbox,
            old_bbox,
            lines_before: block.line_indices.len(),
            lines_after: line_count,
            overflow,
            diagnostics,
        })
    }

    // -- internals --------------------------------------------------------

    /// Fetch a block by index or name the range error.
    fn block(&self, block_index: usize) -> Result<&'m Block, ReflowError> {
        self.model.blocks().get(block_index).ok_or_else(|| {
            ReflowError::BlockIndexOutOfRange(block_index, self.model.blocks().len())
        })
    }

    /// The block's consecutive baseline gaps (top-to-bottom), for the
    /// leading estimate.
    fn baseline_gaps(&self, block: &Block) -> Vec<f64> {
        let baselines: Vec<f64> = block
            .line_indices
            .iter()
            .filter_map(|&li| self.model.lines().get(li))
            .map(|l| f64::from(l.baseline_y))
            .collect();
        baselines
            .windows(2)
            .filter_map(|w| match w {
                [a, b] => {
                    let gap = a - b;
                    (gap > 0.0).then_some(gap)
                }
                _ => None,
            })
            .collect()
    }

    /// Infer the block's alignment from its lines' x-geometry (§3.6).
    fn infer_alignment(&self, block: &Block) -> DetectedAlignment {
        // Per-line (left, right) edges, in block order (top-to-bottom).
        let edges: Vec<(f64, f64)> = block
            .line_indices
            .iter()
            .filter_map(|&li| self.model.lines().get(li))
            .map(|l| (l.bbox.llx, l.bbox.urx))
            .collect();

        let size = block
            .line_indices
            .iter()
            .filter_map(|&li| self.model.lines().get(li))
            .map(|l| f64::from(l.size))
            .fold(0.0_f64, f64::max)
            .max(1.0);
        let tol = (ALIGN_TOL_FRAC * size).max(ALIGN_TOL_MIN);

        let n = edges.len();
        let lefts: Vec<f64> = edges.iter().map(|&(l, _)| l).collect();
        let rights: Vec<f64> = edges.iter().map(|&(_, r)| r).collect();
        let mids: Vec<f64> = edges.iter().map(|&(l, r)| (l + r) / 2.0).collect();

        let left_ragged = range_of(&lefts);
        let right_ragged = range_of(&rights);
        let mid_ragged = range_of(&mids);

        // A single line cannot be classified — default Left, disclosed.
        if n <= 1 {
            return DetectedAlignment {
                alignment: BlockAlignment::Left,
                source: AlignmentSource::SingleLineDefault,
                left_ragged_pt: left_ragged,
                right_ragged_pt: right_ragged,
                mid_ragged_pt: mid_ragged,
                tolerance_pt: tol,
            };
        }

        let block_urx = rights.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        // Body = all lines but the visually last (bottom) one.
        let body_rights = rights.get(..n.saturating_sub(1)).unwrap_or(&[]);
        let last_right = rights.last().copied().unwrap_or(block_urx);

        let left_flush = left_ragged <= tol;
        let right_flush_all = right_ragged <= tol;
        let right_flush_body = range_of(body_rights) <= tol;
        let center_flush = mid_ragged <= tol;
        let last_short = last_right < block_urx - tol;

        let alignment = if n >= JUSTIFY_MIN_LINES && left_flush && right_flush_body && last_short {
            BlockAlignment::Justified
        } else if left_flush && !right_flush_all {
            BlockAlignment::Left
        } else if right_flush_all && !left_flush {
            BlockAlignment::Right
        } else if center_flush && !left_flush && !right_flush_all {
            BlockAlignment::Center
        } else {
            // No clear signal (e.g. a 2-line block flush both margins).
            return DetectedAlignment {
                alignment: BlockAlignment::Left,
                source: AlignmentSource::AmbiguousDefault,
                left_ragged_pt: left_ragged,
                right_ragged_pt: right_ragged,
                mid_ragged_pt: mid_ragged,
                tolerance_pt: tol,
            };
        };

        DetectedAlignment {
            alignment,
            source: AlignmentSource::Detected,
            left_ragged_pt: left_ragged,
            right_ragged_pt: right_ragged,
            mid_ragged_pt: mid_ragged,
            tolerance_pt: tol,
        }
    }
}

/// The x origin (left edge of the shown text) for a line under `alignment`
/// within the wrap box `[block_llx, block_llx + wrap_width]` (§12.7.3.3
/// quadding, extended with Justified).
///
/// Justified full lines and the justified *last* line (never stretched —
/// §3.1) both sit at the left margin, so the origin is `block_llx` for both
/// Left and Justified; only Right and Centre shift the text in from the left
/// edge. The justified stretch itself is not an origin shift — it is the
/// per-gap `TJ`/`Tw` slack in [`ReflowLine::justified_slack`], applied by
/// 15.1, so the left origin is correct for the preview.
///
/// `pub(crate)` so Pass 16.1's boxed add-new-text
/// ([`super::addtext`]) places its freshly wrapped lines with the EXACT same
/// alignment arithmetic the 15.x reflow preview uses — one origin formula, not
/// two that can drift (decision 016 §6 slice 16.1: "reuse the 15.1 recipe").
pub(crate) fn align_origin_x(
    alignment: BlockAlignment,
    block_llx: f64,
    wrap_width: f64,
    natural_width: f64,
) -> f64 {
    match alignment {
        BlockAlignment::Left | BlockAlignment::Justified => block_llx,
        BlockAlignment::Right => block_llx + (wrap_width - natural_width),
        BlockAlignment::Center => block_llx + (wrap_width - natural_width) / 2.0,
    }
}

/// Natural width of the line `words[start..end]` joined by single spaces:
/// Σ word advances + `space_width` per inter-word gap (§9.4.4). Used both by
/// the greedy breaker's fit test and the per-line placement.
///
/// `pub(crate)` so Pass 16.1's boxed add-new-text ([`super::addtext`]) feeds
/// the shared [`crate::linebreak::greedy_pack`] the identical "natural line
/// width" closure the 15.x reflow engine uses — the wrap decisions of a fresh
/// boxed run and a re-wrapped existing block are then computed by one
/// measurement function, never two that can drift.
pub(crate) fn line_natural_width(
    widths: &[f64],
    space_width: f64,
    start: usize,
    end: usize,
) -> f64 {
    let slice = widths.get(start..end).unwrap_or(&[]);
    let words: f64 = slice.iter().sum();
    let gaps = slice.len().saturating_sub(1) as f64;
    words + space_width * gaps
}

/// Join the text of `words[range]` with single spaces — the DERIVED display
/// text for a preview line.
fn join_word_text(words: &[WordTok], range: Range<usize>) -> String {
    let slice = words.get(range).unwrap_or(&[]);
    let mut out = String::new();
    for (i, w) in slice.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&w.text);
    }
    out
}

/// Tokenise a block's glyphs into words (split at U+0020 space glyphs and at
/// line boundaries), returning the words (each carrying its source codes for
/// 15.1 re-emission), the block's space-glyph advances (for the
/// representative space width), and the representative inter-word space's
/// character code (for 15.1 to re-emit the gap byte). See the module docs.
///
/// `pub(crate)` so Pass 15.1's [`super::reflow_apply`] tokenises identically
/// to the 15.0 preview and lines up [`ReflowLine::words`] ranges with the
/// codes it emits.
pub(crate) fn tokenise_block(
    model: &EditableTextModel<'_>,
    page: &PageText,
    block: &Block,
) -> (Vec<WordTok>, Vec<f64>, Option<u8>) {
    let mut words: Vec<WordTok> = Vec::new();
    let mut spaces: Vec<f64> = Vec::new();
    let mut space_code: Option<u8> = None;
    let mut current: Option<WordTok> = None;

    for &li in &block.line_indices {
        let Some(line) = model.lines().get(li) else {
            continue;
        };
        for &gref in &line.glyphs {
            let Some(g) = model.glyph(gref) else { continue };
            let text = glyph_text(page, gref);
            if text == " " {
                // Close the current word; sample the space advance + code.
                if let Some(w) = current.take() {
                    words.push(w);
                }
                spaces.push(f64::from(g.advance));
                if space_code.is_none() {
                    space_code = u8::try_from(g.code).ok();
                }
            } else {
                let w = current.get_or_insert_with(|| WordTok {
                    width: 0.0,
                    text: String::new(),
                    codes: Vec::new(),
                });
                w.width += f64::from(g.advance);
                w.text.push_str(text);
                // Carry the source code so 15.1 re-emits the SAME byte (one
                // byte per glyph — simple font). A code outside 0..=255 can
                // only be composite, refused before apply reaches here.
                if let Ok(b) = u8::try_from(g.code) {
                    w.codes.push(b);
                }
            }
        }
        // A line boundary is a word break (the old wrapping is discarded).
        if let Some(w) = current.take() {
            words.push(w);
        }
    }
    (words, spaces, space_code)
}

/// The decoded text of one glyph — its slice of its run's text — or `""` if
/// the reference is stale. The returned slice borrows `page`.
fn glyph_text(page: &PageText, gref: GlyphRef) -> &str {
    page.runs
        .get(gref.run)
        .and_then(|run| {
            run.glyphs.get(gref.glyph).and_then(|g| {
                let start = g.text_start as usize;
                let end = start + g.text_len as usize;
                run.text.get(start..end)
            })
        })
        .unwrap_or("")
}

/// A human-readable disclosure describing how the alignment was chosen.
fn alignment_disclosure(a: &DetectedAlignment) -> String {
    match a.source {
        AlignmentSource::Detected => format!(
            "reflow: alignment auto-detected as {} from glyph x-positions \
             (left-ragged={:.1}pt right-ragged={:.1}pt mid-ragged={:.1}pt tol={:.1}pt) — \
             preserved by default, overridable (decision 015 §3.6, R77)",
            a.alignment.as_str(),
            a.left_ragged_pt,
            a.right_ragged_pt,
            a.mid_ragged_pt,
            a.tolerance_pt,
        ),
        AlignmentSource::SingleLineDefault => {
            "reflow: single-line block — alignment inferred as left (ambiguous), overridable \
             (decision 015 §3.6)"
                .to_string()
        }
        AlignmentSource::AmbiguousDefault => format!(
            "reflow: no clear alignment signal (left-ragged={:.1}pt right-ragged={:.1}pt \
             mid-ragged={:.1}pt tol={:.1}pt) — defaulted to left, overridable (decision 015 §3.6)",
            a.left_ragged_pt, a.right_ragged_pt, a.mid_ragged_pt, a.tolerance_pt,
        ),
        AlignmentSource::Overridden => format!(
            "reflow: alignment set to {} by operator override (decision 015 §3.4)",
            a.alignment.as_str()
        ),
    }
}

/// The spread (max − min) of a slice of measurements, points. `0.0` for an
/// empty or single-element slice (nothing can be ragged).
fn range_of(xs: &[f64]) -> f64 {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &x in xs {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    if hi >= lo { hi - lo } else { 0.0 }
}

/// The median of `values` (consumes + sorts). `None` for an empty input.
fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values.get(mid).copied()
    } else {
        let lo = values.get(mid.wrapping_sub(1)).copied()?;
        let hi = values.get(mid).copied()?;
        Some((lo + hi) / 2.0)
    }
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
    use crate::text_extract::{ExtractedGlyph, LadderRung, PageText, TextOrigin, TextRun};

    fn line_break() -> TextRun {
        TextRun {
            text: "\n".to_string(),
            origin: TextOrigin::DerivedLineBreak,
            glyphs: Vec::new(),
            artifact: None,
            mcid: None,
            bbox: None,
        }
    }

    fn page(runs: Vec<TextRun>) -> PageText {
        let mut p = PageText::default();
        p.runs = runs;
        p
    }

    /// A mono multi-word line: words separated by a single 6-wide space
    /// glyph, at absolute origin `x`, baseline `y`. Every glyph 6/6 at
    /// size 10.
    fn text_line(text: &str, x: f32, y: f32) -> TextRun {
        let mut run = TextRun {
            text: String::new(),
            origin: TextOrigin::Glyphs,
            glyphs: Vec::new(),
            artifact: None,
            mcid: None,
            bbox: None,
        };
        let mut cur_x = x;
        for ch in text.chars() {
            let start = run.text.len() as u32;
            run.text.push(ch);
            let len = run.text.len() as u32 - start;
            run.glyphs.push(ExtractedGlyph {
                code: 0,
                rung: LadderRung::ToUnicode,
                text_start: start,
                text_len: len,
                x: cur_x,
                y,
                advance: 6.0,
                size: 10.0,
                direction: (1.0, 0.0),
                invisible: false,
                provenance: None,
            });
            cur_x += 6.0;
        }
        run
    }

    fn recognise(p: &PageText) -> EditableTextModel<'_> {
        EditableTextModel::recognize(p, &super::super::BlockRecognitionOptions::default())
    }

    /// Recognise with **first-line-indent paragraph splitting relaxed**.
    ///
    /// A right- or centre-aligned paragraph has, by definition, ragged left
    /// edges, which the 14.0 recogniser's indent rule reads as new-paragraph
    /// indents — fragmenting the paragraph into single-line blocks. Reflow
    /// wants the WHOLE paragraph, so it recognises with the indent threshold
    /// pushed out of reach (paragraphs still split on leading gaps). Delegates
    /// to the ONE public [`reflow_recognition_options`] every reflow consumer
    /// (CLI, GUI, apply path) shares, so the tests exercise the exact config
    /// production uses.
    fn recognise_relaxed(p: &PageText) -> EditableTextModel<'_> {
        EditableTextModel::recognize(p, &reflow_recognition_options())
    }

    // -- greedy re-wrap matches hand-computed breaks --------------------

    #[test]
    fn greedy_rewrap_matches_hand_computed_breaks() {
        // One block, three source lines, six words "aa" (each 2 glyphs =
        // 12pt wide), spaces 6pt. On one line: word=12, gap=6.
        // At wrap_width=30: "aa aa" = 12+6+12 = 30 fits; adding a third
        // "aa" = 30+6+12 = 48 > 30 -> break. So 2 words per line -> 3 lines.
        let runs = vec![
            text_line("aa aa", 72.0, 740.0),
            line_break(),
            text_line("aa aa", 72.0, 726.0),
            line_break(),
            text_line("aa aa", 72.0, 712.0),
        ];
        let p = page(runs);
        let m = recognise(&p);
        assert_eq!(m.blocks().len(), 1);
        let eng = ReflowEngine::new(&m);
        let pv = eng
            .preview(
                0,
                &ReflowRequest {
                    wrap_width: Some(30.0),
                    ..Default::default()
                },
            )
            .unwrap();
        // Six words "aa" -> pairs per line -> 3 lines of two words.
        assert_eq!(pv.diagnostics.words, 6);
        assert_eq!(pv.lines_after, 3);
        for l in &pv.lines {
            assert_eq!(l.gap_count, 1, "two words per line: {:?}", l.text);
            assert!((l.natural_width - 30.0).abs() < 1e-6, "{}", l.natural_width);
        }
        // Narrower: wrap_width=12 forces one word per line -> 6 lines.
        let pv2 = eng
            .preview(
                0,
                &ReflowRequest {
                    wrap_width: Some(12.0),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(pv2.lines_after, 6);
    }

    // -- alignment auto-detect ------------------------------------------

    fn aligned_block(lines: &[(&str, f32)], y0: f32) -> PageText {
        // Each entry: (text, x-origin); baselines step down 14pt.
        let mut runs: Vec<TextRun> = Vec::new();
        for (i, &(t, x)) in lines.iter().enumerate() {
            if i > 0 {
                runs.push(line_break());
            }
            runs.push(text_line(t, x, y0 - 14.0 * i as f32));
        }
        page(runs)
    }

    #[test]
    fn detects_left_alignment() {
        // All left edges at x=72; right edges ragged (different lengths).
        let p = aligned_block(
            &[("aaaa aaaa", 72.0), ("aa", 72.0), ("aaaa aa", 72.0)],
            740.0,
        );
        let m = recognise(&p);
        let eng = ReflowEngine::new(&m);
        let d = eng.detect_alignment(0).unwrap();
        assert_eq!(d.alignment, BlockAlignment::Left);
        assert_eq!(d.source, AlignmentSource::Detected);
    }

    #[test]
    fn detects_right_alignment() {
        // Right edges flush at 200; left edges ragged.
        // width = 6*len. Right edge = x + 6*len; set x so all end at 200.
        let mk = |t: &str| -> (String, f32) {
            let w = 6.0 * t.chars().count() as f32;
            (t.to_string(), 200.0 - w)
        };
        let a = mk("aaaa aaaa");
        let b = mk("aa");
        let c = mk("aaaa aa");
        let p = aligned_block(&[(&a.0, a.1), (&b.0, b.1), (&c.0, c.1)], 740.0);
        // Ragged left edges would fragment into single-line blocks under the
        // default indent rule — reflow recognises with indent splitting
        // relaxed so the paragraph stays whole (see `recognise_relaxed`).
        let m = recognise_relaxed(&p);
        let eng = ReflowEngine::new(&m);
        let d = eng.detect_alignment(0).unwrap();
        assert_eq!(d.alignment, BlockAlignment::Right, "ragged={:?}", d);
    }

    #[test]
    fn detects_center_alignment() {
        // Midpoints flush at 150; both edges ragged.
        let mk = |t: &str| -> (String, f32) {
            let w = 6.0 * t.chars().count() as f32;
            (t.to_string(), 150.0 - w / 2.0)
        };
        let a = mk("aaaa aaaa");
        let b = mk("aa");
        let c = mk("aaaaaa aa");
        let p = aligned_block(&[(&a.0, a.1), (&b.0, b.1), (&c.0, c.1)], 740.0);
        let m = recognise_relaxed(&p);
        let eng = ReflowEngine::new(&m);
        let d = eng.detect_alignment(0).unwrap();
        assert_eq!(d.alignment, BlockAlignment::Center, "ragged={:?}", d);
    }

    #[test]
    fn detects_justified_alignment() {
        // Three body lines flush BOTH margins (x=72, right=72+240=312 -> 40
        // chars each), last line short.
        let body = "a".repeat(40); // 40 glyphs -> right edge 72+240=312
        let last = "aa short"; // short last line
        let p = aligned_block(
            &[(&body, 72.0), (&body, 72.0), (&body, 72.0), (last, 72.0)],
            740.0,
        );
        let m = recognise(&p);
        let eng = ReflowEngine::new(&m);
        let d = eng.detect_alignment(0).unwrap();
        assert_eq!(d.alignment, BlockAlignment::Justified, "ragged={:?}", d);
    }

    #[test]
    fn single_line_defaults_to_left_disclosed() {
        let p = page(vec![text_line("hello world", 72.0, 740.0)]);
        let m = recognise(&p);
        let eng = ReflowEngine::new(&m);
        let d = eng.detect_alignment(0).unwrap();
        assert_eq!(d.alignment, BlockAlignment::Left);
        assert_eq!(d.source, AlignmentSource::SingleLineDefault);
        // And a preview discloses the ambiguity.
        let pv = eng.preview(0, &ReflowRequest::default()).unwrap();
        assert!(
            pv.diagnostics
                .disclosures
                .iter()
                .any(|d| d.contains("single-line block")),
            "{:?}",
            pv.diagnostics.disclosures
        );
    }

    // -- oversized word overflows one line + disclosure -----------------

    #[test]
    fn oversized_word_is_one_overflowing_line_disclosed() {
        // A 10-glyph word (60pt) with wrap_width 30 -> overflows alone.
        let p = page(vec![text_line("aaaaaaaaaa", 72.0, 740.0)]);
        let m = recognise(&p);
        let eng = ReflowEngine::new(&m);
        let pv = eng
            .preview(
                0,
                &ReflowRequest {
                    wrap_width: Some(30.0),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(pv.lines_after, 1);
        assert!(pv.lines[0].is_overflowing_word);
        assert_eq!(pv.diagnostics.overflowing_words, 1);
        assert!(
            pv.diagnostics
                .disclosures
                .iter()
                .any(|d| d.contains("wider than")),
            "{:?}",
            pv.diagnostics.disclosures
        );
    }

    // -- justified slack (last line un-justified) -----------------------

    #[test]
    fn justified_preview_distributes_slack_but_not_the_last_line() {
        // Two words per source line, three lines; force justified override.
        // Each "aa" word = 12, space 6. Wrap 60: "aa aa" natural=30, so a
        // full line could hold up to wrap; slack = 60-30 = 30 on non-last.
        let runs = vec![
            text_line("aa aa", 72.0, 740.0),
            line_break(),
            text_line("aa aa", 72.0, 726.0),
            line_break(),
            text_line("aa", 72.0, 712.0),
        ];
        let p = page(runs);
        let m = recognise(&p);
        let eng = ReflowEngine::new(&m);
        // wrap 30 -> "aa aa"=30 per line for the two full pairs, then "aa".
        let pv = eng
            .preview(
                0,
                &ReflowRequest {
                    wrap_width: Some(30.0),
                    alignment: Some(BlockAlignment::Justified),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(pv.alignment.source, AlignmentSource::Overridden);
        let last = pv.lines.len() - 1;
        for (i, l) in pv.lines.iter().enumerate() {
            if i == last {
                assert!(l.justified_slack.is_none(), "last line not stretched");
            } else if l.gap_count >= 1 {
                assert!(l.justified_slack.is_some(), "full line gets slack");
            }
        }
    }

    // -- page-overflow disclosed (computed, not applied) ----------------

    #[test]
    fn narrowed_width_overflows_page_and_is_disclosed() {
        // A block near the page bottom; narrowing forces many lines that
        // grow downward past a small cropbox bottom.
        let runs = vec![
            text_line("aa aa aa aa", 20.0, 60.0),
            line_break(),
            text_line("aa aa aa aa", 20.0, 46.0),
        ];
        let p = page(runs);
        let m = recognise(&p);
        let eng = ReflowEngine::new(&m);
        // Small page cropbox 0..100 wide, 0..70 tall. Narrow wrap forces
        // one word per line (8 words -> 8 lines * 14 leading -> grows well
        // below y=0).
        let crop = Rect::from_corners(0.0, 0.0, 100.0, 70.0);
        let pv = eng
            .preview(
                0,
                &ReflowRequest {
                    wrap_width: Some(12.0),
                    page_cropbox: Some(crop),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(pv.lines_after >= 8, "narrow wrap makes many lines");
        let ov = pv.overflow.expect("overflow computed");
        assert!(ov.past_bottom_pt > 0.0);
        assert!(ov.lines_outside > 0);
        assert!(
            pv.diagnostics
                .disclosures
                .iter()
                .any(|d| d.contains("past the page bottom")),
            "{:?}",
            pv.diagnostics.disclosures
        );
    }

    // -- placement: right/center origins ---------------------------------

    #[test]
    fn right_alignment_places_text_flush_right() {
        let p = page(vec![text_line("aa aa", 72.0, 740.0)]);
        let m = recognise(&p);
        let eng = ReflowEngine::new(&m);
        // Block width is 30 (5 glyphs*6). Right align in a 100pt box.
        let pv = eng
            .preview(
                0,
                &ReflowRequest {
                    wrap_width: Some(100.0),
                    alignment: Some(BlockAlignment::Right),
                    ..Default::default()
                },
            )
            .unwrap();
        // origin_x = llx + (100 - natural). llx=72, natural=30 -> 142.
        assert!(
            (pv.lines[0].origin_x - 142.0).abs() < 1e-6,
            "{:?}",
            pv.lines[0]
        );
    }

    #[test]
    fn error_on_bad_index_and_width() {
        let p = page(vec![text_line("aa", 72.0, 740.0)]);
        let m = recognise(&p);
        let eng = ReflowEngine::new(&m);
        assert!(matches!(
            eng.preview(9, &ReflowRequest::default()),
            Err(ReflowError::BlockIndexOutOfRange(9, 1))
        ));
        assert!(matches!(
            eng.preview(
                0,
                &ReflowRequest {
                    wrap_width: Some(-5.0),
                    ..Default::default()
                }
            ),
            Err(ReflowError::BadWidth(_))
        ));
    }

    // -- the hoisted one-source-of-truth recognition config ---------------

    #[test]
    fn reflow_recognition_options_disables_indent_split_and_keeps_ragged_left_whole() {
        // The public config pushes the first-line-indent threshold out of
        // reach (so ragged-left right/centre/justified paragraphs are not
        // fragmented), while leaving the rest at defaults.
        let o = reflow_recognition_options();
        let d = super::super::BlockRecognitionOptions::default();
        assert!(
            o.indent_ratio >= 1.0e6,
            "indent split disabled: {}",
            o.indent_ratio
        );
        assert!(d.indent_ratio < 1.0e6, "default still splits on indent");
        // A right-aligned 3-line paragraph (ragged left) stays ONE block under
        // this config and detects Right — the whole point of the hoist.
        let mk = |t: &str| -> (String, f32) {
            let w = 6.0 * t.chars().count() as f32;
            (t.to_string(), 200.0 - w)
        };
        let a = mk("aaaa aaaa");
        let b = mk("aa");
        let c = mk("aaaa aa");
        let p = aligned_block(&[(&a.0, a.1), (&b.0, b.1), (&c.0, c.1)], 740.0);
        let m = EditableTextModel::recognize(&p, &reflow_recognition_options());
        assert_eq!(m.blocks().len(), 1, "ragged-left paragraph stays whole");
        let d = ReflowEngine::new(&m).detect_alignment(0).unwrap();
        assert_eq!(d.alignment, BlockAlignment::Right);
        assert_eq!(d.source, AlignmentSource::Detected);
    }
}

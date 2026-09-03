//! # The editable text model — a second, derived clustering pass
//!
//! This module builds a **Run → Line → Column → Block** hierarchy on top
//! of Pass 4's already-segmented extraction output ([`PageText`]), plus a
//! hit-test and caret/selection resolver over it. It is the read-only
//! first slice of the Acrobat-style in-place text-editing subsystem
//! (`docs/decisions/014-acrobat-text-editing.md` §3 / §5.2's 13.0 slice,
//! shipped as Pass 14.0). **Nothing here writes a byte** — it recognizes
//! structure and lets a caller navigate it; the surgery that mutates a
//! content stream is a later Pass (14.1).
//!
//! ## Everything here is DERIVED — and that is a sourced position
//!
//! An untagged PDF content stream contains **no** notion of word, line,
//! paragraph, column, or reading order. This is not a modelling shortcut;
//! it is the sourced position of ISO 32000-1 §14.8, recorded across the
//! negative results **S1–S9** that `text_extract/layout.rs` already cites
//! from the spec RAG (`iso32000__s__14.8.md`):
//!
//! - **S5** — no line or paragraph markers exist in a content stream, in a
//!   tagged document either.
//! - **S9** — for an untagged document, no definition of *word*, *line*,
//!   *paragraph*, *column* or *reading order* exists anywhere in the
//!   standard.
//!
//! An editor nevertheless *needs* those concepts: a caret walks a line, a
//! selection spans runs, a future reflow targets a block width. The only
//! honest reconciliation (decision 014 §4.1) is to **derive** the
//! structure, **count** every inference, and keep it **reviewable** — a
//! hint layer the operator accepts or corrects, never an authoritative
//! silent re-layout (rule 4, "fuzzy, never sneaky"). The sourced-only
//! truth always remains one call away: [`EditableTextModel::sourced_view`]
//! returns the untouched [`PageText`], whose
//! [`PageText::sourced_text`](crate::text_extract::PageText::sourced_text)
//! is exactly the characters the file provides.
//!
//! ## Why this reuses Pass 4 instead of re-extracting
//!
//! The block layer is a **second clustering pass over the same runs**, not
//! a second glyph walk. Pass 4's `layout.rs` already turned positioned
//! glyphs into [`TextRun`]s and inserted derived line breaks from two
//! geometry signals — a baseline move (rule 1) and a backward jump on one
//! baseline (rule 2, the two-column signal). Those breaks are re-used here
//! directly: a Pass-4 [`TextOrigin::DerivedLineBreak`] run is a line
//! boundary this module trusts, so the "line" layer is Pass 4's own S5
//! derivation, not a re-derivation of it. On top of that, only the
//! genuinely new judgements are made: grouping lines into **columns** by
//! horizontal band, and segmenting a column's lines into **paragraphs** by
//! leading gap and first-line indent. Everything the walk already carries
//! (geometry, and — when [`ExtractOptions`](crate::text_extract::ExtractOptions)
//! `::capture_provenance` was set — the per-glyph
//! [`GlyphProvenance`]) is referenced, never recomputed.
//!
//! ## The recognition pipeline
//!
//! ```text
//! PageText.runs  (Pass 4 output, content order)
//!    │
//!    ├─ Stage 1  Lines    split at DerivedLineBreak runs AND at a
//!    │                    within-run baseline jump > line_baseline_ratio·size
//!    │                    (defensive; a source that omitted breaks still
//!    │                    segments). Artifact runs are excluded + counted;
//!    │                    ActualText runs are counted atomic (no glyphs to
//!    │                    split — §14.9.4 N4 makes per-char mapping
//!    │                    impossible).
//!    │
//!    ├─ Stage 2  Columns  cluster lines whose x-ranges overlap by at least
//!    │                    column_overlap_ratio of the narrower span; order
//!    │                    the resulting columns left-to-right (the derived
//!    │                    reading order for an untagged multi-column page,
//!    │                    §14.8.2.3.1).
//!    │
//!    └─ Stage 3  Blocks   within each column (top-to-bottom), start a new
//!                         paragraph when the baseline gap exceeds the
//!                         column's typical leading by paragraph_leading_ratio,
//!                         or when a line is indented from the column margin
//!                         by more than indent_ratio·size.
//! ```
//!
//! Every threshold above is a **tuning knob with no spec basis** (S1–S9),
//! exposed on [`BlockRecognitionOptions`] for exactly the reason Pass 4
//! exposes its three ratios: a constant with no source is a constant that
//! should be arguable. The defaults are deliberately conservative.
//!
//! ## Hit-test and caret/selection
//!
//! [`EditableTextModel::hit_test`] maps a page-space point to a
//! [`TextPosition`] — a `(run, byte-offset)` caret on a glyph boundary —
//! and [`EditableTextModel::resolve_range`] turns two positions into the
//! glyphs a selection covers. Both are pure geometry/range arithmetic over
//! the borrowed [`PageText`]; they introduce **no** GUI or windowing type
//! (the load-bearing GUI-core separation, `ARCHITECTURE.md` §3), which is
//! what lets a future `pdfce-gui` canvas tool and the `pdfcer` inspector
//! share this one model.

use crate::page_tree::Rect;
use crate::text_extract::{ExtractedGlyph, GlyphProvenance, PageText, TextOrigin};

/// A reference back to one glyph in the source [`PageText`].
///
/// The model never copies glyph data; it indexes it. `run` is an index
/// into [`PageText::runs`](crate::text_extract::PageText) and `glyph` is an
/// index into that run's
/// [`glyphs`](crate::text_extract::TextRun::glyphs). Resolve it with
/// [`EditableTextModel::glyph`] / [`EditableTextModel::provenance`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct GlyphRef {
    /// Index into [`PageText::runs`](crate::text_extract::PageText).
    pub run: usize,
    /// Index into that run's `glyphs`.
    pub glyph: usize,
}

impl GlyphRef {
    /// Construct a reference to run `run`, glyph `glyph`.
    #[must_use]
    pub const fn new(run: usize, glyph: usize) -> Self {
        Self { run, glyph }
    }
}

/// A caret position: a byte offset on a glyph boundary within one run.
///
/// This is the `(run, char-offset)` position decision 014 §5.2 calls for,
/// expressed as a **byte** offset into the run's UTF-8
/// [`text`](crate::text_extract::TextRun::text) — because that is the unit
/// Pass 4 already keys glyphs by
/// ([`ExtractedGlyph::text_start`]/[`ExtractedGlyph::text_len`] are byte
/// offsets, since one code may decode to many code points, §9.10.3). The
/// offset is always at a glyph boundary (0, or the end of some glyph), so
/// it is a valid UTF-8 boundary and a valid caret slot. A UI that wants a
/// character index converts with `str`'s char/byte iterators; the model
/// stays in the coordinate the later surgery needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct TextPosition {
    /// Index into [`PageText::runs`](crate::text_extract::PageText).
    pub run: usize,
    /// Byte offset into that run's `text`, on a glyph boundary.
    pub byte_offset: usize,
}

impl TextPosition {
    /// Construct a position at `byte_offset` within run `run`.
    #[must_use]
    pub const fn new(run: usize, byte_offset: usize) -> Self {
        Self { run, byte_offset }
    }

    /// Order key for the two ends of a selection: runs are ordered by
    /// content order (their index), then by byte offset within a run.
    const fn key(self) -> (usize, usize) {
        (self.run, self.byte_offset)
    }
}

/// A recognized line: a baseline-clustered, x-monotonic group of glyphs.
///
/// Derived (S5). A line is Pass 4's own line — the glyphs between two
/// [`TextOrigin::DerivedLineBreak`] runs — plus the defensive
/// baseline-jump split (see the module docs), grouped so a caret can walk
/// it and a column/paragraph pass can stack it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Line {
    /// The line's glyphs, in page content order, as references into the
    /// source [`PageText`]. Never empty (empty lines are not emitted).
    pub glyphs: Vec<GlyphRef>,
    /// The line's baseline y in default user space (§9.4.4) — the shared
    /// y-origin of its glyphs, taken from the first.
    ///
    /// ★ **Meaningful as a *baseline* only when [`Self::direction`] is
    /// horizontal** (`Pass 139.2`). For a line stamped at 90° every glyph
    /// has a *different* `y` and this is merely the first one's — the line
    /// does not have a shared y at all. The recognition thresholds that
    /// consume it (indent, leading gap, column banding) are page-axis
    /// heuristics for laid-out prose and are not claimed to be meaningful
    /// on a rotated line; see [`Self::direction`].
    pub baseline_y: f32,
    /// **The direction this line's text runs in**, as a unit vector in
    /// default user space — the direction shared by every glyph in it
    /// (`Pass 139.2`).
    ///
    /// `(1.0, 0.0)` for ordinary horizontal text. Taken from the line's
    /// first glyph, which is safe because
    /// [`text_extract::layout`](crate::text_extract) closes a run on a
    /// direction change and this model clusters within runs.
    ///
    /// # What it does and does not fix
    ///
    /// [`Self::bbox`] and [`EditableTextModel::hit_test`] are computed in
    /// this frame, so a click in the middle of a rotated letter lands on
    /// that letter. **Block and column recognition are not** — those
    /// stack lines by page-space `y` and band them by page-space `x`,
    /// which is a model of laid-out prose, not of a title block. A
    /// rotated line is recognised as a line and is hit-testable; it is
    /// not meaningfully assigned to a paragraph. Stated here rather than
    /// discovered.
    pub direction: (f32, f32),
    /// A representative effective font size (the largest glyph's), used as
    /// the yardstick for the indent and baseline-jump thresholds.
    pub size: f32,
    /// Bounding box in default user space, approximated one em tall from
    /// the baseline with a quarter-em descender — the same box Pass 4 uses
    /// for a run, and all a line-level box is for (locating it on the
    /// page).
    pub bbox: Rect,
    /// Which [`EditableTextModel::columns`] band this line was clustered
    /// into (0-based, left-to-right).
    pub column: usize,
    /// Which [`EditableTextModel::blocks`] entry (paragraph) this line
    /// belongs to.
    pub block: usize,
}

/// What kind of block was recognized.
///
/// Only [`BlockKind::Paragraph`] exists in Pass 14.0; the enum is
/// `#[non_exhaustive]` so later Passes can name headings, list items,
/// table cells, etc. without a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BlockKind {
    /// A paragraph: vertically-adjacent lines in one column, bounded by a
    /// leading gap or an indent.
    Paragraph,
}

/// A recognized block (paragraph): the reviewable unit a future UI will
/// split / merge / reorder, and a future reflow will target.
///
/// Derived (S9). A block is a maximal run of vertically-adjacent lines
/// within one column that are not separated by a paragraph-sized leading
/// gap or an indented first line.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Block {
    /// The block's kind (always [`BlockKind::Paragraph`] in this Pass).
    pub kind: BlockKind,
    /// Which column band this block sits in.
    pub column: usize,
    /// Indices into [`EditableTextModel::lines`], top-to-bottom. Not a
    /// contiguous range: a column's lines are a subset of the global,
    /// content-ordered line list.
    pub line_indices: Vec<usize>,
    /// Bounding box in default user space — the union of the block's
    /// lines' boxes.
    pub bbox: Rect,
}

/// What the block-recognition pass had to derive — every count, so the
/// guessing is checkable rather than hidden (rule 4; the same discipline
/// as [`TextDiagnostics`](crate::text_extract::TextDiagnostics)).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct BlockDiagnostics {
    /// Lines recognized (S5). DERIVED.
    pub lines_recognized: u64,
    /// Column bands recognized (§14.8.2.3.1 reading order). DERIVED.
    pub columns_recognized: u64,
    /// Blocks (paragraphs) recognized (S9). DERIVED.
    pub blocks_recognized: u64,
    /// Glyphs placed into the hierarchy.
    pub glyphs_clustered: u64,
    /// Paragraph breaks made because a leading gap exceeded the column's
    /// typical leading. DERIVED — no spec basis (S5).
    pub paragraph_breaks_by_leading: u64,
    /// Paragraph breaks made because a line was indented from the column
    /// margin. DERIVED — no spec basis (S9).
    pub paragraph_breaks_by_indent: u64,
    /// Within-run baseline-jump splits made defensively (a line boundary
    /// Pass 4 did not already mark). DERIVED.
    pub lines_split_by_baseline: u64,
    /// `/ActualText` runs left ATOMIC — counted, not split. §14.9.4 N4
    /// makes per-character mapping to glyph positions impossible, so an
    /// `/ActualText` run has no glyphs to cluster; it is reported, not
    /// forced into the hierarchy.
    pub atomic_runs: u64,
    /// Artifact runs (running heads, folios, watermarks) excluded from the
    /// block hierarchy and counted. They are body-text-adjacent, not body
    /// text; excluding them is policy (§14.8.2.2 A1/A3) and reversible by
    /// reading the source runs directly.
    pub artifact_runs_skipped: u64,
    /// Named, human-readable diagnostics, de-duplicated and in first-seen
    /// order (same shape as `pdfcer-render`'s and Pass 4's note lists).
    pub notes: Vec<String>,
}

impl BlockDiagnostics {
    /// Whether more than one column band was recognized — the signal a UI
    /// uses to offer a reading-order review.
    #[must_use]
    pub const fn is_multi_column(&self) -> bool {
        self.columns_recognized > 1
    }

    /// Record a named diagnostic, de-duplicated by exact text.
    fn note(&mut self, text: String) {
        if !self.notes.contains(&text) {
            self.notes.push(text);
        }
    }
}

/// Tuning knobs on the derived block-recognition pass.
///
/// Every value here has **no spec basis whatsoever** (S1–S9) and is
/// exposed rather than hard-coded for the reason Pass 4 exposes its three
/// ratios: a threshold with no source is a threshold that should be
/// arguable, and a corpus will move it (decision 014 §7 revisit trigger
/// 3). The defaults bias toward *under*-segmenting — merging is a visible,
/// correctable defect; a spurious split scatters a paragraph a future
/// reflow would then mis-wrap.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct BlockRecognitionOptions {
    /// Two lines join one column when their horizontal overlap is at least
    /// this fraction of the narrower line's width. Default `0.25`.
    ///
    /// §14.8.2.3.1 makes column ordering derived in an untagged file, so
    /// this governs a guess the standard declines to define. Lower ⇒ more
    /// eager to merge columns into one; higher ⇒ more eager to split.
    pub column_overlap_ratio: f32,
    /// A baseline-to-baseline gap larger than `(1 + this) ×` the column's
    /// typical leading starts a new paragraph. Default `0.5` (a gap half
    /// again the normal leading).
    pub paragraph_leading_ratio: f32,
    /// A line whose left edge is indented from its column's left margin by
    /// more than this fraction of the line's font size starts a new
    /// paragraph (first-line indent). Default `1.0` (one em).
    pub indent_ratio: f32,
    /// Within a line being accumulated, a glyph whose baseline differs from
    /// the line's by more than this fraction of the font size starts a new
    /// line, even if Pass 4 marked no break there. Default `0.30`, matching
    /// `ExtractOptions::line_gap_ratio` — this is the same rule 1, applied
    /// defensively so the model is correct on runs from any source.
    ///
    /// ★ **Measured PERPENDICULAR TO THE LINE, not along the page's y
    /// axis** (`Pass 139.2`). "Baseline" here means the line's own
    /// baseline, wherever it points. Written in page axes this clause
    /// fired between every letter of a 90° line and shattered a
    /// six-letter vertical label into six one-glyph lines.
    pub line_baseline_ratio: f32,
    /// How nearly parallel two consecutive glyphs' writing directions
    /// must be to stay on one line, as a **cosine**. Default
    /// [`SAME_DIRECTION_COS`](crate::text_extract::SAME_DIRECTION_COS)
    /// (about two degrees), matching
    /// [`ExtractOptions::same_direction_cos`](crate::text_extract::ExtractOptions)
    /// — the same rule 0, applied defensively for the same reason
    /// `line_baseline_ratio` restates rule 1.
    ///
    /// Restating it here is what makes this stage correct on runs from
    /// **any** source, including a caller that assembled a `PageText`
    /// itself. Without it a line could hold glyphs running two different
    /// ways, and [`Line::direction`] — taken from the first glyph — would
    /// be a claim about the rest that nothing enforced.
    pub same_direction_cos: f32,
}

impl Default for BlockRecognitionOptions {
    fn default() -> Self {
        Self {
            column_overlap_ratio: 0.25,
            paragraph_leading_ratio: 0.5,
            indent_ratio: 1.0,
            line_baseline_ratio: 0.30,
            same_direction_cos: crate::text_extract::SAME_DIRECTION_COS,
        }
    }
}

/// The recognized, reviewable block structure of one page, borrowing the
/// Pass 4 [`PageText`] it was derived from.
///
/// Construct with [`EditableTextModel::recognize`]. Read the hierarchy via
/// [`Self::blocks`] / [`Self::lines`] / [`Self::columns`], the honesty
/// counters via [`Self::diagnostics`], and the untouched sourced view via
/// [`Self::sourced_view`]. Navigate with [`Self::hit_test`] and
/// [`Self::resolve_range`]. The model owns no glyph data — it is a set of
/// indices and derived boxes over the borrowed page — so it is cheap to
/// build and discard, and it can never disagree with the extraction it
/// points at.
#[derive(Debug, Clone)]
pub struct EditableTextModel<'a> {
    page: &'a PageText,
    lines: Vec<Line>,
    blocks: Vec<Block>,
    columns: usize,
    diagnostics: BlockDiagnostics,
}

/// A line under construction in Stage 1, before column/block assignment.
struct RawLine {
    glyphs: Vec<GlyphRef>,
    baseline_y: f32,
    size: f32,
    /// The line's writing direction, from its first glyph.
    direction: (f32, f32),
    /// The first glyph's origin — the point every in-frame projection in
    /// [`EditableTextModel::hit_in_line`] is measured from.
    origin: (f32, f32),
    llx: f32,
    lly: f32,
    urx: f32,
    ury: f32,
}

impl RawLine {
    fn new(gref: GlyphRef, g: &ExtractedGlyph) -> Self {
        let mut line = Self {
            glyphs: Vec::new(),
            baseline_y: g.y,
            size: g.size,
            direction: g.direction,
            origin: (g.x, g.y),
            llx: f32::MAX,
            lly: f32::MAX,
            urx: f32::MIN,
            ury: f32::MIN,
        };
        line.push(gref, g);
        line
    }

    fn push(&mut self, gref: GlyphRef, g: &ExtractedGlyph) {
        self.glyphs.push(gref);
        self.size = self.size.max(g.size);
        // Glyph box, one em tall from the baseline with a quarter-em
        // descender — the same approximation Pass 4's run box uses, and
        // `Pass 139.2` makes that literal: this calls the SAME function
        // rather than restating the expression. It used to be a fourth
        // hand-written copy of `min(x, x + advance)` etc., and was
        // therefore wrong for rotated text in the same way the other
        // three were, which is exactly `R92`'s failure mode.
        let cell = crate::text_extract::glyph_cell(g.x, g.y, g.advance, g.size, g.direction);
        self.llx = self.llx.min(cell.llx as f32);
        self.urx = self.urx.max(cell.urx as f32);
        self.lly = self.lly.min(cell.lly as f32);
        self.ury = self.ury.max(cell.ury as f32);
    }

    fn bbox(&self) -> Rect {
        Rect::from_corners(
            f64::from(self.llx),
            f64::from(self.lly),
            f64::from(self.urx),
            f64::from(self.ury),
        )
    }
}

/// A column band under construction in Stage 2.
struct ColumnAgg {
    llx: f32,
    urx: f32,
    lines: Vec<usize>,
}

impl<'a> EditableTextModel<'a> {
    /// Recognize the block structure of one extracted page.
    ///
    /// A pure, allocating, side-effect-free derivation over `page.runs`:
    /// it reads geometry, makes the three staged judgements described in
    /// the module docs, counts every one, and borrows `page` for the life
    /// of the returned model. It never mutates `page` and never writes
    /// anything — this is the READ-ONLY Pass.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use pdfcer_core::document::Document;
    /// use pdfcer_core::{page_tree, text_extract, text_edit};
    ///
    /// let doc = Document::load(std::path::Path::new("in.pdf"))?;
    /// let pages = page_tree::pages(&doc)?;
    /// // Provenance is optional; the block model works without it.
    /// let options = text_extract::ExtractOptions::default().with_provenance(true);
    /// let page = text_extract::extract_page(&doc, &pages[0], 0, &options)?;
    ///
    /// let model = text_edit::EditableTextModel::recognize(
    ///     &page,
    ///     &text_edit::BlockRecognitionOptions::default(),
    /// );
    /// println!("{} blocks in {} columns", model.blocks().len(), model.columns());
    /// // The sourced-only truth is still exactly one call away:
    /// let _sourced = model.sourced_view().sourced_text();
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn recognize(page: &'a PageText, options: &BlockRecognitionOptions) -> Self {
        let mut diagnostics = BlockDiagnostics::default();
        let raw_lines = Self::cluster_lines(page, options, &mut diagnostics);
        let (mut lines, columns) = Self::cluster_columns(raw_lines, options, &mut diagnostics);
        let blocks = Self::segment_blocks(&mut lines, columns, options, &mut diagnostics);

        diagnostics.lines_recognized = lines.len() as u64;
        diagnostics.columns_recognized = columns as u64;
        diagnostics.blocks_recognized = blocks.len() as u64;
        if !lines.is_empty() {
            diagnostics.note(
                "text-blocks: line/column/paragraph structure is DERIVED from glyph geometry \
                 and REVIEWABLE — an untagged content stream defines none of it (ISO 32000-1 \
                 §14.8, S1-S9); the sourced-only text is unchanged"
                    .to_string(),
            );
        }
        if diagnostics.is_multi_column() {
            diagnostics.note(format!(
                "text-blocks: {} column bands were derived and ordered left-to-right — an \
                 untagged file's reading order is not sourced (§14.8.2.3.1); review before relying \
                 on the order",
                columns
            ));
        }

        Self {
            page,
            lines,
            blocks,
            columns,
            diagnostics,
        }
    }

    // -- Stage 1: lines ---------------------------------------------------

    /// Split `page.runs` into raw lines at Pass 4's derived line breaks and
    /// at a defensive within-line baseline jump (module docs, Stage 1).
    fn cluster_lines(
        page: &PageText,
        options: &BlockRecognitionOptions,
        diagnostics: &mut BlockDiagnostics,
    ) -> Vec<RawLine> {
        let mut lines: Vec<RawLine> = Vec::new();
        let mut current: Option<RawLine> = None;

        for (ri, run) in page.runs.iter().enumerate() {
            match run.origin {
                // A Pass-4 derived line break (baseline move OR two-column
                // backward jump, S5) closes the current line.
                TextOrigin::DerivedLineBreak => {
                    if let Some(line) = current.take() {
                        lines.push(line);
                    }
                }
                // A derived word space stays within the line.
                TextOrigin::DerivedWordSpace => {}
                // An /ActualText run has no glyphs to cluster (§14.9.4 N4);
                // count it and leave it out of the hierarchy.
                TextOrigin::ActualText => diagnostics.atomic_runs += 1,
                TextOrigin::Glyphs => {
                    if run.artifact.is_some() {
                        // Body-text-adjacent, not body text: count + skip.
                        diagnostics.artifact_runs_skipped += 1;
                        continue;
                    }
                    for (gi, g) in run.glyphs.iter().enumerate() {
                        let gref = GlyphRef::new(ri, gi);
                        // `Pass 139.2`: the defensive split is measured
                        // PERPENDICULAR TO THE LINE, not along the page's
                        // y axis.
                        //
                        // Written in page axes this clause read
                        // `(g.y - line.baseline_y).abs() > ratio * size`,
                        // and on a 90° line every glyph advances one whole
                        // advance in `y` — so it fired between EVERY
                        // LETTER and shattered a six-letter vertical label
                        // into six one-glyph lines. Measured on
                        // `rotated-text.pdf` before this change: 16 lines
                        // for four lines of text.
                        //
                        // ★ That is worth noticing on its own: `Pass
                        // 139.1` had already stopped the EXTRACTION from
                        // fragmenting rotated text, so `page.runs` held
                        // one clean run per block — and this stage
                        // re-fragmented it immediately. A second copy of
                        // the same page-axis assumption, in a second
                        // module, defeating the fix upstream of it. It was
                        // found by SABOTAGE, not by a failing test: with
                        // the runs already correct, the hit-test tests
                        // passed for the wrong reason (each glyph had
                        // become its own line, so each probe trivially
                        // found "its" line).
                        //
                        // A direction change also splits, for the same
                        // reason `layout::classify` breaks on one: a line
                        // whose glyphs run two different ways has no
                        // single direction to publish, and `Line::bbox`
                        // and `hit_in_line` both need one.
                        let jumped = current.as_ref().is_some_and(|line| {
                            let size = line.size.max(g.size).max(1e-6);
                            let (dx, dy) = line.direction;
                            if dx * g.direction.0 + dy * g.direction.1 < options.same_direction_cos
                            {
                                return true;
                            }
                            // Perpendicular displacement from the line's
                            // own origin: `d × dir`. For `dir = (1, 0)`
                            // this is `−(g.y − baseline_y)`, and only its
                            // magnitude is compared — the historical
                            // expression, term for term.
                            let (ox, oy) = line.origin;
                            let perp = (g.x - ox) * dy - (g.y - oy) * dx;
                            perp.abs() > options.line_baseline_ratio * size
                        });
                        if jumped {
                            if let Some(line) = current.take() {
                                lines.push(line);
                            }
                            diagnostics.lines_split_by_baseline += 1;
                        }
                        diagnostics.glyphs_clustered += 1;
                        match current.as_mut() {
                            Some(line) => line.push(gref, g),
                            None => current = Some(RawLine::new(gref, g)),
                        }
                    }
                }
            }
        }
        if let Some(line) = current.take() {
            lines.push(line);
        }
        lines
    }

    // -- Stage 2: columns -------------------------------------------------

    /// Cluster raw lines into left-to-right column bands by horizontal
    /// overlap, and finalize each into a [`Line`] with its column set
    /// (module docs, Stage 2). Returns the finalized lines and the column
    /// count.
    fn cluster_columns(
        raw: Vec<RawLine>,
        options: &BlockRecognitionOptions,
        _diagnostics: &mut BlockDiagnostics,
    ) -> (Vec<Line>, usize) {
        let mut columns: Vec<ColumnAgg> = Vec::new();

        for (li, line) in raw.iter().enumerate() {
            let (llx, urx) = (line.llx, line.urx);
            let line_w = (urx - llx).max(0.0);
            // Choose the existing band with the greatest qualifying overlap.
            let mut best: Option<(usize, f32)> = None;
            for (ci, col) in columns.iter().enumerate() {
                let overlap = (urx.min(col.urx) - llx.max(col.llx)).max(0.0);
                let col_w = (col.urx - col.llx).max(0.0);
                let narrower = line_w.min(col_w).max(1e-6);
                if overlap >= options.column_overlap_ratio * narrower
                    && best.is_none_or(|(_, b)| overlap > b)
                {
                    best = Some((ci, overlap));
                }
            }
            match best {
                Some((ci, _)) => {
                    if let Some(col) = columns.get_mut(ci) {
                        col.llx = col.llx.min(llx);
                        col.urx = col.urx.max(urx);
                        col.lines.push(li);
                    }
                }
                None => columns.push(ColumnAgg {
                    llx,
                    urx,
                    lines: vec![li],
                }),
            }
        }

        // Order bands left-to-right; that ordering IS the derived reading
        // order for an untagged multi-column page (§14.8.2.3.1). Rank a
        // copy of the (left-edge, band-index) pairs so the comparator never
        // indexes back into `columns` (the crate's panic-free policy).
        let mut ranked: Vec<(f32, usize)> = columns
            .iter()
            .enumerate()
            .map(|(ci, col)| (col.llx, ci))
            .collect();
        ranked.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        // Map original band index -> left-to-right column index.
        let mut column_of = vec![0usize; columns.len()];
        for (new_ci, &(_, old_ci)) in ranked.iter().enumerate() {
            if let Some(slot) = column_of.get_mut(old_ci) {
                *slot = new_ci;
            }
        }

        // Finalize lines, stamping each with its column (block set later).
        let mut lines: Vec<Line> = Vec::with_capacity(raw.len());
        for (ci, col) in columns.iter().enumerate() {
            let column = column_of.get(ci).copied().unwrap_or(0);
            for &li in &col.lines {
                if let Some(src) = raw.get(li) {
                    lines.push(Line {
                        glyphs: src.glyphs.clone(),
                        baseline_y: src.baseline_y,
                        direction: src.direction,
                        size: src.size,
                        bbox: src.bbox(),
                        column,
                        block: 0,
                    });
                }
            }
        }
        (lines, columns.len())
    }

    // -- Stage 3: blocks (paragraphs) -------------------------------------

    /// Segment each column's lines into paragraphs by leading gap and
    /// first-line indent, stamping every line's `block` field (module
    /// docs, Stage 3).
    fn segment_blocks(
        lines: &mut [Line],
        columns: usize,
        options: &BlockRecognitionOptions,
        diagnostics: &mut BlockDiagnostics,
    ) -> Vec<Block> {
        let mut blocks: Vec<Block> = Vec::new();

        for column in 0..columns {
            // Snapshot this column's lines as (index, baseline_y, left,
            // size), top-to-bottom (higher y first). Working on the
            // snapshot keeps every access below out of the panic-prone
            // indexing path (the crate's panic-free policy) — the only
            // index used again is a checked `get_mut` when stamping blocks.
            let mut col_lines: Vec<(usize, f32, f64, f32)> = lines
                .iter()
                .enumerate()
                .filter(|(_, l)| l.column == column)
                .map(|(i, l)| (i, l.baseline_y, l.bbox.llx, l.size))
                .collect();
            col_lines.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            // The column's left margin: the smallest line-left in the band.
            let margin = col_lines
                .iter()
                .map(|&(_, _, llx, _)| llx)
                .fold(f64::MAX, f64::min);

            // Typical leading: the median of consecutive baseline gaps —
            // robust to one outsized paragraph gap, unlike the mean.
            let mut gaps: Vec<f32> = col_lines
                .windows(2)
                .filter_map(|w| match w {
                    [a, b] => Some(a.1 - b.1),
                    _ => None,
                })
                .collect();
            let typical = median(&mut gaps);

            let mut current: Vec<usize> = Vec::new();
            let mut prev_baseline: Option<f32> = None;
            for &(i, baseline_y, llx, size) in &col_lines {
                let start_new = match prev_baseline {
                    None => false,
                    Some(prev_y) => {
                        let gap = prev_y - baseline_y;
                        let leading_break = typical > 0.0
                            && gap > (1.0 + options.paragraph_leading_ratio) * typical;
                        let indent = llx - margin;
                        let indent_break = indent > f64::from(options.indent_ratio * size);
                        if leading_break {
                            diagnostics.paragraph_breaks_by_leading += 1;
                        }
                        // Count an indent break only when it is the reason
                        // (not already a leading break), so the two counters
                        // partition the paragraph starts.
                        if indent_break && !leading_break {
                            diagnostics.paragraph_breaks_by_indent += 1;
                        }
                        leading_break || indent_break
                    }
                };
                if start_new && !current.is_empty() {
                    blocks.push(Self::finish_block(
                        lines,
                        column,
                        std::mem::take(&mut current),
                    ));
                }
                current.push(i);
                prev_baseline = Some(baseline_y);
            }
            if !current.is_empty() {
                blocks.push(Self::finish_block(lines, column, current));
            }
        }

        // Stamp every line with the block it landed in.
        for (bi, block) in blocks.iter().enumerate() {
            for &li in &block.line_indices {
                if let Some(line) = lines.get_mut(li) {
                    line.block = bi;
                }
            }
        }
        blocks
    }

    /// Build a [`Block`] from a column's paragraph line indices, unioning
    /// their boxes.
    fn finish_block(lines: &[Line], column: usize, line_indices: Vec<usize>) -> Block {
        let mut bbox: Option<Rect> = None;
        for &li in &line_indices {
            let Some(line) = lines.get(li) else { continue };
            let b = line.bbox;
            bbox = Some(match bbox {
                None => b,
                Some(acc) => Rect {
                    llx: acc.llx.min(b.llx),
                    lly: acc.lly.min(b.lly),
                    urx: acc.urx.max(b.urx),
                    ury: acc.ury.max(b.ury),
                },
            });
        }
        Block {
            kind: BlockKind::Paragraph,
            column,
            line_indices,
            bbox: bbox.unwrap_or_else(|| Rect::from_corners(0.0, 0.0, 0.0, 0.0)),
        }
    }

    // -- Accessors --------------------------------------------------------

    /// The recognized blocks (paragraphs), in column-major, top-to-bottom
    /// order.
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// The recognized lines. [`Line::column`] and [`Line::block`] index the
    /// column bands and [`Self::blocks`] respectively.
    #[must_use]
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// The number of column bands recognized (S9-derived reading order).
    #[must_use]
    pub const fn columns(&self) -> usize {
        self.columns
    }

    /// The derived-inference counts — the fuzzy-never-sneaky disclosure.
    #[must_use]
    pub const fn diagnostics(&self) -> &BlockDiagnostics {
        &self.diagnostics
    }

    /// The untouched Pass 4 extraction this model was derived from — the
    /// **sourced-only view always remains available** (decision 014 §3).
    /// Call
    /// [`PageText::sourced_text`](crate::text_extract::PageText::sourced_text)
    /// on it for exactly the characters the file provides, unaffected by
    /// any derived block judgement.
    #[must_use]
    pub const fn sourced_view(&self) -> &'a PageText {
        self.page
    }

    /// The [`ExtractedGlyph`] a [`GlyphRef`] points at, or `None` if the
    /// reference is stale for this page.
    #[must_use]
    pub fn glyph(&self, gref: GlyphRef) -> Option<&'a ExtractedGlyph> {
        self.page.runs.get(gref.run)?.glyphs.get(gref.glyph)
    }

    /// The [`GlyphProvenance`] of a referenced glyph — the surgery
    /// substrate — or `None` if the reference is stale or the extraction
    /// was run without
    /// [`ExtractOptions::capture_provenance`](crate::text_extract::ExtractOptions).
    #[must_use]
    pub fn provenance(&self, gref: GlyphRef) -> Option<&'a GlyphProvenance> {
        self.glyph(gref)?.provenance.as_ref()
    }

    /// The characters of one line, concatenated from its glyphs' slices of
    /// the source run text.
    #[must_use]
    pub fn line_text(&self, line: &Line) -> String {
        let mut out = String::new();
        for &gref in &line.glyphs {
            if let Some(run) = self.page.runs.get(gref.run)
                && let Some(g) = run.glyphs.get(gref.glyph)
            {
                let start = g.text_start as usize;
                let end = start + g.text_len as usize;
                if let Some(slice) = run.text.get(start..end) {
                    out.push_str(slice);
                }
            }
        }
        out
    }

    /// The characters of one block, its lines joined with `\n` — a DERIVED
    /// rendering (the line breaks are S5 judgements), suitable for a
    /// review/preview surface, never a sourced accessor.
    #[must_use]
    pub fn block_text(&self, block: &Block) -> String {
        let mut out = String::new();
        for (i, &li) in block.line_indices.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            if let Some(line) = self.lines.get(li) {
                out.push_str(&self.line_text(line));
            }
        }
        out
    }

    // -- Hit-test and selection ------------------------------------------

    /// Map a page-space point to a caret [`TextPosition`].
    ///
    /// Finds the line whose box contains `(x, y)` — or, if none does, the
    /// vertically nearest line — then the glyph on that line whose
    /// horizontal extent contains `x` (or the nearest), and resolves to the
    /// glyph's leading or trailing boundary by which half of the glyph `x`
    /// fell in. Returns `None` only when the page has no clustered glyph at
    /// all. Pure geometry over the borrowed page; introduces no GUI type.
    #[must_use]
    pub fn hit_test(&self, x: f64, y: f64) -> Option<TextPosition> {
        // Pick the line: containing box first, else nearest by baseline.
        let mut chosen: Option<&Line> = None;
        let mut best_dy = f64::MAX;
        for line in &self.lines {
            let b = line.bbox;
            let contains_y = y >= b.lly && y <= b.ury;
            let dy = (y - f64::from(line.baseline_y)).abs();
            if contains_y && x >= b.llx && x <= b.urx {
                return self.hit_in_line(line, x, y);
            }
            if contains_y && dy < best_dy {
                best_dy = dy;
                chosen = Some(line);
            }
        }
        // No box contained the point in x; fall back to the nearest line by
        // baseline distance, then clamp within it.
        if chosen.is_none() {
            for line in &self.lines {
                let dy = (y - f64::from(line.baseline_y)).abs();
                if dy < best_dy {
                    best_dy = dy;
                    chosen = Some(line);
                }
            }
        }
        chosen.and_then(|line| self.hit_in_line(line, x, y))
    }

    /// Resolve a page-space point to a caret within one line: the glyph
    /// whose extent **along the line's own writing direction** contains it
    /// (leading/trailing half), else clamp to the line ends.
    ///
    /// # `Pass 139.2`: projected onto the line, not onto the page x axis
    ///
    /// Every comparison here used to be against `x` alone — `g.x` versus
    /// `g.x + g.advance` — which is right for a horizontal line and
    /// meaningless for any other. On a line stamped at 90° all of its
    /// glyphs share one `x`, so the first glyph "contained" every click
    /// and the caret never moved; on a 180° line the extents ran the wrong
    /// way and both ends collapsed onto one slot. Driven by the consuming
    /// shell before the fix: a sweep down a six-letter 90° string selected
    /// **five** of them, and a sweep along an eight-letter 180° string
    /// selected **nothing at all**.
    ///
    /// The generalisation is one projection. `t` is the point's distance
    /// along the line's direction from the line's own origin, and each
    /// glyph's extent is `[t0, t0 + advance]` in the same coordinate. For
    /// `direction = (1, 0)` and a line whose origin is its leftmost glyph,
    /// `t` is `x − origin.x` and every comparison below reduces term for
    /// term to the ones it replaced.
    ///
    /// The perpendicular component is deliberately **discarded**: the
    /// caller has already chosen the line (by box containment or by
    /// nearest baseline), so how far off the baseline the click was is no
    /// longer a question this function answers.
    fn hit_in_line(&self, line: &Line, x: f64, y: f64) -> Option<TextPosition> {
        let (dx, dy) = (f64::from(line.direction.0), f64::from(line.direction.1));
        // The point, projected onto the line's direction. The origin the
        // projection is measured from cancels out of every comparison
        // below, so any fixed point on the line would do; the first
        // glyph's is used because it is what `RawLine` already records.
        let origin = self
            .glyph(*line.glyphs.first()?)
            .map(|g| (f64::from(g.x), f64::from(g.y)))?;
        let along = |px: f64, py: f64| (px - origin.0) * dx + (py - origin.1) * dy;
        let t = along(x, y);

        let mut best: Option<(GlyphRef, &ExtractedGlyph, f64, f64)> = None;
        for &gref in &line.glyphs {
            let g = self.glyph(gref)?;
            let t0 = along(f64::from(g.x), f64::from(g.y));
            let t1 = t0 + f64::from(g.advance);
            let (lo, hi) = (t0.min(t1), t0.max(t1));
            if t >= lo && t <= hi {
                let mid = (lo + hi) / 2.0;
                return Some(self.boundary(gref, g, t > mid));
            }
            // Track the nearest glyph for the clamp-to-end fallback.
            let dist = if t < lo { lo - t } else { t - hi };
            if best.is_none_or(|(_, _, d, _)| dist < d) {
                best = Some((gref, g, dist, t0));
            }
        }
        best.map(|(gref, g, _, t0)| {
            // Clamp: before the nearest glyph's midpoint ALONG THE LINE ⇒
            // its leading edge; after ⇒ trailing. "Before" and "after" are
            // the line's own sense of the words, not the page's — which is
            // the whole point of the projection, and is why a 180° line
            // used to clamp both ends to the same slot.
            let trailing = t > t0 + f64::from(g.advance) / 2.0;
            self.boundary(gref, g, trailing)
        })
    }

    /// The caret position at a glyph's leading (`trailing == false`) or
    /// trailing edge, as a byte offset into its run's text.
    fn boundary(&self, gref: GlyphRef, g: &ExtractedGlyph, trailing: bool) -> TextPosition {
        let offset = if trailing {
            (g.text_start + g.text_len) as usize
        } else {
            g.text_start as usize
        };
        TextPosition::new(gref.run, offset)
    }

    // -- Boundary lookups (Pass 14.3 GUI: double/triple-click, Home/End) --
    //
    // The GUI's word/line selection and Home/End caret navigation need the
    // Line a caret sits on, and the word/line span around it. Per decision
    // 014 §4.1 ("core owns the derived structure") and Pass 14.3 UI spec
    // §4.3, these live HERE — reusing the exact `text_start`/`text_len`
    // glyph-boundary matching `hit_test`/`hit_in_line` already encode —
    // rather than being re-derived (and possibly diverging) in `pdfce-gui`.
    // All three are pure index/range arithmetic over the borrowed page; they
    // add NO GUI type (the load-bearing GUI-core separation, §3).

    /// The [`Self::lines`] index of the line containing caret `pos`, or
    /// `None` if no line holds a glyph of `pos.run` whose byte range brackets
    /// `pos.byte_offset` (a stale reference, or a `pos.run` that carries no
    /// clustered glyph — an `/ActualText`/whitespace run).
    ///
    /// This is the reverse of `hit_test`'s internal line-then-glyph walk:
    /// where `hit_test` maps a *point* to a `(run, offset)`, this maps a
    /// `(run, offset)` back to the *line* it was clustered into. A run's
    /// glyphs may be split across lines by a baseline jump (module docs,
    /// Stage 1), so the match is per-glyph, not per-run: the first line (in
    /// content order) carrying a `pos.run` glyph whose
    /// `[text_start, text_start+text_len]` closed interval contains
    /// `pos.byte_offset` wins. The interval is closed so a caret exactly on a
    /// glyph's trailing boundary resolves (it is a valid caret slot).
    #[must_use]
    pub fn line_at(&self, pos: TextPosition) -> Option<usize> {
        for (li, line) in self.lines.iter().enumerate() {
            for &gref in &line.glyphs {
                if gref.run != pos.run {
                    continue;
                }
                let Some(g) = self.glyph(gref) else { continue };
                let lo = g.text_start as usize;
                let hi = lo + g.text_len as usize;
                if pos.byte_offset >= lo && pos.byte_offset <= hi {
                    return Some(li);
                }
            }
        }
        None
    }

    /// The [`Self::blocks`] index of the block (paragraph) containing caret
    /// `pos`, or `None` when [`Self::line_at`] finds no line for `pos`.
    ///
    /// Sugar over [`Self::line_at`] then [`Line::block`] — the same "core owns
    /// the derived structure" spirit as `line_at`/`word_range_at`/
    /// `line_range_at` themselves (Pass 14.3 UI spec §4.3), so the three-line
    /// composition does not reappear at every call site (CLI, GUI, tests).
    /// Pure index arithmetic over the borrowed page; adds NO GUI type (the
    /// load-bearing GUI-core separation, §3). Pass 15.2's reflow sub-mode
    /// resolves "which paragraph is the caret in" through this against a model
    /// built with [`super::reflow::reflow_recognition_options`].
    #[must_use]
    pub fn block_at(&self, pos: TextPosition) -> Option<usize> {
        let li = self.line_at(pos)?;
        self.lines.get(li).map(|l| l.block)
    }

    /// The first and last caret positions of the line containing `pos` — the
    /// two ends Home/End move to (Pass 14.3 UI spec §4.5). `None` when
    /// [`Self::line_at`] finds no line for `pos`.
    ///
    /// The ends are the leading boundary of the line's first glyph and the
    /// trailing boundary of its last — each a real [`TextPosition`] on a
    /// glyph boundary. A line may draw glyphs from more than one run (a
    /// derived word space between two runs stays within the line), so the two
    /// returned positions can name different runs; that is fine for caret
    /// navigation (Home/End never commits an edit — a *selection* spanning
    /// >1 run is refused separately, §4.4/UI spec).
    #[must_use]
    pub fn line_range_at(&self, pos: TextPosition) -> Option<(TextPosition, TextPosition)> {
        let li = self.line_at(pos)?;
        let line = self.lines.get(li)?;
        let first = *line.glyphs.first()?;
        let last = *line.glyphs.last()?;
        let fg = self.glyph(first)?;
        let lg = self.glyph(last)?;
        let start = TextPosition::new(first.run, fg.text_start as usize);
        let end = TextPosition::new(last.run, (lg.text_start + lg.text_len) as usize);
        Some((start, end))
    }

    // -- Caret navigation geometry (Pass 14.4 GUI: arrows / Up-Down) ------
    //
    // Pass 14.4 completes the caret model with keyboard navigation (14.3 UI
    // spec §4.5). Left/Right/Up/Down are pure traversals over structure this
    // model already owns, so — like `line_at`/`word_range_at`/`line_range_at`
    // in Pass 14.3 — they live HERE, not re-derived in `pdfce-gui`: the GUI's
    // `PageText`/`TextRun`/`ExtractedGlyph` are `#[non_exhaustive]` and so
    // cannot be constructed in a `pdfce-gui` unit test, which means core is
    // also the only place these can be *headless-tested* (decision 014 §4.1's
    // "core owns the derived structure" argument, reinforced by the crate
    // boundary). All add NO GUI type (the load-bearing GUI-core separation,
    // §3). Home/End need no new method — they are exactly
    // [`Self::line_range_at`]'s two ends.

    /// The page-space x of caret `pos` — the leading edge of the glyph that
    /// begins at `pos.byte_offset`, or the trailing edge of the glyph that ends
    /// there (Pass 14.4 Up/Down "nearest-x", UI spec §4.5). `None` when no
    /// glyph in `pos.run` has a boundary exactly at `pos.byte_offset` (a stale
    /// position, or a run — derived whitespace / `/ActualText` — carrying no
    /// clustered glyph).
    ///
    /// This is the x-half of the vertical segment the GUI draws for a caret,
    /// exposed so vertical navigation can compute a "desired column" through
    /// the SAME glyph-boundary matching [`Self::hit_test`] / [`Self::line_range_at`]
    /// already encode, rather than the GUI re-deriving glyph x-positions.
    /// ★ **A page-axis answer, and on a rotated line it is the wrong
    /// question** (`Pass 139.2`). Every glyph of a 90° line shares one `x`,
    /// so this returns the same number for every caret slot on it. The
    /// signature is the limit — a scalar cannot name a point on a line
    /// that is not horizontal — which is the same shape of defect
    /// `PickedLine::object_index` had in `Pass 138.0`: an answer made
    /// unrepresentable by its own return type.
    ///
    /// It is kept, un-deprecated, because for horizontal text it is
    /// exactly right and is what "desired column" for Up/Down navigation
    /// means. Use [`Self::caret_point`] when the line may be rotated.
    #[must_use]
    pub fn caret_x(&self, pos: TextPosition) -> Option<f32> {
        self.caret_point(pos).map(|(x, _)| x)
    }

    /// **The page-space point of caret `pos`** — the origin of the glyph
    /// that begins at `pos.byte_offset`, or the
    /// [`advance_end`](crate::text_extract::ExtractedGlyph::advance_end) of
    /// the glyph that ends there (`Pass 139.2`).
    ///
    /// `None` under exactly the same conditions as [`Self::caret_x`]: no
    /// glyph in `pos.run` has a boundary at that offset, because the
    /// position is stale or the run carries no clustered glyph (derived
    /// whitespace, `/ActualText`).
    ///
    /// # Why this exists beside [`Self::caret_x`]
    ///
    /// A caret is a *point on a baseline*, and a baseline has a direction.
    /// `caret_x` returns the x half of one, which is complete for
    /// horizontal text and degenerate for anything else — on a 90° line
    /// every slot has the same `x`. Pair this with the line's
    /// [`Line::direction`] and a shell has everything it needs to draw the
    /// caret *along* the text rather than always vertically.
    #[must_use]
    pub fn caret_point(&self, pos: TextPosition) -> Option<(f32, f32)> {
        let run = self.page.runs.get(pos.run)?;
        for g in &run.glyphs {
            let lo = g.text_start as usize;
            let hi = lo + g.text_len as usize;
            if pos.byte_offset == lo {
                return Some((g.x, g.y));
            }
            if pos.byte_offset == hi {
                return Some(g.advance_end());
            }
        }
        None
    }

    /// The caret on line `line_index` whose x-extent is nearest page-space `x`
    /// — the same within-line resolution [`Self::hit_test`] performs
    /// internally, exposed for ONE explicit line so vertical caret navigation
    /// (Pass 14.4 Up/Down, UI spec §4.5) can land on the geometrically nearest
    /// slot of the adjacent line without a third re-implementation of
    /// nearest-glyph matching in the GUI (§3). `None` for an out-of-range
    /// `line_index` or a line whose glyphs are all stale.
    ///
    /// ★ **Page-axis, and it stays that way on purpose** (`Pass 139.2`).
    /// `x` alone cannot name a slot on a line that is not horizontal, so
    /// this delegates with the line's own `baseline_y` as the second
    /// coordinate — which is exact for horizontal text and, for a rotated
    /// line, resolves as though the caller had clicked on its baseline at
    /// that `x`. Use [`Self::caret_on_line_nearest_point`] when the line
    /// may be rotated. Not deprecated: this *is* the right shape for the
    /// Up/Down "desired column" it was built for.
    #[must_use]
    pub fn caret_on_line_nearest_x(&self, line_index: usize, x: f64) -> Option<TextPosition> {
        let line = self.lines.get(line_index)?;
        let y = f64::from(line.baseline_y);
        self.hit_in_line(line, x, y)
    }

    /// The caret on line `line_index` nearest a page-space **point**,
    /// resolved along that line's own writing direction (`Pass 139.2`).
    ///
    /// The two-coordinate twin of [`Self::caret_on_line_nearest_x`], and
    /// the same body — [`Self::hit_test`] calls it too, so there is one
    /// implementation of within-line resolution rather than three. `None`
    /// for an out-of-range `line_index` or a line whose glyphs are all
    /// stale.
    #[must_use]
    pub fn caret_on_line_nearest_point(
        &self,
        line_index: usize,
        x: f64,
        y: f64,
    ) -> Option<TextPosition> {
        let line = self.lines.get(line_index)?;
        self.hit_in_line(line, x, y)
    }

    /// Move the caret one glyph boundary left (Pass 14.4, UI spec §4.5).
    ///
    /// Steps within the run and, at a run's/line's start, across to the
    /// previous run's last slot — a new line begins at a new run after a
    /// [`TextOrigin::DerivedLineBreak`], so this glides across line boundaries
    /// for free. At the document's very first slot it stays put (clamped, never
    /// wraps). Empty runs (derived word-space / line-break / `/ActualText`)
    /// carry no glyph and so contribute no slot — which is exactly why the step
    /// skips over them.
    #[must_use]
    pub fn caret_left(&self, pos: TextPosition) -> TextPosition {
        let key = pos.key();
        self.caret_slots()
            .into_iter()
            .rev()
            .find(|p| p.key() < key)
            .unwrap_or(pos)
    }

    /// Move the caret one glyph boundary right (Pass 14.4, UI spec §4.5). The
    /// mirror of [`Self::caret_left`]; clamps at the document's last slot.
    #[must_use]
    pub fn caret_right(&self, pos: TextPosition) -> TextPosition {
        let key = pos.key();
        self.caret_slots()
            .into_iter()
            .find(|p| p.key() > key)
            .unwrap_or(pos)
    }

    /// Move the caret to the geometrically nearest slot on the line immediately
    /// ABOVE the current one within the same column (Pass 14.4 Up, UI spec
    /// §4.5). `desired_x` is the page-space column to preserve — the GUI passes
    /// [`Self::caret_x`] of the current caret. Stays put when there is no line
    /// above in the column, or the caret is not on a recognized line.
    ///
    /// "Above" is a LARGER baseline y: default user space y increases UP the
    /// page (§9.4.4), the opposite of screen space.
    #[must_use]
    pub fn caret_up(&self, pos: TextPosition, desired_x: f32) -> TextPosition {
        self.caret_vertical(pos, desired_x, true)
    }

    /// Move the caret to the nearest slot on the line immediately BELOW the
    /// current one within the same column (Pass 14.4 Down, UI spec §4.5). The
    /// mirror of [`Self::caret_up`] — "below" is a SMALLER baseline y.
    #[must_use]
    pub fn caret_down(&self, pos: TextPosition, desired_x: f32) -> TextPosition {
        self.caret_vertical(pos, desired_x, false)
    }

    /// Shared body of [`Self::caret_up`] / [`Self::caret_down`]: find the
    /// closest line in the same column on the requested side, then resolve
    /// `desired_x` within it via [`Self::caret_on_line_nearest_x`].
    fn caret_vertical(&self, pos: TextPosition, desired_x: f32, up: bool) -> TextPosition {
        let Some(cur_idx) = self.line_at(pos) else {
            return pos;
        };
        let Some(cur) = self.lines.get(cur_idx) else {
            return pos;
        };
        // The nearest line on the requested side, by baseline distance, in the
        // same column band (§14.8.2.3.1 reading order — never cross columns).
        let mut best: Option<(usize, f32)> = None;
        for (li, line) in self.lines.iter().enumerate() {
            if li == cur_idx || line.column != cur.column {
                continue;
            }
            let dy = line.baseline_y - cur.baseline_y;
            let toward = if up { dy > 0.0 } else { dy < 0.0 };
            if !toward {
                continue;
            }
            let closer = best.is_none_or(|(_, by)| dy.abs() < (by - cur.baseline_y).abs());
            if closer {
                best = Some((li, line.baseline_y));
            }
        }
        match best {
            Some((li, _)) => self
                .caret_on_line_nearest_x(li, f64::from(desired_x))
                .unwrap_or(pos),
            None => pos,
        }
    }

    /// Every caret slot on the page — the leading and trailing byte boundary of
    /// each clustered glyph — in `(run, byte_offset)` content order, de-duped.
    /// The ordered spine [`Self::caret_left`] / [`Self::caret_right`] step
    /// along.
    fn caret_slots(&self) -> Vec<TextPosition> {
        let mut slots = Vec::new();
        for (ri, run) in self.page.runs.iter().enumerate() {
            for g in &run.glyphs {
                let lo = g.text_start as usize;
                let hi = lo + g.text_len as usize;
                slots.push(TextPosition::new(ri, lo));
                slots.push(TextPosition::new(ri, hi));
            }
        }
        slots.sort_by_key(|p| p.key());
        slots.dedup();
        slots
    }

    /// The word boundaries around caret `pos`, split on Unicode whitespace
    /// **within `pos.run`'s own text** — what double-click selects (Pass 14.3
    /// UI spec §4.3).
    ///
    /// A DERIVED judgement with the same honesty posture as every other
    /// boundary in this model: word boundaries do not exist in an untagged
    /// content stream any more than lines do (S1–S9), so this is a
    /// whitespace split over the run's decoded text, not a sourced fact.
    /// Both returned positions name `pos.run` (a word never spans runs), so
    /// the result is always an editable single-run selection. When `pos.run`
    /// is out of range the position is returned collapsed (`(pos, pos)`),
    /// never a panic.
    #[must_use]
    pub fn word_range_at(&self, pos: TextPosition) -> (TextPosition, TextPosition) {
        let Some(run) = self.page.runs.get(pos.run) else {
            return (pos, pos);
        };
        let (lo, hi) = word_bounds(&run.text, pos.byte_offset);
        (
            TextPosition::new(pos.run, lo),
            TextPosition::new(pos.run, hi),
        )
    }

    /// The glyphs covered by the selection between two caret positions.
    ///
    /// Order-insensitive: the two positions are sorted, then every glyph
    /// whose byte range intersects the covered span — from `start.byte_offset`
    /// in the start run, through whole intervening runs, to `end.byte_offset`
    /// in the end run — is returned, in content order. `/ActualText` and
    /// derived-whitespace runs contribute no glyphs (they carry none), so a
    /// selection across them yields exactly the real glyphs it covers.
    #[must_use]
    pub fn resolve_range(&self, a: TextPosition, b: TextPosition) -> Vec<GlyphRef> {
        let (start, end) = if a.key() <= b.key() { (a, b) } else { (b, a) };
        let mut covered = Vec::new();
        let last = self.page.runs.len().saturating_sub(1);
        for ri in start.run..=end.run.min(last) {
            let Some(run) = self.page.runs.get(ri) else {
                break;
            };
            // The byte window of this run that the selection covers.
            let lo = if ri == start.run {
                start.byte_offset
            } else {
                0
            };
            let hi = if ri == end.run {
                end.byte_offset
            } else {
                run.text.len()
            };
            for (gi, g) in run.glyphs.iter().enumerate() {
                let g0 = g.text_start as usize;
                let g1 = g0 + g.text_len as usize;
                // Intersection of [g0, g1) with [lo, hi); a zero-width caret
                // window (lo == hi) selects nothing, which is correct.
                if g0 < hi && g1 > lo {
                    covered.push(GlyphRef::new(ri, gi));
                }
            }
        }
        covered
    }
}

/// The `[start, end)` byte range of the whitespace-delimited word of `text`
/// that contains byte offset `off` (Pass 14.3 double-click, UI spec §4.3).
///
/// `start` is just past the last Unicode-whitespace char strictly before
/// `off` (or 0), and `end` is the first whitespace char at or after `off`
/// (or `text.len()`). Both bounds are UTF-8 char boundaries, so they are
/// valid caret slots; in a simple font each code is one glyph whose text is
/// one code point, so a whitespace boundary is also a glyph boundary. `off`
/// is clamped into range first, so an out-of-range offset never panics. When
/// `off` sits on a whitespace char the returned range is the preceding word
/// (its `end` collapses onto `off`), which is the intuitive double-click
/// result on inter-word space.
fn word_bounds(text: &str, off: usize) -> (usize, usize) {
    let off = off.min(text.len());
    let mut start = 0usize;
    let mut end = text.len();
    for (i, c) in text.char_indices() {
        if i < off {
            if c.is_whitespace() {
                start = i + c.len_utf8();
            }
        } else if c.is_whitespace() {
            end = i;
            break;
        }
    }
    (start, end)
}

/// The median of `gaps` (sorts in place). `0.0` for an empty slice — a
/// column of one line has no leading to speak of, so nothing can exceed
/// it, which correctly yields one block.
fn median(gaps: &mut [f32]) -> f32 {
    if gaps.is_empty() {
        return 0.0;
    }
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = gaps.len() / 2;
    if gaps.len() % 2 == 1 {
        gaps.get(mid).copied().unwrap_or(0.0)
    } else {
        let lo = gaps.get(mid.wrapping_sub(1)).copied().unwrap_or(0.0);
        let hi = gaps.get(mid).copied().unwrap_or(0.0);
        (lo + hi) / 2.0
    }
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
    use crate::text_extract::{ExtractedGlyph, PageText, TextOrigin, TextRun};

    /// A `Glyphs` run built from `(char, x, y, advance, size)` tuples, at a
    /// given starting run text — enough to drive recognition without the
    /// whole extraction pipeline.
    fn glyph_run(chars: &[(&str, f32, f32, f32, f32)]) -> TextRun {
        let mut text = String::new();
        let mut glyphs = Vec::new();
        for &(c, x, y, adv, size) in chars {
            let start = text.len() as u32;
            text.push_str(c);
            glyphs.push(ExtractedGlyph {
                code: 0,
                rung: crate::text_extract::LadderRung::ToUnicode,
                text_start: start,
                text_len: c.len() as u32,
                x,
                y,
                advance: adv,
                size,
                direction: (1.0, 0.0),
                invisible: false,
                provenance: None,
            });
        }
        TextRun {
            text,
            origin: TextOrigin::Glyphs,
            glyphs,
            artifact: None,
            mcid: None,
            bbox: None,
        }
    }

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
        // `PageText` has a private field, so build it through `Default` and
        // set the public `runs` field rather than a struct literal.
        let mut p = PageText::default();
        p.runs = runs;
        p
    }

    #[test]
    fn one_line_is_one_block_one_column() {
        let p = page(vec![glyph_run(&[
            ("H", 72.0, 700.0, 6.0, 10.0),
            ("i", 78.0, 700.0, 4.0, 10.0),
        ])]);
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        assert_eq!(m.lines().len(), 1);
        assert_eq!(m.columns(), 1);
        assert_eq!(m.blocks().len(), 1);
        assert_eq!(m.diagnostics().glyphs_clustered, 2);
        assert_eq!(m.block_text(&m.blocks()[0]), "Hi");
    }

    #[test]
    fn a_leading_gap_splits_two_paragraphs() {
        // Three lines at 14-unit leading, then a 28-unit gap, then two more.
        let runs = vec![
            glyph_run(&[("a", 72.0, 740.0, 6.0, 10.0)]),
            line_break(),
            glyph_run(&[("b", 72.0, 726.0, 6.0, 10.0)]),
            line_break(),
            glyph_run(&[("c", 72.0, 712.0, 6.0, 10.0)]),
            line_break(),
            glyph_run(&[("d", 72.0, 684.0, 6.0, 10.0)]), // 28-unit gap
            line_break(),
            glyph_run(&[("e", 72.0, 670.0, 6.0, 10.0)]),
        ];
        let p = page(runs);
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        assert_eq!(m.lines().len(), 5);
        assert_eq!(m.columns(), 1);
        assert_eq!(m.blocks().len(), 2, "the 2x leading gap starts a paragraph");
        assert_eq!(m.diagnostics().paragraph_breaks_by_leading, 1);
    }

    #[test]
    fn two_x_bands_are_two_columns_ordered_left_to_right() {
        // Right column emitted FIRST in content order; recognition must
        // still order the bands left-to-right.
        let runs = vec![
            glyph_run(&[("R", 322.0, 740.0, 6.0, 10.0)]),
            line_break(),
            glyph_run(&[("L", 72.0, 740.0, 6.0, 10.0)]),
        ];
        let p = page(runs);
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        assert_eq!(m.columns(), 2);
        // The line at x=72 must be column 0, the one at x=322 column 1.
        let left = m.lines().iter().find(|l| l.bbox.llx < 100.0).unwrap();
        let right = m.lines().iter().find(|l| l.bbox.llx > 300.0).unwrap();
        assert_eq!(left.column, 0);
        assert_eq!(right.column, 1);
        assert!(m.diagnostics().is_multi_column());
    }

    #[test]
    fn hit_test_lands_on_a_glyph_boundary() {
        let p = page(vec![glyph_run(&[
            ("H", 72.0, 700.0, 6.0, 10.0),
            ("i", 78.0, 700.0, 4.0, 10.0),
        ])]);
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        // A point in the left third of "H" resolves to its leading edge.
        let pos = m.hit_test(73.0, 702.0).unwrap();
        assert_eq!(pos, TextPosition::new(0, 0));
        // A point past "i" resolves to the trailing edge (offset 2 bytes).
        let end = m.hit_test(90.0, 702.0).unwrap();
        assert_eq!(end, TextPosition::new(0, 2));
    }

    #[test]
    fn resolve_range_covers_the_selected_glyphs() {
        let p = page(vec![glyph_run(&[
            ("H", 72.0, 700.0, 6.0, 10.0),
            ("e", 78.0, 700.0, 6.0, 10.0),
            ("y", 84.0, 700.0, 6.0, 10.0),
        ])]);
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        // Select bytes [0, 2): the first two glyphs.
        let covered = m.resolve_range(TextPosition::new(0, 0), TextPosition::new(0, 2));
        assert_eq!(covered, vec![GlyphRef::new(0, 0), GlyphRef::new(0, 1)]);
        // Order-insensitive.
        let rev = m.resolve_range(TextPosition::new(0, 2), TextPosition::new(0, 0));
        assert_eq!(rev, covered);
    }

    #[test]
    fn artifact_runs_are_excluded_and_counted() {
        let mut art = glyph_run(&[("1", 300.0, 40.0, 6.0, 10.0)]);
        art.artifact = Some(crate::text_extract::ArtifactKind::Pagination);
        let p = page(vec![glyph_run(&[("A", 72.0, 700.0, 6.0, 10.0)]), art]);
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        assert_eq!(m.diagnostics().artifact_runs_skipped, 1);
        // Only the body glyph was clustered.
        assert_eq!(m.diagnostics().glyphs_clustered, 1);
    }

    #[test]
    fn sourced_view_is_the_untouched_page() {
        let p = page(vec![glyph_run(&[("A", 72.0, 700.0, 6.0, 10.0)])]);
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        assert_eq!(m.sourced_view().sourced_text(), "A");
    }

    // -- boundary accessors (Pass 14.3, UI spec §4.3/§4.5) --------------

    #[test]
    fn line_at_maps_a_caret_back_to_its_line() {
        // Two lines; a caret in either must resolve to that line's index.
        let runs = vec![
            glyph_run(&[("a", 72.0, 740.0, 6.0, 10.0), ("b", 78.0, 740.0, 6.0, 10.0)]),
            line_break(),
            glyph_run(&[("c", 72.0, 726.0, 6.0, 10.0)]),
        ];
        let p = page(runs);
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        assert_eq!(m.lines().len(), 2);
        // Caret at the start of run 0 ("a") -> line 0.
        assert_eq!(m.line_at(TextPosition::new(0, 0)), Some(0));
        // Caret on the trailing boundary of "b" (offset 2) -> still line 0.
        assert_eq!(m.line_at(TextPosition::new(0, 2)), Some(0));
        // Caret in run 2 ("c") -> line 1.
        assert_eq!(m.line_at(TextPosition::new(2, 0)), Some(1));
        // A run that carries no clustered glyph resolves to nothing.
        assert_eq!(m.line_at(TextPosition::new(99, 0)), None);
    }

    #[test]
    fn line_range_at_spans_first_to_last_glyph_boundary() {
        let p = page(vec![glyph_run(&[
            ("H", 72.0, 700.0, 6.0, 10.0),
            ("i", 78.0, 700.0, 4.0, 10.0),
        ])]);
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        let (start, end) = m.line_range_at(TextPosition::new(0, 1)).unwrap();
        assert_eq!(start, TextPosition::new(0, 0)); // leading edge of "H"
        assert_eq!(end, TextPosition::new(0, 2)); // trailing edge of "i"
    }

    #[test]
    fn word_range_at_splits_on_whitespace_within_the_run() {
        // One run holding two words separated by a space.
        let p = page(vec![glyph_run(&[
            ("t", 72.0, 700.0, 6.0, 10.0),
            ("h", 78.0, 700.0, 6.0, 10.0),
            ("e", 84.0, 700.0, 6.0, 10.0),
            (" ", 90.0, 700.0, 4.0, 10.0),
            ("c", 94.0, 700.0, 6.0, 10.0),
            ("a", 100.0, 700.0, 6.0, 10.0),
            ("t", 106.0, 700.0, 6.0, 10.0),
        ])]);
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        // A caret inside "the" (byte 1) selects [0, 3).
        let (a, b) = m.word_range_at(TextPosition::new(0, 1));
        assert_eq!((a.byte_offset, b.byte_offset), (0, 3));
        // A caret inside "cat" (byte 5) selects [4, 7).
        let (a, b) = m.word_range_at(TextPosition::new(0, 5));
        assert_eq!((a.byte_offset, b.byte_offset), (4, 7));
        // Both ends stay in the same run — always an editable single-run span.
        assert_eq!(a.run, 0);
        assert_eq!(b.run, 0);
    }

    #[test]
    fn word_range_at_is_panic_free_for_a_stale_run() {
        let p = page(vec![glyph_run(&[("A", 72.0, 700.0, 6.0, 10.0)])]);
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        let pos = TextPosition::new(99, 3);
        assert_eq!(m.word_range_at(pos), (pos, pos));
    }

    #[test]
    fn block_at_maps_caret_to_its_paragraph() {
        // Two paragraphs separated by a blank line (a leading gap), so the
        // default recogniser makes two blocks. A caret in each resolves to a
        // distinct block index; `block_at` == `lines()[line_at].block`.
        // Each paragraph is two closely-spaced lines (14pt leading); a wide
        // blank gap separates them so the leading-gap rule splits the blocks.
        let p = page(vec![
            glyph_run(&[("A", 72.0, 700.0, 6.0, 10.0), ("b", 78.0, 700.0, 6.0, 10.0)]),
            line_break(),
            glyph_run(&[("A", 72.0, 686.0, 6.0, 10.0), ("b", 78.0, 686.0, 6.0, 10.0)]),
            line_break(),
            glyph_run(&[("C", 72.0, 620.0, 6.0, 10.0), ("d", 78.0, 620.0, 6.0, 10.0)]),
            line_break(),
            glyph_run(&[("C", 72.0, 606.0, 6.0, 10.0), ("d", 78.0, 606.0, 6.0, 10.0)]),
        ]);
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        assert!(
            m.blocks().len() >= 2,
            "two paragraphs -> two blocks: {}",
            m.blocks().len()
        );
        let top = m.hit_test(74.0, 700.0).expect("hit top");
        let bot = m.hit_test(74.0, 620.0).expect("hit bottom");
        let bt = m.block_at(top).expect("block for top caret");
        let bb = m.block_at(bot).expect("block for bottom caret");
        assert_ne!(bt, bb, "carets in different paragraphs -> different blocks");
        // Agrees with the hand composition it is sugar over.
        let li = m.line_at(top).unwrap();
        assert_eq!(bt, m.lines()[li].block);
        // An out-of-range run resolves to None, never panics.
        assert_eq!(m.block_at(TextPosition::new(9999, 0)), None);
    }

    // -- caret navigation (Pass 14.4, UI spec §4.5) --------------------

    /// Two lines, each two glyphs, stacked in one column: line 0 ("Hi") at
    /// baseline 700, line 1 ("yo") at baseline 686. Enough to exercise
    /// Left/Right across the run/line boundary and Up/Down nearest-x.
    fn two_line_model_page() -> PageText {
        page(vec![
            glyph_run(&[("H", 72.0, 700.0, 6.0, 10.0), ("i", 78.0, 700.0, 4.0, 10.0)]),
            line_break(),
            glyph_run(&[("y", 72.0, 686.0, 6.0, 10.0), ("o", 78.0, 686.0, 6.0, 10.0)]),
        ])
    }

    #[test]
    fn caret_left_right_step_glyph_boundaries_and_cross_the_line_break() {
        let p = two_line_model_page();
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        // Right from the start of "H" walks the two boundaries of run 0…
        let p0 = TextPosition::new(0, 0);
        let p1 = m.caret_right(p0);
        assert_eq!(p1, TextPosition::new(0, 1)); // between H and i
        let p2 = m.caret_right(p1);
        assert_eq!(p2, TextPosition::new(0, 2)); // trailing edge of "i"
        // …then crosses the DerivedLineBreak (run 1, empty) to run 2's start.
        let p3 = m.caret_right(p2);
        assert_eq!(p3, TextPosition::new(2, 0)); // leading edge of "y"
        // Left is the exact inverse, crossing back over the break.
        assert_eq!(m.caret_left(p3), p2);
        assert_eq!(m.caret_left(p2), p1);
        // Clamp: Left at the very first slot stays put, never wraps/panics.
        assert_eq!(m.caret_left(p0), p0);
        // Clamp: Right at the very last slot stays put.
        let last = TextPosition::new(2, 2);
        assert_eq!(m.caret_right(last), last);
    }

    #[test]
    fn caret_up_down_land_on_the_nearest_x_of_the_adjacent_line() {
        let p = two_line_model_page();
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        // Caret on the trailing edge of "i" (line 0), x ≈ 82. Its desired-x
        // carried DOWN lands on the nearest slot of line 1 — the trailing edge
        // of "o" (x ≈ 84), i.e. offset 2 in run 2.
        let start = TextPosition::new(0, 2);
        let x = m.caret_x(start).expect("caret_x for a real boundary");
        let down = m.caret_down(start, x);
        assert_eq!(down, TextPosition::new(2, 2));
        // Back UP from there returns to line 0's nearest slot (trailing "i").
        let x2 = m.caret_x(down).expect("caret_x");
        assert_eq!(m.caret_up(down, x2), TextPosition::new(0, 2));
        // Up from the TOP line has nowhere to go → stays put (no panic).
        assert_eq!(m.caret_up(start, x), start);
        // Down from the BOTTOM line likewise stays put.
        let bot = TextPosition::new(2, 0);
        let xb = m.caret_x(bot).expect("caret_x");
        assert_eq!(m.caret_down(bot, xb), bot);
    }

    #[test]
    fn caret_x_reads_leading_and_trailing_glyph_edges() {
        let p = two_line_model_page();
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        // Leading edge of "H" is its origin x; trailing edge of "i" is x+adv.
        assert_eq!(m.caret_x(TextPosition::new(0, 0)), Some(72.0));
        assert_eq!(m.caret_x(TextPosition::new(0, 2)), Some(82.0));
        // A stale run / a non-boundary offset yields None, never panics.
        assert_eq!(m.caret_x(TextPosition::new(99, 0)), None);
    }

    #[test]
    fn caret_on_line_nearest_x_clamps_to_a_line_and_is_bounds_checked() {
        let p = two_line_model_page();
        let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
        // A far-left x on line 0 clamps to that line's leading edge.
        assert_eq!(
            m.caret_on_line_nearest_x(0, 0.0),
            Some(TextPosition::new(0, 0))
        );
        // A far-right x clamps to the trailing edge.
        assert_eq!(
            m.caret_on_line_nearest_x(0, 9999.0),
            Some(TextPosition::new(0, 2))
        );
        // An out-of-range line index resolves to None, never panics.
        assert_eq!(m.caret_on_line_nearest_x(999, 50.0), None);
    }
}

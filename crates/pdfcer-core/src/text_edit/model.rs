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
//! glyphs into `TextRun`s and inserted derived line breaks from two
//! geometry signals — a baseline move (rule 1) and a backward jump on one
//! baseline (rule 2, the two-column signal). Those breaks are re-used here
//! directly: a Pass-4 [`TextOrigin::DerivedLineBreak`](crate::text_extract::TextOrigin::DerivedLineBreak) run is a line
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
//!    ├─ Stage 1  Lines    split at DerivedLineBreak runs, at a within-run
//!    │                    baseline jump > line_baseline_ratio·size
//!    │                    (defensive; a source that omitted breaks still
//!    │                    segments), and where the glyph's table cell
//!    │                    changes (recognize_with_cells). Then cut a non-cell
//!    │                    line at a column gutter: a forward gap of at least
//!    │                    gutter_min_em that lines up on gutter_min_lines
//!    │                    lines. Artifact runs are excluded + counted;
//!    │                    ActualText runs are counted atomic (no glyphs to
//!    │                    split — §14.9.4 N4 makes per-char mapping
//!    │                    impossible).
//!    │
//!    ├─ Stage 2  Columns  cluster lines whose x-ranges overlap by at least
//!    │                    column_overlap_ratio of the narrower span; order
//!    │                    the resulting columns left-to-right (the derived
//!    │                    reading order for an untagged multi-column page,
//!    │                    §14.8.2.3.1). Body lines are banded before
//!    │                    cell lines, so a table cannot fuse two columns.
//!    │
//!    └─ Stage 3  Blocks   within each column (top-to-bottom), start a new
//!                         paragraph when the baseline gap exceeds the
//!                         column's typical leading by paragraph_leading_ratio,
//!                         or when a line is indented from the column margin
//!                         by more than indent_ratio·size. Each table cell
//!                         holding text is one BlockKind::TableCell block.
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

mod cells;
mod gutter;
mod navigate;
mod stages;

pub use cells::{CellRegion, detect_cell_regions};

use crate::page_tree::Rect;
use crate::text_extract::{ExtractedGlyph, GlyphProvenance, PageText};

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
/// [`TextOrigin::DerivedLineBreak`](crate::text_extract::TextOrigin::DerivedLineBreak) runs — plus the defensive
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
    /// **Meaningful as a *baseline* only when [`Self::direction`] is
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
    /// Which [`EditableTextModel::blocks`] entry this line belongs to.
    pub block: usize,
    /// Which [`EditableTextModel::cells`] entry this line lies in, or `None`
    /// outside every table cell. A line never spans two cells.
    pub cell: Option<usize>,
}

/// What kind of block was recognized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BlockKind {
    /// A paragraph: vertically-adjacent lines in one column, bounded by a
    /// leading gap or an indent.
    Paragraph,
    /// Every line inside one table cell. The cell's rectangle is
    /// [`Block::cell_rect`]; the fields locate it in its table with the
    /// meaning they have on [`CellRegion`].
    TableCell {
        /// Which table on the page ([`CellRegion::table`]).
        table: usize,
        /// The cell's first row, 0 at the top.
        row: usize,
        /// The cell's first column, 0 at the left.
        column: usize,
    },
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
    /// The block's kind.
    pub kind: BlockKind,
    /// Which column band this block sits in (for a table cell, the band of
    /// its first line).
    pub column: usize,
    /// Indices into [`EditableTextModel::lines`], top-to-bottom. Not a
    /// contiguous range: a column's lines are a subset of the global,
    /// content-ordered line list.
    pub line_indices: Vec<usize>,
    /// Bounding box in default user space — the union of the block's
    /// lines' boxes.
    pub bbox: Rect,
    /// The cell's rectangle when [`Self::kind`] is [`BlockKind::TableCell`],
    /// else `None`. Reflow wraps a cell block at this rectangle's inner
    /// width and reports overflow past its bottom without moving it.
    pub cell_rect: Option<Rect>,
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
    /// Line splits made because consecutive glyphs fall in different table
    /// cells (or one in a cell and one outside).
    pub lines_split_by_cell: u64,
    /// Line cuts made at a column gutter
    /// ([`BlockRecognitionOptions::gutter_min_em`]). DERIVED.
    pub lines_split_by_gutter: u64,
    /// Blocks made from table cells ([`BlockKind::TableCell`]).
    pub table_cell_blocks: u64,
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
    /// **Measured PERPENDICULAR TO THE LINE, not along the page's y
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
    /// A forward gap inside a horizontal line at least this many ems wide
    /// is a column-gutter candidate. Default `1.5`, well past a justified
    /// word space; `f32::INFINITY` disables the gutter rule.
    pub gutter_min_em: f32,
    /// A gutter candidate cuts its line only when at least this many lines,
    /// itself included, carry a candidate overlapping it by half an em.
    /// Default `3`, so one wide gap (a right-aligned page number) is left
    /// alone; a tab-aligned list or form of 3+ rows is cut into bands.
    pub gutter_min_lines: usize,
}

impl Default for BlockRecognitionOptions {
    fn default() -> Self {
        Self {
            column_overlap_ratio: 0.25,
            paragraph_leading_ratio: 0.5,
            indent_ratio: 1.0,
            line_baseline_ratio: 0.30,
            same_direction_cos: crate::text_extract::SAME_DIRECTION_COS,
            gutter_min_em: 1.5,
            gutter_min_lines: 3,
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
    cells: Vec<CellRegion>,
    diagnostics: BlockDiagnostics,
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
        Self::recognize_with_cells(page, options, &[])
    }

    /// [`Self::recognize`], with the page's table cells as hard boundaries:
    /// a line never joins glyphs from two cells, and each cell holding text
    /// becomes one [`BlockKind::TableCell`] block, after every paragraph
    /// block. With no cells this is exactly [`Self::recognize`].
    ///
    /// Get the cells from [`detect_cell_regions`] (ruled tables) or build
    /// them with [`CellRegion::from_tables`]. Reflow resolves block indices
    /// against [`detect_cell_regions`] and
    /// [`reflow_recognition_options`](super::reflow_recognition_options), so
    /// a shell naming a block for reflow builds its model the same way.
    #[must_use]
    pub fn recognize_with_cells(
        page: &'a PageText,
        options: &BlockRecognitionOptions,
        cells: &[CellRegion],
    ) -> Self {
        let mut diagnostics = BlockDiagnostics::default();
        let raw_lines = Self::cluster_lines(page, options, cells, &mut diagnostics);
        let raw_lines = gutter::split_gutters(page, raw_lines, options, &mut diagnostics);
        let (mut lines, columns) = Self::cluster_columns(raw_lines, options);
        let blocks = Self::segment_blocks(&mut lines, columns, cells, options, &mut diagnostics);

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
        if diagnostics.table_cell_blocks > 0 {
            diagnostics.note(format!(
                "text-blocks: {} table cell(s) became blocks; cell boundaries come from table \
                 detection, and no line joins text across them",
                diagnostics.table_cell_blocks
            ));
        }
        if diagnostics.lines_split_by_gutter > 0 {
            diagnostics.note(format!(
                "text-blocks: {} line(s) were cut at a column gutter, a wide gap that lines up \
                 on several lines (DERIVED)",
                diagnostics.lines_split_by_gutter
            ));
        }

        Self {
            page,
            lines,
            blocks,
            columns,
            cells: cells.to_vec(),
            diagnostics,
        }
    }

    // -- Accessors --------------------------------------------------------

    /// The recognized blocks: paragraphs in column-major, top-to-bottom
    /// order, then table cells by table, row and column.
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// The table cells this model was recognized against;
    /// [`Line::cell`] indexes them. Empty after [`Self::recognize`].
    #[must_use]
    pub fn cells(&self) -> &[CellRegion] {
        &self.cells
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
}

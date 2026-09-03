//! # Imposition — N-up, booklet and poster, as pure geometry
//!
//! Acrobat's *Page Sizing & Handling* control is four mutually-exclusive
//! top-level modes, not a flat list of percentages
//! (`Acrobat_Features/printing__scaling_modes.md`, addendum 2026-08-10):
//!
//! | Mode | What it does | Where it lives |
//! |---|---|---|
//! | **Size** | Fit / Actual Size / Shrink Oversized / Custom Scale | [`crate::place_page`] |
//! | **Poster** | ONE oversized page across MANY sheets | [`plan_poster`] |
//! | **Multiple** (N-up) | MANY pages onto ONE sheet | [`plan_n_up`] |
//! | **Booklet** | pages remapped for fold-and-bind assembly | [`plan_booklet`] |
//!
//! `place_page` already covers Size. This module covers the other three,
//! and covers them as **arithmetic only**: it takes a printable area and a
//! list of page sizes in points, and returns rectangles. It rasterises
//! nothing, opens no device, and names no platform type.
//!
//! ## ★ Why there is not one `cfg(windows)` in this file
//!
//! The crate note above [`crate::place_page`] says the geometry half of
//! this crate stays un-gated so it compiles **and its tests run** on the
//! Linux and macOS CI jobs. That is not a portability aspiration; it is
//! how the most test-worthy code in the crate stays covered.
//!
//! The failure mode is specific and was hit once already, on
//! 2026-08-10: a planning function took a `cfg(windows)` type as a
//! parameter, which silently moved the function — and every test of it —
//! behind a `cfg` that two of the three CI jobs do not build. **The tests
//! still passed on Windows and simply stopped existing elsewhere.**
//! Nothing reports that. [`crate::DeviceGeometry`] exists precisely so
//! planning never has to name `PrinterCaps`, and this module inherits the
//! same discipline: it takes `(f64, f64)` printable areas, never a
//! driver type.
//!
//! ## Coordinate convention (one convention, stated once)
//!
//! Every [`Rect`] in this module is in **points** (1/72 inch), with the
//! origin at the **top-left of the printable area**, `+x` to the right and
//! `+y` DOWNWARDS.
//!
//! That is deliberately *not* PDF's own convention (origin bottom-left,
//! `+y` up). It is chosen to match what already exists downstream:
//! `crate::blit_page` converts `Placement`'s offsets to GDI device
//! coordinates, whose origin is the top-left corner of the printable area
//! with `+y` down, and `tiny_skia`'s pixmaps are top-row-first for the same
//! reason. Introducing a second, y-up convention here would mean a flip on
//! every hand-off, and a flip that is applied twice — or zero times —
//! prints upside down, which is obvious on paper and invisible in every
//! test that does not print.
//!
//! Callers converting from PDF user space do the flip once, where they
//! already do it for rendering.
//!
//! ## What "refuse rather than guess" means here
//!
//! Every planner returns `Result<_, ImpositionError>`. The alternative —
//! clamping a nonsensical input to something plausible — is worse in this
//! specific domain than in most, because the output of this module becomes
//! **paper**. A poster whose overlap exceeds the sheet has no finite tiling;
//! a booklet of zero pages has no sheets; a 0-up grid has no cells. Each of
//! those, quietly "fixed", produces a job that looks like it worked and is
//! wrong in a way the operator discovers at the printer.
//!
//! The project's standing posture (`ARCHITECTURE.md`, redaction corollary,
//! and rule 4) is that content shown by mistake is arguable and content
//! lost by mistake is invisible. An imposition that silently drops a page
//! is exactly the invisible kind.
//!
//! ## What is sourced and what is judgement
//!
//! Sourced from `printing__scaling_modes.md` (verified 2026-08-10):
//! the four modes and their sub-options; N-up's four **Page order** values
//! by name; N-up's page-border rule being drawn "around each page's
//! **cell** in the grid"; booklet's four **Binding** values, its
//! **Sheets from/to** being a count of *physical sheets* rather than
//! document pages, and its three-valued **Booklet subset**; poster's
//! **Tile Scale / Overlap / Cut marks / Labels / Tile only large pages**;
//! and the statement that booklet is a real page-to-sheet remap while
//! N-up is not.
//!
//! Judgement calls this module had to make because the RAG records them as
//! GAPs or does not reach them at all — each is flagged again at its own
//! definition, and each is the kind of thing to re-check if a real Acrobat
//! ever becomes available:
//!
//! 1. **What "Reversed" reverses** in the four page orders. The RAG gives
//!    the four names and one example of the *axis* distinction, never the
//!    reversal's meaning. See [`PageOrder`].
//! 2. **How a pages-per-sheet COUNT becomes rows × columns.** The RAG
//!    lists the counts (2/4/6/9/16) and never the grid. See [`NUpGrid`].
//! 3. **The geometric difference between `Left` and `Left (Tall)`** —
//!    recorded as an explicit GAP in the RAG. See [`Binding`].
//! 4. **Which way an auto-rotated page turns** (this module always turns
//!    clockwise). See [`CellFit::rotated`].
//! 5. **Where a partial poster tile's content sits on its sheet** — this
//!    module puts it at the printable-area origin rather than centring it.
//!    See [`PosterTile::sheet_pt`].
//! 6. **What happens with "Tile only large pages" unchecked** on a job
//!    mixing oversized and normal pages — an explicit RAG GAP. See
//!    [`PosterSpec::tiles_page`].
//! 7. **Poster does not auto-rotate.** N-up's auto-rotate is a sourced
//!    sub-option; no equivalent is documented for Poster, so none is
//!    invented here.
//! 8. **No gutter between N-up cells.** Acrobat's dialog exposes no
//!    gutter control in the sourced material, so the cells tile the
//!    printable area exactly and a page border sits on the shared edge.

use crate::{ScaleMode, place_page};

/// Tolerance, in points, for "does this fit" comparisons.
///
/// Matches the constant [`crate::place_page`] uses for the same purpose,
/// and for the same reason: floating-point arithmetic can land a whisker
/// over a boundary and report an overflow nobody could see on paper. Half
/// a point is 1/144 inch — below what any printer resolves and far below
/// what an eye finds.
const EDGE_EPS_PT: f64 = 0.5;

/// Relative tolerance for "is this score strictly better", used when
/// choosing between candidate grids. Comparing two computed scales with
/// `>` alone makes the chosen grid depend on the last bit of a division.
const SCORE_EPS: f64 = 1e-9;

/// Upper bound on cells per sheet for an N-up grid.
///
/// Acrobat's own presets stop at 16. This ceiling is far above anything
/// legible — a 32×32 grid on A4 gives each page an 18×26 point cell — and
/// exists to bound two things rather than to express taste: the divisor
/// search in [`NUpGrid::Count`] resolution, and the allocation a caller
/// could provoke by passing a `u32` straight through from a text field.
pub const MAX_CELLS_PER_SHEET: u32 = 1024;

/// Upper bound on tiles a poster may produce, used when
/// [`PosterSpec::max_tiles`] is left at its default.
///
/// 400 A4 sheets is a poster roughly 8 by 12 metres. The ceiling exists
/// because tile scale is a free-entry percentage: an operator who means
/// 200% and types 2000% would otherwise queue a five-figure sheet count on
/// a shared device, and **spooling cannot be undone** (see the crate
/// docs). Refusing costs a re-typed number; not refusing costs a ream.
pub const DEFAULT_MAX_TILES: u32 = 400;

/// Upper bound on sheets a booklet may produce.
///
/// Purely an allocation guard. [`plan_booklet`]'s page count comes from a
/// real slice and cannot realistically reach this, but
/// [`booklet_pairing`] takes a bare count and is public, so a caller can
/// hand it `usize::MAX`.
pub const MAX_BOOKLET_SHEETS: usize = 100_000;

// ---------------------------------------------------------------------------
// Shared geometry
// ---------------------------------------------------------------------------

/// An axis-aligned rectangle in points, top-left origin, `+y` down.
///
/// See the module's coordinate-convention section. Stored as
/// origin-plus-extent rather than as two corners because every consumer of
/// this module (a GDI blit, a `tiny_skia` draw, a cut-mark stroke) wants a
/// position and a size, and a corner pair would be converted at every one
/// of them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    /// Distance from the printable area's left edge.
    pub x: f64,
    /// Distance from the printable area's top edge, increasing DOWNWARDS.
    pub y: f64,
    /// Extent along `+x`. Never negative in anything this module returns.
    pub width: f64,
    /// Extent along `+y`. Never negative in anything this module returns.
    pub height: f64,
}

impl Rect {
    /// Build a rectangle from an origin and an extent.
    #[must_use]
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The `x` coordinate of the right edge.
    #[must_use]
    pub fn right(self) -> f64 {
        self.x + self.width
    }

    /// The `y` coordinate of the bottom edge — the LARGER `y`, because the
    /// axis points down.
    #[must_use]
    pub fn bottom(self) -> f64 {
        self.y + self.height
    }

    /// Whether the rectangle encloses a positive area.
    ///
    /// Used at the boundaries of this module rather than sprinkled through
    /// it: a zero-extent cell is a caller error that should surface as an
    /// [`ImpositionError`], not as a placement of size zero that renders as
    /// a blank sheet with no explanation.
    #[must_use]
    pub fn is_positive(self) -> bool {
        self.width > 0.0 && self.height > 0.0 && self.width.is_finite() && self.height.is_finite()
    }
}

/// How one page sits inside one cell.
///
/// Produced by [`fit_into_cell`] and carried by both the N-up and the
/// booklet planners, so a caller writes the same placement code for both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellFit {
    /// The **footprint** the page occupies, in printable-area coordinates.
    ///
    /// Already accounts for rotation: when [`Self::rotated`] is true this
    /// rectangle is `height × width` of the source page times
    /// [`Self::scale`], not `width × height`. A caller that rotates the
    /// page and then places it by its unrotated size would produce a
    /// sideways page hanging out of its cell.
    pub rect: Rect,
    /// Multiplier from PDF points to paper points, same meaning as
    /// [`crate::Placement::scale`].
    pub scale: f64,
    /// Whether the page is turned a quarter turn **clockwise** inside its
    /// cell.
    ///
    /// # Which way, and why it is a judgement call
    ///
    /// Auto-rotate is a sourced N-up sub-option; its *direction* is not
    /// sourced anywhere, and both directions produce a page a reader can
    /// read by tilting their head the other way. What matters is that a
    /// job never mixes the two — a sheet with one page turned clockwise
    /// and its neighbour turned counter-clockwise is unreadable at any
    /// head angle. So this module always turns clockwise, everywhere, and
    /// records the choice here rather than leaving it implicit in a
    /// coordinate swap.
    pub rotated: bool,
}

/// Fit one page into one cell, centred, optionally turning it a quarter
/// turn if that makes it bigger.
///
/// # Why this delegates to [`crate::place_page`]
///
/// The fit-and-centre arithmetic already exists, is already tested, and
/// already handles the degenerate cases (a zero-extent page or cell yields
/// a finite scale rather than an infinity). Re-deriving it here would give
/// this crate two implementations of the same thing, and the symptom of
/// their drift would be an N-up cell placing a page a hair differently
/// from a full-sheet print of the same page — which nobody would think to
/// compare. So the cell is handed to `place_page` as if it were a tiny
/// sheet, and the resulting offsets are translated into the cell's origin.
///
/// # The rotation test is "does it gain scale", not "is it landscape"
///
/// Comparing orientations (portrait page into landscape cell → turn) is
/// the obvious rule and is wrong at the boundary: a nearly-square page in
/// a nearly-square cell flips on a rounding difference, and adjacent
/// almost-identical pages in a job would then face different ways.
/// Comparing the achieved scale, with a tolerance, answers the question
/// the operator actually has — *is the page bigger this way* — and is
/// stable because a tie keeps the page upright.
#[must_use]
pub fn fit_into_cell(page: (f64, f64), cell: Rect, auto_rotate: bool) -> CellFit {
    let upright = place_page(page, (cell.width, cell.height), ScaleMode::Fit);
    if auto_rotate {
        let turned = place_page((page.1, page.0), (cell.width, cell.height), ScaleMode::Fit);
        if turned.scale > upright.scale * (1.0 + SCORE_EPS) {
            return CellFit {
                rect: Rect::new(
                    cell.x + turned.offset_x_pt,
                    cell.y + turned.offset_y_pt,
                    page.1 * turned.scale,
                    page.0 * turned.scale,
                ),
                scale: turned.scale,
                rotated: true,
            };
        }
    }
    CellFit {
        rect: Rect::new(
            cell.x + upright.offset_x_pt,
            cell.y + upright.offset_y_pt,
            page.0 * upright.scale,
            page.1 * upright.scale,
        ),
        scale: upright.scale,
        rotated: false,
    }
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// Why an imposition could not be laid out.
///
/// Every variant carries the offending value. An imposition failure is
/// almost always a number an operator typed, and "the overlap is too
/// large" without saying *how* large — or what it is too large for —
/// leaves them guessing at a field they can see.
///
/// Deliberately NOT `Eq`: several variants carry `f64`, and `f64` is not
/// totally ordered. `PartialEq` is enough for the assertions callers and
/// tests actually make.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ImpositionError {
    /// No pages to impose. Distinct from "a booklet of blanks": a job with
    /// nothing in it is never what was meant, and producing an empty sheet
    /// list would look like success.
    #[error("there are no pages to impose")]
    NoPages,

    /// The printable area has no positive extent, so it has no cells.
    #[error(
        "the sheet's printable area is {width_pt} × {height_pt} points, which has no room to \
         place anything — check the printer's reported margins"
    )]
    EmptySheet {
        /// Printable width in points, as given.
        width_pt: f64,
        /// Printable height in points, as given.
        height_pt: f64,
    },

    /// A page's own size has no positive extent.
    #[error(
        "a page measures {width_pt} × {height_pt} points, which cannot be scaled or tiled — \
         the page's MediaBox is likely malformed"
    )]
    DegeneratePage {
        /// Page width in points, as given.
        width_pt: f64,
        /// Page height in points, as given.
        height_pt: f64,
    },

    /// A grid with no cells: zero pages per sheet, or a zero row/column
    /// count.
    #[error("an N-up grid needs at least one cell; a grid of zero cells would place no pages")]
    ZeroCells,

    /// More cells per sheet than [`MAX_CELLS_PER_SHEET`].
    #[error(
        "{requested} pages per sheet exceeds the ceiling of {limit}; at that density no page \
         would be legible and the sheet count is almost certainly a typo"
    )]
    TooManyCells {
        /// Cells the caller asked for.
        requested: u64,
        /// The ceiling that refused it.
        limit: u32,
    },

    /// A sheet range that selects nothing, e.g. `from` after `to`, or a
    /// `from` of zero (the control is 1-based).
    #[error(
        "sheets {from} to {to} selects no sheet; the range is 1-based and inclusive, so `from` \
         must be at least 1 and no greater than `to`"
    )]
    SheetRangeEmpty {
        /// First sheet requested.
        from: usize,
        /// Last sheet requested.
        to: usize,
    },

    /// A sheet range whose start is past the end of the imposed booklet.
    #[error(
        "sheets start at {from}, but this booklet only has {sheets} sheet(s) — nothing would \
         print"
    )]
    SheetRangeBeyondBooklet {
        /// First sheet requested.
        from: usize,
        /// Sheets the booklet actually has.
        sheets: usize,
    },

    /// More booklet sheets than [`MAX_BOOKLET_SHEETS`], or a page count
    /// whose padding overflows.
    #[error(
        "a booklet of {pages} pages needs more than {limit} sheets, which pdfcer will not lay out"
    )]
    BookletTooLarge {
        /// Pages requested.
        pages: usize,
        /// The ceiling that refused it.
        limit: usize,
    },

    /// A tile scale that is not a positive, finite multiplier.
    #[error(
        "a tile scale of {0} is not a usable magnification; it must be finite and greater than \
         zero (1.0 is 100%)"
    )]
    InvalidTileScale(f64),

    /// A negative overlap. Refused rather than clamped to zero, because a
    /// negative overlap describes a GAP between tiles — content that
    /// exists on the poster and lands on no sheet at all.
    #[error(
        "an overlap of {0} points is negative, which would leave a gap between tiles and lose \
         the content that falls in it"
    )]
    NegativeOverlap(f64),

    /// An overlap at least as large as the sheet, which makes the tile
    /// stride zero or negative — an infinite tiling.
    #[error(
        "an overlap of {overlap_pt} points is not smaller than the {sheet_pt}-point sheet, so \
         each tile would advance by nothing and the poster would never finish"
    )]
    OverlapExceedsSheet {
        /// The overlap requested.
        overlap_pt: f64,
        /// The printable extent it was measured against.
        sheet_pt: f64,
    },

    /// More tiles than [`PosterSpec::max_tiles`] allows.
    #[error(
        "this poster needs {tiles} sheets, past the limit of {limit}; raise the limit \
         deliberately or reduce the tile scale"
    )]
    TooManyTiles {
        /// Tiles the geometry produced.
        tiles: u64,
        /// The ceiling that refused it.
        limit: u32,
    },
}

// ---------------------------------------------------------------------------
// Multiple / N-up
// ---------------------------------------------------------------------------

/// The order pages are dealt into the grid's cells.
///
/// Acrobat offers four values by name — Horizontal, Horizontal Reversed,
/// Vertical, Vertical Reversed — and the RAG's example distinguishes only
/// the AXIS ("left-to-right-then-down vs. top-to-bottom-then-right"). What
/// "Reversed" reverses is **not sourced**.
///
/// # The interpretation this module implements, and why
///
/// Reversed mirrors the **columns**, so cells fill right-to-left. It does
/// not mirror the rows: reading still starts at the top.
///
/// Two readings were possible and one is far likelier. Mirroring columns
/// gives right-to-left reading order, which is what a Hebrew, Arabic or
/// vertical-Japanese document needs and is the reason a print dialog would
/// expose the option at all. Mirroring the whole traversal — starting at
/// the bottom-right and ending top-left — serves no reading order anyone
/// uses; it is what falls out of a naive `slots.reverse()`, which is a
/// property of an implementation rather than of a document.
///
/// If a real Acrobat ever contradicts this, the fix is four lines in
/// [`PageOrder::cell`] and nothing else — the traversal is deliberately
/// the ONLY place order is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageOrder {
    /// Left to right, then down. The default a Western reader expects.
    #[default]
    Horizontal,
    /// Right to left, then down.
    HorizontalReversed,
    /// Top to bottom, then right — the first column is filled before the
    /// second is started.
    Vertical,
    /// Top to bottom, then LEFT — the RIGHTMOST column is filled first.
    VerticalReversed,
}

impl PageOrder {
    /// Map a zero-based position within a sheet to its `(row, column)`.
    ///
    /// The single place cell order is decided. Every other part of the
    /// N-up planner works in `(row, column)` and is therefore
    /// order-agnostic — which is what makes a wrong order a four-line fix
    /// rather than a rewrite, and what makes it testable without
    /// constructing a layout at all.
    ///
    /// `rows` and `columns` must both be non-zero; the planner guarantees
    /// that before calling. A zero would divide by zero, so the degenerate
    /// case returns the origin cell rather than panicking — this module
    /// never panics on caller input.
    #[must_use]
    pub fn cell(self, position: usize, rows: usize, columns: usize) -> (usize, usize) {
        if rows == 0 || columns == 0 {
            return (0, 0);
        }
        match self {
            Self::Horizontal => (position / columns, position % columns),
            Self::HorizontalReversed => (position / columns, columns - 1 - (position % columns)),
            Self::Vertical => (position % rows, position / rows),
            Self::VerticalReversed => (position % rows, columns - 1 - (position / rows)),
        }
    }
}

/// How many cells a sheet is divided into.
///
/// # Why a count needs resolving at all
///
/// Acrobat's control is "pages per sheet" with presets 2/4/6/9/16 — a
/// COUNT. The RAG records the counts and never the grid, so how a count
/// becomes rows × columns is this module's decision.
///
/// [`Self::Count`] resolves it by trying every factor pair of `n` and
/// keeping the one that places the job's **first page** largest, rotation
/// included when [`NUpSpec::auto_rotate`] is set. That is not a heuristic
/// dressed up as an optimum: "pages per sheet" has exactly one sensible
/// meaning — get as much page onto the sheet as the count allows — and the
/// factor pairs of a small integer are few enough to enumerate exactly.
///
/// It also produces the layout people actually expect. Two A4 pages on an
/// A4 sheet: side by side gives each page 0.50 scale, stacked gives 0.50
/// too, and stacked-with-rotation gives 0.71. So 2-up turns the pages and
/// stacks them, which is what every 2-up print in the world looks like —
/// arrived at from the arithmetic rather than special-cased.
///
/// **The first page is the reference**, not an average or the largest.
/// A job is overwhelmingly one page size; where it is not, there is no
/// single right grid, and a rule that reads one page is at least one a
/// reader can predict from the document. Callers that want a specific grid
/// regardless of content have [`Self::Custom`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NUpGrid {
    /// A pages-per-sheet count; the planner picks the factor pair.
    Count(u32),
    /// An explicit grid, used exactly as given.
    Custom {
        /// Cells down the sheet.
        rows: u32,
        /// Cells across the sheet.
        columns: u32,
    },
}

impl Default for NUpGrid {
    /// Two pages per sheet — the smallest N-up that is actually N-up, and
    /// the first preset in Acrobat's list.
    fn default() -> Self {
        Self::Count(2)
    }
}

/// The Multiple / N-up mode's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NUpSpec {
    /// Cells per sheet, as a count or an explicit grid.
    pub grid: NUpGrid,
    /// The order cells are filled in.
    pub order: PageOrder,
    /// Draw a thin rule around each page's CELL.
    ///
    /// The cell, not the placed page — sourced wording, and the
    /// distinction is visible: a portrait page in a landscape cell leaves
    /// a gap between the rule and the page, and a border drawn round the
    /// page instead would make a grid of ragged boxes.
    pub border: bool,
    /// Turn a page a quarter turn inside its cell when that makes it
    /// bigger. Acrobat's "Auto-rotate pages", which the RAG notes is
    /// independent of the job-level portrait/landscape control and applies
    /// specifically within the N-up cell layout.
    pub auto_rotate: bool,
}

/// Where one source page lands under an N-up imposition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NUpSlot {
    /// Index into the `page_sizes` slice given to [`plan_n_up`] — i.e. the
    /// page's position in the PRINT sequence, not necessarily its position
    /// in the document. Callers that reordered pages first (a range, a
    /// reverse, [`crate::JobSpec::sequence`]) map back through their own
    /// sequence.
    pub source: usize,
    /// Zero-based index of the physical sheet this page is on.
    pub sheet: usize,
    /// Row within the grid, zero-based, counting DOWN from the top.
    pub row: usize,
    /// Column within the grid, zero-based, counting right from the left.
    pub column: usize,
    /// The cell's full rectangle.
    pub cell: Rect,
    /// Where the page itself lands inside that cell.
    pub fit: CellFit,
    /// The rectangle to stroke when [`NUpSpec::border`] is set, and `None`
    /// when it is not.
    ///
    /// Carried as an `Option` rather than left for the caller to derive
    /// from `border && cell` so the border rule lives in exactly one
    /// place. A caller that stroked `fit.rect` instead would produce
    /// Acrobat's *other* plausible behaviour, and the RAG says cell.
    pub border: Option<Rect>,
}

/// A complete N-up imposition.
#[derive(Debug, Clone, PartialEq)]
pub struct NUpLayout {
    /// Rows in the resolved grid.
    pub rows: usize,
    /// Columns in the resolved grid.
    pub columns: usize,
    /// Physical sheets the job will consume.
    pub sheets: usize,
    /// One slot per input page, in input order.
    pub slots: Vec<NUpSlot>,
}

/// Lay pages onto sheets in a grid.
///
/// `printable_pt` is the PRINTABLE area, not the physical sheet — the same
/// distinction [`crate::place_page`] documents, and for the same reason:
/// fitting to the paper size instead produces a grid whose outer edges the
/// hardware crops, which looks exactly like a pdfcer bug and is not one.
///
/// `page_sizes` is in **print order** — this function does not reorder
/// pages, only places them. Ordering is [`crate::JobSpec::sequence`]'s job,
/// and keeping the two separate is what lets "pages 5-1 reversed, 4-up"
/// mean the obvious thing without either function knowing about the other.
///
/// # Errors
///
/// - [`ImpositionError::NoPages`] when `page_sizes` is empty.
/// - [`ImpositionError::EmptySheet`] when the printable area has no
///   positive extent.
/// - [`ImpositionError::ZeroCells`] for a grid with no cells.
/// - [`ImpositionError::TooManyCells`] past [`MAX_CELLS_PER_SHEET`].
///
/// A page of degenerate size is NOT an error here: it is placed at scale
/// 1.0 in its cell like any other, because [`crate::place_page`] already
/// degrades that case safely, and refusing a whole 40-page job over one
/// malformed MediaBox would lose 39 good pages to save one bad one. Poster
/// mode, where a degenerate page has no tiling at all, does refuse — see
/// [`plan_poster`].
pub fn plan_n_up(
    printable_pt: (f64, f64),
    page_sizes: &[(f64, f64)],
    spec: &NUpSpec,
) -> Result<NUpLayout, ImpositionError> {
    let sheet = Rect::new(0.0, 0.0, printable_pt.0, printable_pt.1);
    if !sheet.is_positive() {
        return Err(ImpositionError::EmptySheet {
            width_pt: printable_pt.0,
            height_pt: printable_pt.1,
        });
    }
    // The reference page for grid resolution. Fetched before the grid is
    // resolved because `Count` needs it, and `first()` doubles as the empty
    // check — one lookup, no indexing, no separate `is_empty`.
    let Some(&reference) = page_sizes.first() else {
        return Err(ImpositionError::NoPages);
    };

    let (rows, columns) = resolve_grid(spec.grid, printable_pt, reference, spec.auto_rotate)?;
    let cells = rows.saturating_mul(columns);
    // `resolve_grid` guarantees this, but a zero here divides by zero three
    // lines down, and a guarantee that holds only because of code in
    // another function is worth one branch.
    if cells == 0 {
        return Err(ImpositionError::ZeroCells);
    }

    let cell_w = printable_pt.0 / columns as f64;
    let cell_h = printable_pt.1 / rows as f64;

    let slots: Vec<NUpSlot> = page_sizes
        .iter()
        .enumerate()
        .map(|(source, &size)| {
            let sheet_index = source / cells;
            let position = source % cells;
            let (row, column) = spec.order.cell(position, rows, columns);
            let cell = Rect::new(column as f64 * cell_w, row as f64 * cell_h, cell_w, cell_h);
            NUpSlot {
                source,
                sheet: sheet_index,
                row,
                column,
                cell,
                fit: fit_into_cell(size, cell, spec.auto_rotate),
                border: if spec.border { Some(cell) } else { None },
            }
        })
        .collect();

    Ok(NUpLayout {
        rows,
        columns,
        sheets: page_sizes.len().div_ceil(cells),
        slots,
    })
}

/// Turn an [`NUpGrid`] into a concrete `(rows, columns)`.
///
/// Split out from [`plan_n_up`] because the count-to-grid decision is the
/// part with a judgement in it (see [`NUpGrid`]), and a decision worth
/// documenting is worth testing on its own rather than only through a
/// whole layout.
fn resolve_grid(
    grid: NUpGrid,
    printable_pt: (f64, f64),
    reference: (f64, f64),
    auto_rotate: bool,
) -> Result<(usize, usize), ImpositionError> {
    match grid {
        NUpGrid::Custom { rows, columns } => {
            let cells = u64::from(rows) * u64::from(columns);
            if cells == 0 {
                return Err(ImpositionError::ZeroCells);
            }
            if cells > u64::from(MAX_CELLS_PER_SHEET) {
                return Err(ImpositionError::TooManyCells {
                    requested: cells,
                    limit: MAX_CELLS_PER_SHEET,
                });
            }
            Ok((rows as usize, columns as usize))
        }
        NUpGrid::Count(n) => {
            if n == 0 {
                return Err(ImpositionError::ZeroCells);
            }
            if n > MAX_CELLS_PER_SHEET {
                return Err(ImpositionError::TooManyCells {
                    requested: u64::from(n),
                    limit: MAX_CELLS_PER_SHEET,
                });
            }
            // Enumerate every factor pair and score each by how large the
            // reference page ends up. Rows ascend, so columns descend, so
            // the FIRST candidate at any score is the widest grid — and
            // `>` (not `>=`) keeps it. That makes the tie-break "prefer
            // more columns", which matters for 2-up on a square sheet
            // where both orientations score identically and an arbitrary
            // winner would flip between builds.
            let mut best: Option<((usize, usize), f64)> = None;
            for rows in 1..=n {
                if n % rows != 0 {
                    continue;
                }
                let columns = n / rows;
                let cell = Rect::new(
                    0.0,
                    0.0,
                    printable_pt.0 / f64::from(columns),
                    printable_pt.1 / f64::from(rows),
                );
                let score = fit_into_cell(reference, cell, auto_rotate).scale;
                let improves = best.is_none_or(|(_, previous)| score > previous + SCORE_EPS);
                if improves {
                    best = Some(((rows as usize, columns as usize), score));
                }
            }
            // `n >= 1` guarantees the (1, n) pair was scored, so `best` is
            // always populated. Written as a fallback rather than an
            // unwrap: this module never panics on caller input, and a 1×n
            // grid is the honest answer if the loop ever did find nothing.
            Ok(best.map_or((1, n as usize), |(g, _)| g))
        }
    }
}

// ---------------------------------------------------------------------------
// Booklet
// ---------------------------------------------------------------------------

/// Which edge the finished booklet is bound on, and therefore how a sheet
/// is split.
///
/// # ★ What the source does and does not say
///
/// Sourced: the four values exist, `Left` is the documented default
/// (book-style, for left-to-right reading text), and the "(Tall)" variants
/// "handle landscape-oriented source pages differently from the plain
/// Left/Right pair". Explicitly recorded as a **GAP**: the exact geometric
/// difference between `Left` and `Left (Tall)`.
///
/// # The interpretation this module implements
///
/// The four values decompose into two independent bits, which is what
/// makes four values rather than three or five:
///
/// - **Split axis** — plain `Left`/`Right` divide the sheet down the
///   middle into two side-by-side halves (a vertical fold line, the
///   ordinary book). The `Tall` variants divide it across the middle into
///   two stacked halves (a horizontal fold line), which is what a
///   landscape source page needs to end up upright in the finished
///   booklet.
/// - **Binding side** — `Left` puts the reader's first page in the half
///   that is second along the split axis (the RIGHT half, or the BOTTOM
///   half when stacked); `Right` mirrors it.
///
/// The pairing ARITHMETIC is identical for `Left` and `LeftTall` — only
/// the axis differs. That is the assumption most consistent with the
/// sourced statement that the Tall variants are about page *orientation*
/// rather than about page *order*, and it is isolated in
/// [`Binding::split`] so a correction touches one function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Binding {
    /// Fold down the middle; first page on the right. Acrobat's default.
    #[default]
    Left,
    /// Fold down the middle; first page on the left (right-to-left
    /// reading).
    Right,
    /// Fold across the middle; first page in the lower half.
    LeftTall,
    /// Fold across the middle; first page in the upper half.
    RightTall,
}

/// Which physical half of a sheet side a page occupies.
///
/// Named by position rather than by role (`Leading`/`Trailing`) because
/// the consumer of this is a renderer placing a rectangle, and "left" is
/// something it can check against the rectangle it was given. A role name
/// would have to be decoded through [`Binding`] at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetHalf {
    /// Left half of a side-by-side split.
    Left,
    /// Right half of a side-by-side split.
    Right,
    /// Upper half of a stacked split.
    Top,
    /// Lower half of a stacked split.
    Bottom,
}

impl Binding {
    /// Whether this binding stacks the two halves instead of placing them
    /// side by side.
    #[must_use]
    pub fn is_stacked(self) -> bool {
        matches!(self, Self::LeftTall | Self::RightTall)
    }

    /// Whether this binding mirrors the spread — i.e. puts the reader's
    /// first page in the FIRST half along the split axis rather than the
    /// second.
    #[must_use]
    pub fn is_mirrored(self) -> bool {
        matches!(self, Self::Right | Self::RightTall)
    }

    /// The two halves of a sheet side, in `(first, second)` order along
    /// the split axis, together with their names.
    ///
    /// `first` is the left half of a side-by-side split, or the upper half
    /// of a stacked one. The pairing in [`booklet_pairing`] is expressed
    /// in these same terms, so the two never need to agree about anything
    /// except which is first.
    #[must_use]
    pub fn split(self, printable_pt: (f64, f64)) -> ((SheetHalf, Rect), (SheetHalf, Rect)) {
        let (w, h) = printable_pt;
        if self.is_stacked() {
            (
                (SheetHalf::Top, Rect::new(0.0, 0.0, w, h / 2.0)),
                (SheetHalf::Bottom, Rect::new(0.0, h / 2.0, w, h / 2.0)),
            )
        } else {
            (
                (SheetHalf::Left, Rect::new(0.0, 0.0, w / 2.0, h)),
                (SheetHalf::Right, Rect::new(w / 2.0, 0.0, w / 2.0, h)),
            )
        }
    }
}

/// Which sides of the imposed sheets actually print.
///
/// The manual-duplex workflow: print [`Self::FrontOnly`], physically flip
/// the stack, print [`Self::BackOnly`] over the same paper. The RAG
/// records a well-documented real failure mode here — operators
/// mis-ordering or mis-flipping the stack between the two passes, giving
/// upside-down or out-of-sequence pages — and notes that disclosing the
/// exact flip instruction on screen is a cheap way to beat Acrobat at it.
/// That disclosure is a shell's job; this enum is what it keys off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BookletSubset {
    /// Both sides, in front-back-front-back order. Needs a duplex printer.
    #[default]
    BothSides,
    /// Front sides only — the first pass of a manual duplex job.
    FrontOnly,
    /// Back sides only — the second pass, after the stack is flipped.
    BackOnly,
}

/// Which side of a physical sheet a slot is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookletSide {
    /// The side printed first.
    Front,
    /// The reverse.
    Back,
}

/// The Booklet mode's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BookletSpec {
    /// Binding edge and split axis.
    pub binding: Binding,
    /// Which sides print.
    pub subset: BookletSubset,
    /// A 1-based, inclusive range of PHYSICAL SHEETS, or `None` for all.
    ///
    /// Sourced, and the sourced part is the trap: this is a count of
    /// sheets of the IMPOSED booklet, **not** a range of document pages.
    /// "Sheets 1 to 1" of a 40-page booklet prints the outermost sheet,
    /// which carries document pages 40, 1, 2 and 39 — which is exactly the
    /// point (a different paper stock for the cover), and exactly what a
    /// document-page reading would get wrong while looking right.
    ///
    /// Modelled as an `Option<(usize, usize)>` rather than a pair of
    /// sentinel zeros so that "all sheets" is a value rather than a
    /// convention someone has to know.
    pub sheets: Option<(usize, usize)>,
    /// Turn a page a quarter turn inside its half when that makes it
    /// bigger.
    ///
    /// Not sourced for booklet — Acrobat documents auto-rotate as an N-up
    /// sub-option. It is offered here because a booklet half is the most
    /// aggressively non-square cell this module produces (half a portrait
    /// sheet is 1:2.9), so a portrait page placed upright in one lands at
    /// roughly half the size it reaches turned. Left to the caller rather
    /// than forced, and flagged as pdfcer's own addition.
    pub auto_rotate: bool,
}

/// The two pages on one side of one sheet, in split-axis order.
///
/// `None` is a genuine blank — a position the fold requires and the
/// document does not fill. It is represented rather than omitted because
/// a blank half still consumes paper, still needs its cell known (a caller
/// may want to stamp it), and above all because a *missing* entry and a
/// *blank* entry would be indistinguishable to a caller counting slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BookletSpread {
    /// The left half (side-by-side split) or upper half (stacked split).
    pub first: Option<usize>,
    /// The right half, or lower half.
    pub second: Option<usize>,
}

/// One physical sheet of an imposed booklet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookletSheet {
    /// 1-based sheet number, matching the operator-facing "Sheets from /
    /// to" control.
    pub number: usize,
    /// The side printed first.
    pub front: BookletSpread,
    /// The reverse of that sheet.
    pub back: BookletSpread,
}

/// Compute the page-to-sheet pairing for a saddle-stitched booklet.
///
/// Returned as its own function, separate from any geometry, because the
/// pairing is the only genuinely non-obvious arithmetic in this module and
/// deserves to be checkable without constructing rectangles.
///
/// # The rule
///
/// A booklet is a stack of sheets folded together, so **each sheet carries
/// four pages: two from the front of the document and two from the back.**
/// Pad the document to a multiple of four with blanks at the END (so the
/// blanks land at the back of the finished booklet, where a reader expects
/// them), then for zero-based sheet `s` of `N` padded pages, in 1-based
/// page numbers:
///
/// ```text
/// front:  outer = N − 2s        inner = 2s + 1
/// back:   inner = 2s + 2        outer = N − 2s − 1
/// ```
///
/// For `N = 8` that is sheet 1 → front `8 | 1`, back `2 | 7`; sheet 2 →
/// front `6 | 3`, back `4 | 5`. Fold the stack and the pages read 1
/// through 8.
///
/// # ★ Why the front and back put the outer page on opposite halves
///
/// This is the part that gets written wrong. On the front side the outer
/// page (`8`) is on the LEFT and the inner page (`1`) on the right. Turn
/// that same sheet over about its vertical axis and left becomes right, so
/// on the back the page behind `1` — page `2` — is now on the LEFT and the
/// page behind `8` — page `7` — is on the right.
///
/// The naive version keeps the outer page on the same side for both faces.
/// It produces a booklet that is right on the front of every sheet and
/// transposed on every back, which reads correctly for the first two pages
/// and then alternates — the sort of wrong that survives a quick glance at
/// sheet one.
///
/// # Non-multiples of four
///
/// Padding is the whole answer, and it must be at the end. A 5-page
/// document pads to 8, so pages 6, 7 and 8 are blank: sheet 1 front is
/// `blank | 1`, back is `2 | blank`; sheet 2 front is `blank | 3`, back is
/// `4 | 5`. The blanks are scattered across both sheets — they are *not*
/// all on the last sheet — which is correct, because the fold interleaves
/// the front and back of the document. An implementation that grouped the
/// blanks onto the final sheet would produce a booklet that reads 1, 2, 3,
/// 4, 5 and then has a blank leaf in the middle.
///
/// # Errors
///
/// - [`ImpositionError::NoPages`] for a zero-page booklet. A booklet of
///   nothing has no sheets, and returning an empty sheet list would be
///   indistinguishable from success.
/// - [`ImpositionError::BookletTooLarge`] past [`MAX_BOOKLET_SHEETS`], or
///   if padding the count would overflow. This function takes a bare
///   count, so it is reachable with `usize::MAX`.
pub fn booklet_pairing(
    page_count: usize,
    binding: Binding,
) -> Result<Vec<BookletSheet>, ImpositionError> {
    if page_count == 0 {
        return Err(ImpositionError::NoPages);
    }
    let Some(padded) = page_count.div_ceil(4).checked_mul(4) else {
        return Err(ImpositionError::BookletTooLarge {
            pages: page_count,
            limit: MAX_BOOKLET_SHEETS,
        });
    };
    let sheet_count = padded / 4;
    if sheet_count > MAX_BOOKLET_SHEETS {
        return Err(ImpositionError::BookletTooLarge {
            pages: page_count,
            limit: MAX_BOOKLET_SHEETS,
        });
    }

    // 1-based page number -> zero-based index, or None if it is one of the
    // pad positions. The single place the blank rule lives.
    let source = |number: usize| -> Option<usize> {
        if number >= 1 && number <= page_count {
            Some(number - 1)
        } else {
            None
        }
    };

    let mirrored = binding.is_mirrored();
    // Assemble a spread from its (outer, inner) pair in READING terms and
    // let the binding decide which physical half each lands in. Doing the
    // mirror once, here, is what keeps `Right` from being a second copy of
    // the whole arithmetic — and a second copy is how the two bindings
    // drift apart.
    let spread = |left: usize, right: usize| -> BookletSpread {
        if mirrored {
            BookletSpread {
                first: source(right),
                second: source(left),
            }
        } else {
            BookletSpread {
                first: source(left),
                second: source(right),
            }
        }
    };

    let sheets = (0..sheet_count)
        .map(|s| BookletSheet {
            number: s + 1,
            // Front: outer page on the left, inner page on the right.
            front: spread(padded - 2 * s, 2 * s + 1),
            // Back: the flip swaps them — see this function's docs.
            back: spread(2 * s + 2, padded - 2 * s - 1),
        })
        .collect();
    Ok(sheets)
}

/// One page position on one side of one sheet of an imposed booklet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BookletSlot {
    /// 1-based sheet number, matching [`BookletSpec::sheets`].
    pub sheet: usize,
    /// Which face of the sheet.
    pub side: BookletSide,
    /// Which physical half of that face.
    pub half: SheetHalf,
    /// The half's rectangle. Present even for a blank, because a blank
    /// half still occupies paper a caller may want to mark.
    pub cell: Rect,
    /// The page placed here, as an index into `page_sizes`, or `None` for
    /// a fold-mandated blank.
    pub source: Option<usize>,
    /// Where that page lands in the half. `None` exactly when
    /// [`Self::source`] is.
    pub fit: Option<CellFit>,
}

/// A complete booklet imposition.
#[derive(Debug, Clone, PartialEq)]
pub struct BookletLayout {
    /// Sheets the whole booklet needs, BEFORE [`BookletSpec::sheets`]
    /// narrows the job. Reported separately from `slots` so a shell can
    /// say "sheets 1–2 of 7" rather than only what it is about to print.
    pub total_sheets: usize,
    /// Pages after padding to a multiple of four.
    pub padded_pages: usize,
    /// Blank positions the fold required. Worth surfacing: three blanks in
    /// a five-page booklet is correct and looks like a bug to an operator
    /// who has not thought about folding.
    pub blank_positions: usize,
    /// The slots that will actually print, in sheet order and, within a
    /// sheet, front side before back.
    pub slots: Vec<BookletSlot>,
}

/// Impose a booklet: pair the pages, split each sheet, place each page.
///
/// # Errors
///
/// - [`ImpositionError::NoPages`] for an empty document.
/// - [`ImpositionError::EmptySheet`] when the printable area has no
///   positive extent.
/// - [`ImpositionError::BookletTooLarge`] past [`MAX_BOOKLET_SHEETS`].
/// - [`ImpositionError::SheetRangeEmpty`] when [`BookletSpec::sheets`]
///   selects nothing — `from` of zero (the control is 1-based) or `from`
///   after `to`.
/// - [`ImpositionError::SheetRangeBeyondBooklet`] when the range starts
///   past the last sheet.
///
/// A range whose END overruns the booklet is CLAMPED rather than refused:
/// "sheets 3 to 99" of a five-sheet booklet is an operator asking for
/// everything from the third sheet on, and there is exactly one thing they
/// can mean. A range that *starts* past the end has no such reading — it
/// would print nothing — so it is refused.
pub fn plan_booklet(
    printable_pt: (f64, f64),
    page_sizes: &[(f64, f64)],
    spec: &BookletSpec,
) -> Result<BookletLayout, ImpositionError> {
    let sheet_rect = Rect::new(0.0, 0.0, printable_pt.0, printable_pt.1);
    if !sheet_rect.is_positive() {
        return Err(ImpositionError::EmptySheet {
            width_pt: printable_pt.0,
            height_pt: printable_pt.1,
        });
    }
    let sheets = booklet_pairing(page_sizes.len(), spec.binding)?;
    let total_sheets = sheets.len();
    let padded_pages = total_sheets * 4;

    let (from, to) = match spec.sheets {
        None => (1, total_sheets),
        Some((from, to)) => {
            if from == 0 || from > to {
                return Err(ImpositionError::SheetRangeEmpty { from, to });
            }
            if from > total_sheets {
                return Err(ImpositionError::SheetRangeBeyondBooklet {
                    from,
                    sheets: total_sheets,
                });
            }
            (from, to.min(total_sheets))
        }
    };

    let ((first_half, first_rect), (second_half, second_rect)) = spec.binding.split(printable_pt);

    // Placing one spread's two halves. Written once and used for both
    // faces: front and back differ only in which pages they carry, and a
    // second copy of the placement would be a second place for the
    // rotation rule to drift.
    let place = |sheet_number: usize, side: BookletSide, spread: BookletSpread| {
        [
            (first_half, first_rect, spread.first),
            (second_half, second_rect, spread.second),
        ]
        .map(|(half, cell, source)| BookletSlot {
            sheet: sheet_number,
            side,
            half,
            cell,
            source,
            fit: source
                .and_then(|i| page_sizes.get(i).copied())
                .map(|size| fit_into_cell(size, cell, spec.auto_rotate)),
        })
    };

    let mut slots = Vec::new();
    let mut blank_positions = 0usize;
    for sheet in &sheets {
        blank_positions += [
            sheet.front.first,
            sheet.front.second,
            sheet.back.first,
            sheet.back.second,
        ]
        .iter()
        .filter(|p| p.is_none())
        .count();

        if sheet.number < from || sheet.number > to {
            continue;
        }
        if spec.subset != BookletSubset::BackOnly {
            slots.extend(place(sheet.number, BookletSide::Front, sheet.front));
        }
        if spec.subset != BookletSubset::FrontOnly {
            slots.extend(place(sheet.number, BookletSide::Back, sheet.back));
        }
    }

    Ok(BookletLayout {
        total_sheets,
        padded_pages,
        blank_positions,
        slots,
    })
}

// ---------------------------------------------------------------------------
// Poster
// ---------------------------------------------------------------------------

/// The Poster mode's settings — one oversized page across many sheets.
///
/// The inverse problem from N-up, and materially more work than it, which
/// the RAG flags directly: poster tiling needs a shared coordinate
/// transform across N physical sheets plus overlap duplication, where
/// N-up is a composition into cells of one sheet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PosterSpec {
    /// Magnification applied to the whole poster BEFORE tiling, where
    /// `1.0` is 100%.
    ///
    /// Distinct from Size mode's Custom Scale (sourced): that fits ONE
    /// page to ONE printable area, this decides how big the assembled
    /// poster is and therefore how many sheets it takes.
    pub tile_scale: f64,
    /// Shared border, in points, duplicated onto adjacent tiles so the
    /// sheets can be aligned and taped without a gap at the seam.
    ///
    /// The RAG found no fixed Acrobat default — community sourcing
    /// suggests operators choose a low single-digit mm/inch value — so
    /// pdfcer does not invent one either; [`Default`] leaves it at zero and
    /// the shell asks.
    pub overlap_pt: f64,
    /// Emit trim geometry for cut marks. The marks themselves are drawn by
    /// the caller at [`PosterTile::trim_pt`]'s boundary.
    pub cut_marks: bool,
    /// Emit assembly labels. The text comes from
    /// [`PosterLayout::tile_label`]; the flag says whether to print it.
    pub labels: bool,
    /// Tile only pages that exceed the printable area; pages that already
    /// fit print normally, untiled, in the same job.
    ///
    /// See [`Self::tiles_page`] for the predicate and for what the source
    /// does not settle.
    pub tile_only_large_pages: bool,
    /// Refuse a poster needing more tiles than this.
    ///
    /// A field rather than a constant because it is a policy an operator
    /// may legitimately override for a genuine wall-sized job, and a hard
    /// constant would make that impossible rather than deliberate.
    /// [`Default`] sets [`DEFAULT_MAX_TILES`].
    pub max_tiles: u32,
}

impl Default for PosterSpec {
    /// 100%, no overlap, no marks, no labels, tile everything, default
    /// ceiling.
    ///
    /// `tile_only_large_pages` defaults to **false** — do exactly what was
    /// asked. Acrobat's own default state for that checkbox is not in the
    /// source material, and defaulting it on would silently pass some
    /// pages through untiled in a mode the operator explicitly chose.
    fn default() -> Self {
        Self {
            tile_scale: 1.0,
            overlap_pt: 0.0,
            cut_marks: false,
            labels: false,
            tile_only_large_pages: false,
            max_tiles: DEFAULT_MAX_TILES,
        }
    }
}

impl PosterSpec {
    /// Whether this page should be tiled at all.
    ///
    /// # The measurement is taken AFTER the tile scale
    ///
    /// A 200-point page at 800% is a 1600-point poster, which is larger
    /// than any sheet — so it is a large page, even though its MediaBox is
    /// smaller than the paper. Measuring the unscaled page instead would
    /// pass it through untiled and print the top-left corner of a poster
    /// eight times too big, silently.
    ///
    /// # What the source does not settle
    ///
    /// The RAG records an explicit GAP: the exact behaviour when the
    /// checkbox is UNCHECKED and a job mixes oversized and normal pages.
    /// pdfcer's answer is the one that needs no special case — with it
    /// unchecked every page is tiled, and a page that fits simply produces
    /// a 1×1 grid. The observable result for that page is the same sheet
    /// it would have got untiled, except that the tile scale applies to
    /// it, which is what selecting Poster mode asked for.
    #[must_use]
    pub fn tiles_page(self, page_pt: (f64, f64), printable_pt: (f64, f64)) -> bool {
        if !self.tile_only_large_pages {
            return true;
        }
        let w = page_pt.0 * self.tile_scale;
        let h = page_pt.1 * self.tile_scale;
        w > printable_pt.0 + EDGE_EPS_PT || h > printable_pt.1 + EDGE_EPS_PT
    }
}

/// One sheet of a poster.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PosterTile {
    /// Row in the tile grid, zero-based, counting down.
    pub row: usize,
    /// Column, zero-based, counting right.
    pub column: usize,
    /// The region of the SOURCE PAGE this tile shows, in the page's own
    /// points with a top-left origin and `+y` down.
    ///
    /// Already divided back through [`PosterSpec::tile_scale`], so a
    /// caller renders this rectangle of the page at
    /// `tile_scale × device-scale` and needs to know nothing about the
    /// poster's coordinate space. The rectangle is CLIPPED to the page, so
    /// a trailing tile that runs past the edge asks for only the part that
    /// exists rather than for pixels the page does not have.
    pub source_pt: Rect,
    /// Where that content lands on this sheet, in printable-area points.
    ///
    /// Anchored at the printable origin, NOT centred. A partial trailing
    /// tile therefore has its content in the top-left corner and blank
    /// paper to the right and below. Centring it is the plausible
    /// alternative and is wrong for assembly: the tiles are taped together
    /// on a grid, and a centred partial tile shifts its content away from
    /// the seam it has to meet. Not sourced either way; chosen from what
    /// the assembled poster needs.
    pub sheet_pt: Rect,
    /// The part of [`Self::sheet_pt`] to KEEP when trimming — the overlap
    /// band is removed from the LEADING edges only.
    ///
    /// The band shared by tiles `c` and `c+1` is the trailing overlap of
    /// `c` and the leading overlap of `c+1`; exactly one copy must go.
    /// Trimming the leading edge means the first tile of each row and
    /// column keeps its full margin, which is what you want at the outside
    /// of the poster. With no overlap this equals `sheet_pt`, and cut
    /// marks have nothing to mark.
    pub trim_pt: Rect,
}

/// A complete poster imposition for ONE source page.
#[derive(Debug, Clone, PartialEq)]
pub struct PosterLayout {
    /// Tile rows.
    pub rows: usize,
    /// Tile columns.
    pub columns: usize,
    /// The assembled poster's size in points, after the tile scale.
    pub poster_pt: (f64, f64),
    /// The overlap actually used, echoed so a caller drawing cut marks
    /// does not have to carry the spec alongside the layout.
    pub overlap_pt: f64,
    /// Whether to draw cut marks at [`PosterTile::trim_pt`].
    pub cut_marks: bool,
    /// Whether to print [`Self::tile_label`] on each sheet.
    pub labels: bool,
    /// The tiles, row-major: all of row 0 left to right, then row 1.
    pub tiles: Vec<PosterTile>,
}

impl PosterLayout {
    /// The assembly label for one tile.
    ///
    /// Sourced as "filename + tile position". The filename is the
    /// caller's — this crate has never seen a file — so it is a parameter
    /// rather than a field, which also keeps [`PosterSpec`] free of an
    /// owned `String` and therefore `Copy`.
    ///
    /// Row and column are reported 1-based and with their totals, because
    /// the label's whole job is to tell somebody holding a sheet where it
    /// goes in a pile of sheets, and "row 2" without "of 3" does not.
    #[must_use]
    pub fn tile_label(&self, tile: &PosterTile, document: &str) -> String {
        format!(
            "{document} — row {} of {}, column {} of {}",
            tile.row + 1,
            self.rows,
            tile.column + 1,
            self.columns
        )
    }
}

/// Tile one page across sheets.
///
/// # Errors
///
/// - [`ImpositionError::EmptySheet`] when the printable area has no
///   positive extent.
/// - [`ImpositionError::DegeneratePage`] when the page has none. Unlike
///   N-up, poster mode cannot degrade this gracefully — a zero-extent
///   poster has no tiles at all, and returning zero sheets for a page the
///   operator asked to print would look like success.
/// - [`ImpositionError::InvalidTileScale`] for a non-finite or
///   non-positive magnification.
/// - [`ImpositionError::NegativeOverlap`] — a negative overlap describes a
///   GAP between tiles, i.e. content that lands on no sheet.
/// - [`ImpositionError::OverlapExceedsSheet`] when the overlap is not
///   smaller than the printable extent, which makes the stride zero or
///   negative and the tiling infinite.
/// - [`ImpositionError::TooManyTiles`] past [`PosterSpec::max_tiles`].
///
/// Callers honouring [`PosterSpec::tile_only_large_pages`] should ask
/// [`PosterSpec::tiles_page`] first and print the page normally when it
/// answers false; this function tiles whatever it is given.
pub fn plan_poster(
    printable_pt: (f64, f64),
    page_pt: (f64, f64),
    spec: &PosterSpec,
) -> Result<PosterLayout, ImpositionError> {
    let (sheet_w, sheet_h) = printable_pt;
    if !Rect::new(0.0, 0.0, sheet_w, sheet_h).is_positive() {
        return Err(ImpositionError::EmptySheet {
            width_pt: sheet_w,
            height_pt: sheet_h,
        });
    }
    if !Rect::new(0.0, 0.0, page_pt.0, page_pt.1).is_positive() {
        return Err(ImpositionError::DegeneratePage {
            width_pt: page_pt.0,
            height_pt: page_pt.1,
        });
    }
    if !spec.tile_scale.is_finite() || spec.tile_scale <= 0.0 {
        return Err(ImpositionError::InvalidTileScale(spec.tile_scale));
    }
    if !spec.overlap_pt.is_finite() || spec.overlap_pt < 0.0 {
        return Err(ImpositionError::NegativeOverlap(spec.overlap_pt));
    }
    // Checked against BOTH axes even when the poster is one tile wide.
    // An overlap wider than the sheet is nonsense whether or not this
    // particular poster happens to hide it, and letting it through on the
    // narrow case means the same setting works on one document and refuses
    // on the next — which reads as a pdfcer bug rather than as bad input.
    if spec.overlap_pt >= sheet_w {
        return Err(ImpositionError::OverlapExceedsSheet {
            overlap_pt: spec.overlap_pt,
            sheet_pt: sheet_w,
        });
    }
    if spec.overlap_pt >= sheet_h {
        return Err(ImpositionError::OverlapExceedsSheet {
            overlap_pt: spec.overlap_pt,
            sheet_pt: sheet_h,
        });
    }

    let poster_w = page_pt.0 * spec.tile_scale;
    let poster_h = page_pt.1 * spec.tile_scale;
    let stride_x = sheet_w - spec.overlap_pt;
    let stride_y = sheet_h - spec.overlap_pt;

    let columns = tile_count(poster_w, spec.overlap_pt, stride_x);
    let rows = tile_count(poster_h, spec.overlap_pt, stride_y);
    let total = (rows as u64).saturating_mul(columns as u64);
    if total == 0 || total > u64::from(spec.max_tiles) {
        return Err(ImpositionError::TooManyTiles {
            tiles: total,
            limit: spec.max_tiles,
        });
    }

    let mut tiles = Vec::with_capacity(rows * columns);
    for row in 0..rows {
        for column in 0..columns {
            let x = column as f64 * stride_x;
            let y = row as f64 * stride_y;
            // Clip the trailing tile to the poster: asking a renderer for
            // page content past the page's own edge is asking for pixels
            // that do not exist, and different renderers invent different
            // things there.
            let w = (poster_w - x).min(sheet_w).max(0.0);
            let h = (poster_h - y).min(sheet_h).max(0.0);
            let lead_x = if column > 0 { spec.overlap_pt } else { 0.0 };
            let lead_y = if row > 0 { spec.overlap_pt } else { 0.0 };
            tiles.push(PosterTile {
                row,
                column,
                source_pt: Rect::new(
                    x / spec.tile_scale,
                    y / spec.tile_scale,
                    w / spec.tile_scale,
                    h / spec.tile_scale,
                ),
                sheet_pt: Rect::new(0.0, 0.0, w, h),
                trim_pt: Rect::new(lead_x, lead_y, (w - lead_x).max(0.0), (h - lead_y).max(0.0)),
            });
        }
    }

    Ok(PosterLayout {
        rows,
        columns,
        poster_pt: (poster_w, poster_h),
        overlap_pt: spec.overlap_pt,
        cut_marks: spec.cut_marks,
        labels: spec.labels,
        tiles,
    })
}

/// How many tiles cover `poster` points when each tile spans
/// `stride + overlap` and advances by `stride`.
///
/// # The derivation, because an off-by-one here costs a sheet per row
///
/// Tile `i` covers `[i·stride, i·stride + sheet)`. The last tile must
/// reach the poster's far edge, so `(n−1)·stride + sheet ≥ poster`, i.e.
/// `n ≥ (poster − overlap) / stride` once `sheet = stride + overlap` is
/// substituted. Hence the ceiling of that ratio, floored at one.
///
/// [`EDGE_EPS_PT`] is subtracted before the ceiling for the same reason
/// [`crate::place_page`] tolerates half a point: an exact fit that lands a
/// float's-breadth over the boundary would otherwise add a whole extra
/// sheet carrying a sliver of content nobody can see. A tolerance in the
/// tile count is worth more than one in a placement — the cost of being
/// wrong is a sheet of paper, not a rounding error.
fn tile_count(poster: f64, overlap: f64, stride: f64) -> usize {
    // `stride.is_finite() && stride > 0.0` rather than `!(stride > 0.0)`:
    // the negated form also catches NaN, but clippy rejects negated
    // comparisons on partially-ordered types for exactly the reason that
    // makes them tempting here — a reader cannot tell whether the NaN case
    // was intended or overlooked. Spelling both conditions out says it.
    if !stride.is_finite() || stride <= 0.0 || !poster.is_finite() {
        // Unreachable from `plan_poster`, which validates the stride
        // first. Returning one rather than panicking keeps this module's
        // "never panic on caller input" property true of every function in
        // it, not just the public ones.
        return 1;
    }
    let raw = (poster - overlap - EDGE_EPS_PT) / stride;
    if !raw.is_finite() {
        return 1;
    }
    let n = raw.ceil();
    if n <= 1.0 {
        1
    } else if n >= u32::MAX as f64 {
        // Saturate rather than wrap through an `as` cast. The caller's
        // `max_tiles` check refuses it immediately afterwards; what must
        // not happen is a huge count becoming a small one on the way.
        u32::MAX as usize
    } else {
        n as usize
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Compare two point measurements for practical equality.
///
/// A tenth of a point is 1/720 inch — three times finer than a 240-DPI
/// device can place a dot. Anything closer than this is a float artefact,
/// and anything further is a real placement difference.
#[cfg(test)]
fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// A4 in points, the reference page for most of these tests.
#[cfg(test)]
const A4: (f64, f64) = (595.0, 842.0);

/// A Letter sheet's printable area with a quarter-inch hardware margin all
/// round — the same figure `lib.rs`'s own tests use, so a reader comparing
/// the two is comparing like with like.
#[cfg(test)]
const LETTER_PRINTABLE: (f64, f64) = (576.0, 756.0);

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod n_up_tests {
    use super::{
        A4, ImpositionError, LETTER_PRINTABLE, MAX_CELLS_PER_SHEET, NUpGrid, NUpSpec, PageOrder,
        approx_eq, plan_n_up,
    };

    /// A 2×2 grid spec with an explicit order, so the ordering tests differ
    /// in exactly one field.
    fn two_by_two(order: PageOrder) -> NUpSpec {
        NUpSpec {
            grid: NUpGrid::Custom {
                rows: 2,
                columns: 2,
            },
            order,
            border: false,
            auto_rotate: false,
        }
    }

    /// The `(row, column)` each of four pages lands in, in input order.
    fn cells(order: PageOrder) -> Vec<(usize, usize)> {
        let sizes = [A4; 4];
        plan_n_up(LETTER_PRINTABLE, &sizes, &two_by_two(order))
            .expect("a 2x2 grid of four A4 pages is layable")
            .slots
            .iter()
            .map(|s| (s.row, s.column))
            .collect()
    }

    /// Horizontal fills left to right, then down.
    ///
    /// If this ever reads `(0,0) (1,0) (0,1) (1,1)` the horizontal and
    /// vertical arms have been transposed — a swap that still produces a
    /// full, tidy-looking sheet, with the pages in the wrong places.
    #[test]
    fn horizontal_fills_left_to_right_then_down() {
        assert_eq!(
            cells(PageOrder::Horizontal),
            vec![(0, 0), (0, 1), (1, 0), (1, 1)]
        );
    }

    /// Horizontal Reversed mirrors the COLUMNS and leaves the rows alone.
    ///
    /// The defect this catches is `slots.reverse()` — the tempting
    /// one-line implementation, which yields `(1,1) (1,0) (0,1) (0,0)` and
    /// starts the reading order at the BOTTOM of the sheet. Both orders
    /// place four pages on four cells; only one of them is a reading
    /// order anybody uses.
    #[test]
    fn horizontal_reversed_mirrors_the_columns_not_the_rows() {
        assert_eq!(
            cells(PageOrder::HorizontalReversed),
            vec![(0, 1), (0, 0), (1, 1), (1, 0)]
        );
    }

    /// Vertical fills a whole column before moving across.
    #[test]
    fn vertical_fills_a_column_before_moving_across() {
        assert_eq!(
            cells(PageOrder::Vertical),
            vec![(0, 0), (1, 0), (0, 1), (1, 1)]
        );
    }

    /// Vertical Reversed starts in the RIGHTMOST column, still top-down.
    ///
    /// Catches the same bottom-start defect as its horizontal twin, and
    /// additionally catches reversing the row index instead of the column
    /// index — which would give `(1,0) (0,0) (1,1) (0,1)`, a sheet that
    /// reads upwards.
    #[test]
    fn vertical_reversed_starts_at_the_rightmost_column() {
        assert_eq!(
            cells(PageOrder::VerticalReversed),
            vec![(0, 1), (1, 1), (0, 0), (1, 0)]
        );
    }

    /// The four orders are four PERMUTATIONS of the same cell set.
    ///
    /// Every order must visit each cell exactly once. An off-by-one in any
    /// arm produces a duplicate cell and a missing one — two pages stacked
    /// on top of each other and a blank quarter-sheet, which on a
    /// text-heavy page can look merely dense rather than broken.
    #[test]
    fn every_order_is_a_permutation_of_the_same_cells() {
        for order in [
            PageOrder::Horizontal,
            PageOrder::HorizontalReversed,
            PageOrder::Vertical,
            PageOrder::VerticalReversed,
        ] {
            let mut got = cells(order);
            got.sort_unstable();
            assert_eq!(
                got,
                vec![(0, 0), (0, 1), (1, 0), (1, 1)],
                "order {order:?} does not visit every cell exactly once"
            );
        }
    }

    /// Cells tile the printable area exactly — no gaps, no overhang.
    ///
    /// A gap here is the classic "I subtracted a margin I never added
    /// back" defect: the grid shrinks towards the top-left and the last
    /// row prints off the bottom of the paper.
    #[test]
    fn cells_tile_the_printable_area_exactly() {
        let sizes = [A4; 4];
        let layout = plan_n_up(LETTER_PRINTABLE, &sizes, &two_by_two(PageOrder::Horizontal))
            .expect("layable");
        let right = layout
            .slots
            .iter()
            .map(|s| s.cell.right())
            .fold(f64::MIN, f64::max);
        let bottom = layout
            .slots
            .iter()
            .map(|s| s.cell.bottom())
            .fold(f64::MIN, f64::max);
        assert!(approx_eq(right, LETTER_PRINTABLE.0), "right edge {right}");
        assert!(
            approx_eq(bottom, LETTER_PRINTABLE.1),
            "bottom edge {bottom}"
        );
        for slot in &layout.slots {
            assert!(approx_eq(slot.cell.width, LETTER_PRINTABLE.0 / 2.0));
            assert!(approx_eq(slot.cell.height, LETTER_PRINTABLE.1 / 2.0));
        }
    }

    /// A fifth page starts a second sheet, in the first cell of it.
    ///
    /// Catches a sheet index computed from a running counter that is never
    /// reset — the symptom being page 5 placed in cell 5 of a 4-cell grid,
    /// i.e. nowhere.
    #[test]
    fn pages_spill_onto_a_second_sheet_and_restart_the_grid() {
        let sizes = [A4; 5];
        let layout = plan_n_up(LETTER_PRINTABLE, &sizes, &two_by_two(PageOrder::Horizontal))
            .expect("layable");
        assert_eq!(layout.sheets, 2);
        assert_eq!(layout.slots[4].sheet, 1);
        assert_eq!((layout.slots[4].row, layout.slots[4].column), (0, 0));
        assert_eq!(layout.slots[3].sheet, 0);
    }

    /// Auto-rotate turns a page only when turning it gains scale, and the
    /// reported footprint is the TURNED one.
    ///
    /// Two defects live here. Rotating on an orientation comparison rather
    /// than a scale comparison flips near-square pages on a rounding
    /// difference. Reporting the unrotated width and height alongside
    /// `rotated: true` puts a landscape page's footprint through the side
    /// of its cell — the page renders sideways and clipped.
    #[test]
    fn auto_rotate_turns_a_page_only_when_it_gains_scale() {
        // One tall cell: half of a portrait sheet, split top/bottom.
        let spec = NUpSpec {
            grid: NUpGrid::Custom {
                rows: 2,
                columns: 1,
            },
            order: PageOrder::Horizontal,
            border: false,
            auto_rotate: true,
        };
        let sizes = [A4; 2];
        let turned = plan_n_up(LETTER_PRINTABLE, &sizes, &spec).expect("layable");
        let slot = &turned.slots[0];
        assert!(slot.fit.rotated, "a portrait page gains scale turned here");
        assert!(
            slot.fit.rect.width > slot.fit.rect.height,
            "a turned portrait page has a landscape footprint: {:?}",
            slot.fit.rect
        );

        let upright = plan_n_up(
            LETTER_PRINTABLE,
            &sizes,
            &NUpSpec {
                auto_rotate: false,
                ..spec
            },
        )
        .expect("layable");
        assert!(!upright.slots[0].fit.rotated);
        assert!(
            upright.slots[0].fit.scale < slot.fit.scale,
            "turning must be the bigger option, or the test proves nothing"
        );
    }

    /// A page whose cell already suits it is NOT turned.
    ///
    /// The other half of the previous test: an always-rotate bug passes
    /// that one and fails this one.
    #[test]
    fn auto_rotate_leaves_a_page_alone_when_turning_would_shrink_it() {
        let spec = NUpSpec {
            grid: NUpGrid::Custom {
                rows: 1,
                columns: 1,
            },
            order: PageOrder::Horizontal,
            border: false,
            auto_rotate: true,
        };
        let layout = plan_n_up(LETTER_PRINTABLE, &[A4], &spec).expect("layable");
        assert!(!layout.slots[0].fit.rotated);
    }

    /// The page border is the CELL, not the placed page.
    ///
    /// Sourced wording. Stroking `fit.rect` instead gives a grid of boxes
    /// of different sizes that do not line up — visibly wrong on a mixed
    /// page-size job and subtly wrong on a uniform one.
    #[test]
    fn the_page_border_is_the_cell_not_the_placed_page() {
        let spec = NUpSpec {
            border: true,
            ..two_by_two(PageOrder::Horizontal)
        };
        // A wide page in a tall cell, so the two rectangles genuinely
        // differ; with a well-matched page the test would pass either way.
        let layout = plan_n_up(LETTER_PRINTABLE, &[(400.0, 100.0); 4], &spec).expect("layable");
        let slot = &layout.slots[0];
        let border = slot.border.expect("border was requested");
        assert_eq!(border, slot.cell);
        assert!(
            border.height > slot.fit.rect.height + 1.0,
            "the cell must be taller than the placed page for this to prove anything"
        );
    }

    /// No border requested means no border geometry — not a zero-sized one.
    #[test]
    fn no_border_requested_yields_no_border_rectangle() {
        let layout = plan_n_up(
            LETTER_PRINTABLE,
            &[A4; 2],
            &two_by_two(PageOrder::Horizontal),
        )
        .unwrap();
        assert!(layout.slots.iter().all(|s| s.border.is_none()));
    }

    /// ★ A pages-per-sheet COUNT resolves to the factor pair that places
    /// the page largest — which for 2-up on a portrait sheet is two
    /// stacked, turned pages.
    ///
    /// The defect this names: hard-coding `1 × n`. It is the obvious
    /// reading of "2 pages per sheet" and it puts two full-height portrait
    /// pages side by side at half scale, where the stacked-and-turned
    /// layout gets 0.71 — a 40% larger page, on the layout everyone else's
    /// 2-up produces.
    #[test]
    fn a_count_grid_picks_the_factor_pair_that_places_the_page_largest() {
        let spec = NUpSpec {
            grid: NUpGrid::Count(2),
            order: PageOrder::Horizontal,
            border: false,
            auto_rotate: true,
        };
        let layout = plan_n_up(LETTER_PRINTABLE, &[A4; 2], &spec).expect("layable");
        assert_eq!((layout.rows, layout.columns), (2, 1));
        assert!(layout.slots[0].fit.rotated);
        assert!(
            layout.slots[0].fit.scale > 0.6,
            "stacked-and-turned should beat side-by-side's 0.5: {:?}",
            layout.slots[0].fit
        );
    }

    /// Square counts resolve squarely, which is the sanity check on the
    /// scoring: 4-up must be 2×2 and 9-up must be 3×3 on any ordinary
    /// sheet, whatever the scoring does at the margins.
    #[test]
    fn square_counts_resolve_to_square_grids() {
        for (count, expected) in [(4u32, (2usize, 2usize)), (9, (3, 3)), (16, (4, 4))] {
            let spec = NUpSpec {
                grid: NUpGrid::Count(count),
                order: PageOrder::Horizontal,
                border: false,
                auto_rotate: true,
            };
            let layout = plan_n_up(LETTER_PRINTABLE, &[A4; 1], &spec).expect("layable");
            assert_eq!((layout.rows, layout.columns), expected, "{count}-up");
        }
    }

    /// An explicit grid is used exactly as given, even when a better one
    /// exists — Custom means custom.
    #[test]
    fn an_explicit_grid_is_not_second_guessed() {
        let spec = NUpSpec {
            grid: NUpGrid::Custom {
                rows: 1,
                columns: 2,
            },
            order: PageOrder::Horizontal,
            border: false,
            auto_rotate: true,
        };
        let layout = plan_n_up(LETTER_PRINTABLE, &[A4; 2], &spec).expect("layable");
        assert_eq!((layout.rows, layout.columns), (1, 2));
    }

    /// A grid with no cells is refused, both spellings of it.
    #[test]
    fn a_grid_with_no_cells_is_refused() {
        for grid in [
            NUpGrid::Count(0),
            NUpGrid::Custom {
                rows: 0,
                columns: 4,
            },
            NUpGrid::Custom {
                rows: 4,
                columns: 0,
            },
        ] {
            let spec = NUpSpec {
                grid,
                ..NUpSpec::default()
            };
            assert_eq!(
                plan_n_up(LETTER_PRINTABLE, &[A4], &spec),
                Err(ImpositionError::ZeroCells),
                "{grid:?} must be refused, not silently made 1x1"
            );
        }
    }

    /// An absurd cell count is refused rather than allocated.
    #[test]
    fn an_absurd_cell_count_is_refused() {
        let spec = NUpSpec {
            grid: NUpGrid::Count(MAX_CELLS_PER_SHEET + 1),
            ..NUpSpec::default()
        };
        assert!(matches!(
            plan_n_up(LETTER_PRINTABLE, &[A4], &spec),
            Err(ImpositionError::TooManyCells { .. })
        ));
    }

    /// An empty page list is refused rather than yielding an empty layout.
    ///
    /// An empty `Vec` of slots and a successful `Ok` look identical to a
    /// caller that only checks the error — and the job then prints nothing
    /// while reporting success.
    #[test]
    fn an_empty_page_list_is_refused() {
        assert_eq!(
            plan_n_up(LETTER_PRINTABLE, &[], &NUpSpec::default()),
            Err(ImpositionError::NoPages)
        );
    }

    /// A degenerate sheet is refused, and the error carries the numbers.
    #[test]
    fn a_degenerate_sheet_is_refused_with_its_measurements() {
        for sheet in [(0.0, 756.0), (576.0, -1.0), (f64::NAN, 756.0)] {
            assert!(
                matches!(
                    plan_n_up(sheet, &[A4], &NUpSpec::default()),
                    Err(ImpositionError::EmptySheet { .. })
                ),
                "{sheet:?} must be refused"
            );
        }
    }

    /// A malformed page size does NOT fail the job — 39 good pages are
    /// worth more than one bad one, and `place_page` already degrades it
    /// to a finite scale.
    #[test]
    fn a_malformed_page_size_is_placed_rather_than_failing_the_job() {
        let sizes = [A4, (0.0, 0.0), A4];
        let layout =
            plan_n_up(LETTER_PRINTABLE, &sizes, &two_by_two(PageOrder::Horizontal)).unwrap();
        assert_eq!(layout.slots.len(), 3);
        assert!(layout.slots.iter().all(|s| s.fit.scale.is_finite()));
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod booklet_tests {
    use super::{
        A4, Binding, BookletSide, BookletSpec, BookletSubset, ImpositionError, LETTER_PRINTABLE,
        SheetHalf, booklet_pairing, plan_booklet,
    };

    /// The whole imposition as `(first, second)` index pairs per side, in
    /// sheet order: `[(front), (back), (front), (back), …]`.
    fn pairs(pages: usize, binding: Binding) -> Vec<(Option<usize>, Option<usize>)> {
        booklet_pairing(pages, binding)
            .expect("a booklet of at least one page is imposable")
            .into_iter()
            .flat_map(|s| {
                [
                    (s.front.first, s.front.second),
                    (s.back.first, s.back.second),
                ]
            })
            .collect()
    }

    /// ★ Eight pages is the textbook imposition, and the case every other
    /// booklet test is a variation on.
    ///
    /// Front of sheet 1 is `8 | 1`; its back is `2 | 7`. **The outer page
    /// swaps sides between the two faces**, because turning a sheet over
    /// swaps left and right. The defect this names is keeping the outer
    /// page on the same half for both faces: that gives `8 | 1` then
    /// `7 | 2`, which is correct for the first two pages of the booklet
    /// and transposed on every back thereafter — wrong in a way that
    /// survives a glance at sheet one.
    #[test]
    fn an_eight_page_booklet_matches_the_textbook_imposition() {
        assert_eq!(
            pairs(8, Binding::Left),
            vec![
                (Some(7), Some(0)), // sheet 1 front: pages 8 | 1
                (Some(1), Some(6)), // sheet 1 back:  pages 2 | 7
                (Some(5), Some(2)), // sheet 2 front: pages 6 | 3
                (Some(3), Some(4)), // sheet 2 back:  pages 4 | 5
            ]
        );
    }

    /// One page still needs a whole sheet, and three of its four positions
    /// are blank.
    ///
    /// The failure this catches is refusing, or producing zero sheets, for
    /// a page count below four. A single-page booklet is a legitimate
    /// thing to ask for (a folded card), and "no sheets" would print
    /// nothing while reporting success.
    #[test]
    fn a_one_page_booklet_still_makes_one_sheet_with_three_blanks() {
        assert_eq!(pairs(1, Binding::Left), vec![(None, Some(0)), (None, None)]);
    }

    /// Two pages: page 1 on the front right, page 2 on the back left.
    ///
    /// Folded, that reads 1 then 2. If page 2 came out on the back RIGHT,
    /// the fold would put it behind page 1 facing outward — the two-page
    /// case is the smallest one where the front/back flip is visible at
    /// all, which is why it is pinned separately.
    #[test]
    fn a_two_page_booklet_puts_page_one_on_the_front_right() {
        assert_eq!(
            pairs(2, Binding::Left),
            vec![(None, Some(0)), (Some(1), None)]
        );
    }

    /// Three pages leaves exactly one blank, and it is the OUTER position
    /// of the front — the back cover.
    #[test]
    fn a_three_page_booklet_leaves_exactly_one_blank_and_it_is_the_back_cover() {
        assert_eq!(
            pairs(3, Binding::Left),
            vec![(None, Some(0)), (Some(1), Some(2))]
        );
    }

    /// ★ Five pages takes TWO sheets, and its three blanks are spread
    /// across both — not grouped onto the last one.
    ///
    /// This is the non-multiple-of-four case that catches a padding bug
    /// most clearly. Padding at the end of the SEQUENCE (correct) puts
    /// blanks at positions 6, 7, 8, which the fold scatters over both
    /// sheets. Appending blank SHEETS instead, or padding at the front,
    /// gives a booklet that reads 1,2,3,4,5 and then has a blank leaf in
    /// the middle of the fold — plausible-looking output, wrong document.
    #[test]
    fn a_five_page_booklet_takes_two_sheets_with_blanks_on_both() {
        assert_eq!(
            pairs(5, Binding::Left),
            vec![
                (None, Some(0)),    // sheet 1 front: blank | 1
                (Some(1), None),    // sheet 1 back:  2 | blank
                (None, Some(2)),    // sheet 2 front: blank | 3
                (Some(3), Some(4)), // sheet 2 back:  4 | 5
            ]
        );
    }

    /// Six pages pads to eight; the two blanks land on the OUTER sheet,
    /// one per face.
    ///
    /// Worth its own case because it is the count where the blanks are
    /// adjacent in the padded sequence (7 and 8) and yet end up on
    /// opposite faces of the same sheet — a result that looks wrong until
    /// you fold it, and that an implementation "tidying" the blanks
    /// together would get wrong in the direction of looking right.
    #[test]
    fn a_six_page_booklet_pads_to_eight() {
        assert_eq!(
            pairs(6, Binding::Left),
            vec![
                (None, Some(0)),    // sheet 1 front: blank(8) | 1
                (Some(1), None),    // sheet 1 back:  2 | blank(7)
                (Some(5), Some(2)), // sheet 2 front: 6 | 3
                (Some(3), Some(4)), // sheet 2 back:  4 | 5
            ]
        );
    }

    /// ★ Every source page appears exactly once, at every count from 1 to
    /// 40, under every binding.
    ///
    /// The invariant that makes the pairing arithmetic trustworthy beyond
    /// the hand-checked cases above. A duplicated page and a dropped page
    /// come in pairs — the arithmetic conserves positions — so a defect
    /// here prints one page twice and another not at all, at some page
    /// count nobody tested by hand. Blanks are counted too: they must be
    /// exactly the padding.
    #[test]
    fn every_source_page_appears_exactly_once_at_every_count() {
        for pages in 1..=40usize {
            for binding in [
                Binding::Left,
                Binding::Right,
                Binding::LeftTall,
                Binding::RightTall,
            ] {
                let sheets = booklet_pairing(pages, binding).unwrap();
                let mut seen: Vec<usize> = Vec::new();
                let mut blanks = 0usize;
                for sheet in &sheets {
                    for slot in [
                        sheet.front.first,
                        sheet.front.second,
                        sheet.back.first,
                        sheet.back.second,
                    ] {
                        match slot {
                            Some(i) => seen.push(i),
                            None => blanks += 1,
                        }
                    }
                }
                seen.sort_unstable();
                assert_eq!(
                    seen,
                    (0..pages).collect::<Vec<_>>(),
                    "{pages} pages, {binding:?}: a page is duplicated or lost"
                );
                assert_eq!(
                    blanks,
                    sheets.len() * 4 - pages,
                    "{pages} pages, {binding:?}: blank count is not the padding"
                );
            }
        }
    }

    /// Right binding mirrors every spread and changes nothing else.
    ///
    /// A right-bound booklet is the same fold read the other way, so each
    /// spread's two halves swap and the sheet-to-page assignment does not.
    /// Re-deriving the arithmetic for the mirrored case instead of
    /// mirroring the result is how the two bindings drift apart, and this
    /// test is what would catch it.
    #[test]
    fn right_binding_mirrors_every_spread_and_nothing_else() {
        let left = pairs(8, Binding::Left);
        let right = pairs(8, Binding::Right);
        let mirrored: Vec<_> = left.iter().map(|&(a, b)| (b, a)).collect();
        assert_eq!(right, mirrored);
    }

    /// The Tall bindings use the SAME pairing as their flat counterparts.
    ///
    /// Recorded here because it is a judgement call standing in for a
    /// sourcing GAP (see [`super::Binding`]) — if Acrobat is ever measured
    /// and disagrees, this assertion is the one that has to change, and it
    /// should fail loudly rather than be quietly compatible.
    #[test]
    fn tall_bindings_reuse_their_flat_counterparts_pairing() {
        assert_eq!(pairs(8, Binding::LeftTall), pairs(8, Binding::Left));
        assert_eq!(pairs(8, Binding::RightTall), pairs(8, Binding::Right));
    }

    /// A flat binding splits the sheet side by side; a Tall one stacks it.
    ///
    /// The geometric half of the same judgement. Getting this backwards
    /// halves the wrong axis — the pages are the right pages, in the right
    /// order, on a fold line that runs the wrong way, so the booklet
    /// cannot be assembled at all.
    #[test]
    fn tall_binding_stacks_the_halves_instead_of_splitting_side_by_side() {
        let sizes = [A4; 4];
        let flat = plan_booklet(
            LETTER_PRINTABLE,
            &sizes,
            &BookletSpec {
                binding: Binding::Left,
                ..BookletSpec::default()
            },
        )
        .unwrap();
        assert_eq!(flat.slots[0].half, SheetHalf::Left);
        assert!(super::approx_eq(
            flat.slots[0].cell.width,
            LETTER_PRINTABLE.0 / 2.0
        ));
        assert!(super::approx_eq(
            flat.slots[0].cell.height,
            LETTER_PRINTABLE.1
        ));

        let tall = plan_booklet(
            LETTER_PRINTABLE,
            &sizes,
            &BookletSpec {
                binding: Binding::LeftTall,
                ..BookletSpec::default()
            },
        )
        .unwrap();
        assert_eq!(tall.slots[0].half, SheetHalf::Top);
        assert!(super::approx_eq(
            tall.slots[0].cell.width,
            LETTER_PRINTABLE.0
        ));
        assert!(super::approx_eq(
            tall.slots[0].cell.height,
            LETTER_PRINTABLE.1 / 2.0
        ));
    }

    /// A blank position gets a cell but no fit — and the two agree.
    ///
    /// `source: None` with `fit: Some(_)` would mean a caller rendering
    /// "whatever page index came along" into a position the fold requires
    /// to be empty.
    #[test]
    fn a_blank_position_has_a_cell_and_no_fit() {
        let layout = plan_booklet(LETTER_PRINTABLE, &[A4; 3], &BookletSpec::default()).unwrap();
        assert_eq!(layout.blank_positions, 1);
        for slot in &layout.slots {
            assert_eq!(slot.source.is_some(), slot.fit.is_some());
            assert!(slot.cell.is_positive(), "even a blank half occupies paper");
        }
    }

    /// The Front-only subset drops every back side, and Back-only the
    /// reverse — the two manual-duplex passes.
    #[test]
    fn the_booklet_subset_selects_one_face_of_each_sheet() {
        let sizes = [A4; 8];
        let front = plan_booklet(
            LETTER_PRINTABLE,
            &sizes,
            &BookletSpec {
                subset: BookletSubset::FrontOnly,
                ..BookletSpec::default()
            },
        )
        .unwrap();
        assert_eq!(front.slots.len(), 4, "two sheets, one face, two halves");
        assert!(front.slots.iter().all(|s| s.side == BookletSide::Front));

        let back = plan_booklet(
            LETTER_PRINTABLE,
            &sizes,
            &BookletSpec {
                subset: BookletSubset::BackOnly,
                ..BookletSpec::default()
            },
        )
        .unwrap();
        assert!(back.slots.iter().all(|s| s.side == BookletSide::Back));

        let both = plan_booklet(LETTER_PRINTABLE, &sizes, &BookletSpec::default()).unwrap();
        assert_eq!(both.slots.len(), front.slots.len() + back.slots.len());
    }

    /// ★ The sheet range selects PHYSICAL SHEETS, not document pages.
    ///
    /// Sheets 1–1 of a 20-page booklet is the outermost sheet, carrying
    /// document pages 20, 1, 2 and 19 — the cover stock. Reading the range
    /// as document pages would give pages 1 and 2 and look entirely
    /// reasonable, which is exactly why this is pinned.
    #[test]
    fn the_sheet_range_selects_physical_sheets_not_document_pages() {
        let sizes = [A4; 20];
        let layout = plan_booklet(
            LETTER_PRINTABLE,
            &sizes,
            &BookletSpec {
                sheets: Some((1, 1)),
                ..BookletSpec::default()
            },
        )
        .unwrap();
        assert_eq!(layout.total_sheets, 5, "20 pages fold onto 5 sheets");
        assert_eq!(layout.slots.len(), 4, "one sheet, two faces, two halves");
        let printed: Vec<_> = layout.slots.iter().filter_map(|s| s.source).collect();
        assert_eq!(printed, vec![19, 0, 1, 18], "pages 20, 1, 2, 19");
    }

    /// A range overrunning the END is clamped; a range starting past the
    /// end is refused.
    ///
    /// The asymmetry is deliberate and is the point of the test: "3 to 99"
    /// has exactly one reading, and "9 to 99" of a five-sheet booklet has
    /// none — it prints nothing, which no operator asked for.
    #[test]
    fn an_overrunning_range_is_clamped_but_one_starting_past_the_end_is_refused() {
        let sizes = [A4; 20];
        let clamped = plan_booklet(
            LETTER_PRINTABLE,
            &sizes,
            &BookletSpec {
                sheets: Some((3, 99)),
                ..BookletSpec::default()
            },
        )
        .unwrap();
        assert_eq!(clamped.slots.len(), 3 * 4, "sheets 3, 4 and 5");

        assert_eq!(
            plan_booklet(
                LETTER_PRINTABLE,
                &sizes,
                &BookletSpec {
                    sheets: Some((9, 99)),
                    ..BookletSpec::default()
                }
            ),
            Err(ImpositionError::SheetRangeBeyondBooklet { from: 9, sheets: 5 })
        );
    }

    /// An inverted or zero-based sheet range is refused. The control is
    /// 1-based, so `from: 0` is a caller that mixed up its bases — and
    /// treating it as 1 would hide the mistake behind correct-looking
    /// output.
    #[test]
    fn an_inverted_or_zero_sheet_range_is_refused() {
        for range in [(0usize, 3usize), (4, 2)] {
            assert_eq!(
                plan_booklet(
                    LETTER_PRINTABLE,
                    &[A4; 20],
                    &BookletSpec {
                        sheets: Some(range),
                        ..BookletSpec::default()
                    }
                ),
                Err(ImpositionError::SheetRangeEmpty {
                    from: range.0,
                    to: range.1
                })
            );
        }
    }

    /// A booklet of nothing is refused rather than yielding no sheets.
    #[test]
    fn a_zero_page_booklet_is_refused() {
        assert_eq!(
            booklet_pairing(0, Binding::Left),
            Err(ImpositionError::NoPages)
        );
        assert_eq!(
            plan_booklet(LETTER_PRINTABLE, &[], &BookletSpec::default()),
            Err(ImpositionError::NoPages)
        );
    }

    /// An absurd page count is refused rather than allocating for it.
    #[test]
    fn an_absurd_page_count_is_refused() {
        assert!(matches!(
            booklet_pairing(usize::MAX, Binding::Left),
            Err(ImpositionError::BookletTooLarge { .. })
        ));
    }

    /// A degenerate sheet is refused before any pairing happens.
    #[test]
    fn a_degenerate_sheet_is_refused_for_booklets_too() {
        assert!(matches!(
            plan_booklet((0.0, 756.0), &[A4; 4], &BookletSpec::default()),
            Err(ImpositionError::EmptySheet { .. })
        ));
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod poster_tests {
    use super::{
        A4, DEFAULT_MAX_TILES, ImpositionError, LETTER_PRINTABLE, PosterSpec, approx_eq,
        plan_poster,
    };

    /// A page no larger than the sheet is one tile, not zero and not four.
    ///
    /// The zero case is what a `ceil` without a floor produces for a page
    /// smaller than the sheet; the four case is what an unfloored `+ 1`
    /// produces. Both are silent — one prints nothing, the other prints
    /// three blank sheets.
    #[test]
    fn a_page_that_fits_makes_exactly_one_tile() {
        let layout = plan_poster(LETTER_PRINTABLE, (500.0, 700.0), &PosterSpec::default()).unwrap();
        assert_eq!((layout.rows, layout.columns), (1, 1));
        assert_eq!(layout.tiles.len(), 1);
    }

    /// A page exactly the size of the sheet is still one tile.
    ///
    /// The boundary, and the reason [`super::EDGE_EPS_PT`] is subtracted
    /// before the ceiling: without it, an exact fit that lands a float's
    /// breadth over 1.0 rounds to 2 and prints a second sheet carrying a
    /// sliver of content nobody can see.
    #[test]
    fn an_exactly_sheet_sized_page_does_not_spill_onto_a_second_tile() {
        let layout = plan_poster(LETTER_PRINTABLE, LETTER_PRINTABLE, &PosterSpec::default())
            .expect("layable");
        assert_eq!((layout.rows, layout.columns), (1, 1));
    }

    /// Without overlap the tiles partition the poster exactly: their
    /// widths sum to the poster's width, and their heights to its height.
    ///
    /// A gap here loses a strip of the drawing between two sheets, which
    /// is invisible until the poster is taped up. An overlap here — with
    /// none requested — duplicates a strip instead, which is visible but
    /// makes the sheets impossible to align.
    #[test]
    fn tiles_without_overlap_partition_the_poster_exactly() {
        let layout = plan_poster(LETTER_PRINTABLE, (1000.0, 1500.0), &PosterSpec::default())
            .expect("layable");
        assert_eq!((layout.rows, layout.columns), (2, 2));

        let row_width: f64 = layout
            .tiles
            .iter()
            .filter(|t| t.row == 0)
            .map(|t| t.sheet_pt.width)
            .sum();
        let column_height: f64 = layout
            .tiles
            .iter()
            .filter(|t| t.column == 0)
            .map(|t| t.sheet_pt.height)
            .sum();
        assert!(approx_eq(row_width, 1000.0), "row width {row_width}");
        assert!(
            approx_eq(column_height, 1500.0),
            "column height {column_height}"
        );
    }

    /// ★ Overlap shortens the stride, and a stride shorter than the sheet
    /// can need an extra tile.
    ///
    /// A poster exactly two sheets wide needs THREE tiles once the sheets
    /// have to share a 36-point border, because the third sheet carries
    /// what the shared border pushed off the second. Computing the tile
    /// count from the sheet size and only then applying the overlap gives
    /// two — and the right-hand strip of the poster simply never prints.
    #[test]
    fn overlap_shortens_the_stride_and_can_add_a_tile() {
        let exactly_two_sheets_wide = (LETTER_PRINTABLE.0 * 2.0, 700.0);
        let without = plan_poster(
            LETTER_PRINTABLE,
            exactly_two_sheets_wide,
            &PosterSpec::default(),
        )
        .unwrap();
        assert_eq!(without.columns, 2);

        let with = plan_poster(
            LETTER_PRINTABLE,
            exactly_two_sheets_wide,
            &PosterSpec {
                overlap_pt: 36.0,
                ..PosterSpec::default()
            },
        )
        .unwrap();
        assert_eq!(with.columns, 3, "the shared border costs a sheet");
        assert_eq!(with.rows, 1, "and does not add a row it did not need");

        // Consecutive tiles genuinely share content: tile 1 starts 36
        // points before tile 0 ends, in poster coordinates.
        let first = with.tiles.iter().find(|t| t.column == 0).unwrap();
        let second = with.tiles.iter().find(|t| t.column == 1).unwrap();
        assert!(approx_eq(
            second.source_pt.x,
            first.source_pt.right() - 36.0
        ));
    }

    /// The trailing tile is CLIPPED to the poster rather than padded out
    /// to a full sheet.
    ///
    /// A source rectangle that runs past the page asks a renderer for
    /// pixels the page does not have, and different renderers invent
    /// different things there — most often a repeat of the edge, which
    /// looks like real content on the assembled poster.
    #[test]
    fn the_trailing_tile_is_clipped_to_the_poster() {
        let layout = plan_poster(LETTER_PRINTABLE, (1000.0, 1500.0), &PosterSpec::default())
            .expect("layable");
        let last = layout
            .tiles
            .iter()
            .find(|t| t.row == 1 && t.column == 1)
            .unwrap();
        assert!(approx_eq(last.sheet_pt.width, 1000.0 - LETTER_PRINTABLE.0));
        assert!(approx_eq(last.sheet_pt.height, 1500.0 - LETTER_PRINTABLE.1));
        assert!(
            last.sheet_pt.x == 0.0 && last.sheet_pt.y == 0.0,
            "a partial tile is anchored at the printable origin, not centred"
        );
    }

    /// ★ Tile scale multiplies the poster BEFORE tiling, and the source
    /// rectangles are divided back through it.
    ///
    /// Two mistakes hide here. Applying the scale after computing the grid
    /// gives the right number of tiles for the wrong poster. Forgetting to
    /// divide `source_pt` back through the scale hands the renderer a
    /// rectangle in poster points, so every tile shows the wrong region of
    /// the page — at 400%, the top-left sixteenth, four times over.
    #[test]
    fn tile_scale_multiplies_the_poster_before_tiling() {
        let quarter_sheet = (LETTER_PRINTABLE.0 / 2.0, LETTER_PRINTABLE.1 / 2.0);
        let spec = PosterSpec {
            tile_scale: 4.0,
            ..PosterSpec::default()
        };
        let layout = plan_poster(LETTER_PRINTABLE, quarter_sheet, &spec).unwrap();
        assert_eq!((layout.rows, layout.columns), (2, 2));
        assert!(approx_eq(layout.poster_pt.0, quarter_sheet.0 * 4.0));

        let bottom_right = layout
            .tiles
            .iter()
            .find(|t| t.row == 1 && t.column == 1)
            .unwrap();
        assert!(approx_eq(bottom_right.source_pt.x, quarter_sheet.0 / 2.0));
        assert!(approx_eq(bottom_right.source_pt.y, quarter_sheet.1 / 2.0));

        // The four source rectangles must cover the page exactly once.
        let covered: f64 = layout
            .tiles
            .iter()
            .map(|t| t.source_pt.width * t.source_pt.height)
            .sum();
        assert!(approx_eq(covered, quarter_sheet.0 * quarter_sheet.1));
    }

    /// The trim rectangle removes the overlap from LEADING edges only.
    ///
    /// The band shared by two tiles must be discarded exactly once. Trim
    /// both edges and a strip is lost from the assembled poster; trim
    /// neither and the sheets cannot be butted together. Trimming the
    /// leading edge also keeps the OUTSIDE margin of the poster intact,
    /// which is where a border would be.
    #[test]
    fn the_trim_rectangle_removes_the_overlap_from_leading_edges_only() {
        let layout = plan_poster(
            LETTER_PRINTABLE,
            (1000.0, 1500.0),
            &PosterSpec {
                overlap_pt: 36.0,
                cut_marks: true,
                ..PosterSpec::default()
            },
        )
        .unwrap();
        let first = layout
            .tiles
            .iter()
            .find(|t| t.row == 0 && t.column == 0)
            .unwrap();
        assert_eq!((first.trim_pt.x, first.trim_pt.y), (0.0, 0.0));
        assert!(approx_eq(first.trim_pt.width, first.sheet_pt.width));

        let inner = layout
            .tiles
            .iter()
            .find(|t| t.row == 1 && t.column == 1)
            .unwrap();
        assert!(approx_eq(inner.trim_pt.x, 36.0));
        assert!(approx_eq(inner.trim_pt.y, 36.0));
        assert!(approx_eq(inner.trim_pt.width, inner.sheet_pt.width - 36.0));
        assert!(layout.cut_marks, "the flag rides along with the geometry");
    }

    /// With no overlap the trim rectangle IS the sheet rectangle — there
    /// is nothing to cut off.
    #[test]
    fn without_overlap_there_is_nothing_to_trim() {
        let layout = plan_poster(LETTER_PRINTABLE, (1000.0, 1500.0), &PosterSpec::default())
            .expect("layable");
        assert!(layout.tiles.iter().all(|t| t.trim_pt == t.sheet_pt));
    }

    /// ★ "Tile only large pages" is measured AFTER the tile scale.
    ///
    /// A 200-point page at 800% is a 1600-point poster and must be tiled,
    /// even though its MediaBox is smaller than the paper. Testing the
    /// unscaled page instead passes it through untiled, printing the
    /// top-left corner of a poster eight times too large — silently, and
    /// only for the pages the operator most wanted tiled.
    #[test]
    fn tile_only_large_pages_is_measured_after_the_tile_scale() {
        let filtering = PosterSpec {
            tile_only_large_pages: true,
            ..PosterSpec::default()
        };
        assert!(!filtering.tiles_page((500.0, 700.0), LETTER_PRINTABLE));
        assert!(filtering.tiles_page((600.0, 700.0), LETTER_PRINTABLE));

        let magnified = PosterSpec {
            tile_scale: 8.0,
            ..filtering
        };
        assert!(
            magnified.tiles_page((200.0, 200.0), LETTER_PRINTABLE),
            "200pt at 800% is 1600pt and does not fit on a 576pt sheet"
        );
        assert!(
            !filtering.tiles_page((200.0, 200.0), LETTER_PRINTABLE),
            "the same page at 100% does fit"
        );
    }

    /// With the filter off, every page tiles — including one that fits,
    /// which becomes a 1×1 grid.
    ///
    /// This is the RAG's explicit GAP resolved the way that needs no
    /// special case. If it ever changes, this test is the record of what
    /// it changed from.
    #[test]
    fn with_the_filter_off_a_page_that_fits_still_tiles_as_one() {
        let spec = PosterSpec::default();
        assert!(!spec.tile_only_large_pages);
        assert!(spec.tiles_page((10.0, 10.0), LETTER_PRINTABLE));
        let layout = plan_poster(LETTER_PRINTABLE, (10.0, 10.0), &spec).unwrap();
        assert_eq!(layout.tiles.len(), 1);
    }

    /// An overlap not smaller than the sheet is refused on either axis.
    ///
    /// The stride would be zero or negative, so no finite number of tiles
    /// covers the poster. Checked against BOTH axes even when the poster
    /// is one tile wide, so the same setting does not work on one document
    /// and refuse on the next.
    #[test]
    fn an_overlap_that_is_not_smaller_than_the_sheet_is_refused() {
        for overlap in [LETTER_PRINTABLE.0, LETTER_PRINTABLE.1, 5000.0] {
            assert!(
                matches!(
                    plan_poster(
                        LETTER_PRINTABLE,
                        A4,
                        &PosterSpec {
                            overlap_pt: overlap,
                            ..PosterSpec::default()
                        }
                    ),
                    Err(ImpositionError::OverlapExceedsSheet { .. })
                ),
                "an overlap of {overlap} must be refused"
            );
        }
    }

    /// A negative overlap is refused rather than clamped to zero.
    ///
    /// It describes a GAP between tiles — content on the poster that lands
    /// on no sheet at all. Clamping would print a poster that is missing
    /// strips the operator asked for.
    #[test]
    fn a_negative_overlap_is_refused_rather_than_clamped() {
        assert_eq!(
            plan_poster(
                LETTER_PRINTABLE,
                A4,
                &PosterSpec {
                    overlap_pt: -12.0,
                    ..PosterSpec::default()
                }
            ),
            Err(ImpositionError::NegativeOverlap(-12.0))
        );
    }

    /// A nonsense tile scale is refused rather than propagated.
    ///
    /// A NaN scale would make every rectangle NaN, which reaches device
    /// coordinates and prints blank sheets with no error anywhere.
    #[test]
    fn a_nonsense_tile_scale_is_refused() {
        for scale in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            assert!(
                matches!(
                    plan_poster(
                        LETTER_PRINTABLE,
                        A4,
                        &PosterSpec {
                            tile_scale: scale,
                            ..PosterSpec::default()
                        }
                    ),
                    Err(ImpositionError::InvalidTileScale(_))
                ),
                "a tile scale of {scale} must be refused"
            );
        }
    }

    /// A tile count past the ceiling is refused, with the count in the
    /// error so the operator can see what they asked for.
    ///
    /// The realistic cause is a mistyped percentage: 2000% instead of
    /// 200%. Spooling cannot be undone, so this refusal is the difference
    /// between a re-typed number and a ream of paper.
    #[test]
    fn a_tile_count_past_the_ceiling_is_refused() {
        let spec = PosterSpec {
            tile_scale: 100.0,
            ..PosterSpec::default()
        };
        match plan_poster(LETTER_PRINTABLE, A4, &spec) {
            Err(ImpositionError::TooManyTiles { tiles, limit }) => {
                assert!(tiles > u64::from(DEFAULT_MAX_TILES));
                assert_eq!(limit, DEFAULT_MAX_TILES);
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// The ceiling is a policy, not a law — a caller can raise it
    /// deliberately for a genuine wall-sized job.
    #[test]
    fn the_tile_ceiling_can_be_raised_deliberately() {
        let spec = PosterSpec {
            tile_scale: 100.0,
            max_tiles: 100_000,
            ..PosterSpec::default()
        };
        let layout = plan_poster(LETTER_PRINTABLE, A4, &spec).expect("raised ceiling allows it");
        assert_eq!(layout.tiles.len(), layout.rows * layout.columns);
    }

    /// A degenerate page is refused for a poster, unlike for N-up.
    ///
    /// Poster mode has nothing to degrade to: a zero-extent poster has no
    /// tiles, and returning zero sheets for a page the operator asked to
    /// print looks exactly like success.
    #[test]
    fn a_degenerate_page_is_refused_for_a_poster() {
        assert!(matches!(
            plan_poster(LETTER_PRINTABLE, (0.0, 800.0), &PosterSpec::default()),
            Err(ImpositionError::DegeneratePage { .. })
        ));
        assert!(matches!(
            plan_poster((0.0, 756.0), A4, &PosterSpec::default()),
            Err(ImpositionError::EmptySheet { .. })
        ));
    }

    /// A tile label names its row and column 1-based, with their totals.
    ///
    /// Without the totals the label cannot do its only job — telling
    /// somebody holding one sheet where it goes among the others.
    #[test]
    fn a_tile_label_names_its_position_and_the_totals() {
        let layout = plan_poster(
            LETTER_PRINTABLE,
            (1000.0, 1500.0),
            &PosterSpec {
                labels: true,
                ..PosterSpec::default()
            },
        )
        .unwrap();
        let tile = layout
            .tiles
            .iter()
            .find(|t| t.row == 0 && t.column == 1)
            .unwrap();
        assert_eq!(
            layout.tile_label(tile, "site-plan.pdf"),
            "site-plan.pdf — row 1 of 2, column 2 of 2"
        );
        assert!(layout.labels);
    }
}

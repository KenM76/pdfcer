//! # `placetext` — pour a plain-text file into as many NEW pages as it needs
//!
//! The **import** half of `export_text`. `pdfcer-gui` shipped
//! `file.export_text` (File ▸ Export) and then filed
//! `request_there_is_no_route_from_a_text_file_back_into_a_pdf.md`, because a
//! document could go OUT as text and nothing could bring text back IN: there
//! was no verb anywhere in `pdfcer-core` that turns a `&str` into pages.
//!
//! ## Which of the two shapes this is, and why the other was not built
//!
//! The request offered two, either of which would have closed it:
//!
//! 1. **A paginating text placer** — a `&str` plus a page template in, `N`
//!    created pages out.
//! 2. **A page-level text replace** — *"this page's text is now this
//!    string"*, for *"I exported it, fixed a typo in Notepad, put it back"*.
//!
//! **Shape 1 is what this module is.** The reasoning, recorded here because
//! it is the kind of choice a later session will otherwise re-litigate from
//! scratch: [`EditSession::edit_text`](crate::edit::EditSession::edit_text)
//! **already** does find-and-replace over located runs, so the typo round trip
//! is largely served today and shape 2 would have been a second, coarser way
//! to reach it. *"Turn a text file into a PDF"* had **no route at all** — not a
//! worse route, none — and an absent capability is worth more than a
//! second path to a present one.
//!
//! It is also the shape the request's own evidence argues for. `plain_text()`
//! derives its line breaks (negative result S5), one glyph is not one
//! character (§9.10.3), and 13 % of runs draw glyphs from more than one show
//! operator — so a text file *cannot* be mapped back onto the runs it came
//! from. Any import that exists has to **author** pages, not reverse them.
//! Shape 1 authors; shape 2 would have had to reconcile.
//!
//! ## What it does NOT invent
//!
//! Nothing here wraps text, measures a glyph, or emits a `BT…ET`. All of that
//! is the shipped [`crate::text_edit::addtext`] boxed path (Pass 16.1), which
//! already wraps to a box through the ONE greedy breaker
//! ([`crate::linebreak::greedy_pack`]) at §9.4.4 AFM advances. What this module
//! adds is the single thing that path deliberately does not do: **overflow
//! creates a page instead of being emitted past the paper edge.**
//!
//! `addtext`'s R76 posture — *"disclose and EMIT, never clip"* — is right for a
//! hand-placed note, where the spilled lines are recoverable content. It is
//! wrong for an import: a 40 KB text file would become a one-page PDF carrying
//! eight ninths of its content painted off the sheet, invisible in every
//! viewer and present in every extraction, reported as a success. The request
//! named that failure before writing a line of it, which is why this module
//! exists rather than a `--continue` flag on `add-text`.
//!
//! ## The pipeline
//!
//! ```text
//!   &str
//!    │  sanitise()      normalise line endings, strip a BOM, drop non-printing
//!    ▼                  controls, split on U+000C (a hard page break)
//!   Section*  ─ paragraphs ─ words                (one census pass, counted)
//!    │  plan()          measure each word with the SAME face metrics addtext
//!    ▼                  uses, greedy_pack each paragraph, cut every
//!   PagePlan*           `lines_per_page` lines into a page
//!    │  EditSession::place_text
//!    ▼
//!   scaffold_bytes()  ─→ Document ─→ insert_pages()   ONE command
//!   add_text(boxed)   ×N                              N commands
//!   coalesce_last(N+1)                                → ONE undo entry
//! ```
//!
//! ### Why the page text is re-wrapped rather than handed over pre-broken
//!
//! [`plan`] decides the line breaks in order to know where the page boundaries
//! fall, and then throws those breaks away: each page is handed the *words* of
//! its share, joined back into a string, and [`crate::text_edit::add_text`]
//! re-derives the identical breaks. That looks like doing the work twice and is
//! deliberate.
//!
//! Greedy first-fit is **prefix-stable**: packing `words[a..]` from a fresh
//! line, where `a` is a line boundary of the full pack, reproduces the full
//! pack's remaining lines exactly — the packer's state at `a` is "empty line",
//! which is the state it starts in. So the re-wrap is not an approximation of
//! the plan, it is the same computation resumed, and there is exactly ONE
//! emission path (16.1's). The alternative — a second emitter here that places
//! pre-broken lines — would be a second way to write a `BT…ET`, and this
//! project has already paid for that twice (`append_contents` in `addtext.rs`,
//! `Pass 111.0`).
//!
//! The one place the identity is imperfect is **justification**, and it is
//! disclosed rather than hidden: a paragraph cut across a page break becomes
//! two paragraphs, and §4.1 never stretches a paragraph's last line, so the
//! line immediately before each such break sets flush-left. See
//! [`PlaceTextReport::paragraphs_split_across_pages`].
//!
//! ## Judgement calls, all disclosed, none silent (rule 4)
//!
//! | Input | What pdfcer does | Why |
//! |---|---|---|
//! | a character the face cannot encode | **refuses the whole import**, naming every offending character and its count ([`Unmappable::Refuse`], the default) | R71 / R-INV-1. ISO 32000-1 specifies the forward map only and imposes **no** inverse obligation, so there is no spec answer to fall back on; refusing is the project's standing posture and it is the only one that cannot lose text. [`Unmappable::Drop`] is an explicit opt-in, never inferred |
//! | a tab (U+0009) | treated as inter-word whitespace; collapses to one space, counted | `WinAnsiEncoding` **assigns no code below 0o40 (32)** — Annex D.2's table body starts at `space`, and footnote 3's `bullet` fallback covers only codes ≥ 0o40, so a tab is unmapped outright. PDF has no tab-stop concept in text showing at all (position is `Td`/`Tm`/`TJ`), so there is nothing to preserve it *as* |
//! | a form feed (U+000C) | **a hard page break** | `export_text` offers U+000C as its page separator, so an exported-then-edited file re-imports with its pagination intact. This is the one round-trip property the pair actually can honour |
//! | CRLF / lone CR | normalised to LF, counted | otherwise a Windows text file gets a stray CR in every line, which WinAnsi cannot encode and which would refuse the entire import for a reason the operator did not cause |
//! | a UTF-8 BOM | stripped, counted | `export_text` writes one optionally; importing it back as a literal U+FEFF would either refuse the import or draw a glyph |
//! | any other non-printing control | dropped, counted | it has no glyph name in the Adobe standard Latin set and nothing to place |
//! | a word wider than the column | placed alone on its line, overflowing it; counted | inherited verbatim from 16.1 — pdfcer does not hyphenate, and breaking a word at an arbitrary point invents a hyphenation decision it has no dictionary for |
//! | empty, or whitespace-only, input | **refused by name** | an empty `.txt` and a successful import of nothing are indistinguishable in the output file, which is exactly the confusion `export_text` already refuses on the way out |
//! | a run of blank lines longer than a page | the page is **created and left blank**, counted | faithful to the input's line count. Swallowing it would silently change the document's shape |
//!
//! **Nothing is trimmed for tidiness.** A paragraph gap that lands on a page
//! boundary consumes the first line of the next page, exactly as it would have
//! consumed a line in the middle of one. Prettier output would mean the
//! imported document no longer has the input's line structure, and an import
//! whose output cannot be predicted from its input is not an import.
//!
//! ## Where the pages go, and why this appends
//!
//! [`EditSession::place_text`](crate::edit::EditSession::place_text) takes an
//! [`InsertPosition`](crate::pageops::InsertPosition) and **appends into the
//! open document** rather than demanding an empty one. Requiring empty would
//! make *"append these notes to this report"* impossible for no gain, and the
//! shell that asked for this has an open document by definition. A caller who
//! wants a fresh document supplies a fresh document.
//!
//! It does need **at least one existing page** to splice beside —
//! [`EditSession::insert_pages`](crate::edit::EditSession::insert_pages)
//! resolves its insertion point from a sibling slot and has none in a page-less
//! document. That is a named refusal ([`PlaceTextError::NoPageToInsertBeside`]),
//! not a panic, and the spec RAG has **no** answer on whether a zero-page
//! document is even conforming (searched 2026-09-06: no minimum-page-count
//! rule, no empty-`/Kids` handling, in either edition's coverage) — so pdfcer
//! does not create one to find out.
//!
//! ## The scaffold document, and the spec that governs it
//!
//! [`scaffold_bytes`] synthesizes a minimal in-memory PDF holding `N` blank
//! pages, which `insert_pages` then copies in. Building a document to copy
//! *from* is cheaper and far safer than a second page-tree splice: `Pass 102`'s
//! splice already handles `/Count` propagation up the ancestor chain, resource
//! remapping and object renumbering, and none of that wants a rival.
//!
//! Per §7.7.3.3 Table 30 the scaffold page carries all four required entries —
//! `/Type`, `/Parent`, `/Resources` and `/MediaBox`. `/Resources` is written as
//! an **empty dictionary** on purpose: the table is explicit that *"If the page
//! requires no resources, the value of this entry shall be an empty
//! dictionary. Omitting the entry entirely indicates that the resources shall
//! be inherited from an ancestor node"* — two different statements, and the
//! blank page means the first. `/Contents` is **absent**, which Table 30 makes
//! legal and meaningful: *"If this entry is absent, the page shall be empty."*
//! (Writing `/Contents []` instead would be a `shall not` — the same table
//! forbids an empty `/Contents` array.) The `/Pages` node carries `/Type`,
//! `/Kids` and `/Count` and **no** `/Parent`, which Table 29 prohibits in the
//! root.
//!
//! ## GUI-core separation
//!
//! Everything here is plain data: a rectangle, four margins, a
//! [`Std14`](crate::fontdata::Std14) face, numbers and strings. No windowing
//! type, no rendering, no file I/O — the shell reads the file and reports the
//! counts (`ARCHITECTURE.md` §3).

use std::collections::BTreeMap;
use std::ops::Range;

use crate::fontdata::{self, BaseEncoding, Std14};
use crate::linebreak::greedy_pack;
use crate::page_tree::Rect;
use crate::paper::{Orientation, PaperSize};
use crate::text_edit::addtext::{AddTextError, face_encoding};
use crate::text_edit::encoding::InverseEncoding;
use crate::text_edit::reflow::BlockAlignment;
use crate::writer::content::emit_number;

/// Ascent as a fraction of the effective size.
///
/// Deliberately the same 0.75 constant `addtext::ASCENT_FRAC` uses, and the
/// duplication is the point: this module computes how many lines fit a page
/// and `addtext` computes where those lines sit, and if the two ever used
/// different ascents the last line of every page would land somewhere this
/// module did not predict. The agreement is asserted by
/// `place_text_lines_never_overflow_their_box`, not assumed.
const ASCENT_FRAC: f64 = 0.75;
/// Descent as a fraction of the effective size — the `addtext` peer of
/// [`ASCENT_FRAC`], and load-bearing for the same reason: the bottom of the
/// last line on a page is `baseline − 0.25·size`, and that is the number
/// [`lines_per_page`] tests against the bottom margin.
const DESCENT_FRAC: f64 = 0.25;
/// Default leading (baseline-to-baseline) as a multiple of size when the
/// template does not specify one — the same 1.2 `addtext` derives, disclosed
/// once here rather than once per page.
const DEFAULT_LEADING_FRAC: f64 = 1.2;
/// Fallback inter-word space width as a fraction of size, used only when the
/// face reports a zero advance for its space glyph. Mirrors
/// `addtext::FALLBACK_SPACE_FRAC` so the pagination measure and the emission
/// measure cannot disagree.
const FALLBACK_SPACE_FRAC: f64 = 0.25;
/// The default margin on all four sides, points (one inch).
///
/// One inch is the near-universal word-processor default and, unlike a
/// typographic choice, it is a number the operator can predict without reading
/// anything.
pub const DEFAULT_MARGIN_PT: f64 = 72.0;
/// A ceiling on the line-fitting loop, so a pathological leading (a
/// denormal, say) cannot spin. Far above any real page: 4000 lines at the
/// smallest sane leading is a sheet metres tall.
const MAX_LINES_PER_PAGE: usize = 4096;

/// What to do about a character the chosen face cannot encode.
///
/// The two are **not** symmetric and the default is the strict one. ISO
/// 32000-1 specifies the code→glyph direction only and imposes no obligation
/// that the inverse be well-defined (`iso32000__ref__inverse_encoding.md`), so
/// there is no conformance rule to appeal to here — this is a policy, and the
/// project's standing one is refuse-and-disclose (R71 / R-INV-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Unmappable {
    /// Refuse the entire import and name every character that caused it.
    ///
    /// The default. An import is bulk content the operator has not read
    /// character by character; dropping silently would be the worst failure
    /// this project recognises, and dropping *loudly* still produces a
    /// document whose text differs from the file that made it.
    #[default]
    Refuse,
    /// Drop the characters and place the rest, disclosing exactly which
    /// characters were dropped and how many of each.
    ///
    /// An explicit opt-in, never inferred from anything (R108). It exists
    /// because a 40 KB export with one stray U+2028 in it is otherwise
    /// unimportable, and the operator who knows that is the one who can say
    /// so.
    Drop,
}

/// The page recipe an import is poured into: sheet, margins, and the face the
/// text is set in.
///
/// Construct with [`Self::new`] (US Letter portrait, one-inch margins,
/// 12 pt Helvetica, black, left-aligned, derived leading) and refine with the
/// `with_*` builders. `#[non_exhaustive]`, so a struct literal is not usable
/// out-of-crate and later fields never break callers — the same shape
/// [`AddTextRequest`](crate::text_edit::AddTextRequest) uses, for the same
/// reason.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PageTemplate {
    /// The sheet, in default user space (§8.3.2.3 — 1/72 inch units). Becomes
    /// each created page's `/MediaBox` (§7.7.3.3 Table 30).
    pub media_box: Rect,
    /// Left margin, points.
    pub margin_left: f64,
    /// Right margin, points.
    pub margin_right: f64,
    /// Top margin, points.
    pub margin_top: f64,
    /// Bottom margin, points.
    pub margin_bottom: f64,
    /// The Standard-14 face the text is set in (no embedding, R79).
    pub face: Std14,
    /// Font size, points.
    pub size: f64,
    /// Baseline-to-baseline leading, points. `None` ⇒ the derived
    /// `1.2 × size`, which is disclosed rather than assumed.
    pub leading: Option<f64>,
    /// Column alignment. Left by default; a fresh column has no glyphs to
    /// detect an alignment from, so this is an explicit input exactly as it is
    /// for a boxed [`add_text`](crate::text_edit::add_text).
    pub alignment: BlockAlignment,
    /// Fill colour.
    pub color: crate::text_edit::NewTextColor,
    /// What to do about characters the face cannot encode.
    pub unmappable: Unmappable,
}

impl Default for PageTemplate {
    fn default() -> Self {
        Self::new()
    }
}

impl PageTemplate {
    /// US Letter portrait, one-inch margins, 12 pt black Helvetica, left
    /// aligned, derived leading, refusing unmappable characters.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::text_edit::PageTemplate;
    ///
    /// let t = PageTemplate::new();
    /// assert!((t.media_box.width() - 612.0).abs() < 1e-9);
    /// assert!((t.margin_left - 72.0).abs() < 1e-9);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            media_box: PaperSize::Letter.rect_with(Orientation::Portrait),
            margin_left: DEFAULT_MARGIN_PT,
            margin_right: DEFAULT_MARGIN_PT,
            margin_top: DEFAULT_MARGIN_PT,
            margin_bottom: DEFAULT_MARGIN_PT,
            face: Std14::Helvetica,
            size: 12.0,
            leading: None,
            alignment: BlockAlignment::Left,
            color: crate::text_edit::NewTextColor::Black,
            unmappable: Unmappable::Refuse,
        }
    }

    /// Set the sheet from a named standard size and orientation.
    #[must_use]
    pub fn with_paper(mut self, paper: PaperSize, orientation: Orientation) -> Self {
        self.media_box = paper.rect_with(orientation);
        self
    }

    /// Set the sheet from an explicit rectangle in points.
    #[must_use]
    pub const fn with_media_box(mut self, media_box: Rect) -> Self {
        self.media_box = media_box;
        self
    }

    /// Set all four margins, points, in the CSS order (left, right, top,
    /// bottom is deliberately NOT that order — see the parameter names).
    #[must_use]
    pub const fn with_margins(mut self, left: f64, right: f64, top: f64, bottom: f64) -> Self {
        self.margin_left = left;
        self.margin_right = right;
        self.margin_top = top;
        self.margin_bottom = bottom;
        self
    }

    /// Set the Standard-14 face.
    #[must_use]
    pub const fn with_font(mut self, face: Std14) -> Self {
        self.face = face;
        self
    }

    /// Set the font size, points.
    #[must_use]
    pub const fn with_size(mut self, size: f64) -> Self {
        self.size = size;
        self
    }

    /// Set an explicit leading (baseline-to-baseline, points); `None` restores
    /// the derived `1.2 × size`.
    #[must_use]
    pub const fn with_leading(mut self, leading: Option<f64>) -> Self {
        self.leading = leading;
        self
    }

    /// Set the column alignment.
    #[must_use]
    pub const fn with_alignment(mut self, alignment: BlockAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Set the fill colour.
    #[must_use]
    pub const fn with_color(mut self, color: crate::text_edit::NewTextColor) -> Self {
        self.color = color;
        self
    }

    /// Set the policy for characters the face cannot encode.
    #[must_use]
    pub const fn with_unmappable(mut self, unmappable: Unmappable) -> Self {
        self.unmappable = unmappable;
        self
    }

    /// The text column: the media box inset by the four margins.
    ///
    /// Public because a shell drawing a preview needs the same rectangle the
    /// import will use, and re-deriving it there is how the preview and the
    /// commit drift apart.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::text_edit::PageTemplate;
    ///
    /// let column = PageTemplate::new().text_column();
    /// assert!((column.width() - (612.0 - 144.0)).abs() < 1e-9);
    /// ```
    #[must_use]
    pub fn text_column(&self) -> Rect {
        Rect::from_corners(
            self.media_box.llx + self.margin_left,
            self.media_box.lly + self.margin_bottom,
            self.media_box.urx - self.margin_right,
            self.media_box.ury - self.margin_top,
        )
    }

    /// The leading actually used, and whether it was derived rather than given.
    fn resolved_leading(&self) -> (f64, bool) {
        match self.leading {
            Some(l) if l.is_finite() && l > 0.0 => (l, false),
            _ => (DEFAULT_LEADING_FRAC * self.size, true),
        }
    }
}

/// What an import created, and everything it had to decide on the way
/// (fuzzy-never-sneaky, rule 4).
///
/// Every count here answers a question the operator cannot answer from the
/// output file: a page that came out blank looks like a page the operator
/// asked for, a collapsed tab looks like a space that was always a space, and
/// a dropped character looks like a character that was never in the file.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct PlaceTextReport {
    /// How many pages the import created.
    pub pages_created: usize,
    /// The 0-based index, in the document as it stands **after** the import, of
    /// the first created page. The created pages are
    /// `first_page_index .. first_page_index + pages_created`.
    pub first_page_index: usize,
    /// How many created pages carry no text, because the input had that many
    /// consecutive blank lines. They are real, deliberate pages, not failures.
    pub blank_pages: usize,
    /// How many lines the text wrapped to, over all pages.
    pub lines_placed: usize,
    /// How many lines fit one page under this template — the pagination
    /// constant everything else follows from.
    pub lines_per_page: usize,
    /// Scalars in the input, before anything was done to it.
    pub chars_input: usize,
    /// Non-whitespace scalars actually written into page content.
    pub chars_placed: usize,
    /// Whitespace scalars normalised away: every space, tab, newline, carriage
    /// return and form feed in the input. Inter-word gaps are re-emitted as one
    /// space each, so the input's own whitespace does not survive as bytes and
    /// is counted here instead of being silently absent.
    pub whitespace_normalised: usize,
    /// Non-printing control scalars removed because no Standard-14 encoding
    /// has a code for them (Annex D.2 assigns nothing below 0o40).
    pub chars_dropped_control: usize,
    /// Scalars removed because the chosen face cannot encode them. Non-zero
    /// only under [`Unmappable::Drop`] — [`Unmappable::Refuse`] refuses the
    /// whole import instead, so this is 0 on every default-policy success.
    pub chars_dropped_unmappable: usize,
    /// The distinct characters counted by
    /// [`Self::chars_dropped_unmappable`], with the number of occurrences of
    /// each. A count alone tells an operator that something is missing; this
    /// tells them **what**, which is the difference between a disclosure they
    /// can act on and one they can only worry about.
    pub dropped_unmappable_chars: Vec<(char, usize)>,
    /// A UTF-8 BOM (U+FEFF) was found at the start of the input and stripped.
    pub bom_stripped: bool,
    /// CR characters normalised away (CRLF pairs plus lone CRs).
    pub crlf_normalised: usize,
    /// Tabs found in the input. Each collapsed into the ordinary inter-word
    /// space its neighbours already implied — so an indented text file loses
    /// its indentation, which is exactly the thing worth saying out loud.
    pub tabs_collapsed: usize,
    /// Form feeds (U+000C) honoured as explicit page breaks.
    pub explicit_page_breaks: usize,
    /// Words wider than the text column, placed alone on their line and
    /// overflowing it. pdfcer does not hyphenate.
    pub overlong_words: usize,
    /// Paragraphs cut across a page boundary. Under
    /// [`BlockAlignment::Justified`] the line immediately before each such
    /// break sets flush-left, because each page is wrapped as its own text and
    /// §4.1 never stretches a paragraph's last line.
    pub paragraphs_split_across_pages: usize,
    /// Lines the emission path reported as overflowing their box — which
    /// should be **0 for every import**, because the pagination exists
    /// precisely to prevent it. A non-zero value means this module's
    /// line-fitting arithmetic and `addtext`'s placement arithmetic have
    /// diverged, and it is surfaced rather than trusted.
    pub box_overflow_lines: usize,
    /// How many undo entries the import actually left on the session's stack.
    /// **1** whenever the fold succeeded.
    pub undo_entries: usize,
    /// Whether the whole import folded into ONE undo entry. `false` only when
    /// the import needed more commands than
    /// [`MAX_UNDO_DEPTH`](crate::edit::MAX_UNDO_DEPTH) — i.e. an import of more
    /// than 255 non-blank pages — in which case every page was still placed and
    /// only the grouping failed.
    pub coalesced: bool,
    /// Whether [`PageTemplate::leading`] was derived (`1.2 × size`) rather than
    /// supplied.
    pub leading_derived: bool,
    /// The leading actually used, points.
    pub leading: f64,
    /// Every operator-facing disclosure, verbatim, ready to print.
    pub disclosures: Vec<String>,
}

/// A failure to place text — every variant a named, clean outcome.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PlaceTextError {
    /// The input is empty.
    #[error("there is nothing to place — the text is empty")]
    EmptyText,
    /// The input has no non-whitespace content.
    #[error(
        "there is nothing to place — the text has no words in it, only whitespace. An empty \
         import and a successful import of nothing produce the same file, so pdfcer refuses \
         rather than writing blank pages you did not ask for"
    )]
    NoWordsToPlace,
    /// The font size is not a positive, finite number of points.
    #[error("font size {0} is not a positive, finite number of points")]
    InvalidSize(f64),
    /// The margins leave no usable column on the sheet.
    #[error(
        "the margins leave a {width:.1} x {height:.1} pt text column on a {sheet_w:.1} x \
         {sheet_h:.1} pt sheet — there is no room to set text"
    )]
    NoColumn {
        /// The column width the margins left, points.
        width: f64,
        /// The column height the margins left, points.
        height: f64,
        /// Sheet width, points.
        sheet_w: f64,
        /// Sheet height, points.
        sheet_h: f64,
    },
    /// Not one line of this size fits the column.
    #[error(
        "not one {size:.1} pt line fits a {height:.1} pt text column at {leading:.1} pt leading \
         — reduce the font size, the leading, or the top and bottom margins"
    )]
    PageTooShort {
        /// The column height, points.
        height: f64,
        /// The font size, points.
        size: f64,
        /// The leading, points.
        leading: f64,
    },
    /// The face cannot encode one or more characters, and the policy is
    /// [`Unmappable::Refuse`].
    ///
    /// The message names **every** offending character with its count, not the
    /// first one. Fixing a text file one refusal at a time is a loop, and the
    /// whole scan is already done by the time this is raised.
    #[error(
        "the font '{base_font}' cannot represent {total} character(s) of this text: {listing}. \
         ISO 32000-1 specifies no inverse of a font encoding, so pdfcer refuses rather than \
         guessing a substitute (R71). Choose a face that covers them, or ask for them to be \
         dropped and pdfcer will report exactly which were lost"
    )]
    Unmappable {
        /// The `/BaseFont` the text was measured against.
        base_font: String,
        /// How many scalars in total could not be encoded.
        total: usize,
        /// A rendered `U+XXXX 'c' ×N, …` listing of the distinct characters.
        listing: String,
        /// The distinct characters and their occurrence counts.
        chars: Vec<(char, usize)>,
    },
    /// The document has no page to splice the new pages beside.
    #[error(
        "this document has no pages, so there is nowhere to insert beside. pdfcer places imported \
         pages relative to an existing one; open or create a document with at least one page first"
    )]
    NoPageToInsertBeside,
    /// The scaffold document pdfcer builds to copy blank pages from could not
    /// be loaded back. Unreachable in practice, and checked anyway: the
    /// scaffold is bytes this module writes and the loader is the real parser,
    /// so "it will obviously parse" is a claim, not evidence (R93).
    #[error("internal: pdfcer could not read back the blank pages it built ({0}). This is a bug")]
    Scaffold(crate::document::DocError),
    /// A page could not be created (certification, page tree, object numbers).
    #[error("the pages could not be created: {0}")]
    Insert(#[from] crate::edit::EditError),
    /// A page's text could not be written.
    #[error("the text could not be written: {0}")]
    Add(#[from] AddTextError),
}

// =====================================================================
// Sanitisation — one pass, every scalar accounted for.
// =====================================================================

/// One page-break-delimited stretch of the sanitised input.
///
/// A section always starts a new page. `paragraphs` are the hard-newline-
/// separated runs; an empty `Vec<String>` is a blank paragraph, which consumes
/// a line and shows nothing (exactly as `addtext`'s `LaidLine::blank` does).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Section {
    paragraphs: Vec<Vec<String>>,
}

/// The per-scalar census a sanitisation pass produces.
///
/// Every scalar of the input lands in exactly one bucket, which is what makes
/// `chars_input == placed + whitespace + control + unmappable + bom` an
/// assertable invariant rather than a hope. `place_text_accounts_for_every_input_character`
/// asserts it.
#[derive(Debug, Clone, Default)]
struct Census {
    chars_input: usize,
    whitespace: usize,
    control: usize,
    bom_stripped: bool,
    crlf: usize,
    tabs: usize,
    page_breaks: usize,
    unmappable: BTreeMap<char, usize>,
}

/// Split `text` into sections/paragraphs/words, counting every scalar.
///
/// ## Line endings are normalised FIRST, and separately
///
/// `\r\n` → `\n` and a lone `\r` → `\n`, before anything else looks at the
/// text. Doing it inside the main loop needs a "was the previous scalar a CR"
/// flag threaded through every arm, and the version of this function that tried
/// that got the CRLF case wrong in a way no single-line test could see: it
/// asked "has any CR been seen" rather than "was the last scalar a CR", so the
/// first CRLF in a file suppressed the blank line after **every** later
/// paragraph. Normalising up front makes the state unrepresentable.
///
/// ## Then, one arm per class, in this order — the order IS the meaning
///
/// 1. a leading U+FEFF — stripped (`export_text` can write one);
/// 2. U+000C — a page break (`export_text`'s own page separator);
/// 3. `\n` — a hard paragraph break;
/// 4. any other whitespace, tab included — an inter-word gap;
/// 5. any other C0/C1 control or U+007F — dropped, no glyph exists;
/// 6. a scalar the face cannot encode — recorded against `unmappable`;
/// 7. everything else — a character of the current word.
///
/// (4) swallows the tab deliberately and (5) does not: a tab IS whitespace and
/// collapses to the gap its neighbours already imply, whereas U+0007 is not
/// whitespace and would otherwise become a word character with no glyph.
///
/// ## The accounting
///
/// [`Census::chars_input`] counts the ORIGINAL text; the loop runs over the
/// normalised one. The difference is exactly the `\r` of each CRLF pair, which
/// is added to [`Census::whitespace`] here so that
/// `chars_input == placed + whitespace + control + unmappable + bom` still
/// holds. A lone `\r` becomes an `\n` and is counted by the loop as one.
fn sanitise(text: &str, inv: &InverseEncoding) -> (Vec<Section>, Census) {
    // `\r\n` first, so the `\r` of a pair is removed rather than turned into a
    // second break; then any survivor is a lone CR and becomes one break.
    let crlf_pairs = text.matches("\r\n").count();
    let normalised = if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_owned()
    };

    let mut census = Census {
        chars_input: text.chars().count(),
        whitespace: crlf_pairs,
        crlf: text.matches('\r').count(),
        ..Census::default()
    };

    let mut sections: Vec<Section> = Vec::new();
    let mut paragraphs: Vec<Vec<String>> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();

    for (i, ch) in normalised.chars().enumerate() {
        if i == 0 && ch == '\u{feff}' {
            census.bom_stripped = true;
            continue;
        }
        match ch {
            '\u{c}' => {
                census.whitespace += 1;
                census.page_breaks += 1;
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                paragraphs.push(std::mem::take(&mut words));
                sections.push(Section {
                    paragraphs: std::mem::take(&mut paragraphs),
                });
            }
            '\n' => {
                census.whitespace += 1;
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                paragraphs.push(std::mem::take(&mut words));
            }
            c if c.is_whitespace() => {
                census.whitespace += 1;
                if c == '\t' {
                    census.tabs += 1;
                }
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c if is_non_printing(c) => {
                census.control += 1;
            }
            c if !inv.has_char(c) => {
                *census.unmappable.entry(c).or_insert(0) += 1;
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    paragraphs.push(words);
    sections.push(Section { paragraphs });

    (sections, census)
}

/// Whether `c` is a control/format scalar with no glyph in any Standard-14
/// encoding: C0 (minus the whitespace the arms above already took), DEL, and
/// C1.
///
/// Grounded, not guessed: Annex D.2's table body assigns **no** code below
/// 0o40 (32) in any of the four predefined encodings, and footnote 3's
/// `bullet` catch-all is scoped to codes ≥ 0o40 — so a control code is not
/// merely unlikely to render, it has no glyph name in the Adobe standard Latin
/// character set at all.
const fn is_non_printing(c: char) -> bool {
    let u = c as u32;
    u < 0x20 || u == 0x7f || (u >= 0x80 && u <= 0x9f)
}

// =====================================================================
// Pagination.
// =====================================================================

/// One page's share of the import, ready to hand to a boxed `add_text`.
pub(crate) struct PagePlan {
    /// The page's text, paragraphs rejoined with `\n` and words with a single
    /// space. Empty when the page is deliberately blank.
    pub(crate) text: String,
    /// Non-whitespace scalars this page carries.
    pub(crate) chars: usize,
    /// How many wrapped lines this page holds (blank lines included).
    pub(crate) lines: usize,
}

/// A planned import: the pages, the column, and the report skeleton.
pub(crate) struct PlacePlan {
    pub(crate) pages: Vec<PagePlan>,
    pub(crate) column: Rect,
    pub(crate) leading: f64,
    pub(crate) report: PlaceTextReport,
}

/// One line of the plan: which paragraph of which section it came from, and
/// which of that paragraph's words it holds.
///
/// Carrying the paragraph identity — rather than just the words — is what lets
/// the page text be reassembled with the right separator between consecutive
/// lines: a space when they continue one paragraph, a newline when they do
/// not. Reassembling with newlines throughout would turn every wrapped line
/// into its own paragraph and, under justification, un-stretch all of them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedLine {
    section: usize,
    paragraph: usize,
    words: Range<usize>,
}

/// How many lines of `size`/`leading` fit a column `height` points tall.
///
/// Mirrors `addtext::layout_boxed`'s placement arithmetic **operation for
/// operation** — `first_baseline = top − 0.75·size`, then
/// `baseline_i = first_baseline − i·leading`, and a line fits while
/// `baseline_i − 0.25·size ≥ bottom`. Written as a loop rather than a closed
/// form on purpose: a closed form would be the same answer computed a
/// different way, and "the same answer computed a different way" is how two
/// float pipelines end up one line apart on exactly one page size.
fn lines_per_page(column: &Rect, size: f64, leading: f64) -> usize {
    let ascent = ASCENT_FRAC * size;
    let descent = DESCENT_FRAC * size;
    let first_baseline = column.ury - ascent;
    let mut n = 0usize;
    while n < MAX_LINES_PER_PAGE {
        let baseline = first_baseline - leading * (n as f64);
        if baseline - descent < column.lly {
            break;
        }
        n += 1;
    }
    n
}

/// Advance width of a run of codes at `size`, points (§9.4.4:
/// `width/1000 × size` with `Tc`/`Tw`/`Tz` at their defaults).
///
/// The `addtext::measure_codes` twin. Duplicated rather than imported because
/// that one is private to its module and widening it would make an internal
/// measurement part of a shared surface; the four lines are asserted equal in
/// effect by every fidelity test here (a divergence changes where lines break,
/// which changes the page count, which the tests pin).
fn measure_codes(font: Std14, enc: BaseEncoding, codes: &[u8], size: f64) -> f64 {
    codes
        .iter()
        .map(|&c| {
            let units = fontdata::encoding_glyph_name(enc, c)
                .and_then(|name| fontdata::std14_width(font, name))
                .unwrap_or(0);
            f64::from(units) / 1000.0 * size
        })
        .sum()
}

/// Plan the import: sanitise, measure, wrap, and cut into pages.
///
/// Pure — it touches no document and allocates no object. Everything that can
/// refuse, refuses here, before a single page exists (rule 4: a refusal after a
/// partial write is not a refusal).
///
/// # Errors
///
/// [`PlaceTextError::EmptyText`], [`PlaceTextError::NoWordsToPlace`],
/// [`PlaceTextError::InvalidSize`], [`PlaceTextError::NoColumn`],
/// [`PlaceTextError::PageTooShort`] or [`PlaceTextError::Unmappable`].
pub(crate) fn plan(text: &str, template: &PageTemplate) -> Result<PlacePlan, PlaceTextError> {
    if text.is_empty() {
        return Err(PlaceTextError::EmptyText);
    }
    if !template.size.is_finite() || template.size <= 0.0 {
        return Err(PlaceTextError::InvalidSize(template.size));
    }
    let column = template.text_column();
    let (w, h) = (column.width(), column.height());
    if !(w.is_finite() && w > 0.0 && h.is_finite() && h > 0.0) {
        return Err(PlaceTextError::NoColumn {
            width: w,
            height: h,
            sheet_w: template.media_box.width(),
            sheet_h: template.media_box.height(),
        });
    }
    let (leading, leading_derived) = template.resolved_leading();
    let per_page = lines_per_page(&column, template.size, leading);
    if per_page == 0 {
        return Err(PlaceTextError::PageTooShort {
            height: h,
            size: template.size,
            leading,
        });
    }

    // The SAME face setup `addtext` uses, from `addtext` — so the repertoire
    // this refuses against and the repertoire the emission encodes against are
    // one table, not two that agree today.
    let (inv, enc, _symbolic) = face_encoding(template.face);
    let (sections, census) = sanitise(text, &inv);

    if !census.unmappable.is_empty() && template.unmappable == Unmappable::Refuse {
        let chars: Vec<(char, usize)> = census.unmappable.iter().map(|(c, n)| (*c, *n)).collect();
        let total = chars.iter().map(|(_, n)| *n).sum();
        return Err(PlaceTextError::Unmappable {
            base_font: fontdata::std14_base_font_name(template.face).to_owned(),
            total,
            listing: describe_chars(&chars),
            chars,
        });
    }

    // Representative inter-word space, measured exactly as `layout_boxed`
    // measures it (the face's own space advance, with the 0.25·size floor).
    let space_code = inv
        .encode_str(" ", &std::collections::BTreeSet::new())
        .ok()
        .and_then(|r| r.codes.first().copied())
        .unwrap_or(b' ');
    let mut space_width = measure_codes(template.face, enc, &[space_code], template.size);
    let mut space_estimated = false;
    if space_width <= 0.0 {
        space_width = FALLBACK_SPACE_FRAC * template.size;
        space_estimated = true;
    }

    // Stages 2 and 3, named rather than inlined — see their own docs.
    let Some(wrapped) = wrap_paragraphs(&sections, template, &inv, enc, w, space_width) else {
        return Err(PlaceTextError::NoWordsToPlace);
    };
    let overlong_words = wrapped.overlong_words;
    let (pages, split_paragraphs) = cut_into_pages(&wrapped.lines, &sections, per_page);

    let chars_placed = pages.iter().map(|p| p.chars).sum();
    let lines_placed = pages.iter().map(|p| p.lines).sum();
    let blank_pages = pages.iter().filter(|p| p.text.is_empty()).count();
    let dropped_unmappable: Vec<(char, usize)> =
        census.unmappable.iter().map(|(c, n)| (*c, *n)).collect();
    let chars_dropped_unmappable = dropped_unmappable.iter().map(|(_, n)| *n).sum();

    let mut report = PlaceTextReport {
        pages_created: pages.len(),
        first_page_index: 0,
        blank_pages,
        lines_placed,
        lines_per_page: per_page,
        chars_input: census.chars_input,
        chars_placed,
        whitespace_normalised: census.whitespace,
        chars_dropped_control: census.control,
        chars_dropped_unmappable,
        dropped_unmappable_chars: dropped_unmappable,
        bom_stripped: census.bom_stripped,
        crlf_normalised: census.crlf,
        tabs_collapsed: census.tabs,
        explicit_page_breaks: census.page_breaks,
        overlong_words,
        paragraphs_split_across_pages: split_paragraphs,
        box_overflow_lines: 0,
        undo_entries: 0,
        coalesced: false,
        leading_derived,
        leading,
        disclosures: Vec::new(),
    };
    build_disclosures(&mut report, template, &column, space_estimated, space_width);

    Ok(PlacePlan {
        pages,
        column,
        leading,
        report,
    })
}

/// The wrapped lines of a whole import, plus what wrapping had to disclose.
struct Wrapped {
    /// Every line, in reading order, blank lines included.
    lines: Vec<PlannedLine>,
    /// Lines holding a single word too wide for the column.
    overlong_words: usize,
}

/// Wrap every paragraph of every section to `wrap_width`, remembering which
/// paragraph each resulting line came from — stage 2 of [`plan`].
///
/// The OUTPUT SHAPE is the load-bearing part. Each [`PlannedLine`] carries its
/// `(section, paragraph)` identity, and that is the only thing that lets
/// [`assemble_page`] rejoin a page's lines with the right separator: a space
/// inside a paragraph, a newline between paragraphs. Rejoining everything with
/// newlines produces a document that is identical under left/centre/right
/// alignment and silently unjustifiable under `justify`, which is a defect no
/// amount of staring at the output finds.
///
/// Returns `None` when the text tokenised to no words at all, which [`plan`]
/// turns into [`PlaceTextError::NoWordsToPlace`]. `None` rather than an empty
/// `Vec`, because a whitespace-only input DOES produce lines — blank ones — so
/// "no lines" and "no words" are different states and only one is a refusal.
fn wrap_paragraphs(
    sections: &[Section],
    template: &PageTemplate,
    inv: &InverseEncoding,
    enc: BaseEncoding,
    wrap_width: f64,
    space_width: f64,
) -> Option<Wrapped> {
    let mut lines: Vec<PlannedLine> = Vec::new();
    let mut overlong_words = 0usize;
    let mut any_word = false;
    for (si, section) in sections.iter().enumerate() {
        for (pi, para) in section.paragraphs.iter().enumerate() {
            if para.is_empty() {
                // A blank paragraph still consumes a line — it is the empty
                // line the operator typed, and swallowing it would change the
                // document's line structure.
                lines.push(PlannedLine {
                    section: si,
                    paragraph: pi,
                    words: 0..0,
                });
                continue;
            }
            any_word = true;
            let widths: Vec<f64> = para
                .iter()
                .map(|word| {
                    // Encoding cannot fail here: `sanitise` already diverted
                    // every scalar the face lacks into the census, so a word
                    // that reached this point is by construction encodable.
                    // `unwrap_or_default` rather than `expect` because this
                    // crate denies panics (lib.rs), and a zero width for an
                    // impossible case degrades to a wrong line break rather
                    // than to a dead process.
                    let codes = inv
                        .encode_str(word, &std::collections::BTreeSet::new())
                        .map(|r| r.codes)
                        .unwrap_or_default();
                    measure_codes(template.face, enc, &codes, template.size)
                })
                .collect();
            let ranges = greedy_pack(para.len(), wrap_width, |s, e| {
                natural_width(&widths, space_width, s, e)
            });
            for r in ranges {
                // A lone word on a line still wider than the column is the
                // unbreakable case: pdfcer has no hyphenation dictionary, so it
                // overflows and is counted rather than cut at a guess.
                if r.len() == 1 && natural_width(&widths, space_width, r.start, r.end) > wrap_width
                {
                    overlong_words += 1;
                }
                lines.push(PlannedLine {
                    section: si,
                    paragraph: pi,
                    words: r,
                });
            }
        }
    }
    any_word.then_some(Wrapped {
        lines,
        overlong_words,
    })
}

/// Cut the wrapped lines into pages of at most `per_page`, and count the
/// paragraphs a cut fell inside — stage 3 of [`plan`].
///
/// Two rules, and the second is the form-feed one: a **section** boundary
/// always starts a new page (that is what U+000C means), and otherwise a page
/// fills to `per_page` lines.
///
/// The returned count is of paragraphs a page break fell INSIDE — the ones that
/// reach [`add_text`](crate::text_edit::add_text) as two paragraphs and
/// therefore, under justification, leave the line before the break flush left.
/// A form-feed break is deliberately not counted: the operator put it there, so
/// the paragraph genuinely ended.
fn cut_into_pages(
    lines: &[PlannedLine],
    sections: &[Section],
    per_page: usize,
) -> (Vec<PagePlan>, usize) {
    let mut pages: Vec<PagePlan> = Vec::new();
    let mut split_paragraphs = 0usize;
    let mut current: Vec<&PlannedLine> = Vec::new();
    let mut current_section = lines.first().map_or(0, |l| l.section);
    for line in lines {
        let section_changed = line.section != current_section;
        if section_changed || current.len() == per_page {
            if !section_changed
                && current.last().is_some_and(|last| {
                    last.section == line.section && last.paragraph == line.paragraph
                })
            {
                split_paragraphs += 1;
            }
            pages.push(assemble_page(&current, sections));
            current.clear();
            current_section = line.section;
        }
        current.push(line);
    }
    pages.push(assemble_page(&current, sections));
    (pages, split_paragraphs)
}

/// Natural width of words `[start, end)`: the sum of their advances plus one
/// representative space per gap.
///
/// The `reflow::line_natural_width` formula, restated for a `&[f64]` this
/// module owns. Same measure, same breaker, so the same breaks.
fn natural_width(widths: &[f64], space: f64, start: usize, end: usize) -> f64 {
    let words: f64 = widths.get(start..end).map_or(0.0, |s| s.iter().sum());
    let gaps = end.saturating_sub(start).saturating_sub(1) as f64;
    words + gaps * space
}

/// Rejoin one page's lines into the string `add_text` will re-wrap.
///
/// Consecutive lines of the SAME paragraph rejoin with a space (they were one
/// paragraph and must wrap as one); a change of paragraph — or of section —
/// emits a `\n`, which is `layout_boxed`'s hard break. A blank line is a
/// paragraph with no words and therefore contributes nothing between its two
/// newlines, which is exactly how `layout_boxed` produces a `LaidLine::blank`.
fn assemble_page(lines: &[&PlannedLine], sections: &[Section]) -> PagePlan {
    let mut text = String::new();
    let mut chars = 0usize;
    let mut previous: Option<(usize, usize)> = None;
    for line in lines {
        match previous {
            None => {}
            Some((s, p)) if s == line.section && p == line.paragraph => text.push(' '),
            Some(_) => text.push('\n'),
        }
        previous = Some((line.section, line.paragraph));
        let words = sections
            .get(line.section)
            .and_then(|s| s.paragraphs.get(line.paragraph));
        if let Some(words) = words
            && let Some(slice) = words.get(line.words.clone())
        {
            for (i, word) in slice.iter().enumerate() {
                if i > 0 {
                    text.push(' ');
                }
                text.push_str(word);
                chars += word.chars().count();
            }
        }
    }
    PagePlan {
        // A page of nothing but blank lines rejoins to a string of newlines,
        // which `add_text` would refuse as `NoWordsToWrap`. Normalising it to
        // empty here is what makes "skip the call, keep the page" a decision
        // taken once rather than a refusal handled at the call site.
        text: if text.chars().all(char::is_whitespace) {
            String::new()
        } else {
            text
        },
        chars,
        lines: lines.len(),
    }
}

/// Render a character census as `U+2022 '•' x3, U+4E2D '中' x1`.
fn describe_chars(chars: &[(char, usize)]) -> String {
    chars
        .iter()
        .map(|(c, n)| format!("U+{:04X} {c:?} x{n}", *c as u32))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Assemble the operator-facing disclosure lines.
///
/// One paragraph per thing pdfcer decided, and **only** for things it actually
/// did: an unconditional "no tabs were found" line trains a reader to skip the
/// block, which is how the line that matters gets skipped too.
fn build_disclosures(
    report: &mut PlaceTextReport,
    template: &PageTemplate,
    column: &Rect,
    space_estimated: bool,
    space_width: f64,
) {
    let d = &mut report.disclosures;
    d.push(format!(
        "imported text was PAGINATED by pdfcer: {} line(s) at {:.2}pt leading{}, {} line(s) per \
         page, into a {:.1} x {:.1}pt column on a {:.1} x {:.1}pt sheet, set {} in {}pt '{}'. The \
         line breaks are DERIVED (greedy first-fit at ISO 32000-1 §9.4.4 advances) — the text \
         file has none",
        report.lines_placed,
        report.leading,
        if report.leading_derived {
            " (derived default 1.2 x size)"
        } else {
            ""
        },
        report.lines_per_page,
        column.width(),
        column.height(),
        template.media_box.width(),
        template.media_box.height(),
        template.alignment.as_str(),
        template.size,
        fontdata::std14_base_font_name(template.face),
    ));
    if report.bom_stripped {
        d.push(
            "a UTF-8 byte-order mark (U+FEFF) began this text and was stripped — it is a file \
             marker, not a character of the document"
                .to_owned(),
        );
    }
    if report.crlf_normalised > 0 {
        d.push(format!(
            "{} carriage return(s) were normalised away (CRLF and lone CR both read as one line \
             break) — no CR reaches the page",
            report.crlf_normalised
        ));
    }
    if report.tabs_collapsed > 0 {
        d.push(format!(
            "{} tab(s) were collapsed into ordinary word spacing. INDENTATION IS LOST: \
             WinAnsiEncoding assigns no code below 32 (ISO 32000-1 Annex D.2) and PDF text \
             showing has no tab stops, so there is nothing to preserve a tab as",
            report.tabs_collapsed
        ));
    }
    if report.explicit_page_breaks > 0 {
        d.push(format!(
            "{} form feed(s) (U+000C) were honoured as PAGE BREAKS — the separator \
             'export text' writes, so an exported-then-edited file keeps its pagination",
            report.explicit_page_breaks
        ));
    }
    if report.chars_dropped_control > 0 {
        d.push(format!(
            "{} non-printing control character(s) were removed — no Standard-14 encoding has a \
             code for them, so they could not have been drawn",
            report.chars_dropped_control
        ));
    }
    if report.chars_dropped_unmappable > 0 {
        d.push(format!(
            "★ {} character(s) were DROPPED because '{}' cannot represent them, at your explicit \
             request: {}. The imported document does NOT contain this text",
            report.chars_dropped_unmappable,
            fontdata::std14_base_font_name(template.face),
            describe_chars(&report.dropped_unmappable_chars)
        ));
    }
    if report.overlong_words > 0 {
        d.push(format!(
            "{} word(s) are wider than the {:.1}pt column and overflow their line unbroken — \
             pdfcer does not hyphenate (whitespace-only breaks)",
            report.overlong_words,
            column.width()
        ));
    }
    if report.blank_pages > 0 {
        d.push(format!(
            "{} created page(s) carry no text, because the input has that many consecutive blank \
             lines. They were kept rather than swallowed so the document has the input's line \
             structure",
            report.blank_pages
        ));
    }
    if report.paragraphs_split_across_pages > 0 && template.alignment.is_justified() {
        d.push(format!(
            "{} paragraph(s) continue across a page break; each page is wrapped as its own text, \
             so the line before each of those breaks is set FLUSH LEFT rather than justified \
             (a paragraph's last line is never stretched)",
            report.paragraphs_split_across_pages
        ));
    } else if report.paragraphs_split_across_pages > 0 {
        d.push(format!(
            "{} paragraph(s) continue across a page break",
            report.paragraphs_split_across_pages
        ));
    }
    if space_estimated {
        d.push(format!(
            "inter-word space width estimated at {space_width:.2}pt (0.25 x size) — the chosen \
             face reports no advance for its space glyph"
        ));
    }
}

// =====================================================================
// The scaffold document.
// =====================================================================

/// Build a minimal in-memory PDF holding `count` blank pages of `media`.
///
/// This is the primitive the engine did not have: **there is no public "create
/// a blank page" verb**, and every page-creating path in `pdfcer-core` copies
/// pages from somewhere. So rather than adding a second page-tree splice, this
/// builds a document worth splicing FROM and lets
/// [`EditSession::insert_pages`](crate::edit::EditSession::insert_pages) — which
/// already handles `/Count` propagation, object renumbering and stream
/// re-staging — do the work it already does.
///
/// The bytes are a classic §7.5.4 cross-reference table, not an xref stream:
/// the file is three lines long conceptually, it never leaves memory, and a
/// table is the form whose offsets are checkable by eye. Each entry is exactly
/// 20 bytes (`nnnnnnnnnn ggggg n \n`) as §7.5.4 requires.
///
/// Object numbering: 1 = catalog, 2 = the `/Pages` root, `3..3+count` = pages.
/// The root node carries **no** `/Parent` (Table 29: *"prohibited in the root
/// node"*), each page carries all four of Table 30's required entries, and no
/// page carries `/Contents` (Table 30: *"If this entry is absent, the page
/// shall be empty"* — whereas `/Contents []` would be a `shall not`).
fn scaffold_bytes(media: Rect, count: usize) -> Vec<u8> {
    let mut buf: Vec<u8> = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::with_capacity(count + 2);

    offsets.push(buf.len());
    buf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    offsets.push(buf.len());
    buf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [");
    for i in 0..count {
        if i > 0 {
            buf.push(b' ');
        }
        buf.extend_from_slice(format!("{} 0 R", i + 3).as_bytes());
    }
    buf.extend_from_slice(format!("] /Count {count} >>\nendobj\n").as_bytes());

    // The media box, emitted once through the writer's own number formatter so
    // a fractional sheet (A4 is 595.2755905511811 pt) round-trips the way every
    // other rectangle pdfcer writes does.
    let mut mediabox: Vec<u8> = Vec::new();
    for (i, v) in [media.llx, media.lly, media.urx, media.ury]
        .into_iter()
        .enumerate()
    {
        if i > 0 {
            mediabox.push(b' ');
        }
        emit_number(&mut mediabox, v);
    }

    for i in 0..count {
        offsets.push(buf.len());
        buf.extend_from_slice(
            format!("{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [", i + 3).as_bytes(),
        );
        buf.extend_from_slice(&mediabox);
        buf.extend_from_slice(b"] /Resources << >> >>\nendobj\n");
    }

    let xref_at = buf.len();
    let size = count + 3;
    buf.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// Build an in-memory document of `count` blank pages, `media` points each.
///
/// **The primitive `pdfcer-core` did not have.** Every other page-creating
/// path in this crate copies pages from an existing document — `pageops::merge`
/// and [`EditSession::insert_pages`](crate::edit::EditSession::insert_pages)
/// both splice, and neither can conjure a page out of nothing. That is why
/// `pdfcer-gui` could export text and not import it: there was no first page to
/// pour it onto.
///
/// [`EditSession::place_text`](crate::edit::EditSession::place_text) uses this
/// as the document it splices FROM. A shell needs it for the other half of the
/// same problem: `pdfcer place-text` with no `--input` has no document at all,
/// and `place_text` deliberately refuses to insert beside nothing
/// ([`PlaceTextError::NoPageToInsertBeside`]).
///
/// The document is a classic §7.5.4 cross-reference table with a catalog, a
/// root `/Pages` node (no `/Parent` — Table 29 prohibits one there) and `count`
/// pages carrying all four of Table 30's required entries. `/Resources` is an
/// **empty dictionary**, which Table 30 distinguishes from an absent one
/// (*"Omitting the entry entirely indicates that the resources shall be
/// inherited"*), and `/Contents` is **absent**, which the same table defines as
/// *"the page shall be empty"* — `/Contents []` would be a `shall not`.
///
/// `count == 0` produces a document with an empty page tree. The spec RAG has
/// **no** statement on whether that is conforming (searched 2026-09-06: no
/// minimum-page-count rule, no empty-`/Kids` handling, in either edition), so
/// pdfcer neither blesses nor blocks it here — but note that
/// [`crate::page_tree::pages`] will then return an empty list and most verbs
/// that take a page index will refuse.
///
/// # Errors
///
/// [`PlaceTextError::Scaffold`] if the bytes this function just wrote do not
/// load back. That "cannot happen", which is exactly why it is checked: the
/// writer here is thirty lines of hand-emitted offsets and the reader is the
/// real parser, so "it will obviously parse" is a claim and not evidence (R93).
///
/// # Examples
///
/// ```
/// use pdfcer_core::page_tree::{self, Rect};
/// use pdfcer_core::text_edit::blank_document;
///
/// let doc = blank_document(Rect::from_corners(0.0, 0.0, 612.0, 792.0), 3).unwrap();
/// assert_eq!(page_tree::pages(&doc).unwrap().len(), 3);
/// ```
pub fn blank_document(
    media: Rect,
    count: usize,
) -> Result<crate::document::Document, PlaceTextError> {
    crate::document::Document::from_bytes(scaffold_bytes(media, count))
        .map_err(PlaceTextError::Scaffold)
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

    fn helvetica() -> InverseEncoding {
        face_encoding(Std14::Helvetica).0
    }

    #[test]
    fn a_crlf_file_produces_the_same_paragraphs_as_an_lf_one() {
        let inv = helvetica();
        let (crlf, c1) = sanitise("one\r\ntwo\r\n\r\nthree", &inv);
        let (lf, c2) = sanitise("one\ntwo\n\nthree", &inv);
        assert_eq!(crlf, lf, "CRLF must normalise to the LF shape");
        assert_eq!(c1.crlf, 3);
        assert_eq!(c2.crlf, 0);
    }

    #[test]
    fn a_tab_is_a_word_gap_and_is_counted() {
        let inv = helvetica();
        let (sections, census) = sanitise("a\tb", &inv);
        assert_eq!(
            sections[0].paragraphs[0],
            vec!["a".to_owned(), "b".to_owned()]
        );
        assert_eq!(census.tabs, 1);
        assert_eq!(census.whitespace, 1);
    }

    #[test]
    fn a_form_feed_starts_a_new_section() {
        let inv = helvetica();
        let (sections, census) = sanitise("a\u{c}b", &inv);
        assert_eq!(sections.len(), 2);
        assert_eq!(census.page_breaks, 1);
    }

    #[test]
    fn a_leading_bom_is_stripped_but_a_later_one_is_not_whitespace() {
        let inv = helvetica();
        let (_, census) = sanitise("\u{feff}hi", &inv);
        assert!(census.bom_stripped);
        // A BOM anywhere else is an ordinary unmappable scalar, not a marker.
        let (_, census) = sanitise("hi\u{feff}", &inv);
        assert!(!census.bom_stripped);
        assert_eq!(census.unmappable.get(&'\u{feff}'), Some(&1));
    }

    #[test]
    fn every_input_scalar_lands_in_exactly_one_bucket() {
        let inv = helvetica();
        let text = "\u{feff}Hi\tthere\r\n\u{7}bad\u{c}\u{4e2d}end";
        let (sections, census) = sanitise(text, &inv);
        let placed: usize = sections
            .iter()
            .flat_map(|s| s.paragraphs.iter())
            .flat_map(|p| p.iter())
            .map(|w| w.chars().count())
            .sum();
        let unmappable: usize = census.unmappable.values().sum();
        assert_eq!(
            census.chars_input,
            placed
                + census.whitespace
                + census.control
                + unmappable
                + usize::from(census.bom_stripped),
            "every scalar must be placed, normalised, dropped, refused or stripped"
        );
    }

    #[test]
    fn the_line_count_matches_the_column_arithmetic() {
        // Letter with 72 pt margins: a 648 pt tall column. 12 pt text at the
        // derived 14.4 pt leading: first baseline at 720 - 9 = 711, and a line
        // fits while baseline - 3 >= 72.
        let template = PageTemplate::new();
        let column = template.text_column();
        let n = lines_per_page(&column, 12.0, 14.4);
        assert_eq!(n, 45);
        // The last line must sit inside the column and one more must not.
        let last = (720.0 - 9.0) - 14.4 * ((n - 1) as f64) - 3.0;
        assert!(last >= column.lly);
        let over = (720.0 - 9.0) - 14.4 * (n as f64) - 3.0;
        assert!(over < column.lly);
    }

    #[test]
    fn a_page_is_rejoined_so_a_wrapped_paragraph_stays_one_paragraph() {
        let sections = vec![Section {
            paragraphs: vec![vec!["a".to_owned(), "b".to_owned(), "c".to_owned()], vec![]],
        }];
        let l0 = PlannedLine {
            section: 0,
            paragraph: 0,
            words: 0..2,
        };
        let l1 = PlannedLine {
            section: 0,
            paragraph: 0,
            words: 2..3,
        };
        let l2 = PlannedLine {
            section: 0,
            paragraph: 1,
            words: 0..0,
        };
        let page = assemble_page(&[&l0, &l1, &l2], &sections);
        assert_eq!(page.text, "a b c\n");
        assert_eq!(page.chars, 3);
    }

    #[test]
    fn the_scaffold_loads_and_has_the_pages_it_claims() {
        let media = Rect::from_corners(0.0, 0.0, 612.0, 792.0);
        let doc = blank_document(media, 3).expect("scaffold loads");
        let pages = crate::page_tree::pages(&doc).expect("page tree");
        assert_eq!(pages.len(), 3);
        for page in &pages {
            assert!((page.media_box.width() - 612.0).abs() < 1e-9);
        }
    }
}

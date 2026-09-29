//! Block-level layout over extracted text: lines grouped into blocks in
//! reading order, each classified as a heading, paragraph, list item,
//! caption, running header, running footer or page number, with column
//! order, alignment and first-line indent.
//!
//! Every classification is an **inference** unless a tagged `/Artifact
//! /Subtype` (ISO 32000-2 Table 363) decided it. [`BlockSource`] says
//! which, and [`LayoutDiagnostics`] counts each decision so a caller can
//! disclose it (fuzzy-never-sneaky, `CLAUDE.md` rule 4).
//!
//! Analysis runs in display orientation (the page's `/Rotate`, Table 30,
//! applied); every box in the output is in default user space, like the
//! rest of extraction. Text that does not run left-to-right on the
//! displayed page is left out of the blocks and counted.
//!
//! The rules, all measured in multiples of the text's own font size:
//! - **Line:** runs on one baseline (within 0.4 em), split where the gap
//!   exceeds 0.8 em, except after a lone list marker.
//! - **Running header/footer:** the same text (digits and a lone roman
//!   numeral normalised) within 4 pt of the same distance from the page
//!   edge, inside the top/bottom margin band, on at least
//!   [`LayoutOptions::running_min_fraction`] of the pages and at least
//!   two. Never position alone. A header or footer whose text is a page
//!   number pattern becomes [`BlockKind::PageNumber`].
//! - **Columns:** x-gutters where line coverage falls to 15% of its peak,
//!   at least 0.5 em wide; a column narrower than 6 em or 12% of the text
//!   width, or without text beside it at the same height, is merged into
//!   its neighbour. A line crossing a gutter spans
//!   the page and splits the reading order into bands.
//! - **Block break:** column change, a baseline step over 1.5 em or
//!   upward, a size change over 15%, a weight change, a list marker, a
//!   caption lead, no horizontal overlap, or a new first-line indent after
//!   a short line or repeating the block's own.
//! - **Heading:** at most 3 lines and 200 characters, and either 15%
//!   larger than the body size or bold (at most 2 lines) where the body
//!   is not. Levels are ranked over the whole document by (size, bold),
//!   largest first, capped at 6.

use std::collections::HashMap;

use pdfcer_model::page_tree::{self, Rect};
use pdfcer_model::view::DocumentView;

use crate::text_extract::{
    ArtifactKind, ArtifactSubtype, ExtractError, ExtractOptions, ExtractedText, TextRun,
    extract_document_view,
};

/// Tuning for [`analyze_layout`] where the right answer depends on the
/// document.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct LayoutOptions {
    /// The fraction of extracted pages a header or footer must repeat on.
    /// Default `0.4`, so alternating odd/even headers qualify.
    pub running_min_fraction: f32,
    /// Depth of the top and bottom margin bands searched for running
    /// text, as a fraction of the page height. Default `0.15`.
    pub margin_band: f32,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            running_min_fraction: 0.4,
            margin_band: 0.15,
        }
    }
}

impl LayoutOptions {
    /// Sets [`Self::running_min_fraction`].
    #[must_use]
    pub const fn with_running_min_fraction(mut self, fraction: f32) -> Self {
        self.running_min_fraction = fraction;
        self
    }

    /// Sets [`Self::margin_band`].
    #[must_use]
    pub const fn with_margin_band(mut self, band: f32) -> Self {
        self.margin_band = band;
        self
    }
}

/// The page box and display rotation layout needs for one page.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PageGeometry {
    /// The visible region (the effective `/CropBox`), default user space.
    pub crop_box: Rect,
    /// Clockwise display rotation, normalised to 0, 90, 180 or 270.
    pub rotate: u16,
}

impl PageGeometry {
    /// A page's geometry.
    #[must_use]
    pub const fn new(crop_box: Rect, rotate: u16) -> Self {
        Self { crop_box, rotate }
    }
}

/// What a block is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BlockKind {
    /// A heading; level 1 is the largest style in the document.
    Heading {
        /// 1 to 6.
        level: u8,
    },
    /// Body text.
    Paragraph,
    /// A list item; `marker` is the bullet or number as it appears.
    ListItem {
        /// The leading marker, e.g. `•`, `3.`, `(b)`.
        marker: String,
    },
    /// A figure or table caption (`Figure 3`, `Table 2`, …).
    Caption,
    /// Text repeated at the top of most pages.
    RunningHeader,
    /// Text repeated at the bottom of most pages.
    RunningFooter,
    /// A running header or footer whose text is a page number.
    PageNumber,
}

/// Who decided a block's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BlockSource {
    /// pdfcer's geometry and typography heuristics.
    Inferred,
    /// The file's own `/Artifact /Subtype` tag.
    Tagged,
    /// A structure element in the file's structure tree
    /// ([`crate::tagged_layout`]).
    Structure,
}

/// How a block's lines are aligned, measured from their extents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Alignment {
    /// Flush left, ragged right.
    Left,
    /// Centred.
    Center,
    /// Flush right, ragged left.
    Right,
    /// Flush on both sides (last line excepted).
    Justified,
}

/// One line of text in reading order.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LayoutLine {
    /// Indices into the page's [`crate::text_extract::PageText::runs`],
    /// left to right.
    pub runs: Vec<usize>,
    /// The line's text, with a space wherever the gap between runs
    /// exceeds 0.15 em.
    pub text: String,
    /// Bounding box, default user space.
    pub bbox: Rect,
    /// The font size carrying the most characters, points.
    pub font_size: f32,
    /// Whether most characters are bold ([`crate::text_extract::FontWeight::is_bold`]).
    pub bold: bool,
}

/// A group of lines with one kind.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Block {
    /// What the block is.
    pub kind: BlockKind,
    /// Who decided [`Self::kind`].
    pub source: BlockSource,
    /// Indices into [`PageLayout::lines`], top to bottom.
    pub lines: Vec<usize>,
    /// Bounding box, default user space.
    pub bbox: Rect,
    /// Index into [`PageLayout::columns`]; `None` for a line that spans
    /// columns, for running text and on a single-column page.
    pub column: Option<usize>,
    /// Line alignment.
    pub alignment: Alignment,
    /// First line's left edge minus the other lines' leftmost edge, in
    /// points; negative for a hanging indent, 0 for one line.
    pub first_line_indent: f32,
    /// The font size carrying the most characters, points.
    pub font_size: f32,
    /// Whether most characters are bold.
    pub bold: bool,
}

impl Block {
    /// The block's text: its lines joined with a space.
    #[must_use]
    pub fn text(&self, page: &PageLayout) -> String {
        self.lines
            .iter()
            .filter_map(|&i| page.lines.get(i))
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// One page's layout.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PageLayout {
    /// Zero-based page number, as in [`crate::text_extract::PageText::page_index`].
    pub page_index: usize,
    /// Columns left to right in display order, default user space.
    /// Empty on a single-column page.
    pub columns: Vec<Rect>,
    /// Every line, in reading order.
    pub lines: Vec<LayoutLine>,
    /// Every block, in reading order.
    pub blocks: Vec<Block>,
}

/// A count per layout decision, so a caller can disclose what was
/// inferred.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[non_exhaustive]
pub struct LayoutDiagnostics {
    /// Pages laid out.
    pub pages: usize,
    /// Blocks produced.
    pub blocks: usize,
    /// Paragraph blocks.
    pub paragraphs: usize,
    /// Headings inferred from a larger size.
    pub headings_from_size: usize,
    /// Headings inferred from weight alone (bold at body size).
    pub headings_from_weight: usize,
    /// List items inferred from a leading marker.
    pub list_items: usize,
    /// Captions inferred from a `Figure`/`Table` lead.
    pub captions: usize,
    /// Running-header blocks inferred from repetition.
    pub running_headers: usize,
    /// Running-footer blocks inferred from repetition.
    pub running_footers: usize,
    /// Page-number blocks inferred from repetition plus pattern.
    pub page_numbers: usize,
    /// Header, footer or page-number blocks decided by an `/Artifact
    /// /Subtype` tag rather than inferred.
    pub tagged_artifact_blocks: usize,
    /// Blocks taken from structure elements ([`BlockSource::Structure`]);
    /// not inferences.
    pub structure_blocks: usize,
    /// Pages with more than one column.
    pub multi_column_pages: usize,
    /// Lines on a multi-column page that cross a gutter.
    pub spanning_lines: usize,
    /// Runs left out because they do not run left-to-right as displayed.
    pub runs_not_horizontal: usize,
    /// Runs left out as watermark or background artifacts.
    pub runs_watermark_skipped: usize,
    /// The body font size (the size carrying the most characters).
    pub body_font_size: Option<f32>,
}

impl LayoutDiagnostics {
    /// Every block whose kind came from heuristics rather than a tag, so
    /// `blocks - tagged_artifact_blocks`. Paragraphs count: calling a group
    /// of lines a paragraph is as much an inference as calling it a heading,
    /// and it matches the per-block [`BlockSource::Inferred`].
    #[must_use]
    pub const fn inferred(&self) -> usize {
        self.paragraphs
            + self.headings_from_size
            + self.headings_from_weight
            + self.list_items
            + self.captions
            + self.running_headers
            + self.running_footers
            + self.page_numbers
    }
}

/// A document's layout, with the extraction its run indices refer to.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DocumentLayout {
    /// The extraction [`LayoutLine::runs`] indexes into.
    pub text: ExtractedText,
    /// One entry per extracted page, in page order.
    pub pages: Vec<PageLayout>,
    /// Decision counts.
    pub diagnostics: LayoutDiagnostics,
}

/// Extracts the document's text and lays it out.
///
/// # Errors
///
/// Whatever [`extract_document_view`] or the page-tree walk returns.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_model::document::Document;
/// use pdfcer_text::block_layout::{LayoutOptions, analyze_layout};
/// use pdfcer_text::text_extract::ExtractOptions;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::load("in.pdf".as_ref())?;
/// let layout = analyze_layout(&doc.view(), &ExtractOptions::default(), &LayoutOptions::default())?;
/// for page in &layout.pages {
///     for block in &page.blocks {
///         println!("{:?}: {}", block.kind, block.text(page));
///     }
/// }
/// # Ok(()) }
/// ```
pub fn analyze_layout(
    view: &DocumentView<'_>,
    extract: &ExtractOptions,
    options: &LayoutOptions,
) -> Result<DocumentLayout, ExtractError> {
    let text = extract_document_view(view, extract)?;
    let pages = page_tree::pages_in(view)?;
    let geometry: Vec<PageGeometry> = text
        .pages
        .iter()
        .map(|p| {
            pages.get(p.page_index).map_or(
                PageGeometry::new(Rect::from_corners(0.0, 0.0, 612.0, 792.0), 0),
                |pg| PageGeometry::new(pg.crop_box, pg.rotate),
            )
        })
        .collect();
    Ok(layout_text(text, &geometry, options))
}

/// Lays out an existing extraction. `geometry[i]` belongs to
/// `text.pages[i]`; a missing entry is treated as US Letter, unrotated.
#[must_use]
pub fn layout_text(
    text: ExtractedText,
    geometry: &[PageGeometry],
    options: &LayoutOptions,
) -> DocumentLayout {
    let mut diag = LayoutDiagnostics {
        pages: text.pages.len(),
        ..LayoutDiagnostics::default()
    };
    let fallback = PageGeometry::new(Rect::from_corners(0.0, 0.0, 612.0, 792.0), 0);
    let geos: Vec<PageGeometry> = (0..text.pages.len())
        .map(|i| geometry.get(i).copied().unwrap_or(fallback))
        .collect();

    let mut frags: Vec<Vec<Frag>> = text
        .pages
        .iter()
        .zip(&geos)
        .map(|(p, g)| page_fragments(&p.runs, g.rotate, &mut diag))
        .collect();

    let provisional_body = body_style(&frags).0;
    classify_running(&mut frags, &geos, options, provisional_body);

    let (body_size, body_bold) = body_style(&frags);
    diag.body_font_size = body_size.map(|s| s as f32);
    let body = body_size.unwrap_or(10.0);

    let mut pages: Vec<PageLayout> = text
        .pages
        .iter()
        .zip(frags)
        .zip(&geos)
        .map(|((p, f), g)| layout_page(p.page_index, f, g, body, &mut diag))
        .collect();

    assign_headings(&mut pages, body, body_bold, &mut diag);

    for page in &pages {
        for b in &page.blocks {
            diag.blocks += 1;
            if b.source == BlockSource::Tagged {
                diag.tagged_artifact_blocks += 1;
                continue;
            }
            match b.kind {
                BlockKind::Paragraph => diag.paragraphs += 1,
                BlockKind::ListItem { .. } => diag.list_items += 1,
                BlockKind::Caption => diag.captions += 1,
                BlockKind::RunningHeader => diag.running_headers += 1,
                BlockKind::RunningFooter => diag.running_footers += 1,
                BlockKind::PageNumber => diag.page_numbers += 1,
                BlockKind::Heading { .. } => {}
            }
        }
    }

    DocumentLayout {
        text,
        pages,
        diagnostics: diag,
    }
}

// ---------------------------------------------------------------------------
// Display space
// ---------------------------------------------------------------------------

fn to_display(rotate: u16, x: f64, y: f64) -> (f64, f64) {
    match rotate {
        90 => (y, -x),
        180 => (-x, -y),
        270 => (-y, x),
        _ => (x, y),
    }
}

fn from_display(rotate: u16, x: f64, y: f64) -> (f64, f64) {
    match rotate {
        90 => (-y, x),
        180 => (-x, -y),
        270 => (y, -x),
        _ => (x, y),
    }
}

/// `(x0, x1, bottom, top)` of a user-space rectangle, displayed.
pub(crate) fn rect_to_display(rotate: u16, r: &Rect) -> (f64, f64, f64, f64) {
    let (ax, ay) = to_display(rotate, r.llx, r.lly);
    let (bx, by) = to_display(rotate, r.urx, r.ury);
    (ax.min(bx), ax.max(bx), ay.min(by), ay.max(by))
}

/// The user-space rectangle displayed as `(x0, x1, bottom, top)`; the
/// inverse of [`rect_to_display`].
pub(crate) fn display_to_rect(rotate: u16, x0: f64, x1: f64, bottom: f64, top: f64) -> Rect {
    let (ax, ay) = from_display(rotate, x0, bottom);
    let (bx, by) = from_display(rotate, x1, top);
    Rect::from_corners(ax, ay, bx, by)
}

/// The smallest rectangle containing `a` and `b`.
pub(crate) fn union(a: &Rect, b: &Rect) -> Rect {
    Rect::from_corners(
        a.llx.min(b.llx),
        a.lly.min(b.lly),
        a.urx.max(b.urx),
        a.ury.max(b.ury),
    )
}

// ---------------------------------------------------------------------------
// Lines
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Body,
    Header(BlockSource),
    Footer(BlockSource),
    PageNumber(BlockSource),
}

#[derive(Debug, Clone)]
struct Item {
    run: usize,
    x0: f64,
    x1: f64,
    bottom: f64,
    top: f64,
    baseline: f64,
    size: f64,
    chars: usize,
    bold_chars: usize,
}

#[derive(Debug, Clone)]
struct Frag {
    items: Vec<Item>,
    text: String,
    x0: f64,
    x1: f64,
    bottom: f64,
    top: f64,
    baseline: f64,
    size: f64,
    bold: bool,
    user: Rect,
    class: Class,
    tagged: Option<ArtifactSubtype>,
}

/// A line this much larger than body text reads as a heading.
const HEADING_SIZE_RATIO: f64 = 1.15;

fn is_skipped_artifact(run: &TextRun) -> bool {
    run.artifact == Some(ArtifactKind::Background)
        || run.artifact_subtype == Some(ArtifactSubtype::Watermark)
}

fn page_fragments(runs: &[TextRun], rotate: u16, diag: &mut LayoutDiagnostics) -> Vec<Frag> {
    let mut items = Vec::new();
    for (i, run) in runs.iter().enumerate() {
        let Some(bbox) = run.bbox else { continue };
        if run.text.trim().is_empty() {
            continue;
        }
        if is_skipped_artifact(run) {
            diag.runs_watermark_skipped += 1;
            continue;
        }
        let (dx, dy) = run.direction();
        let (ddx, _) = to_display(rotate, f64::from(dx), f64::from(dy));
        if ddx < 0.9 {
            diag.runs_not_horizontal += 1;
            continue;
        }
        let (x0, x1, bottom, top) = rect_to_display(rotate, &bbox);
        let mut sizes: HashMap<i64, usize> = HashMap::new();
        let mut bold_chars = 0;
        for g in &run.glyphs {
            *sizes.entry(half_points(f64::from(g.size))).or_default() += 1;
            if g.weight.is_bold() {
                bold_chars += 1;
            }
        }
        let size = sizes
            .iter()
            .max_by_key(|&(s, n)| (*n, *s))
            .map_or((top - bottom) * 0.8, |(s, _)| *s as f64 / 2.0)
            .max(1.0);
        let baseline = run
            .glyphs
            .first()
            .map_or(bottom + (top - bottom) * 0.2, |g| {
                to_display(rotate, f64::from(g.x), f64::from(g.y)).1
            });
        let chars = run.glyphs.len().max(run.text.chars().count());
        items.push(Item {
            run: i,
            x0,
            x1,
            bottom,
            top,
            baseline,
            size,
            chars,
            bold_chars,
        });
    }

    items.sort_by(|a, b| b.baseline.total_cmp(&a.baseline));
    let mut rows: Vec<Vec<Item>> = Vec::new();
    for item in items {
        match rows.last_mut() {
            Some(row)
                if row.first().is_some_and(|f| {
                    (f.baseline - item.baseline).abs() <= 0.4 * f.size.max(item.size)
                }) =>
            {
                row.push(item);
            }
            _ => rows.push(vec![item]),
        }
    }

    let mut frags = Vec::new();
    for mut row in rows {
        row.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        let mut current: Vec<Item> = Vec::new();
        for item in row {
            if let Some(prev) = current.last() {
                let gap = item.x0 - prev.x1;
                let text_so_far: String = current
                    .iter()
                    .filter_map(|it| runs.get(it.run))
                    .map(|r| r.text.as_str())
                    .collect();
                if gap > 0.8 * prev.size.max(item.size)
                    && list_marker(text_so_far.trim()).is_none_or(|(_, rest)| !rest.is_empty())
                {
                    frags.push(make_frag(std::mem::take(&mut current), runs, rotate));
                }
            }
            current.push(item);
        }
        if !current.is_empty() {
            frags.push(make_frag(current, runs, rotate));
        }
    }
    frags
}

fn make_frag(items: Vec<Item>, runs: &[TextRun], rotate: u16) -> Frag {
    let mut text = String::new();
    let mut prev_x1: Option<(f64, f64)> = None;
    let mut sizes: HashMap<i64, usize> = HashMap::new();
    let (mut chars, mut bold_chars) = (0, 0);
    let mut user: Option<Rect> = None;
    let mut tagged: Option<Option<ArtifactSubtype>> = None;
    for it in &items {
        let Some(run) = runs.get(it.run) else {
            continue;
        };
        if let Some((x1, size)) = prev_x1
            && it.x0 - x1 > 0.15 * size.max(it.size)
            && !text.ends_with(char::is_whitespace)
            && !run.text.starts_with(char::is_whitespace)
        {
            text.push(' ');
        }
        text.push_str(&run.text);
        prev_x1 = Some((it.x1, it.size));
        *sizes.entry(half_points(it.size)).or_default() += it.chars;
        chars += it.chars;
        bold_chars += it.bold_chars;
        if let Some(b) = run.bbox {
            user = Some(user.map_or(b, |u| union(&u, &b)));
        }
        let sub = run
            .artifact
            .as_ref()
            .and(run.artifact_subtype.clone())
            .filter(|s| matches!(s, ArtifactSubtype::Header | ArtifactSubtype::Footer));
        tagged = Some(match tagged {
            None => sub,
            Some(t) if t == sub => t,
            Some(_) => None,
        });
    }
    let x0 = items.iter().map(|i| i.x0).fold(f64::INFINITY, f64::min);
    let x1 = items.iter().map(|i| i.x1).fold(f64::NEG_INFINITY, f64::max);
    let bottom = items.iter().map(|i| i.bottom).fold(f64::INFINITY, f64::min);
    let top = items
        .iter()
        .map(|i| i.top)
        .fold(f64::NEG_INFINITY, f64::max);
    let size = sizes
        .iter()
        .max_by_key(|&(s, n)| (*n, *s))
        .map_or(10.0, |(s, _)| *s as f64 / 2.0);
    let baseline = items.first().map_or(bottom, |i| i.baseline);
    Frag {
        text: text.trim().to_owned(),
        x0,
        x1,
        bottom,
        top,
        baseline,
        size,
        bold: bold_chars * 2 > chars,
        user: user.unwrap_or_else(|| display_to_rect(rotate, x0, x1, bottom, top)),
        class: Class::Body,
        tagged: tagged.flatten(),
        items,
    }
}

fn half_points(size: f64) -> i64 {
    // Rounded to the nearest half point; sizes are bounded by the page.
    #[allow(clippy::cast_possible_truncation)]
    let v = (size * 2.0).round() as i64;
    v
}

// ---------------------------------------------------------------------------
// Running headers and footers
// ---------------------------------------------------------------------------

/// A canonical lowercase roman numeral from 1 to 3999 (`mid` is not one).
fn is_roman(s: &str) -> bool {
    const UNITS: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    if s.is_empty() || s.len() > 15 {
        return false;
    }
    let mut rest = s;
    let mut value = 0u32;
    for (v, sym) in UNITS {
        let mut repeats = 0;
        while let Some(r) = rest.strip_prefix(sym) {
            rest = r;
            value += v;
            repeats += 1;
            if repeats > 3 {
                return false;
            }
        }
    }
    rest.is_empty() && value > 0 && {
        let mut canonical = String::new();
        let mut n = value;
        for (v, sym) in UNITS {
            while n >= v {
                canonical.push_str(sym);
                n -= v;
            }
        }
        canonical == s
    }
}

/// Lowercase, whitespace collapsed, digit runs and a lone roman numeral
/// replaced by `#`.
fn normalise(text: &str) -> String {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let roman_alone = match words.as_slice() {
        [w] => is_roman(w),
        ["page", w] => is_roman(w),
        _ => false,
    };
    let mut out = String::new();
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        if roman_alone && is_roman(w) {
            out.push('#');
            continue;
        }
        let mut in_digits = false;
        for c in w.chars() {
            if c.is_ascii_digit() {
                if !in_digits {
                    out.push('#');
                }
                in_digits = true;
            } else {
                out.push(c);
                in_digits = false;
            }
        }
    }
    out
}

fn is_page_number(text: &str) -> bool {
    let n = normalise(text);
    let n = n.trim_matches(|c: char| matches!(c, '-' | '–' | '—' | '[' | ']' | '(' | ')' | ' '));
    matches!(
        n,
        "#" | "page #" | "p. #" | "# of #" | "page # of #" | "#/#" | "# / #"
    )
}

/// `(page, distance from the page edge, fragment)`.
type Member = (usize, f64, usize);

/// `body` is the body size before any running text is set aside; a
/// heading-sized group whose text differs page to page (`Chapter 1`,
/// `Chapter 2`) is a sequence of headings, not running text.
fn classify_running(
    frags: &mut [Vec<Frag>],
    geos: &[PageGeometry],
    options: &LayoutOptions,
    body: Option<f64>,
) {
    // Tagged first: the file's own word beats any heuristic.
    for page in frags.iter_mut() {
        for f in page.iter_mut() {
            let class = match &f.tagged {
                Some(_) if is_page_number(&f.text) => Class::PageNumber(BlockSource::Tagged),
                Some(ArtifactSubtype::Header) => Class::Header(BlockSource::Tagged),
                Some(ArtifactSubtype::Footer) => Class::Footer(BlockSource::Tagged),
                _ => continue,
            };
            f.class = class;
        }
    }

    let page_count = frags.len();
    if page_count < 2 {
        return;
    }
    // (is_top, normalised text) -> [(page, distance from edge, frag)]
    let mut groups: HashMap<(bool, String), Vec<Member>> = HashMap::new();
    for (p, (page, geo)) in frags.iter().zip(geos).enumerate() {
        let (_, _, pb, pt) = rect_to_display(geo.rotate, &geo.crop_box);
        let band = (pt - pb) * f64::from(options.margin_band);
        for (i, f) in page.iter().enumerate() {
            if f.class != Class::Body || f.text.is_empty() {
                continue;
            }
            if f.bottom >= pt - band {
                groups
                    .entry((true, normalise(&f.text)))
                    .or_default()
                    .push((p, pt - f.top, i));
            } else if f.top <= pb + band {
                groups
                    .entry((false, normalise(&f.text)))
                    .or_default()
                    .push((p, f.bottom - pb, i));
            }
        }
    }
    let fraction = f64::from(options.running_min_fraction.clamp(0.0, 1.0));
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let needed = ((page_count as f64 * fraction).ceil() as usize).max(2);
    for ((is_top, _), mut members) in groups {
        members.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut best: &[Member] = &[];
        let mut best_pages = 0;
        for start in 0..members.len() {
            let Some(&(_, d0, _)) = members.get(start) else {
                continue;
            };
            let end = members
                .iter()
                .skip(start)
                .take_while(|m| m.1 - d0 <= 4.0)
                .count()
                + start;
            let window = members.get(start..end).unwrap_or(&[]);
            let mut pages: Vec<usize> = window.iter().map(|m| m.0).collect();
            pages.dedup();
            if pages.len() > best_pages {
                best_pages = pages.len();
                best = window;
            }
        }
        if best_pages < needed {
            continue;
        }
        let texts_differ = best.windows(2).any(|w| {
            let text = |m: &Member| frags.get(m.0).and_then(|pg| pg.get(m.2)).map(|f| &f.text);
            matches!(w, [a, b] if text(a) != text(b))
        });
        let heading_sized = body.is_some_and(|body| {
            best.iter().all(|m| {
                frags
                    .get(m.0)
                    .and_then(|pg| pg.get(m.2))
                    .is_some_and(|f| f.size >= HEADING_SIZE_RATIO * body)
            })
        });
        if texts_differ && heading_sized {
            continue;
        }
        for &(p, _, i) in best {
            if let Some(f) = frags.get_mut(p).and_then(|pg| pg.get_mut(i)) {
                f.class = if is_page_number(&f.text) {
                    Class::PageNumber(BlockSource::Inferred)
                } else if is_top {
                    Class::Header(BlockSource::Inferred)
                } else {
                    Class::Footer(BlockSource::Inferred)
                };
            }
        }
    }
}

/// The size carrying the most body characters, and whether most of
/// those characters are bold.
fn body_style(frags: &[Vec<Frag>]) -> (Option<f64>, bool) {
    let mut by_size: HashMap<i64, (usize, usize)> = HashMap::new();
    for f in frags.iter().flatten().filter(|f| f.class == Class::Body) {
        for it in &f.items {
            let e = by_size.entry(half_points(it.size)).or_default();
            e.0 += it.chars;
            e.1 += it.bold_chars;
        }
    }
    by_size
        .into_iter()
        .max_by_key(|&(s, (n, _))| (n, s))
        .map_or((None, false), |(s, (n, bold))| {
            (Some(s as f64 / 2.0), bold * 2 > n)
        })
}

// ---------------------------------------------------------------------------
// Columns, reading order, blocks
// ---------------------------------------------------------------------------

/// Column x-intervals, display space, left to right; one entry when the
/// page has a single column.
fn find_columns(frags: &[Frag], body: f64) -> Vec<(f64, f64)> {
    let body_frags: Vec<&Frag> = frags.iter().filter(|f| f.class == Class::Body).collect();
    let min_x = body_frags
        .iter()
        .map(|f| f.x0)
        .fold(f64::INFINITY, f64::min);
    let max_x = body_frags
        .iter()
        .map(|f| f.x1)
        .fold(f64::NEG_INFINITY, f64::max);
    if !min_x.is_finite() || !max_x.is_finite() || max_x <= min_x {
        return vec![(0.0, 0.0)];
    }
    let width = max_x - min_x;
    let step = (width / 4000.0).max(1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = (width / step).ceil() as usize + 1;
    let mut cover = vec![0usize; n];
    for f in &body_frags {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (a, b) = (
            ((f.x0 - min_x) / step).floor() as usize,
            ((f.x1 - min_x) / step).ceil() as usize,
        );
        for c in cover.iter_mut().take(b.min(n)).skip(a) {
            *c += 1;
        }
    }
    let peak = cover.iter().copied().max().unwrap_or(0);
    if peak < 4 {
        return vec![(min_x, max_x)];
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let low = (peak as f64 * 0.15).floor() as usize;
    let mut gutters: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        if cover.get(i).is_some_and(|&c| c <= low) {
            let start = i;
            while i < n && cover.get(i).is_some_and(|&c| c <= low) {
                i += 1;
            }
            #[allow(clippy::cast_precision_loss)]
            let wide = (i - start) as f64 * step >= 0.5 * body;
            if start > 0 && i < n && wide {
                gutters.push((start, i));
            }
        } else {
            i += 1;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let at = |b: usize| min_x + b as f64 * step;
    loop {
        let mut cols = Vec::new();
        let mut left = min_x;
        for &(s, e) in &gutters {
            cols.push((left, at(s)));
            left = at(e);
        }
        cols.push((left, max_x));
        let narrow = cols.iter().position(|&(a, b)| {
            let lines = body_frags
                .iter()
                .filter(|f| f.x0 >= a - 0.5 * body && f.x1 <= b + 0.5 * body)
                .count();
            b - a < (6.0 * body).max(0.12 * width) || lines < 3
        });
        let inside = |&(a, b): &(f64, f64)| -> Vec<&&Frag> {
            body_frags
                .iter()
                .filter(|f| f.x0 >= a - 0.5 * body && f.x1 <= b + 0.5 * body)
                .collect()
        };
        // A gutter needs text beside it on both sides at the same height;
        // otherwise it is the ragged edge of one column (a right-aligned or
        // centred block), not a gap between two.
        let lonely = cols.windows(2).position(|pair| {
            let (Some(l), Some(r)) = (pair.first(), pair.get(1)) else {
                return false;
            };
            let (left, right) = (inside(l), inside(r));
            let beside = |a: &[&&Frag], b: &[&&Frag]| {
                let lo = b.iter().map(|f| f.bottom).fold(f64::INFINITY, f64::min);
                let hi = b.iter().map(|f| f.top).fold(f64::NEG_INFINITY, f64::max);
                a.iter()
                    .filter(|f| (f.bottom + f.top) / 2.0 >= lo && (f.bottom + f.top) / 2.0 <= hi)
                    .count()
            };
            beside(&left, &right) < 2 || beside(&right, &left) < 2
        });
        match (narrow, lonely) {
            (None, None) => return cols,
            _ if gutters.is_empty() => return cols,
            (Some(c), _) => {
                let g = c.min(gutters.len() - 1);
                gutters.remove(g);
            }
            (None, Some(g)) => {
                gutters.remove(g);
            }
        }
    }
}

/// `(zone, band, spanning, column, -baseline)`; zone 0 is top running
/// text, 1 the body, 2 bottom running text.
type OrderKey = (u8, usize, u8, usize, i64);

fn layout_page(
    page_index: usize,
    frags: Vec<Frag>,
    geo: &PageGeometry,
    body: f64,
    diag: &mut LayoutDiagnostics,
) -> PageLayout {
    let (page_x0, page_x1, pb, pt) = rect_to_display(geo.rotate, &geo.crop_box);
    let mid = (pb + pt) / 2.0;
    let cols = find_columns(&frags, body);
    let multi = cols.len() > 1;
    if multi {
        diag.multi_column_pages += 1;
    }
    let tol = 0.5 * body;
    let column_of = |f: &Frag| -> Option<usize> {
        if !multi || f.class != Class::Body {
            return None;
        }
        cols.iter()
            .position(|&(a, b)| f.x0 >= a - tol && f.x1 <= b + tol)
    };

    // Reading order: running text at the top, then bands split by lines
    // spanning the columns, then running text at the bottom.
    let mut spanners: Vec<f64> = frags
        .iter()
        .filter(|f| f.class == Class::Body && multi && column_of(f).is_none())
        .map(|f| f.baseline)
        .collect();
    spanners.sort_by(|a, b| b.total_cmp(a));
    diag.spanning_lines += spanners.len();

    let mut keyed: Vec<(OrderKey, Frag, Option<usize>)> = frags
        .into_iter()
        .map(|f| {
            let col = column_of(&f);
            #[allow(clippy::cast_possible_truncation)]
            let y = (-f.baseline * 100.0).round() as i64;
            let key = if f.class == Class::Body {
                let band = spanners.iter().filter(|&&s| s > f.baseline).count();
                match col {
                    Some(c) => (1, band, 0, c, y),
                    None if multi => (1, band, 1, 0, y),
                    None => (1, 0, 0, 0, y),
                }
            } else if f.baseline >= mid {
                (0, 0, 0, 0, y)
            } else {
                (2, 0, 0, 0, y)
            };
            (key, f, col)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.x0.total_cmp(&b.1.x0)));

    let body_x0 = keyed
        .iter()
        .filter(|k| k.1.class == Class::Body)
        .map(|k| k.1.x0)
        .fold(f64::INFINITY, f64::min);
    let body_x1 = keyed
        .iter()
        .filter(|k| k.1.class == Class::Body)
        .map(|k| k.1.x1)
        .fold(f64::NEG_INFINITY, f64::max);

    let columns = if multi {
        let bottom = keyed
            .iter()
            .map(|k| k.1.bottom)
            .fold(f64::INFINITY, f64::min);
        let top = keyed
            .iter()
            .map(|k| k.1.top)
            .fold(f64::NEG_INFINITY, f64::max);
        cols.iter()
            .map(|&(a, b)| display_to_rect(geo.rotate, a, b, bottom, top))
            .collect()
    } else {
        Vec::new()
    };

    // Blocks.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, (_, f, col)) in keyed.iter().enumerate() {
        let start_new = match groups
            .last()
            .and_then(|g| g.last())
            .and_then(|&p| keyed.get(p))
        {
            None => true,
            Some((_, prev, prev_col)) => {
                let block: Vec<&Frag> = groups
                    .last()
                    .into_iter()
                    .flatten()
                    .filter_map(|&j| keyed.get(j))
                    .map(|k| &k.1)
                    .collect();
                let block_x0 = block.iter().map(|b| b.x0).fold(f64::INFINITY, f64::min);
                let block_x1 = block.iter().map(|b| b.x1).fold(f64::NEG_INFINITY, f64::max);
                let first_x0 = block.first().map_or(block_x0, |b| b.x0);
                let block_len = groups.last().map_or(0, Vec::len);
                let block_is_list = groups
                    .last()
                    .and_then(|g| g.first())
                    .and_then(|&j| keyed.get(j))
                    .is_some_and(|k| list_marker(&k.1.text).is_some());
                let em = prev.size.max(f.size);
                let step = prev.baseline - f.baseline;
                prev.class != f.class
                    || prev_col != col
                    || step > 1.5 * em
                    || step < 0.3 * em
                    || (prev.size - f.size).abs() > 0.15 * prev.size.min(f.size)
                    || prev.bold != f.bold
                    || list_marker(&f.text).is_some()
                    || is_caption(&f.text)
                    || f.x0 > prev.x1
                    || f.x1 < prev.x0
                    || (!block_is_list
                        && block_len >= 2
                        && prev.x0 <= block_x0 + 0.3 * em
                        && f.x0 > prev.x0 + 0.8 * em
                        && (prev.x1 < block_x1 - em
                            || (first_x0 - block_x0 > 0.8 * em
                                && (f.x0 - first_x0).abs() <= 0.3 * em)))
            }
        };
        if start_new {
            groups.push(vec![i]);
        } else if let Some(g) = groups.last_mut() {
            g.push(i);
        }
    }

    let mut blocks = Vec::new();
    for g in &groups {
        let members: Vec<&Frag> = g
            .iter()
            .filter_map(|&i| keyed.get(i))
            .map(|k| &k.1)
            .collect();
        let Some(first) = members.first() else {
            continue;
        };
        let col = g.first().and_then(|&i| keyed.get(i)).and_then(|k| k.2);
        // Running text sits outside the body column, so it is aligned
        // against the page.
        let (ca, cb) = if first.class == Class::Body {
            col.and_then(|c| cols.get(c).copied())
                .unwrap_or((body_x0, body_x1))
        } else {
            (page_x0, page_x1)
        };
        let (kind, source) = match first.class {
            Class::Header(s) => (BlockKind::RunningHeader, s),
            Class::Footer(s) => (BlockKind::RunningFooter, s),
            Class::PageNumber(s) => (BlockKind::PageNumber, s),
            Class::Body => {
                let kind = if let Some((marker, _)) = list_marker(&first.text) {
                    BlockKind::ListItem {
                        marker: marker.to_owned(),
                    }
                } else if is_caption(&first.text) {
                    BlockKind::Caption
                } else {
                    BlockKind::Paragraph
                };
                (kind, BlockSource::Inferred)
            }
        };
        let bbox = members
            .iter()
            .skip(1)
            .fold(first.user, |acc, f| union(&acc, &f.user));
        let mut sizes: HashMap<i64, usize> = HashMap::new();
        let (mut chars, mut bold_chars) = (0, 0);
        for f in &members {
            for it in &f.items {
                *sizes.entry(half_points(it.size)).or_default() += it.chars;
                chars += it.chars;
                bold_chars += it.bold_chars;
            }
        }
        let size = sizes
            .iter()
            .max_by_key(|&(s, n)| (*n, *s))
            .map_or(first.size, |(s, _)| *s as f64 / 2.0);
        let rest_x0 = members
            .iter()
            .skip(1)
            .map(|f| f.x0)
            .fold(f64::INFINITY, f64::min);
        let indent = if rest_x0.is_finite() {
            first.x0 - rest_x0
        } else {
            0.0
        };
        #[allow(clippy::cast_possible_truncation)]
        blocks.push(Block {
            kind,
            source,
            lines: g.clone(),
            bbox,
            column: col,
            alignment: alignment(&members, ca, cb, size),
            first_line_indent: indent as f32,
            font_size: size as f32,
            bold: bold_chars * 2 > chars,
        });
    }

    let lines = keyed
        .into_iter()
        .map(|(_, f, _)| {
            #[allow(clippy::cast_possible_truncation)]
            LayoutLine {
                runs: f.items.iter().map(|i| i.run).collect(),
                text: f.text,
                bbox: f.user,
                font_size: f.size as f32,
                bold: f.bold,
            }
        })
        .collect();

    PageLayout {
        page_index,
        columns,
        lines,
        blocks,
    }
}

fn alignment(lines: &[&Frag], col_x0: f64, col_x1: f64, size: f64) -> Alignment {
    let tol = 0.5 * size;
    let spread = |v: &mut dyn Iterator<Item = f64>| {
        let (lo, hi) = v.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
            (lo.min(x), hi.max(x))
        });
        if lo.is_finite() { hi - lo } else { 0.0 }
    };
    match lines {
        [] => Alignment::Left,
        [only] => {
            let left = only.x0 - col_x0;
            let right = col_x1 - only.x1;
            if left > 2.0 * size && (left - right).abs() <= size {
                Alignment::Center
            } else if left > 2.0 * size && right <= tol {
                Alignment::Right
            } else {
                Alignment::Left
            }
        }
        [first, rest @ ..] => {
            let lefts = spread(&mut rest.iter().map(|f| f.x0));
            let lefts_all = spread(&mut lines.iter().map(|f| f.x0));
            let but_last = lines.len() - 1;
            let rights = spread(&mut lines.iter().take(but_last).map(|f| f.x1));
            let rights_all = spread(&mut lines.iter().map(|f| f.x1));
            let centres = spread(&mut lines.iter().map(|f| (f.x0 + f.x1) / 2.0));
            let left_flush = lefts <= tol && (first.x0 - rest_min(rest)).abs() <= 3.0 * size;
            let justified = if lines.len() >= 3 {
                rights <= tol
            } else {
                rights_all <= tol
            };
            if left_flush && justified {
                Alignment::Justified
            } else if left_flush {
                Alignment::Left
            } else if rights_all <= tol && lefts_all > tol {
                Alignment::Right
            } else if centres <= tol {
                Alignment::Center
            } else {
                Alignment::Left
            }
        }
    }
}

fn rest_min(rest: &[&Frag]) -> f64 {
    rest.iter().map(|f| f.x0).fold(f64::INFINITY, f64::min)
}

/// `(marker, rest)` when `text` opens with a bullet or an enumerator
/// followed by whitespace (or is only the marker).
fn list_marker(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start();
    let first = text.chars().next()?;
    let end = if "•◦▪▫‣⁃–—-*·●○■□►▶✓✔➢➤".contains(first) {
        first.len_utf8()
    } else {
        let token_end = text.find(char::is_whitespace).unwrap_or(text.len());
        let token = text.get(..token_end)?;
        let inner = token
            .strip_prefix('(')
            .and_then(|t| t.strip_suffix(')'))
            .or_else(|| token.strip_suffix('.'))
            .or_else(|| token.strip_suffix(')'))?;
        let enumerator =
            (!inner.is_empty() && inner.len() <= 3 && inner.chars().all(|c| c.is_ascii_digit()))
                || (inner.len() == 1 && inner.chars().all(|c| c.is_ascii_alphabetic()))
                || is_roman(&inner.to_lowercase());
        if !enumerator {
            return None;
        }
        token_end
    };
    let marker = text.get(..end)?;
    let rest = text.get(end..)?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some((marker, rest.trim_start()))
}

fn is_caption(text: &str) -> bool {
    let t = text.trim_start();
    ["Figure", "Fig.", "Table", "Chart", "Exhibit", "Plate"]
        .iter()
        .any(|lead| {
            t.strip_prefix(lead).is_some_and(|rest| {
                rest.trim_start()
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
            })
        })
}

fn assign_headings(
    pages: &mut [PageLayout],
    body: f64,
    body_bold: bool,
    diag: &mut LayoutDiagnostics,
) {
    let is_candidate = |b: &Block, page: &PageLayout| -> Option<bool> {
        if b.kind != BlockKind::Paragraph || b.lines.len() > 3 {
            return None;
        }
        let text = b.text(page);
        if text.chars().count() > 200 || !text.chars().any(char::is_alphabetic) {
            return None;
        }
        let size = f64::from(b.font_size);
        if size >= body * HEADING_SIZE_RATIO {
            Some(true)
        } else if b.bold && !body_bold && b.lines.len() <= 2 && size >= body * 0.9 {
            Some(false)
        } else {
            None
        }
    };
    let mut styles: Vec<(i64, bool)> = Vec::new();
    for page in pages.iter() {
        for b in &page.blocks {
            if is_candidate(b, page).is_some() {
                styles.push((half_points(f64::from(b.font_size)), b.bold));
            }
        }
    }
    styles.sort_by(|a, b| b.cmp(a));
    styles.dedup();
    for page in pages.iter_mut() {
        let verdicts: Vec<Option<bool>> =
            page.blocks.iter().map(|b| is_candidate(b, page)).collect();
        for (b, verdict) in page.blocks.iter_mut().zip(verdicts) {
            let Some(from_size) = verdict else { continue };
            let key = (half_points(f64::from(b.font_size)), b.bold);
            let rank = styles.iter().position(|s| *s == key).unwrap_or(0);
            #[allow(clippy::cast_possible_truncation)]
            let level = (rank + 1).min(6) as u8;
            b.kind = BlockKind::Heading { level };
            if from_size {
                diag.headings_from_size += 1;
            } else {
                diag.headings_from_weight += 1;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn list_markers() {
        assert_eq!(list_marker("• item"), Some(("•", "item")));
        assert_eq!(list_marker("3. step"), Some(("3.", "step")));
        assert_eq!(list_marker("(b) case"), Some(("(b)", "case")));
        assert_eq!(list_marker("iv) fourth"), Some(("iv)", "fourth")));
        assert_eq!(list_marker("•"), Some(("•", "")));
        assert_eq!(list_marker("2024. was"), None);
        assert_eq!(list_marker("The end."), None);
        assert_eq!(list_marker("-5 degrees"), None);
    }

    #[test]
    fn page_number_patterns() {
        for t in ["7", "Page 12", "3 of 9", "- 4 -", "xii", "Page iv", "[5]"] {
            assert!(is_page_number(t), "{t}");
        }
        for t in ["Chapter 3", "mid", "dim", "iiii", "2024 Annual Report"] {
            assert!(!is_page_number(t), "{t}");
        }
    }

    #[test]
    fn normalise_folds_digits_but_not_words() {
        assert_eq!(normalise("Report  2024 — p 17"), "report # — p #");
        assert_eq!(normalise("mild civil"), "mild civil");
    }

    #[test]
    fn captions() {
        assert!(is_caption("Figure 3: A plot"));
        assert!(is_caption("Table 12 Results"));
        assert!(!is_caption("Tables are useful"));
    }

    #[test]
    fn display_round_trip() {
        for r in [0, 90, 180, 270] {
            let (x, y) = to_display(r, 3.0, 5.0);
            assert_eq!(from_display(r, x, y), (3.0, 5.0), "{r}");
        }
        // Text running up the page reads left-to-right when displayed at 90.
        assert_eq!(to_display(90, 0.0, 1.0), (1.0, 0.0));
    }
}

//! Block layout and tables taken from a tagged PDF's own structure tree
//! (ISO 32000-1 §14.7–§14.8.4; ISO 32000-2 §14.8.4), so an export follows
//! the author's structure instead of pdfcer's inference.
//!
//! # Contract
//!
//! - **Input.** A [`StructureTree`] and one [`PageGeometry`] per
//!   `tree.text.pages` entry, as for [`layout_text`].
//! - **Blocks.** Each block-level element becomes one block per page it
//!   has text on, in logical order, with [`BlockSource::Structure`]:
//!
//!   | resolved type | [`BlockKind`] |
//!   |---|---|
//!   | `H1`–`H6`, `Hn` (2.0) | `Heading { level: n }`, capped at 6 |
//!   | `H` | `Heading`, level = enclosing `Sect` count (1–6) |
//!   | `Title` (2.0) | `Heading { level: 1 }` |
//!   | `P`, `TOCI`, `BibEntry`, `FENote` | `Paragraph` |
//!   | `LI` | `ListItem`, marker = its first `Lbl`'s text |
//!   | `Caption` | `Caption` |
//!
//!   Inline elements (`Span`, `Link`, `Lbl`, `LBody`, …) inside a block
//!   are part of it. Grouping elements (`L`, `Div`, `Sect`, `BlockQuote`,
//!   `TOC`, …) inside a block start fresh blocks for their own content.
//!   Content owned by an element with no block-level ancestor becomes a
//!   `Paragraph` of its own, counted in
//!   [`TaggedLayoutReport::non_standard_as_paragraph`] when the element's
//!   type never reached a standard name, else in
//!   [`TaggedLayoutReport::untyped_as_paragraph`].
//! - **Lines.** Lines come from [`layout_text`] over the same extraction.
//!   A line whose runs belong to more than one element is split, each
//!   part keeping the line's size and weight. Alignment, indent and column
//!   are the inferred block's that holds the element's first line. The
//!   text is the visible text; `/ActualText` is not substituted.
//! - **Tables.** A `Table` element becomes one [`TaggedTable`] per page it
//!   spans. Rows are its `TR`s, cells their `TH`/`TD`s, placed on a grid
//!   with `RowSpan`/`ColSpan` (§14.8.5.7, Table 349). Header rows are the
//!   leading rows inside a `THead` or made of `TH` only. A table nested in
//!   a cell is flattened into that cell's text and counted.
//! - **Content the tree does not own** (untagged text, artifacts) keeps
//!   its inferred block, placed after the structure block that precedes it
//!   in reading order, and is counted in
//!   [`TaggedLayoutReport::inferred_blocks_kept`].
//! - **Fallback.** Under [`StructureUse::Auto`] the result is the inferred
//!   layout when the tree is absent, claims no laid-out text, or claims
//!   less than [`TaggedLayoutOptions::min_coverage`] of it;
//!   [`TaggedLayoutReport::fallback`] says why.

use std::collections::{BTreeMap, HashMap, HashSet};

use pdfcer_model::page_tree::Rect;

use crate::block_layout::{
    Alignment, Block, BlockKind, BlockSource, DocumentLayout, LayoutDiagnostics, LayoutLine,
    LayoutOptions, PageGeometry, PageLayout, display_to_rect, layout_text, rect_to_display, union,
};
use crate::structure_tree::{StructKid, StructTreatment, StructureTree};
use crate::text_extract::TextRun;

/// Whether to take the layout from the structure tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum StructureUse {
    /// Use the tree when it covers enough of the text, else infer.
    #[default]
    Auto,
    /// Use any tree that exists, however little it covers.
    Always,
    /// Always infer; the tree is ignored.
    Never,
}

/// Tuning for [`layout_from_structure`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct TaggedLayoutOptions {
    /// When to use the tree. Default [`StructureUse::Auto`].
    pub use_structure: StructureUse,
    /// Under [`StructureUse::Auto`], the fraction of laid-out, non-artifact
    /// characters the tree must claim. Default `0.5`.
    pub min_coverage: f32,
}

impl Default for TaggedLayoutOptions {
    fn default() -> Self {
        Self {
            use_structure: StructureUse::Auto,
            min_coverage: 0.5,
        }
    }
}

impl TaggedLayoutOptions {
    /// Sets [`Self::use_structure`].
    #[must_use]
    pub const fn with_use_structure(mut self, use_structure: StructureUse) -> Self {
        self.use_structure = use_structure;
        self
    }

    /// Sets [`Self::min_coverage`].
    #[must_use]
    pub const fn with_min_coverage(mut self, min_coverage: f32) -> Self {
        self.min_coverage = min_coverage;
        self
    }
}

/// Which source the layout came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum LayoutSourceUsed {
    /// The document's structure tree.
    StructureTree,
    /// pdfcer's inference ([`layout_text`]).
    #[default]
    Inferred,
}

/// Why the structure tree was not used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FallbackReason {
    /// [`StructureUse::Never`].
    Disabled,
    /// The document has no `/StructTreeRoot`.
    NoStructureTree,
    /// The tree claims none of the laid-out text.
    NoTextClaimed,
    /// The tree claims less than [`TaggedLayoutOptions::min_coverage`].
    LowCoverage,
}

/// What [`layout_from_structure`] did, for disclosure.
///
/// The element-level counts (`non_standard_as_paragraph`,
/// `untyped_as_paragraph`, `nested_tables_flattened`,
/// `stray_table_content`, `broken_references`) describe the whole
/// document even after [`TaggedLayout::retain_pages`].
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct TaggedLayoutReport {
    /// The source used.
    pub source: LayoutSourceUsed,
    /// Why the tree was not used; `None` when it was.
    pub fallback: Option<FallbackReason>,
    /// Fraction of laid-out, non-artifact characters the tree claims.
    pub coverage: f32,
    /// Blocks taken from structure elements.
    pub structure_blocks: usize,
    /// Content under an element whose type never reached a standard name,
    /// with no block-level ancestor: elements made paragraphs.
    pub non_standard_as_paragraph: usize,
    /// Content directly under a standard grouping or inline element with
    /// no block-level ancestor: elements made paragraphs.
    pub untyped_as_paragraph: usize,
    /// Inferred blocks kept for text the tree does not own.
    pub inferred_blocks_kept: usize,
    /// Tables taken from `Table` elements (one per page spanned).
    pub tables: usize,
    /// Cells in those tables.
    pub table_cells: usize,
    /// Tables inside a cell, flattened into its text.
    pub nested_tables_flattened: usize,
    /// Elements inside a `Table` but outside any `TH`/`TD` whose content
    /// was made a paragraph.
    pub stray_table_content: usize,
    /// MCIDs the tree names that no content declares
    /// ([`StructureDiagnostics::named_not_declared`](crate::structure_tree::StructureDiagnostics::named_not_declared)).
    pub broken_references: usize,
}

/// One cell of a [`TaggedTable`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TaggedCell {
    /// The `TH`/`TD` element: an index into [`StructureTree::elements`].
    pub element: usize,
    /// First row, 0 at the top.
    pub row: usize,
    /// First column, 0 at the left.
    pub col: usize,
    /// Rows covered, at least 1.
    pub row_span: usize,
    /// Columns covered, at least 1.
    pub col_span: usize,
    /// Default user space; the grid bands' intersection for an empty cell.
    pub bbox: Rect,
    /// Indices into the page's [`PageText::runs`](crate::text_extract::PageText::runs),
    /// in reading order.
    pub runs: Vec<usize>,
    /// The cell's lines joined with `\n`.
    pub text: String,
    /// Tagged `TH`.
    pub header: bool,
}

/// One page's part of a `Table` element.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TaggedTable {
    /// The `Table` element.
    pub element: usize,
    /// As [`PageText::page_index`](crate::text_extract::PageText::page_index).
    pub page_index: usize,
    /// Default user space.
    pub bbox: Rect,
    /// Row bands, top to bottom as displayed; default user space.
    pub rows: Vec<Rect>,
    /// Column bands, left to right as displayed; default user space.
    pub columns: Vec<Rect>,
    /// Row-major by top-left corner.
    pub cells: Vec<TaggedCell>,
    /// Leading rows inside a `THead` or made of `TH` only.
    pub header_rows: usize,
}

/// A layout taken from a structure tree (or inferred, on fallback).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TaggedLayout {
    /// The blocks. Its [`DocumentLayout::diagnostics`] counts only the
    /// inferred blocks kept, plus [`LayoutDiagnostics::structure_blocks`].
    pub layout: DocumentLayout,
    /// The tables; empty on fallback.
    pub tables: Vec<TaggedTable>,
    /// What was done.
    pub report: TaggedLayoutReport,
}

impl TaggedLayout {
    /// Keeps only the pages whose [`PageLayout::page_index`] is in `keep`,
    /// and their tables, recounting the block and table counts.
    pub fn retain_pages(&mut self, keep: &[usize]) {
        self.layout.pages.retain(|p| keep.contains(&p.page_index));
        self.tables.retain(|t| keep.contains(&t.page_index));
        let body = self.layout.diagnostics.body_font_size;
        count_blocks(&self.layout.pages, body, &mut self.layout.diagnostics);
        self.layout.diagnostics.pages = self.layout.pages.len();
        self.report.structure_blocks = self.layout.diagnostics.structure_blocks;
        self.report.inferred_blocks_kept =
            self.layout.diagnostics.blocks - self.layout.diagnostics.structure_blocks;
        self.report.tables = self.tables.len();
        self.report.table_cells = self.tables.iter().map(|t| t.cells.len()).sum();
    }
}

/// Lays out `tree`'s text following its structure elements.
///
/// `geometry[i]` belongs to `tree.text.pages[i]`, as for [`layout_text`].
///
/// # Examples
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use pdfcer_model::document::Document;
/// use pdfcer_text::block_layout::{LayoutOptions, PageGeometry};
/// use pdfcer_text::structure_tree::read_structure_tree;
/// use pdfcer_text::tagged_layout::{TaggedLayoutOptions, layout_from_structure};
/// use pdfcer_text::text_extract::ExtractOptions;
///
/// let doc = Document::from_bytes(std::fs::read("tagged.pdf")?)?;
/// let tree = read_structure_tree(&doc.view(), &ExtractOptions::default())?;
/// let pages = pdfcer_model::page_tree::pages_in(&doc.view())?;
/// let geometry: Vec<PageGeometry> = tree
///     .text
///     .pages
///     .iter()
///     .filter_map(|p| pages.get(p.page_index))
///     .map(|p| PageGeometry::new(p.crop_box, p.rotate))
///     .collect();
/// let tagged = layout_from_structure(
///     &tree,
///     &geometry,
///     &LayoutOptions::default(),
///     &TaggedLayoutOptions::default(),
/// );
/// println!("{:?} {:?}", tagged.report.source, tagged.report.fallback);
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn layout_from_structure(
    tree: &StructureTree,
    geometry: &[PageGeometry],
    layout: &LayoutOptions,
    options: &TaggedLayoutOptions,
) -> TaggedLayout {
    let base = layout_text(tree.text.clone(), geometry, layout);
    let mut report = TaggedLayoutReport {
        broken_references: tree.diagnostics.named_not_declared,
        ..TaggedLayoutReport::default()
    };
    let inferred = |base: DocumentLayout, mut report: TaggedLayoutReport, why| {
        report.fallback = Some(why);
        report.inferred_blocks_kept = base.diagnostics.blocks;
        TaggedLayout {
            layout: base,
            tables: Vec::new(),
            report,
        }
    };
    if options.use_structure == StructureUse::Never {
        return inferred(base, report, FallbackReason::Disabled);
    }
    if !tree.diagnostics.struct_tree_present {
        return inferred(base, report, FallbackReason::NoStructureTree);
    }

    let walk = Walk::run(tree);
    let (claimed, total) = coverage(&base, &walk);
    #[allow(clippy::cast_precision_loss)]
    let fraction = if total == 0 {
        0.0
    } else {
        claimed as f32 / total as f32
    };
    report.coverage = fraction;
    if claimed == 0 {
        return inferred(base, report, FallbackReason::NoTextClaimed);
    }
    if options.use_structure == StructureUse::Auto && fraction < options.min_coverage {
        return inferred(base, report, FallbackReason::LowCoverage);
    }

    report.source = LayoutSourceUsed::StructureTree;
    report.non_standard_as_paragraph = walk.non_standard_as_paragraph;
    report.untyped_as_paragraph = walk.untyped_as_paragraph;
    report.nested_tables_flattened = walk.nested_tables;
    report.stray_table_content = walk.stray_table_content;

    let fallback_geo = PageGeometry::new(Rect::from_corners(0.0, 0.0, 612.0, 792.0), 0);
    let mut pages = Vec::with_capacity(base.pages.len());
    let mut cell_lines: HashMap<usize, Vec<CellPart>> = HashMap::new();
    for (pi, page) in base.pages.iter().enumerate() {
        let runs = tree
            .text
            .pages
            .get(pi)
            .map_or(&[][..], |p| p.runs.as_slice());
        pages.push(build_page(page, runs, &walk, &mut cell_lines));
    }
    let mut tables = Vec::new();
    for &t in &walk.tables {
        tables.extend(build_tables(
            t,
            &walk,
            tree,
            &cell_lines,
            &base.pages,
            geometry,
            fallback_geo,
        ));
    }

    let mut diagnostics = base.diagnostics;
    count_blocks(&pages, diagnostics.body_font_size, &mut diagnostics);
    report.structure_blocks = diagnostics.structure_blocks;
    report.inferred_blocks_kept = diagnostics.blocks - diagnostics.structure_blocks;
    report.tables = tables.len();
    report.table_cells = tables.iter().map(|t| t.cells.len()).sum();
    TaggedLayout {
        layout: DocumentLayout {
            text: base.text,
            pages,
            diagnostics,
        },
        tables,
        report,
    }
}

// ---------------------------------------------------------------------------
// The walk: which sink each run belongs to
// ---------------------------------------------------------------------------

/// Where an element's content goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ctx {
    Free,
    Skip,
    Block(usize),
    Table(usize),
    Cell(usize),
}

/// A content destination: a block element or a cell element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Sink {
    Block(usize),
    Cell(usize),
}

struct Walk {
    /// Block element -> its kind.
    kinds: HashMap<usize, BlockKind>,
    /// Sinks in first-content order.
    order: Vec<Sink>,
    /// `(page_index, run)` -> sink; first claim wins.
    owner: HashMap<(usize, usize), Sink>,
    /// Table elements, in logical order.
    tables: Vec<usize>,
    /// Table -> rows; each row is `(header, cells)`.
    rows: HashMap<usize, Vec<(bool, Vec<usize>)>>,
    non_standard_as_paragraph: usize,
    untyped_as_paragraph: usize,
    nested_tables: usize,
    stray_table_content: usize,
}

fn block_kind(t: &str, sect_depth: usize) -> Option<BlockKind> {
    let heading = |n: usize| {
        #[allow(clippy::cast_possible_truncation)]
        let level = n.clamp(1, 6) as u8;
        BlockKind::Heading { level }
    };
    match t {
        "P" | "TOCI" | "BibEntry" | "FENote" => Some(BlockKind::Paragraph),
        "H" => Some(heading(sect_depth)),
        "Title" => Some(heading(1)),
        "LI" => Some(BlockKind::ListItem {
            marker: String::new(),
        }),
        "Caption" => Some(BlockKind::Caption),
        _ => t
            .strip_prefix('H')
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|&n| n >= 1)
            .map(heading),
    }
}

/// Whether a block element nested inside a block of type `outer` starts
/// its own block. Producers nest body `P`s inside heading `P`s; merging
/// them would make a whole section one paragraph. A list item, caption or
/// TOC entry keeps its nested paragraphs, which are its body.
fn splits_nested(outer: &str) -> bool {
    matches!(outer, "P" | "H" | "Title")
        || outer
            .strip_prefix('H')
            .is_some_and(|n| n.parse::<usize>().is_ok())
}

/// Grouping types that start fresh blocks even inside a block.
fn is_grouping(t: &str) -> bool {
    matches!(
        t,
        "L" | "Div"
            | "Sect"
            | "Part"
            | "Art"
            | "BlockQuote"
            | "TOC"
            | "Index"
            | "Aside"
            | "Document"
            | "DocumentFragment"
    )
}

impl Walk {
    fn run(tree: &StructureTree) -> Self {
        let n = tree.elements.len();
        let mut ctx = vec![Ctx::Free; n];
        let mut sect = vec![0usize; n];
        let mut thead = vec![false; n];
        let mut row_of: Vec<Option<usize>> = vec![None; n];
        let mut w = Self {
            kinds: HashMap::new(),
            order: Vec::new(),
            owner: HashMap::new(),
            tables: Vec::new(),
            rows: HashMap::new(),
            non_standard_as_paragraph: 0,
            untyped_as_paragraph: 0,
            nested_tables: 0,
            stray_table_content: 0,
        };
        // Elements are in pre-order, so a parent with a smaller index is
        // already decided.
        for (i, e) in tree.elements.iter().enumerate() {
            let parent = e.parent.filter(|&p| p < i);
            let at = |v: &[Ctx]| parent.and_then(|p| v.get(p).copied()).unwrap_or(Ctx::Free);
            let pc = at(&ctx);
            let t = e.resolved_type.as_str();
            let psect = parent.and_then(|p| sect.get(p).copied()).unwrap_or(0);
            let depth = psect + usize::from(t == "Sect");
            let pthead = parent.and_then(|p| thead.get(p).copied()).unwrap_or(false);
            let prow = parent.and_then(|p| row_of.get(p).copied()).flatten();
            let mut in_thead = pthead || t == "THead";
            let mut row = prow;
            let c = if matches!(
                e.treatment,
                StructTreatment::Artifact | StructTreatment::Private
            ) {
                Ctx::Skip
            } else {
                match pc {
                    Ctx::Skip => Ctx::Skip,
                    Ctx::Cell(c) => {
                        if t == "Table" {
                            w.nested_tables += 1;
                        }
                        Ctx::Cell(c)
                    }
                    Ctx::Table(tb) => match t {
                        "TR" => {
                            w.rows.entry(tb).or_default().push((in_thead, Vec::new()));
                            row = Some(i);
                            Ctx::Table(tb)
                        }
                        "TH" | "TD" => {
                            let rows = w.rows.entry(tb).or_default();
                            if prow.is_none() || rows.is_empty() {
                                rows.push((in_thead, Vec::new()));
                            }
                            if let Some((_, cells)) = rows.last_mut() {
                                cells.push(i);
                            }
                            Ctx::Cell(i)
                        }
                        "Caption" => {
                            w.kinds.insert(i, BlockKind::Caption);
                            Ctx::Block(i)
                        }
                        "Table" => {
                            w.tables.push(i);
                            in_thead = false;
                            row = None;
                            Ctx::Table(i)
                        }
                        _ => Ctx::Table(tb),
                    },
                    Ctx::Block(b)
                        if !is_grouping(t)
                            && t != "Table"
                            && !(block_kind(t, depth).is_some()
                                && tree
                                    .elements
                                    .get(b)
                                    .is_some_and(|o| splits_nested(&o.resolved_type))) =>
                    {
                        Ctx::Block(b)
                    }
                    Ctx::Free | Ctx::Block(_) => {
                        if t == "Table" {
                            w.tables.push(i);
                            in_thead = false;
                            row = None;
                            Ctx::Table(i)
                        } else if let Some(k) = block_kind(t, depth) {
                            w.kinds.insert(i, k);
                            Ctx::Block(i)
                        } else {
                            Ctx::Free
                        }
                    }
                }
            };
            if let Some(slot) = ctx.get_mut(i) {
                *slot = c;
            }
            if let Some(slot) = sect.get_mut(i) {
                *slot = depth;
            }
            if let Some(slot) = thead.get_mut(i) {
                *slot = in_thead;
            }
            if let Some(slot) = row_of.get_mut(i) {
                *slot = row;
            }
            // A list item's marker: its first label.
            if t == "Lbl"
                && let Ctx::Block(b) = c
                && let Some(BlockKind::ListItem { marker }) = w.kinds.get_mut(&b)
                && marker.is_empty()
            {
                tree.element_text(i).trim().clone_into(marker);
            }
        }
        // A row whose cells are all TH is a header row too.
        for rows in w.rows.values_mut() {
            for (header, cells) in rows.iter_mut() {
                if !cells.is_empty()
                    && cells.iter().all(|&c| {
                        tree.elements
                            .get(c)
                            .is_some_and(|e| e.resolved_type == "TH")
                    })
                {
                    *header = true;
                }
            }
        }

        // Content in logical order: kids in order, depth first.
        let mut implicit: HashSet<usize> = HashSet::new();
        let mut seen: HashSet<Sink> = HashSet::new();
        let mut stack: Vec<(usize, usize)> = tree.roots.iter().rev().map(|&r| (r, 0)).collect();
        while let Some((i, k)) = stack.pop() {
            let Some(e) = tree.elements.get(i) else {
                continue;
            };
            let Some(kid) = e.kids.get(k) else {
                continue;
            };
            stack.push((i, k + 1));
            match kid {
                StructKid::Element(c) if *c > i => stack.push((*c, 0)),
                StructKid::MarkedContent {
                    page_index: Some(p),
                    runs,
                    ..
                } if !runs.is_empty() => {
                    let sink = match ctx.get(i).copied().unwrap_or(Ctx::Skip) {
                        Ctx::Skip => continue,
                        Ctx::Block(b) => Sink::Block(b),
                        Ctx::Cell(c) => Sink::Cell(c),
                        Ctx::Free | Ctx::Table(_) => {
                            if implicit.insert(i) {
                                if matches!(ctx.get(i), Some(Ctx::Table(_))) {
                                    w.stray_table_content += 1;
                                } else if e.standard {
                                    w.untyped_as_paragraph += 1;
                                } else {
                                    w.non_standard_as_paragraph += 1;
                                }
                                w.kinds.insert(i, BlockKind::Paragraph);
                            }
                            Sink::Block(i)
                        }
                    };
                    let mut any = false;
                    for &r in runs {
                        if let std::collections::hash_map::Entry::Vacant(v) = w.owner.entry((*p, r))
                        {
                            v.insert(sink);
                            any = true;
                        }
                    }
                    if any && seen.insert(sink) {
                        w.order.push(sink);
                    }
                }
                _ => {}
            }
        }
        w
    }
}

/// `(claimed, total)` non-artifact characters on laid-out lines.
fn coverage(base: &DocumentLayout, walk: &Walk) -> (usize, usize) {
    let (mut claimed, mut total) = (0, 0);
    for (pi, page) in base.pages.iter().enumerate() {
        let Some(text) = base.text.pages.get(pi) else {
            continue;
        };
        for line in &page.lines {
            for &r in &line.runs {
                let Some(run) = text.runs.get(r) else {
                    continue;
                };
                if run.artifact.is_some() {
                    continue;
                }
                let n = run.text.chars().filter(|c| !c.is_whitespace()).count();
                total += n;
                if walk.owner.contains_key(&(page.page_index, r)) {
                    claimed += n;
                }
            }
        }
    }
    (claimed, total)
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

/// One line of a cell: its text and box.
struct CellPart {
    page_index: usize,
    line: usize,
    text: String,
    bbox: Rect,
    runs: Vec<usize>,
}

/// `line` restricted to `keep` (a subset of its runs), or the line itself.
fn sub_line(line: &LayoutLine, keep: &[usize], runs: &[TextRun]) -> LayoutLine {
    if keep.len() == line.runs.len() {
        return line.clone();
    }
    let mut text = String::new();
    let mut prev: Option<f64> = None;
    let mut bbox: Option<Rect> = None;
    let size = f64::from(line.font_size);
    for &r in keep {
        let Some(run) = runs.get(r) else {
            continue;
        };
        let b = run.bbox;
        if let (Some(x1), Some(b)) = (prev, b)
            && b.llx - x1 > 0.15 * size
            && !text.ends_with(char::is_whitespace)
            && !run.text.starts_with(char::is_whitespace)
        {
            text.push(' ');
        }
        text.push_str(&run.text);
        if let Some(b) = b {
            prev = Some(b.urx);
            bbox = Some(bbox.map_or(b, |u| union(&u, &b)));
        }
    }
    LayoutLine {
        runs: keep.to_vec(),
        text: text.trim().to_owned(),
        bbox: bbox.unwrap_or(line.bbox),
        font_size: line.font_size,
        bold: line.bold,
    }
}

fn build_page(
    page: &PageLayout,
    runs: &[TextRun],
    walk: &Walk,
    cell_lines: &mut HashMap<usize, Vec<CellPart>>,
) -> PageLayout {
    let pidx = page.page_index;
    let owner = |r: usize| walk.owner.get(&(pidx, r)).copied();
    // Base line -> base block.
    let mut block_of_line: HashMap<usize, usize> = HashMap::new();
    for (bi, b) in page.blocks.iter().enumerate() {
        for &l in &b.lines {
            block_of_line.insert(l, bi);
        }
    }

    // Each sink's runs on this page, grouped by base line.
    let mut by_sink: HashMap<Sink, BTreeMap<usize, Vec<usize>>> = HashMap::new();
    for (li, line) in page.lines.iter().enumerate() {
        for &r in &line.runs {
            if let Some(s) = owner(r) {
                by_sink.entry(s).or_default().entry(li).or_default().push(r);
            }
        }
    }

    let mut lines: Vec<LayoutLine> = Vec::new();
    let mut tagged: Vec<(usize, Block)> = Vec::new();
    for sink in &walk.order {
        let Some(groups) = by_sink.get(sink) else {
            continue;
        };
        match *sink {
            Sink::Cell(c) => {
                let parts = cell_lines.entry(c).or_default();
                for (&li, keep) in groups {
                    let Some(line) = page.lines.get(li) else {
                        continue;
                    };
                    let l = sub_line(line, keep, runs);
                    parts.push(CellPart {
                        page_index: pidx,
                        line: li,
                        text: l.text,
                        bbox: l.bbox,
                        runs: l.runs,
                    });
                }
            }
            Sink::Block(e) => {
                let first = lines.len();
                let mut first_base = None;
                for (&li, keep) in groups {
                    let Some(line) = page.lines.get(li) else {
                        continue;
                    };
                    first_base.get_or_insert(li);
                    lines.push(sub_line(line, keep, runs));
                }
                let members: Vec<usize> = (first..lines.len()).collect();
                let kind = walk.kinds.get(&e).cloned().unwrap_or(BlockKind::Paragraph);
                let like = first_base
                    .and_then(|l| block_of_line.get(&l))
                    .and_then(|&b| page.blocks.get(b));
                if let Some(block) = make_block(kind, BlockSource::Structure, members, &lines, like)
                {
                    tagged.push((e, block));
                }
            }
        }
    }

    // Text the tree does not own keeps its inferred block, after the
    // structure block that precedes it in reading order.
    let mut front: Vec<Block> = Vec::new();
    let mut after: HashMap<usize, Vec<Block>> = HashMap::new();
    let mut last: Option<usize> = None;
    for b in &page.blocks {
        let mut kept = Vec::new();
        let mut anchor = last;
        for &li in &b.lines {
            let Some(line) = page.lines.get(li) else {
                continue;
            };
            let free: Vec<usize> = line
                .runs
                .iter()
                .copied()
                .filter(|&r| owner(r).is_none())
                .collect();
            if let Some(Sink::Block(e)) = line.runs.iter().find_map(|&r| owner(r)) {
                last = Some(e);
            }
            if free.is_empty() {
                continue;
            }
            if kept.is_empty() {
                anchor = last;
            }
            kept.push(lines.len());
            lines.push(sub_line(line, &free, runs));
        }
        if let Some(block) = make_block(b.kind.clone(), b.source, kept, &lines, Some(b)) {
            match anchor {
                Some(e) => after.entry(e).or_default().push(block),
                None => front.push(block),
            }
        }
    }

    let mut blocks = front;
    for (e, block) in tagged {
        blocks.push(block);
        if let Some(rest) = after.remove(&e) {
            blocks.extend(rest);
        }
    }
    // Anchors whose structure block did not reach this page.
    let mut rest: Vec<(usize, Vec<Block>)> = after.into_iter().collect();
    rest.sort_by_key(|(e, _)| *e);
    blocks.extend(rest.into_iter().flat_map(|(_, b)| b));

    PageLayout {
        page_index: pidx,
        columns: page.columns.clone(),
        lines,
        blocks,
    }
}

/// A block over `members` (indices into `lines`), borrowing alignment,
/// indent and column from `like`.
fn make_block(
    kind: BlockKind,
    source: BlockSource,
    members: Vec<usize>,
    lines: &[LayoutLine],
    like: Option<&Block>,
) -> Option<Block> {
    let ls: Vec<&LayoutLine> = members.iter().filter_map(|&i| lines.get(i)).collect();
    let first = ls.first()?;
    let bbox = ls
        .iter()
        .skip(1)
        .fold(first.bbox, |a, l| union(&a, &l.bbox));
    let mut sizes: HashMap<u32, usize> = HashMap::new();
    let (mut chars, mut bold) = (0usize, 0usize);
    for l in &ls {
        let n = l.text.chars().count().max(1);
        *sizes.entry(l.font_size.to_bits()).or_default() += n;
        chars += n;
        if l.bold {
            bold += n;
        }
    }
    let font_size = sizes
        .iter()
        .max_by_key(|&(s, n)| (*n, *s))
        .map_or(first.font_size, |(s, _)| f32::from_bits(*s));
    Some(Block {
        kind,
        source,
        lines: members,
        bbox,
        column: like.and_then(|b| b.column),
        alignment: like.map_or(Alignment::Left, |b| b.alignment),
        first_line_indent: if ls.len() > 1 {
            like.map_or(0.0, |b| b.first_line_indent)
        } else {
            0.0
        },
        font_size,
        bold: bold * 2 > chars,
    })
}

/// Recounts block decisions after the blocks changed.
pub(crate) fn count_blocks(pages: &[PageLayout], body: Option<f32>, d: &mut LayoutDiagnostics) {
    d.blocks = 0;
    d.structure_blocks = 0;
    d.paragraphs = 0;
    d.headings_from_size = 0;
    d.headings_from_weight = 0;
    d.list_items = 0;
    d.captions = 0;
    d.running_headers = 0;
    d.running_footers = 0;
    d.page_numbers = 0;
    d.tagged_artifact_blocks = 0;
    let body = body.unwrap_or(10.0);
    for b in pages.iter().flat_map(|p| &p.blocks) {
        d.blocks += 1;
        match b.source {
            BlockSource::Structure => {
                d.structure_blocks += 1;
                continue;
            }
            BlockSource::Tagged => {
                d.tagged_artifact_blocks += 1;
                continue;
            }
            BlockSource::Inferred => {}
        }
        match b.kind {
            BlockKind::Paragraph => d.paragraphs += 1,
            BlockKind::Heading { .. } if b.font_size >= body * 1.15 => d.headings_from_size += 1,
            BlockKind::Heading { .. } => d.headings_from_weight += 1,
            BlockKind::ListItem { .. } => d.list_items += 1,
            BlockKind::Caption => d.captions += 1,
            BlockKind::RunningHeader => d.running_headers += 1,
            BlockKind::RunningFooter => d.running_footers += 1,
            BlockKind::PageNumber => d.page_numbers += 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

/// Spans past this are clamped: a `ColSpan` of a million must not cost a
/// million grid slots.
const MAX_SPAN: usize = 1024;
/// Grid slots marked per table before further spans are taken as 1.
const MAX_SLOTS: usize = 1 << 22;

struct Placed {
    element: usize,
    row: usize,
    col: usize,
    row_span: usize,
    col_span: usize,
    header: bool,
}

fn build_tables(
    table: usize,
    walk: &Walk,
    tree: &StructureTree,
    cell_lines: &HashMap<usize, Vec<CellPart>>,
    pages: &[PageLayout],
    geometry: &[PageGeometry],
    fallback: PageGeometry,
) -> Vec<TaggedTable> {
    let Some(rows) = walk.rows.get(&table) else {
        return Vec::new();
    };
    // The grid (HTML-style placement).
    let n_rows = rows.len();
    let mut occupied: HashSet<(usize, usize)> = HashSet::new();
    let mut placed: Vec<Placed> = Vec::new();
    for (r, (header, cells)) in rows.iter().enumerate() {
        let mut col = 0;
        for &c in cells {
            while occupied.contains(&(r, col)) {
                col += 1;
            }
            let e = tree.elements.get(c);
            let span = |v: Option<u32>| {
                usize::try_from(v.unwrap_or(1))
                    .unwrap_or(1)
                    .clamp(1, MAX_SPAN)
            };
            let mut rs = span(e.and_then(|e| e.row_span)).min(n_rows - r);
            let mut cs = span(e.and_then(|e| e.col_span));
            if occupied.len() + rs * cs > MAX_SLOTS {
                rs = 1;
                cs = 1;
            }
            for dr in 0..rs {
                for dc in 0..cs {
                    occupied.insert((r + dr, col + dc));
                }
            }
            placed.push(Placed {
                element: c,
                row: r,
                col,
                row_span: rs,
                col_span: cs,
                header: *header || e.is_some_and(|e| e.resolved_type == "TH"),
            });
            col += cs;
        }
    }

    // Each row's page: its cells' first line's page, else the row above's.
    let mut row_page: Vec<Option<usize>> = vec![None; n_rows];
    for p in &placed {
        if let Some(part) = cell_lines.get(&p.element).and_then(|v| v.first())
            && let Some(slot) = row_page.get_mut(p.row)
            && slot.is_none()
        {
            *slot = Some(part.page_index);
        }
    }
    let mut prev = row_page.iter().flatten().next().copied();
    for slot in &mut row_page {
        match slot {
            Some(p) => prev = Some(*p),
            None => *slot = prev,
        }
    }

    // Split into one table per contiguous run of rows on one page.
    let mut out = Vec::new();
    let mut start = 0;
    while start < n_rows {
        let page = row_page.get(start).copied().flatten();
        let mut end = start + 1;
        while end < n_rows && row_page.get(end).copied().flatten() == page {
            end += 1;
        }
        if let Some(page_index) = page
            && let Some(t) = segment(
                table, &placed, rows, start, end, page_index, cell_lines, pages, geometry, fallback,
            )
        {
            out.push(t);
        }
        start = end;
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn segment(
    table: usize,
    placed: &[Placed],
    rows: &[(bool, Vec<usize>)],
    start: usize,
    end: usize,
    page_index: usize,
    cell_lines: &HashMap<usize, Vec<CellPart>>,
    pages: &[PageLayout],
    geometry: &[PageGeometry],
    fallback: PageGeometry,
) -> Option<TaggedTable> {
    let rotate = pages
        .iter()
        .position(|p| p.page_index == page_index)
        .and_then(|i| geometry.get(i))
        .unwrap_or(&fallback)
        .rotate;
    let n = end - start;
    let cells: Vec<&Placed> = placed
        .iter()
        .filter(|p| p.row >= start && p.row < end)
        .collect();
    let n_cols = cells.iter().map(|p| p.col + p.col_span).max().unwrap_or(0);
    if n_cols == 0 {
        return None;
    }

    // Each cell's lines on this page, in reading order, in display space.
    let content = |e: usize| -> Vec<&CellPart> {
        let mut v: Vec<&CellPart> = cell_lines
            .get(&e)
            .map(|v| v.iter().filter(|p| p.page_index == page_index).collect())
            .unwrap_or_default();
        v.sort_by_key(|p| p.line);
        v
    };
    let disp = |r: &Rect| rect_to_display(rotate, r);
    let mut ext: Option<(f64, f64, f64, f64)> = None;
    let mut row_top = vec![None::<f64>; n];
    let mut row_bottom = vec![None::<f64>; n];
    let mut col_left = vec![None::<f64>; n_cols];
    let mut col_right = vec![None::<f64>; n_cols];
    let mut boxes: Vec<Option<(f64, f64, f64, f64)>> = Vec::with_capacity(cells.len());
    for p in &cells {
        let parts = content(p.element);
        let b = parts
            .iter()
            .map(|c| disp(&c.bbox))
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1), a.2.min(b.2), a.3.max(b.3)));
        if let Some((x0, x1, y0, y1)) = b {
            ext = Some(ext.map_or((x0, x1, y0, y1), |e| {
                (e.0.min(x0), e.1.max(x1), e.2.min(y0), e.3.max(y1))
            }));
            let r = p.row - start;
            if p.row_span == 1 {
                if let Some(t) = row_top.get_mut(r) {
                    *t = Some(t.map_or(y1, |v: f64| v.max(y1)));
                }
                if let Some(t) = row_bottom.get_mut(r) {
                    *t = Some(t.map_or(y0, |v: f64| v.min(y0)));
                }
            }
            if p.col_span == 1 {
                if let Some(t) = col_left.get_mut(p.col) {
                    *t = Some(t.map_or(x0, |v: f64| v.min(x0)));
                }
                if let Some(t) = col_right.get_mut(p.col) {
                    *t = Some(t.map_or(x1, |v: f64| v.max(x1)));
                }
            }
        }
        boxes.push(b);
    }
    let (x0, x1, y0, y1) = ext?;
    // Band edges: top-to-bottom for rows, left-to-right for columns.
    let row_edges = edges(y1, y0, &row_top, &row_bottom, true);
    let col_edges = edges(x0, x1, &col_left, &col_right, false);
    let edge = |v: &[f64], i: usize| v.get(i).copied().unwrap_or(0.0);

    let rows_out: Vec<Rect> = (0..n)
        .map(|r| display_to_rect(rotate, x0, x1, edge(&row_edges, r + 1), edge(&row_edges, r)))
        .collect();
    let cols_out: Vec<Rect> = (0..n_cols)
        .map(|c| display_to_rect(rotate, edge(&col_edges, c), edge(&col_edges, c + 1), y0, y1))
        .collect();

    let mut out_cells = Vec::with_capacity(cells.len());
    for (p, b) in cells.iter().zip(boxes) {
        let r = p.row - start;
        let rs = p.row_span.min(end - p.row);
        let bbox = b.map_or_else(
            || {
                display_to_rect(
                    rotate,
                    edge(&col_edges, p.col),
                    edge(&col_edges, p.col + p.col_span),
                    edge(&row_edges, r + rs),
                    edge(&row_edges, r),
                )
            },
            |(a, b2, c, d)| display_to_rect(rotate, a, b2, c, d),
        );
        let parts = content(p.element);
        out_cells.push(TaggedCell {
            element: p.element,
            row: r,
            col: p.col,
            row_span: rs,
            col_span: p.col_span,
            bbox,
            runs: parts.iter().flat_map(|c| c.runs.iter().copied()).collect(),
            text: parts
                .iter()
                .map(|c| c.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            header: p.header,
        });
    }
    let header_rows = rows
        .get(start..end)
        .map_or(0, |rs| rs.iter().take_while(|(h, _)| *h).count());
    Some(TaggedTable {
        element: table,
        page_index,
        bbox: display_to_rect(rotate, x0, x1, y0, y1),
        rows: rows_out,
        columns: cols_out,
        cells: out_cells,
        header_rows,
    })
}

/// `n + 1` band edges from `first` to `last`. `lead[i]`/`trail[i]` are band
/// `i`'s measured leading and trailing edge (top/bottom for rows,
/// left/right for columns). A shared edge is the midpoint of the two
/// measured sides; an unmeasured edge is interpolated between its known
/// neighbours.
fn edges(
    first: f64,
    last: f64,
    lead: &[Option<f64>],
    trail: &[Option<f64>],
    descending: bool,
) -> Vec<f64> {
    let n = lead.len();
    let mut e: Vec<Option<f64>> = vec![None; n + 1];
    if let Some(v) = e.first_mut() {
        *v = Some(first);
    }
    if let Some(v) = e.last_mut() {
        *v = Some(last);
    }
    for k in 1..n {
        let before = trail.get(k - 1).copied().flatten();
        let after = lead.get(k).copied().flatten();
        let v = match (before, after) {
            (Some(a), Some(b)) => Some(f64::midpoint(a, b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        };
        if let Some(slot) = e.get_mut(k) {
            *slot = v;
        }
    }
    let known: Vec<(usize, f64)> = e
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|v| (i, v)))
        .collect();
    let mut out: Vec<f64> = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let v = e.get(i).copied().flatten().unwrap_or_else(|| {
            let lo = known.iter().rev().find(|(j, _)| *j < i).copied();
            let hi = known.iter().find(|(j, _)| *j > i).copied();
            match (lo, hi) {
                (Some((a, va)), Some((b, vb))) => {
                    #[allow(clippy::cast_precision_loss)]
                    let f = (i - a) as f64 / (b - a) as f64;
                    f.mul_add(vb - va, va)
                }
                _ => first,
            }
        });
        // Keep the edges monotonic.
        let v = match out.last() {
            Some(&p) if descending => v.min(p),
            Some(&p) => v.max(p),
            None => v,
        };
        out.push(v);
    }
    out
}

//! Lines and blocks over an [`OcrPage`]'s words: what the engine reported,
//! or what [`crate::block_layout`] infers from the word boxes when it
//! reported none.

use crate::block_layout::{self, BlockKind, LayoutOptions, PageGeometry};
use crate::page_tree::Rect;
use crate::text_extract::{ExtractedText, PageText, TextOrigin, TextRun};

use super::OcrPage;

/// One recognised line: indices into [`OcrPage::words`], left to right.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct OcrLine {
    /// Word indices, in reading order within the line.
    pub words: Vec<usize>,
}

impl OcrLine {
    /// A line over `words` (indices into [`OcrPage::words`]).
    #[must_use]
    pub const fn new(words: Vec<usize>) -> Self {
        Self { words }
    }
}

/// What a recognised block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum OcrBlockKind {
    /// Body text.
    #[default]
    Paragraph,
    /// A heading.
    Heading,
    /// A list item.
    ListItem,
    /// One cell of a table.
    TableCell,
    /// A figure or table caption.
    Caption,
    /// Anything else (a header, footer, page number, a stray mark).
    Other,
}

/// One recognised block: indices into [`OcrPage::lines`], top to bottom.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct OcrBlock {
    /// What the block is.
    pub kind: OcrBlockKind,
    /// Line indices, in reading order within the block.
    pub lines: Vec<usize>,
}

impl OcrBlock {
    /// A block of `kind` over `lines` (indices into [`OcrPage::lines`]).
    #[must_use]
    pub const fn new(kind: OcrBlockKind, lines: Vec<usize>) -> Self {
        Self { kind, lines }
    }
}

/// Where a written layer's lines and blocks came from
/// ([`super::layer::OcrLayerReport::structure`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum OcrStructureSource {
    /// The engine reported lines and blocks.
    Reported,
    /// The engine reported lines; pdfcer grouped them into blocks.
    BlocksInferred,
    /// The engine reported only words; pdfcer inferred lines and blocks.
    #[default]
    Inferred,
}

/// A block resolved to word indices: `lines[i]` lists word indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedBlock {
    pub(super) kind: OcrBlockKind,
    pub(super) lines: Vec<Vec<usize>>,
}

/// The page's blocks in reading order, every word in exactly one line.
///
/// Reported indices out of range, or naming a word or line a second time,
/// are ignored; words no line names are appended as a final [`OcrBlockKind::Other`]
/// block, one line each, in engine order, so the layer never loses a word.
pub(super) fn resolve(page: &OcrPage) -> (Vec<ResolvedBlock>, OcrStructureSource) {
    let n = page.words.len();
    let (blocks, source) = if !page.blocks.is_empty() && !page.lines.is_empty() {
        (reported(page), OcrStructureSource::Reported)
    } else if !page.lines.is_empty() {
        (blocks_over_lines(page), OcrStructureSource::BlocksInferred)
    } else {
        let ink: Vec<Rect> = page.words.iter().map(|w| w.rect).collect();
        let rects = font_boxes(&ink, &rows_of(&ink));
        let boxes: Vec<(Rect, &str)> = rects
            .iter()
            .zip(&page.words)
            .map(|(r, w)| (*r, w.text.as_str()))
            .collect();
        let singles: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
        (infer(&boxes, &singles), OcrStructureSource::Inferred)
    };
    (cover_every_word(blocks, n), source)
}

fn reported(page: &OcrPage) -> Vec<ResolvedBlock> {
    page.blocks
        .iter()
        .map(|b| ResolvedBlock {
            kind: b.kind,
            lines: b
                .lines
                .iter()
                .filter_map(|&l| page.lines.get(l))
                .map(|l| l.words.clone())
                .collect(),
        })
        .collect()
}

/// Lays out the reported lines as boxes and keeps each line whole.
fn blocks_over_lines(page: &OcrPage) -> Vec<ResolvedBlock> {
    let ink: Vec<Rect> = page.words.iter().map(|w| w.rect).collect();
    let lines: Vec<Vec<usize>> = page
        .lines
        .iter()
        .map(|l| l.words.iter().copied().filter(|&w| w < ink.len()).collect())
        .collect();
    let rects = font_boxes(&ink, &lines);
    let mut boxes = Vec::new();
    let mut members = Vec::new();
    for words in lines {
        let Some(rect) = union_of(&rects, &words) else {
            continue;
        };
        let text: Vec<&str> = words
            .iter()
            .filter_map(|&w| page.words.get(w))
            .map(|w| w.text.as_str())
            .collect();
        boxes.push((rect, text.join(" ")));
        members.push(words);
    }
    let refs: Vec<(Rect, &str)> = boxes.iter().map(|(r, t)| (*r, t.as_str())).collect();
    infer(&refs, &members)
}

fn union_of(rects: &[Rect], words: &[usize]) -> Option<Rect> {
    words
        .iter()
        .filter_map(|&w| rects.get(w))
        .copied()
        .reduce(union)
}

/// Ascender top to descender bottom of a common text face, in ems
/// (Helvetica: 0.718 + 0.207). A recogniser's word box is tight on the ink.
const INK_EM: f64 = 0.93;

/// A row whose ink height is within this factor of the page's median row
/// is set at the body size; outside it, at its own (a heading, a footnote).
const OWN_SIZE: (f64, f64) = (0.7, 1.2);

/// Words in rows: a box joins the current row when its vertical
/// centre lies inside the row's span so far. Rows top to bottom.
fn rows_of(ink: &[Rect]) -> Vec<Vec<usize>> {
    let centre = |r: &Rect| (r.lly + r.ury) / 2.0;
    let mut order: Vec<usize> = (0..ink.len()).collect();
    order.sort_by(|&a, &b| {
        let (ca, cb) = (ink.get(a).map(centre), ink.get(b).map(centre));
        cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut rows: Vec<(f64, f64, Vec<usize>)> = Vec::new();
    for i in order {
        let Some(r) = ink.get(i) else { continue };
        match rows.last_mut() {
            Some((lo, hi, row)) if (*lo..=*hi).contains(&centre(r)) => {
                *lo = lo.min(r.lly);
                *hi = hi.max(r.ury);
                row.push(i);
            }
            _ => rows.push((r.lly, r.ury, vec![i])),
        }
    }
    rows.into_iter().map(|(_, _, row)| row).collect()
}

/// Rewrites tight ink boxes as the font boxes [`block_layout`] sizes a
/// glyph-free run from (em 0.8 of the box height, baseline 0.2 up it), so
/// its line-spacing and size-change rules compare ems, not ink.
///
/// Each row gets one em, from its ink height over [`INK_EM`] (the page's
/// median row height unless outside [`OWN_SIZE`] of it), and one baseline:
/// the upper quartile of its words' bottoms, since words without a
/// descender sit on it. A row of one box is taken to carry a descender.
/// Boxes no row names are returned unchanged.
fn font_boxes(ink: &[Rect], rows: &[Vec<usize>]) -> Vec<Rect> {
    let height = |row: &Vec<usize>| {
        let rs = || row.iter().filter_map(|&i| ink.get(i));
        let top = rs().map(|r| r.ury).fold(f64::NEG_INFINITY, f64::max);
        let bottom = rs().map(|r| r.lly).fold(f64::INFINITY, f64::min);
        top - bottom
    };
    let mut heights: Vec<f64> = rows
        .iter()
        .map(height)
        .filter(|h| h.is_finite() && *h > 0.0)
        .collect();
    heights.sort_by(f64::total_cmp);
    let body = heights.get(heights.len() / 2).copied();
    let mut out = ink.to_vec();
    for row in rows {
        let h = height(row);
        if !(h.is_finite() && h > 0.0) {
            continue;
        }
        let h = match body {
            Some(b) if h >= OWN_SIZE.0 * b && h <= OWN_SIZE.1 * b => b,
            _ => h,
        };
        let em = h / INK_EM;
        let mut bottoms: Vec<f64> = row
            .iter()
            .filter_map(|&i| ink.get(i))
            .map(|r| r.lly)
            .collect();
        bottoms.sort_by(f64::total_cmp);
        let baseline = match bottoms.as_slice() {
            [only] => only + 0.207 * em,
            b => b
                .get(3 * (b.len().saturating_sub(1)) / 4)
                .copied()
                .unwrap_or(0.0),
        };
        let lly = baseline - 0.25 * em;
        for &i in row {
            if let Some(r) = out.get_mut(i) {
                *r = Rect::from_corners(r.llx, lly, r.urx, lly + 1.25 * em);
            }
        }
    }
    out
}

fn union(a: Rect, b: Rect) -> Rect {
    Rect::from_corners(
        a.llx.min(b.llx),
        a.lly.min(b.lly),
        a.urx.max(b.urx),
        a.ury.max(b.ury),
    )
}

/// [`block_layout::layout_text`] over `boxes`, each a glyph-free run;
/// `members[i]` is the word indices box `i` stands for.
fn infer(boxes: &[(Rect, &str)], members: &[Vec<usize>]) -> Vec<ResolvedBlock> {
    let mut page = PageText::default();
    page.runs = boxes.iter().map(|(rect, text)| run(*rect, text)).collect();
    let mut text = ExtractedText::default();
    text.pages = vec![page];
    let crop = boxes
        .iter()
        .map(|(r, _)| *r)
        .reduce(union)
        .unwrap_or_else(|| Rect::from_corners(0.0, 0.0, 612.0, 792.0));
    let layout = block_layout::layout_text(
        text,
        &[PageGeometry::new(crop, 0)],
        &LayoutOptions::default(),
    );
    let Some(laid) = layout.pages.first() else {
        return Vec::new();
    };
    laid.blocks
        .iter()
        .map(|b| ResolvedBlock {
            kind: kind_of(&b.kind),
            lines: b
                .lines
                .iter()
                .filter_map(|&l| laid.lines.get(l))
                .map(|l| {
                    l.runs
                        .iter()
                        .filter_map(|&r| members.get(r))
                        .flatten()
                        .copied()
                        .collect()
                })
                .collect(),
        })
        .collect()
}

fn run(rect: Rect, text: &str) -> TextRun {
    TextRun {
        text: text.to_owned(),
        origin: TextOrigin::Glyphs,
        glyphs: Vec::new(),
        artifact: None,
        mcid: None,
        mcid_stream: None,
        artifact_subtype: None,
        in_ocr_layer: false,
        bbox: Some(rect),
    }
}

fn kind_of(kind: &BlockKind) -> OcrBlockKind {
    match kind {
        BlockKind::Heading { .. } => OcrBlockKind::Heading,
        BlockKind::Paragraph => OcrBlockKind::Paragraph,
        BlockKind::ListItem { .. } => OcrBlockKind::ListItem,
        BlockKind::Caption => OcrBlockKind::Caption,
        _ => OcrBlockKind::Other,
    }
}

/// Drops out-of-range and repeated word indices and empty lines and blocks,
/// then appends the words nothing named.
fn cover_every_word(blocks: Vec<ResolvedBlock>, n: usize) -> Vec<ResolvedBlock> {
    let mut seen = vec![false; n];
    let mut out: Vec<ResolvedBlock> = Vec::new();
    for b in blocks {
        let lines: Vec<Vec<usize>> = b
            .lines
            .into_iter()
            .map(|l| {
                l.into_iter()
                    .filter(|&w| seen.get_mut(w).is_some_and(|s| !std::mem::replace(s, true)))
                    .collect::<Vec<_>>()
            })
            .filter(|l| !l.is_empty())
            .collect();
        if !lines.is_empty() {
            out.push(ResolvedBlock {
                kind: b.kind,
                lines,
            });
        }
    }
    let rest: Vec<Vec<usize>> = (0..n)
        .filter(|&w| !seen.get(w).copied().unwrap_or(true))
        .map(|w| vec![w])
        .collect();
    if !rest.is_empty() {
        out.push(ResolvedBlock {
            kind: OcrBlockKind::Other,
            lines: rest,
        });
    }
    out
}

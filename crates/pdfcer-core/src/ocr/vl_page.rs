//! Layout-aware page reading with PaddleOCR-VL, the way its own pipeline
//! reads a page: PP-DocLayoutV3 ([`LayoutEngine`](crate::ocr::engine_layout::LayoutEngine)) finds the regions, each
//! is read with its class's task prompt ([`LayoutClass::task`](crate::ocr::layout::LayoutClass::task)) and the page
//! comes back as an [`OcrPage`] whose blocks are tagged with their region
//! ([`OcrBlock::with_region`]), so a layer can route them to per-region
//! optional-content groups.
//!
//! Inferred, and disclosed by [`LayoutReading`](crate::ocr::vl_page::LayoutReading)'s fields (rule 4):
//!
//! - **Table cell boxes.** The model returns a table's structure (OTSL) but
//!   not where each cell sits, so cells are placed on an even grid over the
//!   table's box ([`RegionRead::grid_placed`](crate::ocr::vl_page::RegionRead::grid_placed)).
//! - **The whole page as one region**, when the layout model finds none
//!   ([`LayoutReading::whole_page`](crate::ocr::vl_page::LayoutReading::whole_page)).
//!
//! Pictures ([`LayoutClass::task`](crate::ocr::layout::LayoutClass::task) `None`) are listed but not read. Words are
//! image pixels, y-down, like every engine's; the caller maps them onto the
//! page (`words_to_page_space_on`).

use crate::page_tree::Rect;

use super::engine_layout::LayoutEngine;
use super::engine_paddle_vl::{PaddleVlEngine, PaddleVlError, RegionReading};
use super::layout::{LayoutClass, LayoutRegion};
use super::otsl::{self, Table, TableCell};
use super::vl_pre::{self, Grey, VlTask};
use super::{CellPosition, OcrBlock, OcrBlockKind, OcrLine, OcrPage, RecognizedWord};

/// One layout region and what was read from it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RegionRead {
    /// The region, in page image pixels.
    pub region: LayoutRegion,
    /// The task it was read as; `None` for a picture, which is not read.
    pub task: Option<VlTask>,
    /// The model's reading, with `lines` and `ink_box` moved from the
    /// region's crop into page image pixels; `None` when not read.
    pub reading: Option<RegionReading>,
    /// The parsed table, for a table region whose answer held cells.
    pub table: Option<Table>,
    /// Whether this region's words are table cells placed on an even grid
    /// over the region (inferred positions).
    pub grid_placed: bool,
}

/// A page read region by region.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct LayoutReading {
    /// Words, one line per word and one block per region (one per table
    /// cell), in the layout model's reading order.
    pub page: OcrPage,
    /// Every region found, in reading order, pictures included.
    pub regions: Vec<RegionRead>,
    /// The layout model found no region, so the whole page was read as one
    /// untagged text block.
    pub whole_page: bool,
}

/// Read a greyscale page (row-major, one byte per pixel) region by region.
///
/// # Errors
///
/// [`PaddleVlError`] from either model; one failing region fails the page.
pub fn read_page_with_layout(
    vl: &PaddleVlEngine,
    layout: &LayoutEngine,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<LayoutReading, PaddleVlError> {
    let regions = layout.detect(width, height, pixels)?;
    if regions.is_empty() {
        let reading = vl.read_region(width, height, pixels)?;
        let mut page = PageBuilder::default();
        page.push_lines(&reading.lines, None, OcrBlockKind::Paragraph);
        return Ok(LayoutReading {
            page: page.finish(),
            regions: Vec::new(),
            whole_page: true,
        });
    }
    let img = vl_pre::grey(width, height, pixels)?;
    let mut reads = Vec::with_capacity(regions.len());
    for region in regions {
        reads.push(read_one(vl, &img, region)?);
    }
    Ok(assemble(reads))
}

fn read_one(
    vl: &PaddleVlEngine,
    img: &Grey,
    region: LayoutRegion,
) -> Result<RegionRead, PaddleVlError> {
    let task = region.class.task();
    let (Some(task), Some(bx)) = (task, crop_box(region.bbox, img.width, img.height)) else {
        return Ok(RegionRead {
            region,
            task,
            reading: None,
            table: None,
            grid_placed: false,
        });
    };
    let (crop, _) = vl_pre::crop(img, bx, 0);
    let (cw, ch) = (dim(crop.width), dim(crop.height));
    let mut reading = vl.read_region_as(task, cw, ch, &crop.pixels)?;
    offset_reading(&mut reading, bx.0, bx.1);
    let table = (task == VlTask::Table)
        .then(|| table_of(&reading.text))
        .flatten();
    Ok(RegionRead {
        region,
        task: Some(task),
        grid_placed: table.is_some(),
        reading: Some(reading),
        table,
    })
}

/// The table in a table answer; `None` when it holds no text cells.
fn table_of(answer: &str) -> Option<Table> {
    Some(otsl::parse(answer)).filter(|t| t.cells.iter().any(|c| !c.text.is_empty()))
}

fn dim(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The region's box rounded outward to whole pixels and clamped to the
/// image; `None` when nothing is left.
fn crop_box(bbox: [f32; 4], w: usize, h: usize) -> Option<vl_pre::PixelBox> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 0..=w/h first
    let at = |v: f32, max: usize| v.clamp(0.0, max as f32) as usize;
    let [x0, y0, x1, y1] = bbox;
    let b = (
        at(x0.floor(), w),
        at(y0.floor(), h),
        at(x1.ceil(), w),
        at(y1.ceil(), h),
    );
    (b.2 > b.0 && b.3 > b.1).then_some(b)
}

fn offset_reading(r: &mut RegionReading, dx: usize, dy: usize) {
    #[allow(clippy::cast_precision_loss)] // page images are far below 2^52 pixels
    let (fx, fy) = (dx as f64, dy as f64);
    for w in &mut r.lines {
        w.rect = Rect::from_corners(
            w.rect.llx + fx,
            w.rect.lly + fy,
            w.rect.urx + fx,
            w.rect.ury + fy,
        );
    }
    r.ink_box = r
        .ink_box
        .map(|(x0, y0, x1, y1)| (x0 + dx, y0 + dy, x1 + dx, y1 + dy));
}

/// Build the page from read regions, in their order.
fn assemble(regions: Vec<RegionRead>) -> LayoutReading {
    let mut page = PageBuilder::default();
    for r in &regions {
        let Some(reading) = &r.reading else { continue };
        let class = r.region.class;
        if let Some(table) = &r.table {
            let confidence = reading.lines.first().and_then(|w| w.confidence);
            page.push_table(table, r.region.bbox, class, confidence);
        } else if r.task == Some(VlTask::Formula) {
            page.push_formula(reading, r.region.bbox, class);
        } else {
            page.push_lines(&reading.lines, Some(class), block_kind(class));
        }
    }
    LayoutReading {
        page: page.finish(),
        regions,
        whole_page: false,
    }
}

/// The block kind a region's text is written as.
fn block_kind(class: LayoutClass) -> OcrBlockKind {
    use LayoutClass as C;
    match class {
        C::DocTitle | C::ParagraphTitle => OcrBlockKind::Heading,
        C::FigureTitle => OcrBlockKind::Caption,
        C::Table => OcrBlockKind::TableCell,
        C::Header | C::Footer | C::Number | C::FormulaNumber => OcrBlockKind::Other,
        _ => OcrBlockKind::Paragraph,
    }
}

/// A formula's LaTeX without its display delimiters.
fn strip_formula(text: &str) -> &str {
    let t = text.trim();
    for (open, close) in [("\\[", "\\]"), ("$$", "$$"), ("\\(", "\\)"), ("$", "$")] {
        if let Some(inner) = t
            .strip_prefix(open)
            .and_then(|s| s.strip_suffix(close))
            .filter(|s| !s.is_empty())
        {
            return inner.trim();
        }
    }
    t
}

/// A cell's box on an even grid over the table's `bbox`, y-down pixels.
fn cell_rect(table: &Table, cell: &TableCell, bbox: [f32; 4]) -> Rect {
    let [x0, y0, x1, y1] = bbox.map(f64::from);
    #[allow(clippy::cast_precision_loss)] // cell counts are capped at otsl::MAX_CELLS
    let (cols, rows) = (table.cols.max(1) as f64, table.rows.max(1) as f64);
    let (cw, rh) = ((x1 - x0) / cols, (y1 - y0) / rows);
    #[allow(clippy::cast_precision_loss)]
    let at = |n: usize| n as f64;
    Rect::from_corners(
        x0 + cw * at(cell.col),
        y0 + rh * at(cell.row),
        x0 + cw * at(cell.col + cell.col_span),
        y0 + rh * at(cell.row + cell.row_span),
    )
}

#[derive(Default)]
struct PageBuilder {
    page: OcrPage,
}

impl PageBuilder {
    /// One word per line, one block over them.
    fn push_lines(
        &mut self,
        words: &[RecognizedWord],
        class: Option<LayoutClass>,
        kind: OcrBlockKind,
    ) {
        if words.is_empty() {
            return;
        }
        let lines: Vec<usize> = words.iter().map(|w| self.push_word(w.clone())).collect();
        let block = OcrBlock::new(kind, lines);
        self.page.blocks.push(match class {
            Some(c) => block.with_region(c),
            None => block,
        });
    }

    /// One block per non-empty cell, placed on the grid.
    fn push_table(
        &mut self,
        table: &Table,
        bbox: [f32; 4],
        class: LayoutClass,
        confidence: Option<f32>,
    ) {
        for cell in table.cells.iter().filter(|c| !c.text.is_empty()) {
            let word = RecognizedWord {
                text: cell.text.split_whitespace().collect::<Vec<_>>().join(" "),
                rect: cell_rect(table, cell, bbox),
                confidence,
            };
            let line = self.push_word(word);
            let at = CellPosition::new(cell.row, cell.col, cell.row_span, cell.col_span);
            self.page.blocks.push(
                OcrBlock::new(OcrBlockKind::TableCell, vec![line])
                    .with_region(class)
                    .with_cell(at),
            );
        }
    }

    /// The formula's LaTeX as one word over its read lines, or the region.
    fn push_formula(&mut self, reading: &RegionReading, bbox: [f32; 4], class: LayoutClass) {
        let joined = reading
            .lines
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let text = strip_formula(&joined);
        if text.is_empty() {
            return;
        }
        let [x0, y0, x1, y1] = bbox.map(f64::from);
        let rect = reading
            .lines
            .iter()
            .map(|w| w.rect)
            .reduce(|a, b| {
                Rect::from_corners(
                    a.llx.min(b.llx),
                    a.lly.min(b.lly),
                    a.urx.max(b.urx),
                    a.ury.max(b.ury),
                )
            })
            .unwrap_or_else(|| Rect::from_corners(x0, y0, x1, y1));
        let word = RecognizedWord {
            text: text.to_owned(),
            rect,
            confidence: reading.lines.first().and_then(|w| w.confidence),
        };
        let line = self.push_word(word);
        self.page
            .blocks
            .push(OcrBlock::new(block_kind(class), vec![line]).with_region(class));
    }

    fn push_word(&mut self, word: RecognizedWord) -> usize {
        self.page.confidence_available |= word.confidence.is_some();
        self.page.words.push(word);
        let index = self.page.words.len() - 1;
        self.page.lines.push(OcrLine::new(vec![index]));
        self.page.lines.len() - 1
    }

    fn finish(self) -> OcrPage {
        self.page
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::ocr::vl_pre::LinePlacement;

    fn word(text: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> RecognizedWord {
        RecognizedWord {
            text: text.to_owned(),
            rect: Rect::from_corners(x0, y0, x1, y1),
            confidence: Some(0.9),
        }
    }

    fn reading(text: &str, lines: Vec<RecognizedWord>) -> RegionReading {
        RegionReading {
            text: text.to_owned(),
            lines,
            placement: LinePlacement::Bands,
            stop: None,
            tokens: 0,
            image_tokens: 0,
            ink_box: Some((1, 2, 3, 4)),
        }
    }

    fn region(class: LayoutClass, bbox: [f32; 4]) -> LayoutRegion {
        LayoutRegion {
            class,
            score: 0.9,
            bbox,
            order: 0.0,
        }
    }

    fn read(class: LayoutClass, bbox: [f32; 4], r: Option<RegionReading>) -> RegionRead {
        let task = class.task();
        let table = r
            .as_ref()
            .filter(|_| task == Some(VlTask::Table))
            .and_then(|r| table_of(&r.text));
        RegionRead {
            region: region(class, bbox),
            task,
            grid_placed: table.is_some(),
            reading: r,
            table,
        }
    }

    #[test]
    fn crop_boxes_round_outward_and_clamp() {
        assert_eq!(crop_box([1.4, 2.6, 9.2, 9.9], 20, 20), Some((1, 2, 10, 10)));
        assert_eq!(
            crop_box([-5.0, -1.0, 50.0, 8.0], 20, 10),
            Some((0, 0, 20, 8))
        );
        assert_eq!(crop_box([5.0, 5.0, 5.0, 9.0], 20, 20), None);
        assert_eq!(crop_box([30.0, 0.0, 40.0, 5.0], 20, 20), None);
    }

    #[test]
    fn a_crop_reading_moves_into_page_pixels() {
        let mut r = reading("a", vec![word("a", 1.0, 2.0, 5.0, 6.0)]);
        offset_reading(&mut r, 100, 50);
        assert_eq!(
            r.lines[0].rect,
            Rect::from_corners(101.0, 52.0, 105.0, 56.0)
        );
        assert_eq!(r.ink_box, Some((101, 52, 103, 54)));
    }

    #[test]
    fn regions_become_tagged_blocks_and_pictures_are_skipped() {
        let title = reading("Title", vec![word("Title", 0.0, 0.0, 50.0, 10.0)]);
        let body = reading(
            "one\ntwo",
            vec![
                word("one", 0.0, 20.0, 50.0, 30.0),
                word("two", 0.0, 30.0, 50.0, 40.0),
            ],
        );
        let out = assemble(vec![
            read(
                LayoutClass::ParagraphTitle,
                [0.0, 0.0, 50.0, 10.0],
                Some(title),
            ),
            read(LayoutClass::Image, [0.0, 10.0, 50.0, 20.0], None),
            read(LayoutClass::Text, [0.0, 20.0, 50.0, 40.0], Some(body)),
        ]);
        let p = &out.page;
        assert_eq!(out.regions.len(), 3);
        assert_eq!((p.words.len(), p.lines.len(), p.blocks.len()), (3, 3, 2));
        assert_eq!(p.blocks[0].kind, OcrBlockKind::Heading);
        assert_eq!(p.blocks[0].region, Some(LayoutClass::ParagraphTitle));
        assert_eq!(p.blocks[1].kind, OcrBlockKind::Paragraph);
        assert_eq!(p.blocks[1].lines, vec![1, 2]);
        assert_eq!(p.words[p.lines[2].words[0]].text, "two");
        assert!(p.confidence_available);
    }

    #[test]
    fn table_cells_sit_on_an_even_grid() {
        let otsl = "<fcel>Month<fcel>Units<nl><fcel>Jan<ecel><nl><fcel>Total<lcel><nl>";
        let r = reading(otsl, vec![word(otsl, 0.0, 0.0, 1.0, 1.0)]);
        let out = assemble(vec![read(
            LayoutClass::Table,
            [100.0, 200.0, 300.0, 260.0],
            Some(r),
        )]);
        assert!(out.regions[0].grid_placed);
        let p = &out.page;
        // Five cells, the empty one dropped.
        assert_eq!(p.blocks.len(), 4);
        assert!(p.blocks.iter().all(|b| b.kind == OcrBlockKind::TableCell));
        assert_eq!(p.blocks[1].cell, Some(CellPosition::new(0, 1, 1, 1)));
        assert_eq!(
            p.words[1].rect,
            Rect::from_corners(200.0, 200.0, 300.0, 220.0)
        );
        let total = &p.words[3];
        assert_eq!(total.text, "Total");
        assert_eq!(total.rect, Rect::from_corners(100.0, 240.0, 300.0, 260.0));
        assert_eq!(p.blocks[3].cell, Some(CellPosition::new(2, 0, 1, 2)));
    }

    #[test]
    fn a_table_answered_in_prose_stays_prose() {
        let r = reading("no cells", vec![word("no cells", 0.0, 0.0, 9.0, 9.0)]);
        let out = assemble(vec![read(
            LayoutClass::Table,
            [0.0, 0.0, 9.0, 9.0],
            Some(r),
        )]);
        assert!(!out.regions[0].grid_placed);
        assert_eq!(out.page.words[0].text, "no cells");
    }

    #[test]
    fn formulas_lose_their_delimiters() {
        assert_eq!(strip_formula(" \\[E=m c^{2}\\] "), "E=m c^{2}");
        assert_eq!(strip_formula("$$x$$"), "x");
        assert_eq!(strip_formula("$y$"), "y");
        assert_eq!(strip_formula("$"), "$");
        let r = reading("\\[a+b\\]", vec![word("\\[a+b\\]", 10.0, 10.0, 40.0, 20.0)]);
        let out = assemble(vec![read(
            LayoutClass::DisplayFormula,
            [0.0, 0.0, 50.0, 30.0],
            Some(r),
        )]);
        assert_eq!(out.page.words[0].text, "a+b");
        assert_eq!(
            out.page.words[0].rect,
            Rect::from_corners(10.0, 10.0, 40.0, 20.0)
        );
        assert_eq!(out.page.blocks[0].region, Some(LayoutClass::DisplayFormula));
    }
}

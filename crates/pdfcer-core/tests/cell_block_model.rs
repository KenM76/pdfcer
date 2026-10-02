//! The cell-aware block model (G080): table cells as blocks, lines that never
//! join across a cell, Up/Down that moves cell to cell, the column-gutter
//! rule, and reflow that wraps at the cell and discloses a cell overflow.
//! Unit cases drive recognition from hand-built glyph runs; the end-to-end
//! cases use an in-memory ruled 2×2 table.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::text_edit::{
    BlockKind, BlockRecognitionOptions, CellRegion, EditableTextModel, ReflowEngine, ReflowRequest,
    TextPosition, detect_cell_regions, reflow_recognition_options,
};
use pdfcer_core::text_extract::{self, ExtractOptions, PageText, TextRun};
use pdfcer_core::writer::SaveOptions;

use crate::table_detect::{doc, grid_lines};
use crate::unit_text_edit_model::{glyph_run, page};

const ADV: f32 = 5.0;

/// `text` laid out from (x, y) at 10pt, every character `ADV` wide.
fn run_at(text: &str, x: f32, y: f32) -> TextRun {
    let chars: Vec<(&str, f32, f32, f32, f32)> = text
        .char_indices()
        .enumerate()
        .map(|(i, (b, c))| (&text[b..b + c.len_utf8()], x + i as f32 * ADV, y, ADV, 10.0))
        .collect();
    glyph_run(&chars)
}

/// Several pieces on one baseline, in one run, at their own x.
fn row_run(pieces: &[(&str, f32)], y: f32) -> TextRun {
    let mut chars = Vec::new();
    for &(text, x) in pieces {
        for (i, (b, c)) in text.char_indices().enumerate() {
            chars.push((&text[b..b + c.len_utf8()], x + i as f32 * ADV, y, ADV, 10.0));
        }
    }
    glyph_run(&chars)
}

fn cell(row: usize, column: usize, llx: f64, lly: f64, urx: f64, ury: f64) -> CellRegion {
    CellRegion::new(0, row, column, Rect::from_corners(llx, lly, urx, ury))
}

fn opts() -> BlockRecognitionOptions {
    BlockRecognitionOptions::default()
}

/// The text of the line the caret `pos` is on.
fn line_of(m: &EditableTextModel<'_>, pos: TextPosition) -> String {
    let li = m.line_at(pos).expect("the caret is on a line");
    m.line_text(&m.lines()[li])
}

/// A caret at the start of the line whose text starts with `prefix`.
fn caret_on(m: &EditableTextModel<'_>, prefix: &str) -> TextPosition {
    let li = m
        .lines()
        .iter()
        .position(|l| m.line_text(l).starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starts with {prefix:?}"));
    m.caret_on_line_nearest_x(li, 0.0).expect("a caret slot")
}

fn block_texts(m: &EditableTextModel<'_>) -> Vec<String> {
    m.blocks().iter().map(|b| m.block_text(b)).collect()
}

#[test]
fn two_cells_on_one_baseline_are_two_lines_and_two_cell_blocks() {
    let p = page(vec![row_run(&[("Part", 76.0), ("Qty", 204.0)], 686.0)]);
    let cells = [
        cell(0, 0, 72.0, 660.0, 200.0, 700.0),
        cell(0, 1, 200.0, 660.0, 328.0, 700.0),
    ];
    let plain = EditableTextModel::recognize(&p, &opts());
    assert_eq!(
        plain.lines().len(),
        1,
        "control: without cells it is one line"
    );

    let m = EditableTextModel::recognize_with_cells(&p, &opts(), &cells);
    assert_eq!(m.lines().len(), 2, "a row is not a line");
    assert_eq!(m.diagnostics().lines_split_by_cell, 1);
    assert_eq!(m.diagnostics().table_cell_blocks, 2);
    assert_eq!(block_texts(&m), ["Part", "Qty"]);
    for (b, c) in m.blocks().iter().zip(&cells) {
        assert_eq!(
            b.kind,
            BlockKind::TableCell {
                table: 0,
                row: 0,
                column: c.column
            }
        );
        assert_eq!(b.cell_rect, Some(c.rect));
    }
    assert!(m.lines().iter().all(|l| l.cell.is_some()));
    assert_eq!(m.cells(), &cells);
}

#[test]
fn paragraphs_come_first_then_cells_by_table_row_column() {
    let p = page(vec![
        run_at("Heading", 72.0, 750.0),
        run_at("d", 204.0, 646.0),
        run_at("c", 76.0, 646.0),
        run_at("b", 204.0, 686.0),
        run_at("a", 76.0, 686.0),
    ]);
    let cells = [
        cell(1, 1, 200.0, 620.0, 328.0, 660.0),
        cell(0, 1, 200.0, 660.0, 328.0, 700.0),
        cell(1, 0, 72.0, 620.0, 200.0, 660.0),
        cell(0, 0, 72.0, 660.0, 200.0, 700.0),
    ];
    let m = EditableTextModel::recognize_with_cells(&p, &opts(), &cells);
    assert_eq!(block_texts(&m), ["Heading", "a", "b", "c", "d"]);
    assert_eq!(m.blocks()[0].kind, BlockKind::Paragraph);
    assert_eq!(m.blocks()[0].cell_rect, None);
    let kinds: Vec<_> = m.blocks()[1..].iter().map(|b| b.kind).collect();
    let want: Vec<_> = [(0, 0), (0, 1), (1, 0), (1, 1)]
        .iter()
        .map(|&(row, column)| BlockKind::TableCell {
            table: 0,
            row,
            column,
        })
        .collect();
    assert_eq!(kinds, want);
}

/// A 2×2 grid, the top-left cell holding two lines.
fn two_by_two() -> (PageText, [CellRegion; 4]) {
    let p = page(vec![
        run_at("alpha beta", 76.0, 686.0),
        run_at("gamma delta", 76.0, 674.0),
        run_at("Qty", 204.0, 686.0),
        run_at("Bolt", 76.0, 640.0),
        run_at("12", 204.0, 640.0),
    ]);
    let cells = [
        cell(0, 0, 72.0, 660.0, 200.0, 700.0),
        cell(0, 1, 200.0, 660.0, 328.0, 700.0),
        cell(1, 0, 72.0, 620.0, 200.0, 660.0),
        cell(1, 1, 200.0, 620.0, 328.0, 660.0),
    ];
    (p, cells)
}

#[test]
fn down_walks_the_cell_then_the_cell_below_and_up_mirrors_it() {
    let (p, cells) = two_by_two();
    let m = EditableTextModel::recognize_with_cells(&p, &opts(), &cells);
    let x = 76.0;
    let first = caret_on(&m, "alpha");
    let second = m.caret_down(first, x);
    assert_eq!(line_of(&m, second), "gamma delta", "within the cell first");
    let below = m.caret_down(second, x);
    assert_eq!(line_of(&m, below), "Bolt", "then row 2's left cell");
    let back = m.caret_up(below, x);
    assert_eq!(
        line_of(&m, back),
        "gamma delta",
        "up lands on the cell's bottom line"
    );
    // From the right column, Down stays in the right column.
    let qty = caret_on(&m, "Qty");
    assert_eq!(line_of(&m, m.caret_down(qty, 204.0)), "12");
}

#[test]
fn a_row_of_empty_cells_is_skipped() {
    let p = page(vec![
        run_at("top", 76.0, 686.0),
        run_at("bottom", 76.0, 606.0),
    ]);
    let cells = [
        cell(0, 0, 72.0, 660.0, 200.0, 700.0),
        cell(1, 0, 72.0, 620.0, 200.0, 660.0),
        cell(2, 0, 72.0, 580.0, 200.0, 620.0),
    ];
    let m = EditableTextModel::recognize_with_cells(&p, &opts(), &cells);
    assert_eq!(m.blocks().len(), 2, "an empty cell is not a block");
    let down = m.caret_down(caret_on(&m, "top"), 76.0);
    assert_eq!(line_of(&m, down), "bottom");
    let up = m.caret_up(caret_on(&m, "bottom"), 76.0);
    assert_eq!(line_of(&m, up), "top");
}

const LEFT: [&str; 4] = ["aaaa bbbb", "cccc dddd", "eeee ffff", "gggg hhhh"];
const RIGHT: [&str; 4] = ["pppp qqqq", "rrrr ssss", "tttt uuuu", "vvvv wwww"];

fn baseline(i: usize) -> f32 {
    700.0 - 12.0 * i as f32
}

#[test]
fn a_row_major_two_column_page_is_one_block_per_column() {
    let runs = (0..4)
        .map(|i| row_run(&[(LEFT[i], 72.0), (RIGHT[i], 300.0)], baseline(i)))
        .collect();
    let p = page(runs);
    let m = EditableTextModel::recognize(&p, &opts());
    assert_eq!(m.diagnostics().lines_split_by_gutter, 4);
    assert_eq!(
        block_texts(&m),
        [LEFT.join("\n"), RIGHT.join("\n")],
        "one block per column paragraph"
    );

    let mut off = opts();
    off.gutter_min_em = f32::INFINITY;
    let joined = EditableTextModel::recognize(&p, &off);
    assert_eq!(joined.diagnostics().lines_split_by_gutter, 0);
    assert_eq!(
        joined.blocks().len(),
        1,
        "control: rows run across the gutter"
    );
}

#[test]
fn a_column_major_two_column_page_is_one_block_per_column() {
    let mut runs: Vec<TextRun> = (0..4).map(|i| run_at(LEFT[i], 72.0, baseline(i))).collect();
    runs.extend((0..4).map(|i| run_at(RIGHT[i], 300.0, baseline(i))));
    let p = page(runs);
    let m = EditableTextModel::recognize(&p, &opts());
    assert_eq!(block_texts(&m), [LEFT.join("\n"), RIGHT.join("\n")]);
}

#[test]
fn a_table_row_spanning_two_text_columns_does_not_fuse_them() {
    let wide = "x".repeat(65);
    let mut runs = vec![run_at(&wide, 76.0, 686.0)];
    runs.extend((0..4).map(|i| run_at(LEFT[i], 72.0, baseline(i) - 100.0)));
    runs.extend((0..4).map(|i| run_at(RIGHT[i], 300.0, baseline(i) - 100.0)));
    let p = page(runs);
    let cells = [cell(0, 0, 72.0, 660.0, 456.0, 700.0)];
    let m = EditableTextModel::recognize_with_cells(&p, &opts(), &cells);
    assert_eq!(m.columns(), 2);
    assert_eq!(
        block_texts(&m),
        [
            LEFT.join(
                "
"
            ),
            RIGHT.join(
                "
"
            ),
            wide
        ]
    );
}

#[test]
fn two_aligned_wide_gaps_are_not_a_gutter() {
    let runs = (0..2)
        .map(|i| row_run(&[(LEFT[i], 72.0), (RIGHT[i], 300.0)], baseline(i)))
        .collect();
    let p = page(runs);
    let m = EditableTextModel::recognize(&p, &opts());
    assert_eq!(m.diagnostics().lines_split_by_gutter, 0);
    assert_eq!(m.lines().len(), 2);
}

#[test]
fn a_wide_gap_inside_a_cell_is_not_a_gutter() {
    let runs = (0..3)
        .map(|i| row_run(&[("Total", 76.0), ("42", 400.0)], 686.0 - 40.0 * i as f32))
        .collect();
    let p = page(runs);
    let cells: Vec<_> = (0..3)
        .map(|r| {
            let top = 700.0 - 40.0 * r as f64;
            cell(r, 0, 72.0, top - 40.0, 456.0, top)
        })
        .collect();
    let plain = EditableTextModel::recognize(&p, &opts());
    assert_eq!(plain.diagnostics().lines_split_by_gutter, 3, "control");

    let m = EditableTextModel::recognize_with_cells(&p, &opts(), &cells);
    assert_eq!(m.diagnostics().lines_split_by_gutter, 0);
    assert_eq!(m.lines().len(), 3);
}

#[test]
fn a_zero_span_is_one() {
    let c = cell(0, 0, 0.0, 0.0, 1.0, 1.0).with_span(0, 0);
    assert_eq!((c.row_span, c.column_span), (1, 1));
}

/// One two-line cell block "alpha beta / gamma delta" at x 76 in a cell
/// from x 72 to `right`.
fn cell_block(right: f64) -> (PageText, [CellRegion; 1]) {
    let p = page(vec![
        run_at("alpha beta", 76.0, 686.0),
        run_at("gamma delta", 76.0, 674.0),
    ]);
    (p, [cell(0, 0, 72.0, 660.0, right, 700.0)])
}

#[test]
fn a_cell_block_rewraps_at_the_cells_inner_width() {
    let (p, cells) = cell_block(272.0);
    let m = EditableTextModel::recognize_with_cells(&p, &opts(), &cells);
    let pv = ReflowEngine::new(&m)
        .preview(0, &ReflowRequest::new())
        .expect("preview");
    // 272 − 4pt padding (the left inset) − 76.
    assert!((pv.wrap_width - 192.0).abs() < 1e-6, "{}", pv.wrap_width);
    assert_eq!(pv.lines_after, 1, "the cell has room for one line");
    assert!(
        pv.lines
            .iter()
            .all(|l| l.origin_x + l.natural_width <= 272.0)
    );
    assert_eq!(pv.cell_overflow, None);
}

#[test]
fn a_width_wider_than_the_cell_is_disclosed_past_the_right_edge() {
    let (p, cells) = cell_block(150.0);
    let m = EditableTextModel::recognize_with_cells(&p, &opts(), &cells);
    let engine = ReflowEngine::new(&m);
    let fits = engine.preview(0, &ReflowRequest::new()).expect("preview");
    assert_eq!(fits.cell_overflow, None, "the cell width keeps it inside");

    let pv = engine
        .preview(0, &ReflowRequest::new().with_wrap_width(400.0))
        .expect("preview");
    let co = pv.cell_overflow.expect("a cell overflow");
    assert!(co.past_right_pt > 30.0, "{co:?}");
    assert_eq!(co.past_bottom_pt, 0.0);
    assert_eq!(co.lines_outside, 1);
}

#[test]
fn growing_past_the_cell_bottom_is_disclosed() {
    let (p, cells) = cell_block(272.0);
    let m = EditableTextModel::recognize_with_cells(&p, &opts(), &cells);
    let pv = ReflowEngine::new(&m)
        .preview(0, &ReflowRequest::new().with_wrap_width(30.0))
        .expect("preview");
    let co = pv.cell_overflow.expect("a cell overflow");
    assert!(co.past_bottom_pt > 0.0, "{co:?}");
    assert!(co.lines_outside >= 1, "{co:?}");
    assert!(
        pv.diagnostics
            .disclosures
            .iter()
            .any(|n| n.contains("does not fit its table cell")),
        "{:?}",
        pv.diagnostics.disclosures
    );
}

#[test]
fn a_paragraph_block_has_no_cell_overflow() {
    let (p, _) = cell_block(272.0);
    let m = EditableTextModel::recognize(&p, &opts());
    let pv = ReflowEngine::new(&m)
        .preview(0, &ReflowRequest::new().with_wrap_width(30.0))
        .expect("preview");
    assert_eq!(pv.cell_overflow, None);
}

// ---- end to end over a ruled 2×2 table ------------------------------------

const GX: [f32; 3] = [72.0, 272.0, 472.0];
const GY: [f32; 3] = [700.0, 660.0, 620.0];

fn table_doc() -> pdfcer_core::document::Document {
    let text = [
        (76, 686, "alpha beta"),
        (76, 674, "gamma delta"),
        (276, 686, "Qty"),
        (76, 640, "Bolt"),
        (276, 640, "12"),
    ]
    .iter()
    .map(|(x, y, t)| format!("BT /F1 10 Tf {x} {y} Td ({t}) Tj ET\n"))
    .collect::<String>();
    doc(&(grid_lines(&GX, &GY) + &text), 0)
}

fn extract(d: &pdfcer_core::document::Document) -> PageText {
    let pages = page_tree::pages(d).expect("page tree");
    text_extract::extract_page(d, &pages[0], 0, &ExtractOptions::default()).expect("extract")
}

#[test]
fn a_ruled_two_by_two_table_is_four_cell_blocks() {
    let d = table_doc();
    let cells = detect_cell_regions(&d.view(), 0).expect("cells");
    assert_eq!(cells.len(), 4);
    let p = extract(&d);
    let m = EditableTextModel::recognize_with_cells(&p, &reflow_recognition_options(), &cells);
    assert_eq!(m.diagnostics().table_cell_blocks, 4);
    assert!(
        m.blocks()
            .iter()
            .all(|b| matches!(b.kind, BlockKind::TableCell { .. }))
    );
    assert!(
        m.lines()
            .iter()
            .all(|l| !(m.line_text(l).contains("alpha") && m.line_text(l).contains("Qty"))),
        "a row is not a line"
    );
    let down = m.caret_down(m.caret_down(caret_on(&m, "alpha"), 76.0), 76.0);
    assert_eq!(line_of(&m, down), "Bolt");
    assert!(detect_cell_regions(&d.view(), 1).is_err(), "no such page");
}

#[test]
fn reflowing_a_cell_keeps_every_glyph_inside_it() {
    let mut s = EditSession::new(table_doc());
    let report = s
        .reflow_block(0, 0, &ReflowRequest::new())
        .expect("reflow the top-left cell");
    assert_eq!(report.cell_overflow, None);
    let (bytes, _) = s.to_full_bytes(&SaveOptions::default()).expect("save");
    let out = pdfcer_core::document::Document::from_bytes(bytes).expect("reload");
    let p = extract(&out);
    let words: Vec<_> = p
        .runs
        .iter()
        .filter(|r| {
            ["alpha", "beta", "gamma", "delta"]
                .iter()
                .any(|w| r.text.contains(w))
        })
        .collect();
    assert!(!words.is_empty());
    let mut baselines: Vec<f32> = Vec::new();
    for g in words.iter().flat_map(|r| &r.glyphs) {
        assert!(g.x >= 72.0 && g.x + g.advance <= 272.0, "x {} outside", g.x);
        assert!(g.y > 660.0 && g.y < 700.0, "y {} outside", g.y);
        if !baselines.iter().any(|b| (b - g.y).abs() < 0.5) {
            baselines.push(g.y);
        }
    }
    assert_eq!(baselines.len(), 1, "re-wrapped at the cell width: one line");
}

#[test]
fn a_reflow_past_the_cell_bottom_is_reported_and_the_grid_stays() {
    let mut s = EditSession::new(table_doc());
    let report = s
        .reflow_block(0, 0, &ReflowRequest::new().with_wrap_width(30.0))
        .expect("reflow");
    let co = report.cell_overflow.expect("disclosed");
    assert!(co.past_bottom_pt > 0.0, "{co:?}");
    let (bytes, _) = s.to_full_bytes(&SaveOptions::default()).expect("save");
    let out = pdfcer_core::document::Document::from_bytes(bytes).expect("reload");
    let before = detect_cell_regions(&table_doc().view(), 0).expect("cells");
    let after = detect_cell_regions(&out.view(), 0).expect("cells");
    let rects = |c: &[CellRegion]| c.iter().map(|c| c.rect).collect::<Vec<_>>();
    assert_eq!(rects(&after), rects(&before), "the cell is not resized");
}

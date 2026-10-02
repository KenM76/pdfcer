//! `PatternType 1` tiling patterns (ISO 32000-1 §8.7.3, Table 75): fills,
//! strokes and text, coloured and uncoloured, anchored to the parent
//! stream's default space, with the cycle guard and the ceilings.
//!
//! Every fixture is a 200 × 200 pt page rendered at scale 1, so device
//! pixel `(x, row)` covers default-space point `(x + 0.5, 199.5 - row)`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

fn build(objects: &[(u32, String)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let max = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    buf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max + 1).as_bytes());
    for num in 1..=max {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            max + 1
        )
        .as_bytes(),
    );
    buf
}

fn stream(dict: &str, data: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{data}\nendstream",
        data.len() + 1
    )
}

/// A page painting `content` with `/P1` = object 5 = a tiling pattern whose
/// Table 75 entries are `dict` and whose cell content is `cell`.
fn page(dict: &str, cell: &str, content: &str) -> Vec<u8> {
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] >>".into(),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << \
             /Pattern << /P1 5 0 R >> /ColorSpace << /CsP [/Pattern /DeviceRGB] >> \
             /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>"
                .into(),
        ),
        (4, stream("", content)),
        (5, stream(&format!("/PatternType 1 {dict}"), cell)),
    ])
}

/// A coloured 10 × 10 pattern whose cell content is `cell`.
const COLOURED: &str =
    "/PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 /YStep 10 /Resources << >>";
/// Red in the lower-left quarter of the cell.
const QUARTER: &str = "1 0 0 rg 0 0 5 5 re f";
const FILL_PAGE: &str = "/Pattern cs /P1 scn 0 0 200 200 re f";

fn render(bytes: Vec<u8>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    render_page_with(&doc, &p, 1.0, &RenderOptions::default()).expect("render")
}

/// The demultiplied colour at default-space point `(x, y)`.
fn at(r: &RenderedPage, x: u32, y: u32) -> (u8, u8, u8) {
    let p = r.pixmap.pixel(x, 199 - y).expect("in bounds").demultiply();
    (p.red(), p.green(), p.blue())
}

const RED: (u8, u8, u8) = (255, 0, 0);
const WHITE: (u8, u8, u8) = (255, 255, 255);

#[test]
fn a_coloured_pattern_tiles_its_cell_across_the_fill() {
    let r = render(page(COLOURED, QUARTER, FILL_PAGE));
    for (x, y) in [(2, 2), (12, 2), (102, 152), (192, 192)] {
        assert_eq!(at(&r, x, y), RED, "({x}, {y}) is in a red quarter");
    }
    for (x, y) in [(7, 2), (2, 7), (107, 157)] {
        assert_eq!(at(&r, x, y), WHITE, "({x}, {y}) is outside the quarter");
    }
    assert_eq!(r.diagnostics.color.tiling_patterns_painted, 1);
    assert_eq!(r.diagnostics.color.patterns_unpainted, 0);
}

/// Table 75 `PaintType 2`: the shape comes from the cell, the colour from
/// `scn`'s operands in the underlying space, and the cell's own `rg` is
/// ignored.
#[test]
fn an_uncoloured_pattern_paints_in_the_scn_colour() {
    let dict = COLOURED.replace("/PaintType 1", "/PaintType 2");
    let r = render(page(
        &dict,
        QUARTER,
        "/CsP cs 0 0 1 /P1 scn 0 0 200 200 re f",
    ));
    assert_eq!(
        at(&r, 2, 2),
        (0, 0, 255),
        "blue from scn, not the cell's red"
    );
    assert_eq!(at(&r, 7, 2), WHITE);
}

/// §8.7.2: pattern space is anchored to the stream's DEFAULT space, so a
/// `cm` between `scn` and the fill must not rescale the tiles. Under CTM
/// anchoring the cell would be 5 pt wide and x = 7 would land in red.
#[test]
fn a_cm_after_scn_does_not_move_the_tiles() {
    let r = render(page(
        COLOURED,
        QUARTER,
        "/Pattern cs /P1 scn q 0.5 0 0 1 0 0 cm 0 0 400 200 re f Q",
    ));
    assert_eq!(at(&r, 2, 2), RED);
    assert_eq!(at(&r, 7, 2), WHITE, "a 5 pt cell would put red here");
    assert_eq!(at(&r, 12, 2), RED);
}

/// `/Matrix` translation is the tile phase (§8.7.3.1).
#[test]
fn the_pattern_matrix_sets_the_phase() {
    let dict = format!("{COLOURED} /Matrix [1 0 0 1 5 0]");
    let r = render(page(&dict, QUARTER, FILL_PAGE));
    assert_eq!(at(&r, 2, 2), WHITE);
    assert_eq!(at(&r, 7, 2), RED);
}

/// A step wider than `/BBox` leaves gaps between tiles.
#[test]
fn a_step_wider_than_the_bbox_leaves_gaps() {
    let dict = COLOURED.replace("/XStep 10", "/XStep 20");
    let r = render(page(&dict, "1 0 0 rg 0 0 10 10 re f", FILL_PAGE));
    assert_eq!(at(&r, 5, 5), RED);
    assert_eq!(at(&r, 15, 5), WHITE, "the gap between tiles");
    assert_eq!(at(&r, 25, 5), RED);
}

/// A `/BBox` wider than the step overlaps the next tile: content at
/// x = 12..14 in one tile shows at x = 2..4 of the tile to its right.
#[test]
fn a_bbox_wider_than_the_step_overlaps_the_next_tile() {
    let dict = COLOURED.replace("/BBox [0 0 10 10]", "/BBox [0 0 15 10]");
    let r = render(page(&dict, "1 0 0 rg 12 0 2 10 re f", FILL_PAGE));
    assert_eq!(at(&r, 13, 5), RED);
    assert_eq!(at(&r, 3, 5), RED, "the overspill from the tile to the left");
    assert_eq!(at(&r, 7, 5), WHITE);
}

/// Table 75: `/XStep` may be negative; the lattice is the same.
#[test]
fn a_negative_step_tiles_the_same_lattice() {
    let dict = COLOURED.replace("/XStep 10", "/XStep -10");
    let r = render(page(&dict, QUARTER, FILL_PAGE));
    assert_eq!(at(&r, 2, 2), RED);
    assert_eq!(at(&r, 12, 2), RED);
    assert_eq!(at(&r, 7, 2), WHITE);
}

#[test]
fn a_stroke_paints_with_the_stroking_pattern() {
    let r = render(page(
        COLOURED,
        "1 0 0 rg 0 0 10 10 re f",
        "/Pattern CS /P1 SCN 10 w 20 100 m 180 100 l S",
    ));
    assert_eq!(at(&r, 100, 100), RED, "on the stroke");
    assert_eq!(at(&r, 100, 110), WHITE, "beyond its half-width");
    assert_eq!(r.diagnostics.color.tiling_patterns_painted, 1);
}

/// Table 60 `B`: fill, then stroke. A solid stroke over a pattern fill is
/// painted after it, so the stroke's inner half shows.
#[test]
fn a_solid_stroke_over_a_pattern_fill_is_painted_on_top() {
    let r = render(page(
        COLOURED,
        "1 0 0 rg 0 0 10 10 re f",
        "/Pattern cs /P1 scn 0 0 1 RG 20 w 50 50 100 100 re B",
    ));
    assert_eq!(at(&r, 55, 100), (0, 0, 255), "inner half of the stroke");
    assert_eq!(at(&r, 100, 100), RED, "the pattern fill inside");
}

/// §9.3.6: text in a pattern colour paints its glyphs with the pattern.
#[test]
fn text_fills_with_the_pattern() {
    let r = render(page(
        COLOURED,
        "1 0 0 rg 0 0 10 10 re f",
        "/Pattern cs /P1 scn BT /F1 120 Tf 20 40 Td (H) Tj ET",
    ));
    let red = (0..200)
        .flat_map(|y| (0..200).map(move |x| (x, y)))
        .filter(|&(x, y)| at(&r, x, y) == RED)
        .count();
    assert!(red > 2000, "the glyph is painted red, got {red} pixels");
    assert!(r.diagnostics.color.tiling_patterns_painted >= 1);
}

/// A pattern whose cell paints with itself is refused, not recursed into.
#[test]
fn a_self_referencing_pattern_is_refused() {
    let dict = COLOURED.replace(
        "/Resources << >>",
        "/Resources << /Pattern << /P1 5 0 R >> >>",
    );
    let r = render(page(&dict, "/Pattern cs /P1 scn 0 0 10 10 re f", FILL_PAGE));
    assert!(r.diagnostics.color.patterns_unpainted >= 1);
    assert!(
        r.diagnostics
            .color
            .notes
            .iter()
            .any(|n| n.contains("paints itself")),
        "the refusal says why: {:?}",
        r.diagnostics.color.notes
    );
}

/// A step tiny against `/BBox` would fold millions of copies into a cell;
/// it is refused under the ceiling and counted.
#[test]
fn a_tiny_step_is_refused_and_counted() {
    let dict = COLOURED
        .replace("/XStep 10", "/XStep 0.001")
        .replace("/YStep 10", "/YStep 0.001");
    let r = render(page(&dict, QUARTER, FILL_PAGE));
    assert_eq!(r.diagnostics.color.tiling_patterns_painted, 0);
    assert_eq!(r.diagnostics.color.patterns_unpainted, 1);
    assert_eq!(at(&r, 2, 2), WHITE);
}

/// Table 75: `/XStep` "shall not be zero".
#[test]
fn a_zero_step_is_refused_and_counted() {
    let dict = COLOURED.replace("/XStep 10", "/XStep 0");
    let r = render(page(&dict, QUARTER, FILL_PAGE));
    assert_eq!(r.diagnostics.color.patterns_unpainted, 1);
    assert_eq!(r.diagnostics.color.tiling_patterns_painted, 0);
    assert!(
        r.diagnostics.color.notes.iter().any(|n| n.contains("zero")),
        "refused as a zero step, not by a later ceiling: {:?}",
        r.diagnostics.color.notes
    );
}

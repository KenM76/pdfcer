//! A supersampled sheet whose fine raster passes `MAX_PIXMAP_EDGE` is drawn
//! in tiles, not refused: the same pixels an untiled render gives, no seam
//! at a tile edge, and the tile count disclosed in the diagnostics.
//!
//! The sheet is 5,000 x 60 pt at 1x, so the 4x fine raster is 20,000 pixels
//! wide — past the 16,384 edge cap, as A1 is at 150 DPI.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_render::{
    EdgeAntialiasing, InkProbeSource, RenderOptions, render_page_region, render_page_with,
};

/// Red then blue rectangles abutting mid-pixel (x = 914.5) and on the
/// first tile edge (x = 915), so a seam inside a tile and one at a tile
/// join would both show as white.
const CONTENT: &str = "1 0 0 rg 0 0 914.5 60 re f 0 0 1 rg 914.5 0 0.5 60 re f \
                       1 0 0 rg 915 0 1085 60 re f 0 1 0 rg 2000 0 3000 60 re f\n";

fn sheet() -> Document {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 5000 60] >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << >> >>".to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{CONTENT}endstream",
            CONTENT.len()
        ),
    ];
    let mut out = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
    }
    let xref_at = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for off in offsets {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
        objects.len() + 1
    ));
    Document::from_bytes(out.into_bytes()).expect("fixture must load")
}

fn supersampled() -> RenderOptions {
    RenderOptions::default().with_edge_antialiasing(EdgeAntialiasing::Supersampled)
}

#[test]
fn an_oversized_supersampled_sheet_renders_in_tiles() {
    let doc = sheet();
    let page = page_tree::pages(&doc).expect("page tree walks").remove(0);
    let r = render_page_with(&doc, &page, 1.0, &supersampled()).expect("the sheet renders");
    assert_eq!((r.pixmap.width(), r.pixmap.height()), (5000, 60));
    assert!(r.diagnostics.supersample_tiles > 1, "tiling is disclosed");
}

#[test]
fn tiles_leave_no_seam() {
    let doc = sheet();
    let page = page_tree::pages(&doc).expect("page tree walks").remove(0);
    let pm = render_page_with(&doc, &page, 1.0, &supersampled())
        .expect("the sheet renders")
        .pixmap;
    for y in [0, 30, 59] {
        for x in 0..2000 {
            let g = pm.pixel(x, y).expect("in range").green();
            assert_eq!(g, 0, "white shows through at ({x}, {y})");
        }
    }
}

#[test]
fn tiles_are_the_pixels_of_an_untiled_render() {
    let doc = sheet();
    let page = page_tree::pages(&doc).expect("page tree walks").remove(0);
    let tiled = render_page_with(&doc, &page, 1.0, &supersampled())
        .expect("the sheet renders")
        .pixmap;
    let left = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: 2000.0,
        ury: 60.0,
    };
    let one = render_page_region(&doc.view(), &page, 1.0, left, &supersampled())
        .expect("the region renders");
    assert_eq!(one.diagnostics.supersample_tiles, 0, "the region fits");
    let one = one.pixmap;
    for y in 0..60 {
        for x in 0..2000 {
            assert_eq!(
                tiled.pixel(x, y),
                one.pixel(x, y),
                "tiled and untiled differ at ({x}, {y})"
            );
        }
    }
}

#[test]
fn the_probe_is_answered_by_the_tile_holding_it() {
    let doc = sheet();
    let page = page_tree::pages(&doc).expect("page tree walks").remove(0);
    let options = supersampled().with_ink_probe(3000, 30);
    let r = render_page_with(&doc, &page, 1.0, &options).expect("the sheet renders");
    let p = r.diagnostics.ink_probe.expect("probe answered");
    assert_ne!(p.source, InkProbeSource::OutOfRange);
    assert_eq!((p.x, p.y), (3000, 30));
    assert_eq!(p.srgb, Some([0, 255, 0]), "the green rectangle");
}

//! # A non-embedded font with no `/Widths` still advances (§9.6.2.2)
//!
//! `/BaseFont /Arial`, no `/Widths`, no embedded program. Arial is not
//! one of the fourteen standard names, so pdfcer's width ladder —
//! `/Widths` → standard-14 AFM → `/MissingWidth` — fell all the way
//! through to `/MissingWidth`, **whose default is 0**. Every glyph then
//! advanced by nothing and the entire run stacked on a single point.
//!
//! Measured on `pdfium/testing/resources/bookmarks.pdf`: "Page1"
//! rendered as an unreadable pile while pdfium and Acrobat both laid it
//! out correctly — both alias Arial to Helvetica. pdfcer's only
//! disclosure was `substituted=1`, which says the SHAPES are pdfcer's and
//! says nothing about the POSITIONS being wrong.
//!
//! `select::by_name` already mapped Arial to the `Sans` slot in order to
//! pick a face; the fix routes that same answer to the metrics.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

fn build(objects: &[(u32, &str)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..=max_num {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<0102> <0304>] >>\nstartxref\n{xref_at}\n%%EOF\n",
            max_num + 1
        )
        .as_bytes(),
    );
    buf
}

/// One line of text in `base_font`, with NO `/Widths` array at all.
fn page(base_font: &str) -> Vec<u8> {
    let content = "BT /F1 24 Tf 10 40 Td (HHHHHHHH) Tj ET\n";
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 80] \
             /Resources << /Font << /F1 5 0 R >> >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>"),
        (
            4,
            &format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            ),
        ),
        (
            5,
            &format!(
                "<< /Type /Font /Subtype /TrueType /BaseFont /{base_font} \
                 /Encoding /WinAnsiEncoding >>"
            ),
        ),
    ])
}

fn render(bytes: Vec<u8>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    render_page_with(&doc, &p, 1.0, &RenderOptions::default()).expect("render")
}

/// Horizontal extent of the inked pixels — the thing a zero advance
/// destroys. Deliberately a WIDTH rather than a pixel count: eight
/// glyphs stacked on one point still ink a similar number of pixels,
/// so counting ink would not have caught this.
fn ink_width(r: &RenderedPage) -> u32 {
    let pm = &r.pixmap;
    let mut min = u32::MAX;
    let mut max = 0u32;
    for y in 0..pm.height() {
        for x in 0..pm.width() {
            if pm.pixel(x, y).is_some_and(|p| p.demultiply().red() < 200) {
                min = min.min(x);
                max = max.max(x);
            }
        }
    }
    if min == u32::MAX { 0 } else { max - min + 1 }
}

#[test]
fn arial_without_widths_lays_out_instead_of_stacking() {
    // Eight 24pt glyphs must span far more than one glyph's width.
    let width = ink_width(&render(page("Arial")));
    assert!(
        width > 100,
        "eight glyphs stacked on one point instead of laying out (ink width {width})"
    );
}

#[test]
fn the_aliased_font_lays_out_like_the_standard_14_name_it_aliases() {
    // The claim is specifically that Arial takes HELVETICA's metrics —
    // not merely that it advances by something. A wrong-but-nonzero
    // advance would pass the test above and fail this one.
    let arial = ink_width(&render(page("Arial")));
    let helvetica = ink_width(&render(page("Helvetica")));
    assert_eq!(
        arial, helvetica,
        "Arial must advance exactly as Helvetica does"
    );
}

#[test]
fn the_serif_and_fixed_aliases_map_to_their_own_families() {
    // Guards the mapping table against a copy-paste that sends every
    // slot to Helvetica — which would still "lay out" and still pass
    // both tests above.
    let times = ink_width(&render(page("TimesNewRoman")));
    let courier = ink_width(&render(page("CourierNew")));
    let helvetica = ink_width(&render(page("Helvetica")));

    assert!(times > 0 && courier > 0);
    assert_eq!(
        times,
        ink_width(&render(page("Times-Roman"))),
        "TimesNewRoman must advance as Times-Roman"
    );
    assert_eq!(
        courier,
        ink_width(&render(page("Courier"))),
        "CourierNew must advance as Courier"
    );
    assert_ne!(
        courier, helvetica,
        "a monospaced alias must not silently take Helvetica's metrics"
    );
}

//! Abutting image tiles must not leave a background seam at their join.
//!
//! # The defect
//!
//! Reported by the `pdfcer-gui` session on 2026-08-18 and reproduced before
//! anything was changed: every SolidWorks shaded view was banded, because a
//! shaded view is dozens of small masked image XObjects laid edge to edge
//! and pdfcer painted each one as an **antialiased** fill of its unit square.
//! Where two tiles abut, source-over compositing of two partial coverages
//! does not reach 1 —
//!
//! ```text
//! coverage = a + b·(1 − a)      a = b = 0.5  ⇒  0.75
//! ```
//!
//! — so the page showed through a join that should be seamless. Measured on
//! the operator's own drawing: a flat `RGB(195,38,38)` panel crossed by rows
//! of `RGB(206,78,78)` every ~30.5 device pixels, **15,253 seam pixels** in
//! the sampled region.
//!
//! # Why this test builds its own document
//!
//! The reporting drawing is a 10 MB customer file outside the repository and
//! is not a fixture this project may check in (`LEGAL.md` §5). The mechanism
//! does not need it: two abutting images reproduce the artefact exactly, and
//! a synthetic pair pins the property rather than one file's appearance.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, render_page_with};

/// Two 1×1 opaque-red images, each placed as a 100×20 pt tile spanning the
/// full page width, stacked so they share a horizontal edge at y = 40.2.
///
/// Full width on purpose: an earlier draft used 40 pt tiles inset from the
/// left, and the sampling column landed on their right EDGE, so the test
/// read an all-white line and passed against the bug. Spanning the page
/// means any column is inside the pair.
///
/// The join is placed at a **non-integer device row** at the scale the test
/// renders (2.5), because an accidentally integer-aligned boundary would
/// tile exactly even with antialiasing on — and the test would pass against
/// the very bug it exists to catch.
fn two_abutting_red_tiles() -> Document {
    // A 1x1 pixel, 8-bit RGB, opaque red.
    let texel = "\u{c3}\u{26}\u{26}";
    let content = "q 100 0 0 20 0 40.2 cm /Im0 Do Q\nq 100 0 0 20 0 20.2 cm /Im0 Do Q\n";

    let objects: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_owned(),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Im0 5 0 R >> >> >>"
                .to_owned(),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            ),
        ),
        (
            5,
            format!(
                "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB \
                 /BitsPerComponent 8 /Length {} >>\nstream\n{texel}endstream",
                texel.len()
            ),
        ),
    ];

    let mut out = String::from("%PDF-1.7\n");
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &objects {
        offsets.push((*num, out.len()));
        out.push_str(&format!("{num} 0 obj\n{body}\nendobj\n"));
    }
    let xref_at = out.len();
    out.push_str("xref\n0 6\n0000000000 65535 f \n");
    for n in 1..=5u32 {
        let off = offsets
            .iter()
            .find(|(num, _)| *num == n)
            .map_or(0, |(_, o)| *o);
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
    ));

    // The texel bytes are Latin-1 by construction above, so this round-trips
    // to the intended three bytes rather than to UTF-8 multibyte sequences.
    let bytes: Vec<u8> = out.chars().map(|c| c as u32 as u8).collect();
    Document::from_bytes(bytes).expect("fixture must load")
}

/// ★ No pixel strictly inside the stacked pair may be lighter than the tile
/// colour.
///
/// The assertion is deliberately about **background bleed**, not about an
/// exact colour: any seam shows up as a pixel pulled toward the white page,
/// and a test that demanded an exact RGB would also fail on a legitimate
/// change to image filtering.
#[test]
fn two_abutting_image_tiles_leave_no_lighter_row_at_their_join() {
    let doc = two_abutting_red_tiles();
    let page = page_tree::pages(&doc).expect("page tree walks").remove(0);
    let rendered =
        render_page_with(&doc, &page, 2.5, &RenderOptions::default()).expect("the page renders");
    let pm = &rendered.pixmap;

    // Sample a vertical line through the middle of the tiles, staying one
    // pixel clear of the outer edges — the OUTER boundary of the pair is a
    // real image edge against the page and is not what this tests.
    let x = pm.width() / 2;
    let mut bleed: Vec<(u32, [u8; 3])> = Vec::new();
    for y in 0..pm.height() {
        let Some(px) = pm.pixel(x, y) else { continue };
        let (r, g, b) = (px.red(), px.green(), px.blue());
        // Inside the pair the colour is the tile's. A seam is lighter:
        // the green and blue channels rise toward the white page while red
        // stays high.
        if r > 150 && g > 60 && g < 200 {
            bleed.push((y, [r, g, b]));
        }
    }

    // Two rows of genuine antialiasing are expected at the pair's OUTER top
    // and bottom edges, where an image really does meet the page. The join
    // in the middle must contribute none.
    assert!(
        bleed.len() <= 2,
        "expected at most the pair's two outer edges to be soft, found {} lighter rows: {:?}",
        bleed.len(),
        bleed
    );
}

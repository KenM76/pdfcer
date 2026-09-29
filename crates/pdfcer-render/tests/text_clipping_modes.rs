//! Text rendering modes 4–7 add glyph outlines to the clip at `ET`
//! (ISO 32000-1 §9.3.6): nonzero winding, after the object's own paints,
//! and no clipping at all when no shown glyph has an outline.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::render_page_view;

/// A 200×200 page with a non-embedded Helvetica `/F1` and `content`.
fn page(content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    let n = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

/// Fraction of pixels that are strongly `channel`-dominant (0 = R, 2 = B).
fn share(content: &str, channel: usize) -> f64 {
    let doc = Document::from_bytes(page(content)).expect("synthetic page loads");
    let pages = page_tree::pages(&doc).expect("page tree");
    let r = render_page_view(&doc.view(), &pages[0], 1.0).expect("renders");
    let px = r.pixmap.data();
    let hits = px
        .chunks_exact(4)
        .filter(|p| {
            let c = p[channel];
            let others = (0..3)
                .filter(|&i| i != channel)
                .map(|i| p[i])
                .max()
                .unwrap();
            c > 200 && others < 60
        })
        .count();
    #[allow(clippy::cast_precision_loss)] // pixel counts are far below 2^52
    let f = hits as f64 / (px.len() / 4) as f64;
    f
}

const BLUE_PAGE: &str = "0 0 1 rg 0 0 200 200 re f";

#[test]
fn mode_7_confines_later_paint_to_the_glyph() {
    let full = share(BLUE_PAGE, 2);
    assert!(full > 0.99, "unclipped fill covers the page: {full}");
    let clipped = share(
        &format!("BT /F1 150 Tf 7 Tr 20 30 Td (H) Tj ET {BLUE_PAGE}"),
        2,
    );
    assert!(
        clipped > 0.05 && clipped < 0.6,
        "the fill after a mode-7 `H` shows only through the glyph: {clipped}"
    );
}

#[test]
fn a_clip_mode_object_with_only_spaces_does_not_clip() {
    let blue = share(
        &format!("BT /F1 150 Tf 7 Tr 20 30 Td (  ) Tj ET {BLUE_PAGE}"),
        2,
    );
    assert!(blue > 0.99, "no outline shown, so no clip: {blue}");
}

#[test]
fn q_restores_the_clip_a_text_object_set() {
    let blue = share(
        &format!("q BT /F1 150 Tf 7 Tr 20 30 Td (H) Tj ET Q {BLUE_PAGE}"),
        2,
    );
    assert!(blue > 0.99, "Q undoes the text clip: {blue}");
}

#[test]
fn mode_4_fills_the_glyph_and_then_clips() {
    let content = format!("BT /F1 150 Tf 4 Tr 1 0 0 rg 20 30 Td (H) Tj ET {BLUE_PAGE}");
    let red = share(&content, 0);
    let blue = share(&content, 2);
    assert!(
        red < 0.01,
        "the later blue fill covers the red glyph: {red}"
    );
    assert!(
        blue > 0.05 && blue < 0.6,
        "and paints nothing outside it: {blue}"
    );
}

#[test]
fn mode_4_paint_inside_the_object_is_not_clipped_by_its_own_glyphs() {
    // §9.3.6: the clip lands after ALL the object's paints, so the second
    // glyph (drawn away from the first) is painted in full.
    let content = "BT /F1 80 Tf 4 Tr 1 0 0 rg 10 110 Td (H) Tj 0 -100 Td (H) Tj ET";
    let two = share(content, 0);
    let one = share("BT /F1 80 Tf 4 Tr 1 0 0 rg 10 110 Td (H) Tj ET", 0);
    assert!(one > 0.01, "one glyph paints: {one}");
    assert!(
        two > one * 1.8,
        "both glyphs paint, unclipped by each other: {one} vs {two}"
    );
}

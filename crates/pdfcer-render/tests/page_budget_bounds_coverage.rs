//! A page's painted area is bounded across the whole render (`Pass 703.0`,
//! `X067` F028; ARCHITECTURE.md §10.1). Past `MIN_COVERAGE` device pixels
//! (or `COVERAGE_PER_CANVAS` canvases, if larger), the rest of the page is
//! left out and counted in `content_truncated`.
//!
//! The charged paints use `Separation /None`, which charges its reach but
//! draws nothing (§8.6.6.4), so a debug build reaches the ceiling without
//! filling billions of pixels. A red fill after them shows whether the page
//! was still being drawn.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::RenderedPage;
use pdfcer_render::page_budget::{COVERAGE_PER_CANVAS, MIN_COVERAGE};

/// The page side in points, rendered at scale 1.
const SIDE: u64 = 4000;

fn pdf(content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {SIDE} {SIDE}] \
             /Resources << /ColorSpace << /CS0 [/Separation /None /DeviceGray \
             << /FunctionType 2 /Domain [0 1] /C0 [1] /C1 [0] /N 1 >>] >> >> \
             /Contents 4 0 R >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

/// `count` uncharged-colour paints from `op`, then a red square.
fn page(count: u64, op: &str) -> Vec<u8> {
    let mut content = String::from("/CS0 cs 1 scn /CS0 CS 1 SCN\n");
    for _ in 0..count {
        content.push_str(op);
        content.push('\n');
    }
    content.push_str("1 0 0 rg 100 100 200 200 re f\n");
    pdf(&content)
}

fn render(bytes: Vec<u8>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("synthetic file parses");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page(&doc, &pages[0], 1.0).expect("renders")
}

fn red_drawn(p: &RenderedPage) -> bool {
    let y = u32::try_from(SIDE - 200).expect("fits");
    let px = p.pixmap.pixel(200, y).expect("on the page");
    (px.red(), px.green(), px.blue()) == (255, 0, 0)
}

/// Full-page fills that spend the ceiling.
fn fills_to_ceiling() -> u64 {
    let canvas = SIDE * SIDE;
    let limit = (canvas * COVERAGE_PER_CANVAS).max(MIN_COVERAGE);
    limit.div_ceil(canvas)
}

const FULL_FILL: &str = "0 0 4000 4000 re f";

#[test]
fn fills_past_the_ceiling_leave_the_rest_of_the_page_out() {
    let p = render(page(fills_to_ceiling() + 50, FULL_FILL));
    assert!(p.diagnostics.content_truncated > 0);
    assert!(
        !red_drawn(&p),
        "the fill after the ceiling must be left out"
    );
}

#[test]
fn fills_under_the_ceiling_draw_everything() {
    let p = render(page(fills_to_ceiling() - 10, FULL_FILL));
    assert_eq!(p.diagnostics.content_truncated, 0);
    assert!(red_drawn(&p));
}

#[test]
fn diagonal_hairlines_are_charged_by_length_not_bounding_box() {
    // Each bounding box is the whole canvas, so charging boxes would spend
    // the ceiling well inside this count.
    let p = render(page(fills_to_ceiling() * 4, "0 0 m 4000 4000 l S"));
    assert_eq!(p.diagnostics.content_truncated, 0);
    assert!(red_drawn(&p));
}

#[test]
fn clip_only_paths_are_not_charged() {
    // CAD exporters set a page-sized clip with `n` before every stroke.
    let p = render(page(fills_to_ceiling() * 4, "q 0 0 4000 4000 re W n Q"));
    assert_eq!(p.diagnostics.content_truncated, 0);
    assert!(red_drawn(&p));
}

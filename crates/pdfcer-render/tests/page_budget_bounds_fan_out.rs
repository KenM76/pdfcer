//! A page's nested paints are bounded across the whole render, not just
//! per stream (`Pass 703.0`, `X067` F054/F055; ARCHITECTURE.md §10.1).
//!
//! The depth limit and the cycle set already stop unbounded recursion,
//! but not fan-out: distinct forms that each draw the next one twice, or a
//! Type 3 glyph that shows two of its own glyphs, multiply at every level
//! the depth limit allows. Each hostile case is paired with a control that
//! is identical except for its size, which must render whole with nothing
//! truncated.
//!
//! The pages are 20 pt square because each form's cost grows with the
//! canvas area; these tests bound the COUNT of paints, and a full-size page
//! in a debug build would be measuring fill throughput instead.

use std::time::{Duration, Instant};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::RenderedPage;

/// The acceptance bound from `X067`.
const BOUND: Duration = Duration::from_secs(30);

/// Assemble a PDF from numbered object bodies (object 1 is the catalog).
fn pdf(objects: &[String]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn stream(dict: &str, data: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{data}\nendstream",
        data.len()
    )
}

/// A page that draws `Fm0`, where `Fm<i>` draws `Fm<i+1>` twice and the
/// last form fills a small square. `levels` forms means 2^(levels-1)
/// leaf paints.
fn doubling_forms(levels: usize) -> Vec<u8> {
    let first_form = 4;
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] \
             /Resources << /XObject << /Fm0 {first_form} 0 R >> >> \
             /Contents {} 0 R >>",
            first_form + levels
        ),
    ];
    for i in 0..levels {
        let form = if i + 1 == levels {
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 20 20]",
                "0 0 1 rg 5 5 10 10 re f",
            )
        } else {
            stream(
                &format!(
                    "/Type /XObject /Subtype /Form /BBox [0 0 20 20] \
                     /Resources << /XObject << /Fm {} 0 R >> >>",
                    first_form + i + 1
                ),
                "/Fm Do /Fm Do",
            )
        };
        objs.push(form);
    }
    objs.push(stream("", "/Fm0 Do"));
    pdf(&objs)
}

/// A Type 3 font whose coloured (`d0`) glyph `a` fills a blue square and then shows `aa` in the
/// same font at half size.
fn self_doubling_type3() -> Vec<u8> {
    let objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] \
         /Resources << /Font << /F1 4 0 R >> >> /Contents 6 0 R >>"
            .to_string(),
        "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000] \
         /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /a 5 0 R >> \
         /Encoding << /Type /Encoding /Differences [97 /a] >> \
         /FirstChar 97 /LastChar 97 /Widths [1000] \
         /Resources << /Font << /F1 4 0 R >> >> >>"
            .to_string(),
        stream(
            "",
            "1000 0 d0 0 0 1 rg 0 0 1000 1000 re f \
             BT /F1 0.5 Tf (aa) Tj ET",
        ),
        stream("", "BT /F1 10 Tf 5 5 Td (a) Tj ET"),
    ];
    pdf(&objs)
}

fn render(bytes: Vec<u8>) -> (RenderedPage, Duration) {
    let doc = Document::from_bytes(bytes).expect("synthetic file parses");
    let pages = page_tree::pages(&doc).expect("page tree");
    let start = Instant::now();
    let page = pdfcer_render::render_page(&doc, &pages[0], 1.0).expect("renders");
    (page, start.elapsed())
}

fn painted_blue(p: &RenderedPage) -> bool {
    let px = p.pixmap.pixel(10, 10).expect("centre is on the page");
    (px.red(), px.green(), px.blue()) == (0, 0, 255)
}

#[test]
fn doubling_forms_are_truncated_within_the_bound_and_still_draw() {
    // 2^39 leaf paints if nothing bounded the total.
    let (p, took) = render(doubling_forms(40));
    assert!(took < BOUND, "took {took:?}");
    assert!(
        p.diagnostics.content_truncated > 0,
        "the paint budget must have been spent"
    );
    assert_eq!(
        p.diagnostics.xobject_depth_overflows, 0,
        "40 levels is under the depth limit, so only the budget can have stopped it"
    );
    assert!(
        painted_blue(&p),
        "the paints inside the budget must still land"
    );
}

#[test]
fn a_small_doubling_tree_renders_whole() {
    let (p, _) = render(doubling_forms(8));
    assert_eq!(p.diagnostics.content_truncated, 0);
    assert!(painted_blue(&p));
}

#[test]
fn a_self_doubling_type3_glyph_is_truncated_within_the_bound() {
    // 2^64 glyph procedures without a total; the depth limit alone does
    // not help, because every level is under it.
    let (p, took) = render(self_doubling_type3());
    assert!(took < BOUND, "took {took:?}");
    assert!(p.diagnostics.content_truncated > 0);
    assert!(painted_blue(&p));
}

//! A content stream too large to hold whole is drawn a window at a time
//! (`Pass 703.0`, `X067` F152; ARCHITECTURE.md §10.1). The interpreter's
//! state carries across windows, and a decompression bomb draws what it
//! decoded within `MAX_DECODE_RATIO` times its encoded size.

use pdfcer_core::document::Document;
use pdfcer_core::filters::flate;
use pdfcer_core::page_tree;
use pdfcer_render::RenderedPage;

const SIDE: u32 = 400;

/// One page whose `/Contents` is `content`, Flate-encoded.
fn pdf(content: &[u8]) -> Vec<u8> {
    let enc = flate::encode(content);
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    let heads = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {SIDE} {SIDE}] /Contents 4 0 R >>"),
    ];
    for (i, body) in heads.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    offsets.push(out.len());
    out.extend_from_slice(
        format!(
            "4 0 obj\n<< /Length {} /Filter /FlateDecode >>\nstream\n",
            enc.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(&enc);
    out.extend_from_slice(b"\nendstream\nendobj\n");
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

fn render(bytes: Vec<u8>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("synthetic file parses");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page(&doc, &pages[0], 1.0).expect("renders")
}

fn rgb_at(p: &RenderedPage, x: u32, y_up: u32) -> (u8, u8, u8) {
    let px = p.pixmap.pixel(x, SIDE - 1 - y_up).expect("on the page");
    (px.red(), px.green(), px.blue())
}

/// `head`, then `filler` repeated to `len` bytes, then `tail`.
fn content(head: &str, filler: &str, len: usize, tail: &str) -> Vec<u8> {
    let mut c = head.as_bytes().to_vec();
    c.extend_from_slice(&filler.as_bytes().repeat(len / filler.len()));
    c.extend_from_slice(tail.as_bytes());
    c
}

#[test]
fn state_set_in_one_window_paints_in_a_later_one() {
    // 10 MiB of clip-only paths spans several 4 MiB windows; the colour
    // and the `q`/`cm` set in the first are in force for the last fill.
    let c = content(
        "q 1 0 0 1 50 50 cm 1 0 0 rg\n",
        "0 0 1 1 re n\n",
        10 << 20,
        "0 0 100 100 re f Q\n",
    );
    let p = render(pdf(&c));
    assert_eq!(rgb_at(&p, 100, 100), (255, 0, 0));
    assert_eq!(rgb_at(&p, 20, 20), (255, 255, 255));
    assert_eq!(p.diagnostics.content_truncated, 0);
}

#[test]
#[ignore = "decodes 256 MiB; run with --ignored"]
fn a_bomb_draws_its_prefix_and_says_so() {
    let c = content(
        "1 0 0 rg 0 0 100 100 re f\n",
        "0 0 1 1 re n\n",
        300 << 20,
        "0 0 1 rg 200 200 100 100 re f\n",
    );
    let p = render(pdf(&c));
    assert_eq!(rgb_at(&p, 50, 50), (255, 0, 0));
    assert_eq!(rgb_at(&p, 250, 250), (255, 255, 255));
    assert_eq!(p.diagnostics.content_truncated, 1);
    assert!(
        p.diagnostics
            .sample_ops
            .iter()
            .any(|n| n.contains("past 64x its encoded size")),
        "{:?}",
        p.diagnostics.sample_ops
    );
}

#[test]
#[ignore = "decodes 277 MB; run with --ignored"]
fn a_page_past_the_decode_ceiling_draws_whole() {
    // The X067 F152 shape: legitimately huge content (here, distinct
    // coordinates so it does not compress like a bomb) past
    // MAX_DECODED_LEN, which a whole decode refuses.
    let mut c = b"1 0 0 rg\n".to_vec();
    let mut i = 0u64;
    while c.len() < 277_000_000 {
        c.extend_from_slice(format!("{} {} 1 1 re n\n", i % 397, (i / 397) % 389).as_bytes());
        i += 1;
    }
    c.extend_from_slice(b"0 0 100 100 re f\n");
    let p = render(pdf(&c));
    assert_eq!(rgb_at(&p, 50, 50), (255, 0, 0));
    assert_eq!(p.diagnostics.content_truncated, 0);
}

//! A form's `/BBox` clip (ISO 32000-2 §8.10.1) that already contains the
//! clip in force is skipped rather than built into a full-canvas mask
//! (`Pass 703.0`, `X067` large canvases). Skipping must never widen what
//! the page shows.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;

/// Page 100×100; form 5 has `outer`, form 6 `inner`, form 7 the page box.
/// The leaf paints the whole page red.
fn pdf(outer: &str, inner: &str) -> Vec<u8> {
    let form = |bbox: &str, body: &str, res: &str| {
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [{bbox}] {res} /Length {} >>\n\
             stream\n{body}\nendstream",
            body.len()
        )
    };
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] \
         /Resources << /XObject << /F 5 0 R >> >> /Contents 4 0 R >>"
            .to_string(),
        "<< /Length 5 >>\nstream\n/F Do\nendstream".to_string(),
        form(outer, "/F Do", "/Resources << /XObject << /F 6 0 R >> >>"),
        form(inner, "/F Do", "/Resources << /XObject << /F 7 0 R >> >>"),
        form("0 0 100 100", "1 0 0 rg 0 0 100 100 re f", ""),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 8\n0000000000 65535 f \n");
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 8 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

/// Whether the pixel at PDF point (`x`, `y`) of the scale-2 render is red.
fn red_at(bytes: Vec<u8>, points: &[(u32, u32)]) -> Vec<bool> {
    let doc = Document::from_bytes(bytes).expect("synthetic file parses");
    let pages = page_tree::pages(&doc).expect("page tree");
    let p = pdfcer_render::render_page(&doc, &pages[0], 2.0).expect("renders");
    points
        .iter()
        .map(|&(x, y)| {
            let px = p.pixmap.pixel(x * 2, (100 - y) * 2).expect("on the page");
            (px.red(), px.green(), px.blue()) == (255, 0, 0)
        })
        .collect()
}

#[test]
fn an_inner_bbox_still_clips_inside_an_outer_one() {
    let got = red_at(
        pdf("10 10 90 90", "30 30 70 70"),
        &[(50, 50), (20, 20), (80, 80), (5, 5)],
    );
    assert_eq!(got, [true, false, false, false]);
}

#[test]
fn an_inner_bbox_wider_than_the_outer_does_not_widen_it() {
    let got = red_at(
        pdf("10 10 90 90", "0 0 100 100"),
        &[(50, 50), (11, 11), (89, 89), (5, 5), (95, 95)],
    );
    assert_eq!(got, [true, true, true, false, false]);
}

//! A page placed as content (`place_page_content`) renders as the same page
//! placed as a stamp (`place_page_artwork`) into the same rectangle.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree::Rect;
use pdfcer_render::render_page_view;

fn one_page(media: &str, body: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [{media}] /Contents 4 0 R >>"),
        format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn render(session: &EditSession) -> Vec<u8> {
    let page = session.pages().unwrap()[0].clone();
    render_page_view(&session.view(), &page, 1.0)
        .unwrap()
        .pixmap
        .data()
        .to_vec()
}

#[test]
fn content_placement_renders_as_the_stamp_does() {
    // A stroked path on a crop box with a non-zero origin, squashed.
    let source = Document::from_bytes(one_page(
        "20 10 164 82",
        "1 0 0 RG 4 w 30 20 m 150 70 l S 0 0 1 rg 60 30 20 20 re f",
    ))
    .unwrap();
    let target = || EditSession::new(Document::from_bytes(one_page("0 0 300 200", "")).unwrap());
    let rect = Rect {
        llx: 40.0,
        lly: 30.0,
        urx: 240.0,
        ury: 170.0,
    };

    let mut stamped = target();
    stamped
        .place_page_artwork(&source.view(), 0, 0, rect)
        .unwrap();
    let mut drawn = target();
    let placed = drawn
        .place_page_content(&source.view(), 0, 0, rect)
        .unwrap();
    assert!(placed.distorted);

    let (a, b) = (render(&stamped), render(&drawn));
    let blank = render(&target());
    assert_ne!(b, blank, "the content placement drew nothing");
    let worst = a.iter().zip(&b).map(|(x, y)| x.abs_diff(*y)).max().unwrap();
    assert!(worst <= 2, "stamp and content placement differ by {worst}");
}

//! `flatten_annotations` must not change what the page looks like: the
//! burned appearance, its `/CA` group and its `/OC` wrapper render as the
//! annotation did (rule 4's screenshot test, made a pixel comparison).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_render::render_page_view;

fn assemble(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn stream(extra: &str, content: &str) -> String {
    format!(
        "<< {extra} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// A plain burn with a non-identity `/Matrix`, a `/CA 0.5` one overlapping
/// the page content, and one on a hidden layer; `oc_on` picks the layer's
/// state.
fn fixture(rotate: u32, oc_on: bool) -> EditSession {
    let d = if oc_on { "/ON [9 0 R]" } else { "/OFF [9 0 R]" };
    let bytes = assemble(&[
        format!("<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [9 0 R] /D << {d} >> >> >>"),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 80] /Rotate {rotate} /Contents 4 0 R /Annots [5 0 R 6 0 R 7 0 R] >>"),
        stream("", "0 0 1 rg 0 30 100 20 re f"),
        "<< /Type /Annot /Subtype /Square /Rect [10 10 30 30] /F 4 /AP << /N 8 0 R >> >>".into(),
        "<< /Type /Annot /Subtype /Square /Rect [40 20 80 60] /F 4 /CA 0.5 /AP << /N 10 0 R >> >>".into(),
        "<< /Type /Annot /Subtype /Square /Rect [60 60 90 75] /F 4 /OC 9 0 R /AP << /N 10 0 R >> >>".into(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Matrix [0 2 -1 0 10 0]",
            "1 0 0 rg 0 0 10 5 re f",
        ),
        "<< /Type /OCG /Name (Marks) >>".into(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10]",
            "1 0 0 rg 0 0 10 10 re f 0 1 0 rg 2 2 6 6 re f",
        ),
    ]);
    EditSession::new(Document::from_bytes(bytes).expect("parses"))
}

fn max_channel_diff(rotate: u32, oc_on: bool) -> (u8, usize) {
    let mut s = fixture(rotate, oc_on);
    let page = s.pages().unwrap()[0].clone();
    let before = render_page_view(&s.view(), &page, 2.0).unwrap();
    let out = s.flatten_annotations(0, None).unwrap();
    assert_eq!(out.flattened, 3);
    let page = s.pages().unwrap()[0].clone();
    let after = render_page_view(&s.view(), &page, 2.0).unwrap();
    assert_eq!(before.pixmap.width(), after.pixmap.width());
    let (a, b) = (before.pixmap.data(), after.pixmap.data());
    let diff = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap();
    let painted = a.chunks(4).filter(|p| p[..3] != [255, 255, 255]).count();
    (diff, painted)
}

#[test]
fn flattening_does_not_change_the_page() {
    for rotate in [0, 90] {
        for oc_on in [true, false] {
            let (diff, painted) = max_channel_diff(rotate, oc_on);
            assert!(painted > 1000, "the fixture paints something");
            assert!(
                diff <= 1,
                "rotate {rotate}, layer on {oc_on}: max channel difference {diff}"
            );
        }
    }
}

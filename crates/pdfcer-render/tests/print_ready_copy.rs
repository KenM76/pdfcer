//! The print-ready copy paints what the source prints, with annotation drawing
//! off (pdfcer-gui request G273): that is the print service the copy is for.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot_author::FreeTextC;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_render::{AnnotationScope, RenderOptions, render_page_with};

/// Red poster on the left half (prints), blue square on the right (Print clear).
fn source() -> Vec<u8> {
    let form = |content: &str| {
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 50 100] /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
    };
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Annots [4 0 R 5 0 R] >>".to_owned(),
        "<< /Type /Annot /Subtype /3D /F 4 /Rect [0 0 50 100] /AP << /N 6 0 R >> >>".to_owned(),
        "<< /Type /Annot /Subtype /Square /F 0 /Rect [50 0 100 100] /AP << /N 7 0 R >> >>"
            .to_owned(),
        form("1 0 0 rg 0 0 50 100 re f"),
        form("0 0 1 rg 0 0 50 100 re f"),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
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

#[test]
fn the_copy_paints_the_poster_with_annotation_drawing_off() {
    let source = Document::from_bytes(source()).unwrap();
    let copy = EditSession::new(source)
        .to_print_ready_bytes(FreeTextC::default())
        .unwrap();
    let doc = Document::from_bytes(copy.bytes).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let options = RenderOptions::default().with_annotation_scope(AnnotationScope::ContentOnly);
    let out = render_page_with(&doc, &pages[0], 1.0, &options).unwrap();
    let (w, h) = (out.pixmap.width() as usize, out.pixmap.height() as usize);
    let data = out.pixmap.data();
    let at = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        (data[i], data[i + 1], data[i + 2])
    };
    assert_eq!(at(w / 4, h / 2), (255, 0, 0), "the poster prints");
    assert_eq!(at(3 * w / 4, h / 2), (255, 255, 255), "the other does not");
}

/// A red square set to print but not to show, and a blue one the reverse.
fn print_flags_source() -> Vec<u8> {
    let form = |content: &str| {
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 50 100] /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
    };
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Annots [4 0 R 5 0 R] >>".to_owned(),
        "<< /Type /Annot /Subtype /Square /F 36 /Rect [0 0 50 100] /AP << /N 6 0 R >> >>"
            .to_owned(),
        "<< /Type /Annot /Subtype /Square /F 0 /Rect [50 0 100 100] /AP << /N 7 0 R >> >>"
            .to_owned(),
        form("1 0 0 rg 0 0 50 100 re f"),
        form("0 0 1 rg 0 0 50 100 re f"),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
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

/// Table 165: the Print flag decides a print and NoView decides the screen.
#[test]
fn a_print_answers_to_the_print_flag_and_a_screen_to_noview() {
    let doc = Document::from_bytes(print_flags_source()).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let sides = |options: &RenderOptions| {
        let out = render_page_with(&doc, &pages[0], 1.0, options).unwrap();
        let (w, h) = (out.pixmap.width() as usize, out.pixmap.height() as usize);
        let data = out.pixmap.data();
        let at = |x: usize| {
            let i = (h / 2 * w + x) * 4;
            (data[i], data[i + 1], data[i + 2])
        };
        (at(w / 4), at(3 * w / 4))
    };
    let white = (255, 255, 255);
    assert_eq!(
        sides(&RenderOptions::default().with_print_intent(true)),
        ((255, 0, 0), white),
        "print"
    );
    assert_eq!(
        sides(&RenderOptions::default()),
        (white, (0, 0, 255)),
        "screen"
    );
}

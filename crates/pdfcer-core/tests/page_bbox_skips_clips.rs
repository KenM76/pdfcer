//! A page's drawn extent (`PageObjects::page_bbox`) and the off-page scan
//! skip a path ended by `n`: a clip or bare end-path paints nothing.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::offpage::scan_document;

/// A 100 x 100 pt page with the given content stream.
fn page(content: &[u8]) -> Vec<u8> {
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>".to_vec(),
        {
            let mut s = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            s.extend_from_slice(content);
            s.extend_from_slice(b"\nendstream");
            s
        },
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(o);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    let n = objects.len() + 1;
    pdf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

fn drawn_extent(content: &[u8]) -> (f64, f64, f64, f64) {
    let mut s = EditSession::new(Document::from_bytes(page(content)).unwrap());
    let b = s.page_objects(0).expect("decomposes").page_bbox();
    (b.min.x, b.min.y, b.max.x, b.max.y)
}

#[test]
fn a_clip_path_is_not_part_of_the_drawn_extent() {
    assert_eq!(
        drawn_extent(b"0 0 100 100 re W n 10 10 20 20 re S"),
        (10.0, 10.0, 30.0, 30.0)
    );
}

#[test]
fn a_page_whose_only_path_is_a_clip_has_an_empty_drawn_extent() {
    let (x0, _, x1, _) = drawn_extent(b"0 0 100 100 re W n");
    assert!(x0 > x1, "expected empty bounds, got {x0}..{x1}");
}

#[test]
fn a_clip_past_the_page_edge_is_not_reported_off_page() {
    let doc = Document::from_bytes(page(b"-50 -50 200 200 re W n 10 10 20 20 re f")).unwrap();
    let (scans, _) = scan_document(&doc, 0.25).expect("page tree walks");
    assert!(scans[0].objects.is_empty(), "{:?}", scans[0].objects);
}

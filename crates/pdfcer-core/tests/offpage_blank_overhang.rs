//! `scan-offpage` stops counting an image whose part past the page edge is
//! blank: what `redact-offpage` leaves a partially off-page image as, and
//! what a scanned sheet with a white margin a hair past the edge already is.
//!
//! Each fixture is a 100 x 100 pt page with a 4 x 1 grey image placed at
//! x 80..120, 10 pt per sample: samples 0 and 1 are on the page, 2 and 3 off.

use pdfcer_core::document::Document;
use pdfcer_core::offpage::{OffPage, scan_document};

/// A page drawing one 4 x 1 `DeviceGray` image across the right edge.
/// `smask` adds a 4 x 1 soft mask with those alpha samples.
fn page_with_image(samples: [u8; 4], smask: Option<[u8; 4]>) -> Vec<u8> {
    let content = b"q 40 0 0 10 80 50 cm /Im0 Do Q";
    let sm_entry = if smask.is_some() { " /SMask 6 0 R" } else { "" };
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
          /Resources << /XObject << /Im0 5 0 R >> >> >>"
            .to_vec(),
        stream("", content),
        stream(
            &format!(
                "/Type /XObject /Subtype /Image /Width 4 /Height 1 \
                 /ColorSpace /DeviceGray /BitsPerComponent 8{sm_entry}"
            ),
            &samples,
        ),
    ];
    if let Some(alpha) = smask {
        objects.push(stream(
            "/Type /XObject /Subtype /Image /Width 4 /Height 1 \
             /ColorSpace /DeviceGray /BitsPerComponent 8",
            &alpha,
        ));
    }
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

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut s = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    s.extend_from_slice(data);
    s.extend_from_slice(b"\nendstream");
    s
}

/// `(partial images reported, blank overhangs counted)` for the one page.
fn scan(bytes: Vec<u8>) -> (usize, usize) {
    let doc = Document::from_bytes(bytes).expect("synthetic page loads");
    let (scans, unreadable) = scan_document(&doc, 0.25).expect("page tree walks");
    assert!(unreadable.is_empty());
    let page = &scans[0];
    let partial = page
        .objects
        .iter()
        .filter(|o| o.kind == "image" && o.how == OffPage::Partial)
        .count();
    (partial, page.inkless_overhang)
}

#[test]
fn a_white_overhang_is_counted_as_blank_not_off_page() {
    assert_eq!(scan(page_with_image([0, 0, 255, 255], None)), (0, 1));
}

#[test]
fn ink_in_one_off_page_sample_is_still_reported() {
    assert_eq!(scan(page_with_image([0, 0, 255, 0], None)), (1, 0));
}

#[test]
fn ink_on_the_page_side_does_not_count_against_the_overhang() {
    // Only the off-page samples are asked about.
    assert_eq!(scan(page_with_image([0, 0, 255, 255], None)).1, 1);
    assert_eq!(scan(page_with_image([0, 90, 255, 255], None)), (0, 1));
}

#[test]
fn ink_under_a_transparent_soft_mask_is_blank() {
    let hidden = page_with_image([0, 0, 0, 0], Some([255, 255, 0, 0]));
    assert_eq!(scan(hidden), (0, 1));
    let shown = page_with_image([0, 0, 0, 0], Some([255, 255, 0, 1]));
    assert_eq!(scan(shown), (1, 0));
}

//! Redacting off-page content with a black-fill mark leaves the page clean:
//! the overlay is trimmed to the page box, so it adds no new off-page
//! content for the next scan to find. `overlay_off_page` keeps the old
//! behaviour, which also shows the default is what makes the scan clean.

use pdfcer_core::annot_author::{Color, Quad, RedactSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::offpage::{offpage_bands, scan_document};
use pdfcer_core::redact::{RedactOptions, apply_redactions_with};
use pdfcer_core::vartext::Quadding;
use pdfcer_core::writer::SaveOptions;

const TOL: f64 = 0.25;

/// A 100 x 100 pt page: one square on the page, one wholly off it, and one
/// bar overhanging the right edge.
fn page() -> Vec<u8> {
    let content = b"0 g 10 10 20 20 re f 150 40 20 20 re f 80 70 40 10 re f";
    let objects: [Vec<u8>; 4] = [
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

/// The page with every off-page band marked as one black-fill redaction,
/// the way a shell marks them with the operator's usual mark style.
fn marked() -> Document {
    let doc = Document::from_bytes(page()).expect("synthetic page loads");
    let (scans, unreadable) = scan_document(&doc, TOL).expect("page tree walks");
    assert!(unreadable.is_empty());
    assert!(!scans[0].is_clean(), "the fixture draws off the page");
    let quads = offpage_bands(&scans[0], TOL)
        .into_iter()
        .map(Quad::from_rect)
        .collect();
    let mut session = EditSession::new(doc);
    let spec = RedactSpec {
        quads,
        fill: Some(Color::Gray(0.0)),
        overlay_text: None,
        quadding: Quadding::Left,
    };
    session.add_redaction(0, &spec).expect("mark authored");
    let bytes = session.to_full_bytes(&SaveOptions::default()).unwrap().0;
    Document::from_bytes(bytes).unwrap()
}

fn rescan_is_clean(bytes: Vec<u8>) -> bool {
    let doc = Document::from_bytes(bytes).unwrap();
    scan_document(&doc, TOL).unwrap().0[0].is_clean()
}

#[test]
fn a_black_fill_offpage_redaction_leaves_nothing_off_the_page() {
    let (bytes, report) = apply_redactions_with(
        &marked(),
        &SaveOptions::identity(),
        &RedactOptions::default(),
    )
    .unwrap();
    assert!(report.overlay_off_page_skipped + report.overlay_trimmed_to_page > 0);
    assert!(rescan_is_clean(bytes));
}

#[test]
fn the_session_apply_also_leaves_nothing_off_the_page() {
    let mut session = EditSession::new(marked());
    session.apply_redactions().unwrap();
    let bytes = session.to_full_bytes(&SaveOptions::default()).unwrap().0;
    assert!(rescan_is_clean(bytes));
}

#[test]
fn overlay_off_page_draws_the_boxes_where_the_regions_are() {
    let opts = RedactOptions::default().with_overlay_off_page(true);
    let (bytes, report) =
        apply_redactions_with(&marked(), &SaveOptions::identity(), &opts).unwrap();
    assert_eq!(report.overlay_off_page_skipped, 0);
    assert!(!rescan_is_clean(bytes));
}

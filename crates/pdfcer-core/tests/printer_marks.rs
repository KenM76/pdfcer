//! `EditSession::add_printer_marks`: each marked page grows past its trim box,
//! keeps its trim, bleed and art boxes and its own content, and gains marks
//! as a `/Page` artifact outside the trim. Fixtures are built inline (rule 7).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::page_tree::{self, BoxResolution, Rect};
use pdfcer_core::printer_marks::{PrinterMarks, PrinterMarksError};
use pdfcer_core::text_extract::{self, ArtifactKind, ExtractOptions};
use pdfcer_core::writer::SaveOptions;

const HELLO: &str = "BT /F1 12 Tf 20 100 Td (Hello) Tj ET";

/// Two 300 x 200 pages drawing `Hello`: page 0 with a trim box inset 20 and
/// a bleed box inset 11, page 1 with neither.
fn two_pages() -> Vec<u8> {
    let stream = format!("<< /Length {} >>\nstream\n{HELLO}\nendstream", HELLO.len());
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 200] /Resources \
         << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>"
            .to_owned(),
        "<< /Type /Page /Parent 2 0 R /Contents 5 0 R /TrimBox [20 20 280 180] \
         /BleedBox [11 11 289 189] >>"
            .to_owned(),
        "<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".to_owned(),
        stream,
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

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(two_pages()).expect("fixture loads"))
}

fn saved(s: &EditSession) -> Document {
    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;
    Document::from_bytes(bytes).expect("reload")
}

fn rect(llx: f64, lly: f64, urx: f64, ury: f64) -> Rect {
    Rect { llx, lly, urx, ury }
}

#[test]
fn pages_grow_past_the_trim_and_keep_their_boxes_and_content() {
    let mut s = session();
    let out = s
        .add_printer_marks(&PrinterMarks::all("job"))
        .expect("marks");
    assert_eq!(out.pages, vec![0, 1]);
    assert_eq!(out.trim_assumed, vec![1]);
    // Page 0: the 9 pt bleed widens the 6 pt offset; band = 9 + 18.
    assert_eq!(out.grown_by, 27.0);
    assert_eq!(s.undo_kind(), Some(CommandKind::AddPrinterMarks));

    let doc = saved(&s);
    let pages = page_tree::pages(&doc).expect("walks");
    let p0 = &pages[0];
    assert_eq!(p0.media_box, rect(-7.0, -7.0, 307.0, 207.0));
    assert_eq!(p0.crop_box, p0.media_box);
    assert_eq!(p0.trim_box, rect(20.0, 20.0, 280.0, 180.0));
    assert_eq!(p0.bleed_box, rect(11.0, 11.0, 289.0, 189.0));
    assert_eq!(p0.trim_box_resolution, BoxResolution::AsWritten);
    // Page 1's crop box was its trim; it stays the trim.
    assert_eq!(pages[1].trim_box, rect(0.0, 0.0, 300.0, 200.0));
    assert_eq!(pages[1].media_box, rect(-24.0, -24.0, 324.0, 224.0));

    for (i, page) in pages.iter().enumerate() {
        let runs = text_extract::extract_page(&doc, page, i, &ExtractOptions::default())
            .expect("extract")
            .runs;
        assert!(
            runs.iter().any(|r| r.text == "Hello"),
            "page {i} keeps Hello"
        );
        let info = runs
            .iter()
            .find(|r| r.text.starts_with("job"))
            .expect("info line");
        assert_eq!(info.text, format!("job   page {} of 2", i + 1)); // string-gap-exempt: the deliberate page-info separator
        assert_eq!(info.artifact, Some(ArtifactKind::Page));
        let g = info.glyphs.first().expect("glyphs");
        assert!(
            f64::from(g.y) < page.trim_box.lly,
            "info sits below the trim"
        );
    }
}

#[test]
fn the_shared_content_stream_is_not_rewritten() {
    let mut s = session();
    s.add_printer_marks(&PrinterMarks::all("")).expect("marks");
    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;
    let base = two_pages();
    assert_eq!(&bytes[..base.len()], &base[..]);
    let tail = String::from_utf8_lossy(&bytes[base.len()..]);
    assert!(
        !tail.contains("\n5 0 obj"),
        "the page content stream stays put"
    );
}

#[test]
fn undo_restores_the_page() {
    let mut s = session();
    let mut m = PrinterMarks::none("");
    m.trim = true;
    m.pages = Some(vec![0]);
    s.add_printer_marks(&m).expect("marks");
    assert_eq!(s.undo(), Some(CommandKind::AddPrinterMarks));
    let pages = s.pages().expect("pages");
    assert_eq!(pages[0].media_box, rect(0.0, 0.0, 300.0, 200.0));
}

#[test]
fn a_bad_request_is_refused_before_anything_is_written() {
    let mut s = session();
    assert!(matches!(
        s.add_printer_marks(&PrinterMarks::none("")),
        Err(EditError::PrinterMarks(PrinterMarksError::NoMarks))
    ));
    let mut m = PrinterMarks::all("");
    m.pages = Some(vec![2]);
    assert!(matches!(
        s.add_printer_marks(&m),
        Err(EditError::PageOutOfRange { index: 2, count: 2 })
    ));
    m.pages = Some(Vec::new());
    assert!(matches!(
        s.add_printer_marks(&m),
        Err(EditError::PrinterMarks(PrinterMarksError::NoPages))
    ));
    assert_eq!(s.undo_kind(), None);
}

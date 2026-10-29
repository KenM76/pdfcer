//! `EditSession::stamp_running_text`: a header or footer lands on each page as
//! a `/Pagination /Header` (or `/Footer`) artifact with its tokens filled,
//! the page's own content untouched. Fixtures are built inline (rule 7).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::page_tree;
use pdfcer_core::running_text::{RunningText, RunningTextAlign, RunningTextError, RunningTextKind};
use pdfcer_core::text_extract::{self, ArtifactKind, ArtifactSubtype, ExtractOptions, TextRun};
use pdfcer_core::writer::SaveOptions;

/// Three 300 x 200 pages, each drawing `Hello` in the inherited font `F1`:
/// page 0 plain, page 1 leaving `2 0 0 2 0 0 cm` in effect (no `q`/`Q`),
/// page 2 with `/Rotate 90`. `/Resources` lives on the `/Pages` node only.
fn three_pages() -> Vec<u8> {
    let plain = "BT /F1 12 Tf 20 100 Td (Hello) Tj ET";
    let scaled = "2 0 0 2 0 0 cm BT /F1 12 Tf 10 50 Td (Hello) Tj ET";
    let page = |contents: u32, extra: &str| {
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents {contents} 0 R{extra} >>"
        )
    };
    let stream = |s: &str| format!("<< /Length {} >>\nstream\n{s}\nendstream", s.len());
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /Resources << /Font << /F1 \
         << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>"
            .to_owned(),
        page(6, ""),
        page(7, ""),
        page(6, " /Rotate 90"),
        stream(plain),
        stream(scaled),
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
    EditSession::new(Document::from_bytes(three_pages()).expect("fixture loads"))
}

/// Every page's extracted runs, after an incremental save and reload.
fn saved_runs(s: &EditSession) -> Vec<Vec<TextRun>> {
    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;
    let doc = Document::from_bytes(bytes).expect("reload");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    pages
        .iter()
        .enumerate()
        .map(|(i, p)| {
            text_extract::extract_page(&doc, p, i, &ExtractOptions::default())
                .expect("extract")
                .runs
        })
        .collect()
}

fn find<'a>(runs: &'a [TextRun], prefix: &str) -> &'a TextRun {
    runs.iter()
        .find(|r| r.text.starts_with(prefix))
        .expect("a stamped run")
}

#[test]
fn a_footer_fills_its_tokens_per_page_as_a_footer_artifact() {
    let mut s = session();
    let footer = RunningText::new(RunningTextKind::Footer, "Page {page} of {pages}");
    let out = s.stamp_running_text(&footer).expect("stamps");
    assert_eq!(out.pages, vec![0, 1, 2]);
    assert_eq!(out.first_label, "Page 1 of 3");
    assert_eq!(s.undo_kind(), Some(CommandKind::StampRunningText));
    let runs = saved_runs(&s);
    for (i, page) in runs.iter().enumerate() {
        let run = find(page, "Page ");
        assert_eq!(run.text, format!("Page {} of 3", i + 1));
        assert_eq!(run.artifact, Some(ArtifactKind::Pagination));
        assert_eq!(run.artifact_subtype, Some(ArtifactSubtype::Footer));
        assert!(
            page.iter().any(|r| r.text == "Hello"),
            "page {i} keeps its text"
        );
    }
}

#[test]
fn a_left_header_sits_at_the_top_left_of_the_displayed_page() {
    let mut s = session();
    let mut header = RunningText::new(RunningTextKind::Header, "DRAFT");
    header.align = RunningTextAlign::Left;
    header.pages = Some(vec![0]);
    s.stamp_running_text(&header).expect("stamps");
    let runs = saved_runs(&s);
    let run = find(&runs[0], "DRAFT");
    assert_eq!(run.artifact_subtype, Some(ArtifactSubtype::Header));
    let g = run.glyphs.first().expect("glyphs");
    // Margin 36 from the left; baseline = 200 - 36 - ascender (718/1000 of 10).
    assert!(
        (g.x - 36.0).abs() < 0.01 && (g.y - 156.82).abs() < 0.01,
        "({}, {})",
        g.x,
        g.y
    );
    assert!(!runs[1].iter().any(|r| r.text.starts_with("DRAFT")));
}

#[test]
fn a_bad_request_is_refused_before_anything_is_written() {
    let mut s = session();
    let mut t = RunningText::new(RunningTextKind::Footer, "x");
    t.pages = Some(vec![9]);
    assert!(matches!(
        s.stamp_running_text(&t),
        Err(EditError::PageOutOfRange { index: 9, count: 3 })
    ));
    t.pages = Some(Vec::new());
    assert!(matches!(
        s.stamp_running_text(&t),
        Err(EditError::RunningText(RunningTextError::NoPages))
    ));
    assert_eq!(s.undo_kind(), None);
}

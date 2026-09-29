//! `EditSession::stamp_bates`: labels land in order, upright on the displayed
//! page, as `/Pagination /Bates` artifacts, unmoved by a transformation the
//! page leaves in effect, and without hiding inherited resources.
//!
//! Every assertion reads the SAVED-AND-RELOADED bytes through text
//! extraction, so a label is checked where a reader would see it. Fixtures are
//! built inline (project rule 7).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::bates::{BatesError, BatesNumbering, BatesPosition, BatesStamp};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::page_tree;
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

fn stamp() -> BatesStamp {
    BatesStamp::new(BatesNumbering::new("ACME-", 4, ""))
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

/// The label run on a page, and where its last glyph ends.
fn label_end(runs: &[TextRun]) -> (String, f32, f32) {
    let run = runs
        .iter()
        .find(|r| r.text.starts_with("ACME-"))
        .expect("a label run");
    assert_eq!(run.artifact, Some(ArtifactKind::Pagination));
    assert_eq!(
        run.artifact_subtype,
        Some(ArtifactSubtype::Other("Bates".to_owned()))
    );
    let g = run.glyphs.last().expect("glyphs");
    let (dx, dy) = g.direction;
    (run.text.clone(), g.x + dx * g.advance, g.y + dy * g.advance)
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn labels_are_numbered_in_order_and_sit_at_the_displayed_bottom_right() {
    let mut s = session();
    let out = s.stamp_bates(&stamp(), 7).expect("stamps");
    assert_eq!(out.pages, vec![0, 1, 2]);
    assert_eq!((out.first, out.next), (7, 10));
    assert_eq!(out.first_label, "ACME-0007");
    assert_eq!(out.last_label, "ACME-0009");
    assert_eq!(s.undo_kind(), Some(CommandKind::StampBates));

    let runs = saved_runs(&s);
    // Baseline = margin + Helvetica's descender (207/1000 of 10 pt).
    let (text, x, y) = label_end(&runs[0]);
    assert_eq!(text, "ACME-0007");
    assert!(near(x, 264.0) && near(y, 38.07), "page 0 end ({x}, {y})");

    // The page's leftover 2x `cm` must not scale or move the label.
    let (text, x, y) = label_end(&runs[1]);
    assert_eq!(text, "ACME-0008");
    assert!(near(x, 264.0) && near(y, 38.07), "page 1 end ({x}, {y})");

    // /Rotate 90: the displayed page is 200 wide; its bottom edge is user x = 300.
    let (text, x, y) = label_end(&runs[2]);
    assert_eq!(text, "ACME-0009");
    assert!(near(x, 261.93) && near(y, 164.0), "page 2 end ({x}, {y})");
}

#[test]
fn the_page_keeps_its_own_text_and_inherited_font() {
    let mut s = session();
    s.stamp_bates(&stamp(), 1).expect("stamps");
    for (i, runs) in saved_runs(&s).iter().enumerate() {
        let hello = runs
            .iter()
            .find(|r| r.text == "Hello")
            .unwrap_or_else(|| panic!("page {i} lost its text"));
        assert_eq!(hello.artifact, None);
    }
}

#[test]
fn a_selection_is_numbered_in_document_order() {
    let mut s = session();
    let mut req = stamp();
    req.pages = Some(vec![2, 0, 2]);
    req.position = BatesPosition::TopLeft;
    let out = s.stamp_bates(&req, 1).expect("stamps");
    assert_eq!(out.pages, vec![0, 2]);
    let runs = saved_runs(&s);
    assert_eq!(label_end(&runs[0]).0, "ACME-0001");
    assert!(runs[1].iter().all(|r| !r.text.starts_with("ACME-")));
    assert_eq!(label_end(&runs[2]).0, "ACME-0002");
    // Top left: the first glyph starts at the margin; the ascender (718) ends
    // at the top margin.
    let first = &runs[0]
        .iter()
        .find(|r| r.text.starts_with("ACME-"))
        .unwrap()
        .glyphs[0];
    assert!(near(first.x, 36.0) && near(first.y, 200.0 - 36.0 - 7.18));
}

#[test]
fn refusals_happen_before_anything_is_written() {
    let mut s = session();
    let overflow = s
        .stamp_bates(&BatesStamp::new(BatesNumbering::new("", 2, "")), 98)
        .unwrap_err();
    assert!(matches!(
        overflow,
        EditError::Bates(BatesError::Overflow {
            number: 100,
            digits: 2
        })
    ));

    let mut far = stamp();
    far.pages = Some(vec![3]);
    assert!(matches!(
        s.stamp_bates(&far, 1).unwrap_err(),
        EditError::PageOutOfRange { index: 3, count: 3 }
    ));

    let mut negative = stamp();
    negative.margin = -1.0;
    assert!(matches!(
        s.stamp_bates(&negative, 1).unwrap_err(),
        EditError::Bates(BatesError::Geometry { .. })
    ));

    let mut none = stamp();
    none.pages = Some(Vec::new());
    assert!(matches!(
        s.stamp_bates(&none, 1).unwrap_err(),
        EditError::Bates(BatesError::NoPages)
    ));
    assert_eq!(s.undo_kind(), None, "a refusal committed something");
}

#[test]
fn undo_removes_every_label() {
    let mut s = session();
    s.stamp_bates(&stamp(), 1).expect("stamps");
    assert_eq!(s.undo(), Some(CommandKind::StampBates));
    for runs in saved_runs(&s) {
        assert!(runs.iter().all(|r| !r.text.starts_with("ACME-")));
    }
}

/// Every page after an incremental save and reload.
fn saved_pages(s: &EditSession) -> Vec<page_tree::Page> {
    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;
    page_tree::pages(&Document::from_bytes(bytes).expect("reload")).expect("page tree walks")
}

fn has_bates_font(page: &page_tree::Page) -> bool {
    page.resources
        .get(b"Font")
        .and_then(|f| f.as_dict())
        .is_some_and(|f| f.iter().any(|(k, _)| k.as_bytes().starts_with(b"Bates")))
}

#[test]
fn removal_restores_each_page_to_its_own_content() {
    let mut s = session();
    s.stamp_bates(&stamp(), 1).expect("stamps");
    let out = s.remove_bates(None).expect("removes");
    assert_eq!(out.pages, vec![0, 1, 2]);
    assert_eq!(out.labels, ["ACME-0001", "ACME-0002", "ACME-0003"]);
    assert_eq!(s.undo_kind(), Some(CommandKind::RemoveBates));
    for (i, runs) in saved_runs(&s).iter().enumerate() {
        assert!(
            runs.iter().all(|r| !r.text.starts_with("ACME-")),
            "page {i}"
        );
        assert!(
            runs.iter().any(|r| r.text == "Hello"),
            "page {i} lost its text"
        );
    }
    for (i, page) in saved_pages(&s).iter().enumerate() {
        assert_eq!(page.contents.len(), 1, "page {i} kept a q or label stream");
        assert!(!has_bates_font(page), "page {i} kept the label font");
    }
}

#[test]
fn a_twice_stamped_page_loses_both_sets() {
    let mut s = session();
    s.stamp_bates(&stamp(), 1).expect("stamps");
    s.stamp_bates(&stamp(), 50).expect("stamps again");
    let out = s.remove_bates(Some(&[1])).expect("removes");
    assert_eq!(out.pages, vec![1]);
    assert_eq!(out.labels, ["ACME-0002", "ACME-0051"]);
    let pages = saved_pages(&s);
    assert_eq!(pages[1].contents.len(), 1);
    assert!(!has_bates_font(&pages[1]));
    // Unselected pages keep both sets: two q streams, content, two labels.
    assert_eq!(pages[0].contents.len(), 5);
    let runs = saved_runs(&s);
    assert!(runs[1].iter().all(|r| !r.text.starts_with("ACME-")));
    assert_eq!(
        runs[0]
            .iter()
            .filter(|r| r.text.starts_with("ACME-"))
            .count(),
        2
    );
}

#[test]
fn nothing_to_remove_commits_nothing() {
    let mut s = session();
    assert_eq!(s.remove_bates(None).expect("ok"), Default::default());
    assert_eq!(s.undo_kind(), None);
    assert!(matches!(
        s.remove_bates(Some(&[3])).unwrap_err(),
        EditError::PageOutOfRange { index: 3, count: 3 }
    ));
}

#[test]
fn undoing_a_removal_brings_the_labels_back() {
    let mut s = session();
    s.stamp_bates(&stamp(), 1).expect("stamps");
    s.remove_bates(None).expect("removes");
    assert_eq!(s.undo(), Some(CommandKind::RemoveBates));
    assert_eq!(label_end(&saved_runs(&s)[2]).0, "ACME-0003");
}

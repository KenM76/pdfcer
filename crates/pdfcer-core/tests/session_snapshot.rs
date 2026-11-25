//! `EditSession::snapshot`: a snapshot sees the edits made before it and
//! none made to the original after it. Fixtures are built inline (rule 7).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::running_text::{RunningText, RunningTextKind};
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::writer::SaveOptions;

const HELLO: &str = "BT /F1 12 Tf 20 100 Td (Hello) Tj ET";

/// One 300 x 200 page drawing `Hello` in Helvetica.
fn one_page() -> Vec<u8> {
    let stream = format!("<< /Length {} >>\nstream\n{HELLO}\nendstream", HELLO.len());
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 200] /Resources \
         << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>"
            .to_owned(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_owned(),
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
    EditSession::new(Document::from_bytes(one_page()).expect("fixture loads"))
}

/// The text of every run on page 0 of `s` as saved.
fn texts(s: &EditSession) -> Vec<String> {
    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;
    let doc = Document::from_bytes(bytes).expect("reload");
    let pages = page_tree::pages(&doc).expect("pages");
    text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default())
        .expect("extract")
        .runs
        .into_iter()
        .map(|r| r.text)
        .collect()
}

fn stamp(s: &mut EditSession, text: &str) {
    s.stamp_running_text(&RunningText::new(RunningTextKind::Header, text))
        .expect("stamp");
}

#[test]
fn a_snapshot_sees_unsaved_edits_and_not_later_ones() {
    let mut s = session();
    stamp(&mut s, "Before");
    let snap = s.snapshot();
    stamp(&mut s, "After");

    let seen = texts(&snap);
    assert!(seen.iter().any(|t| t == "Before"), "{seen:?}");
    assert!(!seen.iter().any(|t| t == "After"), "{seen:?}");
    assert!(!snap.can_undo(), "a snapshot starts with an empty history");

    let original = texts(&s);
    assert!(original.iter().any(|t| t == "After"), "{original:?}");
    assert_eq!(s.undo_depth(), 2, "the original's history is untouched");
}

#[test]
fn edits_to_a_snapshot_do_not_reach_the_original() {
    let mut s = session();
    let mut snap = s.snapshot();
    stamp(&mut snap, "Fork");
    assert!(!texts(&s).iter().any(|t| t == "Fork"));
    assert!(texts(&snap).iter().any(|t| t == "Fork"));
    stamp(&mut s, "Mine");
    assert!(!texts(&snap).iter().any(|t| t == "Mine"));
}

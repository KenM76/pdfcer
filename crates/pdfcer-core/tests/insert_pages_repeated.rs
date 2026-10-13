//! `EditSession::insert_pages` with a page named more than once
//! (`Pass 567.0`, pdfcer-gui G185): each repeat is its own complete copy,
//! as Acrobat inserts it. One copy listed twice in `/Kids` is a cycle to
//! the page-tree walk.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::pageops::InsertPosition;
use std::collections::BTreeSet;

/// `count` pages, each with a content stream naming `marker` and its index.
fn doc_with_pages(marker: &str, count: usize) -> Document {
    let mut objects: Vec<(usize, String)> = Vec::new();
    let kids: Vec<String> = (0..count).map(|i| format!("{} 0 R", 3 + i * 2)).collect();
    objects.push((1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()));
    objects.push((
        2,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {count} /MediaBox [0 0 200 200] >>",
            kids.join(" ")
        ),
    ));
    for i in 0..count {
        let page = 3 + i * 2;
        objects.push((
            page,
            format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R >>", page + 1),
        ));
        let payload = format!("% {marker}-{i}\n");
        objects.push((
            page + 1,
            format!(
                "<< /Length {} >>\nstream\n{payload}endstream",
                payload.len()
            ),
        ));
    }
    let mut out = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (num, body) in &objects {
        offsets.push(out.len());
        out.push_str(&format!("{num} 0 obj\n{body}\nendobj\n"));
    }
    let xref_at = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for off in offsets {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
        objects.len() + 1
    ));
    Document::from_bytes(out.into_bytes()).expect("fixture must load")
}

#[test]
fn a_repeated_page_is_inserted_as_separate_copies() {
    let src = doc_with_pages("src", 2);
    let mut session = EditSession::new(doc_with_pages("dst", 4));
    session
        .insert_pages(&src.view(), &[0, 0, 0], InsertPosition::End)
        .expect("insert");

    let slots = session.page_slots().expect("the page tree walks");
    assert_eq!(slots.len(), 7);
    let ids: BTreeSet<_> = slots.iter().map(|s| s.id).collect();
    assert_eq!(ids.len(), 7, "every page is its own object");

    // Each copy owns its content stream too: editing one copy cannot
    // change another.
    let graph = session.graph();
    let contents: BTreeSet<_> = slots[4..]
        .iter()
        .map(|s| {
            graph
                .resolved(s.id)
                .as_dict()
                .and_then(|d| d.get(b"Contents"))
                .and_then(pdfcer_core::object::Object::as_reference)
                .expect("a content reference")
        })
        .collect();
    assert_eq!(contents.len(), 3, "three content streams, not one shared");
}

#[test]
fn repeated_copies_save_reload_and_undo() {
    let src = doc_with_pages("src", 2);
    let mut session = EditSession::new(doc_with_pages("dst", 1));
    session
        .insert_pages(&src.view(), &[1, 0, 1], InsertPosition::End)
        .expect("insert");

    let (bytes, _) = session
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::default())
        .expect("save");
    let reloaded = EditSession::new(Document::from_bytes(bytes).expect("reload"));
    assert_eq!(reloaded.page_slots().expect("walks").len(), 4);

    assert!(session.undo().is_some());
    assert_eq!(session.page_slots().expect("walks").len(), 1);
}

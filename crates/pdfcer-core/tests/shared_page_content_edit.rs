//! Editing text on a page whose content stream another page also draws must
//! change that page only. The edited page gets its own copy; the other page
//! renders byte-identically, and the report says so.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::{
    EditOptions, EditRequest, FontSelector, FormatOptions, FormatRequest, edit_text, set_format,
};
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::writer::SaveOptions;

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

fn stream(content: &str) -> String {
    format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// Page 1 (object 3) and page 2 (object 7) with the given `/Contents`.
/// Object 4 draws "BODY TEXT", object 6 draws "HEADER".
fn two_pages(page1: &str, page2: &str) -> Vec<u8> {
    let page = |contents: &str| {
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] \
             /Resources << /Font << /F1 5 0 R >> >> /Contents {contents} >>"
        )
    };
    assemble(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 7 0 R] /Count 2 >>".to_owned(),
        page(page1),
        stream("BT /F1 24 Tf 20 100 Td (BODY TEXT) Tj ET"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        stream("BT /F1 12 Tf 20 20 Td (HEADER) Tj ET"),
        page(page2),
    ])
}

fn page_text(bytes: &[u8], index: usize) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).expect("edited document reloads");
    let text =
        text_extract::extract_document(&doc, &ExtractOptions::default()).expect("extraction runs");
    text.pages[index]
        .runs
        .iter()
        .map(|r| r.text.as_str())
        .collect()
}

const SHARED: &str = "shared a content stream";

#[test]
fn editing_one_page_leaves_a_page_sharing_its_stream_unchanged() {
    let doc = Document::from_bytes(two_pages("4 0 R", "4 0 R")).expect("fixture loads");
    let out = edit_text(
        &doc,
        &EditRequest::find_replace(0, "BODY", "LEAF"),
        &EditOptions::default(),
    )
    .expect("the edit succeeds");
    assert!(page_text(&out.bytes, 0).contains("LEAF"));
    assert!(page_text(&out.bytes, 1).contains("BODY TEXT"));
    assert!(
        out.report.disclosures.iter().any(|d| d.contains(SHARED)),
        "{:?}",
        out.report.disclosures
    );
    assert_eq!(
        out.report.content_object,
        page_contents(&out.bytes, 0),
        "the report names the stream the edit went into, not the shared one"
    );
    assert_ne!(out.report.content_object, 4);
}

/// The single `/Contents` reference of page `index` in `bytes`.
fn page_contents(bytes: &[u8], index: usize) -> u32 {
    let doc = Document::from_bytes(bytes.to_vec()).expect("reloads");
    let pages = pdfcer_core::page_tree::pages(&doc).expect("page tree walks");
    assert_eq!(
        pages[index].contents.len(),
        1,
        "{:?}",
        pages[index].contents
    );
    pages[index].contents[0].num
}

#[test]
fn a_shared_trailing_stream_is_not_emptied_under_the_other_page() {
    let doc = Document::from_bytes(two_pages("[4 0 R 6 0 R]", "6 0 R")).expect("fixture loads");
    let out = edit_text(
        &doc,
        &EditRequest::find_replace(0, "BODY", "LEAF"),
        &EditOptions::default(),
    )
    .expect("the edit succeeds");
    let p1 = page_text(&out.bytes, 0);
    assert!(
        p1.contains("LEAF") && p1.matches("HEADER").count() == 1,
        "{p1:?}"
    );
    assert!(page_text(&out.bytes, 1).contains("HEADER"));
    // Object 4 is page 1's alone, so it is reused; 6 is shared, so nothing is
    // emptied.
    assert_eq!(out.report.content_object, 4);
    assert_eq!(out.report.extra_objects_emptied, 0);
}

#[test]
fn an_unshared_page_says_nothing_about_sharing() {
    let doc = Document::from_bytes(two_pages("4 0 R", "6 0 R")).expect("fixture loads");
    let out = edit_text(
        &doc,
        &EditRequest::find_replace(0, "BODY", "LEAF"),
        &EditOptions::default(),
    )
    .expect("the edit succeeds");
    assert_eq!(out.report.content_object, 4);
    assert!(!out.report.disclosures.iter().any(|d| d.contains(SHARED)));
}

#[test]
fn the_session_edit_decouples_and_undoes_cleanly() {
    for (p1, p2) in [("4 0 R", "4 0 R"), ("[4 0 R 6 0 R]", "6 0 R")] {
        let doc = Document::from_bytes(two_pages(p1, p2)).expect("fixture loads");
        let mut session = EditSession::new(doc);
        let report = session
            .edit_text(
                &EditRequest::find_replace(0, "BODY", "LEAF"),
                &EditOptions::default(),
            )
            .expect("the session edit succeeds");
        assert!(report.disclosures.iter().any(|d| d.contains(SHARED)));
        assert_eq!(session.undo_depth(), 1);
        let (bytes, _) = session
            .to_incremental_bytes(&SaveOptions::identity())
            .expect("the session saves");
        assert!(page_text(&bytes, 0).contains("LEAF"), "{p1}");
        assert_eq!(report.content_object, page_contents(&bytes, 0), "{p1}");
        assert_eq!(report.extra_objects_emptied, 0, "{p1}");
        let other = page_text(&bytes, 1);
        assert!(
            other.contains(if p2 == "4 0 R" { "BODY TEXT" } else { "HEADER" }),
            "{p1}: {other:?}"
        );

        session.undo().expect("there is a command to undo");
        let (_, report) = session
            .to_incremental_bytes(&SaveOptions::identity())
            .expect("the session saves");
        assert_eq!(report.objects_written, 0, "{p1}: {report:?}");
    }
}

/// A format that adds a font resource to the page dictionary AND repoints the
/// page's `/Contents` writes the page once, carrying both changes: the text
/// is set in Courier and the other page still draws the original.
#[test]
fn a_format_adding_a_font_keeps_both_page_changes() {
    let doc = Document::from_bytes(two_pages("4 0 R", "4 0 R")).expect("fixture loads");
    let req = FormatRequest::new(0, "BODY").font(FontSelector::new("Courier"));
    let one_shot = set_format(&doc, &req, &FormatOptions::default()).expect("format succeeds");
    let mut session = EditSession::new(doc);
    let session_report = session
        .format_text(&req, &FormatOptions::default())
        .expect("session format succeeds");
    let (session_bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("the session saves");
    assert_eq!(
        one_shot.report.content_object,
        page_contents(&one_shot.bytes, 0)
    );
    assert_eq!(
        session_report.content_object,
        page_contents(&session_bytes, 0)
    );
    for bytes in [&one_shot.bytes, &session_bytes] {
        let reloaded = Document::from_bytes(bytes.to_vec()).expect("reloads");
        let pages = text_extract::extract_document(&reloaded, &ExtractOptions::default())
            .expect("extraction runs")
            .pages;
        // Courier is monospaced at 600/1000 em; Helvetica's `B` is 667.
        let body = &pages[0].runs[0];
        assert!(
            body.glyphs[..4]
                .iter()
                .all(|g| (g.advance - 0.6 * g.size).abs() < 0.01),
            "{body:?}"
        );
        let pages = pdfcer_core::page_tree::pages(&reloaded).expect("page tree walks");
        let other = pdfcer_core::content::ContentStream::from_page(&reloaded.view(), &pages[1])
            .expect("page 2 parses")
            .buf;
        assert_eq!(
            String::from_utf8_lossy(&other).trim(),
            "BT /F1 24 Tf 20 100 Td (BODY TEXT) Tj ET"
        );
    }
}

#[test]
fn a_vector_delete_leaves_the_other_page_drawing_the_object() {
    let doc = Document::from_bytes(two_pages("4 0 R", "4 0 R")).expect("fixture loads");
    let mut session = EditSession::new(doc);
    let disclosures = session.delete_object(0, 0).expect("the delete succeeds");
    assert!(
        disclosures.iter().any(|d| d.contains(SHARED)),
        "{disclosures:?}"
    );
    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("the session saves");
    assert!(!page_text(&bytes, 0).contains("BODY"));
    assert!(page_text(&bytes, 1).contains("BODY TEXT"));
}

#[test]
fn a_reflow_reports_the_stream_it_wrote() {
    let doc = Document::from_bytes(two_pages("4 0 R", "4 0 R")).expect("fixture loads");
    let req = pdfcer_core::text_edit::ReflowRequest::new().with_wrap_width(60.0);
    let one_shot =
        pdfcer_core::text_edit::apply_reflow(&doc, 0, 0, &req).expect("the reflow succeeds");
    let mut session = EditSession::new(doc);
    let report = session
        .reflow_block(0, 0, &req)
        .expect("session reflow succeeds");
    let (session_bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("the session saves");
    for (bytes, reported, disclosures) in [
        (
            &one_shot.bytes,
            one_shot.report.content_object,
            &one_shot.report.disclosures,
        ),
        (&session_bytes, report.content_object, &report.disclosures),
    ] {
        assert!(
            disclosures.iter().any(|d| d.contains(SHARED)),
            "{disclosures:?}"
        );
        assert_eq!(reported, page_contents(bytes, 0));
        assert!(page_text(bytes, 1).contains("BODY TEXT"));
    }
}

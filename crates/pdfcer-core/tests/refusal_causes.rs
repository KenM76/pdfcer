//! Text-edit refusals as data: each cause matched by variant, never by
//! message text, so a shell can switch on it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::{
    EditError, EditOptions, EditRequest, NotFoundReason, UnsupportedCause,
};

use crate::flatten_annotations::assemble;

/// One Helvetica page whose content stream is `content`.
fn page(content: &str) -> EditSession {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    EditSession::new(Document::from_bytes(assemble(&bodies)).expect("fixture parses"))
}

fn not_found(s: &mut EditSession, find: &str) -> NotFoundReason {
    match s.edit_text(
        &EditRequest::find_replace(0, find, "x"),
        &EditOptions::default(),
    ) {
        Err(EditError::NoMatch { reason, .. }) => reason,
        other => panic!("expected NoMatch, got {other:?}"),
    }
}

/// A line whose trailing word is a separate text object — Word's shape —
/// is reported as spanning text objects, not as absent.
#[test]
fn text_split_across_text_objects_is_named_as_such() {
    let mut s = page("BT /F1 12 Tf 20 100 Td (Hello) Tj ET\nBT /F1 12 Tf 50 100 Td ( world) Tj ET");
    assert_eq!(
        not_found(&mut s, "Hello world"),
        NotFoundReason::SpansTextObjects { objects: 2 }
    );
}

/// Text that is not on the page at all stays `NoSuchText`.
#[test]
fn absent_text_is_no_such_text() {
    let mut s = page("BT /F1 12 Tf 20 100 Td (Hello) Tj ET\nBT /F1 12 Tf 50 100 Td ( world) Tj ET");
    assert_eq!(not_found(&mut s, "Goodbye"), NotFoundReason::NoSuchText);
}

/// `edit_capability` and `edit_text` agree on every text-edit fixture: a
/// run-level refusal from one is the same refusal from the other, and an
/// editable run edits (rewriting it with its own text) unless a single
/// character is refused, which is per-character, not run-level.
#[test]
fn edit_capability_agrees_with_edit_text_across_the_text_corpus() {
    use pdfcer_core::text_extract::{ExtractOptions, extract_page};

    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/text");
    let (mut checked, mut refused) = (0, 0);
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("pdf") {
            continue;
        }
        let Ok(doc) = Document::load(&path) else {
            continue;
        };
        let Ok(pages) = pdfcer_core::page_tree::pages(&doc) else {
            continue;
        };
        let Some(first) = pages.first() else { continue };
        let opts = ExtractOptions::default().with_provenance(true);
        let Ok(text) = extract_page(&doc, first, 0, &opts) else {
            continue;
        };
        // `edit_capability` addresses the page's own content; a form
        // XObject's span indexes a different buffer.
        let Some((span, run_text)) = text.runs.iter().find_map(|r| {
            r.glyphs.iter().find_map(|g| {
                g.provenance
                    .as_ref()
                    .filter(|p| p.content_stream.is_page())
                    .map(|p| (p.operator_span, r.text.clone()))
            })
        }) else {
            continue;
        };
        let mut s = EditSession::new(doc);
        let capability = s.edit_capability(0, span);
        let edit = s.edit_text(
            &EditRequest::whole_operator(0, span, &run_text),
            &EditOptions::default(),
        );
        match (&capability, &edit) {
            (Err(a), Err(b)) => {
                refused += 1;
                assert_eq!(
                    format!("{a:?}"),
                    format!("{b:?}"),
                    "{}: the two answers name different refusals",
                    path.display()
                );
            }
            (Err(a), Ok(_)) => panic!(
                "{}: capability refused ({a:?}) but the edit worked",
                path.display()
            ),
            (Ok(()), Err(EditError::Refused(_))) | (Ok(()), Ok(_)) => {}
            (Ok(()), Err(b)) => panic!(
                "{}: capability said yes, edit refused {b:?}",
                path.display()
            ),
        }
        checked += 1;
    }
    eprintln!("checked {checked}, refused {refused}");
    assert!(
        checked >= 10,
        "only {checked} fixtures reached the comparison"
    );
    assert!(
        refused >= 1 && refused < checked,
        "both outcomes must occur: {refused}/{checked}"
    );
}

fn unsupported(s: &mut EditSession, req: &EditRequest) -> UnsupportedCause {
    match s.edit_text(req, &EditOptions::default()) {
        Err(EditError::Unsupported(cause)) => cause,
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn an_empty_find_is_empty_find() {
    let mut s = page("BT /F1 12 Tf 20 100 Td (Hello) Tj ET");
    assert_eq!(
        unsupported(&mut s, &EditRequest::find_replace(0, "", "x")),
        UnsupportedCause::EmptyFind
    );
}

#[test]
fn a_quote_operator_run_is_quote_operator() {
    let mut s = page("BT /F1 12 Tf 14 TL 20 100 Td (Hello) ' ET");
    assert_eq!(
        unsupported(&mut s, &EditRequest::find_replace(0, "Hello", "Howdy")),
        UnsupportedCause::QuoteOperator
    );
}

#[test]
fn a_page_with_no_contents_is_no_contents() {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >>".to_owned(),
    ];
    let doc = Document::from_bytes(assemble(&bodies)).unwrap();
    // Only a request for the page stream by name is refused: with forms
    // still searchable, an absent stream is simply not a candidate.
    let mut req = EditRequest::find_replace(0, "Hello", "x");
    req.target = pdfcer_core::text_edit::EditTarget::PageContents;
    let err = pdfcer_core::text_edit::edit_text(&doc, &req, &EditOptions::default())
        .expect_err("nothing to edit");
    assert!(
        matches!(err, EditError::Unsupported(UnsupportedCause::NoContents)),
        "{err:?}"
    );
}

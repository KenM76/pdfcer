//! `EditSession::edit_text_preview`: what it lays out is what `edit_text`
//! commits, it refuses what the commit refuses, and it writes nothing.
//!
//! Parity is measured through the public API: preview `find → replace`,
//! commit it, then preview the committed text replaced by itself. The second
//! preview lays out the glyphs the commit actually wrote, so the two must
//! agree glyph for glyph.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::{EditError, EditOptions, EditRequest, EditTarget, TextEditPreview};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("fixture parses"))
}

fn preview(s: &mut EditSession, find: &str, replace: &str) -> Result<TextEditPreview, EditError> {
    s.edit_text_preview(
        &EditRequest::find_replace(0, find, replace),
        &EditOptions::default(),
    )
}

fn assert_same_layout(a: &TextEditPreview, b: &TextEditPreview) {
    assert_eq!(a.glyphs.len(), b.glyphs.len());
    for (x, y) in a.glyphs.iter().zip(&b.glyphs) {
        assert_eq!(x.code, y.code);
        assert_eq!(x.ch, y.ch);
        for (p, q) in x.matrix.iter().zip(&y.matrix) {
            assert!((p - q).abs() < 1e-3, "{:?} vs {:?}", x.matrix, y.matrix);
        }
    }
    for (p, q) in a.bbox.iter().zip(&b.bbox) {
        assert!((p - q).abs() < 1e-3, "{:?} vs {:?}", a.bbox, b.bbox);
    }
}

/// Preview, commit, and re-preview the committed text: same glyphs, same
/// places.
fn parity(rel: &str, find: &str, replace: &str) {
    let mut s = session(rel);
    let before = preview(&mut s, find, replace).expect("previewable");
    assert_eq!(before.glyphs.len(), replace.chars().count());
    assert!(!s.can_undo(), "a preview records no command");
    s.edit_text(
        &EditRequest::find_replace(0, find, replace),
        &EditOptions::default(),
    )
    .expect("the commit succeeds");
    let after = preview(&mut s, replace, replace).expect("the committed text is there");
    assert_same_layout(&before, &after);
}

#[test]
fn preview_matches_the_commit_inside_one_operator() {
    parity("textedit/nonembedded.pdf", "teh", "the");
    parity("textedit/embedded_full.pdf", "teh", "the");
}

#[test]
fn preview_matches_the_commit_when_the_run_changes_length() {
    parity("textedit/tm_follower.pdf", "Hello", "Hi");
}

#[test]
fn preview_refuses_what_the_commit_refuses() {
    let mut s = session("textedit/subset_missing.pdf");
    let p = preview(&mut s, "cat", "caz").expect_err("z is not in the subset");
    let c = s
        .edit_text(
            &EditRequest::find_replace(0, "cat", "caz"),
            &EditOptions::default(),
        )
        .expect_err("z is not in the subset");
    assert_eq!(p.to_string(), c.to_string());
}

#[test]
fn preview_of_missing_text_is_no_match() {
    let mut s = session("textedit/nonembedded.pdf");
    assert!(matches!(
        preview(&mut s, "no such words", "x"),
        Err(EditError::NoMatch { .. })
    ));
}

#[test]
fn a_preview_writes_nothing() {
    let mut s = session("textedit/nonembedded.pdf");
    for _ in 0..3 {
        preview(&mut s, "teh", "the").expect("previewable");
    }
    assert!(!s.can_undo());
    assert!(!s.is_modified(), "nothing staged");
}

#[test]
fn a_form_target_is_refused() {
    let mut s = session("textedit/nonembedded.pdf");
    let mut req = EditRequest::find_replace(0, "teh", "the");
    req.target = EditTarget::Form { object: 4 };
    assert!(matches!(
        s.edit_text_preview(&req, &EditOptions::default()),
        Err(EditError::Unsupported(_))
    ));
}

/// A run placed under `2 0 0 2 10 20 cm`: the glyph matrices and box are
/// page space, so the CTM is in them.
#[test]
fn the_ctm_is_in_the_layout() {
    let content = "q 2 0 0 2 10 20 cm BT /F1 10 Tf 5 7 Td (Hello) Tj ET Q";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] \
         /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let startxref = out.len();
    out.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(b"trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n");
    out.extend_from_slice(format!("{startxref}\n%%EOF\n").as_bytes());

    let mut s = EditSession::new(Document::from_bytes(out).expect("parses"));
    let p = preview(&mut s, "Hello", "Hello").expect("previewable");
    let m = p.glyphs[0].matrix;
    // Tfs 10 under a ×2 CTM; origin (5,7) in text space → (20,34) on the page.
    assert!(
        (m[0] - 20.0).abs() < 1e-9 && (m[3] - 20.0).abs() < 1e-9,
        "{m:?}"
    );
    assert!(
        (m[4] - 20.0).abs() < 1e-9 && (m[5] - 34.0).abs() < 1e-9,
        "{m:?}"
    );
    // Each following glyph moves right by its advance, doubled.
    assert!(p.glyphs[1].matrix[4] > m[4]);
    assert!(p.bbox[0] <= 20.0 + 1e-9 && p.bbox[2] > p.glyphs[4].matrix[4]);
    assert_eq!(p.base_font, "Helvetica");
    assert_eq!(p.font_resource, b"F1");
}

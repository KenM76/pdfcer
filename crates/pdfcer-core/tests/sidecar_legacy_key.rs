//! The ce-dimension sidecar survives the product rename (`Pass 247.1`).
//!
//! Every document saved with ce dimensions by a pre-release build
//! (v0.5.1–v0.27.0, code name `pdfce`) carries its sidecar under
//! `/PieceInfo << /pdfce ... >>`. From `Pass 247.1` the writer emits
//! `/pdfcer` (ISO 32000-1 §14.5: the `/PieceInfo` key is the application's
//! name). This file is the proof that the rename did not cost the operator
//! his measurements:
//!
//! 1. a document whose ONLY sidecar is the legacy `/pdfce` entry opens with
//!    its ce dimensions intact (the reader falls back to the legacy key);
//! 2. saving that document writes `/pdfcer` and RETIRES `/pdfce`, so one
//!    document never carries two sidecars that could disagree.
//!
//! (The reader prefers the current key when both are present; that branch
//! is documented at `EditSession::sidecar_entry` and is not manufactured
//! here — a document in that state is one this writer never produces.)
//!
//! The legacy bytes are manufactured from a fresh save by rewriting the key
//! in place at equal byte length (`/pdfcer ` → `/pdfce  `; a PDF name is
//! delimited by whitespace, so the extra space is syntactically inert), which
//! keeps every xref offset valid without a hand-built fixture.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{DEFAULT_GROUP_ID, DimensionKind};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::{AxisConstraint, Point};
use pdfcer_core::writer::SaveOptions;

fn minimal_pdf() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
    ];
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

fn linear() -> DimensionKind {
    DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(300.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
    }
}

fn save(session: &EditSession) -> Vec<u8> {
    session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0
}

/// A freshly saved document with one ce dimension, under the CURRENT key.
fn saved_with_one_dimension() -> Vec<u8> {
    let doc = Document::from_bytes(minimal_pdf()).unwrap();
    let mut s = EditSession::new(doc);
    s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    save(&s)
}

/// Rewrite the sidecar key in place, at equal length, so the bytes look
/// exactly like a pre-rename build wrote them.
fn with_legacy_key(bytes: &[u8]) -> Vec<u8> {
    let needle = b"/pdfcer ";
    let pos = bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("the fresh save carries /pdfcer");
    let mut out = bytes.to_vec();
    out[pos..pos + needle.len()].copy_from_slice(b"/pdfce  ");
    assert_eq!(out.len(), bytes.len(), "the rewrite must not move a byte");
    out
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

fn piece_info_keys(bytes: Vec<u8>) -> Vec<Vec<u8>> {
    let doc = Document::from_bytes(bytes).unwrap();
    let catalog = doc.catalog().unwrap();
    let piece = catalog.get(b"PieceInfo").unwrap().as_dict().unwrap();
    piece.iter().map(|(k, _)| k.as_bytes().to_vec()).collect()
}

#[test]
fn the_fresh_save_writes_the_current_key_only() {
    let bytes = saved_with_one_dimension();
    assert_eq!(count(&bytes, b"/pdfcer "), 1, "one sidecar entry");
    assert_eq!(count(&bytes, b"/pdfce "), 0, "no legacy entry");
    assert_eq!(piece_info_keys(bytes), vec![b"pdfcer".to_vec()]);
}

#[test]
fn a_pre_rename_sidecar_still_opens_with_its_dimensions() {
    let legacy = with_legacy_key(&saved_with_one_dimension());
    assert_eq!(
        count(&legacy, b"/pdfce "),
        1,
        "the fixture carries ONLY the legacy key"
    );
    assert_eq!(count(&legacy, b"/pdfcer"), 0);

    let doc = Document::from_bytes(legacy).unwrap();
    let s = EditSession::new(doc);
    assert_eq!(
        s.dimension_model().dimensions().len(),
        1,
        "the ce dimension written under the pre-release key is read back"
    );
}

#[test]
fn saving_a_pre_rename_document_retires_the_legacy_key() {
    let legacy = with_legacy_key(&saved_with_one_dimension());
    let doc = Document::from_bytes(legacy).unwrap();
    let mut s = EditSession::new(doc);
    // A second ce dimension forces the sidecar to be rewritten.
    s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    let out = save(&s);

    let keys = piece_info_keys(out.clone());
    assert!(
        keys.contains(&b"pdfcer".to_vec()),
        "the current key is written"
    );
    assert!(
        !keys.contains(&b"pdfce".to_vec()),
        "the legacy key is retired on write, not left beside the new one: {keys:?}"
    );

    let reopened = EditSession::new(Document::from_bytes(out).unwrap());
    assert_eq!(reopened.dimension_model().dimensions().len(), 2);
}

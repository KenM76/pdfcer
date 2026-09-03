//! # Deleting a "field" that is actually a page-tree node (`Pass 185.1`)
//!
//! **Found by `fuzz/fuzz_targets/form_edit_sequence.rs` within two minutes of
//! the target first existing**, on a shape no fixture had: an `/AcroForm`
//! whose `/Fields` names an object that is also part of the **page tree**.
//!
//! `parse_acroform` models such an object as a field — correctly, since the
//! form dictionary says it is one and §12.7.3 states no rule that a field may
//! not also be something else. `delete_field` then removes it, and the
//! document has no page tree.
//!
//! ## Why the severity is worse than the panic suggests
//!
//! The panic came from `debug_assert_page_tree_still_walks`, which is
//! `#[cfg(debug_assertions)]`. In a **release** build — the one operators run
//! — the guard is compiled out, `delete_field` returns `Ok`, and the save
//! produces a file pdfcer itself cannot reopen. That is the exact shape the
//! guard's own message names: *"a verb that returns Ok and produces a document
//! pdfcer cannot reopen"*.
//!
//! ## What a fixture could not have told us
//!
//! Every form fixture in this repository is well-formed, because they were all
//! written by someone who knew what a form is. The class of input where a
//! FIELD and a PAGE are the same object only arrives from a file somebody else
//! produced — or a fuzzer. `form_model` fuzzes the read side and would have
//! parsed this happily; the read side is not wrong.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::writer::SaveOptions;

/// A document whose `/AcroForm /Fields` names the **page** object.
///
/// Deliberately minimal and hand-built rather than reduced from the fuzzer's
/// artifact: the artifact is a mutated PDF whose other damage is irrelevant,
/// and a test that carries it would fail for reasons nobody could read.
fn form_whose_field_is_a_page() -> Vec<u8> {
    let content = "BT /Helv 12 Tf 60 700 Td (page) Tj ET\n";
    let bodies = [
        // The page (object 3) is listed as a form FIELD. It has no `/T`, so
        // its fully-qualified name is empty — which is legal per §12.7.3.2's
        // silence and is how the fuzzer reached it.
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [3 0 R] >> >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 750] /Resources \
         << /Font << /Helv 5 0 R >> >> /Contents 4 0 R /FT /Tx >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
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

/// **A field that is also a page is REFUSED, and the document still opens.**
///
/// The assertion that matters is the second one: whatever `delete_field`
/// decides, the saved bytes must still parse. A refusal is the right answer
/// here — pdfcer cannot delete the field without deleting the page, and
/// deleting the page is not what was asked for — but the test is written so
/// that a future implementation choosing to delete only the *field-ness*
/// would also pass, because that is a legitimate different answer to the same
/// question.
///
/// What must never pass is `Ok` plus a file that does not reopen.
#[test]
fn deleting_a_field_that_is_a_page_never_produces_an_unreadable_file() {
    let mut s = EditSession::new(Document::from_bytes(form_whose_field_is_a_page()).unwrap());
    let result = s.delete_field("");

    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("saving must not fail")
        .0;
    let reloaded = Document::from_bytes(bytes);
    assert!(
        reloaded.is_ok(),
        "delete_field returned {result:?} and produced a file pdfcer cannot reopen"
    );
    let doc = reloaded.unwrap();
    assert!(
        pdfcer_core::page_tree::pages(&doc).is_ok(),
        "the page tree must still walk after the operation, whatever it decided"
    );
}

/// **The same shape reached through the grouping-node verb.**
///
/// ★ Verified as its own case rather than assumed to follow: `delete_field`
/// and `delete_field_group` build their removal sets in different functions,
/// and a guard added to one is exactly the kind of fix that leaves the other
/// broken beside it.
#[test]
fn deleting_a_group_that_is_a_page_never_produces_an_unreadable_file() {
    let mut s = EditSession::new(Document::from_bytes(form_whose_field_is_a_page()).unwrap());
    let result = s.delete_field_group("");

    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("saving must not fail")
        .0;
    let reloaded = Document::from_bytes(bytes);
    assert!(
        reloaded.is_ok(),
        "delete_field_group returned {result:?} and produced a file pdfcer cannot reopen"
    );
}

/// A document whose `/AcroForm /Fields` names the **catalog itself**.
///
/// ★★ The second shape, and the one the first fix walked straight past.
/// `page_slots`'s `ancestors` chain stops at the `/Pages` root and never
/// includes the catalog that points at it, so a guard built from "the page
/// tree" does not contain the object whose loss produces `NoPageTreeRoot`.
///
/// The symptom named this from the start — `NoPageTreeRoot`, not `NoPages` —
/// and it was read as covering the class because the first reproducer went
/// green.
fn form_whose_field_is_the_catalog() -> Vec<u8> {
    let content = "BT /Helv 12 Tf 60 700 Td (page) Tj ET\n";
    let bodies = [
        // Object 1 is the catalog AND is listed as a form field.
        "<< /Type /Catalog /Pages 2 0 R /FT /Tx /AcroForm << /Fields [1 0 R] >> >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 750] /Resources \
         << /Font << /Helv 5 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
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

/// **Deleting a "field" that is the CATALOG never produces an unreadable
/// file.**
///
/// The distinction from the page case is the whole point: a guard assembled
/// from the page tree cannot contain the catalog, because the catalog is what
/// *points at* the page tree rather than part of it.
#[test]
fn deleting_a_field_that_is_the_catalog_never_produces_an_unreadable_file() {
    let mut s = EditSession::new(Document::from_bytes(form_whose_field_is_the_catalog()).unwrap());
    let result = s.delete_field("");

    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("saving must not fail")
        .0;
    let reloaded = Document::from_bytes(bytes);
    assert!(
        reloaded.is_ok(),
        "delete_field returned {result:?} and produced a file pdfcer cannot reopen"
    );
    assert!(
        pdfcer_core::page_tree::pages(&reloaded.unwrap()).is_ok(),
        "the page tree must still walk: losing the CATALOG is what NoPageTreeRoot means"
    );
}

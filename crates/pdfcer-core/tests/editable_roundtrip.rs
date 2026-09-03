//! **Editable round trip** — export, edit, compile back (`Pass 194.0`).
//!
//! # The one property that matters, and the one that is easy to fake
//!
//! It is trivial to write a round trip that "works": export, re-import, save,
//! and the file opens. That proves almost nothing. The property this feature
//! exists for is that **compiling back an UNEDITED export changes nothing**,
//! and that compiling back a **one-object edit** re-emits **one object**.
//!
//! Those are the assertions here, and they are the ones a naive implementation
//! fails silently. The export decodes every stream while the original's streams
//! are compressed, so a byte-for-byte comparison would mark every stream in the
//! document as modified — the incremental update would contain the entire file,
//! the round trip would still "work", every test that only checked
//! openability would still pass, and the feature's whole point would be gone.
//!
//! ★ `assert_eq!(report.modified.len(), 0)` on an untouched export is therefore
//! not a smoke test. It is the feature.
//!
//! # Why an incremental save is asserted on, specifically
//!
//! Because that is the capability qpdf does not have — its own `TODO.md` lists
//! incremental updates and digital-signature support as unimplemented, so every
//! qpdf round trip rewrites the whole file. The test that pdfcer's compile-back
//! produces a genuine §7.5.6 append, with the original bytes as an untouched
//! prefix, is what makes that claim checkable rather than a sentence in a
//! commit message.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::editable::{self, EditableError};
use pdfcer_core::object::{Name, ObjId, Object};
use pdfcer_core::writer::{SaveOptions, save_incremental};
use std::path::{Path, PathBuf};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel)
}

fn load(rel: &str) -> Document {
    Document::load(&fixture(rel)).expect("fixture must load")
}

/// A document with a real stream in it, so the decoded-vs-compressed comparison
/// is actually exercised rather than skipped.
fn with_stream() -> Document {
    load("synthetic/annot/ap-cascade-single-stream.pdf")
}

/// The export is a valid PDF that pdfcer can reopen, with the same object count.
///
/// The floor. If this fails nothing below is meaningful.
#[test]
fn an_export_is_a_valid_pdf_with_the_same_objects() {
    let doc = with_stream();
    let bytes = editable::export(&doc).expect("export");
    let back = Document::from_bytes(bytes).expect("the export must be a loadable PDF");
    assert_eq!(
        back.object_count(),
        doc.object_count(),
        "every object must survive the export"
    );
    assert!(
        pdfcer_core::page_tree::pages_in(back.view().graph()).is_ok(),
        "the exported document's page tree must walk"
    );
}

/// ★ Object streams are EXPANDED, which is the readability win the feature is
/// for: on a real file most interesting dictionaries are compressed inside
/// `/ObjStm` containers where no text search can reach them.
///
/// Skipped, loudly, when the external corpus is absent — it is fetched rather
/// than committed, and a test that failed on a clean checkout would be a false
/// alarm. The skip prints, because a silent skip reads exactly like a pass.
#[test]
fn an_export_expands_object_streams() {
    let path = fixture("external/qpdf/qpdf/qtest/qpdf/big-ostream.pdf");
    let Ok(doc) = Document::load(&path) else {
        eprintln!("SKIP: the external corpus is not present");
        return;
    };
    let before = pdfcer_core::structure::layout(&doc);
    assert!(
        !before.object_streams.is_empty(),
        "this fixture is chosen because it USES object streams"
    );

    let bytes = editable::export(&doc).expect("export");
    let back = Document::from_bytes(bytes).expect("loadable");
    let after = pdfcer_core::structure::layout(&back);
    assert!(
        after.object_streams.is_empty(),
        "the export must contain no object streams; it still has {:?}",
        after.object_streams.keys().collect::<Vec<_>>()
    );
    assert_eq!(back.object_count(), doc.object_count());
}

/// Streams come out decoded, with `/Filter` gone and `/Length` corrected.
#[test]
fn an_export_decodes_streams_and_drops_the_filter() {
    let doc = with_stream();
    let bytes = editable::export(&doc).expect("export");
    let back = Document::from_bytes(bytes).expect("loadable");

    let mut checked = 0;
    for obj in back.objects() {
        if let Object::Stream(s) = &obj.value {
            assert!(
                !s.dict.contains_key(b"Filter"),
                "object {} kept a /Filter after export",
                obj.id
            );
            let len = s.dict.get(b"Length").and_then(Object::as_number);
            assert_eq!(
                len.map(|v| v as usize),
                Some(s.data_span.len),
                "object {}'s /Length must describe its decoded bytes",
                obj.id
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "the fixture must contain a stream, or this asserts nothing"
    );
}

/// ★★★ THE FEATURE. An UNEDITED export compiles back to ZERO changes.
///
/// This is the assertion a naive implementation fails while still appearing to
/// work: comparing a decoded stream against its compressed original byte-for-byte
/// marks every stream modified, and the "incremental" update then contains the
/// whole document. See `editable::stream_is_unchanged`.
#[test]
fn an_unedited_export_compiles_back_to_no_change_at_all() {
    let doc = with_stream();
    let bytes = editable::export(&doc).expect("export");
    let edited = Document::from_bytes(bytes).expect("loadable");

    let (dirty, report) = editable::import(&doc, &edited);
    assert_eq!(
        report.modified,
        Vec::<ObjId>::new(),
        "an untouched export must modify nothing; it reported {:?}",
        report.modified
    );
    assert_eq!(report.added, Vec::<ObjId>::new());
    assert_eq!(report.removed, Vec::<ObjId>::new());
    assert!(dirty.is_empty(), "the dirty set must be empty");

    // ★ And the proof that the stream comparison did real work rather than
    // getting lucky: at least one stream had to be matched AFTER decoding.
    assert!(
        report.streams_matched_after_decode > 0,
        "no stream was compared semantically — the comparison fell back to byte \
         equality, which would mark every stream in a real document as edited"
    );

    // An empty dirty set means an incremental save reproduces the input exactly.
    let (out, save) = save_incremental(&doc, &dirty, &SaveOptions::identity()).expect("save");
    assert!(
        save.byte_identical,
        "zero edits must mean zero bytes appended"
    );
    assert_eq!(out, doc.bytes(), "the file must be byte-identical");
}

/// ★★ A ONE-OBJECT EDIT re-emits ONE object, and the original bytes survive as
/// an untouched prefix.
///
/// The prefix assertion is the signature-preservation claim made checkable: a
/// signature covers a byte range, and a range that did not move and was not
/// rewritten still verifies.
#[test]
fn a_one_object_edit_appends_only_that_object() {
    let doc = with_stream();
    let bytes = editable::export(&doc).expect("export");
    let mut edited = Document::from_bytes(bytes).expect("loadable");

    // Edit exactly one object, through the export's own model: give the
    // annotation a key it did not have.
    let target = edited
        .objects()
        .find(|o| {
            matches!(&o.value, Object::Dict(d)
                if d.get(b"Subtype").and_then(Object::as_name).is_some_and(|n| n.0 == b"Stamp"))
        })
        .map(|o| o.id)
        .expect("the fixture has a /Stamp annotation");

    let mut patched = match edited.get(target).map(|o| o.value.clone()) {
        Some(Object::Dict(d)) => d,
        other => panic!("expected a dictionary, got {other:?}"),
    };
    patched.insert(Name::from(b"Contents"), Object::String(b"edited".to_vec()));
    let mut dirty_for_rebuild = pdfcer_core::writer::DirtySet::empty();
    dirty_for_rebuild.replace(target, Object::Dict(patched));
    let (rebuilt, _) =
        pdfcer_core::writer::save_full(&edited, &dirty_for_rebuild, &SaveOptions::identity())
            .expect("rebuild the edited export");
    edited = Document::from_bytes(rebuilt).expect("edited export must reload");

    let (dirty, report) = editable::import(&doc, &edited);
    assert_eq!(
        report.modified,
        vec![target],
        "exactly one object changed; report was modified={:?} added={:?} removed={:?}",
        report.modified,
        report.added,
        report.removed
    );

    let (out, _) = save_incremental(&doc, &dirty, &SaveOptions::identity()).expect("save");
    assert!(
        out.starts_with(doc.bytes()),
        "an incremental compile-back must leave the ORIGINAL bytes as an untouched \
         prefix — that is what keeps a signature over them valid, and it is the \
         capability qpdf's own TODO lists as unimplemented"
    );
    assert!(
        out.len() > doc.bytes().len(),
        "something must have been appended"
    );

    let back = Document::from_bytes(out).expect("the compiled-back file must load");
    let got = back
        .get(target)
        .and_then(|o| o.value.as_dict())
        .and_then(|d| d.get(b"Contents"))
        .cloned();
    assert_eq!(
        got,
        Some(Object::String(b"edited".to_vec())),
        "the edit must actually be present in the compiled-back file"
    );
}

/// An encrypted document is REFUSED, not silently decrypted to disk.
///
/// Skipped when the fixture is absent rather than asserted vacuously.
#[test]
fn an_encrypted_document_is_refused_rather_than_decrypted() {
    let path = fixture("external/qpdf/qpdf/qtest/qpdf/c-decrypt-with-user.pdf");
    let Ok(doc) = Document::load(&path) else {
        eprintln!("SKIP: the external corpus is not present, or this file needs a password");
        return;
    };
    if doc.encryption().is_none() {
        eprintln!("SKIP: this fixture did not load as an encrypted document");
        return;
    }
    assert_eq!(
        editable::export(&doc).unwrap_err(),
        EditableError::Encrypted,
        "exporting an encrypted document writes its plaintext to disk; that is the \
         operator's decision, not this verb's"
    );
}

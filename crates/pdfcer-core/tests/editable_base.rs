//! The export's recorded base (G112) and compiling an export back into an
//! open session (G113).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, InfoField};
use pdfcer_core::editable::{self, EditableSource, ExportBase};
use pdfcer_core::object::{Name, ObjId, Object};
use pdfcer_core::writer::{DirtySet, SaveOptions, save_full, save_incremental};
use std::path::Path;

fn load(rel: &str) -> Document {
    Document::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(rel),
    )
    .expect("fixture must load")
}

fn stamp_doc() -> Document {
    load("synthetic/annot/ap-cascade-single-stream.pdf")
}

const STAMP: ObjId = ObjId::new(4, 0);

fn reopen(bytes: Vec<u8>) -> Document {
    Document::from_bytes(bytes).expect("reloads")
}

/// `doc` saved with one session edit: a `/Title` in a new `/Info`.
fn titled(doc: &Document, title: &str) -> Document {
    let mut s = EditSession::new(reopen(doc.bytes().to_vec()));
    s.set_info_field(InfoField::Title, Some(title))
        .expect("title");
    let (out, _) =
        save_incremental(s.document(), &s.dirty_set(), &SaveOptions::identity()).expect("save");
    reopen(out)
}

/// `edited` with `changes` applied, rewritten as a fresh export would be.
fn rewrite(edited: &Document, changes: &[(ObjId, Object)]) -> Document {
    let mut dirty = DirtySet::empty();
    for (id, value) in changes {
        dirty.replace(*id, value.clone());
    }
    let (out, _) = save_full(edited, &dirty, &SaveOptions::identity()).expect("rewrite");
    reopen(out)
}

fn stamp_with_contents(doc: &Document, text: &[u8]) -> Object {
    let mut d = doc
        .get(STAMP)
        .and_then(|o| o.value.as_dict())
        .cloned()
        .expect("stamp");
    d.insert(Name::from(b"Contents"), Object::String(text.to_vec()));
    Object::Dict(d)
}

#[test]
fn a_fresh_export_records_its_base_and_imports_as_matching() {
    let doc = stamp_doc();
    let edited = reopen(editable::export(&doc).expect("export"));
    assert_eq!(
        editable::recorded_base(&edited),
        Some(editable::fingerprint(&doc))
    );
    let (_, report) = editable::import(&doc, &edited);
    assert_eq!(report.base, ExportBase::Matches);
}

#[test]
fn an_export_of_an_older_state_imports_as_differs() {
    let doc = stamp_doc();
    let edited = reopen(editable::export(&doc).expect("export"));
    let (_, report) = editable::import(&titled(&doc, "later"), &edited);
    assert_eq!(report.base, ExportBase::Differs);
}

#[test]
fn an_export_without_the_marker_imports_as_unrecorded() {
    let doc = stamp_doc();
    let mut bytes = editable::export(&doc).expect("export");
    let at = bytes
        .windows(17)
        .position(|w| w == b"%PdfcerExportBase")
        .expect("the export carries the marker");
    bytes[at + 1] = b'X';
    let edited = reopen(bytes);
    assert_eq!(editable::recorded_base(&edited), None);
    let (_, report) = editable::import(&doc, &edited);
    assert_eq!(report.base, ExportBase::Unrecorded);
}

/// Decoding streams and expanding object streams changes the layout, not the
/// content, so the export fingerprints the same as its source.
#[test]
fn the_fingerprint_ignores_compression_and_object_streams() {
    let doc = load("verapdf/object-streams.pdf");
    let export = reopen(editable::export(&doc).expect("export"));
    assert_eq!(editable::fingerprint(&export), editable::fingerprint(&doc));
}

#[test]
fn a_session_fingerprints_as_its_saved_and_reopened_document() {
    let doc = stamp_doc();
    let mut s = EditSession::new(reopen(doc.bytes().to_vec()));
    s.set_info_field(InfoField::Title, Some("t"))
        .expect("title");
    let saved = titled(&doc, "t");
    assert_eq!(editable::fingerprint(&s), editable::fingerprint(&saved));
    assert_ne!(editable::fingerprint(&s), editable::fingerprint(&doc));
}

#[test]
fn a_session_export_carries_its_unsaved_edits() {
    let mut s = EditSession::new(stamp_doc());
    s.set_info_field(InfoField::Title, Some("unsaved"))
        .expect("title");
    let export = reopen(editable::export(&s).expect("export"));
    let info = match export.trailer().get(b"Info") {
        Some(Object::Reference(id)) => export.get(*id).and_then(|o| o.value.as_dict()).cloned(),
        _ => None,
    }
    .expect("the export has the session's /Info");
    assert_eq!(
        info.get(b"Title"),
        Some(&Object::String(b"unsaved".to_vec()))
    );

    let report = s.import_editable(&export).expect("import");
    assert!(
        report.is_empty(),
        "an unedited export changes nothing: {report:?}"
    );
    assert_eq!(report.base, ExportBase::Matches);
}

#[test]
fn import_editable_applies_one_undoable_edit() {
    let mut s = EditSession::new(stamp_doc());
    let export = reopen(editable::export(&s).expect("export"));
    let edited = rewrite(&export, &[(STAMP, stamp_with_contents(&export, b"hi"))]);

    let report = s.import_editable(&edited).expect("import");
    assert_eq!(report.modified, vec![STAMP]);
    let contents = |s: &EditSession| {
        s.value(STAMP)
            .and_then(Object::as_dict)?
            .get(b"Contents")
            .cloned()
    };
    assert_eq!(contents(&s), Some(Object::String(b"hi".to_vec())));

    s.undo().expect("one undo entry");
    assert_eq!(contents(&s), None);
    assert!(!s.is_modified());
}

#[test]
fn objects_an_import_adds_are_not_reallocated() {
    let mut s = EditSession::new(stamp_doc());
    let export = reopen(editable::export(&s).expect("export"));
    let added = ObjId::new(9, 0);
    let edited = rewrite(&export, &[(added, Object::Integer(42))]);

    let report = s.import_editable(&edited).expect("import");
    assert_eq!(report.added, vec![added]);
    assert_eq!(s.value(added), Some(&Object::Integer(42)));

    s.set_info_field(InfoField::Title, Some("t"))
        .expect("title");
    let info = match s.trailer_entry(b"Info") {
        Some(Object::Reference(id)) => *id,
        other => panic!("expected an /Info reference, got {other:?}"),
    };
    assert!(
        info.num > added.num,
        "/Info took {info}, at or below the imported {added}"
    );
}

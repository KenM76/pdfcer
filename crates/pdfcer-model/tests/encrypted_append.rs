//! Incremental save of an encrypted document appends under the document's own
//! handler and key (§7.6.2), carrying `/Encrypt` and `/ID[0]` unchanged
//! (§7.6.3).
//!
//! Fixtures: `fixtures/synthetic/encryption/` (passwords `userpw` / `ownerpw`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_model::document::Document;
use pdfcer_model::object::{Name, ObjId, Object};
use pdfcer_model::writer::{
    DirtySet, Rc4Append, SaveOptions, SaveReport, WriteError, save_incremental,
};

const FIELD: ObjId = ObjId::new(4, 0);
const FORM: ObjId = ObjId::new(6, 0);

fn fixture(name: &str) -> Vec<u8> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/encryption")
        .join(name);
    std::fs::read(path).expect("fixture")
}

fn open(bytes: &[u8], password: &[u8]) -> Document {
    Document::from_bytes_with_password(bytes.to_vec(), Some(password)).expect("opens")
}

/// The decrypted data of stream `id`.
fn stream_data(doc: &Document, id: ObjId) -> Vec<u8> {
    let Object::Stream(s) = &doc.get(id).expect("object").value else {
        panic!("{id:?} is not a stream");
    };
    s.data_span.slice(doc.bytes()).expect("in range").to_vec()
}

/// Sets the field's `/V` to `value`, and re-emits the form XObject unchanged
/// in value (so its data is written fresh, under a fresh IV).
fn edit(doc: &Document, value: &[u8]) -> DirtySet {
    let mut field = doc.get(FIELD).expect("field").value.clone();
    let Object::Dict(d) = &mut field else {
        panic!("field is a dictionary")
    };
    d.insert(Name::from(b"V"), Object::String(value.to_vec()));
    let mut dirty = DirtySet::empty();
    dirty.replace(FIELD, field);
    dirty.replace(FORM, doc.get(FORM).expect("form").value.clone());
    dirty
}

fn id_pair(doc: &Document) -> (Vec<u8>, Vec<u8>) {
    let Some(Object::Array(items)) = doc.trailer().get(b"ID") else {
        panic!("no /ID")
    };
    let s = |o: &Object| match o {
        Object::String(s) => s.clone(),
        other => panic!("{other:?}"),
    };
    (s(&items[0]), s(&items[1]))
}

/// Every round trip opts in to RC4 append, which must change nothing for an
/// AES document.
fn round_trip(name: &str) -> SaveReport {
    let original = fixture(name);
    let mut doc = open(&original, b"userpw");
    doc.set_rc4_append(Rc4Append::Preserve);
    let form_before = stream_data(&doc, FORM);

    let (out, report) =
        save_incremental(&doc, &edit(&doc, b"hello"), &SaveOptions::default()).expect("saves");
    assert_eq!(report.objects_reserialized, 2);
    assert_eq!(
        &out[..original.len()],
        &original[..],
        "an append only appends"
    );
    let appended = &out[original.len()..];
    assert!(
        !appended.windows(5).any(|w| w == b"hello"),
        "the new string is not written in clear"
    );

    let reopened = open(&out, b"userpw");
    let Object::Dict(field) = &reopened.get(FIELD).unwrap().value else {
        panic!()
    };
    assert_eq!(field.get(b"V"), Some(&Object::String(b"hello".to_vec())));
    assert_eq!(stream_data(&reopened, FORM), form_before);
    assert_eq!(
        reopened.trailer().get(b"Encrypt"),
        doc.trailer().get(b"Encrypt"),
        "/Encrypt is carried, not rewritten"
    );
    let (before, after) = (id_pair(&doc), id_pair(&reopened));
    assert_eq!(after.0, before.0, "/ID[0] never changes (§7.6.3)");
    assert_ne!(after.1, before.1, "/ID[1] refreshes on a change (§14.4)");
    assert!(open(&out, b"ownerpw").encryption().is_some());
    report
}

#[test]
fn an_aes_128_document_appends_under_its_own_key() {
    assert_eq!(round_trip("enc-aes-128.pdf").rc4_keystream_reused, None);
}

#[test]
fn an_aes_256_r6_document_appends_under_its_own_key() {
    assert_eq!(round_trip("enc-aes-256-r6.pdf").rc4_keystream_reused, None);
}

/// `/V` 1, `/V` 2 and a `/V` 4 `/CFM /V2` crypt filter: the edited field and
/// the re-emitted form both reuse their previous revision's keystream.
#[test]
fn an_opted_in_rc4_document_appends_under_its_own_key() {
    for name in ["enc-rc4-40.pdf", "enc-rc4-128.pdf", "enc-rc4-128-v4.pdf"] {
        assert_eq!(round_trip(name).rc4_keystream_reused, Some(2), "{name}");
    }
}

/// A created object gets a fresh number, so a key no revision used before.
#[test]
fn a_created_object_does_not_count_as_keystream_reuse() {
    let mut doc = open(&fixture("enc-rc4-128.pdf"), b"userpw");
    doc.set_rc4_append(Rc4Append::Preserve);
    let mut dirty = edit(&doc, b"hello");
    let id = ObjId::new(doc.next_object_number().unwrap(), 0);
    dirty.replace(id, Object::String(b"new".to_vec()));
    let (out, report) = save_incremental(&doc, &dirty, &SaveOptions::default()).unwrap();
    assert_eq!(report.rc4_keystream_reused, Some(2));
    let reopened = open(&out, b"userpw");
    assert_eq!(
        reopened.get(id).unwrap().value,
        Object::String(b"new".to_vec())
    );
}

/// A verbatim re-emission copies the stored ciphertext, so it reuses no
/// keystream.
#[test]
fn a_verbatim_reemission_does_not_count_as_keystream_reuse() {
    let mut doc = open(&fixture("enc-rc4-128.pdf"), b"userpw");
    doc.set_rc4_append(Rc4Append::Preserve);
    let mut dirty = DirtySet::identity_reemission([FORM]);
    let field = edit(&doc, b"v").replacement(FIELD).unwrap().clone();
    dirty.replace(FIELD, field);
    let (_, report) = save_incremental(&doc, &dirty, &SaveOptions::default()).unwrap();
    assert_eq!(report.rc4_keystream_reused, Some(1));
}

#[test]
fn rc4_append_is_refused_until_opted_in() {
    let mut doc = open(&fixture("enc-rc4-128-v4.pdf"), b"userpw");
    let enc = doc.encryption().unwrap();
    assert!(enc.uses_rc4() && !enc.appendable());
    assert_eq!(enc.rc4_append(), Rc4Append::Refuse);
    doc.set_rc4_append(Rc4Append::Preserve);
    assert!(doc.encryption().unwrap().appendable());
    doc.set_rc4_append(Rc4Append::Refuse);
    let result = save_incremental(&doc, &edit(&doc, b"x"), &SaveOptions::default());
    assert!(
        matches!(result, Err(WriteError::Rc4AppendRefused)),
        "{result:?}"
    );
}

#[test]
fn an_rc4_document_is_refused_by_name() {
    let doc = open(&fixture("enc-rc4-128.pdf"), b"userpw");
    let mut dirty = DirtySet::empty();
    dirty.replace(FIELD, doc.get(FIELD).unwrap().value.clone());
    match save_incremental(&doc, &dirty, &SaveOptions::default()) {
        Err(WriteError::Rc4AppendRefused) => {}
        other => panic!("expected Rc4AppendRefused, got {other:?}"),
    }
}

#[test]
fn an_empty_edit_returns_the_stored_ciphertext_unchanged() {
    let original = fixture("enc-aes-128.pdf");
    let doc = open(&original, b"userpw");
    let (out, report) =
        save_incremental(&doc, &DirtySet::empty(), &SaveOptions::default()).unwrap();
    assert_eq!(out, original);
    assert!(report.byte_identical);
}

/// A signature dictionary's `/Contents` is never encrypted (ISO 32000-2
/// §7.6.2); its other strings are.
#[test]
fn a_signature_dictionarys_contents_is_written_in_clear() {
    let original = fixture("enc-aes-128.pdf");
    let doc = open(&original, b"userpw");
    let mut sig = pdfcer_model::object::Dict::new();
    sig.insert(Name::from(b"Type"), Object::Name(Name::from(b"Sig")));
    sig.insert(
        Name::from(b"Contents"),
        Object::String(b"CLEARSIG".to_vec()),
    );
    sig.insert(Name::from(b"Name"), Object::String(b"SECRETNAME".to_vec()));
    let id = ObjId::new(doc.next_object_number().unwrap(), 0);
    let mut dirty = DirtySet::empty();
    dirty.replace(id, Object::Dict(sig));
    let (out, _) = save_incremental(&doc, &dirty, &SaveOptions::default()).unwrap();
    let appended = &out[original.len()..];
    let hex: Vec<u8> = b"CLEARSIG"
        .iter()
        .flat_map(|b| format!("{b:02X}").into_bytes())
        .collect();
    let has = |n: &[u8]| appended.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n));
    assert!(has(b"CLEARSIG") || has(&hex), "/Contents in clear");
    assert!(!has(b"SECRETNAME"), "/Name is encrypted");
    let reopened = open(&out, b"userpw");
    let Object::Dict(d) = &reopened.get(id).unwrap().value else {
        panic!()
    };
    assert_eq!(
        d.get(b"Contents"),
        Some(&Object::String(b"CLEARSIG".to_vec()))
    );
    assert_eq!(
        d.get(b"Name"),
        Some(&Object::String(b"SECRETNAME".to_vec()))
    );
}

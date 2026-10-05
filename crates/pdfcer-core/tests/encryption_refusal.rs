//! An encryption refusal reports which cause applies: RC4 not allowed, or a
//! password that does not grant the edit.

use pdfcer_core::crypto::PermissionBit;
use pdfcer_core::document::{Document, EncryptedRefusal};
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::writer::Rc4Append;

const DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/synthetic/encryption/"
);

fn session(name: &str, password: Option<&[u8]>) -> EditSession {
    let bytes = std::fs::read(format!("{DIR}{name}")).expect("fixture");
    EditSession::new(Document::from_bytes_with_password(bytes, password).expect("opens"))
}

/// Rotate page 0, asserting the refusal and returning the session's cause.
fn rotate_refused(s: &mut EditSession) -> Option<EncryptedRefusal> {
    assert!(matches!(
        s.rotate_pages(&[0], 90),
        Err(EditError::DocumentEncrypted)
    ));
    s.encryption_refusal_cause()
}

#[test]
fn rc4_and_permission_refusals_of_one_verb_report_different_causes() {
    let mut rc4 = session("enc-rc4-128.pdf", Some(b"ownerpw"));
    assert_eq!(
        rc4.encryption_refusal(&[PermissionBit::Assemble]),
        Some(EncryptedRefusal::Rc4NotAllowed)
    );
    assert_eq!(
        rotate_refused(&mut rc4),
        Some(EncryptedRefusal::Rc4NotAllowed)
    );

    let mut denied = session("enc-emptyuser-print-only.pdf", None);
    assert_eq!(
        denied.encryption_refusal(&[PermissionBit::Assemble]),
        Some(EncryptedRefusal::PermissionDenied)
    );
    assert_eq!(
        rotate_refused(&mut denied),
        Some(EncryptedRefusal::PermissionDenied)
    );
}

#[test]
fn allowing_rc4_lifts_the_rc4_refusal() {
    let mut s = session("enc-rc4-128.pdf", Some(b"ownerpw"));
    s.set_rc4_append(Rc4Append::Preserve);
    assert_eq!(s.encryption_refusal(&[PermissionBit::Assemble]), None);
    s.rotate_pages(&[0], 90).expect("rotate under Preserve");
}

#[test]
fn a_plain_document_has_no_refusal() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/hello.pdf"
    ))
    .expect("fixture");
    let s = EditSession::new(Document::from_bytes(bytes).expect("opens"));
    assert_eq!(s.encryption_refusal(&[PermissionBit::ModifyContents]), None);
    assert_eq!(s.encryption_refusal_cause(), None);
}

//! `Pass 10.12` — certifying (author) signatures: the `/DocMDP` transform
//! on the signature dictionary and `/Perms /DocMDP` on the catalog
//! (ISO 32000-1 §12.8.2.2, Tables 253/254, §12.8.4 Table 258).
//!
//! Rules under test: a certification is ONE per document and the FIRST
//! signature (§12.8.2.2.1), both refused by name; `P` defaults to 2; the
//! read side (`signature::census`, `signature_verify`) classifies pdfcer's
//! own output exactly as it classifies a foreign certified fixture; and the
//! approval-after-certification matrix (`P=1` refused, `P=2`/`P=3`
//! permitted with the certification still verifying).

#![cfg(feature = "signing")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::sign::apply::{MdpPermission, SignApplyError, SignRequest};
use pdfcer_core::sign::pkcs12::Pkcs12Signer;
use pdfcer_core::signature::census;
use pdfcer_core::signature_verify::{Integrity, verify_all};
use pdfcer_core::writer::SaveOptions;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(rel)).unwrap()
}

fn rsa() -> Pkcs12Signer {
    Pkcs12Signer::from_der(&read("signing/rsa2048-modern.pfx"), "pdfcer").unwrap()
}

const T0: &str = "D:20260906000000Z";

fn certify(level: MdpPermission) -> SignRequest {
    let mut r = SignRequest::at(T0);
    r.certify = Some(level);
    r
}

fn sign(base: &[u8], req: &SignRequest) -> Result<Vec<u8>, SignApplyError> {
    let mut s = EditSession::new(Document::from_bytes(base.to_vec()).unwrap());
    let (out, report) = s.sign(&rsa(), req, &SaveOptions::identity())?;
    assert_eq!(report.certification, req.certify);
    assert_eq!(&out[..base.len()], base, "incremental");
    Ok(out)
}

fn all_verified(bytes: &[u8]) -> usize {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let v = verify_all(&doc.view(), bytes);
    assert!(
        v.iter()
            .all(|v| matches!(v.integrity, Integrity::Verified { .. })),
        "{v:?}"
    );
    v.len()
}

#[test]
fn a_certification_writes_the_docmdp_transform_and_the_catalog_perms() {
    let bytes = sign(
        &read("hello.pdf"),
        &certify(MdpPermission::FormFillSignAnnotate),
    )
    .unwrap();
    // Bytes: Table 253/254 shapes, /V as a NAME, no DigestMethod.
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/TransformMethod /DocMDP"));
    assert!(
        text.contains("/TransformParams <</Type /TransformParams/P 3/V /1.2>>"),
        "{text}"
    );
    assert!(
        !text.contains("/DigestMethod"),
        "deprecated in PDF 2.0; omitted"
    );
    // Catalog: /Perms /DocMDP points at the signature dictionary.
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    let view = doc.view();
    let catalog = view.catalog_dict().unwrap();
    let perms = view
        .resolve(catalog.get(b"Perms").unwrap())
        .as_dict()
        .unwrap()
        .clone();
    let Some(Object::Reference(sig_id)) = perms.get(b"DocMDP") else {
        panic!("/Perms /DocMDP must be a reference: {perms:?}");
    };
    let sig = view
        .resolve(&Object::Reference(*sig_id))
        .as_dict()
        .unwrap()
        .clone();
    assert!(
        sig.contains_key(b"ByteRange"),
        "it is the signature dictionary"
    );
    // Read side, both instruments.
    let c = census(&view);
    assert_eq!(
        (
            c.signatures,
            c.certifications,
            c.certification_permission,
            c.perms_enforced
        ),
        (1, 1, Some(3), true)
    );
    let v = verify_all(&view, &bytes);
    assert_eq!(v.len(), 1);
    assert!(
        matches!(v[0].integrity, Integrity::Verified { .. }),
        "{:?}",
        v[0].integrity
    );
    assert_eq!(v[0].certification, Some(3));
}

#[test]
fn the_read_side_classifies_pdfcer_output_like_a_foreign_certified_fixture() {
    // Criterion 4: the same census verdict for pdfcer's own P=2 output and
    // the synthetic certified fixture the Pass 3.2 model was built on.
    let ours = sign(&read("hello.pdf"), &certify(MdpPermission::FormFillAndSign)).unwrap();
    let foreign = read("forms/certified-p2-form.pdf");
    let (a, b) = (
        Document::from_bytes(ours).unwrap(),
        Document::from_bytes(foreign).unwrap(),
    );
    let (ca, cb) = (census(&a.view()), census(&b.view()));
    assert_eq!(ca.certifications, cb.certifications);
    assert_eq!(ca.certification_permission, cb.certification_permission);
    assert_eq!(ca.certification_permission, Some(2));
    assert_eq!(ca.perms_enforced, cb.perms_enforced);
}

#[test]
fn a_second_certification_is_refused_by_name_and_writes_nothing() {
    let once = sign(&read("hello.pdf"), &certify(MdpPermission::FormFillAndSign)).unwrap();
    let mut s = EditSession::new(Document::from_bytes(once.clone()).unwrap());
    let err = s
        .sign(
            &rsa(),
            &certify(MdpPermission::NoChanges),
            &SaveOptions::identity(),
        )
        .unwrap_err();
    assert!(
        matches!(err, SignApplyError::AlreadyCertified { permission: 2 }),
        "{err:?}"
    );
    assert!(err.to_string().contains("sign without --certify"));
    assert!(!s.is_modified());
}

#[test]
fn a_certification_over_an_existing_approval_is_refused_by_name() {
    let approved = sign(&read("hello.pdf"), &SignRequest::at(T0)).unwrap();
    let mut s = EditSession::new(Document::from_bytes(approved).unwrap());
    let err = s
        .sign(
            &rsa(),
            &certify(MdpPermission::FormFillAndSign),
            &SaveOptions::identity(),
        )
        .unwrap_err();
    assert!(
        matches!(err, SignApplyError::CertificationNotFirst { existing: 1 }),
        "{err:?}"
    );
    assert!(!s.is_modified());
    // And a pyHanko-signed base counts the same way.
    let mut s =
        EditSession::new(Document::from_bytes(read("signing/foreign-pyhanko-first.pdf")).unwrap());
    let err = s
        .sign(
            &rsa(),
            &certify(MdpPermission::FormFillAndSign),
            &SaveOptions::identity(),
        )
        .unwrap_err();
    assert!(
        matches!(err, SignApplyError::CertificationNotFirst { existing: 1 }),
        "{err:?}"
    );
}

#[test]
fn the_approval_after_certification_matrix() {
    // P=2 and P=3: an approval signature is exactly what those levels permit;
    // the certification still verifies after it (its /ByteRange intact).
    for level in [
        MdpPermission::FormFillAndSign,
        MdpPermission::FormFillSignAnnotate,
    ] {
        let certified = sign(&read("hello.pdf"), &certify(level)).unwrap();
        let both = sign(&certified, &SignRequest::at(T0)).expect("an approval is permitted");
        assert_eq!(all_verified(&both), 2, "P={}", level.p());
        let doc = Document::from_bytes(both.clone()).unwrap();
        let v = verify_all(&doc.view(), &both);
        assert_eq!(v[0].certification, Some(level.p()));
        assert_eq!(
            v[1].certification, None,
            "the approval is not a certification"
        );
        let c = census(&doc.view());
        assert_eq!((c.signatures, c.certifications), (2, 1));
    }
    // P=1: nothing may change, so no signature may be added (Pass 10.9's
    // existing refusal, now reachable from pdfcer's own certification).
    let locked = sign(&read("hello.pdf"), &certify(MdpPermission::NoChanges)).unwrap();
    assert_eq!(all_verified(&locked), 1);
    let mut s = EditSession::new(Document::from_bytes(locked).unwrap());
    let err = s
        .sign(&rsa(), &SignRequest::at(T0), &SaveOptions::identity())
        .unwrap_err();
    assert!(
        matches!(err, SignApplyError::CertificationForbids { permission: 1 }),
        "{err:?}"
    );
}

#[test]
fn mdp_permission_round_trips_its_table_254_values() {
    for (m, p, words) in [
        (MdpPermission::NoChanges, 1, "no changes"),
        (
            MdpPermission::FormFillAndSign,
            2,
            "form fill-in and signing",
        ),
        (
            MdpPermission::FormFillSignAnnotate,
            3,
            "form fill-in, signing and annotations",
        ),
    ] {
        assert_eq!(m.p(), p);
        assert_eq!(MdpPermission::from_p(p), Some(m));
        assert_eq!(m.meaning(), words);
    }
    assert_eq!(MdpPermission::from_p(0), None);
    assert_eq!(MdpPermission::from_p(4), None);
}

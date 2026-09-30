//! PAdES B-LT: [`EditSession::add_validation_material`] writes certificates,
//! CRLs and OCSP responses into `/DSS` (ETSI EN 319 142-1 §5.4.2.2), and the
//! verifier then finds them there. Fixtures from `tools/gen-ocsp-fixtures.py`.

#![cfg(feature = "signing")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::sign::apply::{MdpPermission, SignRequest};
use pdfcer_core::sign::ltv::{DssReport, MaterialKind, ValidationMaterial};
use pdfcer_core::sign::pkcs12::Pkcs12Signer;
use pdfcer_core::signature::{
    Integrity, Revocation, RevocationSource, SignatureVerdict, verify_all,
};
use pdfcer_core::writer::SaveOptions;

fn ocsp_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/ocsp")
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(ocsp_dir().join(name)).unwrap()
}

fn signed_with(request: &SignRequest) -> Vec<u8> {
    let hello = ocsp_dir().join("../hello.pdf");
    let mut s = EditSession::new(Document::load(&hello).unwrap());
    let signer = Pkcs12Signer::from_der(&fixture("leaf.pfx"), "pdfcer").unwrap();
    s.sign(&signer, request, &SaveOptions::identity())
        .expect("sign")
        .0
}

fn signed() -> Vec<u8> {
    signed_with(&SignRequest::at("D:20260930000000Z"))
}

/// Add `material` to `base`, save incrementally, return (bytes, report).
fn add(base: &[u8], material: &ValidationMaterial) -> Result<(Vec<u8>, DssReport), EditError> {
    let mut s = EditSession::new(Document::from_bytes(base.to_vec()).unwrap());
    let report = s.add_validation_material(material)?;
    let out = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    assert_eq!(&out[..base.len()], base, "incremental");
    Ok((out, report))
}

fn only_verdict(bytes: &[u8]) -> SignatureVerdict {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let mut all = verify_all(&doc.view(), bytes);
    assert_eq!(all.len(), 1);
    let v = all.remove(0);
    assert!(
        matches!(v.integrity, Integrity::Verified { .. }),
        "{:?}",
        v.integrity
    );
    v
}

fn dss_count(bytes: &[u8], key: &[u8]) -> usize {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let view = doc.view();
    let graph = view.graph();
    let catalog = graph.resolved(graph.catalog_id().unwrap());
    let dss = graph
        .resolve(catalog.as_dict().unwrap().get(b"DSS").expect("/DSS"))
        .as_dict()
        .unwrap();
    dss.get(key)
        .map(|a| graph.resolve(a).as_array().unwrap().len())
        .unwrap_or(0)
}

#[test]
fn material_in_the_dss_makes_the_signature_verifiable_offline() {
    let base = signed();
    assert!(matches!(
        only_verdict(&base).revocation,
        Revocation::NotChecked
    ));
    let material = ValidationMaterial::new()
        .with_ocsp(fixture("ocsp-good-delegated.der"))
        .with_crl(fixture("crl-clear.crl"));
    let (out, report) = add(&base, &material).unwrap();
    assert_eq!(report.ocsps_added, 1);
    assert_eq!(report.crls_added, 1);
    assert_eq!(report.certs_added, report.signature_certificates_added);
    assert!(report.signature_certificates_added >= 1);
    assert_eq!(report.carried_forward, 0);
    let v = only_verdict(&out);
    let Revocation::Good { checked } = &v.revocation else {
        panic!("{:?}", v.revocation);
    };
    assert!(checked.iter().all(|c| c.source == RevocationSource::Dss));
    assert_eq!(dss_count(&out, b"CRLs"), 1);
    assert!(!String::from_utf8_lossy(&out[base.len()..]).contains("/VRI"));
}

#[test]
fn a_second_pass_carries_forward_and_skips_duplicates() {
    let first = ValidationMaterial::new().with_crl(fixture("crl-clear.crl"));
    let (once, _) = add(&signed(), &first).unwrap();
    let second = ValidationMaterial::new()
        .with_crl(fixture("crl-clear.crl"))
        .with_crl(fixture("crl-responder-revoked.crl"))
        .with_ocsp(fixture("ocsp-good.der"))
        .with_ocsp(fixture("ocsp-good.der"));
    let (twice, report) = add(&once, &second).unwrap();
    assert_eq!(report.ocsps_added, 1);
    assert_eq!(report.crls_added, 1);
    assert_eq!(
        report.certs_added, 0,
        "the signer's certs are already there"
    );
    // crl + ocsp copy + every signature cert
    assert_eq!(report.duplicates_skipped, 2 + dss_count(&once, b"Certs"));
    assert_eq!(report.carried_forward, 1 + dss_count(&once, b"Certs"));
    assert_eq!(
        dss_count(&twice, b"CRLs"),
        2,
        "the first CRL is carried forward"
    );
    assert_eq!(dss_count(&twice, b"OCSPs"), 1);
    assert_eq!(dss_count(&twice, b"Certs"), dss_count(&once, b"Certs"));
    // Nothing new: no command, and an empty report.
    let mut s = EditSession::new(Document::from_bytes(twice.clone()).unwrap());
    assert!(s.add_validation_material(&second).unwrap().is_empty());
    assert!(!s.can_undo());
}

#[test]
fn a_bare_basic_response_is_wrapped_and_still_verifies() {
    let material = ValidationMaterial::new()
        .with_ocsp(fixture("ocsp-basic-only.der"))
        .include_signature_certificates(false);
    let (out, report) = add(&signed(), &material).unwrap();
    assert_eq!(report.ocsps_wrapped, 1);
    assert_eq!(report.certs_added, 0);
    let basic = fixture("ocsp-basic-only.der");
    let written = &out[out.len() - (out.len() - signed().len())..];
    let at = written
        .windows(basic.len())
        .position(|w| w == basic.as_slice())
        .expect("the basic response is inside the wrapper");
    // The wrapper's OID sits right before the OCTET STRING holding it.
    let oid = [
        0x06, 0x09, 0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x30, 0x01, 0x01,
    ];
    assert!(written[..at].windows(oid.len()).any(|w| w == oid));
    assert!(matches!(
        only_verdict(&out).revocation,
        Revocation::Good { .. }
    ));
}

#[test]
fn refusals_name_the_blob_and_write_nothing() {
    let base = signed();
    let bad = ValidationMaterial::new()
        .with_crl(fixture("crl-clear.crl"))
        .with_ocsp(fixture("ocsp-good.der"))
        .with_ocsp(fixture("ocsp-trylater.der"));
    let mut s = EditSession::new(Document::from_bytes(base.clone()).unwrap());
    assert!(matches!(
        s.add_validation_material(&bad),
        Err(EditError::ValidationMaterialUnreadable {
            kind: MaterialKind::Ocsp,
            index: 1,
            ..
        })
    ));
    assert!(!s.can_undo());
    let mut trailing = fixture("crl-clear.crl");
    trailing.push(0);
    for material in [
        ValidationMaterial::new().with_crl(trailing),
        ValidationMaterial::new().with_cert(fixture("crl-clear.crl")),
    ] {
        assert!(matches!(
            s.add_validation_material(&material),
            Err(EditError::ValidationMaterialUnreadable { .. })
        ));
    }
    let hello = ocsp_dir().join("../hello.pdf");
    let mut unsigned = EditSession::new(Document::load(&hello).unwrap());
    assert!(matches!(
        unsigned.add_validation_material(&ValidationMaterial::new()),
        Err(EditError::NoSignatureToValidate)
    ));
}

#[test]
fn no_changes_certification_refuses_unless_told() {
    let mut request = SignRequest::at("D:20260930000000Z");
    request.certify = Some(MdpPermission::NoChanges);
    let base = signed_with(&request);
    let material = ValidationMaterial::new().with_crl(fixture("crl-clear.crl"));
    let mut s = EditSession::new(Document::from_bytes(base.clone()).unwrap());
    assert!(matches!(
        s.add_validation_material(&material),
        Err(EditError::DssUnderNoChangesCertification)
    ));
    let (out, report) = add(&base, &material.allow_under_no_changes_certification(true)).unwrap();
    assert_eq!(report.crls_added, 1);
    only_verdict(&out);

    request.certify = Some(MdpPermission::FormFillAndSign);
    let base = signed_with(&request);
    let (_, report) = add(
        &base,
        &ValidationMaterial::new().with_crl(fixture("crl-clear.crl")),
    )
    .unwrap();
    assert_eq!(report.crls_added, 1, "P=2 permits a DSS update");
}

#[test]
fn undo_removes_the_dss_update() {
    let base = signed();
    let mut s = EditSession::new(Document::from_bytes(base.clone()).unwrap());
    s.add_validation_material(&ValidationMaterial::new().with_crl(fixture("crl-clear.crl")))
        .unwrap();
    assert!(s.undo().is_some());
    let out = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    assert!(!String::from_utf8_lossy(&out).contains("/DSS"));
}

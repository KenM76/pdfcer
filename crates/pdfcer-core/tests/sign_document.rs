//! `Pass 10.8` + `Pass 10.9` — signing a document end to end, and proving
//! it with TWO independent verifiers.
//!
//! - **pdfcer's own** (`signature_verify`): a separate implementation of the
//!   arithmetic (decision 129) reading the file the way any other reader
//!   would — `/ByteRange`, `/Contents`, the CMS, the `0x31` retag.
//! - **OpenSSL** (`openssl cms -verify`): a third party's CMS parser and
//!   verifier, fed the exact `/ByteRange` spans as detached content. This is
//!   the check a home-grown DER writer most needs — X.690 §11.6 `SET OF`
//!   ordering and the `signing-certificate-v2` shape are judged by strangers.
//!
//! Plus the invariant that makes signing worth having: **a second signature
//! appends after the first and the first still verifies**, byte for byte.

// The whole file needs the `signing` feature (default on); a
// `--no-default-features` build compiles it to nothing rather than failing.
#![cfg(feature = "signing")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::sign::apply::{SignApplyError, SignRequest};
use pdfcer_core::sign::cms_build::SubFilter;
use pdfcer_core::sign::pkcs12::Pkcs12Signer;
use pdfcer_core::sign::{SignatureAlgorithm, Signer};
use pdfcer_core::signature_verify::{Integrity, verify_all};
use pdfcer_core::writer::SaveOptions;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic")
}

fn pfx(name: &str) -> Pkcs12Signer {
    let bytes = std::fs::read(fixtures().join("signing").join(name)).unwrap();
    Pkcs12Signer::from_der(&bytes, "pdfcer").unwrap()
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixtures().join(rel)).unwrap())
}

const T0: &str = "D:20260905120000Z";

/// Sign `hello.pdf` with `signer`, return the bytes and report.
fn sign_hello(
    signer: &dyn Signer,
    request: &SignRequest,
) -> (Vec<u8>, pdfcer_core::sign::apply::SignReport) {
    let mut s = session("hello.pdf");
    s.sign(signer, request, &SaveOptions::identity())
        .expect("sign")
}

/// The signature's DER `/Contents`, with the zero padding removed (the outer
/// SEQUENCE's own length says where the blob ends), and the two `/ByteRange`
/// spans concatenated — what an external verifier needs.
fn cms_and_content(bytes: &[u8], field_name: &str) -> (Vec<u8>, Vec<u8>) {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let v = verify_all(&doc.view(), bytes)
        .into_iter()
        .find(|v| v.field_name.as_deref() == Some(field_name))
        .expect("verdict for the field");
    let ranges = &v.coverage.ranges;
    let mut content = Vec::new();
    for (start, len) in ranges {
        let (s, l) = (*start as usize, *len as usize);
        content.extend_from_slice(&bytes[s..s + l]);
    }
    // /Contents: find the sig dict through the AcroForm field.
    let form = pdfcer_core::forms::parse_acroform(&doc.view()).unwrap();
    let field = form
        .fields
        .iter()
        .find(|f| f.fully_qualified_name == field_name)
        .unwrap();
    let sig_dict = doc
        .view()
        .resolve(&Object::Reference(field.id))
        .as_dict()
        .and_then(|d| d.get(b"V").cloned())
        .map(|v| doc.view().resolve(&v).clone())
        .unwrap();
    let contents = match sig_dict.as_dict().and_then(|d| d.get(b"Contents")) {
        Some(Object::String(s)) => s.clone(),
        other => panic!("no /Contents: {other:?}"),
    };
    // Trim the hex-zero padding: the DER ContentInfo's outer length.
    let (tlv_len, hdr) = der_outer_len(&contents);
    (contents[..hdr + tlv_len].to_vec(), content)
}

fn der_outer_len(der: &[u8]) -> (usize, usize) {
    assert_eq!(der[0], 0x30);
    let first = der[1] as usize;
    if first < 0x80 {
        (first, 2)
    } else {
        let n = first & 0x7F;
        let mut len = 0usize;
        for i in 0..n {
            len = (len << 8) | der[2 + i] as usize;
        }
        (len, 2 + n)
    }
}

fn openssl_verifies(cms_der: &[u8], content: &[u8], cert_der: &[u8]) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("pdfcer-sign-oracle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sig = dir.join("sig.der");
    let data = dir.join("content.bin");
    std::fs::write(&sig, cms_der).unwrap();
    std::fs::write(&data, content).unwrap();
    // -noverify: check the SIGNATURE and digest, not the (self-signed, untrusted)
    // chain — this test is about the bytes pdfcer wrote, not about trust. The
    // signer certificate comes from the SignedData itself (PAdES a), which is
    // also what proves pdfcer embedded it; `cert_der` is kept for the message.
    let _ = cert_der;
    let out = std::process::Command::new("openssl")
        .args(["cms", "-verify", "-noverify", "-binary", "-inform", "DER"])
        .arg("-in")
        .arg(&sig)
        .arg("-content")
        .arg(&data)
        .arg("-out")
        .arg(dir.join("out.bin"))
        .output()
        .map_err(|e| format!("openssl not runnable: {e} — the oracle needs OpenSSL on PATH"))?;
    let _ = std::fs::remove_dir_all(&dir);
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

fn verified(bytes: &[u8], field: &str) -> pdfcer_core::signature_verify::SignatureVerdict {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    verify_all(&doc.view(), bytes)
        .into_iter()
        .find(|v| v.field_name.as_deref() == Some(field))
        .expect("the field's verdict")
}

// ---------------------------------------------------------------------------

#[test]
fn rsa_pkcs1_signs_hello_and_both_verifiers_accept_it() {
    let signer = pfx("rsa2048-modern.pfx");
    let (bytes, report) = sign_hello(&signer, &SignRequest::at(T0));
    assert_eq!(report.field_name, "Signature1");
    assert_eq!(report.algorithm, SignatureAlgorithm::RsaPkcs1v15Sha256);
    assert_eq!(report.sub_filter, SubFilter::EtsiCadesDetached);
    assert_eq!(report.pades_level, "B-B");
    assert!(report.self_verified);
    assert_eq!(report.prior_signatures, 0);
    assert_eq!(report.certificates, 1);
    assert!(
        report
            .signer_subject
            .contains("pdfcer synthetic RSA signer"),
        "{}",
        report.signer_subject
    );
    assert!(
        report.cms_bytes < report.reserved_bytes,
        "{} < {}",
        report.cms_bytes,
        report.reserved_bytes
    );
    assert_eq!(
        report.byte_range[2] + report.byte_range[3],
        bytes.len() as u64,
        "ByteRange reaches EOF"
    );

    // The original file is an untouched prefix — incremental update.
    let original = std::fs::read(fixtures().join("hello.pdf")).unwrap();
    assert_eq!(
        &bytes[..original.len()],
        &original[..],
        "the base file is byte-identical"
    );

    let v = verified(&bytes, "Signature1");
    assert!(
        matches!(v.integrity, Integrity::Verified { .. }),
        "{:?}",
        v.integrity
    );
    assert!(v.coverage.covers_to_eof());
    assert_eq!(v.sub_filter.as_deref(), Some("ETSI.CAdES.detached"));
    assert_eq!(
        v.date.as_deref(),
        Some(T0),
        "/M is the verdict's `date`; PAdES writes no CMS signing-time"
    );
    assert_eq!(v.signing_time, None, "no CMS signing-time attribute (PC-3)");

    let (cms, content) = cms_and_content(&bytes, "Signature1");
    assert_eq!(cms.len(), report.cms_bytes);
    openssl_verifies(&cms, &content, &signer.certificate_chain()[0])
        .expect("OpenSSL accepts the CMS");
}

#[test]
fn rsa_pss_and_ecdsa_also_round_trip_through_both_verifiers() {
    let rsa = pfx("rsa2048-modern.pfx");
    let ec = pfx("ecp256-modern.pfx");
    for (signer, alg) in [
        (&rsa as &dyn Signer, Some(SignatureAlgorithm::RsaPssSha256)),
        (&ec as &dyn Signer, None),
    ] {
        let mut req = SignRequest::at(T0);
        req.algorithm = alg;
        req.sub_filter = SubFilter::AdbePkcs7Detached;
        req.reason = Some("round trip".to_owned());
        req.location = Some("the test suite".to_owned());
        let (bytes, report) = sign_hello(signer, &req);
        let v = verified(&bytes, "Signature1");
        assert!(
            matches!(v.integrity, Integrity::Verified { .. }),
            "{:?}: {:?}",
            report.algorithm,
            v.integrity
        );
        assert_eq!(v.sub_filter.as_deref(), Some("adbe.pkcs7.detached"));
        assert_eq!(v.reason.as_deref(), Some("round trip"));
        assert_eq!(v.location.as_deref(), Some("the test suite"));
        let (cms, content) = cms_and_content(&bytes, "Signature1");
        openssl_verifies(&cms, &content, &signer.certificate_chain()[0])
            .unwrap_or_else(|e| panic!("{:?}: OpenSSL rejected the CMS: {e}", report.algorithm));
    }
    // ECDSA picked its own default.
    let (_, report) = sign_hello(&ec, &SignRequest::at(T0));
    assert_eq!(report.algorithm, SignatureAlgorithm::EcdsaP256Sha256);
}

#[test]
fn a_second_signature_appends_and_the_first_still_verifies() {
    let rsa = pfx("rsa2048-modern.pfx");
    let ec = pfx("ecp256-modern.pfx");
    let (first, r1) = sign_hello(&rsa, &SignRequest::at(T0));

    let mut s2 = EditSession::new(Document::from_bytes(first.clone()).unwrap());
    let (second, r2) = s2
        .sign(
            &ec,
            &SignRequest::at("D:20260905130000Z"),
            &SaveOptions::identity(),
        )
        .unwrap();
    assert_eq!(r2.field_name, "Signature2", "the name does not collide");
    assert_eq!(r2.prior_signatures, 1);
    assert_eq!(
        &second[..first.len()],
        &first[..],
        "the first signed file is an untouched prefix"
    );

    let doc = Document::from_bytes(second.clone()).unwrap();
    let verdicts = verify_all(&doc.view(), &second);
    assert_eq!(verdicts.len(), 2);
    for v in &verdicts {
        assert!(
            matches!(v.integrity, Integrity::Verified { .. }),
            "{:?}: {:?}",
            v.field_name,
            v.integrity
        );
    }
    let v1 = verdicts
        .iter()
        .find(|v| v.field_name.as_deref() == Some("Signature1"))
        .unwrap();
    let v2 = verdicts
        .iter()
        .find(|v| v.field_name.as_deref() == Some("Signature2"))
        .unwrap();
    assert!(
        !v1.coverage.covers_to_eof(),
        "the first signature covers only its own revision now"
    );
    assert!(v2.coverage.covers_to_eof());
    assert_eq!(v1.coverage.file_len, v2.coverage.file_len);
    let _ = r1;
}

#[test]
fn refusals_are_named_and_write_nothing() {
    let rsa = pfx("rsa2048-modern.pfx");
    let mut s = session("hello.pdf");
    let depth = s.undo_depth();
    let err = s
        .sign(
            &rsa,
            &SignRequest::at("2026-09-05"),
            &SaveOptions::identity(),
        )
        .expect_err("not a PDF date");
    assert!(
        matches!(err, SignApplyError::SigningTimeNotPdfDate { .. }),
        "{err:?}"
    );
    assert_eq!(s.undo_depth(), depth, "a refusal stages nothing");

    // A too-small reservation is refused by name with both numbers, and the
    // staged command is the only trace (the bytes are not returned).
    let mut req = SignRequest::at(T0);
    req.reserve = 64;
    let err = s
        .sign(&rsa, &req, &SaveOptions::identity())
        .expect_err("64 bytes cannot hold an RSA-2048 CMS");
    assert!(
        matches!(
            err,
            SignApplyError::ReservationTooSmall { reserved: 64, .. }
        ),
        "{err:?}"
    );

    // Field-name collision.
    let (first, _) = sign_hello(&rsa, &SignRequest::at(T0));
    let mut s2 = EditSession::new(Document::from_bytes(first).unwrap());
    let mut req = SignRequest::at(T0);
    req.field_name = Some("Signature1".to_owned());
    let err = s2
        .sign(&rsa, &req, &SaveOptions::identity())
        .expect_err("taken");
    assert!(
        matches!(err, SignApplyError::FieldNameTaken { .. }),
        "{err:?}"
    );

    // Page out of range for a visible signature.
    let mut req = SignRequest::at(T0);
    req.visible = Some((
        7,
        pdfcer_core::page_tree::Rect {
            llx: 10.0,
            lly: 10.0,
            urx: 200.0,
            ury: 60.0,
        },
    ));
    let mut s3 = session("hello.pdf");
    let err = s3
        .sign(&rsa, &req, &SaveOptions::identity())
        .expect_err("no page 8");
    assert!(
        matches!(err, SignApplyError::PageOutOfRange { page: 7, .. }),
        "{err:?}"
    );
}

#[test]
fn a_visible_signature_gets_a_widget_with_a_rect_and_an_appearance() {
    let rsa = pfx("rsa2048-modern.pfx");
    let mut req = SignRequest::at(T0);
    req.visible = Some((
        0,
        pdfcer_core::page_tree::Rect {
            llx: 100.0,
            lly: 100.0,
            urx: 300.0,
            ury: 160.0,
        },
    ));
    req.name = Some("Ken".to_owned());
    let (bytes, report) = sign_hello(&rsa, &req);
    let v = verified(&bytes, "Signature1");
    assert!(
        matches!(v.integrity, Integrity::Verified { .. }),
        "{:?}",
        v.integrity
    );
    assert_eq!(v.name.as_deref(), Some("Ken"));
    let doc = Document::from_bytes(bytes).unwrap();
    let slots = pdfcer_core::page_tree::page_slots(&doc.view()).unwrap();
    let annots = pdfcer_core::annot::page_annotations(&doc.view(), slots[0].id);
    let widget = annots
        .iter()
        .find(|a| a.subtype == b"Widget")
        .expect("the signature widget is on page 1");
    let r = widget.rect.unwrap();
    assert_eq!((r.llx, r.lly, r.urx, r.ury), (100.0, 100.0, 300.0, 160.0));
    assert!(
        matches!(
            widget.appearance,
            pdfcer_core::annot::Appearance::Normal { .. }
        ),
        "a visible signature has an /AP: {:?}",
        widget.appearance
    );
    assert_eq!(report.field_name, "Signature1");
}

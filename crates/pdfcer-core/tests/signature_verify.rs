//! # `signature::verify` against signatures pdfcer did not produce
//!
//! The fixtures under `fixtures/synthetic/signature-verify/` were signed by
//! pyHanko (an independent implementation of ISO 32000-1 §12.8.3 and ETSI
//! EN 319 142-1) with self-signed keys generated for the purpose — see
//! `tools/gen-signed-fixtures.py` and the `PROVENANCE.md` beside them. Each
//! file's expected verdict was recorded from pyHanko's own validator before
//! pdfcer's was written, so agreement here is two implementations of the
//! same clauses agreeing, not pdfcer agreeing with itself.
//!
//! The four outcomes the fixtures distinguish, and why each is its own
//! file: a valid signature; the same document with one byte changed INSIDE
//! the signed range (pyHanko: `intact=False, valid=True` — the digest
//! fails, the signature over the attributes still verifies); the same
//! document with one hex digit changed in the signature VALUE (pyHanko:
//! `intact=True, valid=False` — the digest matches, the signature does
//! not); and the same document with an incremental update appended after
//! signing (both true, coverage `ENTIRE_REVISION` not `ENTIRE_FILE`). A
//! verifier that collapsed integrity into one bool could not tell the
//! second from the third, and the operator needs to.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::signature::{self, Integrity, Trust};

fn load(name: &str) -> (Document, Vec<u8>) {
    let path = format!(
        "{}/../../fixtures/synthetic/signature-verify/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    (
        Document::from_bytes(bytes.clone()).expect("fixture parses"),
        bytes,
    )
}

fn verdict(name: &str) -> signature::SignatureVerdict {
    let (doc, bytes) = load(name);
    let all = signature::verify_all(&doc.view(), &bytes);
    assert_eq!(all.len(), 1, "{name}: one signature field");
    all.into_iter().next().unwrap()
}

#[test]
fn an_rsa_pkcs7_detached_signature_verifies_with_full_coverage() {
    let v = verdict("sig-rsa-pkcs7-detached.pdf");
    assert_eq!(v.sub_filter.as_deref(), Some("adbe.pkcs7.detached"));
    match &v.integrity {
        Integrity::Verified {
            digest_algorithm,
            signature_algorithm,
        } => {
            assert_eq!(*digest_algorithm, "SHA-256");
            assert!(
                signature_algorithm.starts_with("RSA PKCS#1 v1.5, 2048-bit"),
                "{signature_algorithm}"
            );
        }
        other => panic!("expected Verified, got {other:?} (notes {:?})", v.notes),
    }
    assert!(v.coverage.covers_to_eof());
    assert!(v.coverage.ranges_well_formed);
    assert_eq!(v.trust, Trust::NotChecked);
    assert!(
        v.signer_subject
            .as_deref()
            .unwrap()
            .contains("CN=pdfce fixture RSA signer"),
        "{:?}",
        v.signer_subject
    );
    assert!(
        v.cert_not_before
            .as_deref()
            .unwrap()
            .starts_with("2026-01-01")
    );
    assert_eq!(v.field_name.as_deref(), Some("Sig1"));
    assert_eq!(v.reason.as_deref(), Some("pdfce verification fixture"));
    assert!(
        v.signing_time.is_some(),
        "pyHanko signs a signingTime attribute"
    );
}

#[test]
fn a_byte_changed_inside_the_range_is_a_digest_mismatch_not_an_invalid_signature() {
    let v = verdict("sig-rsa-tampered.pdf");
    assert_eq!(v.integrity, Integrity::DigestMismatch, "{v:?}");
    assert!(v.coverage.covers_to_eof());
}

#[test]
fn a_changed_signature_value_is_invalid_while_the_digest_still_matches() {
    let v = verdict("sig-rsa-contents-tampered.pdf");
    assert_eq!(v.integrity, Integrity::SignatureInvalid, "{v:?}");
}

#[test]
fn an_update_appended_after_signing_verifies_but_does_not_cover_the_file() {
    let v = verdict("sig-rsa-appended.pdf");
    assert!(matches!(v.integrity, Integrity::Verified { .. }), "{v:?}");
    assert!(!v.coverage.covers_to_eof());
    assert!(v.coverage.uncovered_tail > 0);
}

#[test]
fn a_cades_rsa_pss_signature_verifies() {
    let v = verdict("sig-rsa-pss-cades.pdf");
    assert_eq!(v.sub_filter.as_deref(), Some("ETSI.CAdES.detached"));
    match &v.integrity {
        Integrity::Verified {
            signature_algorithm,
            ..
        } => assert!(
            signature_algorithm.starts_with("RSASSA-PSS (SHA-256, MGF1-SHA-256"),
            "{signature_algorithm}"
        ),
        other => panic!("expected Verified, got {other:?} (notes {:?})", v.notes),
    }
}

#[test]
fn a_cades_ecdsa_p256_signature_verifies() {
    let v = verdict("sig-ecdsa-p256-cades.pdf");
    match &v.integrity {
        Integrity::Verified {
            signature_algorithm,
            ..
        } => assert_eq!(signature_algorithm, "ECDSA P-256"),
        other => panic!("expected Verified, got {other:?} (notes {:?})", v.notes),
    }
    assert!(
        v.signer_subject
            .as_deref()
            .unwrap()
            .contains("ECDSA signer")
    );
}

#[test]
fn a_sha1_signature_verifies_and_is_disclosed_as_weak() {
    let v = verdict("sig-rsa-sha1-pkcs7.pdf");
    match &v.integrity {
        Integrity::Verified {
            digest_algorithm, ..
        } => assert_eq!(*digest_algorithm, "SHA-1"),
        other => panic!("expected Verified, got {other:?} (notes {:?})", v.notes),
    }
    assert!(v.notes.iter().any(|n| n.contains("SHA-1")), "{:?}", v.notes);
}

#[test]
fn the_coverage_fixtures_with_filler_contents_are_unverifiable_by_name() {
    // The no-cryptography coverage fixtures carry zeros for /Contents.
    let path = format!(
        "{}/../../fixtures/synthetic/signature/signed-full-coverage.pdf",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(path).unwrap();
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    let all = signature::verify_all(&doc.view(), &bytes);
    assert_eq!(all.len(), 1);
    match &all[0].integrity {
        Integrity::Unverifiable { reason } => {
            assert!(reason.contains("not a CMS SignedData"), "{reason}");
        }
        other => panic!("filler contents must be unverifiable, got {other:?}"),
    }
    assert!(all[0].coverage.covers_to_eof());
}

#[test]
fn verify_by_index_matches_verify_all() {
    let (doc, bytes) = load("sig-rsa-pkcs7-detached.pdf");
    let one = signature::verify(&doc.view(), &bytes, 0).unwrap();
    assert_eq!(one, signature::verify_all(&doc.view(), &bytes)[0]);
    assert!(signature::verify(&doc.view(), &bytes, 1).is_none());
}

/// `Pass 10.3` — the `anchors` parameter flips trust from NotChecked to a real
/// verdict. With `None`, trust is `NotChecked` (unchanged). With an EMPTY trust
/// store, the signer does not chain to anything, so trust is NOT `NotChecked`
/// — it is `Untrusted` or `SignerUnknown`, never a false `Trusted`. (The
/// `Trusted` path is proven by the `trust_chain` unit tests against a synthetic
/// root→leaf chain.)
#[test]
fn trust_anchors_flip_the_verdict_and_an_empty_store_is_never_trusted() {
    use pdfcer_core::trust_store::TrustAnchorSet;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/signature-verify/sig-ecdsa-p256-cades.pdf");
    let bytes = std::fs::read(&path).expect("read signed fixture");
    let doc = Document::from_bytes(bytes.clone()).expect("load signed fixture");

    // Default (no anchors): NotChecked.
    let plain = signature::verify_all(&doc.view(), &bytes);
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].trust, Trust::NotChecked);

    // Empty anchor store: trust WAS evaluated, and the signer chains to nothing.
    let empty = TrustAnchorSet::default();
    let checked = signature::verify_all_with_trust(&doc.view(), &bytes, Some(&empty));
    assert_eq!(checked.len(), 1);
    assert_ne!(
        checked[0].trust,
        Trust::NotChecked,
        "supplying anchors must evaluate trust, not leave it NotChecked"
    );
    assert!(
        matches!(
            checked[0].trust,
            Trust::Untrusted { .. } | Trust::SignerUnknown
        ),
        "an empty store yields Untrusted/SignerUnknown, never a false Trusted: {:?}",
        checked[0].trust
    );
}

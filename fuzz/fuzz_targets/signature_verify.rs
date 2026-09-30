//! Fuzz target: signature verification (`pdfcer_core::signature::verify_all`,
//! Pass 10.1) over arbitrary bytes.
//!
//! The document is loaded from the fuzz input and every signature field it
//! carries is verified against those same bytes. That drives, with hostile
//! input: the `/ByteRange` geometry checks, the hex-hole decode, the DER
//! reader (`asn1`), the CMS `SignedData` and X.509 walkers (`cms`), the
//! `Uint` arithmetic (`crypto::bignum` — Knuth division on attacker-chosen
//! limb patterns), RSA PKCS#1 v1.5 / PSS and ECDSA P-256 / P-384
//! verification on attacker-chosen keys and signature values, and SHA-1.
//!
//! Invariant: for ANY input the call returns a `Vec<SignatureVerdict>` and
//! never panics, aborts or loops. A second invariant, checked on the way
//! past: a verdict of `Verified` is never produced for a document whose
//! `/ByteRange` is malformed (`ranges_well_formed == false`) — the geometry
//! gate is upstream of every cryptographic check, and a mutation that
//! reached `Verified` around it would be the forgery shape the pipeline
//! exists to refuse.
//!
//! A third: every revocation-location list a verdict carries is within
//! `MAX_REVOCATION_URIS` (a hostile certificate cannot grow it unbounded).
//!
//! Seeds: `fuzz/corpus/signature_verify/seed_*.pdf` — the seven
//! pyHanko-signed fixtures, plus `seed_sig-ecdsa-revocation-uris.pdf`
//! (`pdfcer sign` with `revocation-ecp256-modern.pfx`, whose certificate
//! carries cRLDistributionPoints and authorityInfoAccess).

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::signature::{self, Integrity};

/// `pdfcer_pkix::cms::MAX_REVOCATION_URIS`, the documented per-list cap on
/// `RevocationSources` (the fuzz crate does not depend on pdfcer-pkix).
const MAX_REVOCATION_URIS: usize = 16;

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::from_bytes(data.to_vec()) else {
        return;
    };
    for v in signature::verify_all(&doc.view(), data) {
        if !v.coverage.ranges_well_formed {
            assert!(
                !matches!(v.integrity, Integrity::Verified { .. }),
                "a malformed /ByteRange can never verify"
            );
        }
        for s in &v.revocation_sources {
            for list in [&s.crl, &s.ocsp, &s.ca_issuers] {
                assert!(list.len() <= MAX_REVOCATION_URIS, "revocation list over the cap");
            }
        }
    }
});

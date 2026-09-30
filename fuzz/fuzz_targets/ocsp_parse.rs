//! Fuzz target: the RFC 6960 OCSP response reader and check
//! (`pdfcer_pkix::ocsp`) over arbitrary bytes.
//!
//! The input is parsed as an `OCSPResponse` (or a bare `BasicOCSPResponse`);
//! whatever parses is checked against the synthetic leaf and CA from
//! `fixtures/synthetic/ocsp/`, with the CA's clearing CRL available for a
//! delegated responder, and the input is also fed to the chain walk as the
//! only OCSP response. That drives the DER reader, the CertID matcher, the
//! responder-authorisation walk (EKU, nocheck, the CRL fallback), the time
//! comparisons and ECDSA verification on attacker-chosen response bytes.
//!
//! Invariants: no panic, abort or loop; a parsed response holds at most
//! `MAX_OCSP_ITEMS` single responses and certificates; and nothing is
//! concluded (`NotRevoked` or `Revoked`, alone or through the chain walk)
//! from a response whose signed `ResponseData` is not one a fixture key
//! really signed. A fuzzer cannot sign as the CA or a responder, so any
//! other conclusion is a forgery accepted.
//!
//! Seeds: `fuzz/corpus/ocsp_parse/seed_*.der` — the fixture responses.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_pkix::cms::parse_certificate;
use pdfcer_pkix::crl::parse_crl;
use pdfcer_pkix::ocsp::{self, OcspStatus};
use pdfcer_pkix::revocation::{ChainRevocation, chain_status};

const LEAF: &[u8] = include_bytes!("../../fixtures/synthetic/ocsp/leaf.cer");
const CA: &[u8] = include_bytes!("../../fixtures/synthetic/ocsp/ca.cer");
const CRL: &[u8] = include_bytes!("../../fixtures/synthetic/ocsp/crl-clear.crl");
/// Every fixture response a real fixture key signed; `ocsp-forged` is not.
const SIGNED: [&[u8]; 11] = [
    include_bytes!("../../fixtures/synthetic/ocsp/ocsp-good.der"),
    include_bytes!("../../fixtures/synthetic/ocsp/ocsp-good-delegated.der"),
    include_bytes!("../../fixtures/synthetic/ocsp/ocsp-good-sha256.der"),
    include_bytes!("../../fixtures/synthetic/ocsp/ocsp-good-checked.der"),
    include_bytes!("../../fixtures/synthetic/ocsp/ocsp-basic-only.der"),
    include_bytes!("../../fixtures/synthetic/ocsp/ocsp-revoked.der"),
    include_bytes!("../../fixtures/synthetic/ocsp/ocsp-revoked-after.der"),
    include_bytes!("../../fixtures/synthetic/ocsp/ocsp-unknown.der"),
    include_bytes!("../../fixtures/synthetic/ocsp/ocsp-stale.der"),
    include_bytes!("../../fixtures/synthetic/ocsp/ocsp-noeku.der"),
    include_bytes!("../../fixtures/synthetic/ocsp/ocsp-other-serial.der"),
];
const AT: Option<&str> = Some("2026-09-30T00:00:00Z");

/// Whether `data` parses to a response whose signed bytes a fixture key
/// really signed.
fn genuinely_signed(data: &[u8]) -> bool {
    let Some(parsed) = ocsp::parse_ocsp(data) else {
        return false;
    };
    SIGNED
        .iter()
        .any(|real| ocsp::parse_ocsp(real).is_some_and(|r| r.tbs_der == parsed.tbs_der))
}

fuzz_target!(|data: &[u8]| {
    let leaf = parse_certificate(LEAF).expect("fixture leaf");
    let ca = parse_certificate(CA).expect("fixture CA");
    let crls = [parse_crl(CRL).expect("fixture CRL")];
    if let Some(parsed) = ocsp::parse_ocsp(data) {
        assert!(parsed.responses.len() <= ocsp::MAX_OCSP_ITEMS);
        assert!(parsed.certs.len() <= ocsp::MAX_OCSP_ITEMS);
        let status = ocsp::check_ocsp(&parsed, &leaf, &ca, AT, &crls);
        assert!(
            matches!(status, OcspStatus::Unusable { .. }) || genuinely_signed(data),
            "a response no fixture key signed proved something: {status:?}"
        );
    }
    let chain = chain_status(LEAF, &[LEAF, CA], &[], &[CRL], &[data], AT);
    assert!(
        !matches!(chain, ChainRevocation::Revoked { .. }) || genuinely_signed(data),
        "{chain:?}"
    );
    // The CA's own CRL covers the leaf, so the walk is `NotRevoked` unless a
    // genuine response says revoked.
    assert!(
        matches!(chain, ChainRevocation::NotRevoked(_)) || genuinely_signed(data),
        "{chain:?}"
    );
});

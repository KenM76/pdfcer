//! Fuzz target: the RFC 5280 §5 CRL reader and revocation check
//! (`pdfcer_pkix::crl`) over arbitrary bytes.
//!
//! The input is parsed as a `CertificateList`; whatever parses is checked
//! against the synthetic leaf and CA from `fixtures/synthetic/crl/`, and the
//! input is also fed to the chain walk as the only CRL. That drives the DER
//! reader, the extension and entry walkers, the IDP decoder, the time
//! comparisons and ECDSA verification on attacker-chosen CRL bytes.
//!
//! Invariants: no panic, abort or loop; a parsed CRL holds at most
//! `MAX_CRL_ENTRIES` entries; and nothing is concluded (`NotRevoked` or
//! `Revoked`, alone or through the chain walk) from a CRL whose to-be-signed
//! bytes are not one of the fixture CRLs the CA really signed. A fuzzer
//! cannot sign as the CA, so any other conclusion is a forgery accepted.
//!
//! Seeds: `fuzz/corpus/crl_parse/seed_*.crl` — the fixture CRLs.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_pkix::cms::parse_certificate;
use pdfcer_pkix::crl::{self, CrlStatus};
use pdfcer_pkix::revocation::{ChainRevocation, chain_status};

const LEAF: &[u8] = include_bytes!("../../fixtures/synthetic/crl/leaf.cer");
const CA: &[u8] = include_bytes!("../../fixtures/synthetic/crl/ca.cer");
/// Every fixture CRL the CA signed; the others are forged or foreign.
const CA_SIGNED: [&[u8]; 8] = [
    include_bytes!("../../fixtures/synthetic/crl/crl-good.crl"),
    include_bytes!("../../fixtures/synthetic/crl/crl-revoked.crl"),
    include_bytes!("../../fixtures/synthetic/crl/crl-revoked-after.crl"),
    include_bytes!("../../fixtures/synthetic/crl/crl-stale.crl"),
    include_bytes!("../../fixtures/synthetic/crl/crl-critical.crl"),
    include_bytes!("../../fixtures/synthetic/crl/crl-delta.crl"),
    include_bytes!("../../fixtures/synthetic/crl/crl-idp-user.crl"),
    include_bytes!("../../fixtures/synthetic/crl/crl-idp-other.crl"),
];
const AT: Option<&str> = Some("2026-09-30T00:00:00Z");

/// Whether `data` parses to a CRL whose signed bytes the CA really signed.
fn genuinely_signed(data: &[u8]) -> bool {
    let Some(parsed) = crl::parse_crl(data) else {
        return false;
    };
    CA_SIGNED
        .iter()
        .any(|real| crl::parse_crl(real).is_some_and(|r| r.tbs_der == parsed.tbs_der))
}

fuzz_target!(|data: &[u8]| {
    let leaf = parse_certificate(LEAF).expect("fixture leaf");
    let ca = parse_certificate(CA).expect("fixture CA");
    if let Some(parsed) = crl::parse_crl(data) {
        assert!(parsed.entries.len() <= crl::MAX_CRL_ENTRIES);
        let status = crl::check_crl(&parsed, &leaf, &ca, AT);
        assert!(
            matches!(status, CrlStatus::Unusable { .. }) || genuinely_signed(data),
            "a CRL the CA did not sign proved something: {status:?}"
        );
    }
    let chain = chain_status(LEAF, &[LEAF, CA], &[], &[data], &[], AT);
    assert!(
        matches!(chain, ChainRevocation::Undetermined { .. }) || genuinely_signed(data),
        "{chain:?}"
    );
});

//! pyca/cryptography-minted OCSP responses read back and checked against a
//! leaf and its CA (RFC 6960; fixtures from `tools/gen-ocsp-fixtures.py`).

use std::path::PathBuf;

use pdfcer_pkix::cms::parse_certificate;
use pdfcer_pkix::crl::{CrlReason, parse_crl};
use pdfcer_pkix::ocsp::{OcspStatus, ResponderId, check_ocsp, parse_ocsp};
use pdfcer_pkix::revocation::{ChainRevocation, Evidence, chain_status};

fn fixture(name: &str) -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/ocsp");
    std::fs::read(root.join(name)).unwrap()
}

const SIGNED_AT: &str = "2026-09-30T00:00:00Z";

/// What `resp` says about the leaf, with `crls` available for a delegate.
fn status_with(resp: &str, crls: &[&str]) -> OcspStatus {
    let (der, leaf_der, ca_der) = (fixture(resp), fixture("leaf.cer"), fixture("ca.cer"));
    let resp = parse_ocsp(&der).expect("an OCSP response");
    let leaf = parse_certificate(&leaf_der).unwrap();
    let ca = parse_certificate(&ca_der).unwrap();
    let crl_ders: Vec<Vec<u8>> = crls.iter().map(|c| fixture(c)).collect();
    let crls: Vec<_> = crl_ders.iter().map(|d| parse_crl(d).unwrap()).collect();
    check_ocsp(&resp, &leaf, &ca, Some(SIGNED_AT), &crls)
}

fn status(resp: &str) -> OcspStatus {
    status_with(resp, &[])
}

fn unusable_because(s: OcspStatus, needle: &str) {
    match s {
        OcspStatus::Unusable { reason } => assert!(reason.contains(needle), "{reason}"),
        other => panic!("expected unusable ({needle}), got {other:?}"),
    }
}

fn is_good(s: &OcspStatus) -> bool {
    matches!(s, OcspStatus::NotRevoked { .. })
}

#[test]
fn a_response_reads_its_responder_and_single_response() {
    let der = fixture("ocsp-good.der");
    let r = parse_ocsp(&der).unwrap();
    assert_eq!(r.status, 0);
    assert!(matches!(r.responder, Some(ResponderId::ByName(_))));
    assert_eq!(r.responses.len(), 1);
    assert_eq!(
        r.responses[0].this_update.as_deref(),
        Some("2026-09-20T00:00:00Z")
    );
    assert_eq!(
        r.responses[0].next_update.as_deref(),
        Some("2099-01-01T00:00:00Z")
    );
    let delegated = fixture("ocsp-good-delegated.der");
    let d = parse_ocsp(&delegated).unwrap();
    assert!(
        matches!(d.responder, Some(ResponderId::ByKey(k)) if k.len() == 20),
        "byKey is [2] EXPLICIT OCTET STRING: {:?}",
        d.responder
    );
    assert_eq!(d.certs.len(), 1);
}

#[test]
fn a_good_answer_from_the_ca_says_not_revoked() {
    let s = status("ocsp-good.der");
    assert!(is_good(&s), "{s:?}");
}

#[test]
fn a_sha256_cert_id_matches_too() {
    let s = status("ocsp-good-sha256.der");
    assert!(is_good(&s), "{s:?}");
}

#[test]
fn a_bare_basic_response_is_read_as_legacy_dss_holds_it() {
    let s = status("ocsp-basic-only.der");
    assert!(is_good(&s), "{s:?}");
}

#[test]
fn a_delegated_responder_with_ocsp_signing_and_nocheck_is_accepted() {
    let s = status("ocsp-good-delegated.der");
    assert!(is_good(&s), "{s:?}");
}

#[test]
fn a_revoked_answer_carries_its_date_and_reason() {
    assert_eq!(
        status("ocsp-revoked.der"),
        OcspStatus::Revoked {
            date: Some("2026-09-15T00:00:00Z".to_owned()),
            reason: Some(CrlReason::KeyCompromise),
        }
    );
}

#[test]
fn unknown_is_not_good() {
    unusable_because(status("ocsp-unknown.der"), "does not know");
}

#[test]
fn a_forged_response_is_unusable() {
    unusable_because(
        status("ocsp-forged.der"),
        "does not verify with the issuer's key",
    );
}

#[test]
fn a_responder_without_ocsp_signing_cannot_vouch() {
    unusable_because(status("ocsp-noeku.der"), "not authorized");
}

#[test]
fn an_answer_for_another_certificate_does_not_apply() {
    unusable_because(status("ocsp-other-serial.der"), "has no answer for");
}

#[test]
fn a_response_expired_before_the_reference_time_is_not_used() {
    unusable_because(status("ocsp-stale.der"), "expired");
}

#[test]
fn an_unsuccessful_response_is_unsigned_and_unusable() {
    let der = fixture("ocsp-trylater.der");
    let r = parse_ocsp(&der).unwrap();
    assert_eq!(r.status, 3);
    unusable_because(status("ocsp-trylater.der"), "tryLater");
}

#[test]
fn a_delegate_without_nocheck_needs_a_crl_covering_it() {
    unusable_because(status("ocsp-good-checked.der"), "no usable CRL covers it");
    let s = status_with("ocsp-good-checked.der", &["crl-clear.crl"]);
    assert!(is_good(&s), "{s:?}");
    unusable_because(
        status_with("ocsp-good-checked.der", &["crl-responder-revoked.crl"]),
        "is revoked",
    );
}

fn chain(crls: &[&str], ocsps: &[&str]) -> ChainRevocation {
    let (leaf, ca) = (fixture("leaf.cer"), fixture("ca.cer"));
    let c: Vec<Vec<u8>> = crls.iter().map(|n| fixture(n)).collect();
    let o: Vec<Vec<u8>> = ocsps.iter().map(|n| fixture(n)).collect();
    let c: Vec<&[u8]> = c.iter().map(Vec::as_slice).collect();
    let o: Vec<&[u8]> = o.iter().map(Vec::as_slice).collect();
    chain_status(&leaf, &[&leaf, &ca], &[], &c, &o, Some(SIGNED_AT))
}

#[test]
fn an_ocsp_answer_covers_the_chain_and_is_named_as_ocsp() {
    match chain(&[], &["ocsp-forged.der", "ocsp-good.der"]) {
        ChainRevocation::NotRevoked(covered) => {
            assert_eq!(covered.len(), 1, "the root is not revocation-checked");
            assert_eq!(covered[0].evidence, Evidence::Ocsp(1));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_revoked_ocsp_answer_outranks_a_good_crl() {
    let got = chain(&["crl-clear.crl"], &["ocsp-revoked-after.der"]);
    assert!(
        matches!(
            got,
            ChainRevocation::Revoked {
                evidence: Evidence::Ocsp(0),
                before: Some(false),
                reason: Some(CrlReason::Superseded),
                ..
            }
        ),
        "{got:?}"
    );
}

#[test]
fn only_unusable_responses_leave_the_chain_undetermined_with_the_specific_reason() {
    match chain(&[], &["ocsp-other-serial.der", "ocsp-unknown.der"]) {
        ChainRevocation::Undetermined { reason } => {
            assert!(reason.contains("does not know"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn garbage_is_not_an_ocsp_response() {
    assert_eq!(parse_ocsp(b"not DER"), None);
    assert_eq!(
        parse_ocsp(&fixture("crl-clear.crl")),
        None,
        "a CRL is not a response"
    );
}

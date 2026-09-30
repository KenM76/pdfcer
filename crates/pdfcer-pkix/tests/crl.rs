//! pyca/cryptography-minted CRLs read back and checked against a leaf and
//! its CA (RFC 5280 §5; fixtures from `tools/gen-crl-fixtures.py`).

use std::path::PathBuf;

use pdfcer_pkix::cms::parse_certificate;
use pdfcer_pkix::crl::{CrlReason, CrlStatus, check_crl, parse_crl};
use pdfcer_pkix::revocation::{ChainRevocation, Evidence, chain_status};

fn fixture(name: &str) -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/crl");
    std::fs::read(root.join(name)).unwrap()
}

const SIGNED_AT: &str = "2026-09-30T00:00:00Z";

/// What `crl` says about the leaf, with `issuer` as its CA.
fn status(crl: &str, issuer: &str) -> CrlStatus {
    let (crl_der, leaf_der, ca_der) = (fixture(crl), fixture("leaf.cer"), fixture(issuer));
    let crl = parse_crl(&crl_der).expect("a CRL");
    let leaf = parse_certificate(&leaf_der).unwrap();
    let ca = parse_certificate(&ca_der).unwrap();
    check_crl(&crl, &leaf, &ca, Some(SIGNED_AT))
}

fn unusable_because(s: CrlStatus, needle: &str) {
    match s {
        CrlStatus::Unusable { reason } => assert!(reason.contains(needle), "{reason}"),
        other => panic!("expected unusable ({needle}), got {other:?}"),
    }
}

#[test]
fn a_crl_reads_its_issuer_dates_and_entries() {
    let der = fixture("crl-revoked.crl");
    let crl = parse_crl(&der).unwrap();
    assert!(crl.issuer.contains("pdfcer CRL test CA"), "{}", crl.issuer);
    assert_eq!(crl.this_update.as_deref(), Some("2026-09-20T00:00:00Z"));
    assert_eq!(crl.next_update.as_deref(), Some("2099-01-01T00:00:00Z"));
    assert_eq!(crl.entries.len(), 1);
    assert_eq!(crl.entries[0].serial, [0x10, 0x16]);
    assert_eq!(crl.entries[0].reason, Some(CrlReason::KeyCompromise));
    assert_eq!(crl.defect, None);
}

#[test]
fn a_crl_not_listing_the_leaf_says_not_revoked() {
    assert_eq!(status("crl-good.crl", "ca.cer"), CrlStatus::NotRevoked);
}

#[test]
fn a_crl_listing_the_leaf_says_revoked_with_date_and_reason() {
    assert_eq!(
        status("crl-revoked.crl", "ca.cer"),
        CrlStatus::Revoked {
            date: Some("2026-09-15T00:00:00Z".into()),
            reason: Some(CrlReason::KeyCompromise),
        }
    );
}

#[test]
fn a_forged_crl_is_unusable() {
    unusable_because(
        status("crl-forged.crl", "ca.cer"),
        "signature does not verify",
    );
}

#[test]
fn an_issuer_without_crl_sign_cannot_vouch() {
    unusable_because(
        status("crl-good.crl", "ca-nocrlsign.cer"),
        "not permitted to sign CRLs",
    );
}

#[test]
fn another_issuers_crl_does_not_apply() {
    unusable_because(
        status("crl-other-issuer.crl", "ca.cer"),
        "not the certificate's issuer",
    );
}

#[test]
fn an_unknown_critical_extension_makes_the_crl_unusable() {
    unusable_because(
        status("crl-critical.crl", "ca.cer"),
        "critical extension 1.3.6.1.4.1.55555.1",
    );
}

#[test]
fn a_delta_crl_is_not_used() {
    unusable_because(status("crl-delta.crl", "ca.cer"), "delta CRL");
}

#[test]
fn a_crl_expired_before_the_reference_time_is_not_used() {
    unusable_because(status("crl-stale.crl", "ca.cer"), "expired");
}

#[test]
fn a_user_only_partition_covers_the_leaf() {
    assert_eq!(status("crl-idp-user.crl", "ca.cer"), CrlStatus::NotRevoked);
}

#[test]
fn a_partition_the_leaf_does_not_name_is_not_used() {
    unusable_because(status("crl-idp-other.crl", "ca.cer"), "partitioned CRL");
}

fn chain(crls: &[&str]) -> ChainRevocation {
    let (leaf, ca) = (fixture("leaf.cer"), fixture("ca.cer"));
    let owned: Vec<Vec<u8>> = crls.iter().map(|c| fixture(c)).collect();
    let crls: Vec<&[u8]> = owned.iter().map(Vec::as_slice).collect();
    chain_status(&leaf, &[&leaf, &ca], &[], &crls, &[], Some(SIGNED_AT))
}

#[test]
fn the_chain_is_covered_up_to_the_self_signed_root() {
    match chain(&["crl-forged.crl", "crl-good.crl"]) {
        ChainRevocation::NotRevoked(covered) => {
            assert_eq!(covered.len(), 1, "the root is not revocation-checked");
            assert_eq!(covered[0].evidence, Evidence::Crl(1));
            assert!(covered[0].subject.contains("test signer"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_revocation_before_and_after_the_reference_time_is_reported_as_such() {
    let before = chain(&["crl-good.crl", "crl-revoked.crl"]);
    assert!(
        matches!(
            before,
            ChainRevocation::Revoked {
                before: Some(true),
                evidence: Evidence::Crl(1),
                ..
            }
        ),
        "{before:?}"
    );
    let after = chain(&["crl-revoked-after.crl"]);
    assert!(
        matches!(
            after,
            ChainRevocation::Revoked {
                before: Some(false),
                reason: Some(CrlReason::Superseded),
                ..
            }
        ),
        "{after:?}"
    );
}

#[test]
fn only_unusable_crls_leave_the_chain_undetermined() {
    match chain(&["crl-forged.crl", "crl-delta.crl"]) {
        ChainRevocation::Undetermined { reason } => {
            assert!(reason.contains("cannot be used"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    match chain(&["crl-other-issuer.crl"]) {
        ChainRevocation::Undetermined { reason } => {
            assert!(reason.contains("no CRL or OCSP response from"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_missing_issuer_leaves_the_chain_undetermined() {
    let leaf = fixture("leaf.cer");
    let crl = fixture("crl-good.crl");
    let got = chain_status(&leaf, &[], &[], &[&crl], &[], Some(SIGNED_AT));
    assert!(
        matches!(&got, ChainRevocation::Undetermined { reason } if reason.contains("not available")),
        "{got:?}"
    );
}

#[test]
fn garbage_is_not_a_crl() {
    assert_eq!(parse_crl(b"not DER"), None);
    assert_eq!(
        parse_crl(&fixture("leaf.cer")),
        None,
        "a certificate is not a CRL"
    );
}

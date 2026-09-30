//! A signature's chain checked against RFC 5280 CRLs: supplied by the caller,
//! or carried in the document's `/DSS` (ETSI EN 319 142-1 §5.4.2). Fixtures
//! from `tools/gen-crl-fixtures.py`; the signature claims 2026-09-30 in
//! `/M`, between the two revocation dates the CRLs state.

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
use pdfcer_core::sign::apply::SignRequest;
use pdfcer_core::sign::pkcs12::Pkcs12Signer;
use pdfcer_core::sign::{SignError, SignatureAlgorithm, Signer};
use pdfcer_core::signature::{
    Integrity, Revocation, RevocationSource, SignatureVerdict, SuppliedRevocation, Trust,
    verify_all, verify_all_with_revocation,
};
use pdfcer_core::trust_store::{TrustAnchor, TrustAnchorSet};
use pdfcer_core::writer::SaveOptions;

fn synthetic() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic")
}

fn crl_fixture(name: &str) -> Vec<u8> {
    std::fs::read(synthetic().join("crl").join(name)).unwrap()
}

/// The leaf's key with only the leaf in its chain, so the CA must come from
/// somewhere other than the CMS.
struct LeafOnly(Pkcs12Signer, Vec<Vec<u8>>);

impl Signer for LeafOnly {
    fn sign(&self, algorithm: SignatureAlgorithm, message: &[u8]) -> Result<Vec<u8>, SignError> {
        self.0.sign(algorithm, message)
    }
    fn certificate_chain(&self) -> &[Vec<u8>] {
        &self.1
    }
    fn default_algorithm(&self) -> SignatureAlgorithm {
        self.0.default_algorithm()
    }
    fn key_label(&self) -> String {
        self.0.key_label()
    }
}

fn pfx() -> Pkcs12Signer {
    Pkcs12Signer::from_der(&crl_fixture("leaf.pfx"), "pdfcer").unwrap()
}

fn sign_with(signer: &dyn Signer) -> Vec<u8> {
    let mut s = EditSession::new(Document::load(&synthetic().join("hello.pdf")).unwrap());
    s.sign(
        signer,
        &SignRequest::at("D:20260930000000Z"),
        &SaveOptions::identity(),
    )
    .expect("sign")
    .0
}

fn signed() -> Vec<u8> {
    sign_with(&pfx())
}

fn verdict(bytes: &[u8], anchors: Option<&TrustAnchorSet>, crls: &[&str]) -> SignatureVerdict {
    let supplied = crls
        .iter()
        .fold(SuppliedRevocation::new(), |s, c| s.with_crl(crl_fixture(c)));
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let mut all = verify_all_with_revocation(&doc.view(), bytes, anchors, &supplied);
    assert_eq!(all.len(), 1);
    let v = all.remove(0);
    assert!(
        matches!(v.integrity, Integrity::Verified { .. }),
        "{:?}",
        v.integrity
    );
    v
}

/// The last `N 0 obj` … `endobj` body in `bytes`.
fn last_object(bytes: &[u8], num: u32) -> String {
    let text = String::from_utf8_lossy(bytes);
    let head = format!(
        "
{num} 0 obj"
    );
    let start = text.rfind(&head).unwrap() + head.len();
    let end = start + text[start..].find("endobj").unwrap();
    text[start..end].to_owned()
}

fn last_number_after(bytes: &[u8], key: &str) -> usize {
    let text = String::from_utf8_lossy(bytes);
    let at = text.rfind(key).unwrap() + key.len();
    text[at..]
        .trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

/// Append an incremental update giving catalog object 1 a `/DSS` holding
/// `certs` and `crls` as uncompressed streams (EN 319 142-1 §5.4.2.2).
fn with_dss(mut bytes: Vec<u8>, certs: &[Vec<u8>], crls: &[Vec<u8>]) -> Vec<u8> {
    let catalog = last_object(&bytes, 1).trim().to_owned();
    let size = last_number_after(&bytes, "/Size");
    let prev = last_number_after(&bytes, "startxref");
    let dss = size;
    let mut next = size + 1;
    let mut offsets = Vec::new();
    let mut refs = |items: &[Vec<u8>], bytes: &mut Vec<u8>, offsets: &mut Vec<usize>| {
        let mut list = String::new();
        for item in items {
            offsets.push(bytes.len());
            bytes.extend_from_slice(
                format!("{next} 0 obj\n<</Length {}>>\nstream\n", item.len()).as_bytes(),
            );
            bytes.extend_from_slice(item);
            bytes.extend_from_slice(b"\nendstream\nendobj\n");
            list.push_str(&format!("{next} 0 R "));
            next += 1;
        }
        list
    };
    let cert_refs = refs(certs, &mut bytes, &mut offsets);
    let crl_refs = refs(crls, &mut bytes, &mut offsets);
    let catalog_at = bytes.len();
    let patched = catalog.replacen("<<", &format!("<</DSS {dss} 0 R"), 1);
    bytes.extend_from_slice(format!("1 0 obj\n{patched}\nendobj\n").as_bytes());
    let dss_at = bytes.len();
    bytes.extend_from_slice(
        format!("{dss} 0 obj\n<</Certs [{cert_refs}]/CRLs [{crl_refs}]>>\nendobj\n").as_bytes(),
    );
    let xref_at = bytes.len();
    let mut xref = format!(
        "xref\n1 1\n{catalog_at:010} 00000 n \n{dss} {}\n",
        1 + offsets.len()
    );
    xref.push_str(&format!("{dss_at:010} 00000 n \n"));
    for o in &offsets {
        xref.push_str(&format!("{o:010} 00000 n \n"));
    }
    xref.push_str(&format!(
        "trailer\n<</Size {next}/Root 1 0 R/Prev {prev}>>\nstartxref\n{xref_at}\n%%EOF\n"
    ));
    bytes.extend_from_slice(xref.as_bytes());
    bytes
}

#[test]
fn no_crl_anywhere_means_not_checked() {
    let v = verdict(&signed(), None, &[]);
    assert_eq!(v.revocation, Revocation::NotChecked);
    assert!(
        !v.notes.iter().any(|n| n.starts_with("clock:")),
        "no clock was used: {:?}",
        v.notes
    );
    let doc = Document::from_bytes(signed()).unwrap();
    assert_eq!(
        verify_all(&doc.view(), &signed())[0].revocation,
        Revocation::NotChecked
    );
}

#[test]
fn a_supplied_crl_not_listing_the_signer_is_good() {
    let v = verdict(&signed(), None, &["crl-forged.crl", "crl-good.crl"]);
    let Revocation::Good { checked } = &v.revocation else {
        panic!("{:?}", v.revocation);
    };
    assert_eq!(checked.len(), 1, "the self-signed root is not checked");
    assert!(checked[0].subject.contains("test signer"));
    assert_eq!(checked[0].source, RevocationSource::Supplied);
    assert_eq!(
        checked[0].this_update.as_deref(),
        Some("2026-09-20T00:00:00Z")
    );
    assert_eq!(
        checked[0].next_update.as_deref(),
        Some("2099-01-01T00:00:00Z")
    );
    assert!(
        v.notes
            .iter()
            .any(|n| n.starts_with("clock:") && n.contains("2026-09-30T00:00:00Z")),
        "the /M clock is disclosed: {:?}",
        v.notes
    );
}

#[test]
fn a_revocation_is_placed_before_or_after_the_signing_time_from_m() {
    let before = verdict(&signed(), None, &["crl-revoked.crl"]);
    assert_eq!(
        before.revocation,
        Revocation::Revoked {
            subject: "CN=pdfcer CRL test signer (test fixture, trust nothing)".into(),
            date: Some("2026-09-15T00:00:00Z".into()),
            reason: Some("keyCompromise".into()),
            before_signing: Some(true),
            source: RevocationSource::Supplied,
        }
    );
    let after = verdict(&signed(), None, &["crl-revoked-after.crl"]);
    assert!(
        matches!(
            &after.revocation,
            Revocation::Revoked { before_signing: Some(false), reason: Some(r), .. } if r == "superseded"
        ),
        "{:?}",
        after.revocation
    );
}

#[test]
fn a_crl_that_expired_before_signing_leaves_it_undetermined() {
    let v = verdict(&signed(), None, &["crl-stale.crl"]);
    assert!(
        matches!(&v.revocation, Revocation::Undetermined { reason } if reason.contains("expired")),
        "{:?}",
        v.revocation
    );
}

#[test]
fn crls_and_certificates_in_the_dss_are_used() {
    let pfx = pfx();
    let leaf_only = LeafOnly(pfx, vec![crl_fixture("leaf.cer")]);
    let bare = sign_with(&leaf_only);
    // Without the CA the chain cannot be walked…
    let with_crl_only = with_dss(bare.clone(), &[], &[crl_fixture("crl-revoked.crl")]);
    assert!(
        matches!(
            &verdict(&with_crl_only, None, &[]).revocation,
            Revocation::Undetermined { reason } if reason.contains("not available")
        ),
        "the CA is in neither the CMS nor the DSS"
    );
    // …with the CA in /DSS /Certs it can, and the DSS CRL is the source.
    let full = with_dss(
        bare,
        &[crl_fixture("ca.cer")],
        &[crl_fixture("crl-revoked.crl")],
    );
    let v = verdict(&full, None, &[]);
    assert!(
        matches!(
            v.revocation,
            Revocation::Revoked {
                before_signing: Some(true),
                source: RevocationSource::Dss,
                ..
            }
        ),
        "{:?}",
        v.revocation
    );
    // A supplied CRL is consulted after the DSS's.
    let v = verdict(
        &with_dss(signed(), &[], &[crl_fixture("crl-forged.crl")]),
        None,
        &["crl-good.crl"],
    );
    assert!(
        matches!(&v.revocation, Revocation::Good { checked } if checked[0].source == RevocationSource::Supplied),
        "{:?}",
        v.revocation
    );
}

#[test]
fn a_trust_anchor_gets_the_m_clock_too() {
    let ca = crl_fixture("ca.cer");
    let anchor = TrustAnchor {
        subject: "CN=pdfcer CRL test CA (test fixture, trust nothing)".into(),
        issuer: "CN=pdfcer CRL test CA (test fixture, trust nothing)".into(),
        serial_hex: "1000".into(),
        not_before: None,
        not_after: None,
        sources: vec!["TEST".into()],
        trust_bits: 0,
        policy_oids: Vec::new(),
        der: ca,
        id: None,
    };
    let anchors = TrustAnchorSet {
        anchors: vec![anchor],
        undecodable: 0,
    };
    let v = verdict(&signed(), Some(&anchors), &["crl-good.crl"]);
    assert!(
        matches!(
            v.trust,
            Trust::Trusted {
                validity_checked: true,
                ..
            }
        ),
        "a PAdES signature has no CMS signingTime; /M is the clock: {:?}",
        v.trust
    );
    assert!(
        matches!(&v.revocation, Revocation::Good { checked } if checked.len() == 1),
        "{:?}",
        v.revocation
    );
}

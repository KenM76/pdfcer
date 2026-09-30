//! A real OpenSSL-minted certificate's `cRLDistributionPoints` and
//! `authorityInfoAccess` read back as URIs (RFC 5280 §4.2.1.13, §4.2.2.1).

use std::path::PathBuf;

use pdfcer_pkix::cms::{RevocationUris, parse_certificate};

fn signing(name: &str) -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/signing");
    std::fs::read(root.join(name)).unwrap()
}

#[test]
fn an_openssl_certificate_names_its_crl_ocsp_and_issuer_locations() {
    let der = signing("revocation-ecp256.cer");
    let cert = parse_certificate(&der).expect("certificate");
    assert_eq!(
        cert.revocation_uris,
        RevocationUris {
            crl: vec![
                "http://crl.example.invalid/a.crl".into(),
                "ldap://ldap.example.invalid/cn=CA?certificateRevocationList".into(),
            ],
            ocsp: vec!["http://ocsp.example.invalid/".into()],
            ca_issuers: vec!["http://ca.example.invalid/issuer.cer".into()],
            // The second distribution point is a directoryName.
            unreadable: 1,
        }
    );
}

#[test]
fn a_certificate_without_the_extensions_names_nothing() {
    let der = signing("ecp384.cer");
    let cert = parse_certificate(&der).expect("certificate");
    assert_eq!(cert.revocation_uris, RevocationUris::default());
}

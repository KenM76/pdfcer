//! Self-signed digital-ID creation, checked through the importer.
//!
//! The created `.pfx` must open in [`Pkcs12Signer::from_der`] — a separate
//! reader that verifies the MAC, decrypts both bags and pairs key with
//! certificate — and the key must sign something pdfcer's own verifier
//! (an implementation independent of the signing crates) accepts. The
//! certificate's fixed byte patterns are asserted directly so a regression
//! in a single extension cannot pass behind a successful import.

#![cfg(feature = "signing")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::sign::digital_id::{
    DigitalIdSpec, IdError, IdKeyAlgorithm, IdUsage, create_self_signed_id,
};
use pdfcer_core::sign::pkcs12::{Pkcs12Error, Pkcs12Signer};
use pdfcer_core::sign::{Signer, verify_raw_signature};

/// 2026-09-30T00:00:00Z.
const NOW: u64 = 1_790_726_400;

fn spec(key: IdKeyAlgorithm) -> DigitalIdSpec {
    let mut s = DigitalIdSpec::new("Jane Example", NOW);
    s.key = key;
    s.organization = "Example Works".into();
    s.org_unit = "Drawing Office".into();
    s.email = "jane@example.com".into();
    s.country = "ca".into();
    s.pbes2_iterations = 1000;
    s
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn round_trip(key: IdKeyAlgorithm, label: &str) {
    let id = create_self_signed_id(&spec(key), "pässwörd").unwrap();
    assert_eq!(id.key_label, label);
    let signer = Pkcs12Signer::from_der(&id.pfx, "pässwörd").unwrap();
    let report = signer.report();
    assert_eq!(report.key, label);
    assert_eq!(report.chain_length, 1);
    assert_eq!(report.friendly_name.as_deref(), Some("Jane Example"));
    assert!(
        report.subject.contains("Jane Example"),
        "{}",
        report.subject
    );
    assert!(
        report.key_scheme.contains("AES-256"),
        "{}",
        report.key_scheme
    );
    assert_eq!(
        signer.certificate_chain(),
        std::slice::from_ref(&id.certificate)
    );

    let msg = b"pdfcer digital id";
    let alg = signer.default_algorithm();
    let sig = signer.sign(alg, msg).unwrap();
    verify_raw_signature(&id.certificate, alg, msg, &sig).unwrap();
}

#[test]
fn p256_id_opens_in_the_importer_and_signs() {
    round_trip(IdKeyAlgorithm::EcdsaP256, "EC P-256");
}

#[test]
fn rsa2048_id_opens_in_the_importer_and_signs() {
    round_trip(IdKeyAlgorithm::Rsa2048, "RSA-2048");
}

#[test]
fn wrong_password_is_refused_at_the_mac() {
    let id = create_self_signed_id(&spec(IdKeyAlgorithm::EcdsaP256), "right").unwrap();
    assert!(matches!(
        Pkcs12Signer::from_der(&id.pfx, "wrong"),
        Err(Pkcs12Error::MacMismatch { .. })
    ));
}

#[test]
fn certificate_carries_the_fixed_extensions() {
    let id = create_self_signed_id(&spec(IdKeyAlgorithm::EcdsaP256), "pw").unwrap();
    let c = &id.certificate;
    // basicConstraints, critical, cA FALSE (empty SEQUENCE).
    assert!(contains(
        c,
        &[
            0x30, 0x0c, 0x06, 0x03, 0x55, 0x1d, 0x13, 0x01, 0x01, 0xff, 0x04, 0x02, 0x30, 0x00
        ]
    ));
    // keyUsage, critical, digitalSignature + nonRepudiation.
    assert!(contains(
        c,
        &[
            0x06, 0x03, 0x55, 0x1d, 0x0f, 0x01, 0x01, 0xff, 0x04, 0x04, 0x03, 0x02, 0x06, 0xc0
        ]
    ));
    // Adobe Authentic Documents EKU.
    assert!(contains(
        c,
        &[
            0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x2f, 0x01, 0x01, 0x05
        ]
    ));
    // rfc822Name in subjectAltName.
    let mut san = vec![0x81, 16];
    san.extend_from_slice(b"jane@example.com");
    assert!(contains(c, &san));
    // Country upper-cased, PrintableString.
    assert!(contains(c, &[0x13, 0x02, b'C', b'A']));
    // v3.
    assert!(contains(c, &[0xa0, 0x03, 0x02, 0x01, 0x02]));
    // Validity: UTCTime 2026, five years on.
    assert!(contains(c, b"\x17\x0d260930000000Z\x17\x0d310930000000Z"));
    assert_eq!(id.valid_until, "2031-09-30 00:00:00 UTC");
}

#[test]
fn rsa_signing_and_encryption_adds_key_encipherment() {
    let mut s = spec(IdKeyAlgorithm::Rsa2048);
    s.usage = IdUsage::SigningAndEncryption;
    let id = create_self_signed_id(&s, "pw").unwrap();
    assert!(contains(
        &id.certificate,
        &[0x04, 0x04, 0x03, 0x02, 0x05, 0xe0]
    ));
}

#[test]
fn validity_past_2049_uses_generalized_time() {
    let mut s = spec(IdKeyAlgorithm::EcdsaP256);
    s.valid_years = 30;
    let id = create_self_signed_id(&s, "pw").unwrap();
    assert!(contains(
        &id.certificate,
        b"\x17\x0d260930000000Z\x18\x0f20560930000000Z"
    ));
}

#[test]
fn serials_differ_between_ids() {
    let s = spec(IdKeyAlgorithm::EcdsaP256);
    let a = create_self_signed_id(&s, "pw").unwrap();
    let b = create_self_signed_id(&s, "pw").unwrap();
    assert_ne!(a.certificate, b.certificate);
    assert_ne!(a.sha256_fingerprint, b.sha256_fingerprint);
}

#[test]
fn minimal_spec_omits_empty_fields() {
    let mut s = DigitalIdSpec::new("Solo", NOW);
    s.key = IdKeyAlgorithm::EcdsaP256;
    s.pbes2_iterations = 1000;
    let id = create_self_signed_id(&s, "pw").unwrap();
    // No subjectAltName extension OID when there is no e-mail.
    assert!(!contains(&id.certificate, &[0x06, 0x03, 0x55, 0x1d, 0x11]));
    Pkcs12Signer::from_der(&id.pfx, "pw").unwrap();
}

#[test]
fn invalid_specs_are_refused_by_name() {
    let base = || spec(IdKeyAlgorithm::EcdsaP256);
    let refuse = |s: DigitalIdSpec, pw: &str| create_self_signed_id(&s, pw).unwrap_err();

    let mut s = base();
    s.common_name = "  ".into();
    assert_eq!(refuse(s, "pw"), IdError::EmptyCommonName);
    assert_eq!(refuse(base(), ""), IdError::EmptyPassword);
    assert_eq!(
        refuse(base(), "pw\u{1F600}"),
        IdError::PasswordCharacter {
            character: '\u{1F600}'
        }
    );
    for bad in ["CAN", "C1", "c"] {
        let mut s = base();
        s.country = bad.into();
        assert!(
            matches!(refuse(s, "pw"), IdError::BadCountry { .. }),
            "{bad}"
        );
    }
    for bad in ["no-at", "a@b@c", "@x", "spa ce@x.com"] {
        let mut s = base();
        s.email = bad.into();
        assert!(matches!(refuse(s, "pw"), IdError::BadEmail { .. }), "{bad}");
    }
    let mut s = base();
    s.organization = "x".repeat(65);
    assert!(matches!(
        refuse(s, "pw"),
        IdError::FieldTooLong { max: 64, .. }
    ));
    for years in [0, 101] {
        let mut s = base();
        s.valid_years = years;
        assert_eq!(refuse(s, "pw"), IdError::ValidityOutOfRange { years });
    }
    for value in [0, 600_001] {
        let mut s = base();
        s.pbes2_iterations = value;
        assert_eq!(refuse(s, "pw"), IdError::IterationsOutOfRange { value });
    }
    let mut s = base();
    s.usage = IdUsage::SigningAndEncryption;
    assert_eq!(refuse(s, "pw"), IdError::EncryptionNeedsRsa);
}

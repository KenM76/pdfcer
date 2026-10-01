//! Creating a **self-signed digital ID**: a fresh key pair, an X.509 v3
//! certificate signed with it, and the two wrapped in a password-protected
//! PKCS#12 (`.pfx`) file that [`super::pkcs12::Pkcs12Signer::from_der`]
//! opens.
//!
//! Spec RAG: `security__x509_self_signed_authoring.md` (`XS-*`) for the
//! certificate, `security__pkcs12_export.md` (`PX-*`) for the container.
//! A self-signed end-entity certificate is outside RFC 5280's scope (RFC
//! 6818 §2, `XS-2`), so 5280's MUSTs are followed as compatibility targets.
//!
//! # What is written, and why each choice
//!
//! | Part | Choice | Source |
//! |---|---|---|
//! | serial | 128 random bits, top bit cleared (no pad octet) | `XS-5` |
//! | signature | sha256WithRSAEncryption (NULL params) / ecdsa-with-SHA256 (absent) | `XS-7` |
//! | name | C, O, OU, CN, emailAddress; empty fields omitted; CN/O/OU UTF8String | `XS-10`, `XS-12`, `XS-G2` |
//! | validity | UTCTime through 2049, GeneralizedTime from 2050, per field | `XS-14` |
//! | basicConstraints | critical, cA FALSE — the ID can never act as a CA for whoever trusts it | `XS-23` |
//! | keyUsage | critical; digitalSignature + nonRepudiation, + keyEncipherment for RSA sign-and-encrypt | `XS-25`, `XS-27` |
//! | SKI / AKI | SHA-1 of the subjectPublicKey bits; AKI repeats it | `XS-28`, `XS-29` |
//! | subjectAltName | rfc822Name when an e-mail is given (and the legacy DN attribute, for display) | `XS-13` |
//! | extKeyUsage | emailProtection + Adobe `1.2.840.113583.1.1.5` — passes Acrobat's signing-certificate filter | `XS-39` |
//! | PFX layout | certs in PBES2 `EncryptedData`, key in a shrouded bag inside plain `Data` | `PX-1` |
//! | PBES2 | PBKDF2-HMAC-SHA256 (prf NULL, no keyLength) + AES-256-CBC, 16-byte salt and IV each | `PX-4`–`PX-10` |
//! | MAC | HMAC-SHA256 under the RFC 7292 App. B KDF, 2048 iterations, 16-byte salt | `PX-16`–`PX-19` |
//! | bag attributes | friendlyName = CN, localKeyId = SHA-1(cert) on both bags | `PX-21`–`PX-23` |
//!
//! One password, two encodings (`PX-11`): UTF-8 for PBKDF2, BMPString plus
//! a `0000` terminator for the MAC — which is why characters outside the
//! Basic Multilingual Plane are refused.
//!
//! An ECDSA key cannot carry an encryption key usage (RFC 8813 §3,
//! `XS-26`), so [`IdUsage::SigningAndEncryption`] with an EC key is refused
//! by name rather than silently downgraded.
//!
//! pdfcer reads no clock: the caller passes the creation instant.

use super::der_out;
use super::pkcs12::MacHash;
use super::{KeyMaterial, PdfcerRng, SignError};

/// The key a new digital ID is generated with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum IdKeyAlgorithm {
    /// RSA, 2048-bit modulus — what Acrobat's own wizard creates.
    #[default]
    Rsa2048,
    /// RSA, 3072-bit modulus.
    Rsa3072,
    /// ECDSA over NIST P-256. Signing only (RFC 8813 §3).
    EcdsaP256,
}

/// What the certificate's key usage permits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum IdUsage {
    /// `digitalSignature` + `nonRepudiation`.
    #[default]
    Signing,
    /// Signing plus `keyEncipherment`, so the certificate can also be a
    /// recipient of a certificate-encrypted PDF. RSA keys only.
    SigningAndEncryption,
}

/// The PBES2 iteration count used when the caller does not set one: the
/// highest Windows `certutil` accepts (`PX-9`).
pub const DEFAULT_PBES2_ITERATIONS: u32 = 600_000;

/// The PKCS#12 MAC iteration count (`PX-18`, the OpenSSL default).
const MAC_ITERATIONS: u64 = 2048;

/// What to put in a new digital ID. Build with [`DigitalIdSpec::new`], then
/// set fields; empty strings mean "omit this attribute".
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DigitalIdSpec {
    /// The holder's name (`CN`). Required, at most 64 UTF-8 bytes.
    pub common_name: String,
    /// Organisational unit (`OU`), at most 64 UTF-8 bytes.
    pub org_unit: String,
    /// Organisation (`O`), at most 64 UTF-8 bytes.
    pub organization: String,
    /// E-mail address: ASCII, one `@`, at most 255 bytes. Goes into
    /// subjectAltName and the subject name.
    pub email: String,
    /// ISO 3166 two-letter country code (`C`); lower case is upper-cased.
    pub country: String,
    /// The key to generate.
    pub key: IdKeyAlgorithm,
    /// What the key may be used for.
    pub usage: IdUsage,
    /// Start of validity, seconds since 1970-01-01T00:00:00Z.
    pub not_before_unix: u64,
    /// Length of validity in whole years, `1..=100`. Default 5 (Acrobat's
    /// fixed term).
    pub valid_years: u16,
    /// PBKDF2 iterations protecting the key and certificate,
    /// `1..=600_000`. Default [`DEFAULT_PBES2_ITERATIONS`]; Windows refuses
    /// a file above 600 000 (`PX-9`).
    pub pbes2_iterations: u32,
}

impl DigitalIdSpec {
    /// A spec with the defaults: RSA-2048, signing only, five years from
    /// `not_before_unix`, [`DEFAULT_PBES2_ITERATIONS`].
    #[must_use]
    pub fn new(common_name: impl Into<String>, not_before_unix: u64) -> Self {
        Self {
            common_name: common_name.into(),
            org_unit: String::new(),
            organization: String::new(),
            email: String::new(),
            country: String::new(),
            key: IdKeyAlgorithm::default(),
            usage: IdUsage::default(),
            not_before_unix,
            valid_years: 5,
            pbes2_iterations: DEFAULT_PBES2_ITERATIONS,
        }
    }
}

/// A newly created digital ID.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DigitalId {
    /// The `.pfx` file bytes, protected by the password given.
    pub pfx: Vec<u8>,
    /// The certificate alone (DER), e.g. to export as `.cer` for others
    /// to trust.
    pub certificate: Vec<u8>,
    /// SHA-256 of [`Self::certificate`] — the fingerprint a recipient
    /// compares before trusting it.
    pub sha256_fingerprint: [u8; 32],
    /// `"YYYY-MM-DD HH:MM:SS UTC"`, the certificate's `notAfter`.
    pub valid_until: String,
    /// `RSA-2048`, `EC P-256`, ….
    pub key_label: String,
}

impl std::fmt::Debug for DigitalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The pfx bytes are opaque and large; show their size only.
        f.debug_struct("DigitalId")
            .field("pfx_len", &self.pfx.len())
            .field("certificate_len", &self.certificate.len())
            .field("valid_until", &self.valid_until)
            .field("key_label", &self.key_label)
            .finish_non_exhaustive()
    }
}

/// Why a digital ID was not created. Every variant is a refusal by name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IdError {
    /// The common name is empty or only whitespace.
    #[error("the name (common name) is empty")]
    EmptyCommonName,
    /// The password is empty.
    #[error("the password is empty")]
    EmptyPassword,
    /// The password holds a character outside the Basic Multilingual Plane
    /// (an emoji, say), which the PKCS#12 MAC cannot encode.
    #[error("the password contains {character:?}, which a .pfx password cannot hold")]
    PasswordCharacter {
        /// The first offending character.
        character: char,
    },
    /// The country is not two ASCII letters.
    #[error("country {value:?} is not a two-letter ISO 3166 code")]
    BadCountry {
        /// The value as given.
        value: String,
    },
    /// The e-mail address is not plain ASCII with one `@`, or is too long.
    #[error("e-mail address {value:?} is not usable (ASCII, one @, at most 255 characters)")]
    BadEmail {
        /// The value as given.
        value: String,
    },
    /// A name field is longer than its limit (RFC 5280 App. A.1).
    #[error("the {field} is {len} bytes long; the limit is {max}")]
    FieldTooLong {
        /// `common name`, `organisation` or `organisational unit`.
        field: &'static str,
        /// Its UTF-8 length.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// `valid_years` is outside `1..=100`.
    #[error("validity of {years} years is outside 1 to 100")]
    ValidityOutOfRange {
        /// The value as given.
        years: u16,
    },
    /// `pbes2_iterations` is outside `1..=600_000`.
    #[error("{value} iterations is outside 1 to 600000 (Windows refuses more)")]
    IterationsOutOfRange {
        /// The value as given.
        value: u32,
    },
    /// Encryption usage was asked of an ECDSA key (RFC 8813 §3).
    #[error("an ECDSA key cannot be used for encryption; choose an RSA key or signing only")]
    EncryptionNeedsRsa,
    /// No entropy source (wasm32 without a backend).
    #[error("no random-number source is available: {0}")]
    RandomUnavailable(String),
    /// Key generation or the certificate's self-signature failed.
    #[error("key operation failed: {0}")]
    KeyOperation(String),
}

impl From<SignError> for IdError {
    fn from(e: SignError) -> Self {
        match e {
            SignError::RandomUnavailable(s) => Self::RandomUnavailable(s),
            other => Self::KeyOperation(other.to_string()),
        }
    }
}

/// Create a self-signed digital ID: generate the key, write and self-sign
/// the certificate, check the signature with pdfcer's own verifier, and wrap
/// both in a `.pfx` protected by `password`.
///
/// RSA key generation is the slow step (around a second for 2048 bits in an
/// optimised build, several times that for 3072).
///
/// # Errors
///
/// [`IdError`] for an invalid spec or password (checked before any key is
/// generated), a missing random source, or a failed key operation.
///
/// # Examples
///
/// ```
/// use pdfcer_core::sign::digital_id::{create_self_signed_id, DigitalIdSpec, IdKeyAlgorithm};
/// use pdfcer_core::sign::pkcs12::Pkcs12Signer;
///
/// let mut spec = DigitalIdSpec::new("Jane Example", 1_790_000_000);
/// spec.key = IdKeyAlgorithm::EcdsaP256;
/// spec.pbes2_iterations = 1000; // fast for the example; keep the default in real use
/// let id = create_self_signed_id(&spec, "correct horse")?;
/// let signer = Pkcs12Signer::from_der(&id.pfx, "correct horse").unwrap();
/// assert_eq!(signer.report().key, "EC P-256");
/// # Ok::<(), pdfcer_core::sign::digital_id::IdError>(())
/// ```
pub fn create_self_signed_id(spec: &DigitalIdSpec, password: &str) -> Result<DigitalId, IdError> {
    let checked = validate(spec, password)?;
    let mut rng = PdfcerRng;
    rng.probe()?;

    let key = generate_key(spec.key)?;
    let not_after = add_years(spec.not_before_unix, spec.valid_years);
    let certificate = build_certificate(&checked, &key, spec, not_after)?;

    // XS-G3: a self-signed certificate verifies under its own key.
    let tbs = tbs_of(&certificate)
        .ok_or_else(|| IdError::KeyOperation("re-reading the certificate".into()))?;
    let sig = signature_value_of(&certificate)
        .ok_or_else(|| IdError::KeyOperation("re-reading the signature".into()))?;
    super::verify_raw_signature(&certificate, key.default_algorithm(), tbs, sig)
        .map_err(|e| IdError::KeyOperation(format!("self-signature did not verify: {e}")))?;

    let pkcs8 = private_key_info(&key)?;
    let pfx = build_pfx(
        &certificate,
        &pkcs8,
        &checked.common_name,
        password,
        spec.pbes2_iterations,
    )?;

    let mut sha256_fingerprint = [0u8; 32];
    sha256_fingerprint.copy_from_slice(&MacHash::Sha256.hash(&certificate));
    let (y, mo, d, h, mi, s) = civil(not_after);
    Ok(DigitalId {
        pfx,
        certificate,
        sha256_fingerprint,
        valid_until: format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC"),
        key_label: key.label(),
    })
}

/// The spec's text fields after validation.
struct Checked {
    common_name: String,
    org_unit: String,
    organization: String,
    email: String,
    country: String,
}

fn validate(spec: &DigitalIdSpec, password: &str) -> Result<Checked, IdError> {
    let common_name = spec.common_name.trim().to_owned();
    if common_name.is_empty() {
        return Err(IdError::EmptyCommonName);
    }
    if password.is_empty() {
        return Err(IdError::EmptyPassword);
    }
    if let Some(character) = password.chars().find(|c| u32::from(*c) > 0xFFFF) {
        return Err(IdError::PasswordCharacter { character });
    }
    let org_unit = spec.org_unit.trim().to_owned();
    let organization = spec.organization.trim().to_owned();
    for (field, value) in [
        ("common name", &common_name),
        ("organisation", &organization),
        ("organisational unit", &org_unit),
    ] {
        // XS-11: the bound is in characters; bytes satisfy both readings.
        if value.len() > 64 {
            return Err(IdError::FieldTooLong {
                field,
                len: value.len(),
                max: 64,
            });
        }
    }
    let country = spec.country.trim().to_ascii_uppercase();
    if !country.is_empty()
        && !(country.len() == 2 && country.bytes().all(|b| b.is_ascii_uppercase()))
    {
        return Err(IdError::BadCountry {
            value: spec.country.clone(),
        });
    }
    let email = spec.email.trim().to_owned();
    if !email.is_empty() {
        let ok = email.len() <= 255
            && email.bytes().all(|b| b.is_ascii_graphic())
            && email.bytes().filter(|&b| b == b'@').count() == 1
            && !email.starts_with('@')
            && !email.ends_with('@');
        if !ok {
            return Err(IdError::BadEmail {
                value: spec.email.clone(),
            });
        }
    }
    if !(1..=100).contains(&spec.valid_years) {
        return Err(IdError::ValidityOutOfRange {
            years: spec.valid_years,
        });
    }
    if !(1..=600_000).contains(&spec.pbes2_iterations) {
        return Err(IdError::IterationsOutOfRange {
            value: spec.pbes2_iterations,
        });
    }
    if spec.usage == IdUsage::SigningAndEncryption && spec.key == IdKeyAlgorithm::EcdsaP256 {
        return Err(IdError::EncryptionNeedsRsa);
    }
    Ok(Checked {
        common_name,
        org_unit,
        organization,
        email,
        country,
    })
}

fn generate_key(alg: IdKeyAlgorithm) -> Result<KeyMaterial, IdError> {
    // `probe` has already succeeded, so `UnwrapErr`'s panic-on-error path is
    // unreachable short of the OS entropy source failing mid-call.
    let mut rng = rand_core::UnwrapErr(PdfcerRng);
    match alg {
        IdKeyAlgorithm::Rsa2048 | IdKeyAlgorithm::Rsa3072 => {
            let bits = if alg == IdKeyAlgorithm::Rsa2048 {
                2048
            } else {
                3072
            };
            rsa::RsaPrivateKey::new(&mut rng, bits)
                .map(KeyMaterial::Rsa)
                .map_err(|e| IdError::KeyOperation(e.to_string()))
        }
        IdKeyAlgorithm::EcdsaP256 => {
            use p256::elliptic_curve::Generate as _;
            p256::ecdsa::SigningKey::try_generate_from_rng(&mut PdfcerRng)
                .map(KeyMaterial::P256)
                .map_err(|e| IdError::RandomUnavailable(e.to_string()))
        }
    }
}

// ---------------------------------------------------------------------------
// Certificate (RFC 5280 §4.1)
// ---------------------------------------------------------------------------

const OID_COUNTRY: &str = "2.5.4.6";
const OID_ORG: &str = "2.5.4.10";
const OID_ORG_UNIT: &str = "2.5.4.11";
const OID_COMMON_NAME: &str = "2.5.4.3";
const OID_EMAIL_ADDRESS: &str = "1.2.840.113549.1.9.1";
const OID_BASIC_CONSTRAINTS: &str = "2.5.29.19";
const OID_KEY_USAGE: &str = "2.5.29.15";
const OID_SKI: &str = "2.5.29.14";
const OID_AKI: &str = "2.5.29.35";
const OID_SAN: &str = "2.5.29.17";
const OID_EKU: &str = "2.5.29.37";
const OID_EMAIL_PROTECTION: &str = "1.3.6.1.5.5.7.3.4";
const OID_ADOBE_AUTHENTIC_DOCUMENTS: &str = "1.2.840.113583.1.1.5";
const OID_PRIME256V1: &str = "1.2.840.10045.3.1.7";

use crate::asn1::{IA5_STRING, UTF8_STRING};

/// `der_out::oid` on a literal: the constants above are well-formed, so the
/// `None` arm is unreachable; an empty encoding would fail the self-verify.
fn oid(dotted: &str) -> Vec<u8> {
    der_out::oid(dotted).unwrap_or_default()
}

fn build_certificate(
    c: &Checked,
    key: &KeyMaterial,
    spec: &DigitalIdSpec,
    not_after: u64,
) -> Result<Vec<u8>, IdError> {
    let algorithm = key.default_algorithm();
    let sig_alg = algorithm.signature_algorithm_der();
    let name = name(c);
    let (spki, public_bits) = subject_public_key_info(key);
    let ski = MacHash::Sha1.hash(&public_bits);

    let mut serial = [0u8; 16];
    crate::crypto::rng::fill(&mut serial).map_err(|e| IdError::RandomUnavailable(e.to_string()))?;
    serial[0] &= 0x7F;
    if serial.iter().all(|&b| b == 0) {
        serial[15] = 1;
    }

    let validity = der_out::sequence(&[time(spec.not_before_unix), time(not_after)]);

    let mut usage_bits = vec![0, 1];
    if spec.usage == IdUsage::SigningAndEncryption {
        usage_bits.push(2);
    }
    let mut extensions = vec![
        extension(OID_BASIC_CONSTRAINTS, true, &der_out::sequence(&[])),
        extension(OID_KEY_USAGE, true, &named_bits(&usage_bits)),
        extension(OID_SKI, false, &der_out::octet_string(&ski)),
        extension(
            OID_AKI,
            false,
            &der_out::sequence(&[der_out::tlv(0x80, &ski)]),
        ),
    ];
    if !c.email.is_empty() {
        extensions.push(extension(
            OID_SAN,
            false,
            &der_out::sequence(&[der_out::tlv(0x81, c.email.as_bytes())]),
        ));
    }
    extensions.push(extension(
        OID_EKU,
        false,
        &der_out::sequence(&[
            oid(OID_EMAIL_PROTECTION),
            oid(OID_ADOBE_AUTHENTIC_DOCUMENTS),
        ]),
    ));

    let tbs = der_out::sequence(&[
        der_out::context(0, &der_out::integer_u64(2)),
        der_out::integer(&serial),
        sig_alg.clone(),
        name.clone(),
        validity,
        name,
        spki,
        der_out::context(3, &der_out::sequence(&extensions)),
    ]);
    let signature = key.sign(algorithm, &tbs)?;
    Ok(der_out::sequence(&[tbs, sig_alg, bit_string(&signature)]))
}

/// `Name` in the order C, O, OU, CN, emailAddress (`XS-12`), one attribute
/// per RDN, empty fields omitted.
fn name(c: &Checked) -> Vec<u8> {
    let rdn = |oid_s: &str, tag: u8, value: &str| {
        der_out::set_of(vec![der_out::sequence(&[
            oid(oid_s),
            der_out::tlv(tag, value.as_bytes()),
        ])])
    };
    let mut rdns = Vec::new();
    if !c.country.is_empty() {
        rdns.push(rdn(OID_COUNTRY, crate::asn1::PRINTABLE_STRING, &c.country));
    }
    if !c.organization.is_empty() {
        rdns.push(rdn(OID_ORG, UTF8_STRING, &c.organization));
    }
    if !c.org_unit.is_empty() {
        rdns.push(rdn(OID_ORG_UNIT, UTF8_STRING, &c.org_unit));
    }
    rdns.push(rdn(OID_COMMON_NAME, UTF8_STRING, &c.common_name));
    if !c.email.is_empty() {
        rdns.push(rdn(OID_EMAIL_ADDRESS, IA5_STRING, &c.email));
    }
    der_out::sequence(&rdns)
}

/// The SPKI, and the subjectPublicKey bits the SKI hashes (`XS-28`).
fn subject_public_key_info(key: &KeyMaterial) -> (Vec<u8>, Vec<u8>) {
    use rsa::traits::PublicKeyParts as _;
    let (alg, bits) = match key {
        KeyMaterial::Rsa(k) => (
            der_out::algorithm_identifier(crate::cms::oid::RSA_ENCRYPTION, Some(der_out::null())),
            der_out::sequence(&[
                der_out::integer(&k.n().to_be_bytes()),
                der_out::integer(&k.e().to_be_bytes()),
            ]),
        ),
        KeyMaterial::P256(k) => (
            der_out::algorithm_identifier(
                crate::cms::oid::EC_PUBLIC_KEY,
                Some(oid(OID_PRIME256V1)),
            ),
            k.verifying_key().to_sec1_point(false).as_bytes().to_vec(),
        ),
        // Never generated here; listed so the match stays exhaustive.
        KeyMaterial::P384(k) => (
            der_out::algorithm_identifier(
                crate::cms::oid::EC_PUBLIC_KEY,
                Some(oid("1.3.132.0.34")),
            ),
            k.verifying_key().to_sec1_point(false).as_bytes().to_vec(),
        ),
    };
    (
        der_out::sequence(&[alg.unwrap_or_default(), bit_string(&bits)]),
        bits,
    )
}

/// `Extension ::= SEQUENCE { extnID, critical DEFAULT FALSE, extnValue }` —
/// `critical` written only when TRUE (`XS-20`).
fn extension(oid_s: &str, critical: bool, value: &[u8]) -> Vec<u8> {
    let mut parts = vec![oid(oid_s)];
    if critical {
        parts.push(vec![crate::asn1::BOOLEAN, 1, 0xFF]);
    }
    parts.push(der_out::octet_string(value));
    der_out::sequence(&parts)
}

/// A BIT STRING with zero unused bits.
fn bit_string(bytes: &[u8]) -> Vec<u8> {
    let mut content = Vec::with_capacity(bytes.len() + 1);
    content.push(0);
    content.extend_from_slice(bytes);
    der_out::tlv(crate::asn1::BIT_STRING, &content)
}

/// A named-bit-list BIT STRING with trailing zero bits trimmed (X.690
/// §11.2.2, `XS-25`). Bit 0 is the most significant bit of the first octet.
fn named_bits(bits: &[u8]) -> Vec<u8> {
    let Some(&highest) = bits.iter().max() else {
        return der_out::tlv(crate::asn1::BIT_STRING, &[0]);
    };
    let octets = usize::from(highest / 8) + 1;
    let mut value = vec![0u8; octets];
    for &b in bits {
        if let Some(o) = value.get_mut(usize::from(b / 8)) {
            *o |= 0x80 >> (b % 8);
        }
    }
    let unused = 7 - (highest % 8);
    let mut content = vec![unused];
    content.extend_from_slice(&value);
    der_out::tlv(crate::asn1::BIT_STRING, &content)
}

/// UTCTime through 2049, GeneralizedTime from 2050 (`XS-14`, `XS-15`).
fn time(unix: u64) -> Vec<u8> {
    let (y, mo, d, h, mi, s) = civil(unix);
    if (1950..2050).contains(&y) {
        let text = format!("{:02}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z", y % 100);
        der_out::tlv(crate::asn1::UTC_TIME, text.as_bytes())
    } else {
        let text = format!("{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z");
        der_out::tlv(crate::asn1::GENERALIZED_TIME, text.as_bytes())
    }
}

/// Unix seconds → (year, month, day, hour, minute, second), proleptic
/// Gregorian, UTC (H. Hinnant's `civil_from_days`).
fn civil(unix: u64) -> (u64, u64, u64, u64, u64, u64) {
    let days = unix / 86_400;
    let rem = unix % 86_400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    (y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

/// (year, month, day) → days since 1970-01-01 (`days_from_civil`).
fn days_from_civil(y: u64, m: u64, d: u64) -> u64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The same calendar instant `years` later; 29 February becomes 28
/// February in a non-leap target year.
fn add_years(unix: u64, years: u16) -> u64 {
    let (y, m, d, h, mi, s) = civil(unix);
    let ny = y + u64::from(years);
    let leap = (ny % 4 == 0 && ny % 100 != 0) || ny % 400 == 0;
    let nd = if m == 2 && d == 29 && !leap { 28 } else { d };
    days_from_civil(ny, m, nd) * 86_400 + h * 3600 + mi * 60 + s
}

/// `tbsCertificate`'s complete encoding, from a certificate.
fn tbs_of(cert: &[u8]) -> Option<&[u8]> {
    let (outer, _) = crate::asn1::expect(cert, crate::asn1::SEQUENCE)?;
    Some(crate::asn1::children(outer)?.first()?.raw)
}

/// The signature value's octets (without the unused-bits octet).
fn signature_value_of(cert: &[u8]) -> Option<&[u8]> {
    let (outer, _) = crate::asn1::expect(cert, crate::asn1::SEQUENCE)?;
    let kids = crate::asn1::children(outer)?;
    kids.get(2)
        .filter(|t| t.tag == crate::asn1::BIT_STRING)?
        .content
        .get(1..)
}

// ---------------------------------------------------------------------------
// PKCS#8 + PKCS#12 (RFC 5958, RFC 5915, RFC 7292, RFC 8018)
// ---------------------------------------------------------------------------

const OID_DATA: &str = "1.2.840.113549.1.7.1";
const OID_ENCRYPTED_DATA: &str = "1.2.840.113549.1.7.6";
const OID_SHROUDED_KEY_BAG: &str = "1.2.840.113549.1.12.10.1.2";
const OID_CERT_BAG: &str = "1.2.840.113549.1.12.10.1.3";
const OID_X509_CERTIFICATE: &str = "1.2.840.113549.1.9.22.1";
const OID_FRIENDLY_NAME: &str = "1.2.840.113549.1.9.20";
const OID_LOCAL_KEY_ID: &str = "1.2.840.113549.1.9.21";
const OID_PBES2: &str = "1.2.840.113549.1.5.13";
const OID_PBKDF2: &str = "1.2.840.113549.1.5.12";
const OID_HMAC_SHA256: &str = "1.2.840.113549.2.9";
const OID_AES256_CBC: &str = "2.16.840.1.101.3.4.1.42";
const OID_SHA256: &str = "2.16.840.1.101.3.4.2.1";

/// Key bytes, zeroed on drop.
struct SecretBytes(Vec<u8>);

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.fill(0);
        // Keeps the clearing from being optimised away as a dead store.
        std::hint::black_box(&self.0);
    }
}

fn private_key_info(key: &KeyMaterial) -> Result<SecretBytes, IdError> {
    match key {
        KeyMaterial::Rsa(k) => {
            use rsa::pkcs8::EncodePrivateKey as _;
            let doc = k
                .to_pkcs8_der()
                .map_err(|e| IdError::KeyOperation(e.to_string()))?;
            Ok(SecretBytes(doc.as_bytes().to_vec()))
        }
        KeyMaterial::P256(k) => {
            // ECPrivateKey { version 1, privateKey OCTET STRING (32, I2OSP),
            // publicKey [1] BIT STRING } — `[0]` parameters omitted, as
            // OpenSSL does inside PKCS#8 (PX-26).
            let scalar = SecretBytes(k.to_bytes().to_vec());
            let point = k.verifying_key().to_sec1_point(false).as_bytes().to_vec();
            let ec = SecretBytes(der_out::sequence(&[
                der_out::integer_u64(1),
                der_out::octet_string(&scalar.0),
                der_out::context(1, &bit_string(&point)),
            ]));
            let alg = der_out::algorithm_identifier(
                crate::cms::oid::EC_PUBLIC_KEY,
                Some(oid(OID_PRIME256V1)),
            )
            .unwrap_or_default();
            Ok(SecretBytes(der_out::sequence(&[
                der_out::integer_u64(0),
                alg,
                der_out::octet_string(&ec.0),
            ])))
        }
        KeyMaterial::P384(_) => Err(IdError::KeyOperation(
            "P-384 is not a creation option".into(),
        )),
    }
}

fn random(n: usize) -> Result<Vec<u8>, IdError> {
    let mut v = vec![0u8; n];
    crate::crypto::rng::fill(&mut v).map_err(|e| IdError::RandomUnavailable(e.to_string()))?;
    Ok(v)
}

/// PBES2 (PBKDF2-HMAC-SHA256, AES-256-CBC) over `plaintext`: returns the
/// AlgorithmIdentifier and the ciphertext (`PX-4`; password as UTF-8,
/// `PX-11`).
fn pbes2_encrypt(
    plaintext: &[u8],
    password: &str,
    iterations: u32,
) -> Result<(Vec<u8>, Vec<u8>), IdError> {
    use aes::cipher::block_padding::Pkcs7;
    use aes::cipher::{BlockModeEncrypt as _, KeyIvInit as _};
    let salt = random(16)?;
    let iv = random(16)?;
    let mut key = SecretBytes(vec![0u8; 32]);
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), &salt, iterations, &mut key.0);
    let ciphertext = cbc::Encryptor::<aes::Aes256>::new_from_slices(&key.0, &iv)
        .map_err(|e| IdError::KeyOperation(e.to_string()))?
        .encrypt_padded_vec::<Pkcs7>(plaintext);
    let pbkdf2_params = der_out::sequence(&[
        der_out::octet_string(&salt),
        der_out::integer_u64(u64::from(iterations)),
        der_out::algorithm_identifier(OID_HMAC_SHA256, Some(der_out::null())).unwrap_or_default(),
    ]);
    let params = der_out::sequence(&[
        der_out::algorithm_identifier(OID_PBKDF2, Some(pbkdf2_params)).unwrap_or_default(),
        der_out::algorithm_identifier(OID_AES256_CBC, Some(der_out::octet_string(&iv)))
            .unwrap_or_default(),
    ]);
    let alg = der_out::algorithm_identifier(OID_PBES2, Some(params)).unwrap_or_default();
    Ok((alg, ciphertext))
}

/// `ContentInfo { contentType, [0] EXPLICIT content }`.
fn content_info(type_oid: &str, content: &[u8]) -> Vec<u8> {
    der_out::sequence(&[oid(type_oid), der_out::context(0, content)])
}

fn build_pfx(
    certificate: &[u8],
    pkcs8: &SecretBytes,
    friendly_name: &str,
    password: &str,
    iterations: u32,
) -> Result<Vec<u8>, IdError> {
    let local_key_id = MacHash::Sha1.hash(certificate);
    let bmp: Vec<u8> = friendly_name
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    let attrs = der_out::set_of(vec![
        der_out::sequence(&[
            oid(OID_FRIENDLY_NAME),
            der_out::set_of(vec![der_out::tlv(crate::asn1::BMP_STRING, &bmp)]),
        ]),
        der_out::sequence(&[
            oid(OID_LOCAL_KEY_ID),
            der_out::set_of(vec![der_out::octet_string(&local_key_id)]),
        ]),
    ]);

    // Key: EncryptedPrivateKeyInfo in a shrouded bag, inside plain Data.
    let (key_alg, key_ct) = pbes2_encrypt(&pkcs8.0, password, iterations)?;
    let epki = der_out::sequence(&[key_alg, der_out::octet_string(&key_ct)]);
    let key_bag = der_out::sequence(&[
        oid(OID_SHROUDED_KEY_BAG),
        der_out::context(0, &epki),
        attrs.clone(),
    ]);
    let key_contents = der_out::sequence(&[key_bag]);
    let ci_key = content_info(OID_DATA, &der_out::octet_string(&key_contents));

    // Certificate: CertBag in SafeContents, encrypted as EncryptedData.
    let cert_bag_value = der_out::sequence(&[
        oid(OID_X509_CERTIFICATE),
        der_out::context(0, &der_out::octet_string(certificate)),
    ]);
    let cert_bag = der_out::sequence(&[
        oid(OID_CERT_BAG),
        der_out::context(0, &cert_bag_value),
        attrs,
    ]);
    let cert_contents = der_out::sequence(&[cert_bag]);
    let (cert_alg, cert_ct) = pbes2_encrypt(&cert_contents, password, iterations)?;
    // EncryptedContentInfo { contentType, contentEncryptionAlgorithm,
    // [0] IMPLICIT OCTET STRING } — primitive context tag 0x80.
    let eci = der_out::sequence(&[oid(OID_DATA), cert_alg, der_out::tlv(0x80, &cert_ct)]);
    let encrypted_data = der_out::sequence(&[der_out::integer_u64(0), eci]);
    let ci_cert = content_info(OID_ENCRYPTED_DATA, &encrypted_data);

    let auth_safe = der_out::sequence(&[ci_cert, ci_key]);

    // MAC over the AuthenticatedSafe octets (PX-G1), RFC 7292 App. B KDF id 3.
    let mac_salt = random(16)?;
    let mac_key = SecretBytes(MacHash::Sha256.kdf(password, &mac_salt, MAC_ITERATIONS, 3, 32));
    let mac = MacHash::Sha256.hmac(&mac_key.0, &auth_safe);
    let mac_data = der_out::sequence(&[
        der_out::sequence(&[
            der_out::algorithm_identifier(OID_SHA256, Some(der_out::null())).unwrap_or_default(),
            der_out::octet_string(&mac),
        ]),
        der_out::octet_string(&mac_salt),
        der_out::integer_u64(MAC_ITERATIONS),
    ]);

    Ok(der_out::sequence(&[
        der_out::integer_u64(3),
        content_info(OID_DATA, &der_out::octet_string(&auth_safe)),
        mac_data,
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_bits_trim_trailing_zeros() {
        assert_eq!(named_bits(&[0]), [0x03, 0x02, 0x07, 0x80]);
        assert_eq!(named_bits(&[0, 1]), [0x03, 0x02, 0x06, 0xC0]);
        assert_eq!(named_bits(&[0, 1, 2]), [0x03, 0x02, 0x05, 0xE0]);
        assert_eq!(named_bits(&[0, 8]), [0x03, 0x03, 0x07, 0x80, 0x80]);
    }

    #[test]
    fn times_switch_to_generalized_in_2050() {
        // 2026-09-30T00:00:00Z
        assert_eq!(time(1_790_726_400), b"\x17\x0d260930000000Z");
        // 2051-01-01T00:00:00Z
        let t = time(2_556_144_000);
        assert_eq!(t, b"\x18\x0f20510101000000Z");
    }

    #[test]
    fn add_years_keeps_the_calendar_date() {
        // 2024-02-29T12:00:00Z + 1 year -> 2025-02-28T12:00:00Z
        let leap = days_from_civil(2024, 2, 29) * 86_400 + 12 * 3600;
        assert_eq!(civil(add_years(leap, 1)), (2025, 2, 28, 12, 0, 0));
        assert_eq!(civil(add_years(leap, 4)), (2028, 2, 29, 12, 0, 0));
        assert_eq!(civil(0), (1970, 1, 1, 0, 0, 0));
    }
}

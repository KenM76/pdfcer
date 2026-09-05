//! PKCS#12 (`.pfx` / `.p12`) **import** — RFC 7292, the file-based digital
//! ID that is pdfcer's first key source.
//!
//! Read `security__pkcs12_import.md` (`P12-0`…`P12-14`) for the standard;
//! this header records what the implementation decided where the standard
//! left room.
//!
//! # What comes out
//!
//! A [`Pkcs12Signer`] — the private key (RSA or EC P-256/P-384), the
//! certificate chain leaf-first, and a [`Pkcs12Report`] of what the
//! container was made of (rule 4: which encryption era, which MAC, how many
//! iterations — a shell shows it, the CLI prints it). The signer implements
//! [`super::Signer`] and does nothing else with the key.
//!
//! # Import order (RFC 7292 §5.2, `P12` §6)
//!
//! 1. `PFX { version 3, authSafe, macData }` — version checked (`P12-1`).
//! 2. **MAC first** (`P12-2`): `HMAC-<digest>` keyed by the RFC 7292 Appendix
//!    B KDF (id = 3) over the `authSafe` content octets. A MAC failure *is*
//!    the wrong-password signal ([`Pkcs12Error::MacMismatch`]) and is
//!    reported before anything is decrypted. A file with no `macData`
//!    (`P12-12`) is accepted and the report says the integrity was
//!    unverified — disclosed, never silent.
//! 3. `AuthenticatedSafe = SEQUENCE OF ContentInfo`; each `data` (plain) or
//!    `encryptedData` (decrypted under `P12-9`'s two eras, see below).
//! 4. Every `SafeBag`: `pkcs8ShroudedKeyBag` → decrypt → `PrivateKeyInfo`
//!    → [`super::KeyMaterial`]; `keyBag` (plain PKCS#8, rare) likewise;
//!    `certBag` / `x509Certificate` → DER certificate; bag attributes
//!    `localKeyId` and `friendlyName` recorded.
//! 5. **Pair** the key with its leaf: by equal `localKeyId` first (`P12-6`);
//!    failing that, by public-key identity (the modulus or the EC point of
//!    the private key against each certificate's `SubjectPublicKeyInfo`).
//!    Then order the chain leaf → issuer → issuer… by subject/issuer DN
//!    bytes. Extra, unrelated certificates are dropped and counted.
//!
//! # The two encryption eras, both required (`P12-9`, `P12-10`)
//!
//! | Scheme | Where seen | KDF | Cipher |
//! |---|---|---|---|
//! | **PBES2** (`1.2.840.113549.1.5.13`) | OpenSSL 3, recent Windows/Java | PBKDF2 (HMAC-SHA-1/256/384/512) | AES-128/192/256-CBC, 3-key-3DES-CBC |
//! | `pbeWithSHAAnd3-KeyTripleDES-CBC` (`…1.12.1.3`) | the legacy key bag | RFC 7292 App. B, SHA-1 | 3-key-3DES-CBC |
//! | `pbeWithSHAAnd40BitRC2-CBC` (`…1.12.1.6`) | the legacy cert bags | RFC 7292 App. B, SHA-1 | RC2-CBC, 5-byte key, **40 effective bits** |
//! | `pbeWithSHAAnd128BitRC2-CBC` (`…1.12.1.5`), `…2-KeyTripleDES-CBC` (`…1.12.1.4`) | rarer legacy | same | RC2-128 / 2-key-3DES |
//!
//! The RC4 variants (`…1.12.1.1`, `.2`) are not implemented: they are rare
//! in signing certificates and pdfcer refuses them **by name**
//! ([`Pkcs12Error::UnsupportedScheme`]) rather than guessing.
//!
//! # The KDF details that are classically mis-implemented (`P12-11`)
//!
//! The Appendix B KDF takes the password as a **BMPString with a trailing
//! `0x0000`** — UTF-16BE of each char, then two zero bytes. An empty
//! password is the two zero bytes alone (what OpenSSL does; RFC 7292 B.1 is
//! read both ways and OpenSSL's reading is the one every `.pfx` in the wild
//! was made with). PBES2 (PBKDF2) takes the password as **UTF-8 bytes with
//! no terminator** — a different encoding of the same password inside the
//! same file. RC2-40 is a **5-byte key with the effective key length set to
//! 40 explicitly**.
//!
//! # BER
//!
//! `P12-14`: the standard permits BER (indefinite lengths) inside a PFX.
//! pdfcer's ASN.1 reader is DER-only and this importer inherits that:
//! OpenSSL, Java and Windows all emit definite lengths, so the refusal
//! ([`Pkcs12Error::Malformed`] naming the structure) is expected to be rare.
//! It is recorded here as a known gap rather than papered over.
//!
//! # Provenance
//!
//! Written from RFC 7292 (Appendices A–C), RFC 8018 and RFC 5958 directly.
//! The crate survey (`docs/signing-crate-survey.md` §4) pointed at
//! `p12-keystore`'s `pbes1.rs` as a readable reference for the legacy path;
//! it was consulted for the *fact* that RC2-40 needs an explicit effective
//! key length and for nothing else — no structure or code was borrowed, so
//! no attribution is owed beyond this note. The KDF is checked against the
//! `pkcs12` crate's published test vector (Apache-2.0 OR MIT), quoted in
//! the tests with that credit.
//!
//! # Secrets
//!
//! The password is borrowed for the duration of the call and never stored;
//! the decrypted `PrivateKeyInfo` bytes are zeroized after the key is
//! parsed. The key itself lives inside the `rsa`/`p256` types, which
//! zeroize on drop. Nothing here logs.

use super::{KeyMaterial, SignError, SignatureAlgorithm, Signer};
use crate::asn1::{self, Tlv};

/// Why a `.pfx` could not be imported. Every variant is a refusal by name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Pkcs12Error {
    /// A structure did not parse as the DER the RFC describes. `what` names
    /// the ASN.1 type that failed (`PFX`, `MacData`, `SafeBag`, …). Also the
    /// answer for BER indefinite lengths (module docs).
    #[error(
        "the PKCS#12 container's {what} is malformed (or BER-encoded, which pdfcer does not read)"
    )]
    Malformed {
        /// The ASN.1 production that failed.
        what: &'static str,
    },
    /// `PFX.version` was not 3.
    #[error("PKCS#12 version {found} is not the version 3 RFC 7292 defines")]
    UnsupportedVersion {
        /// What the file said.
        found: u64,
    },
    /// The MAC did not verify under the password — the standard
    /// wrong-password signal. (Or the file is corrupt; the two are
    /// indistinguishable by design.)
    #[error(
        "the password is wrong, or the container is corrupt: its integrity MAC ({mac}) did not verify"
    )]
    MacMismatch {
        /// The MAC's digest algorithm name (`SHA-1`, `SHA-256`).
        mac: String,
    },
    /// An encryption or MAC algorithm pdfcer does not implement.
    #[error("the container uses {oid} ({what}), which pdfcer does not implement")]
    UnsupportedScheme {
        /// The algorithm OID as found.
        oid: String,
        /// Which role it played (`key bag`, `cert bags`, `MAC`, `PBKDF2 PRF`, `PBES2 cipher`).
        what: &'static str,
    },
    /// Decryption produced bytes that were not the expected structure —
    /// with PBES2 that almost always means a wrong password on a container
    /// with no MAC to catch it first.
    #[error(
        "decrypting the {what} produced garbage — the password is wrong, or the container is corrupt"
    )]
    DecryptFailed {
        /// Which encrypted structure.
        what: &'static str,
    },
    /// The container holds no private key bag.
    #[error("the container holds certificates but no private key, so it cannot sign")]
    NoPrivateKey,
    /// The container holds more than one private key. pdfcer signs with one
    /// identity per call and will not pick silently.
    #[error(
        "the container holds {count} private keys; pdfcer needs exactly one identity per container"
    )]
    MultipleKeys {
        /// How many were found.
        count: usize,
    },
    /// The key is neither RSA nor EC on P-256/P-384.
    #[error("the private key algorithm {algorithm}{} is not one pdfcer can sign with (RSA, EC P-256, EC P-384)", curve.as_deref().map(|c| format!(" (curve {c})")).unwrap_or_default())]
    UnsupportedKey {
        /// The `privateKeyAlgorithm` OID.
        algorithm: String,
        /// The named curve OID, for EC keys.
        curve: Option<String>,
    },
    /// The key bytes did not parse in the key crate.
    #[error("the private key did not parse: {detail}")]
    PrivateKey {
        /// The crate's message.
        detail: String,
    },
    /// No certificate in the container matches the private key.
    #[error(
        "none of the {certificates} certificate(s) in the container belongs to the private key"
    )]
    NoMatchingCertificate {
        /// How many certificates were present.
        certificates: usize,
    },
}

/// What a `.pfx` was made of — the rule-4 disclosure that travels with a
/// [`Pkcs12Signer`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Pkcs12Report {
    /// The MAC digest (`SHA-1`, `SHA-256`, …) that verified, or `None` when
    /// the container carried no `macData` and integrity was **not** checked.
    pub mac: Option<String>,
    /// MAC iteration count, when a MAC was present.
    pub mac_iterations: Option<u64>,
    /// The scheme that protected the private key, as a label:
    /// `PBES2/PBKDF2-HMAC-SHA256/AES-256-CBC`, `pbeWithSHAAnd3-KeyTripleDES-CBC`,
    /// or `none` for a plain `keyBag`.
    pub key_scheme: String,
    /// The scheme(s) that protected the certificate bags, deduplicated, in
    /// the order met. Empty when the certificates were in plain `data`.
    pub cert_schemes: Vec<String>,
    /// The key's kind (`RSA-2048`, `EC P-256`).
    pub key: String,
    /// Certificates in the chain that was kept, leaf first.
    pub chain_length: usize,
    /// Certificates in the container that belonged to no chain from the
    /// key's leaf and were dropped.
    pub unrelated_certificates: usize,
    /// The leaf's `friendlyName` bag attribute, if any.
    pub friendly_name: Option<String>,
    /// The leaf's subject DN as `cms.rs` renders it.
    pub subject: String,
}

/// A private key and its certificate chain, loaded from a PKCS#12 file.
/// The one in-core [`Signer`].
pub struct Pkcs12Signer {
    key: KeyMaterial,
    chain: Vec<Vec<u8>>,
    report: Pkcs12Report,
}

impl std::fmt::Debug for Pkcs12Signer {
    // Never print key material; the report is the public face.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pkcs12Signer")
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

impl Pkcs12Signer {
    /// Open a `.pfx`/`.p12` from its bytes with `password`.
    ///
    /// # Errors
    ///
    /// Every [`Pkcs12Error`]; [`Pkcs12Error::MacMismatch`] is the wrong
    /// password.
    pub fn from_der(pfx: &[u8], password: &str) -> Result<Self, Pkcs12Error> {
        let malformed = |what: &'static str| Pkcs12Error::Malformed { what };

        // PFX ::= SEQUENCE { version INTEGER, authSafe ContentInfo, macData MacData OPTIONAL }
        let (pfx_tlv, _) = asn1::expect(pfx, asn1::SEQUENCE).ok_or(malformed("PFX"))?;
        let pfx_kids = asn1::children(pfx_tlv).ok_or(malformed("PFX"))?;
        let version = pfx_kids
            .first()
            .and_then(|t| asn1::integer_bytes(*t))
            .and_then(int_u64)
            .ok_or(malformed("PFX.version"))?;
        if version != 3 {
            return Err(Pkcs12Error::UnsupportedVersion { found: version });
        }
        let auth_safe_ci = pfx_kids.get(1).copied().ok_or(malformed("PFX.authSafe"))?;
        let (auth_safe_type, auth_safe_content) =
            content_info(auth_safe_ci).ok_or(malformed("PFX.authSafe"))?;
        if auth_safe_type != crate::cms::oid::DATA {
            // Public-key integrity mode (signedData) — not implemented.
            return Err(Pkcs12Error::UnsupportedScheme {
                oid: auth_safe_type,
                what: "authSafe (public-key integrity mode)",
            });
        }
        // content [0] EXPLICIT OCTET STRING — the AuthenticatedSafe BER/DER.
        let auth_safe_octets = asn1::expect(auth_safe_content, asn1::OCTET_STRING)
            .map(|(t, _)| t.content)
            .ok_or(malformed("PFX.authSafe content"))?;

        // --- MAC first (P12-2) ---
        let mut mac_label = None;
        let mut mac_iterations = None;
        if let Some(mac_data) = pfx_kids.get(2) {
            let (label, iterations) = verify_mac(*mac_data, auth_safe_octets, password)?;
            mac_label = Some(label);
            mac_iterations = Some(iterations);
        }

        // --- AuthenticatedSafe ::= SEQUENCE OF ContentInfo ---
        let (safe_tlv, _) =
            asn1::expect(auth_safe_octets, asn1::SEQUENCE).ok_or(malformed("AuthenticatedSafe"))?;
        let mut bags: Vec<Bag> = Vec::new();
        let mut cert_schemes: Vec<String> = Vec::new();
        for ci in asn1::children(safe_tlv).ok_or(malformed("AuthenticatedSafe"))? {
            let (ty, content) =
                content_info(ci).ok_or(malformed("AuthenticatedSafe.ContentInfo"))?;
            let safe_contents: Vec<u8> = match ty.as_str() {
                crate::cms::oid::DATA => asn1::expect(content, asn1::OCTET_STRING)
                    .map(|(t, _)| t.content.to_vec())
                    .ok_or(malformed("SafeContents"))?,
                "1.2.840.113549.1.7.6" => {
                    // EncryptedData ::= SEQUENCE { version, encryptedContentInfo SEQUENCE {
                    //   contentType, contentEncryptionAlgorithm, encryptedContent [0] IMPLICIT OCTET STRING } }
                    let (ed, _) =
                        asn1::expect(content, asn1::SEQUENCE).ok_or(malformed("EncryptedData"))?;
                    let ed_kids = asn1::children(ed).ok_or(malformed("EncryptedData"))?;
                    let eci = ed_kids
                        .get(1)
                        .copied()
                        .ok_or(malformed("EncryptedContentInfo"))?;
                    let eci_kids = asn1::children(eci).ok_or(malformed("EncryptedContentInfo"))?;
                    let alg = eci_kids
                        .get(1)
                        .copied()
                        .ok_or(malformed("EncryptedContentInfo"))?;
                    let ciphertext = eci_kids
                        .get(2)
                        .filter(|t| t.tag == asn1::context(0) & !0x20 || t.tag == asn1::context(0))
                        .map(|t| t.content)
                        .ok_or(malformed("EncryptedContentInfo.encryptedContent"))?;
                    let scheme = Scheme::parse(alg, "cert bags")?;
                    if !cert_schemes.contains(&scheme.label) {
                        cert_schemes.push(scheme.label.clone());
                    }
                    scheme.decrypt(password, ciphertext, "cert bags")?
                }
                other => {
                    return Err(Pkcs12Error::UnsupportedScheme {
                        oid: other.to_owned(),
                        what: "AuthenticatedSafe content (public-key enveloped)",
                    });
                }
            };
            collect_bags(&safe_contents, password, &mut bags)?;
        }

        // --- pick the key, pair the leaf, order the chain ---
        let keys: Vec<&Bag> = bags
            .iter()
            .filter(|b| matches!(b.kind, BagKind::Key { .. }))
            .collect();
        let key_bag = match keys.as_slice() {
            [] => return Err(Pkcs12Error::NoPrivateKey),
            [one] => *one,
            many => {
                return Err(Pkcs12Error::MultipleKeys { count: many.len() });
            }
        };
        let (key, key_scheme) = match &key_bag.kind {
            BagKind::Key { material, scheme } => (material, scheme.clone()),
            BagKind::Cert(_) => unreachable!("filtered to key bags above"),
        };
        let certs: Vec<&Bag> = bags
            .iter()
            .filter(|b| matches!(b.kind, BagKind::Cert(_)))
            .collect();
        let cert_der = |b: &Bag| match &b.kind {
            BagKind::Cert(der) => der.clone(),
            BagKind::Key { .. } => Vec::new(),
        };

        let identity = key.public_identity();
        let leaf = certs
            .iter()
            .find(|c| key_bag.local_key_id.is_some() && c.local_key_id == key_bag.local_key_id)
            .or_else(|| {
                certs.iter().find(|c| {
                    crate::cms::parse_certificate(match &c.kind {
                        BagKind::Cert(d) => d,
                        BagKind::Key { .. } => &[],
                    })
                    .is_some_and(|cert| public_identity_of(&cert.key) == identity)
                })
            })
            .copied()
            .ok_or(Pkcs12Error::NoMatchingCertificate {
                certificates: certs.len(),
            })?;

        // Chain: leaf, then whoever issued the last one, until nobody did or
        // it is self-signed.
        let mut chain = vec![cert_der(leaf)];
        let mut used = vec![std::ptr::from_ref(leaf)];
        while let Some(current) = chain.last().and_then(|d| crate::cms::parse_certificate(d)) {
            if current.subject_der == current.issuer_der {
                break;
            }
            let issuer_der = current.issuer_der.to_vec();
            let next = certs.iter().find(|c| {
                !used.contains(&std::ptr::from_ref(**c))
                    && crate::cms::parse_certificate(&cert_der(c))
                        .is_some_and(|cc| cc.subject_der == issuer_der.as_slice())
            });
            match next {
                Some(c) => {
                    used.push(std::ptr::from_ref(*c));
                    chain.push(cert_der(c));
                }
                None => break,
            }
        }
        let unrelated = certs.len().saturating_sub(chain.len());
        let leaf_der_owned = cert_der(leaf);
        let leaf_cert = crate::cms::parse_certificate(&leaf_der_owned);

        let report = Pkcs12Report {
            mac: mac_label,
            mac_iterations,
            key_scheme,
            cert_schemes,
            key: key.label(),
            chain_length: chain.len(),
            unrelated_certificates: unrelated,
            friendly_name: leaf
                .friendly_name
                .clone()
                .or_else(|| key_bag.friendly_name.clone()),
            subject: leaf_cert.map(|c| c.subject).unwrap_or_default(),
        };
        let BagKind::Key { material, .. } = bags
            .into_iter()
            .find_map(|b| match b.kind {
                k @ BagKind::Key { .. } => Some(k),
                BagKind::Cert(_) => None,
            })
            .ok_or(Pkcs12Error::NoPrivateKey)?
        else {
            return Err(Pkcs12Error::NoPrivateKey);
        };
        Ok(Self {
            key: material,
            chain,
            report,
        })
    }

    /// What the container was made of.
    #[must_use]
    pub fn report(&self) -> &Pkcs12Report {
        &self.report
    }
}

impl Signer for Pkcs12Signer {
    fn sign(&self, algorithm: SignatureAlgorithm, message: &[u8]) -> Result<Vec<u8>, SignError> {
        self.key.sign(algorithm, message)
    }

    fn certificate_chain(&self) -> &[Vec<u8>] {
        &self.chain
    }

    fn default_algorithm(&self) -> SignatureAlgorithm {
        self.key.default_algorithm()
    }

    fn key_label(&self) -> String {
        self.key.label()
    }
}

// ---------------------------------------------------------------------------
// Bags
// ---------------------------------------------------------------------------

enum BagKind {
    Key {
        material: KeyMaterial,
        scheme: String,
    },
    Cert(Vec<u8>),
}

struct Bag {
    kind: BagKind,
    local_key_id: Option<Vec<u8>>,
    friendly_name: Option<String>,
}

/// Walk a decrypted/plain `SafeContents ::= SEQUENCE OF SafeBag`, appending
/// key and certificate bags (recursing into `safeContentsBag`).
fn collect_bags(
    safe_contents: &[u8],
    password: &str,
    out: &mut Vec<Bag>,
) -> Result<(), Pkcs12Error> {
    let malformed = |what: &'static str| Pkcs12Error::Malformed { what };
    let (sc, _) = asn1::expect(safe_contents, asn1::SEQUENCE).ok_or(malformed("SafeContents"))?;
    for bag in asn1::children(sc).ok_or(malformed("SafeContents"))? {
        // SafeBag ::= SEQUENCE { bagId OID, bagValue [0] EXPLICIT ANY, bagAttributes SET OPTIONAL }
        let kids = asn1::children(bag).ok_or(malformed("SafeBag"))?;
        let bag_id = kids
            .first()
            .filter(|t| t.tag == asn1::OID)
            .and_then(|t| asn1::oid_to_string(t.content))
            .ok_or(malformed("SafeBag.bagId"))?;
        let value = kids
            .get(1)
            .filter(|t| t.tag == asn1::context(0))
            .map(|t| t.content)
            .ok_or(malformed("SafeBag.bagValue"))?;
        let (local_key_id, friendly_name) = kids
            .get(2)
            .map_or((None, None), |attrs| bag_attributes(*attrs));

        match bag_id.as_str() {
            // keyBag: PrivateKeyInfo in the clear.
            "1.2.840.113549.1.12.10.1.1" => {
                let material = KeyMaterial::from_pkcs8_der(value)?;
                out.push(Bag {
                    kind: BagKind::Key {
                        material,
                        scheme: "none".to_owned(),
                    },
                    local_key_id,
                    friendly_name,
                });
            }
            // pkcs8ShroudedKeyBag: EncryptedPrivateKeyInfo ::= SEQUENCE { AlgorithmIdentifier, OCTET STRING }
            "1.2.840.113549.1.12.10.1.2" => {
                let (epki, _) = asn1::expect(value, asn1::SEQUENCE)
                    .ok_or(malformed("EncryptedPrivateKeyInfo"))?;
                let parts = asn1::children(epki).ok_or(malformed("EncryptedPrivateKeyInfo"))?;
                let alg = parts
                    .first()
                    .copied()
                    .ok_or(malformed("EncryptedPrivateKeyInfo"))?;
                let ciphertext = parts
                    .get(1)
                    .filter(|t| t.tag == asn1::OCTET_STRING)
                    .map(|t| t.content)
                    .ok_or(malformed("EncryptedPrivateKeyInfo.encryptedData"))?;
                let scheme = Scheme::parse(alg, "key bag")?;
                let mut plaintext = scheme.decrypt(password, ciphertext, "key bag")?;
                let material = KeyMaterial::from_pkcs8_der(&plaintext);
                // Zeroize the decrypted PKCS#8 regardless of outcome.
                plaintext.fill(0);
                let material = material?;
                out.push(Bag {
                    kind: BagKind::Key {
                        material,
                        scheme: scheme.label,
                    },
                    local_key_id,
                    friendly_name,
                });
            }
            // certBag ::= SEQUENCE { certId OID, certValue [0] EXPLICIT OCTET STRING }
            "1.2.840.113549.1.12.10.1.3" => {
                let (cb, _) = asn1::expect(value, asn1::SEQUENCE).ok_or(malformed("CertBag"))?;
                let parts = asn1::children(cb).ok_or(malformed("CertBag"))?;
                let cert_id = parts
                    .first()
                    .and_then(|t| asn1::oid_to_string(t.content))
                    .ok_or(malformed("CertBag.certId"))?;
                if cert_id != "1.2.840.113549.1.9.22.1" {
                    // sdsiCertificate or unknown — skip, it is not X.509.
                    continue;
                }
                let der = parts
                    .get(1)
                    .filter(|t| t.tag == asn1::context(0))
                    .and_then(|t| asn1::expect(t.content, asn1::OCTET_STRING))
                    .map(|(t, _)| t.content.to_vec())
                    .ok_or(malformed("CertBag.certValue"))?;
                out.push(Bag {
                    kind: BagKind::Cert(der),
                    local_key_id,
                    friendly_name,
                });
            }
            // safeContentsBag: nested SafeContents.
            "1.2.840.113549.1.12.10.1.6" => collect_bags(value, password, out)?,
            // crlBag, secretBag: nothing a signer needs.
            _ => {}
        }
    }
    Ok(())
}

/// `bagAttributes SET OF PKCS12Attribute { attrId OID, attrValues SET OF ANY }`
/// → (`localKeyId`, `friendlyName`).
fn bag_attributes(attrs: Tlv<'_>) -> (Option<Vec<u8>>, Option<String>) {
    let mut local_key_id = None;
    let mut friendly_name = None;
    for attr in asn1::children(attrs).unwrap_or_default() {
        let Some(kids) = asn1::children(attr) else {
            continue;
        };
        let Some(oid) = kids.first().and_then(|t| asn1::oid_to_string(t.content)) else {
            continue;
        };
        let Some(values) = kids.get(1).and_then(|t| asn1::children(*t)) else {
            continue;
        };
        let Some(first) = values.first() else {
            continue;
        };
        match oid.as_str() {
            "1.2.840.113549.1.9.21" if first.tag == asn1::OCTET_STRING => {
                local_key_id = Some(first.content.to_vec());
            }
            "1.2.840.113549.1.9.20" => friendly_name = asn1::string_value(*first),
            _ => {}
        }
    }
    (local_key_id, friendly_name)
}

/// `ContentInfo ::= SEQUENCE { contentType OID, content [0] EXPLICIT ANY }`
/// → (type OID, the content octets inside the `[0]`).
fn content_info(tlv: Tlv<'_>) -> Option<(String, &[u8])> {
    let kids = asn1::children(tlv)?;
    let ty = asn1::oid_to_string(kids.first().filter(|t| t.tag == asn1::OID)?.content)?;
    let content = kids.get(1).filter(|t| t.tag == asn1::context(0))?.content;
    Some((ty, content))
}

fn int_u64(bytes: &[u8]) -> Option<u64> {
    if bytes.len() > 8 {
        return None;
    }
    Some(bytes.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b)))
}

/// The bytes of a certificate's public key that [`KeyMaterial::public_identity`]
/// compares against.
fn public_identity_of(key: &crate::cms::PublicKey<'_>) -> Vec<u8> {
    match key {
        crate::cms::PublicKey::Rsa { n, .. } => n.to_vec(),
        crate::cms::PublicKey::Ec { point, .. } => point.to_vec(),
        crate::cms::PublicKey::Other(_) => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// MAC (RFC 7292 Appendix A)
// ---------------------------------------------------------------------------

/// `MacData ::= SEQUENCE { mac DigestInfo, macSalt OCTET STRING, iterations INTEGER DEFAULT 1 }`;
/// `DigestInfo ::= SEQUENCE { digestAlgorithm AlgorithmIdentifier, digest OCTET STRING }`.
/// Returns (digest label, iterations) on success.
fn verify_mac(
    mac_data: Tlv<'_>,
    auth_safe_octets: &[u8],
    password: &str,
) -> Result<(String, u64), Pkcs12Error> {
    let malformed = |what: &'static str| Pkcs12Error::Malformed { what };
    let kids = asn1::children(mac_data).ok_or(malformed("MacData"))?;
    let digest_info = asn1::children(*kids.first().ok_or(malformed("MacData.mac"))?)
        .ok_or(malformed("DigestInfo"))?;
    let alg = asn1::children(*digest_info.first().ok_or(malformed("DigestInfo"))?)
        .ok_or(malformed("DigestInfo"))?;
    let digest_oid = alg
        .first()
        .and_then(|t| asn1::oid_to_string(t.content))
        .ok_or(malformed("DigestInfo.algorithm"))?;
    let expected = digest_info
        .get(1)
        .filter(|t| t.tag == asn1::OCTET_STRING)
        .map(|t| t.content)
        .ok_or(malformed("DigestInfo.digest"))?;
    let salt = kids
        .get(1)
        .filter(|t| t.tag == asn1::OCTET_STRING)
        .map(|t| t.content)
        .ok_or(malformed("MacData.macSalt"))?;
    let iterations = kids
        .get(2)
        .and_then(|t| asn1::integer_bytes(*t))
        .and_then(int_u64)
        .unwrap_or(1);

    let hash = MacHash::from_oid(&digest_oid).ok_or(Pkcs12Error::UnsupportedScheme {
        oid: digest_oid.clone(),
        what: "MAC",
    })?;
    let key = hash.kdf(password, salt, iterations, 3, hash.len());
    let computed = hash.hmac(&key, auth_safe_octets);
    // Constant-time compare is not needed here: the MAC value is public
    // (it is in the file) and the only secret, the password, is not
    // reachable from timing of this comparison. Plain equality.
    if computed != expected {
        return Err(Pkcs12Error::MacMismatch {
            mac: hash.label().to_owned(),
        });
    }
    Ok((hash.label().to_owned(), iterations))
}

/// The hash family the PKCS#12 KDF and MAC run on. SHA-1 is the legacy
/// default and the only one every old `.pfx` uses; SHA-2 is what OpenSSL 3
/// writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MacHash {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl MacHash {
    fn from_oid(oid: &str) -> Option<Self> {
        Some(match oid {
            "1.3.14.3.2.26" => Self::Sha1,
            "2.16.840.1.101.3.4.2.1" => Self::Sha256,
            "2.16.840.1.101.3.4.2.2" => Self::Sha384,
            "2.16.840.1.101.3.4.2.3" => Self::Sha512,
            _ => return None,
        })
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Sha1 => "SHA-1",
            Self::Sha256 => "SHA-256",
            Self::Sha384 => "SHA-384",
            Self::Sha512 => "SHA-512",
        }
    }

    /// Output length `u` in bytes.
    const fn len(self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }

    /// Block length `v` in bytes (RFC 7292 B.2: 64 for SHA-1/224/256, 128 for
    /// SHA-384/512).
    const fn block(self) -> usize {
        match self {
            Self::Sha1 | Self::Sha256 => 64,
            Self::Sha384 | Self::Sha512 => 128,
        }
    }

    fn hash(self, data: &[u8]) -> Vec<u8> {
        use sha2::Digest as _;
        match self {
            Self::Sha1 => sha1::Sha1::digest(data).to_vec(),
            Self::Sha256 => sha2::Sha256::digest(data).to_vec(),
            Self::Sha384 => sha2::Sha384::digest(data).to_vec(),
            Self::Sha512 => sha2::Sha512::digest(data).to_vec(),
        }
    }

    fn hmac(self, key: &[u8], data: &[u8]) -> Vec<u8> {
        use hmac::{KeyInit as _, Mac as _};
        macro_rules! run {
            ($d:ty) => {{
                let Ok(mut m) = <hmac::Hmac<$d>>::new_from_slice(key) else {
                    return Vec::new();
                };
                m.update(data);
                m.finalize().into_bytes().to_vec()
            }};
        }
        match self {
            Self::Sha1 => run!(sha1::Sha1),
            Self::Sha256 => run!(sha2::Sha256),
            Self::Sha384 => run!(sha2::Sha384),
            Self::Sha512 => run!(sha2::Sha512),
        }
    }

    /// RFC 7292 Appendix B.2 — the PKCS#12 key derivation function.
    ///
    /// `id`: 1 = encryption key, 2 = IV, 3 = MAC key. `n`: bytes wanted.
    /// The password enters as BMPString + NUL (`P12-11`, module docs).
    fn kdf(self, password: &str, salt: &[u8], iterations: u64, id: u8, n: usize) -> Vec<u8> {
        let u = self.len();
        let v = self.block();
        // D: `id` repeated v times.
        let d = vec![id; v];
        // P: the BMPString password, then S: the salt, each repeated to a
        // whole number of v-blocks (empty stays empty).
        let mut pw: Vec<u8> = password.encode_utf16().flat_map(u16::to_be_bytes).collect();
        pw.extend_from_slice(&[0, 0]);
        let repeat_to_v = |src: &[u8]| -> Vec<u8> {
            if src.is_empty() {
                return Vec::new();
            }
            let len = v * src.len().div_ceil(v);
            src.iter().copied().cycle().take(len).collect()
        };
        let s = repeat_to_v(salt);
        let p = repeat_to_v(&pw);
        let mut i: Vec<u8> = [s, p].concat();

        let c = n.div_ceil(u);
        let mut out = Vec::with_capacity(c * u);
        for _ in 0..c {
            // A = H^r(D || I)
            let mut a = self.hash(&[d.as_slice(), i.as_slice()].concat());
            for _ in 1..iterations {
                a = self.hash(&a);
            }
            out.extend_from_slice(&a);
            // B = A repeated to v bytes; each v-block of I += (B + 1), big-endian.
            let b: Vec<u8> = a.iter().copied().cycle().take(v).collect();
            for block in i.chunks_mut(v) {
                let mut carry = 1u16;
                for (x, &y) in block.iter_mut().rev().zip(b.iter().rev()) {
                    let sum = u16::from(*x) + u16::from(y) + carry;
                    #[allow(clippy::cast_possible_truncation)] // masked
                    {
                        *x = (sum & 0xFF) as u8;
                    }
                    carry = sum >> 8;
                }
            }
        }
        out.truncate(n);
        out
    }
}

// ---------------------------------------------------------------------------
// Encryption schemes (RFC 7292 Appendix B / RFC 8018)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cipher {
    Aes128,
    Aes192,
    Aes256,
    /// 3-key triple DES (`des-EDE3-CBC`).
    TdesEde3,
    /// 2-key triple DES (K1, K2, K1).
    TdesEde2,
    /// RC2 with the given key bytes and effective bits.
    Rc2 {
        key_len: usize,
        effective_bits: usize,
    },
}

impl Cipher {
    const fn key_len(self) -> usize {
        match self {
            Self::Aes128 => 16,
            Self::Aes192 => 24,
            Self::Aes256 => 32,
            Self::TdesEde3 => 24,
            Self::TdesEde2 => 16,
            Self::Rc2 { key_len, .. } => key_len,
        }
    }

    const fn iv_len(self) -> usize {
        match self {
            Self::Aes128 | Self::Aes192 | Self::Aes256 => 16,
            Self::TdesEde3 | Self::TdesEde2 | Self::Rc2 { .. } => 8,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Aes128 => "AES-128-CBC",
            Self::Aes192 => "AES-192-CBC",
            Self::Aes256 => "AES-256-CBC",
            Self::TdesEde3 => "3-key-3DES-CBC",
            Self::TdesEde2 => "2-key-3DES-CBC",
            Self::Rc2 {
                effective_bits: 40, ..
            } => "RC2-40-CBC",
            Self::Rc2 { .. } => "RC2-128-CBC",
        }
    }

    /// CBC-decrypt with PKCS#7 unpadding. `None` when the key/IV lengths are
    /// wrong for the cipher or the padding is invalid (wrong password).
    fn decrypt(self, key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
        use aes::cipher::block_padding::Pkcs7;
        use aes::cipher::{BlockModeDecrypt as _, InnerIvInit as _, KeyIvInit as _};
        macro_rules! cbc {
            ($c:ty) => {
                cbc::Decryptor::<$c>::new_from_slices(key, iv)
                    .ok()?
                    .decrypt_padded_vec::<Pkcs7>(ciphertext)
                    .ok()
            };
        }
        match self {
            Self::Aes128 => cbc!(aes::Aes128),
            Self::Aes192 => cbc!(aes::Aes192),
            Self::Aes256 => cbc!(aes::Aes256),
            Self::TdesEde3 => cbc!(des::TdesEde3),
            Self::TdesEde2 => cbc!(des::TdesEde2),
            Self::Rc2 { effective_bits, .. } => {
                if iv.len() != 8 {
                    return None;
                }
                let inner = rc2::Rc2::new_with_eff_key_len(key, effective_bits);
                let iv_arr: [u8; 8] = iv.try_into().ok()?;
                cbc::Decryptor::<rc2::Rc2>::inner_iv_init(inner, &iv_arr.into())
                    .decrypt_padded_vec::<Pkcs7>(ciphertext)
                    .ok()
            }
        }
    }
}

/// A parsed password-based encryption scheme, ready to derive and decrypt.
struct Scheme {
    label: String,
    kind: SchemeKind,
}

enum SchemeKind {
    /// RFC 7292 Appendix B: SHA-1 KDF, salt + iterations.
    Pkcs12Pbe {
        cipher: Cipher,
        salt: Vec<u8>,
        iterations: u64,
    },
    /// RFC 8018 PBES2: PBKDF2 with the named PRF, explicit IV.
    Pbes2 {
        prf: MacHash,
        cipher: Cipher,
        salt: Vec<u8>,
        iterations: u32,
        iv: Vec<u8>,
    },
}

impl Scheme {
    /// Parse an `AlgorithmIdentifier` naming a PBE scheme.
    fn parse(alg: Tlv<'_>, role: &'static str) -> Result<Self, Pkcs12Error> {
        let malformed = |what: &'static str| Pkcs12Error::Malformed { what };
        let kids = asn1::children(alg).ok_or(malformed("AlgorithmIdentifier"))?;
        let oid = kids
            .first()
            .filter(|t| t.tag == asn1::OID)
            .and_then(|t| asn1::oid_to_string(t.content))
            .ok_or(malformed("AlgorithmIdentifier"))?;
        let params = kids.get(1).copied();
        let unsupported = |oid: &str| Pkcs12Error::UnsupportedScheme {
            oid: oid.to_owned(),
            what: role,
        };

        // pkcs-12PbeIds (RFC 7292 Appendix C)
        let legacy = |cipher: Cipher| -> Result<Self, Pkcs12Error> {
            // pkcs-12PbeParams ::= SEQUENCE { salt OCTET STRING, iterations INTEGER }
            let p = params
                .and_then(asn1::children)
                .ok_or(malformed("pkcs-12PbeParams"))?;
            let salt = p
                .first()
                .filter(|t| t.tag == asn1::OCTET_STRING)
                .map(|t| t.content.to_vec())
                .ok_or(malformed("pkcs-12PbeParams.salt"))?;
            let iterations = p
                .get(1)
                .and_then(|t| asn1::integer_bytes(*t))
                .and_then(int_u64)
                .ok_or(malformed("pkcs-12PbeParams.iterations"))?;
            Ok(Self {
                label: format!("{}/{}", legacy_name(&oid), cipher.label()),
                kind: SchemeKind::Pkcs12Pbe {
                    cipher,
                    salt,
                    iterations,
                },
            })
        };

        match oid.as_str() {
            "1.2.840.113549.1.12.1.3" => legacy(Cipher::TdesEde3),
            "1.2.840.113549.1.12.1.4" => legacy(Cipher::TdesEde2),
            "1.2.840.113549.1.12.1.5" => legacy(Cipher::Rc2 {
                key_len: 16,
                effective_bits: 128,
            }),
            "1.2.840.113549.1.12.1.6" => legacy(Cipher::Rc2 {
                key_len: 5,
                effective_bits: 40,
            }),
            // PBES2 ::= SEQUENCE { keyDerivationFunc AlgorithmIdentifier, encryptionScheme AlgorithmIdentifier }
            "1.2.840.113549.1.5.13" => {
                let p = params
                    .and_then(asn1::children)
                    .ok_or(malformed("PBES2-params"))?;
                let kdf = asn1::children(*p.first().ok_or(malformed("PBES2-params"))?)
                    .ok_or(malformed("PBES2 KDF"))?;
                let kdf_oid = kdf
                    .first()
                    .and_then(|t| asn1::oid_to_string(t.content))
                    .ok_or(malformed("PBES2 KDF"))?;
                if kdf_oid != "1.2.840.113549.1.5.12" {
                    return Err(Pkcs12Error::UnsupportedScheme {
                        oid: kdf_oid,
                        what: "PBES2 key derivation",
                    });
                }
                // PBKDF2-params ::= SEQUENCE { salt OCTET STRING, iterationCount INTEGER,
                //   keyLength INTEGER OPTIONAL, prf AlgorithmIdentifier DEFAULT hmacWithSHA1 }
                let kp = kdf
                    .get(1)
                    .copied()
                    .and_then(asn1::children)
                    .ok_or(malformed("PBKDF2-params"))?;
                let salt = kp
                    .first()
                    .filter(|t| t.tag == asn1::OCTET_STRING)
                    .map(|t| t.content.to_vec())
                    .ok_or(malformed("PBKDF2-params.salt"))?;
                let iterations = kp
                    .get(1)
                    .and_then(|t| asn1::integer_bytes(*t))
                    .and_then(int_u64)
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or(malformed("PBKDF2-params.iterationCount"))?;
                // Skip an optional keyLength INTEGER; the PRF is the trailing SEQUENCE if any.
                let prf = kp
                    .iter()
                    .skip(2)
                    .find(|t| t.tag == asn1::SEQUENCE)
                    .and_then(|t| asn1::children(*t))
                    .and_then(|c| c.first().and_then(|t| asn1::oid_to_string(t.content)))
                    .map_or(Ok(MacHash::Sha1), |prf_oid| match prf_oid.as_str() {
                        "1.2.840.113549.2.7" => Ok(MacHash::Sha1),
                        "1.2.840.113549.2.9" => Ok(MacHash::Sha256),
                        "1.2.840.113549.2.10" => Ok(MacHash::Sha384),
                        "1.2.840.113549.2.11" => Ok(MacHash::Sha512),
                        _ => Err(Pkcs12Error::UnsupportedScheme {
                            oid: prf_oid,
                            what: "PBKDF2 PRF",
                        }),
                    })?;
                let enc = asn1::children(*p.get(1).ok_or(malformed("PBES2-params"))?)
                    .ok_or(malformed("PBES2 cipher"))?;
                let enc_oid = enc
                    .first()
                    .and_then(|t| asn1::oid_to_string(t.content))
                    .ok_or(malformed("PBES2 cipher"))?;
                let cipher = match enc_oid.as_str() {
                    "2.16.840.1.101.3.4.1.2" => Cipher::Aes128,
                    "2.16.840.1.101.3.4.1.22" => Cipher::Aes192,
                    "2.16.840.1.101.3.4.1.42" => Cipher::Aes256,
                    "1.2.840.113549.3.7" => Cipher::TdesEde3,
                    _ => {
                        return Err(Pkcs12Error::UnsupportedScheme {
                            oid: enc_oid,
                            what: "PBES2 cipher",
                        });
                    }
                };
                let iv = enc
                    .get(1)
                    .filter(|t| t.tag == asn1::OCTET_STRING)
                    .map(|t| t.content.to_vec())
                    .ok_or(malformed("PBES2 cipher IV"))?;
                Ok(Self {
                    label: format!("PBES2/PBKDF2-HMAC-{}/{}", prf.label(), cipher.label()),
                    kind: SchemeKind::Pbes2 {
                        prf,
                        cipher,
                        salt,
                        iterations,
                        iv,
                    },
                })
            }
            other => Err(unsupported(other)),
        }
    }

    fn decrypt(
        &self,
        password: &str,
        ciphertext: &[u8],
        what: &'static str,
    ) -> Result<Vec<u8>, Pkcs12Error> {
        let plaintext = match &self.kind {
            SchemeKind::Pkcs12Pbe {
                cipher,
                salt,
                iterations,
            } => {
                let key = MacHash::Sha1.kdf(password, salt, *iterations, 1, cipher.key_len());
                let iv = MacHash::Sha1.kdf(password, salt, *iterations, 2, cipher.iv_len());
                cipher.decrypt(&key, &iv, ciphertext)
            }
            SchemeKind::Pbes2 {
                prf,
                cipher,
                salt,
                iterations,
                iv,
            } => {
                let mut key = vec![0u8; cipher.key_len()];
                match prf {
                    MacHash::Sha1 => pbkdf2::pbkdf2_hmac::<sha1::Sha1>(
                        password.as_bytes(),
                        salt,
                        *iterations,
                        &mut key,
                    ),
                    MacHash::Sha256 => pbkdf2::pbkdf2_hmac::<sha2::Sha256>(
                        password.as_bytes(),
                        salt,
                        *iterations,
                        &mut key,
                    ),
                    MacHash::Sha384 => pbkdf2::pbkdf2_hmac::<sha2::Sha384>(
                        password.as_bytes(),
                        salt,
                        *iterations,
                        &mut key,
                    ),
                    MacHash::Sha512 => pbkdf2::pbkdf2_hmac::<sha2::Sha512>(
                        password.as_bytes(),
                        salt,
                        *iterations,
                        &mut key,
                    ),
                }
                cipher.decrypt(&key, iv, ciphertext)
            }
        };
        plaintext.ok_or(Pkcs12Error::DecryptFailed { what })
    }
}

fn legacy_name(oid: &str) -> &'static str {
    match oid {
        "1.2.840.113549.1.12.1.3" => "pbeWithSHAAnd3-KeyTripleDES-CBC",
        "1.2.840.113549.1.12.1.4" => "pbeWithSHAAnd2-KeyTripleDES-CBC",
        "1.2.840.113549.1.12.1.5" => "pbeWithSHAAnd128BitRC2-CBC",
        "1.2.840.113549.1.12.1.6" => "pbeWithSHAAnd40BitRC2-CBC",
        _ => "pkcs-12PbeId",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// RFC 7292's KDF has no published vectors; this is the one from the
    /// `pkcs12` crate's own tests (RustCrypto/formats, Apache-2.0 OR MIT),
    /// itself cross-checked against OpenSSL: password "ge@äheim" as
    /// BMPString, salt 0x0102030405060708, 100 rounds, id 1, SHA-256, 32
    /// bytes.
    #[test]
    fn kdf_matches_the_reference_vector() {
        let key = MacHash::Sha256.kdf("ge@äheim", &[1, 2, 3, 4, 5, 6, 7, 8], 100, 1, 32);
        assert_eq!(
            key,
            hex("fae4d4957a3cc781e1180b9d4fb79c1e0c8579b746a3177e5b0768a3118bf863")
        );
        let iv = MacHash::Sha256.kdf("ge@äheim", &[1, 2, 3, 4, 5, 6, 7, 8], 100, 2, 32);
        assert_eq!(
            iv,
            hex("e5ff813bc6547de5155b14d2fada85b3201a977349db6e26ccc998d9e8f83d6c")
        );
        let mac = MacHash::Sha256.kdf("ge@äheim", &[1, 2, 3, 4, 5, 6, 7, 8], 100, 3, 32);
        assert_eq!(
            mac,
            hex("136355ed9434516682534f46d63956db5ff06b844702c2c1f3b46321e2524a4d")
        );
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}

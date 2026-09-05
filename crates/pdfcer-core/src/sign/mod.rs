//! # Digital signing — the CREATE side of ISO 32000-1 §12.8 (PAdES B-B first)
//!
//! `signature.rs` measures, `signature_verify.rs` checks; this module
//! **produces**. It is the family the `pdfcer-gui` request of 2026-09-03
//! (*"a document cannot be signed"*) asked for second, after verification,
//! and the operator approved its shape on 2026-09-05: **PAdES B-B, CAdES by
//! default, the first key source a PKCS#12 file**, with Windows-store and
//! PKCS#11 sources to follow as *shell-side* implementations of the same
//! trait.
//!
//! # The load-bearing design — hash in, signature out
//!
//! Every digital-ID source Acrobat offers (a `.pfx` file, the Windows
//! certificate store, a PKCS#11 token, a roaming ID) ends in the same
//! operation: *here is the thing to sign, give me the signature bytes and
//! the certificate chain.* Everything else — the byte-range digest, the CMS
//! `SignedData`, the signature dictionary, the incremental update — is the
//! same for all of them. So [`Signer`] is that one operation, and
//! `pdfcer-core` ships exactly one implementation, [`pkcs12::Pkcs12Signer`],
//! because a `.pfx` is the **only** source whose raw private key is
//! legitimately in this process's memory. A Windows-store or token signer
//! lives in a shell, where the key never leaves its custodian and the OS
//! API does the arithmetic; it hands `pdfcer-core` a signature, nothing
//! else. That keeps the engine free of OS key-store and network
//! dependencies (`ARCHITECTURE.md` §1.1) while giving every source one
//! pipeline.
//!
//! # Sub-modules, in the order a signature is built
//!
//! | Module | Does | Spec |
//! |---|---|---|
//! | [`pkcs12`] | opens a `.pfx`/`.p12`: verifies the MAC (the password check), decrypts the shrouded key bag and the cert bags under both encryption eras, pairs key with leaf, orders the chain | RFC 7292 |
//! | [`der_out`] | the DER encoder the CMS is written with; `SET OF` ordering per X.690 §11.6 | X.690 |
//! | [`cms_build`] | the detached `SignedData` with CAdES signed attributes, the RFC 5652 §5.4 `0x31` retag, the key operation via [`Signer`] | RFC 5652, RFC 5035 |
//! | [`apply`] | the PDF half: signature field + dictionary, the two-pass `/Contents` hole, `/ByteRange` to EOF, incremental-only, self-verified | ISO 32000-1 §12.8, ETSI EN 319 142-1 |
//!
//! # Why the private-key crates, and not the in-crate arithmetic
//!
//! `crypto/bignum.rs` and `crypto/ecdsa.rs` are correct and **not constant
//! time**, by decision 129 — they verify, which handles no secret. Signing
//! holds a private key, so the key operations here come from vetted
//! constant-time crates (`rsa` on `crypto-bigint`, `p256`/`p384`), chosen
//! in `docs/signing-crate-survey.md` (2026-09-05). RSA signs only through
//! the **blinded** (`Randomized*`) paths — the plain `Signer` impls in `rsa`
//! skip blinding — fed by pdfcer's own [`crate::crypto::rng`], which refuses
//! on wasm32; ECDSA is RFC 6979 deterministic and needs no RNG.
//!
//! # Feature gate
//!
//! The whole module compiles only with the `signing` Cargo feature (default
//! on). Without it there is no `sign` module at all, and the verification
//! half of the crate is untouched — a lite build reads and checks signatures
//! and cannot make one.

pub mod apply;
pub mod cms_build;
pub(crate) mod der_out;
pub mod pkcs12;

use sha2::Digest as _;

/// The signature algorithm a [`Signer`] produces and the CMS names.
///
/// Each variant fixes the digest too, because CMS carries the digest
/// algorithm separately from the signature algorithm and the two must agree
/// with what the key operation actually hashed. SHA-256 everywhere except
/// P-384, which conventionally pairs with SHA-384 (RFC 5480 §4, RFC 5758).
/// SHA-1 is deliberately absent — pdfcer never authors a SHA-1 signature
/// (ISO 32000-2 deprecates it; PAdES forbids MD5 and discourages SHA-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SignatureAlgorithm {
    /// RSASSA-PKCS1-v1_5 with SHA-256 (`sha256WithRSAEncryption`,
    /// 1.2.840.113549.1.1.11) — the interoperable default for an RSA key.
    RsaPkcs1v15Sha256,
    /// RSASSA-PSS with SHA-256, MGF1-SHA-256, salt length 32
    /// (`id-RSASSA-PSS`, 1.2.840.113549.1.1.10 with RFC 4055 params) — the
    /// PAdES-preferred RSA scheme.
    RsaPssSha256,
    /// ECDSA over P-256 with SHA-256 (`ecdsa-with-SHA256`, 1.2.840.10045.4.3.2).
    EcdsaP256Sha256,
    /// ECDSA over P-384 with SHA-384 (`ecdsa-with-SHA384`, 1.2.840.10045.4.3.3).
    EcdsaP384Sha384,
}

impl SignatureAlgorithm {
    /// The digest algorithm's OID, for `SignedData.digestAlgorithms` and
    /// `SignerInfo.digestAlgorithm`.
    #[must_use]
    pub const fn digest_oid(self) -> &'static str {
        match self {
            Self::RsaPkcs1v15Sha256 | Self::RsaPssSha256 | Self::EcdsaP256Sha256 => {
                crate::cms::oid::SHA256
            }
            Self::EcdsaP384Sha384 => "2.16.840.1.101.3.4.2.2",
        }
    }

    /// The digest of `data` under this algorithm's hash.
    #[must_use]
    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::EcdsaP384Sha384 => sha2::Sha384::digest(data).to_vec(),
            _ => sha2::Sha256::digest(data).to_vec(),
        }
    }

    /// Whether the key operation is RSA (as opposed to ECDSA).
    #[must_use]
    pub const fn is_rsa(self) -> bool {
        matches!(self, Self::RsaPkcs1v15Sha256 | Self::RsaPssSha256)
    }

    /// The `signatureAlgorithm` `AlgorithmIdentifier`, DER-encoded, as CMS
    /// carries it in `SignerInfo` (RFC 5652 §5.3, RFC 3370, RFC 4055 §3.1
    /// for the PSS parameters, RFC 5758 §3.2 for ECDSA — whose parameters
    /// "MUST be absent").
    #[must_use]
    pub fn signature_algorithm_der(self) -> Vec<u8> {
        // All four OIDs are literal constants, so `oid()` cannot fail; the
        // `unwrap_or_default` keeps the crate panic-free without an
        // `#[allow]`, and a test pins each encoding against the reader.
        match self {
            Self::RsaPkcs1v15Sha256 => der_out::algorithm_identifier(
                crate::cms::oid::SHA256_WITH_RSA,
                Some(der_out::null()),
            ),
            Self::RsaPssSha256 => {
                // RSASSA-PSS-params ::= SEQUENCE {
                //   hashAlgorithm    [0] HashAlgorithm DEFAULT sha1,
                //   maskGenAlgorithm [1] MaskGenAlgorithm DEFAULT mgf1SHA1,
                //   saltLength       [2] INTEGER DEFAULT 20,
                //   trailerField     [3] TrailerField DEFAULT trailerFieldBC }
                // Every field differs from its DEFAULT except trailerField, so
                // DER writes the first three and omits the fourth.
                let sha256 =
                    der_out::algorithm_identifier(crate::cms::oid::SHA256, Some(der_out::null()));
                let mgf1 = sha256
                    .clone()
                    .and_then(|h| der_out::algorithm_identifier("1.2.840.113549.1.1.8", Some(h)));
                match (sha256, mgf1) {
                    (Some(h), Some(m)) => {
                        let params = der_out::sequence(&[
                            der_out::context(0, &h),
                            der_out::context(1, &m),
                            der_out::context(2, &der_out::integer_u64(32)),
                        ]);
                        der_out::algorithm_identifier(crate::cms::oid::RSASSA_PSS, Some(params))
                    }
                    _ => None,
                }
            }
            Self::EcdsaP256Sha256 => der_out::algorithm_identifier("1.2.840.10045.4.3.2", None),
            Self::EcdsaP384Sha384 => der_out::algorithm_identifier("1.2.840.10045.4.3.3", None),
        }
        .unwrap_or_default()
    }
}

/// Why a signing key operation refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SignError {
    /// The signer's key cannot perform `algorithm` — an RSA algorithm on an
    /// EC key, a P-384 algorithm on a P-256 key, and so on.
    #[error("the signing key is {key} and cannot produce a {algorithm:?} signature")]
    AlgorithmMismatch {
        /// The key's kind, as a short label (`RSA-2048`, `EC P-256`).
        key: String,
        /// What was asked for.
        algorithm: SignatureAlgorithm,
    },
    /// The platform gave no random bytes, so an RSA signature could not be
    /// blinded — and pdfcer will not sign RSA unblinded. On wasm32 this is
    /// the expected answer (no `getrandom` backend is compiled in; see
    /// [`crate::crypto::rng`]); ECDSA still works there.
    #[error(
        "RSA signing needs random bytes for blinding and none are available on this target ({0}); \
         use an ECDSA key here, or sign on a native build"
    )]
    RandomUnavailable(String),
    /// The key operation itself failed — the crate reported an error. Rare
    /// (a malformed key that nonetheless parsed); the message is the
    /// crate's.
    #[error("the key operation failed: {0}")]
    KeyOperation(String),
    /// A shell-side signer (Windows store, PKCS#11 token) reported a
    /// custodian error — the token was removed, the PIN was refused, the
    /// store denied access. The text is the custodian's, passed through
    /// so the operator sees the real reason.
    #[error("the key custodian refused: {0}")]
    Custodian(String),
}

/// What signs — the one operation every digital-ID source reduces to.
///
/// Implemented in-core by [`pkcs12::Pkcs12Signer`]; implemented in a shell
/// for the Windows certificate store and PKCS#11 tokens, where the private
/// key must never enter this process. `Send + Sync` so a GUI may sign off
/// its UI thread.
///
/// # Contract
///
/// - [`Signer::sign`] receives the **complete message to be signed** (for
///   CMS: the DER `SignedAttributes` with the `0x31` tag, RFC 5652 §5.4) and
///   returns the raw signature value CMS carries in `SignerInfo.signature`:
///   for RSA the `k`-octet integer, for ECDSA the DER
///   `ECDSA-Sig-Value { r, s }` (RFC 5753 §2.1.1). The signer hashes the
///   message itself under `algorithm`'s digest — a custodian API such as
///   `NCryptSignHash` takes the hash, so a shell impl computes
///   `algorithm.digest(message)` and hands that over.
/// - [`Signer::certificate_chain`] is DER certificates, **leaf first**, then
///   each issuer in order; the trust anchor may be present or not (PAdES
///   requirement a) allows either). The leaf is what
///   `signing-certificate-v2` and `IssuerAndSerialNumber` are built from.
/// - [`Signer::default_algorithm`] is what the key naturally does — PKCS#1
///   v1.5 for RSA (interoperability), the curve's ECDSA for EC. A caller may
///   ask for another the key supports (PSS for RSA); the signer refuses a
///   mismatch by name.
pub trait Signer: Send + Sync {
    /// Sign `message` under `algorithm`. See the trait docs for what
    /// `message` is and what comes back.
    fn sign(&self, algorithm: SignatureAlgorithm, message: &[u8]) -> Result<Vec<u8>, SignError>;

    /// The certificate chain, DER, leaf first.
    fn certificate_chain(&self) -> &[Vec<u8>];

    /// The algorithm this key uses when the caller expresses no preference.
    fn default_algorithm(&self) -> SignatureAlgorithm;

    /// A short human label for the key (`RSA-2048`, `EC P-256`), for
    /// messages and the CLI's disclosure line.
    fn key_label(&self) -> String;
}

/// A decoded private key held in memory — what a [`pkcs12::Pkcs12Signer`]
/// wraps. Private: the only way to obtain one is through a container
/// pdfcer parsed, and the only thing it does is sign.
pub(crate) enum KeyMaterial {
    /// RSA, any modulus size the crate accepts.
    Rsa(rsa::RsaPrivateKey),
    /// ECDSA over NIST P-256.
    P256(p256::ecdsa::SigningKey),
    /// ECDSA over NIST P-384.
    P384(p384::ecdsa::SigningKey),
}

impl KeyMaterial {
    /// Parse a PKCS#8 `PrivateKeyInfo` (RFC 5958) — what a `.pfx` key bag
    /// decrypts to. Dispatches on the algorithm OID inside; anything but
    /// `rsaEncryption` and `id-ecPublicKey` with P-256/P-384 is refused with
    /// the OID named.
    pub(crate) fn from_pkcs8_der(der: &[u8]) -> Result<Self, pkcs12::Pkcs12Error> {
        use p256::pkcs8::DecodePrivateKey as _;
        // PrivateKeyInfo ::= SEQUENCE { version, privateKeyAlgorithm
        // AlgorithmIdentifier, privateKey OCTET STRING, ... } — peek at the
        // algorithm so the refusal can name it rather than trying each crate.
        let alg = (|| {
            let (outer, _) = crate::asn1::expect(der, crate::asn1::SEQUENCE)?;
            let kids = crate::asn1::children(outer)?;
            let alg = kids.get(1).filter(|t| t.tag == crate::asn1::SEQUENCE)?;
            let parts = crate::asn1::children(*alg)?;
            let oid = crate::asn1::oid_to_string(parts.first()?.content)?;
            let params = parts
                .get(1)
                .filter(|t| t.tag == crate::asn1::OID)
                .and_then(|t| crate::asn1::oid_to_string(t.content));
            Some((oid, params))
        })();
        let Some((oid, curve)) = alg else {
            return Err(pkcs12::Pkcs12Error::Malformed {
                what: "PrivateKeyInfo",
            });
        };
        let key_err = |e: String| pkcs12::Pkcs12Error::PrivateKey { detail: e };
        match (oid.as_str(), curve.as_deref()) {
            (crate::cms::oid::RSA_ENCRYPTION, _) => rsa::RsaPrivateKey::from_pkcs8_der(der)
                .map(Self::Rsa)
                .map_err(|e| key_err(e.to_string())),
            (crate::cms::oid::EC_PUBLIC_KEY, Some("1.2.840.10045.3.1.7")) => {
                p256::ecdsa::SigningKey::from_pkcs8_der(der)
                    .map(Self::P256)
                    .map_err(|e| key_err(e.to_string()))
            }
            (crate::cms::oid::EC_PUBLIC_KEY, Some("1.3.132.0.34")) => {
                p384::ecdsa::SigningKey::from_pkcs8_der(der)
                    .map(Self::P384)
                    .map_err(|e| key_err(e.to_string()))
            }
            (o, c) => Err(pkcs12::Pkcs12Error::UnsupportedKey {
                algorithm: o.to_owned(),
                curve: c.map(str::to_owned),
            }),
        }
    }

    /// What this key signs with when the caller expresses no preference:
    /// PKCS#1 v1.5 for RSA (the interoperable default — PSS is opt-in), the
    /// curve's own ECDSA pairing for EC.
    pub(crate) fn default_algorithm(&self) -> SignatureAlgorithm {
        match self {
            Self::Rsa(_) => SignatureAlgorithm::RsaPkcs1v15Sha256,
            Self::P256(_) => SignatureAlgorithm::EcdsaP256Sha256,
            Self::P384(_) => SignatureAlgorithm::EcdsaP384Sha384,
        }
    }

    /// A short human label (`RSA-2048`, `EC P-256`) for messages and the
    /// CLI's disclosure line — the key's kind and size, never its value.
    pub(crate) fn label(&self) -> String {
        use rsa::traits::PublicKeyParts as _;
        match self {
            Self::Rsa(k) => format!("RSA-{}", k.n().bits()),
            Self::P256(_) => "EC P-256".to_owned(),
            Self::P384(_) => "EC P-384".to_owned(),
        }
    }

    /// The public key's identifying bytes as [`crate::cms::PublicKey`]
    /// exposes them from a certificate — RSA modulus (minimal big-endian)
    /// or the uncompressed EC point — so a `.pfx` key can be paired with
    /// its certificate when no `localKeyId` says which one.
    pub(crate) fn public_identity(&self) -> Vec<u8> {
        use rsa::traits::PublicKeyParts as _;
        match self {
            Self::Rsa(k) => {
                let be = k.n().to_be_bytes();
                let first = be.iter().position(|&b| b != 0).unwrap_or(be.len());
                be.get(first..).unwrap_or(&[]).to_vec()
            }
            Self::P256(k) => k.verifying_key().to_sec1_point(false).as_bytes().to_vec(),
            Self::P384(k) => k.verifying_key().to_sec1_point(false).as_bytes().to_vec(),
        }
    }

    /// The key operation. RSA goes through the blinded `Randomized*` paths
    /// only (module docs); ECDSA is RFC 6979 deterministic over the prehash.
    pub(crate) fn sign(
        &self,
        algorithm: SignatureAlgorithm,
        message: &[u8],
    ) -> Result<Vec<u8>, SignError> {
        use rsa::signature::{RandomizedDigestSigner as _, SignatureEncoding as _};
        use signature::hazmat::PrehashSigner as _;
        let mismatch = || SignError::AlgorithmMismatch {
            key: self.label(),
            algorithm,
        };
        match (self, algorithm) {
            (Self::Rsa(key), SignatureAlgorithm::RsaPkcs1v15Sha256) => {
                let mut rng = PdfcerRng;
                rng.probe()?;
                let signing = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(key.clone());
                signing
                    .try_sign_digest_with_rng(&mut rng, |d: &mut sha2::Sha256| {
                        d.update(message);
                        Ok(())
                    })
                    .map(|s| s.to_vec())
                    .map_err(|e| SignError::KeyOperation(e.to_string()))
            }
            (Self::Rsa(key), SignatureAlgorithm::RsaPssSha256) => {
                let mut rng = PdfcerRng;
                rng.probe()?;
                let signing = rsa::pss::SigningKey::<sha2::Sha256>::new(key.clone());
                signing
                    .try_sign_digest_with_rng(&mut rng, |d: &mut sha2::Sha256| {
                        d.update(message);
                        Ok(())
                    })
                    .map(|s| s.to_vec())
                    .map_err(|e| SignError::KeyOperation(e.to_string()))
            }
            (Self::P256(key), SignatureAlgorithm::EcdsaP256Sha256) => {
                let sig: p256::ecdsa::Signature = key
                    .sign_prehash(&algorithm.digest(message))
                    .map_err(|e| SignError::KeyOperation(e.to_string()))?;
                Ok(sig.to_der().as_bytes().to_vec())
            }
            (Self::P384(key), SignatureAlgorithm::EcdsaP384Sha384) => {
                let sig: p384::ecdsa::Signature = key
                    .sign_prehash(&algorithm.digest(message))
                    .map_err(|e| SignError::KeyOperation(e.to_string()))?;
                Ok(sig.to_der().as_bytes().to_vec())
            }
            _ => Err(mismatch()),
        }
    }
}

/// `rand_core` adapter over [`crate::crypto::rng::fill`] — the bytes `rsa`
/// blinds with.
///
/// Twenty lines instead of `rsa`'s `getrandom` feature because that feature
/// pulls `getrandom 0.4`, which does not build on wasm32 without a JS
/// backend and would be a second `getrandom` beside pdfcer's gated 0.2. This
/// adapter compiles everywhere and **refuses at runtime** where no entropy
/// source exists, which [`KeyMaterial::sign`] turns into
/// [`SignError::RandomUnavailable`] *before* any key arithmetic runs
/// ([`PdfcerRng::probe`]).
struct PdfcerRng;

impl PdfcerRng {
    /// Ask for one block up front so an unavailable RNG surfaces as a named
    /// refusal rather than as an error from inside the crate's signing
    /// routine.
    fn probe(&mut self) -> Result<(), SignError> {
        let mut probe = [0u8; 16];
        crate::crypto::rng::fill(&mut probe)
            .map_err(|e| SignError::RandomUnavailable(e.to_string()))
    }
}

impl rand_core::TryRng for PdfcerRng {
    type Error = crate::crypto::rng::RngError;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        let mut b = [0u8; 4];
        crate::crypto::rng::fill(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut b = [0u8; 8];
        crate::crypto::rng::fill(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        crate::crypto::rng::fill(dst)
    }
}

impl rand_core::TryCryptoRng for PdfcerRng {}

/// Check a raw signature value with **pdfcer's own verifier** — the
/// in-crate, verify-only arithmetic of `crypto/rsa.rs` and `crypto/ecdsa.rs`
/// (decision 129), which is a different implementation from the signing
/// crates and therefore a genuine cross-check.
///
/// `certificate` is the signer's DER certificate, `message` the bytes that
/// were signed (the signer hashed them under `algorithm`'s digest),
/// `signature` what [`Signer::sign`] returned. `Ok(())` when the signature
/// verifies; `Err` carries the verifier's reason (an unparseable
/// certificate, an unsupported key, or simply "did not verify").
///
/// Exposed so a shell that implements [`Signer`] against an external
/// custodian can prove its plumbing before signing a document with it, and
/// used by the integration tests as the oracle for every algorithm.
///
/// # Errors
///
/// A `String` naming the failure; never a panic.
pub fn verify_raw_signature(
    certificate: &[u8],
    algorithm: SignatureAlgorithm,
    message: &[u8],
    signature: &[u8],
) -> Result<(), String> {
    let cert = crate::cms::parse_certificate(certificate)
        .ok_or_else(|| "the certificate did not parse".to_owned())?;
    let alg_der = algorithm.signature_algorithm_der();
    let (alg_tlv, _) = crate::asn1::read(&alg_der)
        .ok_or_else(|| "internal: bad AlgorithmIdentifier".to_owned())?;
    let parts = crate::asn1::children(alg_tlv).unwrap_or_default();
    let alg = crate::cms::AlgId {
        oid: parts
            .first()
            .and_then(|t| crate::asn1::oid_to_string(t.content))
            .unwrap_or_default(),
        params: parts.get(1).copied(),
    };
    let hash = crate::signature_verify::hash_for(algorithm.digest_oid())
        .ok_or_else(|| "internal: unknown digest".to_owned())?;
    let digest = algorithm.digest(message);
    let mut notes = Vec::new();
    match crate::signature_verify::check_signature(
        &cert.key, &alg, hash, &digest, signature, &mut notes,
    ) {
        Ok((true, _)) => Ok(()),
        Ok((false, what)) => Err(format!("{what}: the signature did not verify")),
        Err(reason) => Err(reason),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn every_signature_algorithm_identifier_parses_back_to_its_oid() {
        for (alg, oid) in [
            (
                SignatureAlgorithm::RsaPkcs1v15Sha256,
                crate::cms::oid::SHA256_WITH_RSA,
            ),
            (
                SignatureAlgorithm::RsaPssSha256,
                crate::cms::oid::RSASSA_PSS,
            ),
            (SignatureAlgorithm::EcdsaP256Sha256, "1.2.840.10045.4.3.2"),
            (SignatureAlgorithm::EcdsaP384Sha384, "1.2.840.10045.4.3.3"),
        ] {
            let der = alg.signature_algorithm_der();
            assert!(!der.is_empty(), "{alg:?} encoded to nothing");
            let (t, rest) = crate::asn1::read(&der).unwrap();
            assert!(rest.is_empty());
            let kids = crate::asn1::children(t).unwrap();
            assert_eq!(
                crate::asn1::oid_to_string(kids[0].content).unwrap(),
                oid,
                "{alg:?}"
            );
            // ECDSA: parameters MUST be absent (RFC 5758 §3.2).
            if !alg.is_rsa() {
                assert_eq!(kids.len(), 1, "{alg:?} must carry no parameters");
            }
        }
        // PSS params: three explicit context tags, saltLength 32.
        let der = SignatureAlgorithm::RsaPssSha256.signature_algorithm_der();
        let (t, _) = crate::asn1::read(&der).unwrap();
        let kids = crate::asn1::children(t).unwrap();
        let params = crate::asn1::children(kids[1]).unwrap();
        assert_eq!(
            params.iter().map(|p| p.tag).collect::<Vec<_>>(),
            vec![0xA0, 0xA1, 0xA2]
        );
        let salt = crate::asn1::read(params[2].content).unwrap().0;
        assert_eq!(crate::asn1::integer_bytes(salt).unwrap(), &[32]);
    }

    #[test]
    fn digest_lengths_follow_the_algorithm() {
        assert_eq!(SignatureAlgorithm::RsaPkcs1v15Sha256.digest(b"x").len(), 32);
        assert_eq!(SignatureAlgorithm::EcdsaP384Sha384.digest(b"x").len(), 48);
    }
}

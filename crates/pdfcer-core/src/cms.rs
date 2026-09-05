//! CMS `SignedData` (RFC 5652 §5) and X.509 certificate (RFC 5280 §4)
//! reading — the exact subset a PDF signature verifier needs.
//!
//! Spec source: the PDF-spec RAG's `iso32000__s__12.8.3.md` §8 (`SI-C1`
//! through `SI-C4`, RFC 5652 verbatim) and its
//! `iso32000__ref__signature_verification.md`. Every structural claim below
//! cites one of those identifiers; nothing is recalled from memory about
//! "how PKCS#7 usually looks".
//!
//! # What is extracted
//!
//! From `SignedData`: the `eContentType`, the `eContent` (present only for
//! `adbe.pkcs7.sha1`, `SI-W1`), every certificate's raw DER, and the FIRST
//! `SignerInfo` (a PDF signature has exactly one signer — a second is
//! reported, not verified). From the `SignerInfo`: the signer identifier
//! (issuer + serial, or subject key identifier), the digest algorithm OID,
//! the raw `signedAttrs` re-tagged as `SET OF` (`SI-C2`: `0xA0` in the
//! file, `0x31` for the hash — the single most common from-scratch
//! verifier bug), the `messageDigest`, `contentType` and `signingTime`
//! attributes, the signature algorithm OID with its parameters (for
//! RSASSA-PSS), and the signature value.
//!
//! From the certificate: subject and issuer as readable strings, serial,
//! validity dates, the `subjectPublicKeyInfo` algorithm OID, parameters and
//! key bytes. Enough to pick the signer's certificate out of the bag, to
//! verify with its key, and to report who it claims to be — **not** enough
//! to validate a chain, which the verdict says in as many words.
//!
//! # Posture
//!
//! Untrusted input throughout: every accessor is `Option`, malformed
//! structures fall out as `None`, and the caller reports *unverifiable*,
//! never *valid*, for anything it could not fully read.

use crate::asn1::{self, Tlv};

/// OIDs this module matches by name. Dotted-decimal, from RFC 5652 §11,
/// RFC 5754, RFC 8017/4055, RFC 5480 (`SI-C4`).
pub(crate) mod oid {
    pub const SIGNED_DATA: &str = "1.2.840.113549.1.7.2";
    pub const DATA: &str = "1.2.840.113549.1.7.1";
    pub const CONTENT_TYPE: &str = "1.2.840.113549.1.9.3";
    pub const MESSAGE_DIGEST: &str = "1.2.840.113549.1.9.4";
    pub const SIGNING_TIME: &str = "1.2.840.113549.1.9.5";
    pub const SHA1: &str = "1.3.14.3.2.26";
    pub const SHA256: &str = "2.16.840.1.101.3.4.2.1";
    pub const SHA384: &str = "2.16.840.1.101.3.4.2.2";
    pub const SHA512: &str = "2.16.840.1.101.3.4.2.3";
    pub const RSA_ENCRYPTION: &str = "1.2.840.113549.1.1.1";
    pub const SHA1_WITH_RSA: &str = "1.2.840.113549.1.1.5";
    pub const SHA256_WITH_RSA: &str = "1.2.840.113549.1.1.11";
    pub const SHA384_WITH_RSA: &str = "1.2.840.113549.1.1.12";
    pub const SHA512_WITH_RSA: &str = "1.2.840.113549.1.1.13";
    pub const RSASSA_PSS: &str = "1.2.840.113549.1.1.10";
    pub const MGF1: &str = "1.2.840.113549.1.1.8";
    pub const EC_PUBLIC_KEY: &str = "1.2.840.10045.2.1";
    pub const ECDSA_SHA1: &str = "1.2.840.10045.4.1";
    pub const ECDSA_SHA256: &str = "1.2.840.10045.4.3.2";
    pub const ECDSA_SHA384: &str = "1.2.840.10045.4.3.3";
    pub const ECDSA_SHA512: &str = "1.2.840.10045.4.3.4";
    // X.520 attribute types, for readable names.
    pub const CN: &str = "2.5.4.3";
    pub const O: &str = "2.5.4.10";
    pub const OU: &str = "2.5.4.11";
    pub const C: &str = "2.5.4.6";
    pub const EMAIL: &str = "1.2.840.113549.1.9.1";
}

/// An `AlgorithmIdentifier`: the OID and its raw parameters element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AlgId<'a> {
    pub oid: String,
    /// The parameters TLV as it appeared (absent → `None`).
    pub params: Option<Tlv<'a>>,
}

fn alg_id(tlv: Tlv<'_>) -> Option<AlgId<'_>> {
    if tlv.tag != asn1::SEQUENCE {
        return None;
    }
    let kids = asn1::children(tlv)?;
    let oid_tlv = kids.first()?;
    if oid_tlv.tag != asn1::OID {
        return None;
    }
    Some(AlgId {
        oid: asn1::oid_to_string(oid_tlv.content)?,
        params: kids.get(1).copied(),
    })
}

/// How a `SignerInfo` names its certificate (RFC 5652 §5.3 `sid`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SignerId<'a> {
    /// `issuerAndSerialNumber`: the issuer `Name`'s raw DER and the serial.
    IssuerSerial {
        issuer_der: &'a [u8],
        serial: &'a [u8],
    },
    /// `[0] subjectKeyIdentifier` (version 3).
    SubjectKeyId(&'a [u8]),
}

/// The first `SignerInfo` of a `SignedData`.
#[derive(Debug, Clone)]
pub(crate) struct SignerInfo<'a> {
    pub version: u64,
    pub sid: SignerId<'a>,
    pub digest_alg: AlgId<'a>,
    /// The signed attributes with their tag rewritten to `SET OF` (`0x31`),
    /// ready to hash (`SI-C2`). `None` when the signer omitted them — a
    /// shape neither PDF subfilter permits.
    pub signed_attrs_der: Option<Vec<u8>>,
    pub message_digest: Option<&'a [u8]>,
    pub content_type: Option<String>,
    pub signing_time: Option<String>,
    pub signature_alg: AlgId<'a>,
    pub signature: &'a [u8],
}

/// A `SignedData`, as much of it as verification reads.
#[derive(Debug, Clone)]
pub(crate) struct SignedData<'a> {
    pub version: u64,
    pub content_type: String,
    /// `eContent`, when encapsulated (`adbe.pkcs7.sha1` carries the SHA-1 of
    /// the byte range here; `.detached`/CAdES carry nothing).
    pub econtent: Option<&'a [u8]>,
    /// Every certificate's raw DER, in order.
    pub certificates: Vec<&'a [u8]>,
    pub signer_count: usize,
    pub signer: Option<SignerInfo<'a>>,
}

/// Parse the outer `ContentInfo` and its `SignedData`.
pub(crate) fn parse_signed_data(der: &[u8]) -> Option<SignedData<'_>> {
    // ContentInfo ::= SEQUENCE { contentType OID, content [0] EXPLICIT ANY }
    let (ci, _trailing) = asn1::expect(der, asn1::SEQUENCE)?;
    let ci_kids = asn1::children(ci)?;
    let ct = asn1::oid_to_string(ci_kids.first().filter(|t| t.tag == asn1::OID)?.content)?;
    if ct != oid::SIGNED_DATA {
        return None;
    }
    let wrapper = ci_kids.get(1).filter(|t| t.tag == asn1::context(0))?;
    let (sd, _) = asn1::expect(wrapper.content, asn1::SEQUENCE)?;
    let kids = asn1::children(sd)?;
    let mut it = kids.iter().copied();
    let version = small_int(it.next()?)?;
    let _digest_algs = it.next().filter(|t| t.tag == asn1::SET)?;
    // EncapsulatedContentInfo ::= SEQUENCE { eContentType OID, eContent [0] EXPLICIT OCTET STRING OPTIONAL }
    let eci = it.next().filter(|t| t.tag == asn1::SEQUENCE)?;
    let eci_kids = asn1::children(eci)?;
    let content_type =
        asn1::oid_to_string(eci_kids.first().filter(|t| t.tag == asn1::OID)?.content)?;
    let econtent = match eci_kids.get(1) {
        Some(w) if w.tag == asn1::context(0) => {
            let (os, _) = asn1::expect(w.content, asn1::OCTET_STRING)?;
            Some(os.content)
        }
        _ => None,
    };
    let mut certificates = Vec::new();
    let mut next = it.next();
    if let Some(t) = next.filter(|t| t.tag == asn1::context(0)) {
        // certificates [0] IMPLICIT CertificateSet — a SET whose elements are
        // Certificate SEQUENCEs (other choices are tagged and skipped).
        for c in asn1::children(t)? {
            if c.tag == asn1::SEQUENCE {
                certificates.push(c.raw);
            }
        }
        next = it.next();
    }
    if next.is_some_and(|t| t.tag == asn1::context(1)) {
        next = it.next(); // crls, ignored
    }
    let signer_infos = next.filter(|t| t.tag == asn1::SET)?;
    let infos = asn1::children(signer_infos)?;
    let signer = infos.first().and_then(|t| parse_signer_info(*t));
    Some(SignedData {
        version,
        content_type,
        econtent,
        certificates,
        signer_count: infos.len(),
        signer,
    })
}

fn small_int(tlv: Tlv<'_>) -> Option<u64> {
    let bytes = asn1::integer_bytes(tlv)?;
    if bytes.len() > 8 {
        return None;
    }
    Some(bytes.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b)))
}

fn parse_signer_info(tlv: Tlv<'_>) -> Option<SignerInfo<'_>> {
    if tlv.tag != asn1::SEQUENCE {
        return None;
    }
    let kids = asn1::children(tlv)?;
    let mut it = kids.iter().copied();
    let version = small_int(it.next()?)?;
    let sid_tlv = it.next()?;
    let sid = if sid_tlv.tag == asn1::SEQUENCE {
        let parts = asn1::children(sid_tlv)?;
        let issuer = parts.first().filter(|t| t.tag == asn1::SEQUENCE)?;
        let serial = asn1::integer_bytes(*parts.get(1)?)?;
        SignerId::IssuerSerial {
            issuer_der: issuer.raw,
            serial,
        }
    } else if sid_tlv.tag == 0x80 {
        SignerId::SubjectKeyId(sid_tlv.content)
    } else {
        return None;
    };
    let digest_alg = alg_id(it.next()?)?;
    let mut next = it.next()?;
    let mut signed_attrs_der = None;
    let mut message_digest = None;
    let mut content_type = None;
    let mut signing_time = None;
    if next.tag == asn1::context(0) {
        // SI-C2: re-tag [0] IMPLICIT as the EXPLICIT SET OF for hashing.
        let mut der = next.raw.to_vec();
        if let Some(first) = der.first_mut() {
            *first = asn1::SET;
        }
        signed_attrs_der = Some(der);
        for attr in asn1::children(next)? {
            // Attribute ::= SEQUENCE { attrType OID, attrValues SET OF ANY }
            let parts = asn1::children(attr)?;
            let t = asn1::oid_to_string(parts.first().filter(|t| t.tag == asn1::OID)?.content)?;
            let values = asn1::children(*parts.get(1).filter(|t| t.tag == asn1::SET)?)?;
            let Some(v) = values.first() else {
                continue;
            };
            match t.as_str() {
                oid::MESSAGE_DIGEST if v.tag == asn1::OCTET_STRING => {
                    message_digest = Some(v.content);
                }
                oid::CONTENT_TYPE if v.tag == asn1::OID => {
                    content_type = asn1::oid_to_string(v.content);
                }
                oid::SIGNING_TIME => signing_time = asn1::time_value(*v),
                _ => {}
            }
        }
        next = it.next()?;
    }
    let signature_alg = alg_id(next)?;
    let sig = it.next().filter(|t| t.tag == asn1::OCTET_STRING)?;
    Some(SignerInfo {
        version,
        sid,
        digest_alg,
        signed_attrs_der,
        message_digest,
        content_type,
        signing_time,
        signature_alg,
        signature: sig.content,
    })
}

/// The public key inside a certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PublicKey<'a> {
    /// `rsaEncryption`: the `RSAPublicKey` SEQUENCE's modulus and exponent.
    Rsa { n: &'a [u8], e: &'a [u8] },
    /// `id-ecPublicKey`: the named-curve OID and the SEC1 point.
    Ec { curve_oid: String, point: &'a [u8] },
    /// Something else, named.
    Other(String),
}

/// What a certificate says about itself — reported, never trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Certificate<'a> {
    pub subject: String,
    pub issuer: String,
    pub issuer_der: &'a [u8],
    pub serial: &'a [u8],
    pub not_before: Option<String>,
    pub not_after: Option<String>,
    pub key: PublicKey<'a>,
    /// The `SubjectKeyIdentifier` extension's value, if present (for a
    /// `subjectKeyIdentifier` signer id).
    pub subject_key_id: Option<&'a [u8]>,
    /// The RAW `TBSCertificate` bytes (RFC 5280 §4.1.1.1) — exactly what the
    /// issuer signed. Chain validation hashes THIS and checks it against
    /// [`sig_value`](Self::sig_value) with the issuer's key (`Pass 10.3`).
    pub tbs_der: &'a [u8],
    /// The subject `Name`'s raw DER — matched against a candidate issuer's
    /// [`issuer_der`](Self::issuer_der) to link a chain (`Pass 10.3`).
    pub subject_der: &'a [u8],
    /// The OUTER `signatureAlgorithm` OID (RFC 5280 §4.1.1.2), naming both the
    /// signature scheme and its hash (e.g. `sha256WithRSAEncryption`).
    pub sig_alg_oid: Option<String>,
    /// The `signatureValue` BIT STRING contents — the issuer's signature over
    /// [`tbs_der`](Self::tbs_der).
    pub sig_value: &'a [u8],
}

/// Parse an X.509 v3 certificate (RFC 5280 §4.1).
pub(crate) fn parse_certificate(der: &[u8]) -> Option<Certificate<'_>> {
    let (cert, _) = asn1::expect(der, asn1::SEQUENCE)?;
    let kids = asn1::children(cert)?;
    let tbs = kids.first().filter(|t| t.tag == asn1::SEQUENCE)?;
    let tbs_der = tbs.raw;
    // The outer signatureAlgorithm (kids[1]) names the hash+scheme; the
    // signatureValue (kids[2]) is the issuer's signature over `tbs_der`.
    let sig_alg_oid = kids
        .get(1)
        .and_then(|alg| asn1::children(*alg))
        .and_then(|c| c.into_iter().next())
        .filter(|t| t.tag == asn1::OID)
        .and_then(|t| asn1::oid_to_string(t.content));
    let sig_value = kids
        .get(2)
        .filter(|t| t.tag == asn1::BIT_STRING)
        .and_then(|t| asn1::bit_string_bytes(*t))
        .unwrap_or(&[]);
    let tbs_kids = asn1::children(*tbs)?;
    let mut it = tbs_kids.iter().copied().peekable();
    // version [0] EXPLICIT INTEGER DEFAULT v1
    if it.peek().is_some_and(|t| t.tag == asn1::context(0)) {
        it.next();
    }
    let serial = asn1::integer_bytes(it.next()?)?;
    let _sig_alg = it.next()?;
    let issuer_tlv = it.next().filter(|t| t.tag == asn1::SEQUENCE)?;
    let validity = it.next().filter(|t| t.tag == asn1::SEQUENCE)?;
    let v = asn1::children(validity)?;
    let not_before = v.first().and_then(|t| asn1::time_value(*t));
    let not_after = v.get(1).and_then(|t| asn1::time_value(*t));
    let subject_tlv = it.next().filter(|t| t.tag == asn1::SEQUENCE)?;
    let spki = it.next().filter(|t| t.tag == asn1::SEQUENCE)?;
    let key = parse_spki(spki)?;
    let mut subject_key_id = None;
    // Optional issuerUniqueID [1], subjectUniqueID [2], extensions [3].
    for t in it {
        if t.tag == asn1::context(3) {
            let (exts, _) = asn1::expect(t.content, asn1::SEQUENCE).unwrap_or((t, &[]));
            for ext in asn1::children(exts).unwrap_or_default() {
                let parts = asn1::children(ext).unwrap_or_default();
                let Some(oid_t) = parts.first().filter(|t| t.tag == asn1::OID) else {
                    continue;
                };
                if asn1::oid_to_string(oid_t.content).as_deref() == Some("2.5.29.14") {
                    // extnValue OCTET STRING wrapping an OCTET STRING.
                    if let Some(outer) = parts.last().filter(|t| t.tag == asn1::OCTET_STRING)
                        && let Some((inner, _)) = asn1::expect(outer.content, asn1::OCTET_STRING)
                    {
                        subject_key_id = Some(inner.content);
                    }
                }
            }
        }
    }
    Some(Certificate {
        subject: name_to_string(subject_tlv),
        issuer: name_to_string(issuer_tlv),
        issuer_der: issuer_tlv.raw,
        serial,
        not_before,
        not_after,
        key,
        subject_key_id,
        tbs_der,
        subject_der: subject_tlv.raw,
        sig_alg_oid,
        sig_value,
    })
}

/// `SubjectPublicKeyInfo ::= SEQUENCE { algorithm AlgorithmIdentifier, subjectPublicKey BIT STRING }`.
fn parse_spki(tlv: Tlv<'_>) -> Option<PublicKey<'_>> {
    let kids = asn1::children(tlv)?;
    let alg = alg_id(*kids.first()?)?;
    let key_bits = asn1::bit_string_bytes(*kids.get(1)?)?;
    Some(match alg.oid.as_str() {
        oid::RSA_ENCRYPTION => {
            // RSAPublicKey ::= SEQUENCE { modulus INTEGER, publicExponent INTEGER }
            let (seq, _) = asn1::expect(key_bits, asn1::SEQUENCE)?;
            let parts = asn1::children(seq)?;
            PublicKey::Rsa {
                n: asn1::integer_bytes(*parts.first()?)?,
                e: asn1::integer_bytes(*parts.get(1)?)?,
            }
        }
        oid::EC_PUBLIC_KEY => {
            let curve = alg.params.filter(|p| p.tag == asn1::OID)?;
            PublicKey::Ec {
                curve_oid: asn1::oid_to_string(curve.content)?,
                point: key_bits,
            }
        }
        other => PublicKey::Other(other.to_string()),
    })
}

/// A `Name` as `CN=…, O=…, C=…` — the attributes an operator recognises,
/// in the order the certificate lists them; unknown types are shown by OID.
fn name_to_string(name: Tlv<'_>) -> String {
    let mut parts = Vec::new();
    for rdn in asn1::children(name).unwrap_or_default() {
        for atv in asn1::children(rdn).unwrap_or_default() {
            let kids = asn1::children(atv).unwrap_or_default();
            let (Some(t), Some(v)) = (kids.first(), kids.get(1)) else {
                continue;
            };
            let Some(oid) = asn1::oid_to_string(t.content) else {
                continue;
            };
            let label = match oid.as_str() {
                oid::CN => "CN".to_string(),
                oid::O => "O".to_string(),
                oid::OU => "OU".to_string(),
                oid::C => "C".to_string(),
                oid::EMAIL => "E".to_string(),
                "2.5.4.7" => "L".to_string(),
                "2.5.4.8" => "ST".to_string(),
                other => other.to_string(),
            };
            let value = asn1::string_value(*v).unwrap_or_else(|| "?".to_string());
            parts.push(format!("{label}={value}"));
        }
    }
    parts.join(", ")
}

impl<'a> SignedData<'a> {
    /// The RAW DER of the signer's certificate (the one [`signer_certificate`]
    /// parses) — the starting point for chain building (`Pass 10.3`).
    pub(crate) fn signer_certificate_der(&self) -> Option<&'a [u8]> {
        let signer = self.signer.as_ref()?;
        self.certificates.iter().copied().find(|der| {
            parse_certificate(der).is_some_and(|c| match &signer.sid {
                SignerId::IssuerSerial { issuer_der, serial } => {
                    c.issuer_der == *issuer_der && c.serial == *serial
                }
                SignerId::SubjectKeyId(id) => c.subject_key_id == Some(*id),
            })
        })
    }

    /// The certificate the first signer's `sid` names, parsed.
    pub(crate) fn signer_certificate(&self) -> Option<Certificate<'_>> {
        let signer = self.signer.as_ref()?;
        self.certificates
            .iter()
            .filter_map(|der| parse_certificate(der))
            .find(|c| match &signer.sid {
                SignerId::IssuerSerial { issuer_der, serial } => {
                    c.issuer_der == *issuer_der && c.serial == *serial
                }
                SignerId::SubjectKeyId(id) => c.subject_key_id == Some(*id),
            })
    }
}

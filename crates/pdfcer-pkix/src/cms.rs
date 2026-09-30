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
use pdfcer_model::crypto::rsa::Hash;

/// OIDs this module matches by name. Dotted-decimal, from RFC 5652 §11,
/// RFC 5754, RFC 8017/4055, RFC 5480 (`SI-C4`).
pub mod oid {
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
    /// `cRLDistributionPoints` extension (RFC 5280 §4.2.1.13).
    pub const CRL_DISTRIBUTION_POINTS: &str = "2.5.29.31";
    /// `authorityInfoAccess` extension (RFC 5280 §4.2.2.1).
    pub const AUTHORITY_INFO_ACCESS: &str = "1.3.6.1.5.5.7.1.1";
    /// `id-ad-ocsp` access method (RFC 5280 §4.2.2.1).
    pub const AD_OCSP: &str = "1.3.6.1.5.5.7.48.1";
    /// `id-ad-caIssuers` access method (RFC 5280 §4.2.2.1).
    pub const AD_CA_ISSUERS: &str = "1.3.6.1.5.5.7.48.2";
    pub const EXT_KEY_USAGE: &str = "2.5.29.37";
    pub const KP_OCSP_SIGNING: &str = "1.3.6.1.5.5.7.3.9";
    pub const OCSP_BASIC: &str = "1.3.6.1.5.5.7.48.1.1";
    pub const OCSP_NOCHECK: &str = "1.3.6.1.5.5.7.48.1.5";
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
pub struct AlgId<'a> {
    pub oid: String,
    /// The parameters TLV as it appeared (absent → `None`).
    pub params: Option<Tlv<'a>>,
}

/// Decode an `AlgorithmIdentifier` SEQUENCE (RFC 5280 §4.1.1.2); shared with
/// the CRL reader. `None` when it is not a SEQUENCE starting with an OID.
pub(crate) fn alg_id(tlv: Tlv<'_>) -> Option<AlgId<'_>> {
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
pub enum SignerId<'a> {
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
pub struct SignerInfo<'a> {
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
pub struct SignedData<'a> {
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
pub fn parse_signed_data(der: &[u8]) -> Option<SignedData<'_>> {
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
pub enum PublicKey<'a> {
    /// `rsaEncryption`: the `RSAPublicKey` SEQUENCE's modulus and exponent.
    Rsa { n: &'a [u8], e: &'a [u8] },
    /// `id-ecPublicKey`: the named-curve OID and the SEC1 point.
    Ec { curve_oid: String, point: &'a [u8] },
    /// Something else, named.
    Other(String),
}

/// What a certificate says about itself — reported, never trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Certificate<'a> {
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
    /// The OUTER `signatureAlgorithm` (RFC 5280 §4.1.1.2) — its OID names the
    /// scheme and hash (e.g. `sha256WithRSAEncryption`), and its `params` carry
    /// the `RSASSA-PSS-params` when the scheme is RSA-PSS (needed to verify a
    /// PSS-signed certificate, `Pass 10.5`).
    pub sig_alg: Option<AlgId<'a>>,
    /// The `signatureValue` BIT STRING contents — the issuer's signature over
    /// [`tbs_der`](Self::tbs_der).
    pub sig_value: &'a [u8],
    /// `true` iff the `basicConstraints` extension (RFC 5280 §4.2.1.9, OID
    /// `2.5.29.19`) is present with `cA` TRUE. A certificate used as an
    /// *intermediate* issuer must be a CA; a leaf (`false`) that appears as an
    /// issuer is a chain defect (`Pass 10.5`).
    pub is_ca: bool,
    /// Whether the `keyUsage` extension (RFC 5280 §4.2.1.3, OID `2.5.29.15`)
    /// asserts `keyCertSign` (bit 5): `Some(true)` present-and-set,
    /// `Some(false)` present-but-clear, `None` extension absent. An issuer with
    /// `Some(false)` must not be used to sign certificates (`Pass 10.5`); `None`
    /// leaves the constraint unstated and is not, alone, disqualifying.
    pub key_usage_cert_sign: Option<bool>,
    /// Whether `keyUsage` asserts `cRLSign` (bit 6), with the same three
    /// states as [`key_usage_cert_sign`](Self::key_usage_cert_sign). RFC
    /// 10007 §6.3.3(f): a v3 CRL issuer's certificate must assert it.
    pub key_usage_crl_sign: Option<bool>,
    /// The X.509 version: 1, 2 or 3 (the `[0]` field's value plus one).
    pub version: u8,
    /// Where the certificate says its revocation status can be fetched:
    /// `cRLDistributionPoints` and `authorityInfoAccess` URIs. Claims —
    /// nothing here has been fetched or checked.
    pub revocation_uris: RevocationUris,
    /// The `subjectPublicKey` BIT STRING's value without its unused-bits
    /// octet — what an OCSP `CertID.issuerKeyHash` and a `byKey` responder
    /// id hash (RFC 6960 §4.1.1, errata 6165–6167).
    pub spki_key_bits: &'a [u8],
    /// `extKeyUsage` (RFC 5280 §4.2.1.12) lists `id-kp-OCSPSigning`: a CA may
    /// delegate OCSP signing to this certificate (RFC 6960 §4.2.2.2).
    pub ocsp_signing: bool,
    /// The `id-pkix-ocsp-nocheck` extension is present (RFC 6960
    /// §4.2.2.2.1): a delegated responder whose own revocation is not checked.
    pub ocsp_nocheck: bool,
}

/// The URIs a certificate names for revocation checking (RFC 5280
/// §4.2.1.13 `cRLDistributionPoints`, §4.2.2.1 `authorityInfoAccess`).
///
/// Only `uniformResourceIdentifier` names are kept; a directory-name
/// distribution point or a URI that is not printable ASCII is counted in
/// [`unreadable`](Self::unreadable) instead. Each list holds at most
/// [`MAX_REVOCATION_URIS`] entries, the rest counted the same way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevocationUris {
    /// CRL distribution point URIs (`fullName` `uniformResourceIdentifier`).
    pub crl: Vec<String>,
    /// OCSP responder URIs (`id-ad-ocsp`).
    pub ocsp: Vec<String>,
    /// Issuer-certificate URIs (`id-ad-caIssuers`).
    pub ca_issuers: Vec<String>,
    /// Location entries present but not kept.
    pub unreadable: usize,
}

/// Per-list ceiling on kept revocation URIs; a hostile certificate cannot
/// make a verdict carry an unbounded list.
pub const MAX_REVOCATION_URIS: usize = 16;

/// GeneralName `uniformResourceIdentifier [6] IMPLICIT IA5String`
/// (RFC 5280 §4.2.1.6): context-specific, primitive, tag 6.
const GENERAL_NAME_URI: u8 = 0x86;

impl RevocationUris {
    /// Keep `name` in `list` if it is a URI GeneralName of printable ASCII
    /// and the list has room; otherwise count it unreadable.
    fn keep(list: &mut Vec<String>, unreadable: &mut usize, name: Tlv<'_>) {
        let printable =
            !name.content.is_empty() && name.content.iter().all(|b| (0x21..=0x7E).contains(b));
        if name.tag == GENERAL_NAME_URI && printable && list.len() < MAX_REVOCATION_URIS {
            list.push(String::from_utf8_lossy(name.content).into_owned());
        } else {
            *unreadable += 1;
        }
    }

    /// `CRLDistributionPoints ::= SEQUENCE OF DistributionPoint`;
    /// `DistributionPoint ::= SEQUENCE { distributionPoint [0]
    /// DistributionPointName OPTIONAL, reasons [1], cRLIssuer [2] }`.
    /// `distributionPoint` tags a CHOICE, so it is explicit (`0xA0`
    /// wrapping the choice); `fullName [0] GeneralNames` is implicit
    /// (`0xA0` holding the names directly). `nameRelativeToCRLIssuer [1]`
    /// has no URI and is counted unreadable.
    fn read_crl_distribution_points(&mut self, value: &[u8]) {
        let Some((seq, _)) = asn1::expect(value, asn1::SEQUENCE) else {
            self.unreadable += 1;
            return;
        };
        for point in asn1::children(seq).unwrap_or_default() {
            let Some(fields) = asn1::children(point) else {
                self.unreadable += 1;
                continue;
            };
            let Some(name) = fields.iter().find(|f| f.tag == asn1::context(0)) else {
                // cRLIssuer-only: the CRL's location is the issuer's own
                // directory entry, not a URI.
                self.unreadable += 1;
                continue;
            };
            match asn1::read(name.content) {
                Some((full, _)) if full.tag == asn1::context(0) => {
                    for general in asn1::children(full).unwrap_or_default() {
                        Self::keep(&mut self.crl, &mut self.unreadable, general);
                    }
                }
                _ => self.unreadable += 1,
            }
        }
    }

    /// `AuthorityInfoAccessSyntax ::= SEQUENCE OF AccessDescription`;
    /// `AccessDescription ::= SEQUENCE { accessMethod OID, accessLocation
    /// GeneralName }`. Access methods other than OCSP and caIssuers are
    /// skipped without counting — they are not revocation locations.
    fn read_authority_info_access(&mut self, value: &[u8]) {
        let Some((seq, _)) = asn1::expect(value, asn1::SEQUENCE) else {
            self.unreadable += 1;
            return;
        };
        for description in asn1::children(seq).unwrap_or_default() {
            let parts = asn1::children(description).unwrap_or_default();
            let (Some(method), Some(location)) = (parts.first(), parts.get(1)) else {
                self.unreadable += 1;
                continue;
            };
            let method = (method.tag == asn1::OID)
                .then(|| asn1::oid_to_string(method.content))
                .flatten();
            match method.as_deref() {
                Some(oid::AD_OCSP) => Self::keep(&mut self.ocsp, &mut self.unreadable, *location),
                Some(oid::AD_CA_ISSUERS) => {
                    Self::keep(&mut self.ca_issuers, &mut self.unreadable, *location);
                }
                _ => {}
            }
        }
    }
}

/// Parse an X.509 v3 certificate (RFC 5280 §4.1).
pub fn parse_certificate(der: &[u8]) -> Option<Certificate<'_>> {
    let (cert, _) = asn1::expect(der, asn1::SEQUENCE)?;
    let kids = asn1::children(cert)?;
    let tbs = kids.first().filter(|t| t.tag == asn1::SEQUENCE)?;
    let tbs_der = tbs.raw;
    // The outer signatureAlgorithm (kids[1]) names the hash+scheme; the
    // signatureValue (kids[2]) is the issuer's signature over `tbs_der`.
    let sig_alg = kids.get(1).and_then(|alg| alg_id(*alg));
    let sig_value = kids
        .get(2)
        .filter(|t| t.tag == asn1::BIT_STRING)
        .and_then(|t| asn1::bit_string_bytes(*t))
        .unwrap_or(&[]);
    let tbs_kids = asn1::children(*tbs)?;
    let mut it = tbs_kids.iter().copied().peekable();
    // version [0] EXPLICIT INTEGER DEFAULT v1
    let mut version = 1u8;
    if let Some(v) = it.next_if(|t| t.tag == asn1::context(0)) {
        version = asn1::expect(v.content, asn1::INTEGER)
            .and_then(|(i, _)| i.content.first().copied())
            .map_or(0, |n| n.saturating_add(1));
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
    let spki_key_bits = asn1::children(spki)
        .and_then(|k| k.get(1).copied())
        .and_then(asn1::bit_string_bytes)
        .unwrap_or(&[]);
    let mut ocsp_signing = false;
    let mut ocsp_nocheck = false;
    let mut subject_key_id = None;
    let mut is_ca = false;
    let mut key_usage_cert_sign = None;
    let mut key_usage_crl_sign = None;
    let mut revocation_uris = RevocationUris::default();
    // Optional issuerUniqueID [1], subjectUniqueID [2], extensions [3].
    for t in it {
        if t.tag == asn1::context(3) {
            let (exts, _) = asn1::expect(t.content, asn1::SEQUENCE).unwrap_or((t, &[]));
            for ext in asn1::children(exts).unwrap_or_default() {
                let parts = asn1::children(ext).unwrap_or_default();
                let Some(oid_t) = parts.first().filter(|t| t.tag == asn1::OID) else {
                    continue;
                };
                let oid_s = asn1::oid_to_string(oid_t.content);
                if oid_s.as_deref() == Some("2.5.29.14") {
                    // extnValue OCTET STRING wrapping an OCTET STRING.
                    if let Some(outer) = parts.last().filter(|t| t.tag == asn1::OCTET_STRING)
                        && let Some((inner, _)) = asn1::expect(outer.content, asn1::OCTET_STRING)
                    {
                        subject_key_id = Some(inner.content);
                    }
                } else if oid_s.as_deref() == Some("2.5.29.19") {
                    // basicConstraints (RFC 5280 §4.2.1.9): extnValue OCTET STRING
                    // wrapping SEQUENCE { cA BOOLEAN DEFAULT FALSE, pathLen … }.
                    // cA is TRUE iff the first child is a BOOLEAN 0xFF.
                    if let Some(outer) = parts.last().filter(|t| t.tag == asn1::OCTET_STRING)
                        && let Some((seq, _)) = asn1::expect(outer.content, asn1::SEQUENCE)
                        && let Some(kids) = asn1::children(seq)
                        && let Some(first) = kids.first()
                        && first.tag == asn1::BOOLEAN
                    {
                        is_ca = first.content.first().copied() == Some(0xFF);
                    }
                } else if oid_s.as_deref() == Some("2.5.29.15") {
                    // keyUsage (RFC 5280 §4.2.1.3): extnValue OCTET STRING wrapping
                    // a BIT STRING. keyCertSign is bit 5 → (first bit byte & 0x04).
                    // The BIT STRING content is [unused_count, bit_bytes…]; a
                    // nonzero unused count is normal here, so read content raw.
                    if let Some(outer) = parts.last().filter(|t| t.tag == asn1::OCTET_STRING)
                        && let Some((bits, _)) = asn1::expect(outer.content, asn1::BIT_STRING)
                    {
                        let first = bits.content.get(1).copied().unwrap_or(0);
                        key_usage_cert_sign = Some(first & 0x04 != 0);
                        key_usage_crl_sign = Some(first & 0x02 != 0);
                    }
                } else if let Some(outer) = parts.last().filter(|t| t.tag == asn1::OCTET_STRING) {
                    // extnValue OCTET STRING wraps the extension's own DER.
                    match oid_s.as_deref() {
                        Some(oid::CRL_DISTRIBUTION_POINTS) => {
                            revocation_uris.read_crl_distribution_points(outer.content);
                        }
                        Some(oid::AUTHORITY_INFO_ACCESS) => {
                            revocation_uris.read_authority_info_access(outer.content);
                        }
                        Some(oid::EXT_KEY_USAGE) => {
                            ocsp_signing = asn1::expect(outer.content, asn1::SEQUENCE)
                                .and_then(|(seq, _)| asn1::children(seq))
                                .unwrap_or_default()
                                .iter()
                                .any(|k| {
                                    k.tag == asn1::OID
                                        && asn1::oid_to_string(k.content).as_deref()
                                            == Some(oid::KP_OCSP_SIGNING)
                                });
                        }
                        Some(oid::OCSP_NOCHECK) => ocsp_nocheck = true,
                        _ => {}
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
        sig_alg,
        sig_value,
        is_ca,
        key_usage_cert_sign,
        key_usage_crl_sign,
        version,
        revocation_uris,
        spki_key_bits,
        ocsp_signing,
        ocsp_nocheck,
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
pub(crate) fn name_to_string(name: Tlv<'_>) -> String {
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
    pub fn signer_certificate_der(&self) -> Option<&'a [u8]> {
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
    pub fn signer_certificate(&self) -> Option<Certificate<'_>> {
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

/// The digest a `digestAlgorithm` OID names, or `None` for one pdfcer does
/// not verify (MD5 is deliberately absent: PAdES forbids it and ISO 32000-2
/// deprecates it, so a verdict over it would be `Unverifiable` by name).
pub fn hash_for(oid_str: &str) -> Option<Hash> {
    match oid_str {
        oid::SHA1 => Some(Hash::Sha1),
        oid::SHA256 => Some(Hash::Sha256),
        oid::SHA384 => Some(Hash::Sha384),
        oid::SHA512 => Some(Hash::Sha512),
        _ => None,
    }
}

/// `RSASSA-PSS-params` (RFC 4055 §3.1): `(hash, mgf1 hash, salt length)`,
/// with the RFC's defaults for absent fields.
pub fn pss_params(alg: &AlgId<'_>) -> Result<(Hash, Hash, usize), String> {
    let mut hash = Hash::Sha1;
    let mut mgf = Hash::Sha1;
    let mut salt = 20usize;
    let Some(params) = alg.params.filter(|p| p.tag == crate::asn1::SEQUENCE) else {
        return Ok((hash, mgf, salt));
    };
    for field in crate::asn1::children(params).unwrap_or_default() {
        match field.tag {
            0xA0 => {
                let (h, _) = crate::asn1::read(field.content).ok_or("bad PSS hashAlgorithm")?;
                let oid_s = crate::asn1::children(h)
                    .and_then(|k| k.first().copied())
                    .and_then(|t| crate::asn1::oid_to_string(t.content))
                    .ok_or("bad PSS hashAlgorithm")?;
                hash = hash_for(&oid_s).ok_or_else(|| format!("PSS hash {oid_s} unsupported"))?;
            }
            0xA1 => {
                let (m, _) = crate::asn1::read(field.content).ok_or("bad PSS maskGenAlgorithm")?;
                let kids = crate::asn1::children(m).ok_or("bad PSS maskGenAlgorithm")?;
                let mgf_oid = kids
                    .first()
                    .and_then(|t| crate::asn1::oid_to_string(t.content))
                    .ok_or("bad PSS maskGenAlgorithm")?;
                if mgf_oid != oid::MGF1 {
                    return Err(format!(
                        "PSS mask generation function {mgf_oid} is not MGF1"
                    ));
                }
                let inner = kids.get(1).copied().ok_or("bad PSS MGF1 parameters")?;
                let oid_s = crate::asn1::children(inner)
                    .and_then(|k| k.first().copied())
                    .and_then(|t| crate::asn1::oid_to_string(t.content))
                    .ok_or("bad PSS MGF1 hash")?;
                mgf =
                    hash_for(&oid_s).ok_or_else(|| format!("PSS MGF1 hash {oid_s} unsupported"))?;
            }
            0xA2 => {
                let (i, _) = crate::asn1::read(field.content).ok_or("bad PSS saltLength")?;
                let b = crate::asn1::integer_bytes(i).ok_or("bad PSS saltLength")?;
                salt = b.iter().fold(0usize, |acc, &x| (acc << 8) | usize::from(x));
            }
            0xA3 => {
                let (i, _) = crate::asn1::read(field.content).ok_or("bad PSS trailerField")?;
                if crate::asn1::integer_bytes(i) != Some(&[1]) {
                    return Err("PSS trailerField is not 1 (0xBC)".into());
                }
            }
            _ => {}
        }
    }
    Ok((hash, mgf, salt))
}

#[cfg(test)]
// Test DER is built from small literals; a panic is the failure report.
#[allow(clippy::unwrap_used, clippy::cast_possible_truncation)]
mod revocation_tests {
    use super::*;

    /// DER TLV, short or two-byte long-form length.
    fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        if content.len() < 0x80 {
            out.push(content.len() as u8);
        } else {
            let n = u16::try_from(content.len()).unwrap();
            out.push(0x82);
            out.extend(n.to_be_bytes());
        }
        out.extend_from_slice(content);
        out
    }

    fn uri(s: &[u8]) -> Vec<u8> {
        tlv(GENERAL_NAME_URI, s)
    }

    /// `DistributionPoint { distributionPoint [0] { fullName [0] names } }`.
    fn full_name_point(names: &[Vec<u8>]) -> Vec<u8> {
        tlv(0x30, &tlv(0xA0, &tlv(0xA0, &names.concat())))
    }

    fn cdp(points: &[Vec<u8>]) -> RevocationUris {
        let mut r = RevocationUris::default();
        r.read_crl_distribution_points(&tlv(0x30, &points.concat()));
        r
    }

    #[test]
    fn full_name_uris_are_kept_and_other_names_counted() {
        let r = cdp(&[
            full_name_point(&[uri(b"http://a.invalid/x.crl"), tlv(0xA4, &tlv(0x30, &[]))]),
            // nameRelativeToCRLIssuer [1]: no URI to keep.
            tlv(0x30, &tlv(0xA0, &tlv(0xA1, &[]))),
            // cRLIssuer [2] only.
            tlv(0x30, &tlv(0xA2, &[])),
            full_name_point(&[uri("http://é.invalid".as_bytes()), uri(b"")]),
        ]);
        assert_eq!(r.crl, ["http://a.invalid/x.crl"]);
        assert_eq!(r.unreadable, 5);
    }

    #[test]
    fn each_list_is_capped() {
        let names: Vec<_> = (0..MAX_REVOCATION_URIS + 3)
            .map(|i| uri(format!("http://c.invalid/{i}").as_bytes()))
            .collect();
        let r = cdp(&[full_name_point(&names)]);
        assert_eq!(r.crl.len(), MAX_REVOCATION_URIS);
        assert_eq!(r.unreadable, 3);
    }

    #[test]
    fn access_methods_route_to_their_lists_and_others_are_skipped() {
        let oid = |dotted_tail: &[u8]| {
            tlv(
                asn1::OID,
                &[&[0x2B, 6, 1, 5, 5, 7, 48][..], dotted_tail].concat(),
            )
        };
        let desc = |method: Vec<u8>, name: Vec<u8>| tlv(0x30, &[method, name].concat());
        let aia = tlv(
            0x30,
            &[
                desc(oid(&[1]), uri(b"http://ocsp.invalid")),
                desc(oid(&[2]), uri(b"http://ca.invalid/ca.cer")),
                desc(oid(&[3]), uri(b"http://tsa.invalid")),
                desc(oid(&[1]), tlv(0xA4, &[])),
            ]
            .concat(),
        );
        let mut r = RevocationUris::default();
        r.read_authority_info_access(&aia);
        assert_eq!(r.ocsp, ["http://ocsp.invalid"]);
        assert_eq!(r.ca_issuers, ["http://ca.invalid/ca.cer"]);
        assert_eq!(
            r.unreadable, 1,
            "the directoryName OCSP location; id-ad-timeStamping skipped"
        );
    }

    #[test]
    fn malformed_extension_values_count_once() {
        let mut r = RevocationUris::default();
        r.read_crl_distribution_points(&[0x04, 0x00]);
        r.read_authority_info_access(&[0xFF]);
        assert_eq!(
            r,
            RevocationUris {
                unreadable: 2,
                ..RevocationUris::default()
            }
        );
    }
}

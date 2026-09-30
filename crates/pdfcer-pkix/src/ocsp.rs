//! OCSP responses (RFC 6960): read one, and decide what it says about a
//! certificate.
//!
//! Offline only: the responses come from the document's `/DSS` or from a
//! caller that fetched them. Every doubt resolves to
//! [`OcspStatus::Unusable`], never to a false "not revoked":
//!
//! - Only a `successful` response carrying an `id-pkix-ocsp-basic` body is
//!   evidence; the other statuses are unsigned (§4.2.1).
//! - The answer is the `SingleResponse` whose `CertID` matches the
//!   certificate on all four fields, never simply the first (§4.2.2.3).
//! - The signer must be named by the `ResponderID`, its signature must
//!   verify over `tbsResponseData`, and it must be the issuing CA or a
//!   certificate that CA issued with `id-kp-OCSPSigning` (§4.2.2.2). A
//!   delegate without `id-pkix-ocsp-nocheck` is used only when a usable CRL
//!   shows it is not revoked.
//! - `unknown`, a `nextUpdate` before the reference time, a version other
//!   than v1 and an unrecognised critical extension make it unusable.
//!
//! Tagging is EXPLICIT by default in this module (§4), so `byKey` is
//! `A2 04 14 …` and `revoked` is an IMPLICIT `A1` holding no inner SEQUENCE.

use pdfcer_model::crypto::rsa::Hash;

use crate::asn1::{self, Tlv};
use crate::cms::{self, AlgId, Certificate, oid};
use crate::crl::{self, Crl, CrlReason, CrlStatus};
use crate::trust_chain::{verify_cert_signature, verify_signed};

/// The most `SingleResponse`s or `certs` one response may carry before it is
/// refused as unusable.
pub const MAX_OCSP_ITEMS: usize = 10_000;

const ENUMERATED: u8 = 0x0A;

/// `ResponderID` (RFC 6960 §4.2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponderId<'a> {
    /// `byName`: the responder's subject `Name`, raw DER.
    ByName(&'a [u8]),
    /// `byKey`: the SHA-1 of the responder's `subjectPublicKey` value.
    ByKey(&'a [u8]),
}

/// A `CertStatus` (RFC 6960 §4.2.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CertStatus {
    Good,
    Revoked {
        /// `revocationTime`, ISO-8601.
        time: Option<String>,
        reason: Option<CrlReason>,
    },
    Unknown,
}

/// One `SingleResponse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleResponse<'a> {
    /// The `CertID` hash, or `None` for one pdfcer does not compute.
    pub hash: Option<Hash>,
    pub issuer_name_hash: &'a [u8],
    pub issuer_key_hash: &'a [u8],
    /// The serial number, as [`asn1::integer_bytes`] strips it.
    pub serial: &'a [u8],
    pub status: CertStatus,
    /// `thisUpdate`, ISO-8601.
    pub this_update: Option<String>,
    /// `nextUpdate`, ISO-8601; `None` when absent ("newer information is
    /// always available", §4.2.2.1).
    pub next_update: Option<String>,
    /// Why this answer cannot be used whatever its signature, if it cannot.
    pub defect: Option<String>,
}

/// A parsed `OCSPResponse` (or a bare `BasicOCSPResponse`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcspResponse<'a> {
    /// `responseStatus`; 0 is `successful`. A bare `BasicOCSPResponse`
    /// reads as 0.
    pub status: u8,
    pub responder: Option<ResponderId<'a>>,
    /// `producedAt`, ISO-8601.
    pub produced_at: Option<String>,
    pub responses: Vec<SingleResponse<'a>>,
    /// The raw `tbsResponseData` — what the responder signed.
    pub tbs_der: &'a [u8],
    pub sig_alg: Option<AlgId<'a>>,
    /// The `signature` BIT STRING contents.
    pub sig_value: &'a [u8],
    /// The `certs` the responder attached, raw DER.
    pub certs: Vec<&'a [u8]>,
    /// Why this response cannot be used at all, if it cannot.
    pub defect: Option<String>,
}

/// Parse a DER `OCSPResponse`, or a bare `BasicOCSPResponse` as legacy
/// `/DSS` entries hold (ETSI EN 319 142-1 §5.4.2.2). `None` when it is
/// neither.
#[must_use]
pub fn parse_ocsp(der: &[u8]) -> Option<OcspResponse<'_>> {
    let (outer, _) = asn1::expect(der, asn1::SEQUENCE)?;
    let (first, _) = asn1::read(outer.content)?;
    match first.tag {
        ENUMERATED => parse_full(outer),
        asn1::SEQUENCE => parse_basic(outer),
        _ => None,
    }
}

fn parse_full(outer: Tlv<'_>) -> Option<OcspResponse<'_>> {
    let kids = asn1::children(outer)?;
    let status_tlv = kids.first()?;
    let status = match status_tlv.content {
        [s] => *s,
        _ => return None,
    };
    let unsuccessful = |defect: String| OcspResponse {
        status,
        responder: None,
        produced_at: None,
        responses: Vec::new(),
        tbs_der: &[],
        sig_alg: None,
        sig_value: &[],
        certs: Vec::new(),
        defect: Some(defect),
    };
    if status != 0 {
        return Some(unsuccessful(format!(
            "the responder answered {} rather than with a status",
            status_name(status)
        )));
    }
    let Some(bytes) = kids.get(1).filter(|t| t.tag == asn1::context(0)) else {
        return Some(unsuccessful(
            "a successful response carries no responseBytes".to_owned(),
        ));
    };
    let (rb, _) = asn1::expect(bytes.content, asn1::SEQUENCE)?;
    let parts = asn1::children(rb)?;
    let kind = asn1::oid_to_string(parts.first().filter(|t| t.tag == asn1::OID)?.content)?;
    if kind != oid::OCSP_BASIC {
        return Some(unsuccessful(format!(
            "its response type {kind} is not id-pkix-ocsp-basic"
        )));
    }
    let body = parts.get(1).filter(|t| t.tag == asn1::OCTET_STRING)?;
    let (basic, _) = asn1::expect(body.content, asn1::SEQUENCE)?;
    parse_basic(basic)
}

fn status_name(status: u8) -> &'static str {
    match status {
        1 => "malformedRequest",
        2 => "internalError",
        3 => "tryLater",
        5 => "sigRequired",
        6 => "unauthorized",
        _ => "an unknown response status",
    }
}

/// `BasicOCSPResponse ::= SEQUENCE { tbsResponseData, signatureAlgorithm,
/// signature BIT STRING, certs [0] EXPLICIT SEQUENCE OF Certificate OPTIONAL }`.
fn parse_basic(basic: Tlv<'_>) -> Option<OcspResponse<'_>> {
    let kids = asn1::children(basic)?;
    let tbs = kids.first().filter(|t| t.tag == asn1::SEQUENCE)?;
    let sig_alg = kids.get(1).and_then(|t| cms::alg_id(*t));
    let sig_value = kids
        .get(2)
        .and_then(|t| asn1::bit_string_bytes(*t))
        .unwrap_or(&[]);
    let mut out = OcspResponse {
        status: 0,
        responder: None,
        produced_at: None,
        responses: Vec::new(),
        tbs_der: tbs.raw,
        sig_alg,
        sig_value,
        certs: Vec::new(),
        defect: None,
    };
    if let Some(certs) = kids.get(3).filter(|t| t.tag == asn1::context(0)) {
        let (seq, _) = asn1::expect(certs.content, asn1::SEQUENCE)?;
        for c in asn1::children(seq)?.into_iter().take(MAX_OCSP_ITEMS) {
            out.certs.push(c.raw);
        }
    }
    let mut fields = asn1::children(*tbs)?.into_iter().peekable();
    if let Some(v) = fields.next_if(|t| t.tag == asn1::context(0)) {
        let is_v1 = asn1::expect(v.content, asn1::INTEGER).is_some_and(|(i, _)| i.content == [0]);
        if !is_v1 {
            out.defect = Some("its version is not v1".to_owned());
        }
    }
    let responder = fields.next()?;
    out.responder = match responder.tag {
        0xA1 => Some(ResponderId::ByName(
            asn1::expect(responder.content, asn1::SEQUENCE)?.0.raw,
        )),
        0xA2 => Some(ResponderId::ByKey(
            asn1::expect(responder.content, asn1::OCTET_STRING)?
                .0
                .content,
        )),
        _ => return None,
    };
    out.produced_at = asn1::time_value(fields.next()?);
    let list = fields.next().filter(|t| t.tag == asn1::SEQUENCE)?;
    let mut rest = list.content;
    while !rest.is_empty() {
        if out.responses.len() >= MAX_OCSP_ITEMS {
            out.defect = Some(format!(
                "it carries more than {MAX_OCSP_ITEMS} single responses"
            ));
            break;
        }
        let Some((single, r)) = asn1::expect(rest, asn1::SEQUENCE) else {
            out.defect = Some("a single response is malformed".to_owned());
            break;
        };
        rest = r;
        match read_single(single) {
            Some(s) => out.responses.push(s),
            None => {
                out.defect = Some("a single response is malformed".to_owned());
                break;
            }
        }
    }
    if let Some(exts) = fields.next_if(|t| t.tag == asn1::context(1))
        && let Some(why) = critical_unknown(exts, &[])
    {
        out.defect.get_or_insert(why);
    }
    Some(out)
}

/// `SingleResponse ::= SEQUENCE { certID, certStatus, thisUpdate,
/// nextUpdate [0] EXPLICIT OPTIONAL, singleExtensions [1] EXPLICIT OPTIONAL }`.
fn read_single(single: Tlv<'_>) -> Option<SingleResponse<'_>> {
    let mut f = asn1::children(single)?.into_iter().peekable();
    let id = asn1::children(f.next().filter(|t| t.tag == asn1::SEQUENCE)?)?;
    let hash = cms::alg_id(*id.first()?).and_then(|a| cms::hash_for(&a.oid));
    let issuer_name_hash = id.get(1).filter(|t| t.tag == asn1::OCTET_STRING)?.content;
    let issuer_key_hash = id.get(2).filter(|t| t.tag == asn1::OCTET_STRING)?.content;
    let serial = asn1::integer_bytes(*id.get(3)?)?;
    let st = f.next()?;
    let mut defect = None;
    let status = match st.tag {
        0x80 => CertStatus::Good,
        0x82 => CertStatus::Unknown,
        0xA1 => {
            // [1] IMPLICIT RevokedInfo: the SEQUENCE's fields sit directly
            // inside; the reason is [0] EXPLICIT ENUMERATED.
            let (time, rest) = asn1::read(st.content)?;
            let reason = match asn1::read(rest) {
                Some((r, _)) if r.tag == asn1::context(0) => {
                    let code =
                        asn1::expect(r.content, ENUMERATED).and_then(|(e, _)| match e.content {
                            [c] => CrlReason::from_code(*c),
                            _ => None,
                        });
                    if code.is_none() {
                        defect = Some("its revocationReason is not a known CRLReason".to_owned());
                    }
                    code
                }
                _ => None,
            };
            CertStatus::Revoked {
                time: asn1::time_value(time),
                reason,
            }
        }
        _ => return None,
    };
    let this_update = asn1::time_value(f.next()?);
    let next_update = f
        .next_if(|t| t.tag == asn1::context(0))
        .and_then(|t| asn1::read(t.content))
        .and_then(|(t, _)| asn1::time_value(t));
    if let Some(exts) = f.next_if(|t| t.tag == asn1::context(1))
        && let Some(why) = critical_unknown(exts, &[])
    {
        defect.get_or_insert(why);
    }
    Some(SingleResponse {
        hash,
        issuer_name_hash,
        issuer_key_hash,
        serial,
        status,
        this_update,
        next_update,
        defect,
    })
}

/// The first critical extension in `[n] EXPLICIT Extensions` whose OID is not
/// in `known`, described.
fn critical_unknown(exts: Tlv<'_>, known: &[&str]) -> Option<String> {
    let Some((seq, _)) = asn1::expect(exts.content, asn1::SEQUENCE) else {
        return Some("its extensions are malformed".to_owned());
    };
    for e in asn1::children(seq).unwrap_or_default() {
        let parts = asn1::children(e).unwrap_or_default();
        let oid = parts
            .first()
            .filter(|t| t.tag == asn1::OID)
            .and_then(|t| asn1::oid_to_string(t.content))
            .unwrap_or_default();
        let critical = parts
            .get(1)
            .is_some_and(|t| t.tag == asn1::BOOLEAN && t.content == [0xFF]);
        if critical && !known.contains(&oid.as_str()) {
            return Some(format!(
                "it carries the unrecognised critical extension {oid}"
            ));
        }
    }
    None
}

/// What one OCSP response says about one certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcspStatus {
    /// The response is usable and says the certificate is good.
    NotRevoked {
        this_update: Option<String>,
        next_update: Option<String>,
    },
    /// The response is usable and says the certificate is revoked.
    Revoked {
        date: Option<String>,
        reason: Option<CrlReason>,
    },
    /// The response cannot answer for this certificate, and why.
    Unusable { reason: String },
}

/// Whether `single` names `cert`, issued by `issuer` (RFC 6960 §4.1.1). The
/// key hash is accepted with or without the BIT STRING's unused-bits octet:
/// §4.1.1's text includes it, errata 6165–6167 and every measured producer
/// omit it, and for every RSA/EC key the two differ only by that `00`.
fn names(single: &SingleResponse<'_>, cert: &Certificate<'_>, issuer: &Certificate<'_>) -> bool {
    let Some(hash) = single.hash else {
        return false;
    };
    let with_octet: Vec<u8> = std::iter::once(0)
        .chain(issuer.spki_key_bits.iter().copied())
        .collect();
    single.serial == cert.serial
        && single.issuer_name_hash == hash.digest(cert.issuer_der).as_slice()
        && (single.issuer_key_hash == hash.digest(issuer.spki_key_bits).as_slice()
            || single.issuer_key_hash == hash.digest(&with_octet).as_slice())
}

/// Whether `responder` is who the `ResponderID` names.
fn is_named(id: ResponderId<'_>, responder: &Certificate<'_>) -> bool {
    match id {
        ResponderId::ByName(name) => name == responder.subject_der,
        ResponderId::ByKey(hash) => hash == Hash::Sha1.digest(responder.spki_key_bits).as_slice(),
    }
}

/// Check `cert` against `resp`, where `issuer` issued `cert`. `at` is the
/// ISO-8601 reference time the response must still be current at; `None`
/// skips that test. `crls` are consulted only for a delegated responder
/// without `id-pkix-ocsp-nocheck`.
#[must_use]
pub fn check_ocsp(
    resp: &OcspResponse<'_>,
    cert: &Certificate<'_>,
    issuer: &Certificate<'_>,
    at: Option<&str>,
    crls: &[Crl<'_>],
) -> OcspStatus {
    let unusable = |reason: String| OcspStatus::Unusable { reason };
    if let Some(defect) = &resp.defect {
        return unusable(defect.clone());
    }
    let Some(single) = resp.responses.iter().find(|s| names(s, cert, issuer)) else {
        return unusable(format!("it has no answer for {}", cert.subject));
    };
    let Some(id) = resp.responder else {
        return unusable("it names no responder".to_owned());
    };
    if let Err(why) = authorize(resp, id, issuer, crls) {
        return unusable(why);
    }
    if let Some(defect) = &single.defect {
        return unusable(defect.clone());
    }
    if let (Some(at), Some(next)) = (at, single.next_update.as_deref())
        && next < at
    {
        return unusable(format!("it expired (nextUpdate {next}) before {at}"));
    }
    match &single.status {
        CertStatus::Good => OcspStatus::NotRevoked {
            this_update: single.this_update.clone(),
            next_update: single.next_update.clone(),
        },
        CertStatus::Revoked { time, reason } => OcspStatus::Revoked {
            date: time.clone(),
            reason: *reason,
        },
        CertStatus::Unknown => unusable("the responder does not know the certificate".to_owned()),
    }
}

/// Find the signer the `ResponderID` names — the issuer itself or a
/// certificate in `resp.certs` — and require that it signed the response and
/// may answer for `issuer`'s certificates (RFC 6960 §4.2.2.2).
fn authorize(
    resp: &OcspResponse<'_>,
    id: ResponderId<'_>,
    issuer: &Certificate<'_>,
    crls: &[Crl<'_>],
) -> Result<(), String> {
    let signed_by = |c: &Certificate<'_>| {
        verify_signed(resp.tbs_der, resp.sig_alg.as_ref(), resp.sig_value, &c.key)
    };
    if is_named(id, issuer) {
        return if signed_by(issuer) {
            Ok(())
        } else {
            Err("its signature does not verify with the issuer's key".to_owned())
        };
    }
    let mut why = "it was signed by a responder whose certificate is not available".to_owned();
    for delegate in resp.certs.iter().filter_map(|d| cms::parse_certificate(d)) {
        if !is_named(id, &delegate) {
            continue;
        }
        if !signed_by(&delegate) {
            why = "its signature does not verify with the responder's key".to_owned();
            continue;
        }
        if delegate.issuer_der != issuer.subject_der
            || !verify_cert_signature(&delegate, &issuer.key)
        {
            why = format!(
                "its responder {} was not issued by {}",
                delegate.subject, issuer.subject
            );
            continue;
        }
        if !delegate.ocsp_signing {
            why = format!(
                "its responder {} is not authorized to sign OCSP responses (no id-kp-OCSPSigning)",
                delegate.subject
            );
            continue;
        }
        if let (Some(at), Some(nb), Some(na)) = (
            resp.produced_at.as_deref(),
            delegate.not_before.as_deref(),
            delegate.not_after.as_deref(),
        ) && (at < nb || at > na)
        {
            why = format!(
                "its responder {} was not valid when the response was produced",
                delegate.subject
            );
            continue;
        }
        if delegate.ocsp_nocheck {
            return Ok(());
        }
        let mut responder_status = None;
        for c in crls.iter().filter(|c| c.issuer_der == issuer.subject_der) {
            match crl::check_crl(c, &delegate, issuer, resp.produced_at.as_deref()) {
                CrlStatus::NotRevoked => responder_status = Some(true),
                CrlStatus::Revoked { .. } => {
                    return Err(format!("its responder {} is revoked", delegate.subject));
                }
                CrlStatus::Unusable { .. } => {}
            }
        }
        if responder_status == Some(true) {
            return Ok(());
        }
        why = format!(
            "its responder {} carries no ocsp-nocheck and no usable CRL covers it",
            delegate.subject
        );
    }
    Err(why)
}

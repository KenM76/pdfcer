//! RFC 3161 signature time-stamps — the PAdES **B-T** carrier.
//!
//! Spec: `D:\Dev\Rag-Specialized\PDF_Spec\security\security__rfc3161_timestamp.md`
//! (`TS-0`…`TS-11`); RFC 3161 §2.4 and Appendix A; ETSI EN 319 142-1 §5.3
//! (B-T = B-B + a signature time-stamp).
//!
//! The core never touches a network: a shell supplies a
//! [`TimestampAuthority`] that turns a DER `TimeStampReq` into a DER
//! `TimeStampResp` (HTTP `POST`, `application/timestamp-query` →
//! `application/timestamp-reply`, `TS-8`). Everything around that exchange —
//! building the request, checking the answer, verifying the token and
//! embedding it — is here, so no shell can embed an unchecked token (`TS-10`).
//!
//! What is stamped (`TS-5`/`TS-6`): the `SignerInfo.signature` OCTET STRING
//! value, hashed under the signature's own digest. The token goes into
//! `SignerInfo.unsignedAttrs` as `id-aa-timeStampToken`; it is unsigned
//! because it stamps the signature value, so adding it moves no signed byte.

use crate::cms::{self, oid};
use crate::crypto::rsa::Hash;
use crate::{asn1, asn1::Tlv};

use super::der_out;

/// `id-aa-timeStampToken` (RFC 3161 Appendix A).
pub const TIME_STAMP_TOKEN_OID: &str = "1.2.840.113549.1.9.16.2.14";
/// `id-ct-TSTInfo` (RFC 3161 §2.4.2) — the token's `eContentType`.
pub const TST_INFO_OID: &str = "1.2.840.113549.1.9.16.1.4";
/// `id-kp-timeStamping` (RFC 3161 §2.3).
const KP_TIME_STAMPING: &str = "1.3.6.1.5.5.7.3.8";
/// `extendedKeyUsage` (RFC 5280 §4.2.1.12).
const EXT_KEY_USAGE: &str = "2.5.29.37";

/// A time-stamping authority: the one network act B-T needs, done by a shell.
///
/// # Contract
///
/// [`time_stamp`](Self::time_stamp) receives a complete DER `TimeStampReq`
/// and returns the TSA's complete DER `TimeStampResp`, unmodified. It does
/// not interpret the answer — a rejection is still `Ok` here and is refused
/// by name afterwards. `Err` is for transport failure only (unreachable
/// host, HTTP error status, wrong content type); its text reaches the
/// operator verbatim. `Send + Sync` so a GUI may sign off its UI thread.
pub trait TimestampAuthority: Send + Sync {
    /// Exchange one request for one response.
    ///
    /// # Errors
    ///
    /// A description of the transport failure.
    fn time_stamp(&self, request_der: &[u8]) -> Result<Vec<u8>, String>;
}

/// What the embedded time-stamp says — the rule-4 disclosure. `genTime` is
/// the TSA's assertion, reported verbatim; pdfcer never infers a time.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TimestampInfo {
    /// `TSTInfo.genTime` as ISO 8601 UTC (`YYYY-MM-DDTHH:MM:SSZ`).
    pub gen_time: String,
    /// `TSTInfo.serialNumber`, upper-case hex.
    pub serial_hex: String,
    /// `TSTInfo.policy`, dotted OID.
    pub policy_oid: String,
    /// The TSA certificate's subject (`CN=…, O=…`).
    pub tsa_subject: String,
    /// The imprint's digest algorithm (`SHA-256`, `SHA-384`).
    pub digest_algorithm: &'static str,
    /// The token's DER size — what it added to the `/Contents` hole.
    pub token_bytes: usize,
}

/// Why a time-stamp was not embedded. Every variant is a refusal: pdfcer
/// never silently downgrades a requested B-T to B-B.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TimestampError {
    /// The authority could not be reached; the text is the shell's.
    #[error("the time-stamping authority could not be reached: {0}")]
    Transport(String),
    /// The platform gave no random bytes for the request nonce.
    #[error("no random bytes are available for the time-stamp nonce ({0})")]
    RandomUnavailable(String),
    /// The response is not a well-formed `TimeStampResp`/token.
    #[error("the time-stamping authority's response is malformed: {0}")]
    Malformed(&'static str),
    /// `PKIStatus` was neither granted (0) nor grantedWithMods (1) (`TS-4`).
    #[error("the time-stamping authority refused the request (status {status}){detail}")]
    Rejected {
        /// The `PKIStatus` integer.
        status: u64,
        /// `": "` + the status string and named failure bits, or empty.
        detail: String,
    },
    /// The token stamps a different hash than the one requested (`TS-10`).
    #[error(
        "the time-stamp token's message imprint does not match the signature it was requested for"
    )]
    ImprintMismatch,
    /// The token does not echo the request nonce (`TS-2`).
    #[error(
        "the time-stamp token does not echo the request nonce (a replayed or substituted response)"
    )]
    NonceMismatch,
    /// The token carries no TSA certificate although `certReq` was TRUE (`TS-3`).
    #[error("the time-stamp token does not include the authority's certificate")]
    NoTsaCertificate,
    /// The TSA certificate lacks the critical `id-kp-timeStamping` extended
    /// key usage RFC 3161 §2.3 requires.
    #[error(
        "the authority's certificate is not a time-stamping certificate (RFC 3161 §2.3 needs a critical extendedKeyUsage of id-kp-timeStamping)"
    )]
    NotATimeStampingCertificate,
    /// pdfcer's own `SignedData` could not take the token — an internal
    /// inconsistency, reported rather than patched around.
    #[error("internal: the time-stamp token could not be embedded in the signature's SignerInfo")]
    EmbedFailed,
    /// The TSA's signature over the `TSTInfo` did not verify.
    #[error("the time-stamp token's signature is invalid: {0}")]
    TokenSignatureInvalid(String),
}

/// A built request plus what its answer must echo.
pub(crate) struct Request {
    pub der: Vec<u8>,
    hash: Hash,
    imprint: Vec<u8>,
    nonce: [u8; 8],
}

/// `TimeStampReq` (RFC 3161 §2.4.1) over `datum` hashed with `hash`:
/// version 1, a random 64-bit nonce (`TS-2`), `certReq` TRUE (`TS-3`), no
/// policy, no extensions.
pub(crate) fn build_request(hash: Hash, datum: &[u8]) -> Result<Request, TimestampError> {
    let mut nonce = [0u8; 8];
    crate::crypto::rng::fill(&mut nonce)
        .map_err(|e| TimestampError::RandomUnavailable(e.to_string()))?;
    Ok(build_request_with_nonce(hash, datum, nonce))
}

fn build_request_with_nonce(hash: Hash, datum: &[u8], nonce: [u8; 8]) -> Request {
    let imprint = hash.digest(datum);
    let der = der_out::sequence(&[
        der_out::integer_u64(1),
        message_imprint(hash, &imprint),
        der_out::integer(&nonce),
        der_out::tlv(0x01, &[0xFF]),
    ]);
    Request {
        der,
        hash,
        imprint,
        nonce,
    }
}

/// The fixed request the `timestamp_response` fuzz target answers: SHA-256
/// of `b"pdfcer-fuzz"`, nonce `01 02 03 04 05 06 07 08`. `#[doc(hidden)]`:
/// an instrument, not an API.
#[doc(hidden)]
#[must_use]
pub fn fuzz_request_der() -> Vec<u8> {
    fuzz_request().der
}

fn fuzz_request() -> Request {
    build_request_with_nonce(Hash::Sha256, b"pdfcer-fuzz", [1, 2, 3, 4, 5, 6, 7, 8])
}

/// Fuzz instrument: [`accept_response`] on untrusted bytes against
/// [`fuzz_request_der`]'s request, then, when accepted, [`embed_token`] of
/// the token into `cms`. Returns whether the response was accepted.
#[doc(hidden)]
#[must_use]
pub fn fuzz_accept_and_embed(response: &[u8], cms: &[u8]) -> bool {
    match accept_response(response, &fuzz_request()) {
        Ok((token, _)) => {
            let _ = signature_value(cms);
            let _ = embed_token(cms, &token);
            true
        }
        Err(_) => false,
    }
}

fn hash_oid(hash: Hash) -> &'static str {
    match hash {
        Hash::Sha1 => oid::SHA1,
        Hash::Sha256 => oid::SHA256,
        Hash::Sha384 => oid::SHA384,
        Hash::Sha512 => oid::SHA512,
    }
}

fn message_imprint(hash: Hash, imprint: &[u8]) -> Vec<u8> {
    // RFC 5754 §2: the SHA-2 AlgorithmIdentifier parameters are absent.
    der_out::sequence(&[
        der_out::algorithm_identifier(hash_oid(hash), None).unwrap_or_default(),
        der_out::octet_string(imprint),
    ])
}

/// Check a `TimeStampResp` against `request` and return the token's DER
/// and what it asserts (`TS-4`, `TS-10`).
pub(crate) fn accept_response(
    response: &[u8],
    request: &Request,
) -> Result<(Vec<u8>, TimestampInfo), TimestampError> {
    use TimestampError::Malformed;
    let (resp, _) = asn1::expect(response, asn1::SEQUENCE).ok_or(Malformed("not a SEQUENCE"))?;
    let kids = asn1::children(resp).ok_or(Malformed("TimeStampResp"))?;
    let status_info = kids
        .first()
        .filter(|t| t.tag == asn1::SEQUENCE)
        .ok_or(Malformed("no PKIStatusInfo"))?;
    let status_kids = asn1::children(*status_info).ok_or(Malformed("PKIStatusInfo"))?;
    let status = status_kids
        .first()
        .copied()
        .and_then(small_uint)
        .ok_or(Malformed("PKIStatus"))?;
    if status > 1 {
        return Err(TimestampError::Rejected {
            status,
            detail: rejection_detail(&status_kids),
        });
    }
    let token = kids
        .get(1)
        .filter(|t| t.tag == asn1::SEQUENCE)
        .ok_or(Malformed("status granted but no TimeStampToken (TS-4)"))?;
    let info = check_token(token.raw, request)?;
    Ok((token.raw.to_vec(), info))
}

fn small_uint(t: Tlv<'_>) -> Option<u64> {
    let b = asn1::integer_bytes(t)?;
    (b.len() <= 8).then(|| b.iter().fold(0u64, |a, &x| (a << 8) | u64::from(x)))
}

/// `": text; failInfo: badAlg, …"` from the optional status fields.
fn rejection_detail(status_kids: &[Tlv<'_>]) -> String {
    const FAIL: [(usize, &str); 8] = [
        (0, "badAlg"),
        (2, "badRequest"),
        (5, "badDataFormat"),
        (14, "timeNotAvailable"),
        (15, "unacceptedPolicy"),
        (16, "unacceptedExtension"),
        (17, "addInfoNotAvailable"),
        (25, "systemFailure"),
    ];
    let mut parts = Vec::new();
    for t in status_kids.iter().skip(1) {
        if t.tag == asn1::SEQUENCE {
            let text: Vec<String> = asn1::children(*t)
                .unwrap_or_default()
                .into_iter()
                .filter_map(asn1::string_value)
                .collect();
            if !text.is_empty() {
                parts.push(text.join(" "));
            }
        } else if t.tag == 0x03 {
            // BIT STRING: named bits, bit 0 = the MSB of the first octet.
            let bits = t.content.get(1..).unwrap_or(&[]);
            let set: Vec<&str> = FAIL
                .iter()
                .filter(|(n, _)| bits.get(n / 8).is_some_and(|b| b & (0x80 >> (n % 8)) != 0))
                .map(|(_, name)| *name)
                .collect();
            if !set.is_empty() {
                parts.push(format!("failInfo: {}", set.join(", ")));
            }
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(": {}", parts.join("; "))
    }
}

/// Verify a `TimeStampToken` (a CMS `SignedData` over a `TSTInfo`) against
/// the request it answers.
fn check_token(token: &[u8], request: &Request) -> Result<TimestampInfo, TimestampError> {
    use TimestampError::Malformed;
    let sd = cms::parse_signed_data(token).ok_or(Malformed("the token is not a CMS SignedData"))?;
    if sd.content_type != TST_INFO_OID {
        return Err(Malformed("the token's eContentType is not id-ct-TSTInfo"));
    }
    if sd.signer_count != 1 {
        return Err(Malformed(
            "the token must carry exactly one signature (RFC 3161 §2.4.2)",
        ));
    }
    let tst = sd
        .econtent
        .ok_or(Malformed("the token carries no TSTInfo"))?;
    let parsed = parse_tst_info(tst).ok_or(Malformed("TSTInfo"))?;
    if parsed.imprint_oid != hash_oid(request.hash) || parsed.imprint != request.imprint {
        return Err(TimestampError::ImprintMismatch);
    }
    if parsed.nonce.map(strip_zeros) != Some(strip_zeros(&request.nonce)) {
        return Err(TimestampError::NonceMismatch);
    }

    let signer = sd
        .signer
        .as_ref()
        .ok_or(Malformed("the token's SignerInfo"))?;
    let cert = sd
        .signer_certificate()
        .ok_or(TimestampError::NoTsaCertificate)?;
    if !has_critical_time_stamping_eku(cert.tbs_der) {
        return Err(TimestampError::NotATimeStampingCertificate);
    }
    let hash = cms::hash_for(&signer.digest_alg.oid).ok_or(Malformed(
        "the token's digest algorithm is not SHA-1/256/384/512",
    ))?;
    let invalid = |why: &str| TimestampError::TokenSignatureInvalid(why.to_owned());
    if signer.content_type.as_deref() != Some(TST_INFO_OID) {
        return Err(invalid("its content-type attribute is not id-ct-TSTInfo"));
    }
    if signer.message_digest != Some(hash.digest(tst).as_slice()) {
        return Err(invalid("its messageDigest does not match the TSTInfo"));
    }
    let attrs = signer
        .signed_attrs_der
        .as_deref()
        .ok_or_else(|| invalid("it has no signed attributes"))?;
    let mut notes = Vec::new();
    match crate::signature_verify::check_signature(
        &cert.key,
        &signer.signature_alg,
        hash,
        &hash.digest(attrs),
        signer.signature,
        &mut notes,
    ) {
        Ok((true, _)) => {}
        Ok((false, alg)) => return Err(invalid(&format!("{alg} did not verify"))),
        Err(reason) => return Err(TimestampError::TokenSignatureInvalid(reason)),
    }
    Ok(TimestampInfo {
        gen_time: parsed.gen_time,
        serial_hex: parsed.serial.iter().map(|b| format!("{b:02X}")).collect(),
        policy_oid: parsed.policy,
        tsa_subject: cert.subject,
        digest_algorithm: request.hash.name(),
        token_bytes: token.len(),
    })
}

fn strip_zeros(b: &[u8]) -> &[u8] {
    let i = b.iter().position(|&x| x != 0).unwrap_or(b.len());
    b.get(i..).unwrap_or(&[])
}

struct TstInfo<'a> {
    policy: String,
    imprint_oid: String,
    imprint: &'a [u8],
    serial: &'a [u8],
    gen_time: String,
    nonce: Option<&'a [u8]>,
}

/// `TSTInfo` (RFC 3161 §2.4.2). Returns `None` on any structural defect.
fn parse_tst_info(der: &[u8]) -> Option<TstInfo<'_>> {
    let (seq, _) = asn1::expect(der, asn1::SEQUENCE)?;
    let kids = asn1::children(seq)?;
    let mut it = kids.into_iter();
    if small_uint(it.next()?)? != 1 {
        return None;
    }
    let policy = asn1::oid_to_string(it.next().filter(|t| t.tag == asn1::OID)?.content)?;
    let mi = asn1::children(it.next().filter(|t| t.tag == asn1::SEQUENCE)?)?;
    let alg = asn1::children(*mi.first().filter(|t| t.tag == asn1::SEQUENCE)?)?;
    let imprint_oid = asn1::oid_to_string(alg.first().filter(|t| t.tag == asn1::OID)?.content)?;
    let imprint = mi.get(1).filter(|t| t.tag == asn1::OCTET_STRING)?.content;
    let serial = asn1::integer_bytes(it.next()?)?;
    let gen_tlv = it.next().filter(|t| t.tag == asn1::GENERALIZED_TIME)?;
    let gen_time = asn1::time_value(gen_tlv)?;
    // accuracy SEQUENCE, ordering BOOLEAN, nonce INTEGER, [0] tsa, [1] ext —
    // all optional, in that order; only the nonce is read (`TS-11`).
    let nonce = it
        .find(|t| t.tag == asn1::INTEGER)
        .and_then(asn1::integer_bytes);
    Some(TstInfo {
        policy,
        imprint_oid,
        imprint,
        serial,
        gen_time,
        nonce,
    })
}

/// Whether `tbs` (a `TBSCertificate`) carries an `extendedKeyUsage`
/// extension that is critical and names `id-kp-timeStamping`.
fn has_critical_time_stamping_eku(tbs: &[u8]) -> bool {
    let Some((seq, _)) = asn1::read(tbs) else {
        return false;
    };
    let Some(fields) = asn1::children(seq) else {
        return false;
    };
    // extensions [3] EXPLICIT Extensions — the last field of a v3 TBS.
    let Some(ext_wrapper) = fields.iter().find(|t| t.tag == asn1::context(3)) else {
        return false;
    };
    let Some((exts, _)) = asn1::expect(ext_wrapper.content, asn1::SEQUENCE) else {
        return false;
    };
    for ext in asn1::children(exts).unwrap_or_default() {
        let parts = asn1::children(ext).unwrap_or_default();
        let is_eku = parts
            .first()
            .and_then(|t| asn1::oid_to_string(t.content))
            .is_some_and(|o| o == EXT_KEY_USAGE);
        if !is_eku {
            continue;
        }
        let critical = parts
            .get(1)
            .is_some_and(|t| t.tag == 0x01 && t.content == [0xFF]);
        let value = parts.iter().find(|t| t.tag == asn1::OCTET_STRING);
        let names_ts = value
            .and_then(|v| asn1::expect(v.content, asn1::SEQUENCE))
            .and_then(|(s, _)| asn1::children(s))
            .is_some_and(|purposes| {
                purposes
                    .iter()
                    .any(|p| asn1::oid_to_string(p.content).as_deref() == Some(KP_TIME_STAMPING))
            });
        return critical && names_ts;
    }
    false
}

/// The `SignerInfo.signature` value of `cms` (a DER `ContentInfo`) — the
/// datum a signature time-stamp hashes (`TS-6`).
pub(crate) fn signature_value(cms_der: &[u8]) -> Option<Vec<u8>> {
    let sd = cms::parse_signed_data(cms_der)?;
    Some(sd.signer?.signature.to_vec())
}

/// `cms` with `token` added to its (only) `SignerInfo` as the unsigned
/// attribute `id-aa-timeStampToken` (`TS-6`). Every other byte of the
/// `SignerInfo` — the signed attributes and the signature — is copied
/// verbatim; only the enclosing lengths change.
pub(crate) fn embed_token(cms_der: &[u8], token: &[u8]) -> Option<Vec<u8>> {
    let (ci, _) = asn1::expect(cms_der, asn1::SEQUENCE)?;
    let ci_kids = asn1::children(ci)?;
    let ct = ci_kids.first()?;
    let wrapper = ci_kids.get(1).filter(|t| t.tag == asn1::context(0))?;
    let (sd, _) = asn1::expect(wrapper.content, asn1::SEQUENCE)?;
    let sd_kids = asn1::children(sd)?;
    let (infos, head) = sd_kids.split_last()?;
    if infos.tag != asn1::SET {
        return None;
    }
    let signer_infos = asn1::children(*infos)?;
    let [si] = signer_infos.as_slice() else {
        return None;
    };
    let si_kids = asn1::children(*si)?;
    if si_kids.iter().any(|t| t.tag == asn1::context(1)) {
        return None; // already carries unsigned attributes
    }
    let attribute = der_out::sequence(&[
        der_out::oid(TIME_STAMP_TOKEN_OID)?,
        der_out::tlv(asn1::SET, token),
    ]);
    let mut si_body: Vec<u8> = si_kids.iter().flat_map(|t| t.raw.to_vec()).collect();
    si_body.extend(der_out::tlv(asn1::context(1), &attribute));
    let new_si = der_out::tlv(asn1::SEQUENCE, &si_body);
    let mut sd_body: Vec<u8> = head.iter().flat_map(|t| t.raw.to_vec()).collect();
    sd_body.extend(der_out::tlv(asn1::SET, &new_si));
    let new_sd = der_out::tlv(asn1::SEQUENCE, &sd_body);
    let mut ci_body = ct.raw.to_vec();
    ci_body.extend(der_out::tlv(asn1::context(0), &new_sd));
    Some(der_out::tlv(asn1::SEQUENCE, &ci_body))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn request_encodes_version_imprint_nonce_and_cert_req() {
        let r = build_request_with_nonce(Hash::Sha256, b"sig", [0x80, 1, 2, 3, 4, 5, 6, 7]);
        let (seq, rest) = asn1::expect(&r.der, asn1::SEQUENCE).unwrap();
        assert!(rest.is_empty());
        let kids = asn1::children(seq).unwrap();
        assert_eq!(kids.len(), 4);
        assert_eq!(small_uint(kids[0]), Some(1));
        let mi = asn1::children(kids[1]).unwrap();
        assert_eq!(mi[1].content, Hash::Sha256.digest(b"sig").as_slice());
        // A high-bit nonce gains the sign-padding zero (X.690 §8.3.2).
        assert_eq!(kids[2].content[0], 0);
        assert_eq!(asn1::integer_bytes(kids[2]).unwrap(), &r.nonce);
        assert_eq!((kids[3].tag, kids[3].content), (0x01, &[0xFF][..]));
    }

    #[test]
    fn a_rejection_is_refused_with_its_named_failure() {
        // TimeStampResp { PKIStatusInfo { 2, {"busy"}, failInfo badAlg } }
        let status = der_out::sequence(&[
            der_out::integer_u64(2),
            der_out::sequence(&[der_out::tlv(0x0C, b"busy")]),
            der_out::tlv(0x03, &[0x07, 0x80]),
        ]);
        let resp = der_out::sequence(&[status]);
        let req = build_request_with_nonce(Hash::Sha256, b"x", [1; 8]);
        match accept_response(&resp, &req) {
            Err(TimestampError::Rejected { status: 2, detail }) => {
                assert_eq!(detail, ": busy; failInfo: badAlg");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn granted_without_a_token_is_malformed() {
        let resp = der_out::sequence(&[der_out::sequence(&[der_out::integer_u64(0)])]);
        let req = build_request_with_nonce(Hash::Sha256, b"x", [1; 8]);
        assert!(matches!(
            accept_response(&resp, &req),
            Err(TimestampError::Malformed(_))
        ));
    }

    #[test]
    fn only_the_tsa_certificate_has_a_critical_time_stamping_eku() {
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/synthetic/signing/"
        );
        for (name, want) in [("tsa-rsa2048.cer", true), ("rsa2048.cer", false)] {
            let der = std::fs::read(format!("{dir}{name}")).unwrap();
            let cert = cms::parse_certificate(&der).unwrap();
            assert_eq!(has_critical_time_stamping_eku(cert.tbs_der), want, "{name}");
        }
    }
}

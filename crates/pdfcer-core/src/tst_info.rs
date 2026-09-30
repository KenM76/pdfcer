//! RFC 3161 `TSTInfo` reading (§2.4.2), shared by the signing side and
//! the verifier (which is not behind the `signing` feature).

use crate::asn1::{self, Tlv};
use crate::cms;
use pdfcer_model::crypto::rsa::Hash;

/// `id-ct-TSTInfo` (RFC 3161 §2.4.2).
pub(crate) const TST_INFO_OID: &str = "1.2.840.113549.1.9.16.1.4";

/// `(imprint digest, imprint, genTime)` from a `TSTInfo`, for the verifier.
pub(crate) fn tst_imprint(tst: &[u8]) -> Option<(Hash, Vec<u8>, String)> {
    let t = parse_tst_info(tst)?;
    let hash = cms::hash_for(&t.imprint_oid)?;
    Some((hash, t.imprint.to_vec(), t.gen_time))
}

/// A DER INTEGER of at most eight bytes as `u64`; `None` if longer.
pub(crate) fn small_uint(t: Tlv<'_>) -> Option<u64> {
    let b = asn1::integer_bytes(t)?;
    (b.len() <= 8).then(|| b.iter().fold(0u64, |a, &x| (a << 8) | u64::from(x)))
}

// Without `signing`, only the verifier reads it, and it needs no policy,
// serial or nonce.
#[cfg_attr(not(feature = "signing"), allow(dead_code))]
pub(crate) struct TstInfo<'a> {
    pub(crate) policy: String,
    pub(crate) imprint_oid: String,
    pub(crate) imprint: &'a [u8],
    pub(crate) serial: &'a [u8],
    pub(crate) gen_time: String,
    pub(crate) nonce: Option<&'a [u8]>,
}

/// `TSTInfo` (RFC 3161 §2.4.2). Returns `None` on any structural defect.
pub(crate) fn parse_tst_info(der: &[u8]) -> Option<TstInfo<'_>> {
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

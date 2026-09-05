//! A minimal **DER encoder** — the write-side twin of [`crate::asn1`]
//! (ITU-T X.690 §8, §10, §11.6), sized for exactly what a CMS `SignedData`
//! and its CAdES attributes need.
//!
//! # Why an encoder of our own rather than the `der` crate's derive types
//!
//! `der 0.8` is already in the dependency tree (through `rsa` and `p256`),
//! so taking it would cost nothing in crates — but it would put a foreign
//! type system between pdfcer's ASN.1 *reader* (`asn1.rs`, `cms.rs`) and its
//! *writer*, and the two must agree byte-for-byte: the round-trip test that
//! makes this module trustworthy is *"the existing verifier parses what this
//! wrote and computes the same signed-attributes digest"*. Two hundred lines
//! of encoder against a parser we already own is a smaller surface than a
//! second object model, and no secret passes through either (decision 129's
//! constant-time concern does not reach encoding).
//!
//! # What DER adds over BER, and where each rule lives here
//!
//! - **Definite lengths only** ([`tlv`]) — RFC 5652 §5.3 requires DER for
//!   `SignedAttributes` ("even if the rest of the structure is BER encoded")
//!   because the signature is over their encoding (`CB-8`).
//! - **Minimal INTEGER** ([`integer`]) — no redundant leading `0x00`, but a
//!   `0x00` IS prepended when the high bit of a non-negative value is set,
//!   or the value would read as negative (X.690 §8.3.2).
//! - **`SET OF` element ordering** ([`set_of`]) — X.690 §11.6: elements sorted
//!   by their complete encodings as octet strings, shorter-is-a-prefix first.
//!   Rust's `Vec<u8>` ordering is exactly that comparison. This is the rule a
//!   naive builder gets wrong, and strict verifiers (Adobe, ETSI checkers)
//!   reject the result while lenient ones (OpenSSL, pdfcer's own reader)
//!   accept it — so a test pins the order against a hand-computed case.
//! - **Short-form length below 128, long-form above** — X.690 §10.1 forbids
//!   the long form where the short form fits.
//!
//! Tags are single-byte only, matching the reader (nothing in CMS or X.509
//! uses a multi-byte tag).

/// Encode one TLV with a definite DER length (X.690 §8.1.3).
pub(crate) fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let len = content.len();
    let mut out = Vec::with_capacity(len + 6);
    out.push(tag);
    if len < 0x80 {
        // Short form: one octet, bit 8 clear.
        #[allow(clippy::cast_possible_truncation)] // len < 128 by the branch
        out.push(len as u8);
    } else {
        // Long form: 0x80 | count, then the length big-endian in the fewest
        // octets that hold it (X.690 §10.1 — DER forbids leading zero octets).
        let be = len.to_be_bytes();
        let first = be.iter().position(|&b| b != 0).unwrap_or(be.len() - 1);
        let digits = be.get(first..).unwrap_or(&[]);
        #[allow(clippy::cast_possible_truncation)] // at most 8 octets
        out.push(0x80 | digits.len() as u8);
        out.extend_from_slice(digits);
    }
    out.extend_from_slice(content);
    out
}

/// `SEQUENCE { items… }` (tag `0x30`), items already encoded, in order.
pub(crate) fn sequence(items: &[Vec<u8>]) -> Vec<u8> {
    tlv(crate::asn1::SEQUENCE, &items.concat())
}

/// `SET OF { items… }` (tag `0x31`) with the X.690 §11.6 DER ordering
/// applied — the elements are **sorted by their encodings**, so the caller
/// may pass them in any order.
///
/// The sort is by the complete encoding as an octet string (tag and length
/// included), ascending, with a shorter encoding that is a prefix of a
/// longer one sorting first — which is `Vec<u8>`'s natural `Ord`.
pub(crate) fn set_of(mut items: Vec<Vec<u8>>) -> Vec<u8> {
    items.sort();
    tlv(crate::asn1::SET, &items.concat())
}

/// A context-specific **constructed** tag `[n]` wrapping already-encoded
/// content — used both for `EXPLICIT [n]` (content is one complete TLV) and
/// for `IMPLICIT [n]` over a constructed type (content is the inner type's
/// content octets), which encode identically on the wire.
pub(crate) fn context(n: u8, content: &[u8]) -> Vec<u8> {
    tlv(crate::asn1::context(n), content)
}

/// `INTEGER` from big-endian magnitude bytes of a **non-negative** value
/// (X.690 §8.3): strip redundant leading zeros, then prepend one `0x00` if
/// the top bit is set so the value does not read as negative. An empty
/// input encodes zero.
pub(crate) fn integer(magnitude: &[u8]) -> Vec<u8> {
    let first = magnitude.iter().position(|&b| b != 0);
    let mut content: Vec<u8> = match first {
        Some(i) => magnitude.get(i..).unwrap_or(&[]).to_vec(),
        None => vec![0],
    };
    if content.first().is_some_and(|&b| b & 0x80 != 0) {
        content.insert(0, 0);
    }
    tlv(crate::asn1::INTEGER, &content)
}

/// `INTEGER` from a `u64`.
pub(crate) fn integer_u64(v: u64) -> Vec<u8> {
    integer(&v.to_be_bytes())
}

/// `OCTET STRING`.
pub(crate) fn octet_string(bytes: &[u8]) -> Vec<u8> {
    tlv(crate::asn1::OCTET_STRING, bytes)
}

/// `NULL` (tag `0x05`, zero length) — the parameters of `sha256` and
/// `rsaEncryption` AlgorithmIdentifiers (RFC 3370 / RFC 8017 A.2.4 say the
/// NULL "shall" be present for `rsaEncryption`; for the SHA-2 digest OIDs it
/// "should" be absent, but every deployed CMS producer writes it and every
/// verifier accepts both — pdfcer writes it, matching OpenSSL).
pub(crate) fn null() -> Vec<u8> {
    vec![0x05, 0x00]
}

/// `OBJECT IDENTIFIER` from dotted decimal (X.690 §8.19). `None` for a
/// string that is not an OID (fewer than two arcs, a non-numeric arc, or a
/// first arc above 2).
pub(crate) fn oid(dotted: &str) -> Option<Vec<u8>> {
    let arcs: Vec<u64> = dotted
        .split('.')
        .map(|a| a.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let (&first, rest) = arcs.split_first()?;
    let (&second, rest) = rest.split_first()?;
    if first > 2 || (first < 2 && second > 39) {
        return None;
    }
    let mut content = Vec::new();
    push_base128(&mut content, first.checked_mul(40)?.checked_add(second)?);
    for &arc in rest {
        push_base128(&mut content, arc);
    }
    Some(tlv(crate::asn1::OID, &content))
}

/// Base-128 with continuation bits, most significant group first (X.690
/// §8.19.2).
fn push_base128(out: &mut Vec<u8>, mut v: u64) {
    let mut groups = [0u8; 10];
    let mut n = 0;
    loop {
        #[allow(clippy::cast_possible_truncation)] // masked to 7 bits
        let g = (v & 0x7F) as u8;
        if let Some(slot) = groups.get_mut(n) {
            *slot = g;
        }
        n += 1;
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    for i in (0..n).rev() {
        let Some(&g) = groups.get(i) else { continue };
        out.push(if i == 0 { g } else { g | 0x80 });
    }
}

/// `AlgorithmIdentifier ::= SEQUENCE { algorithm OID, parameters ANY OPTIONAL }`
/// with the given already-encoded parameters (or none).
pub(crate) fn algorithm_identifier(oid_dotted: &str, params: Option<Vec<u8>>) -> Option<Vec<u8>> {
    let mut items = vec![oid(oid_dotted)?];
    if let Some(p) = params {
        items.push(p);
    }
    Some(sequence(&items))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::asn1;

    #[test]
    fn lengths_take_the_shortest_legal_form_and_round_trip() {
        let short = tlv(asn1::OCTET_STRING, &[1, 2, 3]);
        assert_eq!(short, vec![0x04, 0x03, 1, 2, 3]);
        let exactly_127 = tlv(asn1::OCTET_STRING, &[7u8; 127]);
        assert_eq!(&exactly_127[..2], &[0x04, 0x7F]);
        let exactly_128 = tlv(asn1::OCTET_STRING, &[7u8; 128]);
        assert_eq!(&exactly_128[..3], &[0x04, 0x81, 0x80]);
        let big = tlv(asn1::SEQUENCE, &[0u8; 70_000]);
        assert_eq!(&big[..5], &[0x30, 0x83, 0x01, 0x11, 0x70]);
        for enc in [&short, &exactly_127, &exactly_128, &big] {
            let (t, rest) = asn1::read(enc).unwrap();
            assert!(rest.is_empty());
            assert_eq!(t.raw, enc.as_slice());
        }
    }

    #[test]
    fn integers_are_minimal_and_never_negative() {
        assert_eq!(integer(&[0, 0, 0x05]), vec![0x02, 0x01, 0x05]);
        assert_eq!(
            integer(&[0x80]),
            vec![0x02, 0x02, 0x00, 0x80],
            "high bit needs a pad"
        );
        assert_eq!(integer(&[]), vec![0x02, 0x01, 0x00]);
        assert_eq!(integer(&[0, 0]), vec![0x02, 0x01, 0x00]);
        assert_eq!(integer_u64(1), vec![0x02, 0x01, 0x01]);
        assert_eq!(integer_u64(256), vec![0x02, 0x02, 0x01, 0x00]);
        let enc = integer(&[0xFF, 0x01]);
        let (t, _) = asn1::read(&enc).unwrap();
        assert_eq!(asn1::integer_bytes(t).unwrap(), &[0xFF, 0x01]);
    }

    #[test]
    fn oids_match_the_readers_decoding() {
        for s in [
            "1.2.840.113549.1.7.2",
            "2.16.840.1.101.3.4.2.1",
            "1.2.840.113549.1.9.16.2.47",
            "1.3.14.3.2.26",
            "0.9.2342.19200300.100.1.1",
        ] {
            let enc = oid(s).unwrap();
            let (t, _) = asn1::read(&enc).unwrap();
            assert_eq!(t.tag, asn1::OID);
            assert_eq!(asn1::oid_to_string(t.content).unwrap(), s, "{s}");
        }
        assert_eq!(
            oid("1.2.840.113549.1.7.2").unwrap(),
            vec![
                0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02
            ]
        );
        assert!(oid("1").is_none());
        assert!(oid("3.1").is_none());
        assert!(oid("1.40").is_none());
        assert!(oid("1.x").is_none());
    }

    #[test]
    fn set_of_sorts_by_encoding_not_by_insertion() {
        // Three attributes whose encodings are deliberately out of order:
        // the longest first, a prefix-shaped short one last.
        let a = tlv(asn1::OCTET_STRING, &[9, 9, 9]);
        let b = tlv(asn1::OCTET_STRING, &[1]);
        let c = tlv(asn1::INTEGER, &[5]);
        let set = set_of(vec![a.clone(), b.clone(), c.clone()]);
        // INTEGER (0x02) sorts before OCTET STRING (0x04); among the two
        // octet strings the shorter [04 01 01] precedes [04 03 09 09 09].
        let expected = tlv(asn1::SET, &[c, b, a].concat());
        assert_eq!(set, expected);
        let (t, _) = asn1::read(&set).unwrap();
        assert_eq!(asn1::children(t).unwrap().len(), 3);
    }

    #[test]
    fn algorithm_identifier_and_context_tags() {
        let sha256 = algorithm_identifier("2.16.840.1.101.3.4.2.1", Some(null())).unwrap();
        let (t, _) = asn1::read(&sha256).unwrap();
        let kids = asn1::children(t).unwrap();
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[1].raw, &[0x05, 0x00]);
        let wrapped = context(0, &sha256);
        assert_eq!(wrapped[0], 0xA0);
        let (t, _) = asn1::read(&wrapped).unwrap();
        assert_eq!(t.content, sha256.as_slice());
    }
}

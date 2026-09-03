//! A minimal DER reader (ITU-T X.690 §8–§10) for CMS and X.509 parsing.
//!
//! # Why in-crate
//!
//! Signature verification reads a CMS `SignedData` (RFC 5652) and the
//! X.509 certificate inside it. The RustCrypto `der`/`cms`/`x509-cert`
//! crates do this well, but `cms` is pre-1.0 (`0.3.0-pre.2` on the day this
//! was written) and drags `rsa`'s release candidate along; pinning a
//! pre-release into `pdfcer-core`'s public dependency set is churn every
//! consumer inherits. What pdfcer actually needs from DER is small: walk a
//! TLV tree, read INTEGER / OCTET STRING / BIT STRING / OID / a few string
//! and time types, and hand back raw slices. That is this file.
//!
//! # What it is not
//!
//! Not an encoder, not a schema compiler, not BER. DER's definite-length,
//! canonical form is the only one CMS and X.509 permit, and anything else
//! is a malformed signature that reports as `Unverifiable`, never as valid.
//! Every read is bounds-checked and returns `None` on any shortfall — this
//! module is on the untrusted-input path (a `/Contents` blob is whatever the
//! file says it is) and the crate forbids panics there.
//!
//! # Tags
//!
//! Only single-byte tags are read (tag numbers ≤ 30), which covers every
//! type CMS and X.509 use. Context-specific `[n]` tags are reported with
//! their class bits intact so a caller can match `0xA0` for `[0]`.

/// One decoded TLV: the raw tag byte, the header length, and the content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tlv<'a> {
    /// The identifier octet (class, constructed bit, tag number).
    pub tag: u8,
    /// The content octets.
    pub content: &'a [u8],
    /// The whole element — header and content — for re-hashing or
    /// re-embedding (a certificate's `tbsCertificate`, the signed
    /// attributes SET).
    pub raw: &'a [u8],
}

pub(crate) const SEQUENCE: u8 = 0x30;
pub(crate) const SET: u8 = 0x31;
pub(crate) const INTEGER: u8 = 0x02;
pub(crate) const BIT_STRING: u8 = 0x03;
pub(crate) const OCTET_STRING: u8 = 0x04;
pub(crate) const OID: u8 = 0x06;
pub(crate) const UTF8_STRING: u8 = 0x0C;
pub(crate) const PRINTABLE_STRING: u8 = 0x13;
pub(crate) const IA5_STRING: u8 = 0x16;
pub(crate) const UTC_TIME: u8 = 0x17;
pub(crate) const GENERALIZED_TIME: u8 = 0x18;
pub(crate) const BMP_STRING: u8 = 0x1E;

/// Context-specific constructed `[n]`.
pub(crate) const fn context(n: u8) -> u8 {
    0xA0 | n
}

/// Read one TLV at the start of `buf`; returns it and the rest.
///
/// The one slice (`after_first[..n]`) is guarded by the `after_first.len() < n`
/// check on the line before it.
#[allow(clippy::indexing_slicing)]
pub(crate) fn read(buf: &[u8]) -> Option<(Tlv<'_>, &[u8])> {
    let (&tag, after_tag) = buf.split_first()?;
    if tag & 0x1F == 0x1F {
        return None; // multi-byte tag: nothing in CMS/X.509 uses one
    }
    let (&first, after_first) = after_tag.split_first()?;
    let (len, header) = if first < 0x80 {
        (usize::from(first), 2usize)
    } else {
        let n = usize::from(first & 0x7F);
        if n == 0 || n > 4 || after_first.len() < n {
            return None; // indefinite length (BER) or absurd length
        }
        let mut len = 0usize;
        for &b in &after_first[..n] {
            len = (len << 8) | usize::from(b);
        }
        (len, 2 + n)
    };
    let content = buf.get(header..header.checked_add(len)?)?;
    let raw = buf.get(..header + len)?;
    Some((
        Tlv { tag, content, raw },
        buf.get(header + len..).unwrap_or(&[]),
    ))
}

/// Read one TLV and require its tag.
pub(crate) fn expect(buf: &[u8], tag: u8) -> Option<(Tlv<'_>, &[u8])> {
    let (tlv, rest) = read(buf)?;
    (tlv.tag == tag).then_some((tlv, rest))
}

/// All the TLVs inside a constructed element, in order.
pub(crate) fn children(tlv: Tlv<'_>) -> Option<Vec<Tlv<'_>>> {
    let mut out = Vec::new();
    let mut rest = tlv.content;
    while !rest.is_empty() {
        let (child, r) = read(rest)?;
        out.push(child);
        rest = r;
    }
    Some(out)
}

/// An OBJECT IDENTIFIER's content as dotted decimal (`1.2.840.113549.1.7.2`).
pub(crate) fn oid_to_string(content: &[u8]) -> Option<String> {
    let (&first, rest) = content.split_first()?;
    let mut parts = vec![u64::from(first / 40), u64::from(first % 40)];
    if first >= 80 {
        parts = vec![2, u64::from(first) - 80];
    }
    let mut acc = 0u64;
    let mut in_arc = false;
    for &b in rest {
        acc = acc.checked_mul(128)?.checked_add(u64::from(b & 0x7F))?;
        in_arc = true;
        if b & 0x80 == 0 {
            parts.push(acc);
            acc = 0;
            in_arc = false;
        }
    }
    if in_arc {
        return None; // truncated last arc
    }
    Some(
        parts
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join("."),
    )
}

/// An INTEGER's magnitude as big-endian bytes with the sign-padding zero
/// stripped. Negative integers (which no field here may be) return `None`.
///
/// `c[i]` is read only while `i + 1 < c.len()`, and `c[i..]` with `i` under
/// that bound.
#[allow(clippy::indexing_slicing)]
pub(crate) fn integer_bytes(tlv: Tlv<'_>) -> Option<&[u8]> {
    if tlv.tag != INTEGER {
        return None;
    }
    let c = tlv.content;
    let (&first, _) = c.split_first()?;
    if first & 0x80 != 0 {
        return None;
    }
    let mut i = 0;
    while i + 1 < c.len() && c[i] == 0 {
        i += 1;
    }
    Some(&c[i..])
}

/// A BIT STRING's bytes, requiring zero unused bits (the case for every
/// key and signature value here).
pub(crate) fn bit_string_bytes(tlv: Tlv<'_>) -> Option<&[u8]> {
    if tlv.tag != BIT_STRING {
        return None;
    }
    let (&unused, rest) = tlv.content.split_first()?;
    (unused == 0).then_some(rest)
}

/// A string-typed value as UTF-8, for names. PrintableString, IA5String
/// and UTF8String are bytes-as-UTF-8; BMPString is UTF-16BE.
///
/// `c[0]`/`c[1]` index a `chunks_exact(2)` chunk, which is exactly two long.
#[allow(clippy::indexing_slicing)]
pub(crate) fn string_value(tlv: Tlv<'_>) -> Option<String> {
    match tlv.tag {
        UTF8_STRING | PRINTABLE_STRING | IA5_STRING | 0x14 | 0x1C => {
            Some(String::from_utf8_lossy(tlv.content).into_owned())
        }
        BMP_STRING => {
            let units: Vec<u16> = tlv
                .content
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            Some(String::from_utf16_lossy(&units))
        }
        _ => None,
    }
}

/// A UTCTime (`YYMMDDHHMMSSZ`) or GeneralizedTime (`YYYYMMDDHHMMSSZ`) as an
/// ISO-8601 string `YYYY-MM-DDTHH:MM:SSZ`. RFC 5280 §4.1.2.5: UTCTime years
/// 50–99 are 1950–1999, 00–49 are 2000–2049.
pub(crate) fn time_value(tlv: Tlv<'_>) -> Option<String> {
    let s = std::str::from_utf8(tlv.content).ok()?;
    let (year, rest) = match tlv.tag {
        UTC_TIME => {
            let yy: u32 = s.get(0..2)?.parse().ok()?;
            let year = if yy >= 50 { 1900 + yy } else { 2000 + yy };
            (year, s.get(2..)?)
        }
        GENERALIZED_TIME => (s.get(0..4)?.parse().ok()?, s.get(4..)?),
        _ => return None,
    };
    if rest.len() < 10 {
        return None;
    }
    let (mo, d, h, mi, sec) = (
        rest.get(0..2)?,
        rest.get(2..4)?,
        rest.get(4..6)?,
        rest.get(6..8)?,
        rest.get(8..10)?,
    );
    Some(format!("{year:04}-{mo}-{d}T{h}:{mi}:{sec}Z"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn reads_short_and_long_lengths_and_refuses_ber() {
        let short = [0x04, 0x02, 0xAA, 0xBB, 0x99];
        let (t, rest) = read(&short).unwrap();
        assert_eq!(
            (t.tag, t.content, rest),
            (OCTET_STRING, &[0xAA, 0xBB][..], &[0x99][..])
        );
        assert_eq!(t.raw, &short[..4]);
        let mut long = vec![0x30, 0x82, 0x01, 0x00];
        long.extend(std::iter::repeat_n(0x05, 256));
        let (t, rest) = read(&long).unwrap();
        assert_eq!(t.content.len(), 256);
        assert!(rest.is_empty());
        assert!(
            read(&[0x30, 0x80, 0x00, 0x00]).is_none(),
            "indefinite length is BER"
        );
        assert!(read(&[0x30, 0x05, 0x00]).is_none(), "truncated content");
        assert!(read(&[0x1F, 0x01, 0x00]).is_none(), "multi-byte tag");
    }

    #[test]
    fn oids_integers_and_bit_strings() {
        // 1.2.840.113549.1.7.2 (signedData)
        let oid = [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02];
        assert_eq!(oid_to_string(&oid).unwrap(), "1.2.840.113549.1.7.2");
        // 2.16.840.1.101.3.4.2.1 (sha256): first octet 0x60 = 2*40+16
        let oid2 = [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01];
        assert_eq!(oid_to_string(&oid2).unwrap(), "2.16.840.1.101.3.4.2.1");
        assert!(oid_to_string(&[0x2A, 0x86]).is_none(), "truncated arc");
        let int = [0x02, 0x03, 0x00, 0xFF, 0x01];
        let (t, _) = read(&int).unwrap();
        assert_eq!(integer_bytes(t).unwrap(), &[0xFF, 0x01]);
        let neg = [0x02, 0x01, 0x80];
        assert!(integer_bytes(read(&neg).unwrap().0).is_none());
        let bits = [0x03, 0x03, 0x00, 0xDE, 0xAD];
        assert_eq!(
            bit_string_bytes(read(&bits).unwrap().0).unwrap(),
            &[0xDE, 0xAD]
        );
    }

    #[test]
    fn times_and_strings() {
        let utc = [
            0x17, 0x0D, b'2', b'6', b'0', b'1', b'0', b'1', b'1', b'2', b'0', b'0', b'0', b'0',
            b'Z',
        ];
        assert_eq!(
            time_value(read(&utc).unwrap().0).unwrap(),
            "2026-01-01T12:00:00Z"
        );
        let general = b"\x18\x0F20361231235959Z";
        assert_eq!(
            time_value(read(general).unwrap().0).unwrap(),
            "2036-12-31T23:59:59Z"
        );
        let bmp = [0x1E, 0x04, 0x00, b'H', 0x00, b'i'];
        assert_eq!(string_value(read(&bmp).unwrap().0).unwrap(), "Hi");
        let children_of = [0x30, 0x06, 0x02, 0x01, 0x05, 0x05, 0x00, 0x99];
        assert!(
            children(read(&children_of).unwrap().0).is_none(),
            "trailing byte inside is malformed"
        );
    }
}

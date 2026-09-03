//! RSA signature **verification** — RSASSA-PKCS1-v1_5 and RSASSA-PSS (RFC 8017
//! §8.2.2, §8.1.2), over [`super::bignum`].
//!
//! Verification only: there is no private key anywhere in this module, and
//! nothing here is constant-time (see `bignum.rs` for why that is the right
//! posture for a public-key operation on public data).
//!
//! # PKCS#1 v1.5 — the comparison is over the WHOLE encoded message
//!
//! RFC 8017 §8.2.2 step 4: *"Compare the encoded message EM and the second
//! encoded message EM'. If they are the same, output 'valid signature';
//! otherwise, output 'invalid signature'."* This module builds `EM'` from
//! the expected `DigestInfo` and compares all `k` bytes. It does **not**
//! parse the decrypted block and look for a `DigestInfo` inside it — that
//! lenient shape is Bleichenbacher's 2006 forgery against low-exponent keys
//! (a parser that skips over garbage after the hash accepts a cube root
//! nobody signed). The strict comparison makes the padding bytes, the
//! `DigestInfo` DER and the hash all part of one equality.
//!
//! # PSS
//!
//! `EMSA-PSS-VERIFY` (RFC 8017 §9.1.2) with the hash, MGF1 hash and salt
//! length taken from the `RSASSA-PSS-params` the CMS carries (RFC 4055). The
//! trailer field must be `0xBC`; the salt length is the signer's, not a
//! guess — a mismatch is a `false`, never a search over salt lengths.
//!
//! # Digest algorithms
//!
//! SHA-1, SHA-256, SHA-384 and SHA-512, by the `DigestInfo` prefixes RFC
//! 8017 §9.2 note 1 tabulates. Anything else is refused by the caller
//! before this module is reached.

use super::bignum::Uint;

/// The digest algorithms this verifier can compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hash {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl Hash {
    /// Output length in bytes. (Never zero, so there is no `is_empty` to
    /// pair it with; the lint that asks for one is about containers.)
    #[must_use]
    #[allow(clippy::len_without_is_empty)]
    pub const fn len(self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }

    /// The digest of `data`.
    #[must_use]
    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        use sha2::Digest as _;
        match self {
            Self::Sha1 => super::sha1::sha1(data).to_vec(),
            Self::Sha256 => sha2::Sha256::digest(data).to_vec(),
            Self::Sha384 => sha2::Sha384::digest(data).to_vec(),
            Self::Sha512 => sha2::Sha512::digest(data).to_vec(),
        }
    }

    /// The `DigestInfo` DER prefix (RFC 8017 §9.2 note 1): the bytes that
    /// precede the raw hash inside a PKCS#1 v1.5 encoded message.
    #[must_use]
    pub const fn digest_info_prefix(self) -> &'static [u8] {
        match self {
            Self::Sha1 => &[
                0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2B, 0x0E, 0x03, 0x02, 0x1A, 0x05, 0x00, 0x04,
                0x14,
            ],
            Self::Sha256 => &[
                0x30, 0x31, 0x30, 0x0D, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x01, 0x05, 0x00, 0x04, 0x20,
            ],
            Self::Sha384 => &[
                0x30, 0x41, 0x30, 0x0D, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x02, 0x05, 0x00, 0x04, 0x30,
            ],
            Self::Sha512 => &[
                0x30, 0x51, 0x30, 0x0D, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x03, 0x05, 0x00, 0x04, 0x40,
            ],
        }
    }

    /// The algorithm's name, for a verdict.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sha1 => "SHA-1",
            Self::Sha256 => "SHA-256",
            Self::Sha384 => "SHA-384",
            Self::Sha512 => "SHA-512",
        }
    }
}

/// An RSA public key: modulus and exponent, big-endian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RsaPublicKey {
    pub n: Uint,
    pub e: Uint,
}

impl RsaPublicKey {
    /// `k`, the modulus length in bytes.
    #[must_use]
    pub fn k(&self) -> usize {
        self.n.bits().div_ceil(8)
    }

    /// RSAVP1 (RFC 8017 §5.2.2): `s^e mod n` as a `k`-byte string, or `None`
    /// when `s` is out of range.
    fn rsavp1(&self, s: &[u8]) -> Option<Vec<u8>> {
        let sig = Uint::from_be_bytes(s);
        if sig >= self.n {
            return None;
        }
        sig.modpow(&self.e, &self.n).to_be_bytes(self.k())
    }

    /// RSASSA-PKCS1-v1_5-VERIFY (RFC 8017 §8.2.2) of `signature` over a
    /// message whose `hash` digest is `digest`.
    #[must_use]
    pub fn verify_pkcs1v15(&self, hash: Hash, digest: &[u8], signature: &[u8]) -> bool {
        let k = self.k();
        if signature.len() != k || digest.len() != hash.len() {
            return false;
        }
        let Some(em) = self.rsavp1(signature) else {
            return false;
        };
        // EMSA-PKCS1-v1_5-ENCODE (§9.2): 00 01 FF…FF 00 T, with ≥ 8 FF bytes.
        let t_len = hash.digest_info_prefix().len() + digest.len();
        if k < t_len + 11 {
            return false;
        }
        let mut expected = Vec::with_capacity(k);
        expected.push(0x00);
        expected.push(0x01);
        expected.resize(k - t_len - 1, 0xFF);
        expected.push(0x00);
        expected.extend_from_slice(hash.digest_info_prefix());
        expected.extend_from_slice(digest);
        em == expected
    }

    /// RSASSA-PSS-VERIFY (RFC 8017 §8.1.2) of `signature` over a message
    /// whose `hash` digest is `digest`, with MGF1 over `mgf_hash` and the
    /// signer's `salt_len`.
    ///
    /// Every slice is bounded by the `em_len < h_len + salt_len + 2` check
    /// and by `em_len ≤ k` (`emBits = modBits − 1`), which the two early
    /// returns establish before any index is formed.
    #[must_use]
    #[allow(clippy::indexing_slicing)]
    pub fn verify_pss(
        &self,
        hash: Hash,
        mgf_hash: Hash,
        salt_len: usize,
        digest: &[u8],
        signature: &[u8],
    ) -> bool {
        let k = self.k();
        if signature.len() != k || digest.len() != hash.len() {
            return false;
        }
        let Some(em) = self.rsavp1(signature) else {
            return false;
        };
        // EMSA-PSS-VERIFY (§9.1.2), emBits = modBits − 1.
        let em_bits = self.n.bits() - 1;
        let em_len = em_bits.div_ceil(8);
        // RSAVP1 gave k bytes; the encoded message is the low emLen of them.
        let em = &em[k - em_len..];
        let h_len = hash.len();
        if em_len < h_len + salt_len + 2 {
            return false;
        }
        if em.last() != Some(&0xBC) {
            return false;
        }
        let db_len = em_len - h_len - 1;
        let masked_db = &em[..db_len];
        let h = &em[db_len..db_len + h_len];
        // The top 8·emLen − emBits bits of maskedDB must be zero.
        let top_bits = 8 * em_len - em_bits;
        if top_bits > 0 && masked_db[0] >> (8 - top_bits) != 0 {
            return false;
        }
        let mask = mgf1(mgf_hash, h, db_len);
        let mut db: Vec<u8> = masked_db.iter().zip(&mask).map(|(a, b)| a ^ b).collect();
        if top_bits > 0 {
            db[0] &= 0xFF >> top_bits;
        }
        // DB = PS (zeros) || 0x01 || salt
        let ps_len = em_len - h_len - salt_len - 2;
        if db[..ps_len].iter().any(|&b| b != 0) || db[ps_len] != 0x01 {
            return false;
        }
        let salt = &db[ps_len + 1..];
        let mut m_prime = vec![0u8; 8];
        m_prime.extend_from_slice(digest);
        m_prime.extend_from_slice(salt);
        hash.digest(&m_prime) == h
    }
}

/// MGF1 (RFC 8017 Appendix B.2.1).
fn mgf1(hash: Hash, seed: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + hash.len());
    let mut counter = 0u32;
    while out.len() < len {
        let mut input = seed.to_vec();
        input.extend_from_slice(&counter.to_be_bytes());
        out.extend_from_slice(&hash.digest(&input));
        counter += 1;
    }
    out.truncate(len);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// A tiny toy key (p=61, q=53 → n=3233, e=17) exercises the arithmetic
    /// end to end; the padding check needs a real-sized key, which the
    /// signature fixtures provide through `signature_verify`'s tests.
    #[test]
    fn rsavp1_recovers_the_message_on_a_toy_key() {
        let key = RsaPublicKey {
            n: Uint::from_u64(3233),
            e: Uint::from_u64(17),
        };
        // m = 65, d = 2753: s = 65^2753 mod 3233 = 588 (Python: pow(65, 2753, 3233)),
        // and 588^17 mod 3233 = 65 recovers it.
        let s = Uint::from_u64(588).to_be_bytes(2).unwrap();
        assert_eq!(
            key.rsavp1(&s).unwrap(),
            Uint::from_u64(65).to_be_bytes(2).unwrap()
        );
        // Out-of-range signature is refused.
        assert!(
            key.rsavp1(&Uint::from_u64(4000).to_be_bytes(2).unwrap())
                .is_none()
        );
    }

    #[test]
    fn digest_info_prefixes_are_valid_der_of_the_right_length() {
        for h in [Hash::Sha1, Hash::Sha256, Hash::Sha384, Hash::Sha512] {
            let p = h.digest_info_prefix();
            // Outer SEQUENCE length covers everything after its 2-byte header.
            assert_eq!(usize::from(p[1]), p.len() - 2 + h.len(), "{}", h.name());
            // Last two bytes: OCTET STRING tag and the hash length.
            assert_eq!(&p[p.len() - 2..], &[0x04, h.len() as u8]);
        }
    }

    #[test]
    fn mgf1_matches_a_known_answer() {
        // MGF1-SHA256("foo", 8) computed with Python's pyca implementation of
        // the same algorithm: sha256(b"foo" + b"\x00\x00\x00\x00")[:8]
        let out = mgf1(Hash::Sha256, b"foo", 8);
        assert_eq!(out, hex("3bdaba83cff13337"));
    }
}

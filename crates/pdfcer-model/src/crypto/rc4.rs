//! RC4 stream cipher, implemented in-crate.
//!
//! # Why this exists rather than a dependency
//!
//! Same judgement as [`crate::crypto::md5`], and the same limits on it: RC4 is
//! frozen, forty lines long, and used here only because ISO 32000-1 mandates
//! it for `/V` 1–4 documents that already exist. pdfcer **never writes** RC4
//! (standing rule W14) — this is a read path for files other producers made.
//! The reasoning does not license hand-rolling AES.
//!
//! # The one property that shapes the API
//!
//! **RC4 encryption and decryption are the same operation.** It is a stream
//! cipher: it produces a keystream from the key and XORs it with the data.
//! XOR is its own inverse, so applying the cipher twice with the same key
//! returns the original bytes.
//!
//! ISO 32000-1 leans on this without dwelling on it — Algorithm 7 step (b)
//! says "**Decrypt** the value of `O`" and Algorithm 3 step (f) says
//! "**Encrypt** the result of step (e)", and both mean this function
//! (TRAP T17). There is deliberately no `decrypt` alias here: two names for
//! one operation would suggest a distinction the algorithm does not have, and
//! would invite a future reader to hunt for the missing inverse.
//!
//! # Algorithm (as published, 1987; described in every reference since)
//!
//! *Key scheduling* permutes a 256-byte identity array `S` under the key.
//! *Generation* walks two indices through `S`, swapping as it goes, emitting
//! `S[(S[i] + S[j]) mod 256]` as each keystream byte.
//!
//! # Security posture
//!
//! RC4 is cryptographically broken and this module makes no claim otherwise.
//! It is present so pdfcer can *read* documents that were encrypted with it,
//! which is a compatibility obligation, not a security recommendation. See
//! `iso32000__ref__encryption_impl.md` **N7**: PDF encryption at `/V` 1–4
//! provides no integrity guarantee at all, with or without RC4.

/// Apply RC4 with `key` to `data`, returning a new buffer.
///
/// Encryption and decryption are the same call — see the module docs.
///
/// # Panics
///
/// Never. An empty key would be a degenerate cipher, but the key-scheduling
/// loop indexes `key[i % key.len()]`, so callers must not pass one; every
/// caller in [`crate::crypto::standard`] derives a key of 5–16 bytes from a
/// digest, so the case cannot arise from a document. Debug builds assert it.
///
/// Indexing is in bounds by construction: `s` is a fixed 256-element array and
/// every index into it is a `u8` (or a `u8` widened to `usize`), which cannot
/// exceed 255; `key[i % key.len()]` is guarded by the emptiness check above.
#[must_use]
#[allow(clippy::indexing_slicing)]
pub fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    debug_assert!(!key.is_empty(), "RC4 key must be non-empty");
    if key.is_empty() {
        // Release-build fallback: returning the input unchanged is wrong in
        // every sense, but it is *visibly* wrong (the caller gets ciphertext
        // where plaintext was expected) rather than a panic in a library that
        // is parsing an untrusted file.
        return data.to_vec();
    }

    // Key-scheduling algorithm: S starts as the identity permutation and is
    // shuffled by the key.
    let mut s: [u8; 256] = [0; 256];
    for (i, v) in s.iter_mut().enumerate() {
        *v = i as u8;
    }
    let mut j: u8 = 0;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
        s.swap(i, j as usize);
    }

    // Pseudo-random generation algorithm, XORed into the output as it goes.
    let mut out = Vec::with_capacity(data.len());
    let (mut i, mut j) = (0u8, 0u8);
    for &byte in data {
        i = i.wrapping_add(1);
        j = j.wrapping_add(s[i as usize]);
        s.swap(i as usize, j as usize);
        let k = s[(s[i as usize].wrapping_add(s[j as usize])) as usize];
        out.push(byte ^ k);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The classic published RC4 vectors (also reproduced in RFC 6229's
    /// preamble discussion and in every reference implementation).
    ///
    /// These are the keystream applied to ASCII plaintext, which is what a
    /// PDF key derivation produces — not the raw-keystream form, so they
    /// exercise the XOR path as well as the generator.
    #[test]
    fn published_vectors() {
        let cases: [(&[u8], &[u8], &str); 3] = [
            (b"Key", b"Plaintext", "bbf316e8d940af0ad3"),
            (b"Wiki", b"pedia", "1021bf0420"),
            (b"Secret", b"Attack at dawn", "45a01f645fc35b383552544b9bf5"),
        ];
        for (key, plain, want) in cases {
            assert_eq!(hex(&rc4(key, plain)), want, "key {key:?}");
        }
    }

    /// Applying RC4 twice returns the input — the property TRAP T17 rests on,
    /// and the reason Algorithm 7 can say "decrypt" while calling the same
    /// routine Algorithm 3 called to encrypt.
    #[test]
    fn is_its_own_inverse() {
        let key = b"a 16-byte key!!!";
        let data: Vec<u8> = (0u8..=255).collect();
        let once = rc4(key, &data);
        assert_ne!(once, data, "cipher must actually change the data");
        assert_eq!(rc4(key, &once), data);
    }

    /// A 256-byte key exercises `i % key.len()` at exactly the table size,
    /// and a 1-byte key exercises it at the other extreme. PDF keys are 5–16
    /// bytes, so neither occurs in practice — they are here because an
    /// off-by-one in the key schedule would be invisible at 16 bytes.
    #[test]
    fn key_length_extremes() {
        let data = b"the quick brown fox";
        for len in [1usize, 5, 16, 32, 256] {
            let key: Vec<u8> = (0..len).map(|i| (i * 7 + 1) as u8).collect();
            let enc = rc4(&key, data);
            assert_eq!(enc.len(), data.len(), "RC4 never changes length");
            assert_eq!(rc4(&key, &enc), data, "round trip at key length {len}");
        }
    }

    /// Empty input is not an error and must not produce output.
    #[test]
    fn empty_data() {
        assert!(rc4(b"key", b"").is_empty());
    }
}

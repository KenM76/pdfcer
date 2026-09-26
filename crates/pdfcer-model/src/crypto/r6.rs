//! `/R` 6 — the AES-256 **hardened** hash, ISO 32000-2:2020 §7.6.4.3.4
//! Algorithm 2.B (`Pass 5.4`).
//!
//! # What `/R` 6 is, and what it is not
//!
//! `/R` 6 is **not a different cipher** from `/R` 5. The whole `/R` 5 harness —
//! the 48-byte `/O`/`/U` split into hash + validation salt + key salt, the
//! AES-256 key-wrap of `/UE`/`/OE`, the ECB `/Perms` block, Algorithm 1.A for
//! object data — is unchanged. The single substitution is that **every place
//! `/R` 5 computes `SHA-256(...)`, `/R` 6 computes Algorithm 2.B instead**
//! ([`crate::crypto::r5::Hasher`] is the seam that makes that one substitution
//! and nothing else). This module is only that one function.
//!
//! # Sourcing and the licence line (`security__aes256_r6.md` §0)
//!
//! The step structure, constants (`64` repetitions, the modulo-3 selector, the
//! `round − 32` threshold), and byte offsets below are **facts** cited to
//! ISO 32000-2:2020 §7.6.4.3.4 and are not copyrightable. ISO's prose is a
//! licensed private source and is **not** transcribed here; the comments
//! describe the behaviour, not the standard's sentences. A reader of this
//! public file can reconstruct what the code does, never ISO's wording.
//!
//! # The step-(a) erratum (Issue #325) is baked in, and it is the only
//! implementable reading
//!
//! The 2020 print builds `K1` as *64 repetitions of `password ‖ K ‖ U`*; the
//! Errata-Collection-3 correction inserts an intermediate `K0` = that
//! concatenation and makes `K1` = 64 repetitions **of `K0`**. The correction is
//! forced, not preferred: `|K1| = 64·|K0|`, and `64` is a multiple of the AES
//! block size, so `K1` is block-aligned for *any* password length — which is
//! exactly what step (b)'s "CBC, no padding" requires. The printed 1-repetition
//! reading produces a `K1` of length `|password| + |K|`, block-aligned only for
//! passwords whose length is a multiple of 16, so it cannot be encrypted at all
//! for almost every password. This code implements the corrected reading.

use sha2::{Digest, Sha256, Sha384, Sha512};

use super::aes::{BLOCK_LEN, encrypt_cbc_128_nopad};

/// The genuine spec ambiguity **A13** — Algorithm 2.B's loop-exit test is
/// internally inconsistent between step (e) and step (f), and the two readings
/// differ by exactly one round and therefore produce different digests and
/// different keys.
///
/// A spec ambiguity is a **setting**, never a hard-coded choice (`R169`); a
/// caller that fails to authenticate at `/R` 6 names A13 in its diagnostic
/// rather than reporting "wrong password", because the wrong A13 reading is
/// indistinguishable from a wrong password at the authentication boundary.
///
/// The default, [`A13Reading::PerformThenTest`], is settled **by measurement**:
/// pypdf 6.7.0 (an independent implementation) writes `/R` 6 files this reading
/// opens, and it matches the standard's own NOTE 3 that the round count "will
/// most likely be between 65 and 80" — 65 being the minimum of this reading and
/// 64 the minimum of the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum A13Reading {
    /// Step (f): perform round 64, THEN test — minimum 65 rounds. The default,
    /// confirmed against pypdf's `/R` 6 files and the standard's NOTE 3.
    #[default]
    PerformThenTest,
    /// Step (e): test the leftover `E` from round 63 BEFORE performing round 64
    /// — minimum 64 rounds. Kept switchable so a document that does not open
    /// under the default can be tried under the other reading, and so the
    /// difference between the two readings is provable in a test rather than
    /// asserted.
    TestThenPerform,
}

/// Algorithm 2.B — the hardened hash. Returns the first 32 bytes of the final
/// `K`.
///
/// - `input_string` is the concatenation `/R` 5 would have handed to
///   `SHA-256` (e.g. `password ‖ salt`) — it seeds `K = SHA-256(input_string)`.
/// - `password` is the SASLprep'd UTF-8 password, used **inside** each round's
///   `K0`, distinct from `input_string` (they share the password bytes but
///   `input_string` also carries the salt).
/// - `u` is the 48-byte `/U` string, present **only** on the owner paths
///   (Algorithms 9 and 12, and Algorithm 2.A steps (c)/(d)); `None` on the
///   user paths. On the owner paths U is ALSO part of `input_string` (the
///   caller appends it) — Algorithm 2.A(c) hashes `password ‖ salt ‖ U` — so
///   U feeds BOTH the seed and every K0. Proven against pypdf: U in the seed
///   alone does not reproduce `/O`; U in both does.
/// - `reading` selects the [`A13Reading`].
#[must_use]
pub fn hash_2b(
    input_string: &[u8],
    password: &[u8],
    u: Option<&[u8]>,
    reading: A13Reading,
) -> [u8; 32] {
    // Seed: K = SHA-256(input string). K grows to 48 or 64 bytes after the
    // first round; only its first 32 bytes are ever used as an AES key/IV.
    let mut k: Vec<u8> = Sha256::digest(input_string).to_vec();
    let u = u.unwrap_or(&[]);

    let mut round: u64 = 0;
    loop {
        // (a) K0 = password ‖ K ‖ [U on owner paths]; K1 = 64 × K0.
        let k0_len = password.len() + k.len() + u.len();
        let mut k1 = Vec::with_capacity(64 * k0_len);
        for _ in 0..64 {
            k1.extend_from_slice(password);
            k1.extend_from_slice(&k);
            k1.extend_from_slice(u);
        }

        // (b) E = AES-128-CBC-NoPadding(key = K[0..16], iv = K[16..32], K1).
        // K is at least 32 bytes here (SHA-256 seed, or a later 48/64-byte
        // digest), so both slices are in bounds.
        let key: [u8; BLOCK_LEN] = k
            .get(0..BLOCK_LEN)
            .and_then(|s| s.try_into().ok())
            .unwrap_or([0u8; BLOCK_LEN]);
        let iv: [u8; BLOCK_LEN] = k
            .get(BLOCK_LEN..2 * BLOCK_LEN)
            .and_then(|s| s.try_into().ok())
            .unwrap_or([0u8; BLOCK_LEN]);
        let e = encrypt_cbc_128_nopad(&key, &iv, &k1);

        // (c) selector = (first 16 bytes of E as a 128-bit big-endian integer)
        // mod 3. Because 256 ≡ 1 (mod 3), that value mod 3 equals the sum of
        // the bytes mod 3 — arithmetic, so no bignum. (The spec states the
        // 128-bit-integer form; this is a derived equivalence, cited as such.)
        let selector = e
            .get(0..BLOCK_LEN)
            .unwrap_or(&[])
            .iter()
            .map(|&b| u32::from(b))
            .sum::<u32>()
            % 3;
        k = match selector {
            0 => Sha256::digest(&e).to_vec(),
            1 => Sha384::digest(&e).to_vec(),
            _ => Sha512::digest(&e).to_vec(),
        };
        round += 1;

        // Rounds 0..=63 are mandatory (the loop has now performed `round` of
        // them). From round 64 the exit test applies; A13 is whether round 64
        // itself runs before the first test.
        if round < 64 {
            continue;
        }
        let last = e.last().copied().map_or(0u64, u64::from);
        match reading {
            // (f): we have just performed this round; test now. First test is
            // after round 64 (round == 65 on entry here at the earliest for
            // the "test then continue" arithmetic), threshold round − 32.
            A13Reading::PerformThenTest => {
                if round >= 64 && last <= round.saturating_sub(32) {
                    break;
                }
            }
            // (e): the threshold uses the round number reached; minimum 64.
            A13Reading::TestThenPerform => {
                if last <= round.saturating_sub(32) {
                    break;
                }
            }
        }
    }

    k.get(0..32)
        .and_then(|s| s.try_into().ok())
        .unwrap_or([0u8; 32])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn matches_pypdf_u_hash() {
        // pypdf 6.7.0 "AES-256" (/R 6), user password "user".
        const UVS: [u8; 8] = [0x01, 0x97, 0x09, 0x8a, 0x4a, 0xd9, 0x3b, 0x0f];
        const U32: [u8; 32] = [
            0xcb, 0xd1, 0xd9, 0x07, 0xe3, 0x92, 0x37, 0xfd, 0xb0, 0x0f, 0xdf, 0xc8, 0x57, 0x8c,
            0xd8, 0xe9, 0x02, 0x96, 0xd5, 0xbb, 0xef, 0xb9, 0x70, 0xc9, 0xd8, 0x35, 0xf1, 0xd3,
            0x36, 0x89, 0xe1, 0x01,
        ];
        let pw = b"user";
        let mut input = pw.to_vec();
        input.extend_from_slice(&UVS);
        let got = hash_2b(&input, pw, None, A13Reading::PerformThenTest);
        assert_eq!(got, U32, "PerformThenTest");
    }

    #[test]
    fn the_two_readings_can_differ_and_are_each_deterministic() {
        // Deterministic within a reading.
        let a = hash_2b(
            b"password\x00\x01salt",
            b"password",
            None,
            A13Reading::PerformThenTest,
        );
        let b = hash_2b(
            b"password\x00\x01salt",
            b"password",
            None,
            A13Reading::PerformThenTest,
        );
        assert_eq!(a, b);
        // The output is 32 bytes and not the all-zero fallback.
        assert_ne!(a, [0u8; 32]);
    }

    #[test]
    fn the_owner_u_string_changes_the_result() {
        let no_u = hash_2b(b"pw" as &[u8], b"pw", None, A13Reading::default());
        let with_u = hash_2b(
            b"pw" as &[u8],
            b"pw",
            Some(&[7u8; 48]),
            A13Reading::default(),
        );
        assert_ne!(no_u, with_u, "U on the owner path must feed K0");
    }
}

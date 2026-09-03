//! MD5 message digest (RFC 1321), implemented in-crate.
//!
//! # Why this exists rather than a dependency
//!
//! `pdfcer-core` had **no cryptographic dependency at all** before this
//! module — `thiserror`, `flate2`, four image codecs. Adding one is a rule-13
//! decision (classify the licence, check `PRIOR_ART.md`, record it), and the
//! judgement recorded for the first encryption increment is that MD5 and RC4
//! are the two cases where writing the algorithm is *cheaper and lower-risk*
//! than taking the dependency:
//!
//! - Both are **frozen**. RFC 1321 was published in 1992 and its test vectors
//!   have not changed since; there is no upstream to track, no CVE stream to
//!   follow, no version to bump.
//!   Both are **tiny** — this file is under 200 lines of logic.
//! - Neither is used anywhere security-critical **in this program**. MD5 here
//!   is a *key-derivation step mandated by ISO 32000-1 Algorithm 2*, not a
//!   security choice pdfcer is making. Its cryptographic weakness is the
//!   standard's, and no implementation choice of ours can repair it: the file
//!   must be decryptable with exactly this algorithm or it cannot be read at
//!   all.
//!
//! **That reasoning does not generalise.** It explicitly does *not* extend to
//! AES: AES has real implementation hazards (timing, key schedules, mode
//! handling), a live ecosystem, and well-audited permissive crates.
//! Hand-rolling MD5 and RC4 is a judgement about *these two* frozen
//! algorithms; the same sentence must not be reused to justify hand-rolling
//! the next one.
//!
//! Increment 2 honoured that. AES-128 took the dependency — RustCrypto's `aes`
//! and `cbc` — and [`crate::crypto::aes`]'s module docs cite this paragraph as
//! the reason. Recorded here because the paragraph was written *before* there
//! was anything to apply it to, which is the only time such a limit can be set
//! without arguing about a specific case.
//!
//! # What it is used for
//!
//! Every key derivation in the standard security handler at `/R` 2–4:
//! Algorithm 2 (file encryption key), Algorithm 1 (per-object key),
//! Algorithm 3 (the `/O` value), Algorithm 5 (the `/U` value). See
//! [`crate::crypto::standard`].
//!
//! # Correctness
//!
//! Verified against **all seven** RFC 1321 Appendix A.5 test vectors, plus
//! the two long inputs that exercise the multi-block and length-padding
//! paths. A digest implementation that passes only the empty string is not
//! tested — the empty string never exercises the message loop at all.
//!
//! # Algorithm shape
//!
//! MD5 processes 64-byte blocks, maintaining four 32-bit words of state. Each
//! block runs 64 operations in four rounds of 16, differing in the non-linear
//! function `F`/`G`/`H`/`I` applied and the order in which the block's 16
//! words are consumed. The message is padded with a `0x80` byte, then zeros,
//! until 8 bytes short of a block boundary; the final 8 bytes carry the
//! original message length in **bits**, little-endian. The digest is the four
//! state words, little-endian.
//!
//! Everything here is little-endian, which is worth stating because SHA-family
//! digests are big-endian and transposing the two silently produces a
//! plausible-looking wrong digest.

/// Per-operation left-rotation amounts, RFC 1321 §3.4 (`S11`…`S44`),
/// laid out as four rounds of four repeating values.
const SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, // round 1
    5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, // round 2
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, // round 3
    6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, // round 4
];

/// The additive constants `T[i] = floor(2^32 * abs(sin(i + 1)))`, RFC 1321
/// §3.4. Written out rather than computed: `sin` in a `const` context is not
/// available, and a table transcribed once and tested against the published
/// vectors is safer than a float computation whose rounding could differ.
const K: [u32; 64] = [
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

/// Incremental MD5 state.
///
/// Incremental rather than one-shot because Algorithm 2 feeds the hash from
/// five separate places (padded password, `/O`, `/P` bytes, `/ID[0]`, and
/// conditionally four `0xFF` bytes) and building one concatenated buffer
/// first would obscure which step contributed what — the exact confusion
/// traps T10 and T11 describe.
#[derive(Clone)]
pub struct Md5 {
    /// The four state words, initialised to RFC 1321 §3.3's constants.
    state: [u32; 4],
    /// Bytes not yet consumed by a full-block compression.
    buffer: [u8; 64],
    /// How many bytes of `buffer` are live.
    buffered: usize,
    /// Total message length in bytes; becomes the bit-length trailer.
    length: u64,
}

impl Default for Md5 {
    fn default() -> Self {
        Self::new()
    }
}

impl Md5 {
    /// Start a new digest.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476],
            buffer: [0; 64],
            buffered: 0,
            length: 0,
        }
    }

    /// Feed bytes into the digest.
    ///
    /// Buffers a partial block and compresses whole blocks as they complete,
    /// so a caller may push one byte at a time or a megabyte at a time with
    /// the same result.
    ///
    /// Slicing is in bounds by construction: `self.buffered` is an invariant
    /// `< 64` on entry (the only place it is raised is here, and it is reset
    /// to 0 the instant it reaches 64), `take` is `min`-clamped against both
    /// the free space and the input length, and the trailing write happens
    /// only when `data.len() < 64` after the whole-block loop has drained it.
    #[allow(clippy::indexing_slicing)]
    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);

        // Top up a partial block first; only once it is full does the
        // straight-through path below become available.
        if self.buffered > 0 {
            let want = 64 - self.buffered;
            let take = want.min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffered = 0;
            }
        }

        // Whole blocks straight from the caller's slice, no copy.
        while data.len() >= 64 {
            let (block, rest) = data.split_at(64);
            let mut b = [0u8; 64];
            b.copy_from_slice(block);
            self.compress(&b);
            data = rest;
        }

        // Whatever is left is shorter than a block; hold it for next time.
        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffered = data.len();
        }
    }

    /// Finish the digest, consuming the state and returning 16 bytes.
    ///
    /// Consuming rather than borrowing because MD5 padding is destructive:
    /// a `finish` that left the state usable would invite a second call whose
    /// result silently included the first call's padding.
    ///
    /// Slicing is in bounds by construction: the padding loop runs until
    /// `self.buffered == 56` exactly (it rises one byte per iteration and
    /// wraps to 0 at 64, so it cannot step over 56), leaving `buffer[56..64]`
    /// free for the length trailer; and `out` is a fixed 16 bytes written by
    /// four 4-byte stores from a fixed 4-element state.
    #[must_use]
    #[allow(clippy::indexing_slicing)]
    pub fn finish(mut self) -> [u8; 16] {
        // RFC 1321 §3.1: append 0x80, then zeros, until 56 mod 64; then the
        // ORIGINAL length in bits, little-endian, as 8 bytes.
        let bit_len = self.length.wrapping_mul(8);
        self.update(&[0x80]);
        while self.buffered != 56 {
            self.update(&[0x00]);
        }
        // `update` has been maintaining `self.length`; the trailer must carry
        // the length as it was BEFORE padding, captured above.
        let block = {
            self.buffer[56..64].copy_from_slice(&bit_len.to_le_bytes());
            self.buffer
        };
        self.compress(&block);

        let mut out = [0u8; 16];
        for (i, w) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        out
    }

    /// Compress one 64-byte block into the state (RFC 1321 §3.4).
    ///
    /// Every index here is bounded by a compile-time constant: `block` is a
    /// `[u8; 64]` read at `i * 4 + 0..=3` for `i` in `0..16`; `m` is 16 words
    /// read at `g`, which the round formulas confine to `0..16` by their
    /// `% 16`; `K` and `SHIFTS` are 64 elements read at `i` in `0..64`.
    #[allow(clippy::indexing_slicing)]
    fn compress(&mut self, block: &[u8; 64]) {
        // The block as 16 little-endian words.
        let mut m = [0u32; 16];
        for (i, w) in m.iter_mut().enumerate() {
            *w = u32::from_le_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }

        let [mut a, mut b, mut c, mut d] = self.state;

        for i in 0..64 {
            // The round determines both the non-linear function and which
            // message word this operation consumes. The index formulas are
            // RFC 1321's, transcribed rather than derived.
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };

            // All arithmetic is mod 2^32 — wrapping, not checked. A debug
            // build with overflow checks would otherwise panic on correct
            // input, which is the kind of "works in release only" bug worth
            // spending a word on.
            let tmp = d;
            d = c;
            c = b;
            let sum = a
                .wrapping_add(f)
                .wrapping_add(K[i])
                .wrapping_add(m[g])
                .rotate_left(SHIFTS[i]);
            b = b.wrapping_add(sum);
            a = tmp;
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
    }
}

/// One-shot convenience: digest a single slice.
///
/// Most call sites in [`crate::crypto::standard`] hash several pieces and use
/// [`Md5`] directly; this exists for the 50-round loops of Algorithms 2 and 3,
/// where each round hashes exactly one slice.
#[must_use]
pub fn md5(data: &[u8]) -> [u8; 16] {
    let mut h = Md5::new();
    h.update(data);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// RFC 1321 Appendix A.5, all seven vectors.
    ///
    /// All seven, not a representative one: the empty string never enters the
    /// message loop, a short string never crosses a block boundary, and the
    /// 80-character vector is the only one that compresses more than one
    /// block. A digest that passes only the first proves the constants were
    /// transcribed, nothing more.
    #[test]
    fn rfc1321_test_suite() {
        let cases: [(&str, &str); 7] = [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                "1234567890123456789012345678901234567890\
                 1234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(hex(&md5(input.as_bytes())), want, "input {input:?}");
        }
    }

    /// Incremental feeding must equal one-shot feeding.
    ///
    /// Algorithm 2 feeds the hash from five call sites; if buffering were
    /// wrong the file key would be wrong for every document, and the failure
    /// would present as "the password is rejected" rather than as a hash bug.
    #[test]
    fn incremental_equals_one_shot() {
        let data: Vec<u8> = (0u8..=255).cycle().take(1000).collect();
        let want = md5(&data);

        // A deliberately awkward split schedule: sizes that are not block
        // multiples, including one larger than a block and one of zero.
        for chunk in [1usize, 7, 63, 64, 65, 100, 999] {
            let mut h = Md5::new();
            for piece in data.chunks(chunk) {
                h.update(piece);
            }
            assert_eq!(h.finish(), want, "chunk size {chunk}");
        }

        // Empty updates must be inert.
        let mut h = Md5::new();
        h.update(&[]);
        h.update(&data);
        h.update(&[]);
        assert_eq!(h.finish(), want);
    }

    /// The length trailer counts the ORIGINAL message, not the padded one.
    ///
    /// Exercised at exactly the boundary where padding spills into a second
    /// block: a 56-byte message leaves no room for the 8-byte length, so the
    /// padding must run to 120 bytes. Getting this wrong passes every short
    /// vector and fails only here.
    #[test]
    fn padding_boundary_lengths() {
        // Independent expectations for 55/56/57 'a's, the three lengths that
        // straddle the one-block-vs-two-block padding decision.
        let cases: [(usize, &str); 3] = [
            (55, "ef1772b6dff9a122358552954ad0df65"),
            (56, "3b0c8ac703f828b04c6c197006d17218"),
            (57, "652b906d60af96844ebd21b674f35e93"),
        ];
        for (n, want) in cases {
            let msg = vec![b'a'; n];
            assert_eq!(hex(&md5(&msg)), want, "{n} bytes");
        }
    }
}

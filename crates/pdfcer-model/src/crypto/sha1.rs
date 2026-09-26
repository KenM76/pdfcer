//! SHA-1 (FIPS 180-4 §6.1), implemented in-crate, for **signature
//! verification only**.
//!
//! # Why in-crate
//!
//! The same three facts that put MD5 here (`md5.rs`): the algorithm is
//! frozen, it is small, and its weakness is the signer's choice rather than
//! pdfcer's. `adbe.pkcs7.sha1` and pre-2010 `adbe.pkcs7.detached`
//! signatures use SHA-1, and ISO 32000-1 permits it; a verifier that could
//! not compute it would report every such signature *unverifiable*, which is
//! honest but useless to an operator holding a 2009 drawing. What pdfcer does
//! with a SHA-1 signature it verified is a **disclosure** question — the
//! verdict names the digest algorithm so a shell can say "verified, with an
//! algorithm no longer considered collision-resistant" — not a reason to
//! refuse to compute it.
//!
//! **It is never used to produce a signature.** Signing (Backlog) takes the
//! `sha2` dependency's family and a modern digest.
//!
//! # Test vectors
//!
//! FIPS 180-4's own: `"abc"`, the empty string, and the 56-byte two-block
//! message — the standard's published values, not ones derived here.

/// One 64-byte block of SHA-1.
///
/// `w` is a fixed 80-element schedule indexed by `chunks_exact(4)` positions
/// (0..16) and the `16..80` loop's `i − 3/8/14/16`, all inside it; each
/// chunk is exactly four bytes.
#[allow(clippy::indexing_slicing)]
fn compress(state: &mut [u32; 5], block: &[u8; 64]) {
    let mut w = [0u32; 80];
    for (i, chunk) in block.chunks_exact(4).enumerate() {
        w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *state;
    for (i, &wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | (!b & d), 0x5A82_7999u32),
            20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
            _ => (b ^ c ^ d, 0xCA62_C1D6),
        };
        let t = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = t;
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
}

/// The SHA-1 digest of `data`.
///
/// `out[i*4..i*4+4]` for `i` in `0..5` stays inside the 20-byte output.
#[must_use]
#[allow(clippy::indexing_slicing)]
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut state = [
        0x6745_2301u32,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut blocks = data.chunks_exact(64);
    for block in &mut blocks {
        let mut b = [0u8; 64];
        b.copy_from_slice(block);
        compress(&mut state, &b);
    }
    // Padding: 0x80, zeros to 56 mod 64, then the bit length big-endian.
    let rest = blocks.remainder();
    let mut tail = Vec::with_capacity(128);
    tail.extend_from_slice(rest);
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in tail.chunks_exact(64) {
        let mut b = [0u8; 64];
        b.copy_from_slice(block);
        compress(&mut state, &b);
    }
    let mut out = [0u8; 20];
    for (i, s) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&s.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(d: &[u8]) -> String {
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn fips_180_4_vectors() {
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(&sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        // A message that pads into a second block (55, 56 and 64 bytes).
        assert_eq!(
            hex(&sha1(&[b'a'; 55])),
            "c1c8bbdc22796e28c0e15163d20899b65621d65a"
        );
        assert_eq!(
            hex(&sha1(&[b'a'; 56])),
            "c2db330f6083854c99d4b5bfb6e8f29f201be699"
        );
        assert_eq!(
            hex(&sha1(&[b'a'; 64])),
            "0098ba824b5c16427bd7a1122a5a442a25ec644d"
        );
    }
}

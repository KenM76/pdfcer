//! Arbitrary-precision unsigned integers for **public-key verification only**.
//!
//! # Why this exists rather than a dependency
//!
//! Signature verification needs modular exponentiation (RSA, with the
//! *public* exponent) and prime-field arithmetic (ECDSA over P-256/P-384).
//! The ecosystem's answers — `num-bigint`, `crypto-bigint`, the `p256`/
//! `p384`/`ecdsa`/`elliptic-curve` stack — resolve to twenty-five crates,
//! several carrying `unsafe` with cfg-selected constant-time backends
//! (`cmov`, `hybrid-array`) in exactly the shape decision 039 accepted for
//! `aes` only after argument. That shape exists for a reason that does not
//! apply here: constant-time code protects a **secret**, and verification
//! handles none. The public key, the signature and the digest are all in
//! the file. A timing leak of any of them leaks nothing.
//!
//! So the judgement recorded for MD5 and RC4 (`md5.rs`: frozen, small, and
//! "the cryptographic weakness is the standard's") is extended, narrowly,
//! to *verification-side* big-number arithmetic: schoolbook multiplication
//! and shift-subtract division, correct and slow, in safe Rust, with no
//! secret to protect. **It does NOT extend to signing.** A signing
//! implementation holds a private key, must be constant-time, and gets the
//! dependency — the `aes` argument, not the `md5` one.
//!
//! # Correctness posture
//!
//! A wrong answer here has one of two consequences: a valid signature
//! reported as unverifiable (fail closed, annoying, visible) or a forged
//! signature reported as valid (the one claim in pdfcer that must never be
//! wrong). The second needs an arithmetic error that happens to land on the
//! verification equation, which is not a failure mode arithmetic bugs have;
//! the tests below pin the primitives against values computed by an
//! independent implementation (Python's `pow`) so the first cannot hide
//! either.
//!
//! # Representation
//!
//! Little-endian `u32` limbs, no leading-zero limbs after normalisation,
//! zero is the empty vector. Sizes here are ≤ 8192 bits (RSA-8192 would be
//! absurd but legal) — every operation is `O(n²)` at worst and a full
//! RSA-4096 verification is under a millisecond in release.

use std::cmp::Ordering;

/// An unsigned integer of arbitrary size.
///
/// Limb indexing throughout this `impl` is in bounds by construction: every
/// index is a loop counter bounded by a `.len()` taken from the same vector
/// (or a vector sized `a.len() + b.len()` / `n + m + 1` for exactly that
/// loop), which is the shape the sibling crypto modules carry the same
/// allow for (`md5.rs`, `rc4.rs`). Stated once here rather than at each
/// method; the crate-level policy is documented in `lib.rs`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Uint {
    /// Little-endian limbs; no trailing (most-significant) zero limbs.
    limbs: Vec<u32>,
}

#[allow(clippy::indexing_slicing)]
impl Uint {
    /// Zero.
    #[must_use]
    pub const fn zero() -> Self {
        Self { limbs: Vec::new() }
    }

    /// One.
    #[must_use]
    pub fn one() -> Self {
        Self { limbs: vec![1] }
    }

    /// From a small value.
    #[must_use]
    pub fn from_u64(v: u64) -> Self {
        let mut s = Self {
            limbs: vec![v as u32, (v >> 32) as u32],
        };
        s.trim();
        s
    }

    /// From big-endian bytes (the DER / SEC1 / PKCS#1 convention).
    #[must_use]
    pub fn from_be_bytes(bytes: &[u8]) -> Self {
        let mut limbs = Vec::with_capacity(bytes.len().div_ceil(4));
        for chunk in bytes.rchunks(4) {
            let mut limb = 0u32;
            for &b in chunk {
                limb = (limb << 8) | u32::from(b);
            }
            limbs.push(limb);
        }
        let mut s = Self { limbs };
        s.trim();
        s
    }

    /// Big-endian bytes, zero-padded on the left to exactly `len` bytes.
    /// Returns `None` if the value does not fit.
    #[must_use]
    pub fn to_be_bytes(&self, len: usize) -> Option<Vec<u8>> {
        let mut out = vec![0u8; len];
        let mut i = len;
        for limb in &self.limbs {
            for k in 0..4 {
                let byte = ((limb >> (8 * k)) & 0xFF) as u8;
                if i == 0 {
                    if byte != 0 {
                        return None;
                    }
                    continue;
                }
                i -= 1;
                out[i] = byte;
            }
        }
        Some(out)
    }

    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    /// Whether the value is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// Number of significant bits.
    #[must_use]
    pub fn bits(&self) -> usize {
        match self.limbs.last() {
            None => 0,
            Some(top) => (self.limbs.len() - 1) * 32 + (32 - top.leading_zeros() as usize),
        }
    }

    /// Bit `i` (0 = least significant).
    #[must_use]
    pub fn bit(&self, i: usize) -> bool {
        self.limbs
            .get(i / 32)
            .is_some_and(|l| (l >> (i % 32)) & 1 == 1)
    }

    /// `self + other`.
    #[must_use]
    pub fn add(&self, other: &Self) -> Self {
        let n = self.limbs.len().max(other.limbs.len());
        let mut out = Vec::with_capacity(n + 1);
        let mut carry = 0u64;
        for i in 0..n {
            let a = u64::from(self.limbs.get(i).copied().unwrap_or(0));
            let b = u64::from(other.limbs.get(i).copied().unwrap_or(0));
            let s = a + b + carry;
            out.push(s as u32);
            carry = s >> 32;
        }
        if carry > 0 {
            out.push(carry as u32);
        }
        let mut s = Self { limbs: out };
        s.trim();
        s
    }

    /// `self - other`, or `None` if it would go negative.
    #[must_use]
    pub fn checked_sub(&self, other: &Self) -> Option<Self> {
        if self.cmp(other) == Ordering::Less {
            return None;
        }
        let mut out = Vec::with_capacity(self.limbs.len());
        let mut borrow = 0i64;
        for i in 0..self.limbs.len() {
            let a = i64::from(self.limbs[i]);
            let b = i64::from(other.limbs.get(i).copied().unwrap_or(0));
            let mut d = a - b - borrow;
            if d < 0 {
                d += 1 << 32;
                borrow = 1;
            } else {
                borrow = 0;
            }
            out.push(d as u32);
        }
        let mut s = Self { limbs: out };
        s.trim();
        Some(s)
    }

    /// `self * other` (schoolbook).
    #[must_use]
    pub fn mul(&self, other: &Self) -> Self {
        if self.is_zero() || other.is_zero() {
            return Self::zero();
        }
        let mut out = vec![0u32; self.limbs.len() + other.limbs.len()];
        for (i, &a) in self.limbs.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &b) in other.limbs.iter().enumerate() {
                let cur = u64::from(out[i + j]) + u64::from(a) * u64::from(b) + carry;
                out[i + j] = cur as u32;
                carry = cur >> 32;
            }
            let mut k = i + other.limbs.len();
            while carry > 0 {
                let cur = u64::from(out[k]) + carry;
                out[k] = cur as u32;
                carry = cur >> 32;
                k += 1;
            }
        }
        let mut s = Self { limbs: out };
        s.trim();
        s
    }

    /// `self << bits`.
    #[must_use]
    pub fn shl(&self, bits: usize) -> Self {
        if self.is_zero() {
            return Self::zero();
        }
        let limb_shift = bits / 32;
        let bit_shift = bits % 32;
        let mut out = vec![0u32; limb_shift];
        if bit_shift == 0 {
            out.extend_from_slice(&self.limbs);
        } else {
            let mut carry = 0u32;
            for &l in &self.limbs {
                out.push((l << bit_shift) | carry);
                carry = l >> (32 - bit_shift);
            }
            if carry > 0 {
                out.push(carry);
            }
        }
        let mut s = Self { limbs: out };
        s.trim();
        s
    }

    /// `self mod m`. `m` must be non-zero (a zero modulus yields zero).
    ///
    /// Knuth's Algorithm D (TAOCP 4.3.1) in base 2³²: the divisor is
    /// normalised so its top limb has its high bit set, each quotient limb
    /// is estimated from the top two limbs of the running remainder and the
    /// top limb of the divisor, corrected at most twice, and the remainder
    /// is de-normalised at the end. Quadratic in the limb count, which for
    /// a 4096-bit product over a 2048-bit modulus is a few thousand limb
    /// operations — against the ~250,000 a bit-serial shift-and-subtract
    /// costs, which is what makes ECDSA verification (thousands of field
    /// reductions) take milliseconds rather than seconds.
    #[must_use]
    pub fn rem(&self, m: &Self) -> Self {
        if m.is_zero() {
            return Self::zero();
        }
        if self.cmp(m) == Ordering::Less {
            return self.clone();
        }
        if m.limbs.len() == 1 {
            // Single-limb divisor: plain long division.
            let d = u64::from(m.limbs[0]);
            let mut r = 0u64;
            for &limb in self.limbs.iter().rev() {
                r = ((r << 32) | u64::from(limb)) % d;
            }
            return Self::from_u64(r);
        }
        // Normalise: shift both so the divisor's top limb has bit 31 set.
        let shift = m.limbs[m.limbs.len() - 1].leading_zeros() as usize;
        let v = m.shl(shift).limbs;
        let mut u = self.shl(shift).limbs;
        u.push(0); // room for the extra top limb Algorithm D needs
        let n = v.len();
        let mmax = u.len() - n - 1;
        let v_top = u64::from(v[n - 1]);
        let v_next = u64::from(v[n - 2]);
        for j in (0..=mmax).rev() {
            // Estimate q̂ from the top two remainder limbs.
            let num = (u64::from(u[j + n]) << 32) | u64::from(u[j + n - 1]);
            let mut qhat = num / v_top;
            let mut rhat = num % v_top;
            while qhat >= (1 << 32) || qhat * v_next > ((rhat << 32) | u64::from(u[j + n - 2])) {
                qhat -= 1;
                rhat += v_top;
                if rhat >= (1 << 32) {
                    break;
                }
            }
            // Multiply and subtract q̂·v from the remainder window.
            let mut borrow = 0i64;
            let mut carry = 0u64;
            for i in 0..n {
                let p = qhat * u64::from(v[i]) + carry;
                carry = p >> 32;
                let t = i64::from(u[i + j]) - i64::from(p as u32) - borrow;
                u[i + j] = t as u32;
                borrow = i64::from(t < 0);
            }
            let t = i64::from(u[j + n]) - i64::from(carry as u32) - borrow;
            u[j + n] = t as u32;
            if t < 0 {
                // q̂ was one too large: add v back.
                let mut c = 0u64;
                for i in 0..n {
                    let s2 = u64::from(u[i + j]) + u64::from(v[i]) + c;
                    u[i + j] = s2 as u32;
                    c = s2 >> 32;
                }
                u[j + n] = u[j + n].wrapping_add(c as u32);
            }
        }
        // The remainder is the low n limbs, de-normalised.
        u.truncate(n);
        let mut r = Self { limbs: u };
        r.trim();
        r.shr(shift)
    }

    /// `self >> bits`.
    #[must_use]
    pub fn shr(&self, bits: usize) -> Self {
        let limb_shift = bits / 32;
        let bit_shift = bits % 32;
        if limb_shift >= self.limbs.len() {
            return Self::zero();
        }
        let src = &self.limbs[limb_shift..];
        let mut out = Vec::with_capacity(src.len());
        if bit_shift == 0 {
            out.extend_from_slice(src);
        } else {
            for i in 0..src.len() {
                let lo = src[i] >> bit_shift;
                let hi = src.get(i + 1).map_or(0, |&h| h << (32 - bit_shift));
                out.push(lo | hi);
            }
        }
        let mut s = Self { limbs: out };
        s.trim();
        s
    }

    /// `(self * other) mod m`.
    #[must_use]
    pub fn mulmod(&self, other: &Self, m: &Self) -> Self {
        self.mul(other).rem(m)
    }

    /// `(self + other) mod m`, for operands already below `m`.
    #[must_use]
    pub fn addmod(&self, other: &Self, m: &Self) -> Self {
        let s = self.add(other);
        if s.cmp(m) == Ordering::Less {
            s
        } else {
            s.checked_sub(m).unwrap_or_default()
        }
    }

    /// `(self - other) mod m`, for operands already below `m`.
    #[must_use]
    pub fn submod(&self, other: &Self, m: &Self) -> Self {
        match self.checked_sub(other) {
            Some(d) => d,
            None => self.add(m).checked_sub(other).unwrap_or_default(),
        }
    }

    /// `self ^ exp mod m` by square-and-multiply. Not constant-time, by
    /// design (module docs): every operand here is public.
    #[must_use]
    pub fn modpow(&self, exp: &Self, m: &Self) -> Self {
        if m.is_zero() {
            return Self::zero();
        }
        let mut result = Self::one().rem(m);
        let base = self.rem(m);
        for i in (0..exp.bits()).rev() {
            result = result.mulmod(&result, m);
            if exp.bit(i) {
                result = result.mulmod(&base, m);
            }
        }
        result
    }

    /// Modular inverse for a PRIME modulus, by Fermat: `self ^ (m-2)`.
    /// Returns zero for a zero input (which has no inverse).
    #[must_use]
    pub fn invmod_prime(&self, m: &Self) -> Self {
        let two = Self::from_u64(2);
        match m.checked_sub(&two) {
            Some(e) => self.modpow(&e, m),
            None => Self::zero(),
        }
    }
}

impl Ord for Uint {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.limbs.len().cmp(&other.limbs.len()) {
            Ordering::Equal => {}
            o => return o,
        }
        for (a, b) in self.limbs.iter().rev().zip(other.limbs.iter().rev()) {
            match a.cmp(b) {
                Ordering::Equal => continue,
                o => return o,
            }
        }
        Ordering::Equal
    }
}

impl PartialOrd for Uint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Uint {
        let bytes: Vec<u8> = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect();
        Uint::from_be_bytes(&bytes)
    }

    #[test]
    fn round_trips_bytes_and_trims_zeros() {
        let v = Uint::from_be_bytes(&[0, 0, 1, 2, 3]);
        assert_eq!(v.to_be_bytes(3).unwrap(), vec![1, 2, 3]);
        assert_eq!(v.to_be_bytes(5).unwrap(), vec![0, 0, 1, 2, 3]);
        assert!(v.to_be_bytes(2).is_none());
        assert_eq!(v.bits(), 17);
        assert!(Uint::from_be_bytes(&[0, 0]).is_zero());
    }

    #[test]
    fn arithmetic_matches_u128() {
        let a = 0xFFFF_FFFF_FFFF_FFFF_1234_5678u128;
        let b = 0x9ABC_DEF0_1234_5678_9ABC_DEF0u128;
        let ua = Uint::from_be_bytes(&a.to_be_bytes());
        let ub = Uint::from_be_bytes(&b.to_be_bytes());
        assert_eq!(ua.add(&ub).to_be_bytes(16).unwrap(), (a + b).to_be_bytes());
        assert_eq!(
            ua.checked_sub(&ub).unwrap().to_be_bytes(16).unwrap(),
            (a - b).to_be_bytes()
        );
        assert!(ub.checked_sub(&ua).is_none());
        let m = 0x1_0000_0000_0000_0007u128;
        let um = Uint::from_be_bytes(&m.to_be_bytes());
        assert_eq!(ua.rem(&um).to_be_bytes(16).unwrap(), (a % m).to_be_bytes());
        // (a * b) mod m via u128 is not directly computable; use small ones.
        let sa = Uint::from_u64(0xDEAD_BEEF_CAFE);
        let sb = Uint::from_u64(0x1234_5678_9ABC);
        let prod = 0xDEAD_BEEF_CAFEu128 * 0x1234_5678_9ABCu128;
        assert_eq!(sa.mul(&sb).to_be_bytes(16).unwrap(), prod.to_be_bytes());
        assert_eq!(
            sa.shl(37).to_be_bytes(16).unwrap(),
            (0xDEAD_BEEF_CAFEu128 << 37).to_be_bytes()
        );
    }

    /// The bit-serial reference `rem`, kept only to cross-check Algorithm D.
    fn rem_reference(a: &Uint, m: &Uint) -> Uint {
        let mut r = Uint::zero();
        for i in (0..a.bits()).rev() {
            r = r.shl(1);
            if a.bit(i) {
                r = r.add(&Uint::one());
            }
            if r.cmp(m) != Ordering::Less {
                r = r.checked_sub(m).unwrap();
            }
        }
        r
    }

    #[test]
    fn knuth_division_agrees_with_the_bit_serial_reference() {
        // A cheap deterministic generator (xorshift) — no dependency.
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for case in 0..400 {
            let alen = 1 + (next() % 40) as usize;
            let mlen = 1 + (next() % 20) as usize;
            let a_bytes: Vec<u8> = (0..alen).map(|_| next() as u8).collect();
            let mut m_bytes: Vec<u8> = (0..mlen).map(|_| next() as u8).collect();
            if m_bytes.iter().all(|&b| b == 0) {
                m_bytes[0] = 1;
            }
            // Exercise the q̂-correction paths: divisors with a high top limb.
            if case % 3 == 0 {
                m_bytes[0] |= 0x80;
            }
            let a = Uint::from_be_bytes(&a_bytes);
            let m = Uint::from_be_bytes(&m_bytes);
            assert_eq!(
                a.rem(&m),
                rem_reference(&a, &m),
                "case {case}: {a_bytes:02X?} mod {m_bytes:02X?}"
            );
        }
        // Edge: remainder exactly zero, and dividend a multiple of the divisor.
        let m = hex("FFFFFFFF00000001000000000000000000000000FFFFFFFFFFFFFFFFFFFFFFFF");
        assert!(m.mul(&Uint::from_u64(12345)).rem(&m).is_zero());
        assert_eq!(m.shr(0), m);
        assert_eq!(m.shr(8).shl(8).add(&Uint::from_u64(0xFF)), m);
    }

    #[test]
    fn modpow_matches_python_pow() {
        // pow(0xDEADBEEF, 65537, 0xFFFFFFFB) == 0x8E338BE9 (computed with Python)
        let r = Uint::from_u64(0xDEAD_BEEF)
            .modpow(&Uint::from_u64(65537), &Uint::from_u64(0xFFFF_FFFB));
        assert_eq!(r, Uint::from_u64(0x8E33_8BE9));
        // A 256-bit case against Python:
        // p = 2^256 - 2^224 + 2^192 + 2^96 - 1 (the P-256 field prime)
        // pow(3, p-2, p) * 3 % p == 1
        let p = hex("FFFFFFFF00000001000000000000000000000000FFFFFFFFFFFFFFFFFFFFFFFF");
        let three = Uint::from_u64(3);
        let inv = three.invmod_prime(&p);
        assert_eq!(inv.mulmod(&three, &p), Uint::one());
        assert_eq!(
            inv,
            hex("AAAAAAAA00000000AAAAAAAAAAAAAAAAAAAAAAAB555555555555555555555555")
        );
    }
}

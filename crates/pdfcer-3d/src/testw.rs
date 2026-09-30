//! Test-only PRC bit writer: the inverse of [`crate::bits::BitReader`].

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use crate::acof;

/// MSB-first bit sink.
#[derive(Default, Clone)]
pub(crate) struct W {
    bits: Vec<bool>,
}

impl W {
    pub(crate) fn put(&mut self, v: u64, n: u32) -> &mut Self {
        for i in (0..n).rev() {
            self.bits.push((v >> i) & 1 == 1);
        }
        self
    }

    pub(crate) fn bit(&mut self, b: bool) -> &mut Self {
        self.bits.push(b);
        self
    }

    pub(crate) fn len(&self) -> usize {
        self.bits.len()
    }

    pub(crate) fn append(&mut self, other: &W) -> &mut Self {
        self.bits.extend_from_slice(&other.bits);
        self
    }

    pub(crate) fn uint(&mut self, mut v: u32) -> &mut Self {
        while v != 0 {
            self.bit(true).put(u64::from(v & 0xff), 8);
            v >>= 8;
        }
        self.bit(false)
    }

    pub(crate) fn int(&mut self, v: i32) -> &mut Self {
        if v == 0 {
            return self.bit(false);
        }
        self.bit(true);
        let mut v = i64::from(v);
        loop {
            let byte = (v & 0xff) as u8;
            self.put(u64::from(byte), 8);
            v >>= 8;
            let done = (v == 0 && byte & 0x80 == 0) || (v == -1 && byte & 0x80 != 0);
            self.bit(!done);
            if done {
                return self;
            }
        }
    }

    pub(crate) fn string(&mut self, s: Option<&str>) -> &mut Self {
        let Some(s) = s else {
            return self.bit(false);
        };
        self.bit(true).uint(s.len() as u32);
        for b in s.bytes() {
            self.put(u64::from(b), 8);
        }
        self
    }

    /// Any finite value but ±0 through an exponent row and a literal
    /// mantissa; 0.0 through the two-bit zero code.
    pub(crate) fn double(&mut self, v: f64) -> &mut Self {
        if v == 0.0 {
            return self.put(0b01, 2);
        }
        let bits = v.to_bits();
        let (n, c) = acof::exponent_code(((bits >> 52) & 0x7ff) as u32).expect("exponent row");
        let le = bits.to_le_bytes();
        self.put(u64::from(c), n)
            .bit(v < 0.0)
            .bit(true)
            .put(u64::from(le[6] & 0x0f), 4);
        for i in (0..=5).rev() {
            self.bit(true).put(u64::from(le[i]), 8);
        }
        self
    }

    pub(crate) fn bytes(&self) -> Vec<u8> {
        self.bits
            .chunks(8)
            .map(|c| {
                c.iter()
                    .enumerate()
                    .fold(0u8, |a, (i, &b)| a | (u8::from(b) << (7 - i)))
            })
            .collect()
    }
}

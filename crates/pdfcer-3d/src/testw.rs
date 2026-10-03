//! Test-only PRC bit writer: the inverse of [`crate::bits::BitReader`].

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use crate::acof;

/// MSB-first bit sink.
#[derive(Default, Clone)]
pub(crate) struct W {
    bits: Vec<bool>,
}

impl W {
    /// The low `n` bits of `v`, most significant first.
    pub(crate) fn put(&mut self, v: u64, n: u32) -> &mut Self {
        for i in (0..n).rev() {
            self.bits.push((v >> i) & 1 == 1);
        }
        self
    }

    /// One bit.
    pub(crate) fn bit(&mut self, b: bool) -> &mut Self {
        self.bits.push(b);
        self
    }

    /// Bits written so far.
    pub(crate) fn len(&self) -> usize {
        self.bits.len()
    }

    /// Every bit of `other`, unaligned.
    pub(crate) fn append(&mut self, other: &W) -> &mut Self {
        self.bits.extend_from_slice(&other.bits);
        self
    }

    /// A PRC UnsignedInteger: continuation bit, then 8 bits, per byte.
    pub(crate) fn uint(&mut self, mut v: u32) -> &mut Self {
        while v != 0 {
            self.bit(true).put(u64::from(v & 0xff), 8);
            v >>= 8;
        }
        self.bit(false)
    }

    /// A PRC Integer: sign-extended bytes, each followed by a continue bit.
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

    /// A PRC String: presence bit, byte count, raw bytes.
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

    /// A Huffman container: `leaves` are `(symbol, stored code, length)`,
    /// each value is emitted as its leaf's code.
    pub(crate) fn huffman(
        &mut self,
        nbits: u32,
        len_width: u32,
        leaves: &[(u32, u32, u32)],
        values: &[i32],
    ) -> &mut Self {
        let mask = (1u32 << nbits) - 1;
        let codes: Vec<(u32, u32)> = values
            .iter()
            .map(|&v| {
                let l = leaves
                    .iter()
                    .find(|l| l.0 & mask == v as u32 & mask)
                    .expect("a leaf per value");
                (l.1, l.2)
            })
            .collect();
        self.huffman_raw(nbits, len_width, leaves, &codes)
    }

    /// As [`Self::huffman`] with each element's `(code, length)` given.
    pub(crate) fn huffman_raw(
        &mut self,
        nbits: u32,
        len_width: u32,
        leaves: &[(u32, u32, u32)],
        codes: &[(u32, u32)],
    ) -> &mut Self {
        let mut blob = Vec::new();
        let lsb = |blob: &mut Vec<bool>, v: u32, n: u32| {
            for i in 0..n {
                blob.push((v >> i) & 1 == 1);
            }
        };
        lsb(&mut blob, leaves.len() as u32, nbits + 1);
        lsb(&mut blob, len_width, 8);
        for &(s, c, l) in leaves {
            lsb(&mut blob, s, nbits);
            lsb(&mut blob, l, len_width);
            lsb(&mut blob, c, l);
        }
        lsb(&mut blob, codes.len() as u32, 32);
        for &(c, l) in codes {
            for i in (0..l).rev() {
                blob.push((c >> i) & 1 == 1);
            }
        }
        let bits = blob.len();
        let words = bits.div_ceil(32);
        blob.resize(words * 32, false);
        self.uint(words as u32);
        for byte in blob.chunks(8) {
            let b = byte
                .iter()
                .enumerate()
                .fold(0u32, |a, (i, &x)| a | (u32::from(x) << i));
            self.put(u64::from(b), 8);
        }
        self.uint((bits - (words - 1) * 32) as u32)
    }

    /// The bits packed MSB-first, the last byte zero-padded.
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

/// A one-file-structure PRC stream holding the given section bytes and
/// model file. The file structure's id is `[5, 6, 7, 8]`.
pub(crate) fn prc_container(globals: &[u8], tree: &[u8], tess: &[u8], model: &[u8]) -> Vec<u8> {
    prc_container_n(&[[globals, tree, tess]], model)
}

/// A PRC stream with one file structure per `[globals, tree, tess]`, the
/// k-th with id `[5, 6, 7, 8 + k]`.
pub(crate) fn prc_container_n(structures: &[[&[u8]; 3]], model: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let zlib = |data: &[u8]| {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    };
    let le = |out: &mut Vec<u8>, vs: &[u32]| {
        vs.iter()
            .for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    };
    let header_len = 59 + 48 * structures.len();
    let mut body = Vec::new();
    let mut descriptions = Vec::new();
    for (k, &[globals, tree, tess]) in structures.iter().enumerate() {
        let id = [5, 6, 7, 8 + k as u32];
        let start = (header_len + body.len()) as u32;
        body.extend_from_slice(b"PRC");
        le(&mut body, &[8137, 8137]);
        le(&mut body, &id);
        le(&mut body, &[0, 0, 0, 0, 0]);
        le(&mut descriptions, &id);
        le(&mut descriptions, &[0, 6, start]);
        for s in [globals, tree, tess, &[], &[]] {
            le(&mut descriptions, &[(header_len + body.len()) as u32]);
            body.extend(zlib(s));
        }
    }
    let mf_start = (header_len + body.len()) as u32;
    body.extend(zlib(model));
    let mf_end = (header_len + body.len()) as u32;

    let mut out = b"PRC".to_vec();
    le(&mut out, &[8137, 8137, 1, 2, 3, 4, 0, 0, 0, 0]);
    le(&mut out, &[structures.len() as u32]);
    out.extend(descriptions);
    le(&mut out, &[mf_start, mf_end, 0]);
    assert_eq!(out.len(), header_len);
    out.extend(body);
    out
}

/// Asserts `fixtures/synthetic/prc/{name}` holds `bytes`;
/// `PDFCER_WRITE_FIXTURES=1` rewrites it.
pub(crate) fn check_fixture(name: &str, bytes: &[u8]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/prc")
        .join(name);
    if std::env::var_os("PDFCER_WRITE_FIXTURES").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
    }
    assert_eq!(
        std::fs::read(&path).unwrap(),
        bytes,
        "{name}: rerun with PDFCER_WRITE_FIXTURES=1"
    );
}

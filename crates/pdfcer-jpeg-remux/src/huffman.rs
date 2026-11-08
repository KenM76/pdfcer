//! Huffman tables: DHT parsing, decoding (T.81 F.2.2.3) and encoding
//! (T.81 C.2 / Annex K.3).

use crate::RemuxError;
use crate::bits::{BitReader, BitWriter};

/// One table as DHT carries it: `BITS` (code count per length 1–16) and
/// `HUFFVAL` (symbols in code order). T.81 B.2.4.2.
#[derive(Clone)]
pub(crate) struct Spec {
    pub(crate) bits: [u8; 16],
    pub(crate) vals: Vec<u8>,
}

/// Decoding form of a [`Spec`]: `MINCODE`/`MAXCODE`/`VALPTR` per length.
#[derive(Clone)]
pub(crate) struct Decoder {
    mincode: [i32; 17],
    maxcode: [i32; 17],
    valptr: [i32; 17],
    vals: Vec<u8>,
}

impl Decoder {
    /// Builds the decoder, refusing a `BITS` list that over-fills the code
    /// space (T.81 C.2: canonical codes, at most `2^l - 1` codes of length
    /// `l` once shorter ones are placed).
    pub(crate) fn new(spec: &Spec) -> Result<Self, RemuxError> {
        let total: usize = spec.bits.iter().map(|&n| usize::from(n)).sum();
        if total != spec.vals.len() || total > 256 {
            return Err(RemuxError::Malformed("DHT counts and symbols disagree"));
        }
        let mut d = Self {
            mincode: [0; 17],
            maxcode: [-1; 17],
            valptr: [0; 17],
            vals: spec.vals.clone(),
        };
        let (mut code, mut k) = (0i32, 0i32);
        let slots = d
            .valptr
            .iter_mut()
            .zip(d.mincode.iter_mut())
            .zip(d.maxcode.iter_mut())
            .skip(1);
        for (l, (&count, ((vp, mn), mx))) in (1u32..).zip(spec.bits.iter().zip(slots)) {
            let n = i32::from(count);
            if n > 0 {
                *vp = k;
                *mn = code;
                code += n;
                k += n;
                *mx = code - 1;
                if code > (1 << l) {
                    return Err(RemuxError::Malformed("DHT over-fills the code space"));
                }
            }
            code <<= 1;
        }
        Ok(d)
    }

    /// T.81 F.2.2.3 `DECODE`.
    pub(crate) fn decode(&self, r: &mut BitReader<'_>) -> Result<u8, RemuxError> {
        let mut code = r.bit() as i32;
        let rows = self.maxcode.iter().zip(&self.valptr).zip(&self.mincode);
        for ((&max, &ptr), &min) in rows.skip(1) {
            if code <= max {
                return usize::try_from(ptr + code - min)
                    .ok()
                    .and_then(|i| self.vals.get(i).copied())
                    .ok_or(RemuxError::Malformed("Huffman code outside its table"));
            }
            code = (code << 1) | r.bit() as i32;
        }
        Err(RemuxError::Malformed("no Huffman code matches"))
    }
}

/// Encoding form: `EHUFCO`/`EHUFSI` per symbol; size 0 = symbol absent.
pub(crate) struct Encoder {
    code: [u16; 256],
    size: [u8; 256],
}

impl Encoder {
    /// The code table for `spec` (T.81 C.2); symbols past `spec.vals` stay absent.
    pub(crate) fn new(spec: &Spec) -> Self {
        let mut e = Self {
            code: [0; 256],
            size: [0; 256],
        };
        let mut code = 0u32;
        let mut vals = spec.vals.iter();
        for (l, &count) in (1..=16u8).zip(spec.bits.iter()) {
            for &sym in vals.by_ref().take(usize::from(count)) {
                let sym = usize::from(sym);
                if let (Some(c), Some(s)) = (e.code.get_mut(sym), e.size.get_mut(sym)) {
                    *c = code as u16;
                    *s = l;
                }
                code += 1;
            }
            code <<= 1;
        }
        e
    }

    /// Writes `symbol`'s code, refusing a symbol the table does not carry.
    pub(crate) fn put(&self, w: &mut BitWriter, symbol: u8) -> Result<(), RemuxError> {
        let s = usize::from(symbol);
        match (self.size.get(s), self.code.get(s)) {
            (Some(&n), Some(&code)) if n > 0 => {
                w.put(u32::from(code), u32::from(n));
                Ok(())
            }
            _ => Err(RemuxError::Malformed(
                "coefficient outside the output tables",
            )),
        }
    }
}

/// Parses a DHT payload (everything after the length) into
/// `(class, destination, spec)` triples. Class 0 is DC, 1 is AC.
pub(crate) fn parse_dht(mut p: &[u8]) -> Result<Vec<(u8, u8, Spec)>, RemuxError> {
    const BAD: RemuxError = RemuxError::Malformed("truncated DHT segment");
    let mut out = Vec::new();
    while let Some(&head) = p.first() {
        let (tc, th) = (head >> 4, head & 0x0F);
        if tc > 1 || th > 3 {
            return Err(RemuxError::Malformed(
                "DHT class or destination out of range",
            ));
        }
        let bits: [u8; 16] = p.get(1..17).ok_or(BAD)?.try_into().map_err(|_| BAD)?;
        let n: usize = bits.iter().map(|&b| usize::from(b)).sum();
        let vals = p.get(17..17 + n).ok_or(BAD)?.to_vec();
        out.push((tc, th, Spec { bits, vals }));
        p = p.get(17 + n..).ok_or(BAD)?;
    }
    Ok(out)
}

/// T.81 Table K.3, luminance DC.
pub(crate) const K3_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
/// T.81 Table K.4, chrominance DC.
pub(crate) const K4_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
/// HUFFVAL of both K.3 and K.4: categories 0–11.
pub(crate) const DC_VALS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
/// T.81 Table K.5, luminance AC.
pub(crate) const K5_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7D];
#[rustfmt::skip]
pub(crate) const K5_VALS: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2A, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7,
    0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5,
    0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2,
    0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8,
    0xF9, 0xFA,
];
/// T.81 Table K.6, chrominance AC.
pub(crate) const K6_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
#[rustfmt::skip]
pub(crate) const K6_VALS: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xA1, 0xB1, 0xC1, 0x09, 0x23, 0x33, 0x52, 0xF0,
    0x15, 0x62, 0x72, 0xD1, 0x0A, 0x16, 0x24, 0x34, 0xE1, 0x25, 0xF1, 0x17, 0x18, 0x19, 0x1A, 0x26,
    0x27, 0x28, 0x29, 0x2A, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5,
    0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3,
    0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA,
    0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8,
    0xF9, 0xFA,
];

/// The four output tables in DHT order: luminance DC, chrominance DC,
/// luminance AC, chrominance AC, as `(class, destination, spec)`.
pub(crate) fn annex_k() -> [(u8, u8, Spec); 4] {
    let spec = |bits: [u8; 16], vals: &[u8]| Spec {
        bits,
        vals: vals.to_vec(),
    };
    [
        (0, 0, spec(K3_BITS, &DC_VALS)),
        (0, 1, spec(K4_BITS, &DC_VALS)),
        (1, 0, spec(K5_BITS, &K5_VALS)),
        (1, 1, spec(K6_BITS, &K6_VALS)),
    ]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Every symbol 8-bit sequential data can need: DC categories 0–11, and
    /// AC EOB, ZRL and every run 0–15 with size 1–10 (T.81 F.1.2.1–F.1.2.2).
    #[test]
    fn the_annex_k_tables_code_every_symbol_eight_bit_data_needs() {
        let [dc_l, dc_c, ac_l, ac_c] = annex_k();
        for (_, _, s) in [&dc_l, &dc_c] {
            let e = Encoder::new(s);
            assert!((0..=11).all(|c| e.size[c] > 0));
            Decoder::new(s).unwrap();
        }
        for (_, _, s) in [&ac_l, &ac_c] {
            let e = Encoder::new(s);
            assert!(e.size[0x00] > 0 && e.size[0xF0] > 0);
            for r in 0..16usize {
                assert!((1..=10).all(|z| e.size[r << 4 | z] > 0), "run {r}");
            }
            Decoder::new(s).unwrap();
        }
    }

    #[test]
    fn an_encoded_symbol_decodes_to_itself() {
        let [_, _, ac, _] = annex_k();
        let (e, d) = (Encoder::new(&ac.2), Decoder::new(&ac.2).unwrap());
        let mut w = BitWriter::default();
        for &s in &K5_VALS {
            e.put(&mut w, s).unwrap();
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        for &s in &K5_VALS {
            assert_eq!(d.decode(&mut r).unwrap(), s);
        }
    }

    #[test]
    fn an_overfull_code_space_is_refused() {
        let mut bits = [0u8; 16];
        bits[0] = 3;
        let spec = Spec {
            bits,
            vals: vec![0, 1, 2],
        };
        assert!(Decoder::new(&spec).is_err());
    }
}

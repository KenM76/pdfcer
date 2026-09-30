//! The `acofdoe` prefix-code table behind the PRC `Double` [WD 11.17.2].
//!
//! `data/acofdoe_8137.csv` is the table extracted from the working draft,
//! byte-identical to the spec RAG's `_sources/prc/acofdoe_8137_from_SC2N570.csv`
//! (sha256 `df0e117e5922fd701c6aaa8e2b3bf6da3a6d12f8f171ed236b948228dae8353c`);
//! decision 169 permits embedding it. 2077 rows: 29 frequent doubles and one
//! exponent row per IEEE exponent 0..=2047.

use std::collections::HashMap;
use std::sync::OnceLock;

const CSV: &str = include_str!("../data/acofdoe_8137.csv");

/// Number of rows [WD `NUMBEROFELEMENTINACOFDOE`].
pub(crate) const ROWS: usize = 2077;

/// The longest code, in bits.
pub(crate) const MAX_CODE_BITS: u32 = 22;

/// What a code decodes to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Entry {
    /// A frequent value, returned with the sign bit applied.
    Double(f64),
    /// The upper 32 bits of a binary64 (exponent set, mantissa zero); the
    /// mantissa follows in the stream.
    Exponent(u32),
}

fn key(nbits: u32, code: u32) -> u32 {
    (nbits << 24) | code
}

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.strip_prefix("0x")?, 16).ok()
}

fn parse() -> Option<HashMap<u32, Entry>> {
    let mut map = HashMap::with_capacity(ROWS);
    for line in CSV.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.trim_end().split(',').collect();
        let (kind, nbits, code, upper, lower) = (
            *f.get(1)?,
            f.get(2)?.parse::<u32>().ok()?,
            hex(f.get(3)?)?,
            hex(f.get(4)?)?,
            hex(f.get(5)?)?,
        );
        let entry = match kind {
            "double" => Entry::Double(f64::from_bits((u64::from(upper) << 32) | u64::from(lower))),
            "exponent" => Entry::Exponent(upper),
            _ => return None,
        };
        if nbits == 0 || nbits > MAX_CODE_BITS || map.insert(key(nbits, code), entry).is_some() {
            return None;
        }
    }
    (map.len() == ROWS).then_some(map)
}

fn table() -> Option<&'static HashMap<u32, Entry>> {
    static TABLE: OnceLock<Option<HashMap<u32, Entry>>> = OnceLock::new();
    TABLE.get_or_init(parse).as_ref()
}

/// The entry for an `nbits`-long code, if one exists.
pub(crate) fn lookup(nbits: u32, code: u32) -> Option<Entry> {
    table()?.get(&key(nbits, code)).copied()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_table_parses_to_2077_prefix_free_rows() {
        let t = table().expect("embedded table parses");
        assert_eq!(t.len(), ROWS);
        let doubles = t.values().filter(|e| matches!(e, Entry::Double(_))).count();
        assert_eq!(doubles, 29);
        // Prefix-free: no code is a prefix of a longer one.
        for &k in t.keys() {
            let (n, c) = (k >> 24, k & 0xff_ffff);
            for m in 1..n {
                assert!(!t.contains_key(&key(m, c >> (n - m))), "{n}:{c:#x}");
            }
        }
    }

    #[test]
    fn known_rows() {
        assert_eq!(lookup(2, 0b01), Some(Entry::Double(0.0)));
        assert_eq!(lookup(22, 0xd1d33), Some(Entry::Exponent(0x0010_0000)));
        assert_eq!(lookup(21, 0x68e98), Some(Entry::Exponent(0x7ff8_0000)));
        assert_eq!(lookup(3, 0b01), None);
    }
}

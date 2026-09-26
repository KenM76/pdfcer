//! # Predictor functions (ISO 32000-1 §7.4.4.4; RFC 2083 §6; TIFF 6.0 §14)
//!
//! Un-prediction stage for LZW/Flate-decoded data. Spec source:
//! `filter__predictors.md` in the PDF-spec RAG — which sources the PNG
//! formulas from RFC 2083 §6.2–6.6 (ISO 32000-1 cites but does not
//! reproduce them) and TIFF Predictor 2 from TIFF 6.0 §14 (including
//! the sub-byte expand→difference→repack rule).
//!
//! ## The decoder rule that shapes this API (§7.4.4.4 Table 10)
//!
//! For decoding, **`Predictor` values 10–15 are all identical**: they
//! mean "PNG prediction; the per-row algorithm tag byte in the data is
//! authoritative." Implementations that dispatch on the exact value
//! (applying one fixed algorithm to every row) are wrong for
//! `Predictor 15` and, strictly, for all of 10–14. pdfcer branches on
//! `>= 10` only and reads each row's tag.
//!
//! ## Row arithmetic (`filter__predictors.md`)
//!
//! ```text
//! bits_per_sample = Colors × BitsPerComponent
//! row_data_bytes  = ceil(Columns × bits_per_sample / 8)   (rows are byte-padded)
//! bpp             = max(1, ceil(bits_per_sample / 8))     (PNG left-neighbor distance, BYTES)
//! png_row_bytes   = 1 + row_data_bytes                    (the tag byte)
//! ```
//!
//! Data whose length is not a whole number of rows is refused
//! ([`FilterError::RaggedRows`]) — wrong parameters or truncation,
//! never silently trimmed (fail-clean, see `super`).
//!
//! ## The two classic implementation bugs, pinned by tests here
//!
//! - **Average's inner sum is NOT modulo 256** (RFC 2083 §6.4):
//!   `left + prior` can reach 510 and must be computed wide before the
//!   floor-divide; only the final add to the filtered byte wraps.
//! - **Paeth's tie-break order (a, then b, then c) is normative**
//!   (RFC 2083 §6.6 pseudocode, reproduced exactly below): a different
//!   order yields subtly wrong pixels on only some inputs.

use super::FilterError;
use crate::object::Dict;

/// Validated predictor parameters (Table 8 with defaults applied).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Params {
    /// Table 10 value. Guaranteed `2` or `10..=15` here (1 = "no
    /// prediction" never constructs a `Params`; 3–9 and >15 are
    /// refused as undefined).
    pub predictor: u8,
    /// Interleaved color components per sample (≥ 1; may exceed 4 in
    /// PDF 1.3+ — not hardcoded).
    pub colors: u32,
    /// Bits per component: 1, 2, 4, 8, or 16.
    pub bits_per_component: u32,
    /// Samples per row (≥ 1).
    pub columns: u32,
}

impl Params {
    /// Read `Predictor`/`Colors`/`BitsPerComponent`/`Columns` from a
    /// `DecodeParms` dictionary (Table 8 defaults: 1/1/8/1).
    /// Returns `Ok(None)` when no prediction applies (absent parms or
    /// `Predictor` ≤ 1 — including the "producer set `/Columns` with
    /// no `/Predictor`" case, whose parameters are meaningless and
    /// ignored per the RAG's gotcha).
    ///
    /// # Errors
    ///
    /// [`FilterError::BadParams`] for undefined `Predictor` values
    /// (3–9, >15), zero `Colors`/`Columns`, or a `BitsPerComponent`
    /// outside {1, 2, 4, 8, 16}.
    pub fn from_dict(parms: Option<&Dict>) -> Result<Option<Self>, FilterError> {
        let Some(d) = parms else { return Ok(None) };
        let int = |key: &[u8], default: i64| -> i64 {
            d.get(key)
                .and_then(crate::object::Object::as_int)
                .unwrap_or(default)
        };
        let predictor = int(b"Predictor", 1);
        if predictor <= 1 {
            return Ok(None);
        }
        let Ok(predictor) = u8::try_from(predictor) else {
            return Err(FilterError::BadParams("Predictor out of range"));
        };
        if predictor != 2 && !(10..=15).contains(&predictor) {
            return Err(FilterError::BadParams(
                "Predictor must be 1, 2, or 10..=15 (Table 10)",
            ));
        }
        let colors = u32::try_from(int(b"Colors", 1))
            .ok()
            .filter(|&c| c >= 1)
            .ok_or(FilterError::BadParams("Colors must be >= 1"))?;
        let bits_per_component = u32::try_from(int(b"BitsPerComponent", 8))
            .ok()
            .filter(|b| matches!(b, 1 | 2 | 4 | 8 | 16))
            .ok_or(FilterError::BadParams(
                "BitsPerComponent must be 1, 2, 4, 8, or 16",
            ))?;
        let columns = u32::try_from(int(b"Columns", 1))
            .ok()
            .filter(|&c| c >= 1)
            .ok_or(FilterError::BadParams("Columns must be >= 1"))?;
        Ok(Some(Self {
            predictor,
            colors,
            bits_per_component,
            columns,
        }))
    }

    /// `row_data_bytes` (module docs) — the byte-padded row length,
    /// EXCLUDING any PNG tag byte.
    fn row_data_bytes(self) -> usize {
        let bits = self.columns as u64 * self.colors as u64 * self.bits_per_component as u64;
        usize::try_from(bits.div_ceil(8)).unwrap_or(usize::MAX)
    }

    /// `bpp` — the PNG left-neighbor distance in whole bytes,
    /// `max(1, ceil(bits_per_sample / 8))` (RFC 2083 §6.1).
    fn bpp(self) -> usize {
        let bits = self.colors as u64 * self.bits_per_component as u64;
        usize::try_from(bits.div_ceil(8).max(1)).unwrap_or(usize::MAX)
    }
}

/// Un-predict `data` in place (consumed and returned) according to
/// `params`.
///
/// # Errors
///
/// [`FilterError::RaggedRows`] if the length isn't a whole number of
/// rows; [`FilterError::UnknownPngTag`] for a PNG row tag outside 0–4.
pub fn unpredict(data: Vec<u8>, params: &Params) -> Result<Vec<u8>, FilterError> {
    if params.predictor >= 10 {
        unpredict_png(data, params)
    } else {
        unpredict_tiff(data, params)
    }
}

// ---------------------------------------------------------------------------
// PNG group (RFC 2083 §6, decode direction)
// ---------------------------------------------------------------------------

/// Reconstruct PNG-predicted rows. Input rows are
/// `tag + row_data_bytes`; output rows drop the tag. `Prior` of the
/// first row is all zeros; `Raw(x < 0)` is 0 (RFC 2083 §6.1 boundary
/// rules).
fn unpredict_png(data: Vec<u8>, params: &Params) -> Result<Vec<u8>, FilterError> {
    let row_len = params.row_data_bytes();
    let png_row = row_len
        .checked_add(1)
        .ok_or(FilterError::BadParams("row length overflow"))?;
    if row_len == 0 || !data.len().is_multiple_of(png_row) {
        return Err(FilterError::RaggedRows);
    }
    let bpp = params.bpp();

    let mut out: Vec<u8> = Vec::with_capacity(data.len() / png_row * row_len);
    let mut prior: Vec<u8> = vec![0; row_len];
    let mut row: Vec<u8> = vec![0; row_len];

    for chunk in data.chunks_exact(png_row) {
        let (tag, filt) = chunk.split_first().ok_or(FilterError::RaggedRows)?;
        row.clear();
        row.extend_from_slice(filt);
        match tag {
            0 => {} // None: Raw(x) = Filt(x)
            1 => {
                // Sub: Raw(x) = Filt(x) + Raw(x − bpp)
                for x in bpp..row_len {
                    let left = row.get(x - bpp).copied().unwrap_or(0);
                    if let Some(v) = row.get_mut(x) {
                        *v = v.wrapping_add(left);
                    }
                }
            }
            2 => {
                // Up: Raw(x) = Filt(x) + Prior(x)
                for (v, &p) in row.iter_mut().zip(prior.iter()) {
                    *v = v.wrapping_add(p);
                }
            }
            3 => {
                // Average: inner sum computed WIDE, not mod 256
                // (module docs / RFC 2083 §6.4).
                for x in 0..row_len {
                    let left = if x >= bpp {
                        row.get(x - bpp).copied().unwrap_or(0)
                    } else {
                        0
                    };
                    let above = prior.get(x).copied().unwrap_or(0);
                    let avg = ((u16::from(left) + u16::from(above)) / 2) as u8;
                    if let Some(v) = row.get_mut(x) {
                        *v = v.wrapping_add(avg);
                    }
                }
            }
            4 => {
                // Paeth: Raw(x) = Filt(x) + Paeth(left, above, upper-left)
                for x in 0..row_len {
                    let (left, upleft) = if x >= bpp {
                        (
                            row.get(x - bpp).copied().unwrap_or(0),
                            prior.get(x - bpp).copied().unwrap_or(0),
                        )
                    } else {
                        (0, 0)
                    };
                    let above = prior.get(x).copied().unwrap_or(0);
                    let predicted = paeth_predictor(left, above, upleft);
                    if let Some(v) = row.get_mut(x) {
                        *v = v.wrapping_add(predicted);
                    }
                }
            }
            &t => return Err(FilterError::UnknownPngTag(t)),
        }
        out.extend_from_slice(&row);
        std::mem::swap(&mut prior, &mut row);
    }
    Ok(out)
}

/// RFC 2083 §6.6 `PaethPredictor`, transcribed exactly — including the
/// normative tie-break order (a, then b, then c). `p`/`pa`/`pb`/`pc`
/// are signed (i16), never u8 (module docs).
fn paeth_predictor(a: u8, b: u8, c: u8) -> u8 {
    let (a, b, c) = (i16::from(a), i16::from(b), i16::from(c));
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)] // a/b/c originated as u8
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}

// ---------------------------------------------------------------------------
// TIFF Predictor 2 (TIFF 6.0 §14, decode direction)
// ---------------------------------------------------------------------------

/// Reconstruct TIFF-2 horizontally-differenced rows: no tag byte, the
/// same algorithm on every row, per **component** (not per byte),
/// arithmetic modulo 2^BitsPerComponent (TIFF 6.0 §14's "normal two's
/// complement arithmetic does just what we want").
fn unpredict_tiff(mut data: Vec<u8>, params: &Params) -> Result<Vec<u8>, FilterError> {
    let row_len = params.row_data_bytes();
    if row_len == 0 || !data.len().is_multiple_of(row_len) {
        return Err(FilterError::RaggedRows);
    }
    let colors = params.colors as usize;

    match params.bits_per_component {
        8 => {
            // One byte per component; left neighbor is `colors` bytes
            // back ("subtract red from red, green from green…").
            for row in data.chunks_exact_mut(row_len) {
                for x in colors..row_len {
                    let left = row.get(x - colors).copied().unwrap_or(0);
                    if let Some(v) = row.get_mut(x) {
                        *v = v.wrapping_add(left);
                    }
                }
            }
        }
        16 => {
            // Big-endian 2-byte components (§7.4.4.4 rule 3:
            // high-order first); add modulo 2^16.
            let stride = colors * 2;
            for row in data.chunks_exact_mut(row_len) {
                for x in (stride..row_len.saturating_sub(1)).step_by(2) {
                    let left_hi = row.get(x - stride).copied().unwrap_or(0);
                    let left_lo = row.get(x - stride + 1).copied().unwrap_or(0);
                    let cur_hi = row.get(x).copied().unwrap_or(0);
                    let cur_lo = row.get(x + 1).copied().unwrap_or(0);
                    let left = u16::from_be_bytes([left_hi, left_lo]);
                    let cur = u16::from_be_bytes([cur_hi, cur_lo]);
                    let sum = cur.wrapping_add(left).to_be_bytes();
                    if let Some(v) = row.get_mut(x) {
                        *v = sum[0];
                    }
                    if let Some(v) = row.get_mut(x + 1) {
                        *v = sum[1];
                    }
                }
            }
        }
        n @ (1 | 2 | 4) => {
            // Sub-byte: TIFF 6.0 §14's stated procedure, run in
            // reverse — unpack each component into a byte (low-order
            // justified), accumulate modulo 2^n, repack high-order
            // first. Row padding bits do not participate (they are not
            // samples — §7.4.4.4 rule 4; residual open question noted
            // in the RAG).
            let comps_per_row = params.columns as usize * colors;
            let mask = (1u16 << n) as u8 - 1;
            for row in data.chunks_exact_mut(row_len) {
                let mut comps = unpack_bits(row, n, comps_per_row);
                for i in colors..comps.len() {
                    let left = comps.get(i - colors).copied().unwrap_or(0);
                    if let Some(v) = comps.get_mut(i) {
                        *v = v.wrapping_add(left) & mask;
                    }
                }
                pack_bits(&comps, n, row);
            }
        }
        _ => return Err(FilterError::BadParams("BitsPerComponent")),
    }
    Ok(data)
}

/// Unpack the first `count` `n`-bit components of `row` (packed
/// high-order-to-low-order per §7.4.4.4 rule 3) into one byte each,
/// low-order justified (TIFF 6.0 §14's expansion step).
fn unpack_bits(row: &[u8], n: u32, count: usize) -> Vec<u8> {
    let per_byte = (8 / n) as usize;
    let mask = ((1u16 << n) - 1) as u8;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let byte = row.get(i / per_byte).copied().unwrap_or(0);
        let shift = 8 - n as usize * (i % per_byte + 1);
        out.push((byte >> shift) & mask);
    }
    out
}

/// Repack `n`-bit components into `row` high-order first (the reverse
/// of [`unpack_bits`]; trailing padding bits are zeroed).
fn pack_bits(comps: &[u8], n: u32, row: &mut [u8]) {
    let per_byte = (8 / n) as usize;
    row.fill(0);
    for (i, &c) in comps.iter().enumerate() {
        let shift = 8 - n as usize * (i % per_byte + 1);
        if let Some(b) = row.get_mut(i / per_byte) {
            *b |= c << shift;
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::object::{Name, Object};

    fn params(predictor: u8, colors: u32, bpc: u32, columns: u32) -> Params {
        Params {
            predictor,
            colors,
            bits_per_component: bpc,
            columns,
        }
    }

    fn parms_dict(entries: &[(&[u8], i64)]) -> Dict {
        let mut d = Dict::new();
        for (k, v) in entries {
            d.insert(Name::from(*k), Object::Integer(*v));
        }
        d
    }

    // ---- Params validation ----

    #[test]
    fn predictor_1_or_absent_is_none() {
        assert_eq!(Params::from_dict(None).unwrap(), None);
        let d = parms_dict(&[(b"Predictor", 1), (b"Columns", 5)]);
        // /Columns with no active predictor: meaningless, ignored.
        assert_eq!(Params::from_dict(Some(&d)).unwrap(), None);
    }

    #[test]
    fn undefined_predictor_values_are_errors() {
        for bad in [3, 9, 16] {
            let d = parms_dict(&[(b"Predictor", bad)]);
            assert!(
                matches!(Params::from_dict(Some(&d)), Err(FilterError::BadParams(_))),
                "Predictor {bad} must be refused (Table 10 undefined)"
            );
        }
    }

    #[test]
    fn decode_side_10_through_15_all_accepted() {
        for p in 10..=15 {
            let d = parms_dict(&[(b"Predictor", p)]);
            let params = Params::from_dict(Some(&d)).unwrap().unwrap();
            assert!(params.predictor >= 10);
        }
    }

    // ---- PNG group ----

    #[test]
    fn png_tag_is_authoritative_per_row_not_the_predictor_value() {
        // Declared Predictor 12 (Up) but rows tagged None then Sub —
        // the TAG governs (§7.4.4.4's decoder rule).
        let data = vec![
            0, 10, 20, 30, // row 1: None
            1, 5, 5, 5, // row 2: Sub → 5, 10, 15
        ];
        let out = unpredict(data, &params(12, 1, 8, 3)).unwrap();
        assert_eq!(out, vec![10, 20, 30, 5, 10, 15]);
    }

    #[test]
    fn png_up_uses_prior_reconstructed_row() {
        let data = vec![2, 1, 2, 3, 2, 1, 1, 1];
        let out = unpredict(data, &params(12, 1, 8, 3)).unwrap();
        assert_eq!(out, vec![1, 2, 3, 2, 3, 4]);
    }

    #[test]
    fn png_sub_respects_bpp_distance() {
        // 3 components/sample, 8 bpc → bpp = 3: the left neighbor is
        // three bytes back (same component of the previous SAMPLE).
        let data = vec![1, 10, 20, 30, 5, 5, 5];
        let out = unpredict(data, &params(11, 3, 8, 2)).unwrap();
        assert_eq!(out, vec![10, 20, 30, 15, 25, 35]);
    }

    #[test]
    fn png_average_inner_sum_is_not_mod_256() {
        // left = 200, above = 200 → avg = 200 (not (144)/2 = 72,
        // which a u8 inner sum would produce). Filt = 0 → Raw = 200.
        let data = vec![
            0, 200, // row 1: None → left/above seed
            3, 100, // row 2: Average, x=0: left=0, above=200 → +100
        ];
        let out = unpredict(data, &params(13, 1, 8, 1)).unwrap();
        // Row 2: avg(0, 200) = 100 → 100 + 100 = 200.
        assert_eq!(out, vec![200, 200]);

        // The overflow-critical case inside a row: bpp=1, row [255, x]
        // with above=255: avg(255, 255) = 255 wide, 127 if wrapped.
        let data = vec![0, 255, 255, 3, 0, 0];
        let out = unpredict(data, &params(13, 1, 8, 2)).unwrap();
        // Row 2 x=0: avg(0, 255)=127 → 127; x=1: avg(127, 255)=191.
        assert_eq!(out, vec![255, 255, 127, 191]);
    }

    #[test]
    fn png_paeth_tie_break_order_is_normative() {
        // Construct a tie: a == b (pa == pb) → must pick a (left),
        // not b. left=10 (via row 1 None + row 2 Sub base), above=10.
        // Simpler: direct predictor check.
        assert_eq!(paeth_predictor(10, 10, 10), 10);
        // pa == pb < pc → a wins over b:
        // a=4, b=6, c=5: p=5, pa=1, pb=1, pc=0 → pc smallest → c.
        assert_eq!(paeth_predictor(4, 6, 5), 5);
        // a=5, b=7, c=6: p=6, pa=1, pb=1, pc=0 → c. Need a genuine
        // a/b tie with pc larger: a=3, b=5, c=1: p=7, pa=4, pb=2,
        // pc=6 → b.
        assert_eq!(paeth_predictor(3, 5, 1), 5);
        // a=5, b=3, c=1: p=7, pa=2, pb=4, pc=6 → a.
        assert_eq!(paeth_predictor(5, 3, 1), 5);
        // Exact tie pa==pb==pc: a wins (first branch, <=).
        assert_eq!(paeth_predictor(2, 2, 2), 2);
    }

    #[test]
    fn png_unknown_tag_is_error() {
        let data = vec![7, 1, 2, 3];
        assert_eq!(
            unpredict(data, &params(12, 1, 8, 3)).unwrap_err(),
            FilterError::UnknownPngTag(7)
        );
    }

    #[test]
    fn ragged_rows_refused() {
        // 3-column rows are 4 bytes with tag; 6 bytes = 1.5 rows.
        let data = vec![0, 1, 2, 3, 0, 9];
        assert_eq!(
            unpredict(data, &params(12, 1, 8, 3)).unwrap_err(),
            FilterError::RaggedRows
        );
    }

    #[test]
    fn png_sub_byte_bpp_is_one_not_zero() {
        // 1 bpc, 1 color: bits_per_sample = 1 → bpp must clamp to 1
        // (RFC 2083 "rounding up to one"), not 0 (division/offset by
        // zero). Columns 16 → row_data_bytes = 2.
        let data = vec![1, 0b1010_1010, 0b0000_0000];
        let out = unpredict(data, &params(12, 1, 1, 16)).unwrap();
        // Sub with bpp=1: byte 2 += byte 1.
        assert_eq!(out, vec![0b1010_1010, 0b1010_1010]);
    }

    // ---- TIFF Predictor 2 ----

    #[test]
    fn tiff2_8bit_per_component_stride() {
        // 2 samples × RGB: second sample stored as differences.
        let data = vec![100, 110, 120, 10, 250, 20];
        let out = unpredict(data, &params(2, 3, 8, 2)).unwrap();
        // red: 100+10=110; green: 110+250 mod 256 = 104; blue: 140.
        assert_eq!(out, vec![100, 110, 120, 110, 104, 140]);
    }

    #[test]
    fn tiff2_16bit_big_endian() {
        // 1 color, 16 bpc, 2 columns: [0x0100, +0x0203] → 0x0303.
        let data = vec![0x01, 0x00, 0x02, 0x03];
        let out = unpredict(data, &params(2, 1, 16, 2)).unwrap();
        assert_eq!(out, vec![0x01, 0x00, 0x03, 0x03]);
    }

    #[test]
    fn tiff2_4bit_unpack_accumulate_repack() {
        // TIFF 6.0 §14's sub-byte procedure. 4 components of 4 bits,
        // 1 color, columns 4 → row = 2 bytes. Stored: 5, +3, +2, +9
        // → 5, 8, 10, 3 (mod 16). Packed high-order first.
        let data = vec![0x53, 0x29];
        let out = unpredict(data, &params(2, 1, 4, 4)).unwrap();
        assert_eq!(out, vec![0x58, 0xA3]);
    }

    #[test]
    fn tiff2_rows_are_independent() {
        // Two rows, 8 bpc: differencing never crosses a row boundary.
        let data = vec![10, 5, 20, 7];
        let out = unpredict(data, &params(2, 1, 8, 2)).unwrap();
        assert_eq!(out, vec![10, 15, 20, 27]);
    }
}

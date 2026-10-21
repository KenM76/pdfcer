//! CCITT Group 4 (ITU-T T.6, `/K -1`) encoder.
//!
//! Spec source: `filters/filter__ccitt_g4_encoder.md` in the PDF-spec RAG
//! (T.6 §2.2.1-§2.2.5 coding procedure, §2.4.1 EOFB, Tables 1-3/T.6).
//! Pass mode is always a single pass, which T.6 §2.2.4 Step 1 Note
//! permits. No EOL per row, no uncompressed mode; the block ends with
//! EOFB and zero pad bits (§2.4.1.1-§2.4.1.2), so the stream matches
//! Table 11's defaults for `/EndOfBlock` and `/EncodedByteAlign`.

use super::tables::{
    BLACK_MAKEUP, BLACK_TERMINATING, EXTENDED_MAKEUP, WHITE_MAKEUP, WHITE_TERMINATING,
};

/// Encodes 1-bit samples as a CCITT Group 4 block.
///
/// `samples` holds `height` rows of `width` pels, one bit per pel, MSB
/// first, each row padded to a whole byte (the PDF image sample layout,
/// ISO 32000-2 §8.9.3). Bits past the end of `samples` read as 0.
/// `one_is_black` picks the CCITT colour a 1 bit is coded as: pair `true`
/// with `/BlackIs1 true` and `false` with `/BlackIs1` omitted, and the
/// decoded samples equal `samples` either way (ISO 32000-1 §7.4.6
/// Table 11). The two choices code the same picture at different sizes,
/// and which is smaller depends on the picture, so a caller that wants
/// the smaller stream encodes both.
///
/// The output carries no parameters of its own: the stream needs
/// `/K -1` and `/Columns width`, and should carry `/Rows height`.
///
/// # Examples
///
/// ```
/// use pdfcer_image_codec::ccitt::encode_g4;
///
/// // An all-white 8 x 2 image is one V(0) per row, then EOFB.
/// let g4 = encode_g4(&[0x00, 0x00], 8, 2, true);
/// assert_eq!(g4, [0b1100_0000, 0b0000_0100, 0b0000_0000, 0b0100_0000]);
/// ```
#[must_use]
pub fn encode_g4(samples: &[u8], width: u32, height: u32, one_is_black: bool) -> Vec<u8> {
    let stride = (width as usize).div_ceil(8);
    let mut out = Bits::default();
    let mut reference = Vec::new();
    for row in 0..height as usize {
        let start = row.saturating_mul(stride);
        let line = samples.get(start..).unwrap_or_default();
        let coding = changes(line, width, one_is_black);
        code_line(&mut out, &coding, &reference, width);
        reference = coding;
    }
    // EOFB: EOL twice (T.6 §2.4.1.1).
    out.push(1, 12);
    out.push(1, 12);
    out.finish()
}

/// The changing elements of one row (T.4 §4.2.1.3.1): every pel whose
/// colour differs from the pel before it, pel -1 being white. Even
/// indices are white-to-black changes, odd ones black-to-white.
fn changes(line: &[u8], width: u32, one_is_black: bool) -> Vec<u32> {
    let mut out = Vec::new();
    let mut black = false;
    for x in 0..width {
        let byte = line.get((x / 8) as usize).copied().unwrap_or(0);
        let bit = (byte >> (7 - x % 8)) & 1 == 1;
        if (bit == one_is_black) != black {
            black = !black;
            out.push(x);
        }
    }
    out
}

/// The first changing element right of `a0`, or `width` (T.6 §2.2.5.2).
fn next_change(line: &[u32], a0: i64, width: u32) -> u32 {
    let at = line.partition_point(|&p| i64::from(p) <= a0);
    line.get(at).copied().unwrap_or(width)
}

/// `b1` and `b2` (T.4 §4.2.1.3.1): the first reference-line change right
/// of `a0` whose colour is opposite to `a0`'s, and the change after it.
fn b1_b2(reference: &[u32], a0: i64, black: bool, width: u32) -> (u32, u32) {
    let mut at = reference.partition_point(|&p| i64::from(p) <= a0);
    // A change at an even index turns the line black.
    if (at % 2 == 0) == black {
        at += 1;
    }
    let get = |i: usize| reference.get(i).copied().unwrap_or(width);
    (get(at), get(at + 1))
}

/// Codes one row against its reference row (T.6 §2.2.4, Figure 5/T.6).
fn code_line(out: &mut Bits, coding: &[u32], reference: &[u32], width: u32) {
    let w = i64::from(width);
    let mut a0: i64 = -1;
    let mut black = false;
    while a0 < w {
        let a1 = next_change(coding, a0, width);
        let (b1, b2) = b1_b2(reference, a0, black, width);
        let d = i64::from(a1) - i64::from(b1);
        if b2 < a1 {
            out.push(0b0001, 4);
            a0 = i64::from(b2);
        } else if d.abs() <= 3 {
            let (code, len) = vertical(d);
            out.push(code, len);
            a0 = i64::from(a1);
            black = !black;
        } else {
            let a2 = next_change(coding, i64::from(a1), width);
            out.push(0b001, 3);
            // §2.2.5.1: the first run of a line counts from pel 0.
            let first = u32::try_from(i64::from(a1) - a0.max(0)).unwrap_or(0);
            run(out, first, black);
            run(out, a2 - a1, !black);
            a0 = i64::from(a2);
        }
    }
}

/// Table 1/T.6 vertical-mode code for `a1 - b1`, which is within ±3.
const fn vertical(d: i64) -> (u16, u8) {
    match d {
        0 => (0b1, 1),
        1 => (0b011, 3),
        2 => (0b000011, 6),
        3 => (0b0000011, 7),
        -1 => (0b010, 3),
        -2 => (0b000010, 6),
        _ => (0b0000010, 7),
    }
}

/// One run length (T.6 §2.2.4 Step 2 iii): make-up codes of 2560 while
/// the run is 2560 or longer, at most one smaller make-up code, then
/// exactly one terminating code, T(0) included.
fn run(out: &mut Bits, mut r: u32, black: bool) {
    let get = |table: &[(u16, u8)], i: u32| table.get(i as usize).copied().unwrap_or((0, 0));
    while r >= 2560 {
        let (code, len) = get(&EXTENDED_MAKEUP, (2560 - 1792) / 64);
        out.push(code, len);
        r -= 2560;
    }
    if r >= 64 {
        let m = r / 64 * 64;
        let (code, len) = if m >= 1792 {
            get(&EXTENDED_MAKEUP, (m - 1792) / 64)
        } else if black {
            get(&BLACK_MAKEUP, m / 64 - 1)
        } else {
            get(&WHITE_MAKEUP, m / 64 - 1)
        };
        out.push(code, len);
        r -= m;
    }
    let (code, len) = get(
        if black {
            &BLACK_TERMINATING
        } else {
            &WHITE_TERMINATING
        },
        r,
    );
    out.push(code, len);
}

/// An MSB-first bit packer.
#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    acc: u32,
    used: u32,
}

impl Bits {
    /// Appends the low `len` bits of `code`, most significant first.
    fn push(&mut self, code: u16, len: u8) {
        self.acc = (self.acc << len) | u32::from(code);
        self.used += u32::from(len);
        while self.used >= 8 {
            self.used -= 8;
            self.out.push((self.acc >> self.used) as u8);
        }
        self.acc &= (1 << self.used) - 1;
    }

    /// Pads the last byte with 0 bits (T.6 §2.4.1.2).
    fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            self.out.push((self.acc << (8 - self.used)) as u8);
        }
        self.out
    }
}

#[cfg(test)]
#[path = "encode_tests.rs"]
mod tests;

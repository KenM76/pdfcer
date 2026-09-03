//! # RunLengthDecode (ISO 32000-1 §7.4.5) — byte-oriented RLE
//!
//! Spec source: `filter__runlength.md` in the PDF-spec RAG (§7.4.5,
//! Table 6). Takes **no parameters** and is **self-limiting** — it
//! carries an explicit end-of-data byte, which is why it is one of the
//! filters whose presence makes an inline image's data extent
//! discoverable without a `/Length` (§8.9.7).
//!
//! ## The format, in full
//!
//! "The encoded data shall be a sequence of **runs**, where each run
//! shall consist of a **length byte** followed by 1 to 128 bytes of
//! data."
//!
//! | Length byte `L` | Meaning |
//! |---|---|
//! | `0 … 127` | the following **`L + 1` (1 to 128)** bytes are copied **literally** |
//! | `128` | **EOD** |
//! | `129 … 255` | the following **single** byte is repeated **`257 − L` (2 to 128)** times |
//!
//! Note the asymmetry the RAG calls out: literal runs are `L + 1` bytes
//! (so `L = 0` means **one** literal byte, never zero) while repeat runs
//! are `257 − L` (so `L = 255` means **two** copies and `L = 129` means
//! 128). The named implementation trap is writing `L <= 128` for the
//! literal branch, which consumes the EOD marker as data and then
//! reads a run length out of the following object's bytes.
//!
//! ## Expansion bound
//!
//! §7.4.5 NOTE: best case ~**64:1** (two input bytes → 128 output
//! bytes), worst case an *expansion* of 127:128. That makes
//! [`MAX_DECODED_LEN`] a formality here rather than the binding
//! constraint it is for Flate and LZW — but it is still enforced
//! incrementally, because "this filter cannot bomb" is an argument, and
//! guards are cheaper than arguments.
//!
//! ## Failure semantics
//!
//! - **No EOD before the input runs out** — the spec does not address
//!   it. Treated as an *implicit* EOD when the input ends exactly on a
//!   run boundary, which is the only interpretation that loses no data.
//!   Not spec-sanctioned; recorded here because it is a deliberate
//!   tolerance.
//! - **Truncated mid-run** (fewer than `L + 1` bytes remain for a
//!   literal, or no byte at all for a repeat) — [`FilterError::Truncated`].
//!   `filter__runlength.md` suggests emitting what is available plus a
//!   diagnostic; pdfcer does **not**, because [`super`]'s fail-clean
//!   contract puts truncated-but-partially-decodable streams in the
//!   `Err` bucket for Pass 1. A *labeled* best-effort recovery mode is a
//!   later, explicit feature — never a silent default.
//!
//! ## No encoder (rule R28)
//!
//! Read-compat only. pdfcer writes no image-codec or RLE data.

use super::{FilterError, MAX_DECODED_LEN};

/// The length byte that terminates the stream (§7.4.5: "A length value
/// of 128 shall denote EOD").
const EOD: u8 = 128;

/// Decode `data` as RunLengthDecode.
///
/// The filter takes no parameters (Table 6), so there is deliberately no
/// `parms` argument — an unused one would invite a caller to believe
/// `/DecodeParms` means something here.
///
/// # Errors
///
/// [`FilterError::Truncated`] when the input ends part-way through a
/// run; [`FilterError::OutputTooLarge`] when [`MAX_DECODED_LEN`] is
/// crossed.
///
/// # Examples
///
/// ```
/// use pdfcer_core::filters::runlength;
///
/// // 0x02 → three literal bytes; 0xFE → 257−254 = 3 copies of 'z';
/// // 0x80 → EOD.
/// let encoded = b"\x02abc\xFEz\x80";
/// assert_eq!(runlength::decode(encoded).unwrap(), b"abczzz");
/// ```
pub fn decode(data: &[u8]) -> Result<Vec<u8>, FilterError> {
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;

    loop {
        // Input exhausted on a run boundary: implicit EOD (module docs).
        let Some(&length) = data.get(i) else {
            return Ok(out);
        };
        i += 1;

        if length == EOD {
            return Ok(out);
        }

        if length < EOD {
            // Literal run: L + 1 bytes, so 1..=128 — never zero.
            let count = usize::from(length) + 1;
            let Some(run) = data.get(i..i + count) else {
                return Err(FilterError::Truncated {
                    filter: "RunLengthDecode",
                });
            };
            if out.len().saturating_add(count) > MAX_DECODED_LEN {
                return Err(FilterError::OutputTooLarge);
            }
            out.extend_from_slice(run);
            i += count;
        } else {
            // Repeat run: 257 − L copies, so 2..=128 for L in 129..=255.
            let count = 257 - usize::from(length);
            let Some(&byte) = data.get(i) else {
                return Err(FilterError::Truncated {
                    filter: "RunLengthDecode",
                });
            };
            if out.len().saturating_add(count) > MAX_DECODED_LEN {
                return Err(FilterError::OutputTooLarge);
            }
            out.resize(out.len() + count, byte);
            i += 1;
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

    #[test]
    fn literal_run_length_is_l_plus_one() {
        // L = 0 means ONE literal byte, not zero.
        assert_eq!(decode(b"\x00A\x80").unwrap(), b"A");
        assert_eq!(decode(b"\x03ABCD\x80").unwrap(), b"ABCD");
    }

    #[test]
    fn maximum_literal_run_is_128_bytes() {
        let payload = vec![b'x'; 128];
        let mut stream = vec![127u8];
        stream.extend_from_slice(&payload);
        stream.push(EOD);
        assert_eq!(decode(&stream).unwrap(), payload);
    }

    #[test]
    fn repeat_run_count_is_257_minus_l() {
        // L = 255 → 2 copies (the minimum); L = 129 → 128 (the maximum).
        assert_eq!(decode(b"\xFFq\x80").unwrap(), b"qq");
        assert_eq!(decode(b"\x81q\x80").unwrap(), vec![b'q'; 128]);
        assert_eq!(decode(b"\xFEq\x80").unwrap(), b"qqq");
    }

    #[test]
    fn one_two_eight_is_eod_not_a_literal_run() {
        // THE named trap (`filter__runlength.md`): an implementation
        // using `L <= 128` for the literal branch swallows the EOD and
        // then reads whatever follows the stream as run data.
        let stream = b"\x01AB\x80\x00\xFF\xFF\xFF";
        assert_eq!(decode(stream).unwrap(), b"AB");
    }

    #[test]
    fn missing_eod_is_an_implicit_end_on_a_run_boundary() {
        assert_eq!(decode(b"\x01AB").unwrap(), b"AB");
        assert!(decode(b"").unwrap().is_empty());
    }

    #[test]
    fn truncated_literal_run_errs() {
        // Declares 4 literal bytes, supplies 2 — fail-clean, never a
        // short-but-plausible buffer.
        assert_eq!(
            decode(b"\x03AB").unwrap_err(),
            FilterError::Truncated {
                filter: "RunLengthDecode"
            }
        );
    }

    #[test]
    fn truncated_repeat_run_errs() {
        assert_eq!(
            decode(b"\xFE").unwrap_err(),
            FilterError::Truncated {
                filter: "RunLengthDecode"
            }
        );
    }
}

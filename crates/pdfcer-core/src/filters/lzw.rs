//! # LZWDecode (ISO 32000-1 §7.4.4 + TIFF 6.0 §13) — decompress + un-predict
//!
//! Spec source: `filter__lzw.md` in the PDF-spec RAG, which sources the
//! algorithm from **TIFF 6.0 §13** verbatim (ISO 32000-1's own
//! §7.4.4.2 is two sentences long and describes only the clear-table
//! code, delegating everything else to "the LZW method as adopted by
//! TIFF"). The facts that matter here, all quoted in that file:
//!
//! | Fact | Source |
//! |---|---|
//! | Code 256 = `ClearCode`, 257 = `EndOfInformation`, 258+ = table | TIFF 6.0 §13 |
//! | Characters are **bytes**; initial code width **9 bits** | TIFF 6.0 §13 |
//! | Maximum code width **12 bits** (4096 entries) | TIFF 6.0 §13 |
//! | Packing is **MSB-first**, byte-oriented, endianness-independent | TIFF 6.0 §13 ("`FillOrder` is assumed to be 1") |
//! | Decoder widens at table size 510 / 1022 / 2046 | TIFF 6.0 §13 decoder text |
//! | A stream **begins with `ClearCode`** and **ends with `EndOfInformation`** | TIFF 6.0 §13 + ISO 32000-1 §7.4.4.2 |
//!
//! ## Why a crate rather than the ~200 in-house lines
//!
//! `docs/decisions/005-image-codecs.md` §3.4/§5.3 decided this
//! explicitly and it was the closest call in that record: the spec is
//! fully sourced and the code is small, so in-house was genuinely
//! plausible. `weezl` won on three counts — `#![forbid(unsafe_code)]`
//! plus five cargo-fuzz targets plus 114M downloads is a fuzz history
//! no new implementation has on day one; weezl 0.2.1's own release note
//! is a fixed LZW correctness bug ("overflow into low codes in burst
//! derive") that an in-house decoder would have had to rediscover; and
//! `decode_bytes(inp, out)` is what makes [`MAX_DECODED_LEN`]
//! enforceable **incrementally** rather than as an afterthought.
//!
//! ## `/EarlyChange` is the constructor choice and nothing else
//!
//! Table 8's `EarlyChange` (**default 1**) selects when the decoder
//! widens its code length. Two independently-sourced documents agree on
//! the mapping (`filter__lzw.md` §"`EarlyChange`" derives it from TIFF
//! 6.0 §13's decoder text; weezl's own documentation states it):
//!
//! | `/EarlyChange` | Widens at table size | `weezl` constructor |
//! |---|---|---|
//! | **1 (default)** | 510 / 1022 / 2046 | [`Decoder::with_tiff_size_switch`] — "It switches one symbol sooner" |
//! | 0 | 511 / 1023 / 2047 | [`Decoder::new`] — "compatible with the original specification" |
//!
//! [`BitOrder::Msb`] is **mandatory**: GIF's LZW is LSB-packed and is a
//! structurally different codec (`filter__lzw.md` gotchas). Getting
//! either of these backwards desynchronizes the decoder partway through
//! and produces plausible-looking output that degenerates into garbage —
//! which is why both modes are round-trip tested below.
//!
//! ## Fail-clean, and the two sourced recoveries
//!
//! The module contract in [`super`] applies unchanged: corrupt in →
//! `Err` out, never plausible-looking garbage. Two **non-conformant but
//! recoverable** framings are exceptions, both sourced from
//! `filter__lzw.md`'s gotchas, and both are *counted* rather than
//! silently absorbed (`fuzzy, never sneaky`; decision 005 §4.2):
//!
//! 1. **A stream that does not begin with `ClearCode`.** Non-conformant
//!    per TIFF 6.0 §13 and §7.4.4.2. Recovery: the table is initialized
//!    anyway (which is what `weezl` does regardless) and decoding
//!    proceeds. Counted in [`super::FilterNotes::lzw_framing_anomalies`].
//! 2. **A stream with no `EndOfInformation`.** Common in the wild.
//!    Recovery: terminate on input exhaustion, do not error. Also
//!    counted.
//!
//! Anything else — an invalid code, a code past the end of the table —
//! is [`FilterError::Corrupt`].
//!
//! ## Bomb guard
//!
//! §7.4.4.1 NOTE 2: LZW's best case "approach[es] **1365:1** for long
//! files", so it is a decompression-bomb vector comparable to Flate.
//! Output accumulates in 64 KiB chunks and the decode aborts the moment
//! [`MAX_DECODED_LEN`] is crossed — never decompress-then-check.
//!
//! ## pdfcer never WRITES LZW (rule R28)
//!
//! §7.4.4.1 NOTE 1 makes Flate strictly better on every axis except
//! encode speed. `weezl`'s encoder is compiled in (the crate has no
//! decoder-only feature) and is referenced **only** from this module's
//! `#[cfg(test)]` block, to build known-good fixtures. No shipping code
//! path produces LZW bytes.
//!
//! [`Decoder::with_tiff_size_switch`]: weezl::decode::Decoder::with_tiff_size_switch
//! [`Decoder::new`]: weezl::decode::Decoder::new
//! [`BitOrder::Msb`]: weezl::BitOrder::Msb

use weezl::BitOrder;
use weezl::decode::Decoder;
use weezl::{LzwError, LzwStatus};

use super::{FilterError, FilterNotes, MAX_DECODED_LEN, predictor};
use crate::object::Dict;

/// The LZW code width a PDF/TIFF stream starts at, in bits, expressed
/// the way `weezl` wants it: the width of a *character*, not of a code.
///
/// TIFF 6.0 §13: "The 'characters' that make up the LZW strings are
/// **bytes**", and the first code is 9 bits wide (8-bit alphabet + the
/// two reserved codes). `weezl` derives the 9-bit initial code width
/// from this 8. It is fixed for PDF — unlike GIF, where the initial
/// code size varies with the image's bit depth.
const LZW_CHARACTER_BITS: u8 = 8;

/// Decode `data` as LZW and apply any predictor from `parms`.
///
/// Table 8 parameters: `EarlyChange` (LZW-only, default 1) selects the
/// code-width switch point; `Predictor`/`Colors`/`BitsPerComponent`/
/// `Columns` are handled by the one shared [`predictor`] implementation,
/// identically to FlateDecode.
///
/// `notes` accumulates the two recoverable framing anomalies described
/// in the module docs. It is a `&mut` out-parameter rather than part of
/// the return type so that [`super::decode_stream`]'s
/// bytes-in-bytes-out shape — which every other byte-stream filter
/// shares — is preserved (decision 005 R23: LZW *is* a byte-stream
/// filter, not an image codec).
///
/// # Errors
///
/// [`FilterError::Corrupt`] for an invalid code, [`FilterError::OutputTooLarge`]
/// when [`MAX_DECODED_LEN`] is crossed mid-decode, and whatever
/// [`predictor::unpredict`] reports for inconsistent predictor
/// parameters or ragged rows.
pub fn decode(
    data: &[u8],
    parms: Option<&Dict>,
    notes: &mut FilterNotes,
) -> Result<Vec<u8>, FilterError> {
    let decompressed = decompress_bounded(data, early_change(parms), notes)?;
    match predictor::Params::from_dict(parms)? {
        None => Ok(decompressed),
        Some(p) => predictor::unpredict(decompressed, &p),
    }
}

/// Read Table 8's `EarlyChange`, defaulting to **1**.
///
/// The default matters more than most: §7.4.4.3 says the parameter
/// "is included because LZW sample code distributed by some vendors
/// increases the code length one code earlier than necessary", and
/// `filter__lzw.md` records the consequence — an implementation
/// supporting only `EarlyChange 0` "will fail on almost every real LZW
/// stream."
///
/// Table 8 defines only the values 0 and 1. Any other value is
/// undefined by the spec; pdfcer treats it as the default (1) rather
/// than refusing the stream, because a producer typo in a parameter
/// with a well-known default is not a reason to drop an otherwise
/// decodable image. Only an explicit `0` selects the postponed mode.
fn early_change(parms: Option<&Dict>) -> bool {
    parms
        .and_then(|d| d.get(b"EarlyChange"))
        .and_then(crate::object::Object::as_int)
        .is_none_or(|v| v != 0)
}

/// Incremental, ceiling-bounded LZW decompression (module docs).
///
/// The loop is deliberately shaped like [`super::flate`]'s: a fixed
/// scratch chunk, the ceiling checked against *produced* bytes before
/// they are appended, and explicit detection of "no progress possible"
/// so a truncated stream cannot spin.
fn decompress_bounded(
    data: &[u8],
    early_change: bool,
    notes: &mut FilterNotes,
) -> Result<Vec<u8>, FilterError> {
    // 64 KiB, matching flate.rs: large enough to amortize the call
    // overhead, small enough that the ceiling overshoot is negligible.
    const CHUNK: usize = 64 * 1024;

    if !begins_with_clear_code(data) {
        // Non-conformant framing #1 (module docs). weezl initializes its
        // table on construction regardless, so decoding proceeds; the
        // divergence is reported, not hidden.
        notes.lzw_framing_anomalies += 1;
    }

    let mut decoder = if early_change {
        Decoder::with_tiff_size_switch(BitOrder::Msb, LZW_CHARACTER_BITS)
    } else {
        Decoder::new(BitOrder::Msb, LZW_CHARACTER_BITS)
    };

    let mut out: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; CHUNK];
    let mut consumed = 0usize;

    loop {
        let remaining = data.get(consumed..).unwrap_or(&[]);
        let result = decoder.decode_bytes(remaining, &mut chunk);

        let status = match result.status {
            Ok(status) => status,
            Err(LzwError::InvalidCode) => {
                return Err(FilterError::Corrupt {
                    filter: "LZWDecode",
                    detail: "invalid code in LZW stream".to_owned(),
                });
            }
        };

        if out.len().saturating_add(result.consumed_out) > MAX_DECODED_LEN {
            return Err(FilterError::OutputTooLarge);
        }
        out.extend_from_slice(chunk.get(..result.consumed_out).unwrap_or(&[]));
        consumed = consumed.saturating_add(result.consumed_in);

        match status {
            // `EndOfInformation` reached — the conformant end.
            LzwStatus::Done => return Ok(out),
            // Input exhausted with no end marker, or the decoder cannot
            // advance. Non-conformant framing #2 (module docs): the
            // sourced recovery is to terminate on input exhaustion
            // WITHOUT erroring, because a missing EOI is common in real
            // files and the bytes already produced are correct.
            LzwStatus::NoProgress => {
                notes.lzw_framing_anomalies += 1;
                return Ok(out);
            }
            LzwStatus::Ok => {
                if result.consumed_in == 0 && result.consumed_out == 0 {
                    // Defensive: `Ok` with zero movement would spin.
                    // weezl reports that as `NoProgress`, so this arm is
                    // unreachable in practice — but a decoder loop that
                    // *can* spin on hostile input is exactly the kind of
                    // thing ARCHITECTURE.md §10 exists to prevent, so
                    // the guard is not left to the vendor's promise.
                    notes.lzw_framing_anomalies += 1;
                    return Ok(out);
                }
            }
        }
    }
}

/// Does the stream begin with the 9-bit `ClearCode` (256)?
///
/// TIFF 6.0 §13: "each strip begins with a `ClearCode`"; ISO 32000-1
/// §7.4.4.2: "The encoder shall **begin by issuing a clear-table
/// code**." Packing is MSB-first, so code 256 as a 9-bit value
/// (`1_0000_0000`) occupies the whole first byte as `0x80` plus one
/// clear high bit of the second byte.
///
/// A stream too short to hold a 9-bit code cannot begin with one, so it
/// reports `false` — which is the honest answer and costs only a
/// counted diagnostic.
fn begins_with_clear_code(data: &[u8]) -> bool {
    match (data.first(), data.get(1)) {
        (Some(0x80), Some(second)) => second & 0x80 == 0,
        _ => false,
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

    /// Build an LZW stream with weezl's own encoder.
    ///
    /// **Test-only** (rule R28: pdfcer ships no LZW encoder). `weezl`'s
    /// encoder is the natural fixture generator because the two
    /// constructors mirror the decoder's exactly, so `EarlyChange`
    /// round-trips are testable without hand-assembling bit streams —
    /// and because a fixture built by the *same* library the decoder
    /// uses still catches a wrong `BitOrder` or a wrong constructor
    /// choice on pdfcer's side, which is what these tests are for.
    fn lzw_encode(data: &[u8], early_change: bool) -> Vec<u8> {
        use weezl::encode::Encoder;
        let mut enc = if early_change {
            Encoder::with_tiff_size_switch(BitOrder::Msb, LZW_CHARACTER_BITS)
        } else {
            Encoder::new(BitOrder::Msb, LZW_CHARACTER_BITS)
        };
        enc.encode(data).unwrap()
    }

    fn parms_with(entries: &[(&[u8], i64)]) -> Dict {
        let mut d = Dict::new();
        for (k, v) in entries {
            d.insert(Name::from(*k), Object::Integer(*v));
        }
        d
    }

    #[test]
    fn roundtrip_early_change_default() {
        // Table 8: EarlyChange defaults to 1, so ABSENT parms must
        // select the tiff-size-switch decoder. Long enough (>510
        // distinct table entries' worth of input) that the 9→10 bit
        // widening actually happens — a short fixture would pass with
        // either constructor and prove nothing.
        let original: Vec<u8> = (0..40_000u32)
            .map(|i| (i.wrapping_mul(37) >> 3) as u8)
            .collect();
        let encoded = lzw_encode(&original, true);
        let mut notes = FilterNotes::default();
        assert_eq!(decode(&encoded, None, &mut notes).unwrap(), original);
        assert_eq!(
            notes.lzw_framing_anomalies, 0,
            "weezl emits a conformant ClearCode + EndOfInformation framing"
        );
    }

    #[test]
    fn roundtrip_early_change_one_explicit() {
        let original = b"the quick brown fox jumps over the lazy dog. ".repeat(400);
        let encoded = lzw_encode(&original, true);
        let parms = parms_with(&[(b"EarlyChange", 1)]);
        let mut notes = FilterNotes::default();
        assert_eq!(
            decode(&encoded, Some(&parms), &mut notes).unwrap(),
            original
        );
    }

    #[test]
    fn roundtrip_early_change_zero() {
        // "postponed as long as possible" — the OTHER constructor.
        let original = b"the quick brown fox jumps over the lazy dog. ".repeat(400);
        let encoded = lzw_encode(&original, false);
        let parms = parms_with(&[(b"EarlyChange", 0)]);
        let mut notes = FilterNotes::default();
        assert_eq!(
            decode(&encoded, Some(&parms), &mut notes).unwrap(),
            original
        );
    }

    #[test]
    fn early_change_modes_are_not_interchangeable() {
        // THE regression test for the mapping in the module table. A
        // stream long enough to cross the first widening threshold
        // decodes to something DIFFERENT (or fails) under the wrong
        // mode — if this ever passes, the two constructors have been
        // wired to the same thing.
        let original: Vec<u8> = (0..40_000u32)
            .map(|i| (i.wrapping_mul(37) >> 3) as u8)
            .collect();
        let encoded = lzw_encode(&original, false);
        let wrong = parms_with(&[(b"EarlyChange", 1)]);
        let mut notes = FilterNotes::default();
        let got = decode(&encoded, Some(&wrong), &mut notes);
        assert!(
            got.map(|v| v != original).unwrap_or(true),
            "EarlyChange 1 must NOT decode an EarlyChange-0 stream correctly"
        );
    }

    #[test]
    fn corrupt_stream_errs_never_passes_raw_bytes() {
        // The fail-clean contract (decision 001 §6.1 item 4). 0xFF bytes
        // decode to high codes that cannot exist in a freshly cleared
        // table.
        let garbage = vec![0xFFu8; 64];
        let mut notes = FilterNotes::default();
        let result = decode(&garbage, None, &mut notes);
        assert!(
            matches!(result, Err(FilterError::Corrupt { .. })),
            "got {result:?}"
        );
    }

    #[test]
    fn missing_clear_code_is_recovered_and_counted() {
        // filter__lzw.md gotcha: non-conformant per TIFF 6.0 §13, but
        // recoverable. Strip weezl's leading ClearCode by re-encoding
        // the payload and hand-building a stream that starts straight
        // at a literal: a single 9-bit code 0x41 ('A') then EOI (257).
        //   0 0100 0001  1 0000 0001  -> 0x20 0xC0 0x40 (zero-padded)
        let stream = [0x20u8, 0xC0, 0x40];
        let mut notes = FilterNotes::default();
        let out = decode(&stream, None, &mut notes).unwrap();
        assert_eq!(out, b"A");
        assert_eq!(notes.lzw_framing_anomalies, 1);
    }

    #[test]
    fn missing_end_of_information_terminates_without_error() {
        // filter__lzw.md gotcha: "A missing EndOfInformation at end of
        // stream is common. Terminate on input exhaustion; do not
        // error."  ClearCode (256) then 'A' (0x41), no EOI:
        //   1 0000 0000  0 0100 0001  -> 0x80 0x10 0x40 (padded)
        let stream = [0x80u8, 0x10, 0x40];
        let mut notes = FilterNotes::default();
        let out = decode(&stream, None, &mut notes).unwrap();
        assert_eq!(out, b"A");
        assert_eq!(notes.lzw_framing_anomalies, 1, "no EOI is one anomaly");
    }

    #[test]
    fn empty_stream_is_empty_output_and_an_anomaly() {
        let mut notes = FilterNotes::default();
        assert!(decode(&[], None, &mut notes).unwrap().is_empty());
        assert!(notes.lzw_framing_anomalies > 0);
    }

    #[test]
    fn decompression_bomb_is_aborted_at_ceiling() {
        // LZW's best case is ~1365:1 (§7.4.4.1 NOTE 2). A long run of
        // one byte compresses to a fraction of the ceiling and must
        // abort mid-decode rather than allocate the full claim.
        let bomb_plain = vec![0u8; MAX_DECODED_LEN + 1024];
        let bomb = lzw_encode(&bomb_plain, true);
        assert!(bomb.len() < 4 * 1024 * 1024, "bomb should compress tiny");
        let mut notes = FilterNotes::default();
        assert_eq!(
            decode(&bomb, None, &mut notes).unwrap_err(),
            FilterError::OutputTooLarge
        );
    }

    #[test]
    fn predictor_applies_over_lzw_output() {
        // §7.4.4.4: predictors apply identically to LZW and Flate
        // output. PNG Up (tag 2) over two 4-byte rows, exactly the
        // shape flate.rs's twin test uses.
        let filtered: Vec<u8> = [&[2u8][..], &[1, 2, 3, 4], &[2], &[10, 20, 30, 40]].concat();
        let encoded = lzw_encode(&filtered, true);
        let parms = parms_with(&[(b"Predictor", 12), (b"Columns", 4)]);
        let mut notes = FilterNotes::default();
        let out = decode(&encoded, Some(&parms), &mut notes).unwrap();
        assert_eq!(out, vec![1, 2, 3, 4, 11, 22, 33, 44]);
    }

    #[test]
    fn undefined_early_change_value_falls_back_to_the_default() {
        let original = b"abcabcabcabc".repeat(50);
        let encoded = lzw_encode(&original, true);
        let parms = parms_with(&[(b"EarlyChange", 7)]);
        let mut notes = FilterNotes::default();
        assert_eq!(
            decode(&encoded, Some(&parms), &mut notes).unwrap(),
            original
        );
    }

    #[test]
    fn clear_code_detection_matches_msb_packing() {
        assert!(begins_with_clear_code(&[0x80, 0x00]));
        assert!(begins_with_clear_code(&[0x80, 0x7F]));
        assert!(!begins_with_clear_code(&[0x80, 0x80]));
        assert!(!begins_with_clear_code(&[0x00, 0x00]));
        assert!(!begins_with_clear_code(&[0x80]));
        assert!(!begins_with_clear_code(&[]));
    }
}

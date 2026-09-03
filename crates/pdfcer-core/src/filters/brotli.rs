//! `/BrotliDecode` — Brotli decompression (EXTN-BROTLI-1 v1.3 §5).
//!
//! # ★ THIS FILTER IS NOT IN ISO 32000-2, AND THE FIRST THING A READER MEETS IS A FALSE CITATION
//!
//! Every web-search summary of this filter asserts it is *"specified in ISO
//! 32000-2:2020 §7.4.11"*. **That clause does not exist.** The string `7.4.11`
//! returns zero hits across the standard's 1,023 pages, §7.4 stops at 7.4.10
//! (`Crypt`), and **Table 6 enumerates exactly ten filters** — in both the
//! 2008 and 2020 editions. A scan of all 2,840 Errata Collection 3 markup
//! annotations returns nothing Brotli-related either, so the negative holds
//! for the corrected text and not merely for a text extractor's view of it.
//! The false citation traces to the body of an unmerged pypdf PR (#3254);
//! pypdf's own maintainer-written issue (#3223) says the opposite and is
//! right.
//!
//! What actually exists is **`EXTN-BROTLI-1 v1.3`, *Brotli compression in PDF
//! 2.0***, a **PDF Association PDF TWG extension** announced 2026-08-19,
//! CC-BY-4.0, registered under developer prefix `PDFa` with `BaseVersion 2.0`
//! / `ExtensionLevel 1`. ISO/TC 171/SC 2's catalogue has **no** Brotli work
//! item — there is no amendment and no dated path. Full sourcing, including
//! the one-command refutation of the false clause:
//! `D:\Dev\Rag-Specialized\PDF_Spec\filters\filter__brotli.md`.
//!
//! Consequence for how this module is read: its `SHALL`s bind conformance **to
//! the extension**, not to ISO 32000-2. A file using this filter is not
//! ISO-conformant PDF 2.0 by virtue of using it.
//!
//! # The decode contract, and the four rules that bite
//!
//! | # | Rule | Modality |
//! |---|---|---|
//! | BR-1 | Brotli "in accordance with **IETF RFC 7932**" | the definition |
//! | BR-2 | **Large window Brotli (RFC 9841) SHALL be supported** | `SHALL` |
//! | BR-3 | **RFC 9841's framing format SHALL NOT be used** | `SHALL NOT` |
//! | BR-4 | Shared dictionaries — LZ77-prefix and custom static word/transform — are "specifically not supported in PDF" | negative |
//!
//! **BR-2 and BR-3 are easy to conflate and mean opposite things.** RFC 9841
//! defines *both* a large-window mode and a framing format; the extension
//! takes the first and refuses the second. So a decoder needs large-window
//! support **enabled** while the stream carries **raw** Brotli, never RFC 9841
//! frames. `brotli-decompressor` handles large windows natively — its
//! `kBrotliLargeMaxWbits` is 30 — and this module never wraps or unwraps a
//! frame, which is BR-3 satisfied by construction rather than by a check.
//!
//! ★ **`FlateDecode`'s predictors apply VERBATIM**, and this is the rule that
//! makes the work small. The extension retitles Table 8 to include Brotli, so
//! [`super::predictor`] is reused **unchanged** — there is no Brotli variant
//! of a predictor and there must not be one.
//!
//! # What this module deliberately does NOT do
//!
//! **No encoder.** Emitting Brotli interacts with project rule 3
//! (round-trip / minimal-diff): an untouched Brotli stream must be re-emitted
//! byte-identical, which is a byte-copy question rather than a re-compress
//! one, and re-compressing at a different quality level would rewrite streams
//! pdfcer never edited. That is a separate and larger decision, deliberately
//! not taken here. The crate's compressor half is not even compiled in — see
//! the feature list in `Cargo.toml`.
//!
//! **No inline-image support.** §5.2: *"`BrotliDecode` **SHALL NOT** be used
//! for inline images"*, and no Table 92 abbreviation exists for it. The
//! refusal lives at the dispatch in [`super`], not here, because that is where
//! inline-image context is known.

use ::brotli::{BrotliDecompressStream, BrotliResult, BrotliState, HeapAlloc, HuffmanCode};

use super::{FilterError, MAX_DECODED_LEN, predictor};
use crate::object::Dict;

/// Decompress `data` and apply any predictor from `parms`.
///
/// The predictor parameters are Table 8's, read by the **same**
/// [`super::predictor`] code `FlateDecode` and `LZWDecode` use — see the
/// module docs for why that sharing is required rather than convenient.
/// (`EarlyChange` is LZW-only and is ignored here, exactly as it is for
/// Flate.)
///
/// # Errors
///
/// [`FilterError`] — corrupt or truncated Brotli data, the output ceiling
/// crossed, or invalid/inconsistent predictor parameters.
pub fn decode(data: &[u8], parms: Option<&Dict>) -> Result<Vec<u8>, FilterError> {
    let plain = decompress_bounded(data)?;
    match predictor::Params::from_dict(parms)? {
        None => Ok(plain),
        Some(p) => predictor::unpredict(plain, &p),
    }
}

/// Incremental, ceiling-bounded Brotli decompression.
///
/// # Why the raw state machine and not `brotli::Decompressor`
///
/// For the reason `flate::inflate_bounded` gives, which applies identically
/// here: the `std::io::Read` wrapper reports a **truncated** stream as a clean
/// `Ok(0)` EOF, which is indistinguishable from success. That is the
/// silent-partial-data failure the fail-clean contract forbids — a truncated
/// content stream would decode to a *prefix* and render as a page that is
/// simply missing its end, with no error anywhere. With the raw API
/// completeness is explicit: only [`BrotliResult::ResultSuccess`] is success,
/// and input exhausted while the decoder still wants more is
/// [`FilterError::Truncated`].
///
/// # The ceiling
///
/// `ARCHITECTURE.md` §10 requires every filter to bound its output.
/// Brotli's worst-case expansion is far higher than deflate's ~1032:1 —
/// a small window of literals can be repeated across a 16 MB large window —
/// so an unbounded decoder is a decompression bomb with a very short fuse.
/// The check is applied **per chunk, before extending**, so a hostile stream
/// costs at most [`MAX_DECODED_LEN`] plus one chunk rather than whatever it
/// claimed.
fn decompress_bounded(data: &[u8]) -> Result<Vec<u8>, FilterError> {
    // 64 KiB, matching `flate::inflate_bounded`: large enough to amortize the
    // per-call overhead, small enough that the ceiling overshoot is one chunk.
    const CHUNK: usize = 64 * 1024;

    // Three allocators, one per arena the decoder keeps (bytes, u32s and
    // Huffman table entries). Spelled with explicit types because the
    // `HuffmanCode` arena's default value is a struct rather than a zero, so
    // inference cannot pick it.
    let mut state = BrotliState::new(
        HeapAlloc::<u8>::new(0),
        HeapAlloc::<u32>::new(0),
        HeapAlloc::<HuffmanCode>::new(HuffmanCode::default()),
    );
    let mut out: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; CHUNK];

    let mut available_in = data.len();
    let mut input_offset = 0usize;
    let mut total_out = 0usize;

    loop {
        let mut available_out = CHUNK;
        let mut output_offset = 0usize;
        let status = BrotliDecompressStream(
            &mut available_in,
            &mut input_offset,
            data,
            &mut available_out,
            &mut output_offset,
            &mut chunk,
            &mut total_out,
            &mut state,
        );

        // `output_offset` is how far into `chunk` this call wrote.
        let produced = output_offset;
        if out.len().saturating_add(produced) > MAX_DECODED_LEN {
            return Err(FilterError::OutputTooLarge);
        }
        out.extend_from_slice(chunk.get(..produced).unwrap_or(&[]));

        match status {
            BrotliResult::ResultSuccess => return Ok(out),
            // The decoder wants bytes that are not there. Distinguished from
            // corruption deliberately: a truncated stream is the common
            // real-world damage (a clipped download, a bad byte range) and an
            // operator can act on that differently from "these bytes are not
            // Brotli at all".
            BrotliResult::NeedsMoreInput => {
                return Err(FilterError::Truncated {
                    filter: "BrotliDecode",
                });
            }
            BrotliResult::ResultFailure => {
                return Err(FilterError::Corrupt {
                    filter: "BrotliDecode",
                    detail: "brotli stream rejected by the decoder".to_owned(),
                });
            }
            // More output room wanted: loop and give it another chunk. Guarded
            // against a decoder that asks for room and then writes nothing,
            // which would otherwise spin forever on a crafted stream — the
            // ceiling alone would not stop it, because a loop producing zero
            // bytes never reaches the ceiling.
            BrotliResult::NeedsMoreOutput => {
                if produced == 0 {
                    return Err(FilterError::Corrupt {
                        filter: "BrotliDecode",
                        detail: "decoder requested more output room without \
                                 producing any bytes"
                            .to_owned(),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Compress with the reference encoder so the fixtures in these tests are
    /// real Brotli rather than hand-assembled bytes.
    ///
    /// This is a **test-only** use of the encoder. The shipped filter decodes
    /// only — see the module docs on why an encoder is a separate decision.
    fn compress(plain: &[u8]) -> Vec<u8> {
        compress_at(plain, ::brotli::enc::BrotliEncoderParams::default().quality)
    }

    /// As [`compress`], at an explicit quality level.
    ///
    /// ★ The ceiling test needs a stream that decodes to more than 256 MiB,
    /// and the encoder's DEFAULT quality is 11 — the slowest setting it has.
    /// Encoding a quarter-gigabyte at quality 11 took **88 seconds**, in a
    /// suite this project runs on every change. Quality 1 produces a stream
    /// that is just as far over the ceiling, because the input is a run of
    /// zeros and every quality level compresses that to nearly nothing; the
    /// only thing the high setting bought was the wait.
    fn compress_at(plain: &[u8], quality: i32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut input = plain;
        let params = ::brotli::enc::BrotliEncoderParams {
            quality,
            ..Default::default()
        };
        ::brotli::BrotliCompress(&mut input, &mut out, &params)
            .expect("reference encoder compresses");
        out
    }

    #[test]
    fn a_round_trip_returns_the_original_bytes() {
        let plain = b"pdfcer brotli round trip, with enough text to actually compress. \
                      pdfcer brotli round trip, with enough text to actually compress."
            .repeat(4);
        let decoded = decode(&compress(&plain), None).expect("decodes");
        assert_eq!(decoded, plain);
    }

    /// ★ The assertion the `Read`-wrapper API would have failed.
    ///
    /// A truncated stream must be an ERROR, never a short read. If this ever
    /// returns `Ok`, pdfcer is silently rendering a prefix of a content stream
    /// and calling it a page — the exact failure the module docs give as the
    /// reason for using the raw state machine.
    #[test]
    fn a_truncated_stream_is_an_error_and_not_a_short_read() {
        let plain = b"enough bytes that the encoder emits a multi-part stream".repeat(64);
        let full = compress(&plain);
        let cut = full
            .get(..full.len() / 2)
            .expect("half of a slice is in bounds");
        match decode(cut, None) {
            Err(FilterError::Truncated { filter }) => assert_eq!(filter, "BrotliDecode"),
            Err(other) => panic!("truncation must be Truncated, got {other:?}"),
            Ok(v) => panic!(
                "a truncated stream decoded to {} bytes instead of erroring — \
                 this is the silent-partial-data failure",
                v.len()
            ),
        }
    }

    #[test]
    fn garbage_is_rejected_rather_than_guessed_at() {
        let err = decode(&[0xff_u8; 64], None).unwrap_err();
        assert!(
            matches!(
                err,
                FilterError::Corrupt {
                    filter: "BrotliDecode",
                    ..
                } | FilterError::Truncated {
                    filter: "BrotliDecode"
                }
            ),
            "expected a Brotli-attributed refusal, got {err:?}"
        );
    }

    /// The ceiling is a real bound, not a comment.
    ///
    /// A highly-compressible input larger than [`MAX_DECODED_LEN`] must be
    /// refused rather than allocated — Brotli's expansion ratio makes this the
    /// decompression-bomb case, and it is asserted rather than trusted.
    #[test]
    fn the_output_ceiling_refuses_a_bomb() {
        let plain = vec![0u8; MAX_DECODED_LEN + (1 << 20)];
        let err = decode(&compress_at(&plain, 1), None).unwrap_err();
        assert!(
            matches!(err, FilterError::OutputTooLarge),
            "expected OutputTooLarge, got {err:?}"
        );
    }
}

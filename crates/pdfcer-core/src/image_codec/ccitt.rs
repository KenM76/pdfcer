//! # CCITTFaxDecode (ISO 32000-1 §7.4.6, Table 11; ITU-T T.4/T.6) — the fax adapter
//!
//! Spec source: `filters/filter__ccitt.md` in the PDF-spec RAG, whose
//! Table 11 quotation was **verified against the source PDF on
//! 2026-07-30** (printed pp. 30–31) — including the three defaults that
//! decision 005 §4.3 held as a *blocking prerequisite* for this Pass
//! because they had been recalled rather than read (`Columns` 1728,
//! `EndOfBlock` true, `BlackIs1` false; all three turned out correct,
//! but they are now sourced). The codec itself is delegated by ISO
//! 32000-1 to ITU-T T.4 and T.6 — §7.4.6 NOTE 2, verbatim: "The
//! encoding algorithm is not described in detail in this standard but
//! can be found in ITU Recommendations T.4 and T.6."
//!
//! Crate choice is decision 005 §4.3: **`hayro-ccitt`**, with `fax` as
//! the named fallback.
//!
//! ## What this module is, and is not
//!
//! An **adapter**. `hayro-ccitt` does the Huffman run-length decoding,
//! the 2-D vertical/pass/horizontal mode machine, the EOL/EOFB/RTC
//! scanning and the reference-line bookkeeping. Everything here is
//! Table 11 → [`DecodeSettings`] translation, resource ceilings and
//! geometry reconciliation — which is precisely where pdfcer's own bugs
//! would live, and therefore where the doc comments, the tests and the
//! `image_codec_ccitt` fuzz target aim (decision 005 §6.5).
//!
//! ## Table 11 → `DecodeSettings`, one row at a time
//!
//! The mapping is deliberately 1:1 — that property is *why* decision 005
//! §3.5 chose this crate over `fax`, whose TIFF-shaped API would have
//! left the trichotomy, the byte alignment and the polarity as pdfcer
//! glue rather than as fuzzed vendor code.
//!
//! | Table 11 key | Default | `DecodeSettings` field | Note |
//! |---|---|---|---|
//! | `K` | 0 | `encoding` | trichotomy — see below |
//! | `EndOfLine` | false | `end_of_line` | advisory; the filter "shall always accept" EOL patterns |
//! | `EncodedByteAlign` | false | `rows_are_byte_aligned` | |
//! | `Columns` | **1728** | `columns` | the ITU-T A4 scan width, *not* `/Width` |
//! | `Rows` | 0 | `rows` | 0 ⇒ fall back to the image dictionary's `/Height` |
//! | `EndOfBlock` | **true** | `end_of_block` | overrides `Rows` |
//! | `BlackIs1` | **false** | `invert_black` | the polarity trap — see below |
//! | `DamagedRowsBeforeError` | 0 | *(none)* | unimplemented; named diagnostic (R27) |
//!
//! ### `K` is trichotomous, and only trichotomous
//!
//! Table 11, verbatim: "The filter shall **distinguish among negative,
//! zero, and positive values of K** to determine how to interpret the
//! encoded data; however, it **shall not distinguish between different
//! positive K values**." So `K = 4` and `K = 40` select the same decoder
//! path, and building a per-`K` table would be a bug dressed as
//! thoroughness. [`encoding_mode`] is that rule and nothing else.
//!
//! ### `BlackIs1` is the polarity trap
//!
//! Table 11 describes the flag as "whether **1 bits shall be interpreted
//! as black pixels and 0 bits as white pixels, the reverse of the normal
//! PDF convention for image data**", default **false**. Two consequences
//! that compound:
//!
//! 1. The *normal* PDF convention is therefore `0 = black` — which is
//!    what the DeviceGray default `Decode [0 1]` produces at 1 bit per
//!    component (sample 0 → grey 0.0). So with `BlackIs1` absent, this
//!    filter must emit **0 for a black pixel**.
//! 2. T.4/T.6 speak in "white runs" and "black runs", not in bit values.
//!    `hayro-ccitt` reports each pixel through its sink as a `white`
//!    flag, already XORed with its `invert_black` setting. pdfcer's
//!    [`BilevelSink`] writes a `1` bit for white. Composing those two
//!    facts gives the mapping this module uses:
//!    **`invert_black = BlackIs1`** — the direct assignment, not the
//!    negation.
//!
//! Getting this backwards renders every fax image as its own negative,
//! which looks deliberate rather than broken. It is the single most
//! likely correctness bug in this filter, so it is pinned from both
//! sides by `mod.rs`'s `ccitt_black_is_1_inverts_every_sample` test:
//! the same codestream is decoded with `/BlackIs1` false and true, and
//! the two results must be exact bitwise complements.
//!
//! `/BlackIs1` is also the **one named exception** to rule R26 ("the
//! codec layer never decides colour"): it is a Table 11 *filter
//! parameter*, so it belongs to the adapter, unlike `/Decode`, which
//! stays `pdfcer-render`'s alone.
//!
//! ## Where `Rows` comes from when it is absent
//!
//! Table 11: "**If the value is 0 or absent, the image's height is not
//! predetermined**, and the encoded data shall be terminated by an
//! end-of-block bit pattern or by the end of the filter's data."
//!
//! `hayro-ccitt` has no "unknown height" mode — its Group 4 loop
//! terminates on `decoded_rows == settings.rows`, so passing `0` would
//! decode **zero rows**. pdfcer therefore supplies a bound, in the order
//! decision 005 §1.2 named as one of the reasons this codec needs the
//! image dictionary at all:
//!
//! 1. `/Rows`, when positive;
//! 2. otherwise the image dictionary's `/Height`, which is Required by
//!    Table 89 and is what the image actually is;
//! 3. otherwise [`row_ceiling`] — a pdfcer-derived bound, never an
//!    invented "sensible" number (rule R25).
//!
//! In every case the decoder still stops early on EOFB/RTC or on
//! exhausted input, so the bound is a ceiling and not an assertion. The
//! rows actually produced are what [`CodedImage::height`] reports.
//!
//! ## Resource ceilings are pdfcer's (rule R25)
//!
//! `hayro-ccitt` has **no** ceilings of its own: it allocates strictly
//! from the `columns`/`rows` it is handed, which puts the whole burden
//! here. Both are checked against [`MAX_IMAGE_DIMENSION`] and
//! [`MAX_IMAGE_PIXELS`] *before* decoding, and [`BilevelSink`] enforces
//! a byte budget *during* decoding so an EOFB-less stream with an
//! over-large row bound cannot allocate past it.

use hayro_ccitt::{DecodeError, DecodeSettings, DecoderContext, EncodingMode};

use super::bilevel::BilevelSink;
use super::{
    Codec, CodecColorModel, CodecNotes, CodedImage, ImageCodecError, MAX_IMAGE_DIMENSION,
    MAX_IMAGE_PIXELS, MAX_IMAGE_SAMPLE_BYTES,
};
// decision 018: the codecs resolve indirect entries through a `DocumentView`
// rather than a `&Document`, so an image whose dictionary lives in an
// editing session decodes as the operator currently has it. `Document` is
// still named by the back-compat `decode_image` wrapper in `mod.rs`.
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::view::DocumentView;

/// Table 11's parameters, resolved against their verified defaults.
///
/// A struct rather than eight locals so [`settings`] can be unit-tested
/// against the table directly, and so a reviewer can compare the
/// defaults with `filter__ccitt.md` in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Params {
    /// `K` — the encoding-scheme selector. Signed on purpose: the sign
    /// *is* the selector.
    k: i64,
    /// `EndOfLine`, default false.
    end_of_line: bool,
    /// `EncodedByteAlign`, default false.
    encoded_byte_align: bool,
    /// `Columns`, default 1728 (the ITU-T A4 scan width).
    columns: i64,
    /// `Rows`, default 0 ("height is not predetermined").
    rows: i64,
    /// `EndOfBlock`, default **true**.
    end_of_block: bool,
    /// `BlackIs1`, default **false** (i.e. 0 = black).
    black_is_1: bool,
    /// `DamagedRowsBeforeError`, default 0.
    damaged_rows_before_error: i64,
}

impl Default for Params {
    /// Table 11's defaults, verbatim from the verified extraction. These
    /// are what pdfcer applies when a key is **absent**, regardless of
    /// what `hayro-ccitt`'s own API would default to — `filter__ccitt.md`
    /// says to verify the crate's defaults against this table rather
    /// than assume they match, and [`DecodeSettings`] has no `Default`
    /// impl at all, so every field below is set explicitly.
    fn default() -> Self {
        Self {
            k: 0,
            end_of_line: false,
            encoded_byte_align: false,
            columns: 1728,
            rows: 0,
            end_of_block: true,
            black_is_1: false,
            damaged_rows_before_error: 0,
        }
    }
}

/// Decode a `CCITTFaxDecode` codestream.
///
/// `data` is the codestream *after* any byte-stream filter prefix;
/// `parms` is the codec's own `/DecodeParms` entry; `dict` is the image
/// dictionary, consulted for the `/Rows` → `/Height` fallback and for
/// geometry reconciliation; `notes` accumulates the honesty counters.
///
/// # Errors
///
/// [`ImageCodecError::TooLarge`] when `Columns`/`Rows` cross a pdfcer
/// ceiling or the sink's byte budget is exhausted mid-decode;
/// [`ImageCodecError::FeatureUnsupported`] with the key
/// `"CCITT/damaged-rows"` when the stream is damaged *and* the file
/// asked for damage tolerance pdfcer does not implement (rule R27);
/// [`ImageCodecError::Corrupt`] for a malformed codestream or a
/// nonsensical `/Columns`.
pub(super) fn decode(
    doc: &DocumentView<'_>,
    data: &[u8],
    parms: Option<&Dict>,
    dict: &Dict,
    notes: &mut CodecNotes,
) -> Result<CodedImage, ImageCodecError> {
    let params = params(doc, parms);

    // `Columns` sizes every row-relative index in the sink, so it is
    // validated before anything else. Table 11 gives no upper bound; the
    // ceiling is pdfcer's (rule R25).
    let columns = u32::try_from(params.columns)
        .ok()
        .filter(|&c| c > 0)
        .ok_or_else(|| corrupt("/Columns is not a positive integer"))?;
    if columns > MAX_IMAGE_DIMENSION {
        return Err(ImageCodecError::TooLarge);
    }

    let ceiling = row_ceiling(columns);
    let rows = declared_rows(doc, &params, dict).unwrap_or(ceiling);
    if u64::from(columns).saturating_mul(u64::from(rows)) > MAX_IMAGE_PIXELS {
        return Err(ImageCodecError::TooLarge);
    }
    let rows = rows.min(ceiling);

    let mut sink = BilevelSink::new(columns, rows);
    let mut ctx = DecoderContext::new(settings(&params, columns, rows));
    let outcome = hayro_ccitt::decode(data, &mut sink, &mut ctx);

    // The budget check comes FIRST: an overflow means the decoder was
    // still producing rows when pdfcer stopped accepting them, so any
    // error it went on to report is a consequence of the truncation and
    // not the interesting failure.
    if sink.overflowed() {
        return Err(ImageCodecError::TooLarge);
    }
    let decoded_rows = sink.rows();
    outcome.map_err(|err| decode_error(&params, err))?;

    let (samples, height) = sink.finish();
    debug_assert_eq!(height, decoded_rows);
    if height == 0 {
        return Err(corrupt("no scan lines decoded"));
    }

    notes.geometry_mismatch = geometry_disagrees(doc, dict, columns, height);

    Ok(CodedImage {
        samples,
        codec: Some(Codec::Ccitt),
        width: columns,
        height,
        components: 1,
        // §8.9.5 Table 89: "a CCITTFaxDecode or JBIG2Decode filter shall
        // always deliver 1-bit samples". Not an assumption — the filter
        // has no other output shape, since T.4/T.6 code bi-level runs.
        bits_per_component: 1,
        color_model: CodecColorModel::Bilevel,
        // T.4/T.6 carry no colour information of any kind, so there is
        // no embedded profile to reconcile against `/ColorSpace`.
        icc_profile: None,
        // Only JPXDecode carries alpha inside the codestream
        // (`/SMaskInData`, Table 89); a fax codestream has one bilevel
        // component and nothing else.
        embedded_alpha: None,
        notes: *notes,
    })
}

// ---------------------------------------------------------------------------
// Table 11 → DecodeSettings
// ---------------------------------------------------------------------------

/// Read Table 11's eight parameters, applying the verified defaults.
///
/// Values of the wrong COS type are treated as absent rather than as
/// errors: Table 11's preamble makes these parameters *assertions about
/// how the data was encoded* ("all values supplied to the decoding
/// filter by any of these parameters shall match those used when the
/// data was encoded"), so a producer that wrote `/Columns /Foo` has
/// written a malformed file whose only recoverable reading is the
/// default. Refusing the image outright would lose more than it
/// protects, and the decode still fails cleanly if the default is wrong
/// for the data.
fn params(doc: &DocumentView<'_>, parms: Option<&Dict>) -> Params {
    let mut out = Params::default();
    let Some(parms) = parms else { return out };
    let int = |key: &[u8]| -> Option<i64> {
        parms
            .get(key)
            .map(|o| doc.resolve(o))
            .and_then(Object::as_int)
    };
    let flag = |key: &[u8]| -> Option<bool> {
        match parms.get(key).map(|o| doc.resolve(o)) {
            Some(&Object::Boolean(b)) => Some(b),
            _ => None,
        }
    };
    if let Some(v) = int(b"K") {
        out.k = v;
    }
    if let Some(v) = flag(b"EndOfLine") {
        out.end_of_line = v;
    }
    if let Some(v) = flag(b"EncodedByteAlign") {
        out.encoded_byte_align = v;
    }
    if let Some(v) = int(b"Columns") {
        out.columns = v;
    }
    if let Some(v) = int(b"Rows") {
        out.rows = v;
    }
    if let Some(v) = flag(b"EndOfBlock") {
        out.end_of_block = v;
    }
    if let Some(v) = flag(b"BlackIs1") {
        out.black_is_1 = v;
    }
    if let Some(v) = int(b"DamagedRowsBeforeError") {
        out.damaged_rows_before_error = v;
    }
    out
}

/// Table 11's `K` trichotomy, and only the trichotomy.
///
/// - `K < 0` — "Pure two-dimensional encoding (Group 4)"
/// - `K = 0` — "Pure one-dimensional encoding (Group 3, 1-D)"
/// - `K > 0` — "Mixed one- and two-dimensional encoding (Group 3, 2-D)"
///
/// The positive `k` is carried into [`EncodingMode::Group3_2D`] because
/// the crate's API asks for it, but the spec forbids *distinguishing*
/// between positive values, and `hayro-ccitt`'s Group 3 2-D loop indeed
/// reads the per-line tag bit rather than counting against `k` — so the
/// value is inert on both sides of the boundary. It is clamped rather
/// than truncated so a `K` of `i64::MAX` cannot wrap to something small.
const fn encoding_mode(k: i64) -> EncodingMode {
    if k < 0 {
        EncodingMode::Group4
    } else if k == 0 {
        EncodingMode::Group3_1D
    } else {
        EncodingMode::Group3_2D {
            k: if k > u32::MAX as i64 {
                u32::MAX
            } else {
                k as u32
            },
        }
    }
}

/// Build the vendor settings from Table 11's parameters.
///
/// Split out from [`decode`] so the mapping can be asserted field by
/// field against the table without a codestream in hand.
fn settings(params: &Params, columns: u32, rows: u32) -> DecodeSettings {
    DecodeSettings {
        columns,
        rows,
        end_of_block: params.end_of_block,
        // Advisory only. Table 11: "The CCITTFaxDecode filter shall
        // ALWAYS accept end-of-line bit patterns" — the flag says
        // whether they are *expected*, not whether they are *tolerated*.
        // `hayro-ccitt` is unconditionally lenient (it attempts an EOL
        // read at the start of every Group 3 stream and after every
        // line), which is the conformant behaviour, so the value is
        // forwarded for completeness rather than to change anything.
        end_of_line: params.end_of_line,
        rows_are_byte_aligned: params.encoded_byte_align,
        encoding: encoding_mode(params.k),
        // THE polarity line. `invert_black` is the DIRECT assignment,
        // not the negation — see the module docs' derivation.
        invert_black: params.black_is_1,
    }
}

/// The row count Table 11 lets pdfcer know in advance, if any.
///
/// `/Rows` first ("the height of the image in scan lines"), then the
/// image dictionary's `/Height` — the fallback decision 005 §1.2 named
/// as one of the two mechanical reasons this codec needs `&Document` at
/// all. `None` means neither is usable and the caller falls back to
/// [`row_ceiling`].
fn declared_rows(doc: &DocumentView<'_>, params: &Params, dict: &Dict) -> Option<u32> {
    u32::try_from(params.rows)
        .ok()
        .filter(|&r| r > 0)
        .or_else(|| {
            dict.get(b"Height")
                .map(|o| doc.resolve(o))
                .and_then(Object::as_int)
                .and_then(|v| u32::try_from(v).ok())
                .filter(|&r| r > 0)
        })
}

/// The largest row count pdfcer will decode for a given width.
///
/// Derived from [`MAX_IMAGE_PIXELS`] and [`MAX_IMAGE_SAMPLE_BYTES`],
/// never from a vendor default — `hayro-ccitt` has none, so an
/// EOFB-less stream with `/Rows 0` and no `/Height` would otherwise
/// decode until the input ran out, which for a crafted file is
/// unbounded work. Also clamped to [`MAX_IMAGE_DIMENSION`] so a
/// one-pixel-wide image cannot claim 32 million rows.
fn row_ceiling(columns: u32) -> u32 {
    let by_pixels = MAX_IMAGE_PIXELS / u64::from(columns.max(1));
    let by_bytes = (MAX_IMAGE_SAMPLE_BYTES as u64) / (u64::from(columns.max(1)).div_ceil(8));
    let limit = by_pixels.min(by_bytes).min(u64::from(MAX_IMAGE_DIMENSION));
    u32::try_from(limit).unwrap_or(MAX_IMAGE_DIMENSION).max(1)
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

/// Map a `hayro-ccitt` failure to a structured pdfcer error.
///
/// One case is a **named feature gap** rather than corruption
/// (rule R27). Table 11's `DamagedRowsBeforeError` — "the number of
/// damaged rows of data that shall be tolerated before an error occurs",
/// applying "only if `EndOfLine` is true and `K` is non-negative" —
/// describes a resynchronization behaviour (find the next `EndOfLine`
/// pattern, then substitute the previous row or a white scan line) that
/// `hayro-ccitt` does not implement and pdfcer does not add. When a file
/// asked for that tolerance and the stream then failed, saying
/// "corrupt" would be true but useless: the operator's actual question
/// is *which* missing capability would have saved it. So the diagnostic
/// names the feature, and it is counted by name in the renderer.
///
/// Every other failure is [`ImageCodecError::Corrupt`] — the fail-clean
/// contract (decision 001 §6.1.4): corrupt in, `Err` out, never
/// plausible-looking garbage. `hayro-ccitt` documents that some rows may
/// already have been written when it errors; pdfcer deliberately does
/// **not** return that partial image, because a silently short fax page
/// is exactly the "looks deliberate" failure this project refuses.
fn decode_error(params: &Params, err: DecodeError) -> ImageCodecError {
    if params.damaged_rows_before_error > 0 && params.end_of_line && params.k >= 0 {
        return ImageCodecError::FeatureUnsupported {
            feature: "CCITT/damaged-rows",
        };
    }
    ImageCodecError::Corrupt {
        codec: Codec::Ccitt,
        detail: err.to_string(),
    }
}

/// A corrupt-codestream error raised by pdfcer's own parameter checks.
fn corrupt(detail: &str) -> ImageCodecError {
    ImageCodecError::Corrupt {
        codec: Codec::Ccitt,
        detail: detail.to_owned(),
    }
}

/// Does the image dictionary disagree with what the filter produced?
///
/// Unlike DCT — where the codestream carries its own geometry — CCITT
/// has none: `Columns` and `Rows` are `/DecodeParms` entries, so a
/// disagreement here is a disagreement *between two parts of the same
/// dictionary*. It is still worth counting, because the two most common
/// real-world fax defects both show up as one:
///
/// - `/Columns` omitted on a non-1728-wide scan, so the 1728 default
///   governs the decoder's line length while `/Width` governs the image
///   — which shears the picture rather than failing;
/// - a stream that ran out of data before `/Height` rows, which yields
///   fewer rows than the dictionary promises.
///
/// `/BitsPerComponent` is included for the same reason as in the DCT
/// adapter: Table 89 makes an entry inconsistent with the filter an
/// error, and this filter "shall always deliver 1-bit samples". An
/// **absent** `/BitsPerComponent` is not a disagreement — image masks
/// routinely omit it, and §8.9.6.2 fixes it at 1 for them anyway.
fn geometry_disagrees(doc: &DocumentView<'_>, dict: &Dict, width: u32, height: u32) -> bool {
    let int = |key: &[u8]| -> Option<i64> {
        dict.get(key)
            .map(|o| doc.resolve(o))
            .and_then(Object::as_int)
    };
    let differs = |key: &[u8], actual: u32| -> bool {
        int(key).is_some_and(|v| u32::try_from(v).map(|v| v != actual).unwrap_or(true))
    };
    differs(b"Width", width) || differs(b"Height", height) || differs(b"BitsPerComponent", 1)
}

/// The vendor sink impl.
///
/// `hayro-ccitt` has **already** applied `invert_black` by the time it
/// calls the sink (`push_pixels` computes
/// `color.is_white() ^ invert_black`), so the flag it hands over is
/// exactly pdfcer's notion of "white" and needs no further translation.
/// That is the composition the module docs derive: `BlackIs1` goes into
/// `invert_black` unchanged, and the sink writes a `1` bit for white.
impl hayro_ccitt::Decoder for BilevelSink {
    fn push_pixel(&mut self, white: bool) {
        self.push_white(white);
    }

    fn push_pixel_chunk(&mut self, white: bool, chunk_count: u32) {
        self.push_white_chunk(white, chunk_count);
    }

    fn next_line(&mut self) {
        self.end_row();
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
    fn table_11_defaults_match_the_verified_extraction() {
        // Read straight off `filter__ccitt.md`'s verified Table 11. If
        // one of these ever changes, it changes HERE and nowhere else.
        let p = Params::default();
        assert_eq!(p.k, 0);
        assert!(!p.end_of_line);
        assert!(!p.encoded_byte_align);
        assert_eq!(p.columns, 1728, "the ITU-T A4 scan width");
        assert_eq!(p.rows, 0);
        assert!(p.end_of_block, "EndOfBlock defaults to TRUE");
        assert!(!p.black_is_1, "BlackIs1 defaults to FALSE, i.e. 0 = black");
        assert_eq!(p.damaged_rows_before_error, 0);
    }

    #[test]
    fn k_is_trichotomous_and_positive_values_are_indistinguishable() {
        assert_eq!(encoding_mode(-1), EncodingMode::Group4);
        assert_eq!(encoding_mode(i64::MIN), EncodingMode::Group4);
        assert_eq!(encoding_mode(0), EncodingMode::Group3_1D);
        assert!(matches!(encoding_mode(4), EncodingMode::Group3_2D { k: 4 }));
        assert!(matches!(
            encoding_mode(40),
            EncodingMode::Group3_2D { k: 40 }
        ));
        // "shall not distinguish between different positive K values" —
        // both land on the same variant, which is the whole rule.
        assert_eq!(
            core::mem::discriminant(&encoding_mode(4)),
            core::mem::discriminant(&encoding_mode(40))
        );
        // No wrap on an absurd K.
        assert!(matches!(
            encoding_mode(i64::MAX),
            EncodingMode::Group3_2D { k: u32::MAX }
        ));
    }

    #[test]
    fn settings_map_one_to_one_onto_table_11() {
        let params = Params {
            k: -1,
            end_of_line: true,
            encoded_byte_align: true,
            columns: 100,
            rows: 5,
            end_of_block: false,
            black_is_1: true,
            damaged_rows_before_error: 3,
        };
        let s = settings(&params, 100, 5);
        assert_eq!(s.columns, 100);
        assert_eq!(s.rows, 5);
        assert!(!s.end_of_block);
        assert!(s.end_of_line);
        assert!(s.rows_are_byte_aligned);
        assert_eq!(s.encoding, EncodingMode::Group4);
        assert!(
            s.invert_black,
            "BlackIs1 -> invert_black is a DIRECT assignment"
        );
    }

    #[test]
    fn the_row_ceiling_is_derived_not_invented() {
        // A 1728-wide A4 fax: 32 Mpx / 1728 = 19418 rows, well past any
        // real scan, and far past the 16384 a vendor default would have
        // imposed (rule R25's whole point).
        assert_eq!(row_ceiling(1728), (MAX_IMAGE_PIXELS / 1728) as u32);
        // A one-pixel-wide image cannot claim 32 million rows.
        assert_eq!(row_ceiling(1), MAX_IMAGE_DIMENSION);
        // Never zero, whatever the width.
        assert!(row_ceiling(MAX_IMAGE_DIMENSION) >= 1);
    }

    #[test]
    fn damaged_rows_is_named_only_when_table_11_says_it_applies() {
        // "This entry shall apply only if EndOfLine is true and K is
        // non-negative." Outside that window the parameter is inert and
        // a failure is plain corruption.
        let applies = Params {
            damaged_rows_before_error: 2,
            end_of_line: true,
            k: 0,
            ..Params::default()
        };
        assert_eq!(
            decode_error(&applies, DecodeError::InvalidCode),
            ImageCodecError::FeatureUnsupported {
                feature: "CCITT/damaged-rows"
            }
        );
        for inert in [
            Params {
                damaged_rows_before_error: 2,
                end_of_line: false,
                k: 0,
                ..Params::default()
            },
            Params {
                damaged_rows_before_error: 2,
                end_of_line: true,
                k: -1,
                ..Params::default()
            },
            Params {
                damaged_rows_before_error: 0,
                end_of_line: true,
                k: 0,
                ..Params::default()
            },
        ] {
            assert!(matches!(
                decode_error(&inert, DecodeError::InvalidCode),
                ImageCodecError::Corrupt { .. }
            ));
        }
    }
}

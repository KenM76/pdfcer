//! # Stream filters (ISO 32000-1 §7.4) — the decode pipeline
//!
//! Decodes stream data through its `/Filter` chain. Spec sources:
//! `filter__pipeline.md` (cascade rules), `filter__flate.md`,
//! `filter__predictors.md` in the PDF-spec RAG; Table 5 of
//! `iso32000__s__7.3.8.md` for the `/Filter`//`/DecodeParms` shapes.
//! Clause numbers are ISO 32000-1:2008.
//!
//! ## Fail-clean is a TYPE-LEVEL contract here
//!
//! Every decoder returns `Result<Vec<u8>, FilterError>`, and **no code
//! path returns undecoded or partial bytes as if they had decoded**
//! (docs/decisions/001-oxidize-pdf-adopt-vs-build.md §6.1 item 4).
//! This is a deliberate divergence from observed prior art: the audited
//! oxidize-pdf filter layer falls back to returning raw bytes when
//! zlib/predictor decoding fails, which silently corrupts downstream
//! consumers. pdfcer's corrupted-stream regression tests (one per
//! filter) pin the opposite behavior: corrupt in → `Err` out, never
//! plausible-looking garbage. Truncated-but-partially-decodable
//! streams also `Err` in Pass 1; a *labeled* best-effort mode for
//! damaged-file recovery is a later, explicit, corpus-driven feature —
//! never a silent default.
//!
//! ## Resource ceiling (ARCHITECTURE.md §10.1 — pdfcer policy, not spec)
//!
//! Deflate's worst case is ~1032:1 expansion — a 1 MB stream can claim
//! a gigabyte. Every decode enforces [`MAX_DECODED_LEN`] *incrementally*
//! (abort mid-decode when crossed, never inflate-then-check). `/DL` is
//! a hint (Table 5) and is deliberately NOT the bound.
//!
//! ## This module handles BYTE-STREAM filters only (decision 005 R23)
//!
//! A byte-stream filter consumes bytes, produces bytes, and composes
//! with the next stage of the cascade. An **image codec** —
//! `DCTDecode`, `CCITTFaxDecode`, `JBIG2Decode`, `JPXDecode` — does
//! not: its output is *samples* whose geometry and colour model are
//! declared by the codestream itself, and §8.9.5 Table 89 goes as far
//! as making the codestream's declarations **outrank the image
//! dictionary** for JPXDecode. A `Vec<u8>` return type has nowhere to
//! put that information, and re-deriving it would mean parsing hostile
//! input twice.
//!
//! So [`decode_stream`] never decodes an image codec. Reaching one
//! returns [`FilterError::ImageCodec`], which says "this is a codec,
//! decode it through [`crate::image_codec`]" — deliberately distinct
//! from [`FilterError::UnsupportedFilter`], which says "pdfcer does not
//! implement this filter at all." The full rationale is in
//! `docs/decisions/005-image-codecs.md` §1.2, §4.6 and §5.2.
//!
//! ## Coverage
//!
//! - **FlateDecode** (with PNG/TIFF predictors) — object streams, xref
//!   streams, content streams, and most images.
//! - **ASCIIHexDecode / ASCII85Decode** ([`ascii`]) — the ASCII
//!   armouring filters. They compress nothing, but they are the only
//!   two filters that make an inline image's data length unambiguous
//!   (§8.9.7), which is why they land with the image-rendering slice
//!   rather than later.
//! - **LZWDecode** ([`lzw`], Pass 2.1) — legacy but still present in
//!   pre-PDF-1.2 files and legacy tooling output. Shares the predictor
//!   stage with Flate; `/EarlyChange` is its one extra parameter.
//! - **RunLengthDecode** ([`runlength`], Pass 2.1) — trivial, no
//!   parameters, self-limiting.
//!
//! Any other name — a codec handled elsewhere, a crypt filter, or
//! something non-standard — is *recognized* and refused with a precise
//! diagnostic rather than guessed at, the same detect-don't-misparse
//! posture as the xref layer.
//!
//! ## Notes vs errors
//!
//! Some streams decode correctly while still being *non-conformant* in
//! a way an operator should be able to see (an LZW stream with no
//! `ClearCode`, for instance). Those are counted in [`FilterNotes`] and
//! surfaced by [`decode_stream_with_notes`] — "fuzzy, never sneaky"
//! applied at the filter layer. [`decode_stream`] is the same decode
//! with the notes dropped, for the many callers (xref streams, object
//! streams, content streams) that have nowhere to put them.

pub mod ascii;
/// `/BrotliDecode` — a PDF Association EXTENSION, not ISO 32000-2. See the
/// module docs before believing any citation you find for it.
pub mod brotli;
pub mod flate;
pub mod lzw;
pub mod predictor;
pub mod runlength;

use crate::object::{Dict, Object};

/// Hard ceiling on a single stream's decoded size.
///
/// pdfcer policy (ARCHITECTURE.md §10.1): no Annex C limit exists for
/// this; 256 MiB decoded is beyond any legitimate single stream
/// (content streams are KBs; even a 300-DPI A4 uncompressed RGB image
/// is ~35 MB) while bounding a decompression bomb's blast radius.
pub const MAX_DECODED_LEN: usize = 256 * 1024 * 1024;

/// A filter-decode error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FilterError {
    /// The compressed data is invalid (bad zlib header, corrupt
    /// deflate stream, checksum structurally unreadable, …).
    #[error("corrupt {filter} data: {detail}")]
    Corrupt {
        /// Which filter failed.
        filter: &'static str,
        /// Human-readable failure detail from the underlying decoder.
        detail: String,
    },
    /// The stream ended before the filter's own end-of-data — a
    /// truncated stream (§7.3.8.2 extent inconsistency).
    #[error("{filter} stream truncated")]
    Truncated {
        /// Which filter detected the truncation.
        filter: &'static str,
    },
    /// Decoded output crossed [`MAX_DECODED_LEN`] (pdfcer guard) — the
    /// decode was aborted mid-stream.
    #[error("decoded output exceeds MAX_DECODED_LEN ({MAX_DECODED_LEN} bytes)")]
    OutputTooLarge,
    /// `/DecodeParms` carried an invalid parameter combination
    /// (unknown `Predictor` value, zero `Columns`, bad
    /// `BitsPerComponent`, …).
    #[error("invalid decode parameters: {0}")]
    BadParams(&'static str),
    /// Predicted data length is not a whole number of rows — wrong
    /// parameters or truncation (`filter__predictors.md` row
    /// arithmetic).
    #[error("predicted data is not a whole number of rows")]
    RaggedRows,
    /// A PNG row carried an algorithm tag outside 0–4 (RFC 2083 §6:
    /// an error).
    #[error("unknown PNG predictor tag {0}")]
    UnknownPngTag(u8),
    /// A standard filter pdfcer doesn't implement yet (Pass 1 scope).
    /// The payload is the filter name as written.
    #[error("filter {0:?} is not yet supported")]
    UnsupportedFilter(String),
    /// The chain reaches an image codec, which [`decode_stream`] does
    /// not handle **by design** (decision 005 R23). Decode it through
    /// [`crate::image_codec::decode_image`], which has the `&Document`
    /// and the image dictionary that these codecs require, and which
    /// returns the codec-declared geometry and colour model that a
    /// `Vec<u8>` cannot carry.
    ///
    /// Deliberately distinct from [`FilterError::UnsupportedFilter`]:
    /// that one means "pdfcer cannot decode this at all", this one means
    /// "you called the wrong entry point." A caller that only wants
    /// bytes (an xref stream, an object stream) genuinely cannot
    /// proceed, so it is still an error there — but the operator-facing
    /// message stays honest about which of the two situations it is.
    #[error("filter {0:?} is an image codec; decode it via image_codec::decode_image")]
    ImageCodec(String),
    /// `/Filter` held something other than a name or array of names.
    #[error("malformed /Filter entry")]
    BadFilterEntry,
}

/// Divergences that did **not** stop a stream from decoding.
///
/// Separate from [`FilterError`] because the operator's question is
/// different: an error means "these bytes are unusable", a note means
/// "these bytes are fine but the file that produced them is not
/// conformant." Accumulated across the whole `/Filter` chain.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FilterNotes {
    /// LZW streams that did not begin with a `ClearCode`, or that ended
    /// without an `EndOfInformation`. Both are non-conformant (TIFF 6.0
    /// §13; ISO 32000-1 §7.4.4.2), both are recovered, both are
    /// reported. See [`lzw`] for the sourced recovery rules.
    pub lzw_framing_anomalies: usize,
}

/// Decode a stream's raw bytes through its full `/Filter` chain.
///
/// `dict` is the stream dictionary; `raw` its encoded bytes (the
/// `data_span` slice). Handles the Table 5 shapes: `/Filter` absent
/// (identity), a single name, or an array of zero or more names
/// applied **in array order** (decode order = written order, Table 5);
/// `/DecodeParms` correspondingly a dict, an array (with `null` for
/// parameterless positions), or absent.
///
/// # Errors
///
/// [`FilterError`] — see the variants; fail-clean per the module docs.
pub fn decode_stream(dict: &Dict, raw: &[u8]) -> Result<Vec<u8>, FilterError> {
    decode_stream_with_notes(dict, raw).map(|(data, _)| data)
}

/// [`decode_stream`], keeping the non-fatal [`FilterNotes`].
///
/// Use this wherever the notes have somewhere to go — the image path
/// surfaces them in the renderer's diagnostics. Every other caller
/// (xref streams, object streams, content streams) has no diagnostics
/// channel and uses [`decode_stream`].
///
/// # Errors
///
/// Identical to [`decode_stream`].
pub fn decode_stream_with_notes(
    dict: &Dict,
    raw: &[u8],
) -> Result<(Vec<u8>, FilterNotes), FilterError> {
    let filters = filter_names(dict)?;
    let mut notes = FilterNotes::default();
    let data = decode_prefix(dict, raw, filters.len(), &mut notes)?;
    Ok((data, notes))
}

/// Apply the first `take` filters of `dict`'s chain to `raw`.
///
/// Exists so [`crate::image_codec`] can run the **byte-stream prefix**
/// of a chain that ends in an image codec (e.g.
/// `/Filter [/ASCII85Decode /DCTDecode]`, which is a real and legal
/// shape) and then dispatch the terminal codec itself. `take` equal to
/// the full chain length is the ordinary [`decode_stream`] case.
///
/// # Errors
///
/// [`FilterError`] from any stage; in particular
/// [`FilterError::ImageCodec`] if a codec name falls inside the prefix,
/// which means the caller mis-computed where the chain's byte-stream
/// part ends.
pub(crate) fn decode_prefix(
    dict: &Dict,
    raw: &[u8],
    take: usize,
    notes: &mut FilterNotes,
) -> Result<Vec<u8>, FilterError> {
    let filters = filter_names(dict)?;
    let mut data: Vec<u8> = raw.to_vec();
    for (i, name) in filters.iter().enumerate().take(take) {
        let parms = decode_parms_for(dict, i, filters.len());
        data = apply_one(name, parms, &data, notes)?;
    }
    Ok(data)
}

/// Apply a single named filter.
///
/// Abbreviated spellings (Table 94) are accepted alongside the full
/// names because inline images legally use them; `pdfcer_core::content`
/// normalizes the ones it sees, but a stream dictionary that spells a
/// filter the short way is not worth refusing.
fn apply_one(
    name: &[u8],
    parms: Option<&Dict>,
    data: &[u8],
    notes: &mut FilterNotes,
) -> Result<Vec<u8>, FilterError> {
    match name {
        b"FlateDecode" | b"Fl" => flate::decode(data, parms),
        // EXTN-BROTLI-1 §5.2. NO ABBREVIATION: the extension defines none,
        // and Table 92's abbreviations exist for inline images, which this
        // filter `SHALL NOT` be used for. ★ MuPDF accepts a `/Br` alias that
        // does not exist in the extension -- that is MuPDF's behaviour, not
        // the specification, and pdfcer does not follow it. Accepting `/Br`
        // would make pdfcer read files no conformant writer produces and no
        // other reader agrees on.
        b"BrotliDecode" => brotli::decode(data, parms),
        // §7.4.2/§7.4.3 — parameterless (Table 6), so `parms` is
        // deliberately not forwarded.
        b"ASCIIHexDecode" | b"AHx" => ascii::decode_hex(data),
        b"ASCII85Decode" | b"A85" => ascii::decode_85(data),
        b"LZWDecode" | b"LZW" => lzw::decode(data, parms, notes),
        // §7.4.5 — parameterless (Table 6).
        b"RunLengthDecode" | b"RL" => runlength::decode(data),
        // Image codecs are a TERMINAL stage, not a byte-stream filter
        // (R23). Named separately so the operator-facing message says
        // "wrong entry point", not "unsupported".
        b"DCTDecode" | b"DCT" | b"CCITTFaxDecode" | b"CCF" | b"JBIG2Decode" | b"JPXDecode" => Err(
            FilterError::ImageCodec(String::from_utf8_lossy(name).into_owned()),
        ),
        // Anything else: a crypt filter, or a non-standard name.
        // Refused with a precise diagnostic rather than guessed at.
        other => Err(FilterError::UnsupportedFilter(
            String::from_utf8_lossy(other).into_owned(),
        )),
    }
}

/// Extract the ordered filter-name list from `/Filter` (name, array of
/// names, or absent → empty).
pub(crate) fn filter_names(dict: &Dict) -> Result<Vec<Vec<u8>>, FilterError> {
    match dict.get(b"Filter") {
        None => Ok(Vec::new()),
        Some(Object::Name(n)) => Ok(vec![n.as_bytes().to_vec()]),
        Some(Object::Array(items)) => items
            .iter()
            .map(|o| match o {
                Object::Name(n) => Ok(n.as_bytes().to_vec()),
                _ => Err(FilterError::BadFilterEntry),
            })
            .collect(),
        Some(_) => Err(FilterError::BadFilterEntry),
    }
}

/// The `/DecodeParms` entry for filter position `i` of `total`
/// (Table 5 shape rules). `None` = defaults. A malformed entry is
/// treated as absent — the individual filter then validates whatever
/// it actually needs; parameters it never looks at can't hurt it.
fn decode_parms_for(dict: &Dict, i: usize, total: usize) -> Option<&Dict> {
    let parms = dict.get(b"DecodeParms").or_else(|| dict.get(b"DP"))?;
    match parms {
        Object::Dict(d) if total == 1 => Some(d),
        Object::Array(items) => match items.get(i) {
            Some(Object::Dict(d)) => Some(d),
            _ => None, // null / absent position = defaults (Table 5)
        },
        _ => None,
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
    use crate::object::Name;

    fn dict_with_filter(filter: Object) -> Dict {
        let mut d = Dict::new();
        d.insert(Name::from(b"Filter"), filter);
        d
    }

    #[test]
    fn no_filter_is_identity() {
        let d = Dict::new();
        assert_eq!(decode_stream(&d, b"plain").unwrap(), b"plain");
    }

    #[test]
    fn empty_filter_array_is_identity() {
        // Table 5: an array of ZERO names is legal.
        let d = dict_with_filter(Object::Array(vec![]));
        assert_eq!(decode_stream(&d, b"plain").unwrap(), b"plain");
    }

    #[test]
    fn image_codec_in_the_byte_stream_cascade_is_its_own_error() {
        // R23: `decode_stream` never decodes a codec, and says so in a
        // way distinguishable from "pdfcer cannot do this at all".
        for name in [
            &b"DCTDecode"[..],
            b"CCITTFaxDecode",
            b"JBIG2Decode",
            b"JPXDecode",
        ] {
            let d = dict_with_filter(Object::Name(Name::from(name)));
            let e = decode_stream(&d, b"\xFF\xD8").unwrap_err();
            assert_eq!(
                e,
                FilterError::ImageCodec(String::from_utf8_lossy(name).into_owned())
            );
        }
    }

    #[test]
    fn unsupported_filter_is_named_in_error() {
        let d = dict_with_filter(Object::Name(Name::from(b"Crypt")));
        let e = decode_stream(&d, b"anything").unwrap_err();
        assert_eq!(e, FilterError::UnsupportedFilter("Crypt".into()));
    }

    /// ★ `/Br` IS NOT A FILTER NAME, and pdfcer refuses it on purpose.
    ///
    /// **MuPDF accepts `/Br` as an alias for `/BrotliDecode`** — its
    /// `pdf-stream.c` tests `pdf_name_eq(f, PDF_NAME(BrotliDecode)) ||
    /// pdf_name_eq(f, PDF_NAME(Br))`. **The extension defines no such
    /// abbreviation**, and could not sensibly: Table 92's abbreviations exist
    /// for inline images, and `/BrotliDecode` `SHALL NOT` appear in one.
    ///
    /// This is a behaviour-versus-specification split, and following the
    /// behaviour would be the worse choice in both directions — pdfcer would
    /// read files no conformant writer emits, and would disagree with every
    /// reader that is not MuPDF. Pinned as a test so a future reader who
    /// meets a `/Br` in the wild adds it deliberately, with a decision
    /// record, rather than as an obvious-looking omission.
    #[test]
    fn br_is_not_accepted_as_an_abbreviation_for_brotli() {
        let mut notes = FilterNotes::default();
        let e = apply_one(b"Br", None, b"anything", &mut notes).unwrap_err();
        assert_eq!(e, FilterError::UnsupportedFilter("Br".into()));
    }

    /// `/BrotliDecode` reaches its decoder rather than the unsupported arm.
    ///
    /// Asserted through the DISPATCH rather than by calling `brotli::decode`
    /// directly: the filter module's own tests prove the decoder works, and
    /// this proves the name is wired to it. Those are different claims, and
    /// `Pass 2.x`'s history has an instance of each passing while the other
    /// failed.
    #[test]
    fn brotli_decode_is_dispatched_and_not_refused() {
        let mut notes = FilterNotes::default();
        // Deliberately invalid Brotli. Success here would mean the name was
        // silently ignored; what must happen is that the BROTLI decoder
        // refuses it, which is a differently-shaped error from
        // `UnsupportedFilter` and is what distinguishes "wired" from "not".
        let e = apply_one(b"BrotliDecode", None, &[0xff; 32], &mut notes).unwrap_err();
        assert!(
            !matches!(e, FilterError::UnsupportedFilter(_)),
            "/BrotliDecode must reach its decoder, got {e:?}"
        );
    }

    #[test]
    fn runlength_runs_in_the_cascade() {
        let d = dict_with_filter(Object::Name(Name::from(b"RunLengthDecode")));
        assert_eq!(decode_stream(&d, b"\x02abc\xFEz\x80").unwrap(), b"abczzz");
    }

    #[test]
    fn lzw_notes_reach_the_caller_through_the_cascade() {
        // A stream that does not begin with a ClearCode still decodes,
        // and the non-conformance is COUNTED rather than absorbed.
        let d = dict_with_filter(Object::Name(Name::from(b"LZWDecode")));
        let (data, notes) = decode_stream_with_notes(&d, &[0x20, 0xC0, 0x40]).unwrap();
        assert_eq!(data, b"A");
        assert_eq!(notes.lzw_framing_anomalies, 1);
    }

    #[test]
    fn non_name_filter_entry_is_error() {
        let d = dict_with_filter(Object::Integer(5));
        assert_eq!(
            decode_stream(&d, b"x").unwrap_err(),
            FilterError::BadFilterEntry
        );
    }

    #[test]
    fn flate_roundtrip_via_pipeline() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write as _;

        let original = b"BT /F1 12 Tf 72 712 Td (Hello) Tj ET".repeat(10);
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&original).unwrap();
        let compressed = enc.finish().unwrap();

        let d = dict_with_filter(Object::Name(Name::from(b"FlateDecode")));
        assert_eq!(decode_stream(&d, &compressed).unwrap(), original);
    }
}

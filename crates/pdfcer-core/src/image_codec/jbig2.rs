//! # JBIG2Decode (ISO 32000-1 §7.4.7, Table 12; ITU-T T.88) — the JBIG2 adapter
//!
//! Spec source: `filters/filter__jbig2.md` in the PDF-spec RAG. ISO
//! 32000-1 §7.4.7 defines only the **PDF embedding** — the
//! `/JBIG2Globals` parameter and the stream framing — and delegates the
//! codec itself to ITU-T T.88 (published identically as ISO/IEC 14492;
//! §7.4.7's own reference to "ISO/IEC 11544" is a defect in the source
//! text, verified against T.88 p. 3 and recorded in the RAG — do not
//! "correct" it back).
//!
//! Crate choice is decision 005 §4.4: **`hayro-jbig2`**. The alternatives
//! were all GPL-3.0 C FFI bindings to Artifex's `jbig2dec`, untouched
//! since 2020 — disqualified three times over (copyleft against
//! `LEGAL.md` §1, C FFI against the wasm32/portable
//! invariants, abandonment against `ARCHITECTURE.md` §10). `hayro-jbig2`
//! removes the question rather than posing it, and it is the reason no
//! `LEGAL.md` §6.2 escalation was needed for this Pass.
//!
//! **Amended 2026-08-07**: this said "an *undecided* `LEGAL.md` §1". The
//! licence was decided **MIT** on 2026-08-01, which makes the copyleft
//! disqualification **stronger, not weaker** — an MIT project cannot
//! link GPL-3.0 at all (`LEGAL.md` §6.1), so what was a risk pending a
//! decision is now a categorical bar. The choice made here needs no
//! revisiting; only its stated reason was out of date.
//!
//! ## `/JBIG2Globals` is why this codec needs `&Document`
//!
//! Table 12's single parameter is a **stream reference**: the JBIG2
//! global (page-0) segments — typically a symbol dictionary. Table 12's
//! second sentence makes the placement a `shall` keyed on **segment page
//! association == 0**, "even if only a single JBIG2 image XObject refers
//! to it" — sharing across images is a separate *permissive* statement
//! (§7.4.7 bullet 4), not what selects a segment into this stream.
//! Resolving the reference needs the document, which is one of the
//! two mechanical reasons decision 005 §1.2 gave for the two-tier filter
//! architecture — `(dict, raw)` cannot reach it.
//!
//! `hayro-jbig2`'s `Image::new_embedded(data, globals)` takes the globals
//! as a separate byte slice, which *is* the PDF embedding shape
//! (T.88 Annex D.3, the "embedded stream" organization with no file
//! header and no end-of-file segment). Decision 005 §3.5 records that no
//! general-purpose JBIG2 decoder exposes that split, and it is the
//! single largest reason this crate fits without glue.
//!
//! The globals stream is itself a PDF stream and is very often
//! `FlateDecode`d, so it goes through [`crate::filters::decode_stream`]
//! before reaching the decoder. An **absent** `/JBIG2Globals` is normal
//! and not an error: an image whose segments are all inline needs none.
//!
//! ## Forbidden in inline images
//!
//! §7.4.7, verbatim: "the JBIG2Decode filter shall not be used with
//! inline images." §8.9.7 says the same from the other side —
//! `JBIG2Decode` and `JPXDecode` are absent from Table 94's
//! abbreviation list "because those filters shall not be used with
//! inline images." That rule is enforced *before* any bytes are touched,
//! in [`super::decode_image`] via [`super::Codec::allowed_inline`], so
//! this module is never reached from the inline path at all.
//!
//! ## Polarity: T.88 says 1 is black, PDF says 0 is black
//!
//! A JBIG2 bitmap stores `1` for a black pixel — the convention T.88
//! §6.2.6 states explicitly for the MMR path ("pixels decoded by the MMR
//! decoder having the value 'black' shall be treated as having the value
//! 1") and uses throughout. `hayro-jbig2` surfaces it faithfully: its
//! sink's flag is named `black`.
//!
//! PDF's convention for bilevel image data is the opposite, and §7.4.6
//! Table 11 is where the spec says so out loud, in `BlackIs1`'s
//! description: 1-means-black is "the reverse of the normal PDF
//! convention for image data". With the DeviceGray default
//! `Decode [0 1]` at 1 bit per component, sample 0 → grey 0.0 → black.
//!
//! Unlike CCITT, JBIG2Decode has **no polarity parameter** — Table 12
//! carries `/JBIG2Globals` and nothing else — so the inversion is
//! unconditional and belongs here, not to the file. This module
//! therefore pushes `!black` into [`BilevelSink`], whose contract is
//! "true writes a 1 bit (white)". Every other shipping PDF engine does
//! the identical flip at the identical place.
//!
//! ## Resource ceilings are pdfcer's (rule R25)
//!
//! `Image::new_embedded` parses segment headers only — bounded by the
//! input length — and reports the page geometry through `width()` and
//! `height()` **before** anything is decoded. `decode()` then allocates
//! the full page bitmap from that geometry. So the ceiling check has
//! exactly one correct place, between those two calls, and that is where
//! [`MAX_IMAGE_PIXELS`] and [`MAX_IMAGE_DIMENSION`] are applied. A page
//! information segment claiming 65535 × 65535 is refused there rather
//! than after a 4 Gbit allocation.
//!
//! [`BilevelSink`]: super::bilevel::BilevelSink

use hayro_jbig2::Image;

use super::bilevel::BilevelSink;
use super::{
    Codec, CodecColorModel, CodecNotes, CodedImage, ImageCodecError, MAX_IMAGE_DIMENSION,
    MAX_IMAGE_PIXELS,
};
// decision 018: the codecs resolve indirect entries through a `DocumentView`
// rather than a `&Document`, so an image whose dictionary lives in an
// editing session decodes as the operator currently has it. `Document` is
// still named by the back-compat `decode_image` wrapper in `mod.rs`.
use crate::filters;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::view::DocumentView;

/// Decode a `JBIG2Decode` codestream.
///
/// `data` is the embedded JBIG2 stream (T.88 Annex D.3 organization)
/// after any byte-stream filter prefix; `parms` is the codec's own
/// `/DecodeParms` entry, consulted for `/JBIG2Globals`; `dict` is the
/// image dictionary, used only for geometry reconciliation; `notes`
/// accumulates the honesty counters.
///
/// # Errors
///
/// [`ImageCodecError::TooLarge`] when the page geometry crosses a pdfcer
/// ceiling; [`ImageCodecError::Corrupt`] when the globals stream cannot
/// be decoded, when the segment stream is malformed, or when decoding
/// fails. `hayro-jbig2` returns a structured error for every failure
/// mode, so the mapping is total and nothing here can panic.
pub(super) fn decode(
    doc: &DocumentView<'_>,
    data: &[u8],
    parms: Option<&Dict>,
    dict: &Dict,
    notes: &mut CodecNotes,
) -> Result<CodedImage, ImageCodecError> {
    let globals = globals(doc, parms)?;

    let image = Image::new_embedded(data, globals.as_deref()).map_err(corrupt)?;
    let (width, height) = (image.width(), image.height());

    // The ONE correct place for the ceiling: after the segment headers
    // have declared the page geometry, before `decode()` allocates a
    // bitmap from it (module docs, rule R25).
    if width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || u64::from(width).saturating_mul(u64::from(height)) > MAX_IMAGE_PIXELS
    {
        return Err(ImageCodecError::TooLarge);
    }

    let mut sink = BilevelSink::new(width, height);
    let outcome = image.decode(&mut sink);
    // Checked before the decode result for the same reason as in the
    // CCITT adapter: an overflow means pdfcer stopped accepting rows, so
    // whatever the decoder reported afterwards is downstream of that.
    if sink.overflowed() {
        return Err(ImageCodecError::TooLarge);
    }
    outcome.map_err(corrupt)?;

    let (samples, rows) = sink.finish();
    if rows == 0 {
        return Err(corrupt_detail("no scan lines decoded"));
    }

    notes.geometry_mismatch = geometry_disagrees(doc, dict, width, rows);

    Ok(CodedImage {
        samples,
        codec: Some(Codec::Jbig2),
        width,
        // The rows actually emitted, which equals the page height for
        // any well-formed stream. Reporting the emitted count rather
        // than the declared one is what lets `pdfcer-render` mark a short
        // image `truncated` instead of reading past the buffer.
        height: rows,
        components: 1,
        // §8.9.5 Table 89: "a CCITTFaxDecode or JBIG2Decode filter shall
        // always deliver 1-bit samples."
        bits_per_component: 1,
        color_model: CodecColorModel::Bilevel,
        // T.88 codes bi-level bitmaps; there is no colour information in
        // the codestream to reconcile against `/ColorSpace`. (T.88's
        // colour-palette segment type exists but is out of scope for the
        // PDF embedding, and `hayro-jbig2` surfaces no palette.)
        icc_profile: None,
        // T.88 codes a bi-level bitmap; there is no alpha channel to
        // carry. Only JPXDecode populates this field.
        embedded_alpha: None,
        notes: *notes,
    })
}

/// Resolve and decode `/JBIG2Globals` (Table 12), if present.
///
/// The entry is "a stream containing the JBIG2 global (page 0)
/// segments", required there by Table 12 "even if only a single JBIG2
/// image XObject refers to it" (page-0 association, not sharing, is the
/// selector). Three things have to happen and all three can fail
/// differently:
///
/// 1. the entry resolves through the document (it is an indirect
///    reference in every real file — which also lets multiple XObjects
///    share one globals stream, the permissive case in §7.4.7);
/// 2. it is a **stream**, whose raw bytes come out of the retained
///    buffer by span;
/// 3. those bytes run through their own `/Filter` chain, because a
///    globals stream is ordinarily `FlateDecode`d like any other.
///
/// A `/JBIG2Globals` that resolves to something other than a stream is
/// treated as **absent** rather than as an error: many images need no
/// globals at all, a non-stream value carries none, and refusing the
/// image would turn a producer's stray entry into a blank page. If the
/// segments really were needed, `Image::new_embedded` fails immediately
/// afterwards with a structured error naming the missing reference —
/// which is the more useful diagnostic anyway.
///
/// # Errors
///
/// [`ImageCodecError::Filter`] if the globals stream exists but its own
/// filter chain fails. That one *is* an error: the file said the
/// segments are over there, and they cannot be read.
fn globals(
    doc: &DocumentView<'_>,
    parms: Option<&Dict>,
) -> Result<Option<Vec<u8>>, ImageCodecError> {
    let Some(entry) = parms.and_then(|d| d.get(b"JBIG2Globals")) else {
        return Ok(None);
    };
    let Object::Stream(stream) = doc.resolve(entry) else {
        return Ok(None);
    };
    // `view.slice`, not `span.slice(doc.bytes())`: a session view resolves a
    // globals stream authored this session out of the R45 staging half
    // (decision 018 §4). Same refusal for an out-of-bounds span.
    let Some(raw) = doc.slice(stream.data_span) else {
        return Err(corrupt_detail("/JBIG2Globals stream data is out of bounds"));
    };
    Ok(Some(filters::decode_stream(&stream.dict, raw)?))
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

/// Wrap a `hayro-jbig2` failure as a structured pdfcer error.
///
/// The crate's `DecodeError` is an enum over parse/segment/region/
/// symbol/huffman/template/overflow faults and implements `Display`, so
/// the detail string names the actual fault rather than "decode
/// failed". No variant is mapped to
/// [`ImageCodecError::FeatureUnsupported`] today: unlike DCT, whose
/// unsupported *frame types* are a fixed, enumerable set that pdfcer
/// pre-sniffs, `hayro-jbig2` implements the whole of T.88's decoding
/// side (generic, refinement, symbol/text, halftone/pattern, MMR and
/// arithmetic), so there is no known sub-feature to name. If one turns
/// up, it becomes a named key here (rule R27) rather than an added
/// tolerance.
fn corrupt(err: hayro_jbig2::DecodeError) -> ImageCodecError {
    ImageCodecError::Corrupt {
        codec: Codec::Jbig2,
        detail: err.to_string(),
    }
}

/// A corrupt-stream error raised by pdfcer's own checks.
fn corrupt_detail(detail: &str) -> ImageCodecError {
    ImageCodecError::Corrupt {
        codec: Codec::Jbig2,
        detail: detail.to_owned(),
    }
}

/// Does the image dictionary disagree with the page the segments
/// declared?
///
/// JBIG2 is the one bilevel codec that *does* carry its own geometry: a
/// page information segment (T.88 §7.4.8) states the page bitmap's width
/// and height independently of `/Width` and `/Height`. A disagreement is
/// a producer bug — Table 89's "entries inconsistent with each other" —
/// counted and reported, never acted on: `pdfcer-render` keeps the
/// dictionary's numbers for placement and the codestream's for the row
/// stride, which is the only combination that neither shears the picture
/// nor moves it.
///
/// `/BitsPerComponent` is included because Table 89 fixes it at 1 for
/// this filter. An **absent** entry is not a disagreement.
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

/// The vendor sink impl: T.88's `1 = black` becomes pdfcer's
/// `1 = white` (module docs).
impl hayro_jbig2::Decoder for BilevelSink {
    fn push_pixel(&mut self, black: bool) {
        self.push_white(!black);
    }

    fn push_pixel_chunk(&mut self, black: bool, chunk_count: u32) {
        self.push_white_chunk(!black, chunk_count);
    }

    fn next_line(&mut self) {
        self.end_row();
    }
}

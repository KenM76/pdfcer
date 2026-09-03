//! ★ `/BrotliDecode` is reachable **from a page**, not merely correct in
//! isolation.
//!
//! # Why this file exists separately from the filter's own unit tests
//!
//! `filters::brotli`'s tests prove the decoder round-trips bytes, refuses a
//! truncated stream, and honours the output ceiling. **None of them proves a
//! PDF can get to it.** That gap is not hypothetical here: a Pass shipped
//! earlier the same week was very nearly dead code for exactly this reason —
//! correct arithmetic no content stream could reach — and it was caught only
//! because somebody built a file to exercise it.
//!
//! So this decodes a real document: `/Filter /BrotliDecode` on a page's
//! content stream, through the ordinary `decode_stream` entry point that
//! every consumer uses.
//!
//! # The predictor case, and whose bug it guards against
//!
//! EXTN-BROTLI-1 retitles Table 8 to include Brotli, so `FlateDecode`'s
//! predictors apply **verbatim** and pdfcer reuses that code unchanged. The
//! second fixture exercises `/Predictor 12` (PNG-Up) over Brotli, and the two
//! documents draw **identical** content — so a predictor that failed to
//! round-trip would show as a content mismatch rather than as an error.
//!
//! ★ **pdf.js silently ignores `/DecodeParms` predictors on Brotli** while
//! honouring them for Flate and LZW. That divergence is pdf.js's, and this
//! test is the reason pdfcer will not acquire it by accident.
//!
//! Fixtures: `tools/gen-brotli-fixture.py`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use pdfcer_core::document::Document;
use pdfcer_core::object::Object;
use pdfcer_core::{filters, page_tree};

/// Decode the first page's content stream of one fixture.
fn page_content(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/brotli")
        .join(name);
    let doc =
        Document::from_bytes(std::fs::read(&path).expect("fixture file")).expect("fixture parses");
    let pages = page_tree::pages(&doc).expect("page tree");
    let page_dict = doc
        .get(pages[0].id)
        .and_then(|o| o.value.as_dict().cloned())
        .expect("page object is a dict");
    let contents = page_dict
        .get(b"Contents")
        .map(|o| doc.resolve(o))
        .expect("page has /Contents");
    let Object::Stream(stream) = contents else {
        panic!("/Contents is not a stream");
    };
    let raw = stream
        .data_span
        .slice(doc.bytes())
        .expect("stream bytes are in range");
    filters::decode_stream(&stream.dict, raw).expect("BrotliDecode stream decodes")
}

/// The operators survive the round trip, byte for byte.
///
/// Asserted against the exact expected content rather than "it decoded to
/// something non-empty": a decoder that returned a plausible prefix would
/// satisfy the weaker claim, and a prefix is precisely the failure mode the
/// filter's raw-state-machine design exists to prevent.
#[test]
fn a_brotli_content_stream_decodes_to_its_operators() {
    let decoded = page_content("brotli-content.pdf");
    assert_eq!(
        decoded,
        b"1 0 0 RG 0.9 0.2 0.2 rg\n20 20 120 80 re f\n0.2 0.4 0.9 rg\n60 60 120 80 re f\n\n",
        "the decoded content stream must be the authored operators exactly"
    );
}

/// ★ Brotli + PNG-Up predictor produces the SAME content as Brotli alone.
///
/// This is the assertion that would catch a Brotli-specific predictor
/// regression — including the tempting "Brotli needs its own predictor
/// variant", which the extension explicitly does not create.
#[test]
fn the_png_up_predictor_round_trips_over_brotli() {
    assert_eq!(
        page_content("brotli-with-predictor.pdf"),
        page_content("brotli-content.pdf"),
        "/Predictor 12 over BrotliDecode must decode to the same operators as \
         the unpredicted stream; Table 8 applies verbatim and there is no \
         Brotli-specific predictor"
    );
}

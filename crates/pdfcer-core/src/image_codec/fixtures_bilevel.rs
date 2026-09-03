//! # Synthetic CCITT and JBIG2 codestreams for the bilevel adapters' tests
//!
//! **GENERATED FILE — do not hand-edit.** Regenerate with:
//!
//! ```text
//! python tools/gen-bilevel-fixtures.py
//! ```
//!
//! ## Provenance (docs/LEGAL.md §5)
//!
//! Every byte array below was produced on a developer machine by
//! `tools/gen-bilevel-fixtures.py` from a **16 x 4 pixel pattern authored
//! for this project**. Nothing here was downloaded. This matters more
//! than usual for these two codecs: the obvious sources — pdf.js's and
//! PDFBox's CCITT/JBIG2 regression suites, and the public JBIG2 test
//! streams — are all third-party files of unknown provenance, which
//! `LEGAL.md` §5 forbids outright. Generating them is not a convenience,
//! it is the only permitted route.
//!
//! - **CCITT**: encoded by **libtiff** (through Pillow 12.1.0's TIFF
//!   writer, `compression='group3'`/`'group4'`) and lifted out of the
//!   resulting file's strip. A TIFF strip compressed with tag 259 = 3 or
//!   4 *is* a CCITT Group 3/4 bit stream, so no transcoding happens.
//! - **JBIG2**: assembled segment by segment from T.88 clause 7's layout,
//!   carrying the Group 4 payload above as an MMR generic region
//!   (§6.2.6 defines MMR coding as T.6). There is no pure-Python JBIG2
//!   encoder, and `hayro-jbig2` publishes no test vectors — its crate
//!   package excludes `/tests/`.
//!
//! ## The polarity derivation these fixtures rest on
//!
//! libtiff's fax codec is purely bit-based: it codes runs of 0 bits as
//! T.4 "white" and runs of 1 bits as T.4 "black", irrespective of the
//! PhotometricInterpretation tag. Pillow packs a mode-`'1'` white pixel
//! as a 1 bit. So a T.4 white run corresponds to a Pillow **black**
//! pixel, and — since `/BlackIs1` defaults to false, making a T.4 white
//! run decode to sample 1 — the decoded PDF samples are the complement
//! of Pillow's raw bitmap. The generator therefore builds the source
//! image as the *visual complement* of the picture these fixtures
//! represent.
//!
//! That derivation is asserted, not assumed: [`BILEVEL_16X4_SAMPLES`] is
//! the byte-exact expected output and every fixture here is checked
//! against it.
//!
//! ## Why byte arrays rather than files
//!
//! Hermetic tests, exactly as in the sibling `fixtures` module: embedding
//! the bytes means the tests run identically from any working directory,
//! under `cargo test`, under `cargo fuzz`, and in a `wasm32` check.

// Shared by TWO test suites — `pdfcer-core`'s codec tests and
// `pdfcer-render`'s rasterizer tests (which pull this file in with
// `#[path]`) — and neither uses the whole set.
#![allow(dead_code)]

/// The expected decoded samples for every fixture in this file:
/// 16 x 4, one bit per sample, **PDF convention (0 = black)**, rows
/// padded to the 2-byte stride.
///
/// Table 11's `BlackIs1` describes 1-means-black as "the reverse of the
/// normal PDF convention for image data", and its default is **false** —
/// so with no `/BlackIs1` entry the filter must emit 0 for a black pixel.
/// With DeviceGray's default `Decode [0 1]` at 1 bit per component,
/// sample 0 maps to grey 0.0, which is black. This constant is that rule
/// made byte-exact.
///
/// All three CCITT fixtures and the JBIG2 fixture must decode to exactly
/// these bytes. That the two codecs agree is the point: CCITT reaches it
/// through `/BlackIs1` false -> `invert_black` false, JBIG2 through the
/// unconditional inverse of T.88's "1 is black", and the two routes have
/// nothing in common but the answer.
pub const BILEVEL_16X4_SAMPLES: &[u8] = &[0x00, 0xFF, 0xFF, 0x00, 0x55, 0xAA, 0x3C, 0x3C];

/// The same picture with **1 = ink**, i.e. the exact bitwise complement
/// of [`BILEVEL_16X4_SAMPLES`].
///
/// This is what `/BlackIs1 true` must produce — Table 11's "1 bits shall
/// be interpreted as black pixels" — so the pair pins the polarity from
/// both sides. A decoder with the flag wired backwards passes neither.
pub const BILEVEL_16X4_INK: &[u8] = &[0xFF, 0x00, 0x00, 0xFF, 0xAA, 0x55, 0xC3, 0xC3];

/// Group 4 (pure two-dimensional, T.6) — Table 11's **K < 0**.
///
/// Encoded by libtiff via Pillow (`compression='group4'`) and lifted out
/// of the TIFF strip; see `tools/gen-bilevel-fixtures.py` for the
/// polarity derivation that makes the source image the visual complement
/// of the decoded picture. libtiff terminates a Group 4 strip with EOFB,
/// so this fixture also exercises `EndOfBlock`'s default of **true**.
///
/// This same byte string is reused as the MMR payload of
/// [`JBIG2_MMR_16X4`], because T.88 §6.2.6 codes an MMR generic region
/// with exactly T.6.
pub const CCITT_G4_16X4: &[u8] = &[
    0x26, 0xA2, 0xCC, 0xC5, 0x26, 0xA8, 0x8E, 0x88, 0xE8, 0x22, 0x9C, 0xA1, 0xCA, 0x1C, 0x25, 0xB1,
    0x8C, 0x5C, 0x00, 0x40, 0x04,
];

/// Group 3 one-dimensional (T.4 §4.1) — Table 11's **K = 0**, the
/// default encoding scheme.
///
/// libtiff writes an EOL pattern before each line, so this fixture also
/// covers `EndOfLine`. Table 11 says the filter "shall always accept
/// end-of-line bit patterns" whatever the flag says, and `hayro-ccitt` is
/// unconditionally lenient about them — which is why the same bytes
/// decode identically with `/EndOfLine` absent, true, or false.
pub const CCITT_G3_1D_16X4: &[u8] = &[
    0x00, 0x13, 0x51, 0x66, 0x00, 0x33, 0x14, 0x00, 0x4D, 0x50, 0xE8, 0x74, 0x3A, 0x74, 0x3A, 0x1D,
    0x0E, 0x80, 0x04, 0xD7, 0xB7, 0x78,
];

/// Group 3 mixed one- and two-dimensional (T.4 §4.2) — Table 11's
/// **K > 0**.
///
/// Produced by setting TIFF tag 292 (T4Options) bit 0, libtiff's switch
/// for 2-D coding. Each line carries a tag bit saying whether it was
/// coded 1-D or 2-D, which is the structural difference from
/// [`CCITT_G3_1D_16X4`] and the reason `K` must be trichotomous rather
/// than boolean. Table 11 also forbids distinguishing between different
/// positive `K` values, so this fixture must decode identically for
/// `/K 1`, `/K 4` and `/K 40`.
pub const CCITT_G3_2D_16X4: &[u8] = &[
    0x00, 0x19, 0xA8, 0xB3, 0x00, 0x11, 0x98, 0xA0, 0x03, 0x35, 0x43, 0xA1, 0xD0, 0xE9, 0xD0, 0xE8,
    0x74, 0x3A, 0x00, 0x15, 0x8C, 0x62, 0xE0,
];

/// A complete **embedded** JBIG2 stream (T.88 Annex D.3): a page
/// information segment followed by an immediate generic region segment,
/// MMR-coded, with no `/JBIG2Globals` needed.
///
/// Assembled byte by byte in `tools/gen-bilevel-fixtures.py` from T.88
/// clause 7's segment layout, carrying [`CCITT_G4_16X4`] as its MMR
/// payload — legal because §6.2.6 defines MMR coding as T.6. There is no
/// pure-Python JBIG2 encoder and every available JBIG2 test corpus is
/// third-party (`docs/LEGAL.md` §5), so assembling one is not a shortcut
/// but the only permitted route.
///
/// It must decode to [`BILEVEL_16X4_SAMPLES`] — the same bytes the CCITT
/// fixtures produce. T.88 §6.2.6 fixes MMR black at bitmap value 1 and
/// PDF's convention is the opposite, so the adapter's unconditional
/// inversion is exactly what makes the two agree.
pub const JBIG2_MMR_16X4: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x30, 0x00, 0x01, 0x00, 0x00, 0x00, 0x13, 0x00, 0x00, 0x00, 0x10, 0x00,
    0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x01, 0x26, 0x00, 0x01, 0x00, 0x00, 0x00, 0x27, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00,
    0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x26, 0xA2, 0xCC, 0xC5, 0x26,
    0xA8, 0x8E, 0x88, 0xE8, 0x22, 0x9C, 0xA1, 0xCA, 0x1C, 0x25, 0xB1, 0x8C, 0x5C, 0x00, 0x40, 0x04,
];

/// The page information segment of [`JBIG2_MMR_16X4`], **alone** — the
/// shape a `/JBIG2Globals` stream has (Table 12: "a stream containing the
/// JBIG2 global segments").
///
/// Paired with [`JBIG2_MMR_16X4_PAGE`]. Neither half decodes on its own:
/// the globals carry no region to draw, and the page carries no geometry
/// to draw it on. That is what makes the pair a real test of Table 12's
/// plumbing rather than a decoration — a decoder that silently ignored
/// `/JBIG2Globals` would fail with "missing page information" rather than
/// produce a wrong picture.
pub const JBIG2_MMR_16X4_GLOBALS: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x30, 0x00, 0x01, 0x00, 0x00, 0x00, 0x13, 0x00, 0x00, 0x00, 0x10, 0x00,
    0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The immediate generic region segment of [`JBIG2_MMR_16X4`], **alone**.
///
/// The image-stream half of the `/JBIG2Globals` pair. Decoding it without
/// [`JBIG2_MMR_16X4_GLOBALS`] must fail cleanly (no page information
/// segment); decoding it *with* them must produce
/// [`BILEVEL_16X4_SAMPLES`].
pub const JBIG2_MMR_16X4_PAGE: &[u8] = &[
    0x00, 0x00, 0x00, 0x01, 0x26, 0x00, 0x01, 0x00, 0x00, 0x00, 0x27, 0x00, 0x00, 0x00, 0x10, 0x00,
    0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x26, 0xA2, 0xCC,
    0xC5, 0x26, 0xA8, 0x8E, 0x88, 0xE8, 0x22, 0x9C, 0xA1, 0xCA, 0x1C, 0x25, 0xB1, 0x8C, 0x5C, 0x00,
    0x40, 0x04,
];

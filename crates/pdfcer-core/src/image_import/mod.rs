//! # Raster-image import — turning a PNG/JPEG/BMP/TIFF file into a PDF image XObject
//!
//! The **write** direction of `crate::image_codec`. That module reads image
//! XObjects *out of* a document so `pdfcer-render` can paint them; this one
//! builds an image XObject *from* an external raster file so
//! [`EditSession::add_image`](crate::edit::EditSession::add_image) can place
//! it on a page. The two never share code, because they are not inverses:
//! decoding must tolerate every producer's output, and importing must emit
//! exactly one shape that pdfcer chose.
//!
//! ## The governing design decision: RE-ENCODE NOTHING WE DO NOT HAVE TO
//!
//! Every branch here is organised around one rule:
//!
//! > If the source file's compressed bytes are *already* a legal PDF stream
//! > payload, they are copied **verbatim** and the PDF dictionary is written
//! > to describe them. pdfcer decodes and re-compresses only when the PDF
//! > object model cannot express the source's layout.
//!
//! Three reasons, in descending order of how much they matter:
//!
//! 1. **A re-encode is a generation loss.** JPEG is lossy; decoding and
//!    re-compressing a scan or a CAD export degrades it every time it is
//!    placed. There is no quality setting that makes this free, and the
//!    operator did not ask for it.
//! 2. **R28 gates the encoder.** `ROADMAP.md` R28: *"Read-compat only:
//!    pdfcer writes none of these codecs. No image encoder enters any pdfcer
//!    crate without a new decision record."* Verbatim passthrough introduces
//!    **no encoder** — the bytes were encoded by whatever produced the file.
//!    A transcode introduces one, and R28 held [`ImageCompression::Jpeg`] at
//!    a refusal until the operator made that decision on 2026-08-08. It is
//!    now implemented (see [`jpeg_encode`]), which changes what R28 means
//!    here but not the ordering of this list: an encoder now exists, and the
//!    default is still not to reach for it. R28's real subject was never
//!    difficulty — it is that an encoder is a permanent, licence-bearing
//!    surface that must be *chosen*, and choosing one does not make running
//!    it the right default.
//! 3. **It is faster and smaller.** A 4 MB JPEG stays 4 MB.
//!
//! ### Where the rule bites, per format
//!
//! | Source | Branch | PDF filter |
//! |---|---|---|
//! | JPEG, any baseline/extended/progressive frame | **verbatim** | `/DCTDecode` |
//! | PNG colour type 0/2/3, non-interlaced | **verbatim IDAT** | `/FlateDecode` + `/Predictor 15` |
//! | PNG colour type 4/6 (alpha interleaved in the rows) | decode + re-deflate | `/FlateDecode` |
//! | PNG interlaced (Adam7) | **refused by name** | — |
//! | BMP (no compressed form pdfcer accepts) | decode + deflate | `/FlateDecode` |
//! | TIFF, single-strip Deflate, no predictor, nothing to transform | **verbatim strip** | `/FlateDecode` |
//! | TIFF, everything else in the baseline | decode + deflate | `/FlateDecode` |
//! | TIFF, tiled / planar / CCITT / JPEG-in-TIFF / BigTIFF / … | **refused by name** | — |
//!
//! TIFF is the one row where the rule usually **cannot** be honoured, and the
//! reason is worth stating rather than reading as a shortcut: two of the three
//! compressions pdfcer accepts have no PDF encoder in the project at all (R28 —
//! LZW and PackBits are readable, neither is writable), a multi-strip image is
//! several independent zlib streams where a PDF stream is one, and any sample
//! that has to be byte-swapped, complemented or un-differenced has stopped
//! being the bytes the dictionary would describe. The narrow case where none
//! of that applies **is** passed through, and is verified before it is (see
//! [`tiff`]).
//!
//! Re-deflating a PNG is **lossless** — the pixels are bit-identical, only
//! the deflate stream differs — so branch 3 costs nothing but bytes. It is
//! taken only because PDF has no way to express an interleaved alpha
//! channel: §8.9.5 Table 89 gives an image one `/ColorSpace` and one
//! `/BitsPerComponent`, and opacity travels in a *separate* `/SMask` image.
//! De-interleaving requires the samples, and having the samples means the
//! predictor has already been undone, so the rows must be re-deflated.
//!
//! ### …but the rule is a DEFAULT, not a policy
//!
//! The operator asked (2026-08-08) for *"a user option for each image"*, so
//! verbatim passthrough is what happens when nobody says otherwise, not
//! what always happens. [`ImageCompression`] is that option, passed per
//! import through [`ImportOptions`] to [`import_with`]; [`import`] is the
//! same call at the default. The policy that actually ran — which is not
//! always the one asked for, because a BMP has no compressed bytes to keep
//! and a PNG already has nothing to gain — is reported in
//! [`ImportNotes::applied_compression`] beside the requested one, so a
//! substitution is never something to be discovered by diffing bytes.
//!
//! Re-encoding as JPEG ([`ImageCompression::Jpeg`]) was **refused by name**
//! for the whole of this module's first life: it needs an encoder, and R28
//! forbids one entering the project without a dated decision record. That
//! decision was made on 2026-08-08 and the policy is now implemented in
//! [`jpeg_encode`] — read that module before touching anything
//! four-component, because the CMYK polarity trap of decision 006 exists on
//! the write side too and getting it wrong produces a photographic negative
//! that *looks deliberate*. Resolution capping remains scoped out, but the
//! reason has now changed: it is no longer "there is no encoder to write the
//! smaller image back out" but "a resampler is a visible quality decision
//! (box vs. Lanczos) that deserves to be chosen on its own merits."
//!
//! ## The PNG passthrough, and why it is sound
//!
//! This is the one non-obvious claim in the module, so it is argued rather
//! than asserted. **The spec RAG does not state it** — it is an inference
//! from four separately-sourced facts, every one of them quoted here so a
//! future reader can check the reasoning rather than trust it:
//!
//! 1. **The container is the same.** ISO 32000-1 §7.4.4.1 defines
//!    FlateDecode entirely by reference to *"Internet RFCs 1950 … and
//!    1951"* — a zlib stream, 2-byte header + deflate + Adler-32. RFC 2083
//!    §10.1 wraps PNG's `IDAT` payload in exactly the same zlib stream.
//! 2. **The prediction is the same.** §7.4.4.4: *"a `Predictor` value
//!    greater than or equal to 10 shall indicate that a PNG predictor is in
//!    use; the specific predictor function used shall be explicitly encoded
//!    in the incoming data."* And: *"The postprediction data for each
//!    PNG-predicted row shall begin with an explicit algorithm tag."* That
//!    tag byte is RFC 2083 §6's filter-type byte, in the same place, with the
//!    same five values and the same reconstruction formulas (§7.4.4.4 does
//!    not reproduce them — it cites RFC 2083).
//! 3. **The row geometry is the same.** §7.4.4.4: rows run *"from the top
//!    row to the bottom row and, within a row, from left to right"*; *"A row
//!    shall occupy a whole number of bytes, rounded up if necessary"*;
//!    samples are *"packed into bytes from high-order to low-order bits"*;
//!    and out-of-image components *"shall be 0"* (RFC 2083's
//!    `Raw(x) = 0 for x < 0` and `Prior(x) = 0` on the first row). The
//!    left-neighbour distance is `bpp = max(1, ceil(Colors × BPC / 8))` in
//!    both.
//! 4. **The parameters line up 1:1.** Table 8's `/Colors` is PNG's channel
//!    count, `/BitsPerComponent` is PNG's bit depth, `/Columns` is PNG's
//!    width.
//!
//! Therefore a non-interlaced PNG's concatenated `IDAT` payload **is** a
//! conforming `/FlateDecode` stream under
//! `<< /Predictor 15 /Colors n /BitsPerComponent d /Columns w >>`, byte for
//! byte, with no arithmetic performed on it at all.
//!
//! `/Predictor 15` rather than 10–14 because §7.4.4.4 makes 10–15 identical
//! on decode (*"The value of `Predictor` supplied by the decoding filter
//! need not match the value used when the data was encoded if they are both
//! greater than or equal to 10"*) and Table 10 defines 15 as *"PNG
//! prediction (on encoding, PNG optimum)"* — which is precisely what a PNG
//! encoder does when it picks a filter per row. 15 is the honest label.
//!
//! **Interlaced PNG is the exception that proves the inference.** Adam7
//! splits the image into seven passes, each with its own row width and its
//! own `Prior(x) = 0` first row. PDF has no interlacing — §7.4.4.4's
//! first-row rule applies exactly once — so the byte stream means something
//! different in the two formats and cannot be reused. pdfcer has no
//! de-interlacer, so an interlaced PNG is **refused by name**
//! ([`ImageImportError::Unsupported`] with feature `"PNG/interlaced"`)
//! rather than silently mis-decoded into stripes.
//!
//! ## What this module refuses, and why each refusal is by name
//!
//! Every refusal carries a stable feature key, in the R27 tradition
//! (*"Unsupported codec sub-features fail clean and are counted BY NAME.
//! Never a grey box, never a guessed pixel, never a generic 'decode
//! failed.'"*), and every operator-facing message names the formats that
//! **do** work. A drag-and-drop gesture that fails silently, or that places
//! something wrong-looking, is worse than one that says *"pdfcer places PNG,
//! JPEG, BMP and TIFF; that file is a WebP."*
//!
//! | Refusal | Key | Why not supported |
//! |---|---|---|
//! | BigTIFF | `BigTIFF` | 8-byte offsets and a different directory layout — a different parser, not a superset of classic TIFF. Declined under its OWN name so the message ("pdfcer places … TIFF") is advice rather than a contradiction. |
//! | TIFF sub-features | `TIFF/tiled`, `TIFF/ccitt-g4`, … | Classic TIFF **is** placed ([`tiff`]); the sub-features it declines carry their own keys. The largest gap is CCITT G3/G4, which is what fax-lineage scanners emit — pdfcer has a fuzzed CCITT decoder, but it is reachable only through a PDF image dictionary. |
//! | GIF | `GIF` | LZW with GIF's own LSB bit packing and sub-block framing, plus animation frames and a per-frame transparent index. A real surface, not a cheap one. |
//! | WebP / AVIF / HEIC / JPEG 2000 files | `WEBP` etc. | Need a decoder dependency. Project rule 13 makes adding one a licence-classified, `PRIOR_ART.md`-recorded decision, not something a feature Pass does in passing. |
//! | Arithmetic / lossless / differential / 12-bit JPEG | `JPEG/arithmetic` … | These are different codecs wearing JPEG's marker syntax. `/DCTDecode` is defined against *"the JPEG baseline format"* (§7.4.8) plus the PDF 1.3 progressive extension; nothing else is in scope. |
//! | Adam7 PNG | `PNG/interlaced` | See above. |
//! | RLE / bitfield BMP | `BMP/rle8` … | Uncompressed `BI_RGB` only. |
//!
//! ## Colour, and the trap already documented in this project
//!
//! **CMYK JPEGs are embedded exactly as they are stored, with no `/Decode`
//! array.** This is R29 applied to the write direction: *"pdfcer never
//! applies an 'Adobe CMYK inversion' … `/Decode` is the sole polarity
//! control."* Decision 006 established that no shipping PDF engine inverts,
//! that the word "invert" appears zero times in Adobe TN #5116, and that
//! marker-gated inversion has been shipped and reverted twice upstream.
//! Since the codestream travels verbatim, its APP14 marker travels with it
//! and §7.4.8 Table 13's precedence chain (marker outranks `/DecodeParms`
//! outranks the component-count default) resolves in the reader exactly as
//! it did in the source file.
//!
//! The one shape R30 names — four components, effective `ColorTransform` 0,
//! no `/Decode` — is **reported, never repaired**:
//! [`ImportNotes::cmyk_polarity_unverifiable`]. pdfcer is passing through
//! bytes whose polarity nothing in the file declares, and it says so.
//!
//! `/DecodeParms /ColorTransform` is deliberately **not** written. Table 13:
//! *"If the encoding algorithm has inserted the Adobe-defined marker code …
//! the value of this dictionary entry shall be ignored."* Writing it would
//! be inert on every marked JPEG and would state a second, possibly
//! disagreeing opinion on every unmarked one. The default rule (1 for three
//! components, 0 otherwise) is already what the source encoder assumed.
//!
//! ## Spec sources
//!
//! - `iso32000__s__8.9.md` — §8.9.4 image coordinate system, §8.9.5 Table 89
//! - `iso32000__s__8.9.5.2.md` — Table 90 default `/Decode`, §8.9.6.4 colour-key `/Mask`
//! - `filter__dct.md` — §7.4.8, Table 13, the APP14 layout, R29/R30
//! - `filter__flate.md` — §7.4.4, Table 8
//! - `filter__predictors.md` — §7.4.4.4, Tables 9/10
//! - `color__indexed.md` — §8.6.6.3 `[/Indexed base hival lookup]`, `hival ≤ 255`
//! - `iso32000__s__7.3.8.md` — Table 5 stream entries, `/Length`
//! - RFC 2083 (PNG) and ITU-T T.81 (JPEG) for the container formats, which
//!   ISO 32000-1 cites but does not reproduce
//!
//! ### Recorded spec gap (reported, not worked around)
//!
//! **§11.6.5.3 (soft-mask images) is not in the spec RAG.** Clause 11 is
//! entirely uningested, so the corpus cannot answer: whether an `/SMask`
//! image's `/ColorSpace` must be `DeviceGray`; whether it may itself carry
//! `/Mask` or `/SMask`; `/Matte`'s semantics; or **whether an `/SMask`'s
//! `/Width`/`/Height` must equal the base image's**. What this module emits
//! is therefore the conservative intersection of what §8.9 Table 89 does
//! say and what every reader is known to accept: a `DeviceGray` image of
//! **identical dimensions**, with no mask of its own, no `/Matte`, and no
//! `/Decode`. `pdfcer-spec-librarian` should be dispatched for §11.6.5.3
//! before anything here is relaxed.

pub mod bmp;
pub mod jpeg;
/// The `/DCTDecode` **writer** behind [`ImageCompression::Jpeg`].
///
/// The module is `pub` but exports **nothing**: its entry point is
/// `pub(crate)` on purpose, because a second public way to produce an
/// [`ImportedImage`] would be a way to skip [`import_with`]'s policy
/// bookkeeping — and an image whose `applied_compression` disagrees with
/// its bytes is precisely the disclosure failure rule 4 exists to prevent.
///
/// What it publishes is the **documentation**: the licence argument for
/// pdfcer's first (and only) image encoder, and the CMYK polarity derivation
/// that decision 006 makes load-bearing. Read it before touching the
/// four-component path.
pub mod jpeg_encode;
pub mod png;
pub mod tiff;

use crate::image_codec::{MAX_IMAGE_DIMENSION, MAX_IMAGE_PIXELS, MAX_IMAGE_SAMPLE_BYTES};

/// The raster container formats pdfcer can place on a page.
///
/// Deliberately small, and deliberately not a superset of what
/// [`sniff`] can *recognise*: recognising GIF, WebP, HEIC and BigTIFF is how
/// pdfcer refuses them by name instead of saying "unknown file".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ImageFormat {
    /// Portable Network Graphics (RFC 2083).
    Png,
    /// JPEG/JFIF/EXIF (ITU-T T.81 codestream in a JFIF or EXIF container).
    Jpeg,
    /// Windows Bitmap (`BITMAPINFOHEADER`, uncompressed `BI_RGB`).
    Bmp,
    /// Tagged Image File Format (TIFF 6.0, classic 42-magic — **not**
    /// BigTIFF, which [`sniff`] declines under its own name).
    ///
    /// Only the baseline subset [`tiff`] accepts; everything else is refused
    /// with a stable feature key rather than mis-decoded. See that module's
    /// docs for the accepted/refused tables.
    Tiff,
}

impl ImageFormat {
    /// A short, stable, operator-facing name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Bmp => "BMP",
            Self::Tiff => "TIFF",
        }
    }
}

/// The formats [`import`] accepts, as one sentence, for every refusal
/// message.
///
/// A single constant rather than a phrase repeated per error, because the
/// day a format is added is the day the third copy of that sentence starts
/// lying — and a stale "PNG, JPEG or BMP" in one error path is exactly the
/// kind of drift nothing tests.
pub const SUPPORTED_FORMATS: &str = "PNG, JPEG, BMP and TIFF";

/// Why an image file could not be turned into a PDF image XObject.
///
/// Every variant names the specific thing pdfcer would not do. There is no
/// generic "could not read image" — see the module docs on refusing by name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ImageImportError {
    /// The file is a raster image in a format pdfcer does not place.
    ///
    /// `format` is the format pdfcer *recognised* (`"TIFF"`, `"GIF"`, …), so
    /// the operator is told what they actually handed over rather than that
    /// something unspecified went wrong.
    #[error("pdfcer does not place {format} images — it places {SUPPORTED_FORMATS}")]
    UnsupportedFormat {
        /// The recognised container format's short name.
        format: &'static str,
    },

    /// The bytes are not a raster image pdfcer recognises at all.
    #[error(
        "this file is not a {SUPPORTED_FORMATS} image (its first bytes match no known image format)"
    )]
    NotAnImage,

    /// The format is supported but the file uses a sub-feature that is not.
    ///
    /// `feature` is a stable key (`"PNG/interlaced"`, `"JPEG/arithmetic"`,
    /// `"BMP/rle8"`), suitable for counting, in the R27 tradition.
    #[error("this image uses {feature}, which pdfcer cannot place")]
    Unsupported {
        /// The stable feature key.
        feature: &'static str,
    },

    /// The container is malformed.
    #[error("this image file is damaged or truncated: {detail}")]
    Corrupt {
        /// What specifically did not parse.
        detail: String,
    },

    /// The image exceeds pdfcer's decode ceilings ([`MAX_IMAGE_PIXELS`],
    /// [`MAX_IMAGE_DIMENSION`], [`MAX_IMAGE_SAMPLE_BYTES`]).
    ///
    /// The same ceilings the *decode* path enforces, deliberately: an image
    /// pdfcer would refuse to render is an image pdfcer should refuse to
    /// place, or placing it would produce a document pdfcer cannot open.
    #[error("this image exceeds pdfcer's size ceilings ({MAX_IMAGE_PIXELS} pixels maximum)")]
    TooLarge,

    /// The image has a zero width or height.
    ///
    /// Refused rather than placed: §8.9.4 maps the image onto the unit
    /// square, and an empty sample grid has no meaningful mapping — every
    /// reader would do something different with it.
    #[error("this image has no pixels ({width}×{height})")]
    Empty {
        /// Declared width in samples.
        width: u32,
        /// Declared height in samples.
        height: u32,
    },

    /// [`ImageCompression::Jpeg`]'s `quality` is outside 1–100.
    ///
    /// # Why this refuses rather than clamps
    ///
    /// A clamp would be pdfcer **choosing an encoder setting the operator did
    /// not choose** and then baking the result permanently into a document.
    /// Rule 4 tolerates an inference only when it is disclosed *before* it
    /// becomes document state, and a quality of 0 or 255 is not an inference
    /// worth disclosing — it is a plainly out-of-domain number with no
    /// defensible reading. (`0` in particular is not "maximum compression":
    /// libjpeg's quality scale, which `jpeg-encoder` reproduces, is defined
    /// on 1–100 and divides by zero outside it.) Saying so and changing
    /// nothing is both cheaper and more honest than picking a value and
    /// hoping the operator notices.
    ///
    /// Checked before the file is even parsed, so the operator learns the
    /// real blocker rather than a second-order complaint about the image.
    #[error(
        "JPEG quality must be between 1 and 100 — {quality} is outside that range. \
         pdfcer does not clamp it: a quality pdfcer picked is a setting you did not pick, \
         and the result would be stored permanently."
    )]
    InvalidQuality {
        /// The value the caller supplied.
        quality: u8,
    },

    /// The requested [`ImageCompression`] policy cannot be applied to *this*
    /// image, for a reason specific to the image rather than to pdfcer.
    ///
    /// Distinct from [`Self::Unsupported`], which means "pdfcer cannot place
    /// this file at all": here the image places perfectly well under another
    /// policy, and `reason` says which property of the image rules this one
    /// out. Naming the property rather than the policy is what lets the
    /// operator act — "use `passthrough`" is only useful advice if they know
    /// what they are giving up.
    ///
    /// The one case today is a colour-key `/Mask` under
    /// [`ImageCompression::Jpeg`] — see
    /// [`ImportedImage::color_key_mask`] and §8.9.6.4.
    #[error("pdfcer cannot re-encode this image as {policy}: {reason}")]
    CompressionRefused {
        /// The policy name the operator asked for.
        policy: &'static str,
        /// The property of *this* image that rules the policy out, phrased
        /// as a clause that completes the sentence above.
        reason: &'static str,
    },

    /// The source could not be decoded for a policy that needs its samples.
    ///
    /// Only reachable on [`ImageCompression::Lossless`] applied to a JPEG,
    /// which is the one policy that must run the codec. Carries the codec's
    /// own diagnosis rather than flattening it, so an arithmetic-coded or
    /// truncated codestream still reports what it actually was.
    #[error("this image could not be decoded for lossless storage: {detail}")]
    DecodeFailed {
        /// The codec's own diagnosis.
        detail: String,
    },

    /// Re-compression failed (a `flate2`/`miniz_oxide` error).
    ///
    /// Only reachable on the decode-and-re-deflate branch, and effectively
    /// only on allocation failure — but it is a real fallible step and is
    /// not swallowed.
    #[error("the image could not be re-compressed: {0}")]
    Compress(String),
}

/// A PDF colour space an imported image can land in.
///
/// The set is closed at four because those are the four §8.6 spaces whose
/// sample layout an ordinary raster file can be mapped onto without
/// inventing colorimetry pdfcer does not have. `CalRGB`/`Lab`/`ICCBased`/
/// `Separation`/`DeviceN` are all reachable *from* a PDF but not *to* one
/// from a PNG or a JPEG — choosing one would be pdfcer asserting a
/// colour-management fact the source file did not state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImportColorSpace {
    /// `/DeviceGray` — 1 component, 0.0 black … 1.0 white (§8.6.4.2).
    DeviceGray,
    /// `/DeviceRGB` — 3 additive components (§8.6.4.3).
    DeviceRgb,
    /// `/DeviceCMYK` — 4 subtractive components (§8.6.4.4).
    DeviceCmyk,
    /// `[/Indexed /DeviceRGB hival <lookup>]` (§8.6.6.3).
    ///
    /// The base is always `DeviceRGB` here, because both palette sources
    /// pdfcer reads (PNG `PLTE`, BMP's `bmiColors`) are RGB triples. `hival`
    /// is `palette_entries - 1` and §8.6.6.3 caps it at 255, which is why an
    /// `Indexed` image can never carry `/BitsPerComponent 16`.
    Indexed {
        /// The largest valid index — `lookup.len() / 3 - 1`.
        hival: u8,
        /// `3 × (hival + 1)` bytes, one RGB triple per entry, consecutive
        /// (§8.6.6.3: *"The colour components for each entry in the table
        /// shall appear consecutively"*).
        lookup: Vec<u8>,
    },
}

impl ImportColorSpace {
    /// Components per sample, as `/DecodeParms /Colors` and a colour-key
    /// `/Mask` array both count them.
    ///
    /// **One** for `Indexed` — §8.9.6.4's *"number of colour components in
    /// the image's colour space"* is the index count, not the base space's,
    /// because the image's colour space *is* the `Indexed` space. The spec
    /// RAG flags that as an inference rather than a quotation; it is the
    /// only reading consistent with `Decode` defaulting to `[0 N]` (two
    /// entries) for `Indexed` in Table 90.
    #[must_use]
    pub const fn components(&self) -> u8 {
        match self {
            Self::DeviceGray | Self::Indexed { .. } => 1,
            Self::DeviceRgb => 3,
            Self::DeviceCmyk => 4,
        }
    }
}

/// Which PDF filter the imported stream bytes are already encoded with.
///
/// Note what this type does **not** have: a "raw/uncompressed" variant.
/// Every branch of this module emits a compressed stream, because an
/// uncompressed image stream is never the right answer for a file pdfcer is
/// adding to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImportFilter {
    /// `/DCTDecode` — the source JPEG codestream, byte for byte (§7.4.8).
    DctDecode,
    /// `/FlateDecode` with no predictor — a re-deflated sample buffer.
    Flate,
    /// `/FlateDecode` with `/Predictor 15` — a PNG `IDAT` payload, byte for
    /// byte, described by Table 8 parameters (§7.4.4.4).
    FlatePngPredictor {
        /// `/Colors`.
        colors: u8,
        /// `/BitsPerComponent` (the predictor's, which equals the image's).
        bits_per_component: u8,
        /// `/Columns` — the image width in samples.
        columns: u32,
    },
}

/// A soft mask (`/SMask`) accompanying an imported image.
///
/// Always the same dimensions and bit depth as the base image, always
/// `DeviceGray`, always plain `/FlateDecode`. See the module docs' recorded
/// spec gap: §11.6.5.3 is not in the RAG, so this is the conservative shape
/// rather than a sourced one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SoftMask {
    /// Width in samples — equal to the base image's.
    pub width: u32,
    /// Height in samples — equal to the base image's.
    pub height: u32,
    /// 8 or 16, matching the base image's alpha channel depth.
    pub bits_per_component: u8,
    /// zlib-compressed `DeviceGray` samples, row-major, high-order bit
    /// first, each row byte-aligned (§8.9.3).
    pub data: Vec<u8>,
}

/// Why pdfcer decoded and re-compressed instead of passing bytes through.
///
/// Carried on the outcome so the operator can be told *why* their file grew
/// or changed shape, rather than merely that it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecompressReason {
    /// The source interleaves an alpha channel with the colour channels, and
    /// PDF carries opacity in a separate `/SMask` image (§8.9.5 Table 89
    /// gives an image one `/ColorSpace`), so the samples had to be split.
    AlphaSplit,
    /// The source format has no compressed form PDF can express — an
    /// uncompressed BMP.
    NoCompressedSource,
    /// The source **is** compressed, but its compressed bytes could not be
    /// reused as a PDF stream payload — the ordinary TIFF case.
    ///
    /// Distinct from [`Self::NoCompressedSource`] because the two are
    /// different facts about the operator's file, and a front end that
    /// conflates them tells a TIFF owner their file was uncompressed. There
    /// were bytes; they were simply not reusable, for one of four reasons,
    /// none of which pdfcer can do anything about:
    ///
    /// - the codec has no PDF **encoder** in pdfcer (R28 — LZW and PackBits
    ///   are readable but pdfcer writes neither);
    /// - the image is stored in several independently-compressed strips, and
    ///   two concatenated zlib streams are not one zlib stream;
    /// - the samples needed a transform before they meant what the PDF
    ///   dictionary would say they mean (a 16-bit little-endian byte swap, a
    ///   `WhiteIsZero` complement, `Predictor 2` un-differencing);
    /// - an extra sample had to be de-interleaved.
    ///
    /// Lossless in every case: the pixels are exactly the source's.
    SourceCodecNotReusable,
    /// The operator asked for [`ImageCompression::Lossless`] on a source
    /// stored in a lossy codec, so it was decoded and stored as
    /// `/FlateDecode`.
    ///
    /// One of the two reasons in this enum the operator **chose** (the other
    /// is [`Self::JpegRequested`]); [`Self::AlphaSplit`] and
    /// [`Self::NoCompressedSource`] are properties of the file. Distinguished
    /// because "you asked for this" and "your file forced this" deserve
    /// different sentences — and because a chosen reason is not a
    /// *substitution*, so a front end should not apologise for it.
    ///
    /// Costs bytes, never picture: the pixels are exactly the ones the lossy
    /// codestream decoded to.
    LosslessRequested,
    /// The operator asked for [`ImageCompression::Jpeg`], so the source was
    /// decoded and re-encoded as `/DCTDecode` — **a lossy act, on purpose**.
    ///
    /// The second reason in this enum the operator *chose*, and the only one
    /// that costs picture quality rather than only bytes.
    /// [`ImportNotes::jpeg_from_lossy`] says whether the source was already
    /// lossy, because a second DCT pass over existing artefacts compounds
    /// them rather than merely adding one generation of loss.
    JpegRequested,
}

impl RecompressReason {
    /// A stable key for the machine-readable channel.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::AlphaSplit => "alpha-split",
            Self::NoCompressedSource => "no-compressed-source",
            Self::SourceCodecNotReusable => "source-codec-not-reusable",
            Self::LosslessRequested => "lossless-requested",
            Self::JpegRequested => "jpeg-requested",
        }
    }
}

/// How an imported image's pixels are stored in the PDF.
///
/// # Why this is a per-image option and not a project-wide policy
///
/// The operator asked for it in those words (2026-08-08): *"to recompress or
/// not recompress should be a user option for each image and have settings
/// to tune compression."* One document legitimately mixes a 600 dpi scan
/// that must not be touched with a screenshot that should be squeezed, so a
/// global switch would be wrong for one of them whichever way it was set.
///
/// # Why [`Passthrough`](Self::Passthrough) is the default
///
/// Because opting **into** a loss should be a choice, and opting **out** of
/// one should not be something you have to know to do. A silently-degraded
/// image is precisely what rule 4 exists to prevent: the damage is
/// invisible at editing zoom, permanent, and compounds on every placement.
///
/// # What each policy costs
///
/// | Policy | JPEG source | PNG source | BMP source |
/// |---|---|---|---|
/// | `Passthrough` | codestream verbatim, `/DCTDecode` | `IDAT` verbatim, `/FlateDecode` + `/Predictor 15` | nothing to pass through → lossless Flate |
/// | `Lossless` | decode + `/FlateDecode` — **much larger**, and recovers nothing | already lossless; the verbatim `IDAT` is kept | lossless Flate |
/// | `Jpeg { quality }` | decode + re-encode — **a SECOND lossy pass**, artefacts compound | decode + lossy encode — exact pixels degraded once | decode + lossy encode |
///
/// The `Lossless`-on-a-JPEG row is the one that most needs saying out loud,
/// and [`ImportNotes::lossless_from_lossy`] says it: **converting a JPEG to
/// lossless storage does not recover quality already lost.** It preserves
/// exactly the pixels the JPEG decodes to — artefacts included — while
/// typically multiplying the stored size by five to twenty. It is the right
/// move before further editing, and the wrong move if the goal was a better
/// picture.
///
/// # Still scoped out by name: downsampling / resolution capping
///
/// A "resample to at most N dpi" setting is deliberately **not** here, and
/// its original justification has now expired: it used to be that without an
/// encoder, downsampling would make files *bigger* (decode → resample →
/// `/FlateDecode`, which on photographic data is far larger than the DCT
/// stream it replaced). [`Jpeg`](Self::Jpeg) removes that objection
/// entirely.
///
/// What survives is the **second** reason, which was always the real one: a
/// resampler is a visible quality decision, not an implementation detail.
/// Box, bilinear and Lanczos differ in ways an operator can see — ringing on
/// hard edges, softness on text, moiré on a screened halftone — and picking
/// one silently inside a compression flag would be exactly the kind of
/// unannounced inference rule 4 forbids. It belongs in its own Pass with its
/// own disclosure, next to the resampler choice. Placing an image at a size
/// that implies a wasteful resolution is meanwhile **reported**, via
/// [`ImageAuthorDisclosures::effective_dpi`](crate::edit::ImageAuthorDisclosures::effective_dpi).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ImageCompression {
    /// Embed the source's own compressed bytes unchanged. **The default.**
    ///
    /// Not always literally achievable — a BMP has no compressed form, and a
    /// PNG with an interleaved alpha channel must be split — so the policy
    /// that actually ran is reported in [`ImportNotes::applied_compression`]
    /// rather than assumed.
    #[default]
    Passthrough,
    /// Store the decoded samples losslessly as `/FlateDecode`.
    ///
    /// A no-op for a source that was already stored losslessly: the verbatim
    /// bytes are kept, because re-deflating identical pixels is strictly
    /// worse than not doing it. The substitution is reported.
    Lossless,
    /// Re-encode as `/DCTDecode` at the given quality (1–100).
    ///
    /// **A deliberate, disclosed generation loss.** Held at a refusal until
    /// 2026-08-08 (R28 forbids an image encoder entering the project without
    /// a dated decision, and every credible JPEG encoder carries a licence
    /// consequence); the operator ruled, and it is now implemented against
    /// `jpeg-encoder` — see [`jpeg_encode`]'s module documentation for the
    /// licence argument, the CMYK polarity derivation, and what is refused.
    ///
    /// What it does and does not preserve:
    ///
    /// - The **base image** is decoded to samples and quantised. On a source
    ///   that was already a JPEG this is a **second** lossy pass, which
    ///   compounds artefacts rather than merely adding a generation; that
    ///   case is disclosed separately as [`ImportNotes::jpeg_from_lossy`].
    /// - An **`/SMask` is kept exactly** — §8.9.5 Table 89 makes the soft
    ///   mask its own image XObject, so it stays lossless `/FlateDecode`
    ///   rather than acquiring JPEG's worst artefacts along an alpha edge.
    /// - A **palette is expanded** to `/DeviceRGB` (JPEG has none) and
    ///   **16-bit samples are reduced to 8** (Table 89 fixes `/DCTDecode` at
    ///   8-bit).
    /// - A **colour-key `/Mask` is refused by name**
    ///   ([`ImageImportError::CompressionRefused`]): §8.9.6.4 matches exact
    ///   sample values, and lossy encoding moves them.
    ///
    /// `quality` outside 1–100 is refused rather than clamped — see
    /// [`ImageImportError::InvalidQuality`].
    Jpeg {
        /// Encoder quality, 1–100. Larger is better and bigger; 100 is still
        /// lossy.
        quality: u8,
    },
}

impl ImageCompression {
    /// A stable key for the machine-readable channel and for CLI parsing.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Passthrough => "passthrough",
            Self::Lossless => "lossless",
            Self::Jpeg { .. } => "jpeg",
        }
    }
}

/// Settings that shape how a source file becomes a PDF image XObject.
///
/// A struct rather than a bare [`ImageCompression`] argument because
/// "settings to tune compression" is explicitly plural in the operator's
/// request, and `#[non_exhaustive]` means a later Pass can add a resolution
/// cap or an encoder-specific knob without breaking a single caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ImportOptions {
    /// How the image's pixels are stored.
    pub compression: ImageCompression,
}

impl ImportOptions {
    /// The defaults: [`ImageCompression::Passthrough`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            compression: ImageCompression::Passthrough,
        }
    }

    /// Set the compression policy.
    #[must_use]
    pub const fn with_compression(mut self, compression: ImageCompression) -> Self {
        self.compression = compression;
        self
    }
}

/// Where the imported image's resolution claim came from.
///
/// pdfcer never *applies* this silently — see
/// [`ImportedImage::natural_size_pt`]. It is reported so an operator (or the
/// CLI's `--natural` flag) can decide, and so that "72 dpi" that was assumed
/// is distinguishable from "72 dpi" that the file actually said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DpiSource {
    /// Nothing in the file stated a resolution. 72 dpi (one pixel = one
    /// point) is pdfcer's stated assumption, not the file's claim.
    #[default]
    Assumed,
    /// A JFIF APP0 density field (units 1 = dpi, 2 = dots/cm).
    JfifDensity,
    /// EXIF `XResolution`/`YResolution` + `ResolutionUnit` (tags 0x011A/
    /// 0x011B/0x0128).
    ExifResolution,
    /// A PNG `pHYs` chunk with unit specifier 1 (metre).
    PngPhys,
    /// A BMP `biXPelsPerMeter`/`biYPelsPerMeter` pair.
    BmpPelsPerMeter,
    /// TIFF `XResolution`/`YResolution` (282/283) under a `ResolutionUnit`
    /// (296) of 2 (inch) or 3 (centimetre).
    ///
    /// Unit **1** deliberately does not produce this: it means "no absolute
    /// unit", so the two numbers are an aspect ratio rather than a
    /// resolution, and reading them as dpi would invent a physical size the
    /// file explicitly declined to state.
    TiffResolution,
}

/// The EXIF orientation an imported JPEG declared (`IFD0` tag `0x0112`).
///
/// # Why this is applied in the placement matrix rather than to the pixels
///
/// Rotating the pixels means decoding and re-encoding the JPEG — a
/// generation loss, and an image encoder R28 forbids. Rotating the
/// *placement* is exact, free, and reversible: every one of the eight EXIF
/// orientations is an isometry of the unit square, so it composes into the
/// `cm` matrix §8.9.4 already requires. The stored bytes stay verbatim and
/// the picture comes out the right way up.
///
/// The alternative — ignoring it — is not neutral. A phone or camera JPEG
/// with orientation 6 placed as stored appears rotated 90°, which reads as a
/// pdfcer bug rather than as a property of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Orientation {
    /// 1 — stored upright.
    #[default]
    Identity,
    /// 2 — mirrored left-to-right.
    MirrorHorizontal,
    /// 3 — rotated 180°.
    Rotate180,
    /// 4 — mirrored top-to-bottom.
    MirrorVertical,
    /// 5 — mirrored, then rotated 270° clockwise.
    MirrorRotate270,
    /// 6 — rotated 90° clockwise.
    Rotate90,
    /// 7 — mirrored, then rotated 90° clockwise.
    MirrorRotate90,
    /// 8 — rotated 270° clockwise (90° counter-clockwise).
    Rotate270,
}

impl Orientation {
    /// Map an EXIF tag value to an orientation; anything outside 1–8 (and 1
    /// itself) is [`Self::Identity`].
    #[must_use]
    pub const fn from_exif(value: u16) -> Self {
        match value {
            2 => Self::MirrorHorizontal,
            3 => Self::Rotate180,
            4 => Self::MirrorVertical,
            5 => Self::MirrorRotate270,
            6 => Self::Rotate90,
            7 => Self::MirrorRotate90,
            8 => Self::Rotate270,
            _ => Self::Identity,
        }
    }

    /// The EXIF tag value this orientation came from, or `None` for
    /// [`Self::Identity`] (which is both "1" and "absent", and disclosing
    /// "orientation 1 was applied" would be noise).
    #[must_use]
    pub const fn exif_value(self) -> Option<u8> {
        match self {
            Self::Identity => None,
            Self::MirrorHorizontal => Some(2),
            Self::Rotate180 => Some(3),
            Self::MirrorVertical => Some(4),
            Self::MirrorRotate270 => Some(5),
            Self::Rotate90 => Some(6),
            Self::MirrorRotate90 => Some(7),
            Self::Rotate270 => Some(8),
        }
    }

    /// Whether this orientation transposes the image — so the *displayed*
    /// width is the stored **height** and vice versa.
    #[must_use]
    pub const fn transposes(self) -> bool {
        matches!(
            self,
            Self::MirrorRotate270 | Self::Rotate90 | Self::MirrorRotate90 | Self::Rotate270
        )
    }

    /// The orientation as a matrix mapping the unit square onto itself,
    /// in PDF's `[a b c d e f]` row-vector form: `x' = a·u + c·v + e`,
    /// `y' = b·u + d·v + f` (§8.3 Table 57).
    ///
    /// `Do` paints the image into the unit square with (0, 0) at the
    /// **lower-left** of the square corresponding to image-space `(0, h)` —
    /// the image's bottom row (§8.9.4). Each matrix below is stated as the
    /// point map it performs on that square, so the reasoning is checkable:
    ///
    /// | Orientation | `(u, v) ↦` | Why |
    /// |---|---|---|
    /// | 1 | `(u, v)` | identity |
    /// | 2 | `(1−u, v)` | mirror across the vertical centre line |
    /// | 3 | `(1−u, 1−v)` | 180° |
    /// | 4 | `(u, 1−v)` | mirror across the horizontal centre line |
    /// | 5 | `(1−v, 1−u)` | mirror, then 270° CW |
    /// | 6 | `(v, 1−u)` | 90° CW: the stored top-left lands top-right |
    /// | 7 | `(v, u)` | mirror, then 90° CW |
    /// | 8 | `(1−v, u)` | 270° CW |
    #[must_use]
    pub const fn unit_square_matrix(self) -> [f64; 6] {
        match self {
            Self::Identity => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            Self::MirrorHorizontal => [-1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
            Self::Rotate180 => [-1.0, 0.0, 0.0, -1.0, 1.0, 1.0],
            Self::MirrorVertical => [1.0, 0.0, 0.0, -1.0, 0.0, 1.0],
            Self::MirrorRotate270 => [0.0, -1.0, -1.0, 0.0, 1.0, 1.0],
            Self::Rotate90 => [0.0, -1.0, 1.0, 0.0, 0.0, 1.0],
            Self::MirrorRotate90 => [0.0, 1.0, 1.0, 0.0, 0.0, 0.0],
            Self::Rotate270 => [0.0, 1.0, -1.0, 0.0, 1.0, 0.0],
        }
    }
}

/// Everything about an import the operator cannot see by looking at the
/// result — the raw material for
/// [`ImageAuthorDisclosures`](crate::edit::ImageAuthorDisclosures).
///
/// These are computed by [`import`], **before** anything is written to a
/// document, which is what lets a front end show them and let the operator
/// back out. Rule 4 is a requirement on disclosure, and a disclosure that
/// arrives only after the edit is committed is a report, not a disclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ImportNotes {
    /// pdfcer decoded and re-compressed rather than passing the source bytes
    /// through, for this reason. `None` means verbatim passthrough.
    pub recompressed: Option<RecompressReason>,
    /// The source had an alpha channel, written as a separate `/SMask`
    /// image (§8.9.5 Table 89).
    pub alpha_to_soft_mask: bool,
    /// A single fully-transparent colour (PNG `tRNS` on a truecolour or
    /// greyscale image) became a colour-key `/Mask` array (§8.9.6.4) —
    /// which costs nothing and preserves the verbatim passthrough.
    pub transparent_colour_to_mask: bool,
    /// A palette with per-entry alpha (PNG `tRNS` on colour type 3) became
    /// an 8-bit `/SMask`, while the indexed image data itself still passed
    /// through verbatim.
    pub palette_alpha_to_soft_mask: bool,
    /// The source carried embedded colour-management data (a PNG `iCCP`,
    /// `sRGB`, `gAMA` or `cHRM` chunk; a JPEG ICC profile in APP2) that
    /// pdfcer did **not** carry over. The image is placed in a device colour
    /// space, so colours may shift.
    ///
    /// Disclosed rather than refused, and not silently embedded: writing an
    /// `/ICCBased` space requires asserting the profile's component count
    /// matches `/N` (§8.6.5.5 Table 66: *"`N` shall be 1, 3, or 4"* and
    /// *"shall match the number of components actually in the ICC
    /// profile"*), and §8.6.5.5's Table 68 — the list of profile types a
    /// conforming **writer** may use — is a recorded gap in the spec RAG.
    /// Embedding a profile pdfcer cannot validate would be exactly the
    /// silent-guess this project refuses.
    pub colour_profile_dropped: bool,
    /// R30's shape: a four-component JPEG whose effective `ColorTransform`
    /// is 0 with no `/Decode`. Nothing in the file declares its polarity,
    /// and pdfcer is passing it through unchanged (R29). If it looks like a
    /// photographic negative, that is why.
    ///
    /// A statement about what is **stored**, not about the source file, so
    /// [`ImageCompression::Jpeg`] clears it: that policy writes YCCK
    /// (transform 2), which declares its own colour transform and is never
    /// the ambiguous shape — even when the source it re-encoded was.
    pub cmyk_polarity_unverifiable: bool,
    /// The JPEG uses the progressive frame type (SOF2).
    ///
    /// Legal from PDF 1.3 (§7.4.8: *"beginning with PDF 1.3, the
    /// `DCTDecode` filter shall support the progressive JPEG extension"*).
    /// Disclosed because §7.4.8 NOTE 5 is explicit that there is *"no
    /// benefit to using progressive JPEG for stream data that is embedded
    /// in a PDF file"* — it decodes slower and uses more memory. pdfcer
    /// embeds it anyway rather than transcoding: a transcode is a generation
    /// loss, and the default policy does not spend one to fix a performance
    /// note. The operator is told, so they can re-save as baseline if the
    /// file will be opened a lot — or ask for
    /// [`ImageCompression::Jpeg`], which writes baseline and therefore
    /// clears this flag. Like [`Self::cmyk_polarity_unverifiable`], it
    /// describes what is **stored**, not what was read.
    pub progressive_jpeg: bool,
    /// An EXIF orientation other than 1 was found and will be applied in the
    /// placement matrix rather than to the pixels.
    pub exif_orientation: Option<u8>,
    /// A 32-bit BMP's fourth byte per pixel was ignored.
    ///
    /// A `BITMAPINFOHEADER` 32-bit `BI_RGB` bitmap has **no alpha channel** —
    /// the fourth byte is padding, and many writers leave it zero. Treating
    /// it as opacity would make such an image entirely invisible, which is
    /// the single most spectacular way to get BMP wrong.
    pub bmp_fourth_byte_ignored: bool,
    /// The image uses a feature that requires a PDF version newer than 1.0,
    /// with the version it requires. Compared against the document's own
    /// header by [`EditSession::add_image`](crate::edit::EditSession::add_image).
    pub requires_pdf_version: Option<PdfFeature>,
    /// Where the resolution in [`ImportedImage::dpi`] came from.
    pub dpi_source: DpiSource,
    /// The compression policy the caller asked for.
    pub requested_compression: ImageCompression,
    /// The compression policy that actually ran.
    ///
    /// Equal to [`ImageCompression::Passthrough`] **exactly when the
    /// source's own compressed bytes are in the document unchanged** — which
    /// is the only definition of the word that means anything to an
    /// operator. A BMP asked to pass through lands on
    /// [`ImageCompression::Lossless`], because there were no compressed
    /// bytes to keep; a PNG asked to be stored losslessly lands on
    /// `Passthrough`, because its bytes already were.
    ///
    /// Its own field rather than an inference from
    /// [`Self::recompressed`] because the coordinating requirement is
    /// explicit: *"'I chose passthrough and got a re-encode' must not be
    /// discoverable only by diffing bytes."*
    pub applied_compression: ImageCompression,
    /// A LOSSY source was decoded and stored losslessly at the operator's
    /// request.
    ///
    /// Its own disclosure because the natural reading of "lossless" is
    /// wrong here in a way that costs money: it preserves exactly the pixels
    /// the JPEG decodes to — artefacts included — and **recovers nothing**,
    /// while typically multiplying the stored size several-fold. Someone who
    /// chose it hoping for a better picture got a bigger file and the same
    /// picture, and is entitled to be told so at the moment it happens.
    pub lossless_from_lossy: bool,
    /// [`ImageCompression::Jpeg`] was applied to a source that was **already
    /// lossy** — a second DCT pass over existing artefacts.
    ///
    /// Its own disclosure, separate from
    /// [`RecompressReason::JpegRequested`], because the two acts are not the
    /// same and rule 4 does not let them share a sentence. Re-encoding a PNG
    /// quantises exact pixels once: the loss is real, bounded, and roughly
    /// what the quality number predicts. Re-encoding a JPEG quantises pixels
    /// that already carry ringing, blocking and chroma bleed, and the second
    /// pass sharpens the first pass's artefacts into the picture rather than
    /// smoothing them — the damage **compounds**, is invisible at editing
    /// zoom, and is not recoverable by raising the quality.
    ///
    /// The honest advice when this is set is usually *"place the original
    /// instead"*, which is advice the operator can only take if they are told.
    pub jpeg_from_lossy: bool,
    /// The quality the JPEG encoder actually ran at, when
    /// [`ImageCompression::Jpeg`] was applied.
    ///
    /// Redundant with [`Self::applied_compression`]'s payload by
    /// construction, and kept anyway: a front end reading one field to answer
    /// "what quality is this stored at?" should not have to match on an enum
    /// whose other variants have no quality at all.
    pub jpeg_quality: Option<u8>,
    /// A multi-page TIFF's **further** pages, which pdfcer did not place.
    ///
    /// `0` for every single-page image and for every other format. A
    /// sheet-fed scanner's ordinary output is one TIFF with one IFD per
    /// sheet; pdfcer places the first and says how many it left, because
    /// silently dropping the rest is precisely the shape rule 4 forbids and
    /// refusing outright would turn the commonest scanner output into a dead
    /// end. See [`tiff`]'s module docs.
    ///
    /// A **lower bound** in the one pathological case where the IFD chain is
    /// damaged (a cycle, or more than the walk's ceiling): the walk stops
    /// rather than failing, because the first page is already complete and
    /// placeable.
    pub tiff_pages_ignored: u32,
    /// A TIFF's colour samples were stored **premultiplied by alpha**
    /// (`ExtraSamples 1`, "associated alpha" — TIFF 6.0 §18) and pdfcer
    /// un-premultiplied them so they could be carried by a straight-alpha
    /// `/SMask`.
    ///
    /// Its own disclosure because the reconstruction is **lossy in the
    /// low-alpha tail** and nothing on screen shows it: a sample stored at
    /// alpha 8/255 retains about three bits of colour, and no later step can
    /// put back what the premultiplication quantised away. Exact at full
    /// opacity, and good everywhere the pixel is actually visible.
    ///
    /// The alternative — keeping the premultiplied samples and declaring
    /// `/SMask << … /Matte [0 0 0] >>` (§11.6.5.3), which is exactly what
    /// TIFF's associated alpha is — is the faithful representation and is not
    /// yet available: [`SoftMask`] carries no matte through to the writer.
    pub tiff_associated_alpha_unpremultiplied: bool,
    /// A TIFF declared `PhotometricInterpretation 0` (`WhiteIsZero`) and
    /// pdfcer complemented every sample into `/DeviceGray`'s polarity
    /// (§8.6.4.2, where 0.0 is black).
    ///
    /// Not the "Adobe CMYK inversion" R29 forbids, and the difference is the
    /// whole point: R29 governs a polarity **nothing in the file declares**,
    /// where any inversion is a guess. Here a required tag declares it
    /// unambiguously, and the complement is exact at every bit depth. It is
    /// still disclosed, because the stored bytes are no longer the file's.
    pub tiff_white_is_zero_inverted: bool,
    /// Extra samples per pixel a TIFF declared as **unspecified**
    /// (`ExtraSamples 0`, or the tag omitted entirely) which pdfcer dropped
    /// rather than reading as opacity.
    ///
    /// Dropping is the safe direction: a channel of undeclared meaning read
    /// as alpha can make an otherwise-perfect image entirely invisible — the
    /// same trap [`ImportNotes::bmp_fourth_byte_ignored`] names for a 32-bit
    /// BMP's padding byte. The count is reported so an operator whose file
    /// *did* carry alpha in an undeclared channel can see why it vanished.
    pub tiff_extra_samples_dropped: u32,
    /// A TIFF `ColorMap`'s values were all ≤ 255 and were therefore read as
    /// **already 8-bit**, against TIFF 6.0 §16's own definition (*"0
    /// represents the minimum intensity and 65535 represents the maximum"*).
    ///
    /// A real standard-vs-practice divergence, not a shortcut: a long tail of
    /// writers stores 0–255 in those `SHORT`s, and a reader that trusts the
    /// specification renders such a palette almost entirely black. libtiff
    /// and everything built on it apply this heuristic. pdfcer applies it and
    /// says so, because its one false positive — a genuine 16-bit palette
    /// every component of which is darker than 1/257 of full intensity — is
    /// possible, however unlikely, and the operator can see the result.
    ///
    /// A candidate for an operator setting under R169; hard-coded here only
    /// because this module does not reach the settings surface.
    pub tiff_palette_assumed_8bit: bool,
    /// Bytes of the source **file** handed to [`import_with`].
    ///
    /// Populated for every policy, not only the re-encoding ones, so that
    /// "did this get smaller?" is answerable without diffing the output.
    /// Note that it counts the whole container — a JPEG's EXIF block, a
    /// PNG's ancillary chunks — while [`Self::stored_bytes`] counts only what
    /// reached the PDF, so a *verbatim* passthrough legitimately shrinks.
    pub source_bytes: usize,
    /// Bytes of the stream(s) actually stored: [`ImportedImage::data`] plus
    /// the [`SoftMask`]'s payload when one was written.
    ///
    /// Stream payloads only — the image XObject's dictionary, the content
    /// stream and the page patch are the placement's cost, not the image's,
    /// and folding them in here would make two unrelated numbers move
    /// together.
    pub stored_bytes: usize,
}

/// A PDF feature an imported image needs, and the version that introduced
/// it.
///
/// pdfcer does **not** rewrite the document's `%PDF-x.y` header to
/// accommodate one — that is a structural normalization of a file the
/// operator only asked to add an image to. It discloses the mismatch and
/// places the image, which is what every real producer does and what makes
/// the outcome inspectable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PdfFeature {
    /// `/BitsPerComponent 16` — Table 89, *"1, 2, 4, 8, or (PDF 1.5) 16"*.
    BitsPerComponent16,
    /// The progressive JPEG extension to `/DCTDecode` — §7.4.8, PDF 1.3.
    ProgressiveJpeg,
    /// A colour-key `/Mask` array — §8.9.6.4, PDF 1.3.
    ColourKeyMask,
    /// An `/SMask` soft-mask image — §8.9.5 Table 89, PDF 1.4.
    SoftMask,
}

impl PdfFeature {
    /// The `(major, minor)` version that introduced the feature.
    #[must_use]
    pub const fn since(self) -> (u8, u8) {
        match self {
            Self::ProgressiveJpeg | Self::ColourKeyMask => (1, 3),
            Self::SoftMask => (1, 4),
            Self::BitsPerComponent16 => (1, 5),
        }
    }

    /// A short operator-facing name for the feature.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::BitsPerComponent16 => "16-bit-per-channel image samples",
            Self::ProgressiveJpeg => "progressive JPEG",
            Self::ColourKeyMask => "colour-key transparency",
            Self::SoftMask => "a soft mask (alpha channel)",
        }
    }

    /// The later of two features, so one image reports one floor.
    fn max(self, other: Self) -> Self {
        if other.since() > self.since() {
            other
        } else {
            self
        }
    }
}

/// A raster image, parsed and converted into exactly the bytes and
/// dictionary values a PDF image XObject needs.
///
/// Produced by [`import`] and consumed by
/// [`EditSession::add_image`](crate::edit::EditSession::add_image). The two
/// steps are separate on purpose: this one is pure and fallible, touches no
/// document, and yields every disclosure — so a front end can show the
/// operator what will happen *before* the document changes.
///
/// `Eq` is deliberately **not** derived: [`Self::dpi`] is a pair of `f64`,
/// and a resolution is a measurement rather than an identity — two imports
/// of the same file are the same image whether or not their dpi compares
/// bitwise. `PartialEq` is kept because tests compare whole images.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ImportedImage {
    /// The container format it came from.
    pub format: ImageFormat,
    /// `/Width` — width in samples, as **stored** (before any EXIF
    /// orientation).
    pub width: u32,
    /// `/Height` — height in samples, as **stored**.
    pub height: u32,
    /// `/BitsPerComponent` — 1, 2, 4, 8 or 16 (Table 89).
    pub bits_per_component: u8,
    /// `/ColorSpace`.
    pub color_space: ImportColorSpace,
    /// Which filter [`Self::data`] is encoded with.
    pub filter: ImportFilter,
    /// The stream payload, ready to write with no further processing.
    pub data: Vec<u8>,
    /// `/SMask`, when the source carried opacity that only a soft mask can
    /// express.
    pub soft_mask: Option<SoftMask>,
    /// `/Mask` as a colour-key range array `[min₁ max₁ … minₙ maxₙ]`
    /// (§8.9.6.4), when the source declared exactly one transparent colour.
    ///
    /// Values are in **source sample space**, before `/Decode` — §8.9.6.4:
    /// *"Each integer shall be in the range 0 to 2^BitsPerComponent − 1,
    /// representing colour values BEFORE decoding with the `Decode`
    /// array."*
    pub color_key_mask: Option<Vec<i64>>,
    /// How the image must be turned to appear the right way up.
    pub orientation: Orientation,
    /// The file's declared resolution in dots per inch, if it declared one.
    pub dpi: Option<(f64, f64)>,
    /// Everything about the conversion the operator cannot see.
    pub notes: ImportNotes,
}

impl ImportedImage {
    /// The image's size in samples **as displayed** — stored dimensions,
    /// swapped when the EXIF orientation transposes.
    #[must_use]
    pub const fn display_size_px(&self) -> (u32, u32) {
        if self.orientation.transposes() {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }

    /// The image's natural size in PDF user-space units (points, 1/72 inch),
    /// from its declared resolution.
    ///
    /// # Why this is a method the caller may ignore, not a placement default
    ///
    /// Applying an embedded DPI silently is exactly the move rule 4 forbids:
    /// the resulting size is something pdfcer *inferred* from metadata the
    /// operator never saw, and a scanner that wrote `pHYs` as 300 dpi and a
    /// phone that wrote nothing would place the same picture at wildly
    /// different sizes with no visible reason. So placement is driven by the
    /// caller's rectangle, and this is offered for a caller that explicitly
    /// asks for natural size — with [`ImportNotes::dpi_source`] alongside,
    /// so "the file said 300 dpi" and "pdfcer assumed 72" are distinguishable.
    ///
    /// When no resolution was declared, one pixel becomes one point (72 dpi),
    /// which is PDF's own default user-space unit (§8.3.2.3).
    #[must_use]
    pub fn natural_size_pt(&self) -> (f64, f64) {
        let (px_w, px_h) = self.display_size_px();
        let (dx, dy) = self.dpi.unwrap_or((72.0, 72.0));
        let (dx, dy) = (
            if dx > 0.0 { dx } else { 72.0 },
            if dy > 0.0 { dy } else { 72.0 },
        );
        // The orientation transposes the pixel grid, so it transposes the
        // per-axis resolutions with it.
        let (dx, dy) = if self.orientation.transposes() {
            (dy, dx)
        } else {
            (dx, dy)
        };
        (f64::from(px_w) * 72.0 / dx, f64::from(px_h) * 72.0 / dy)
    }
}

/// Recognise a file's raster format from its leading bytes.
///
/// Returns `Ok(format)` for what pdfcer places, and `Err` naming the format
/// for what it recognises but declines — never a bare "unknown" for a file
/// that is plainly a GIF, a WebP or a BigTIFF.
///
/// # Errors
///
/// [`ImageImportError::UnsupportedFormat`] for a recognised-but-declined
/// container; [`ImageImportError::NotAnImage`] for anything else.
pub fn sniff(data: &[u8]) -> Result<ImageFormat, ImageImportError> {
    // Signatures, longest-first where one is a prefix of another.
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if data.starts_with(PNG) {
        return Ok(ImageFormat::Png);
    }
    // SOI + any marker. A bare `FF D8` is enough: JFIF and EXIF differ only
    // in which APPn follows, and both are the same T.81 codestream.
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok(ImageFormat::Jpeg);
    }
    if data.starts_with(b"BM") {
        return Ok(ImageFormat::Bmp);
    }
    // TIFF: "II" + 42 little-endian, or "MM" + 42 big-endian (TIFF 6.0 §2).
    if data.starts_with(b"II\x2a\x00") || data.starts_with(b"MM\x00\x2a") {
        return Ok(ImageFormat::Tiff);
    }

    let declined = |format| Err(ImageImportError::UnsupportedFormat { format });
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return declined("GIF");
    }
    // BigTIFF's magic is 43, and it is a DIFFERENT parse — 8-byte offsets, a
    // different directory layout, a 16-byte header. Declined under its own
    // name rather than as "TIFF", so the message reads "pdfcer does not place
    // BigTIFF images — it places … TIFF", which is exactly the actionable
    // sentence (re-save as classic TIFF) rather than a contradiction.
    if data.starts_with(b"II\x2b\x00") || data.starts_with(b"MM\x00\x2b") {
        return declined("BigTIFF");
    }
    // RIFF....WEBP
    if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        return declined("WebP");
    }
    // ISO-BMFF brands: `....ftyp<brand>`.
    if data.get(4..8) == Some(b"ftyp") {
        return match data.get(8..12) {
            Some(b"avif" | b"avis") => declined("AVIF"),
            Some(b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"mif1" | b"msf1") => {
                declined("HEIC")
            }
            _ => Err(ImageImportError::NotAnImage),
        };
    }
    // JPEG 2000 as a standalone file: the JP2 signature box, or a bare
    // codestream's SOC+SIZ. pdfcer *reads* JPXDecode images already, but
    // importing one means authoring a `/JPXDecode` XObject whose colour
    // space comes from the codestream (Table 89 makes `/ColorSpace` optional
    // and `/BitsPerComponent` ignored there) — a distinct enough shape to be
    // its own scoped decision rather than a fourth branch here.
    if data.starts_with(b"\x00\x00\x00\x0cjP  \r\n\x87\n")
        || data.starts_with(&[0xFF, 0x4F, 0xFF, 0x51])
    {
        return declined("JPEG 2000");
    }
    if data.starts_with(b"\x00\x00\x01\x00") {
        return declined("ICO");
    }
    Err(ImageImportError::NotAnImage)
}

/// Parse a raster image file into a PDF-ready image XObject payload.
///
/// This is the module's whole public entry point. It never touches a
/// document and never mutates anything; everything it decides is visible in
/// the returned [`ImportedImage`], including every disclosure.
///
/// # Errors
///
/// - [`ImageImportError::NotAnImage`] / [`ImageImportError::UnsupportedFormat`]
///   — the file is not something pdfcer places, named either way.
/// - [`ImageImportError::Unsupported`] — a supported container using a
///   sub-feature pdfcer declines (interlaced PNG, arithmetic JPEG, RLE BMP),
///   named by a stable feature key.
/// - [`ImageImportError::Corrupt`] — malformed or truncated.
/// - [`ImageImportError::Empty`] / [`ImageImportError::TooLarge`] — outside
///   the dimension ceilings.
/// - [`ImageImportError::Compress`] — re-compression failed (decode branch
///   only).
///
/// # Examples
///
/// ```
/// use pdfcer_core::image_import::{self, ImageImportError, SUPPORTED_FORMATS};
///
/// // Refusals name the format rather than failing generically.
/// let err = image_import::import(b"GIF89a\0\0\0\0").unwrap_err();
/// assert!(matches!(err, ImageImportError::UnsupportedFormat { format: "GIF" }));
///
/// // Asserted against the constant, not a copy of it. This line said
/// // "PNG, JPEG and BMP" until TIFF support landed and turned it into a
/// // failing doctest — the message had moved on and the assertion had
/// // not. Referencing `SUPPORTED_FORMATS` makes that impossible: the
/// // next format to arrive updates this example by construction.
/// assert!(err.to_string().contains(SUPPORTED_FORMATS));
/// ```
pub fn import(data: &[u8]) -> Result<ImportedImage, ImageImportError> {
    import_with(data, &ImportOptions::new())
}

/// [`import`], with an explicit compression policy.
///
/// # Why the policy lives here and not on the placement spec
///
/// The coordinating brief proposed hanging it off
/// [`NewImage`](crate::edit::NewImage). It belongs here instead, for a
/// reason that is about rule 4 rather than about tidiness: **this function
/// is where the stream bytes are produced, and it is pure.** A front end
/// can import at one policy, show the operator the resulting size and every
/// disclosure, import again at another, and compare — all before a single
/// byte of document state changes. Hanging the policy off the placement
/// verb would move the decision to the moment of commit, which is exactly
/// the "disclosed after the fact" shape rule 4 rejects.
///
/// It is still *per image*, as the operator asked: one call, one file, one
/// policy. Nothing here is global.
///
/// The policy that actually ran is reported in
/// [`ImportNotes::applied_compression`], which is not always the one asked
/// for — see that field.
///
/// # Errors
///
/// Everything [`import`] can return, plus:
///
/// - [`ImageImportError::InvalidQuality`] — [`ImageCompression::Jpeg`] with a
///   `quality` outside 1–100, refused rather than clamped.
/// - [`ImageImportError::CompressionRefused`] — [`ImageCompression::Jpeg`] on
///   an image whose colour-key `/Mask` lossy encoding would corrupt.
/// - [`ImageImportError::DecodeFailed`] — [`ImageCompression::Lossless`] or
///   [`ImageCompression::Jpeg`] on a source whose samples could not be
///   recovered, or whose colour model has no device colour space.
///
/// # Examples
///
/// ```
/// use pdfcer_core::image_import::{self, ImageCompression, ImageImportError, ImportOptions};
///
/// // An out-of-range quality is refused BEFORE the file is parsed, so the
/// // operator learns the real blocker rather than a complaint about bytes
/// // that were never going to be encoded.
/// let options = ImportOptions::new().with_compression(ImageCompression::Jpeg { quality: 0 });
/// let err = image_import::import_with(&[0xFF, 0xD8, 0xFF, 0xE0], &options).unwrap_err();
/// assert!(matches!(err, ImageImportError::InvalidQuality { quality: 0 }));
/// assert!(err.to_string().contains("between 1 and 100"));
/// ```
pub fn import_with(
    data: &[u8],
    options: &ImportOptions,
) -> Result<ImportedImage, ImageImportError> {
    // Validated BEFORE the file is parsed. A decode spent on a request that
    // was never going to be honoured only delays the same answer, and the
    // operator would have to read past a complaint about their file to reach
    // the real one about their flag.
    if let ImageCompression::Jpeg { quality } = options.compression
        && !(jpeg_encode::MIN_QUALITY..=jpeg_encode::MAX_QUALITY).contains(&quality)
    {
        return Err(ImageImportError::InvalidQuality { quality });
    }

    let mut img = match sniff(data)? {
        ImageFormat::Png => png::import(data)?,
        ImageFormat::Jpeg => jpeg::import(data)?,
        ImageFormat::Bmp => bmp::import(data)?,
        ImageFormat::Tiff => tiff::import(data)?,
    };

    // The two cases where a policy changes the bytes the importers produced.
    // Everything else is already what it should be — a PNG or BMP is
    // lossless whichever of the two lossless policies was named, and
    // `Passthrough` is what each importer emits by construction.
    match options.compression {
        // A lossy source the operator asked to store losslessly.
        ImageCompression::Lossless if img.filter == ImportFilter::DctDecode => {
            img = to_lossless(&img)?;
        }
        // A deliberate lossy re-encode, of any source.
        ImageCompression::Jpeg { quality } => {
            img = jpeg_encode::to_jpeg(&img, quality)?;
        }
        _ => {}
    }

    img.notes.requested_compression = options.compression;
    // Derived from what HAPPENED rather than from what was asked, which is
    // the whole point: `Passthrough` here means, and only means, that the
    // source's own compressed bytes are in the document unchanged.
    img.notes.applied_compression = match img.notes.recompressed {
        None => ImageCompression::Passthrough,
        // The quality carried here is the one that ran, not the one
        // requested. They are equal today (an out-of-range value is refused
        // rather than clamped), and reading it from `notes` rather than from
        // `options` keeps that an assertion the code makes rather than an
        // assumption it relies on.
        Some(RecompressReason::JpegRequested) => ImageCompression::Jpeg {
            quality: img.notes.jpeg_quality.unwrap_or(jpeg_encode::MAX_QUALITY),
        },
        Some(_) => ImageCompression::Lossless,
    };
    // Recorded for EVERY policy, so "did this get smaller?" never requires
    // diffing the output. Set last so no branch above can forget it.
    img.notes.source_bytes = data.len();
    img.notes.stored_bytes = img.data.len() + img.soft_mask.as_ref().map_or(0, |m| m.data.len());
    Ok(img)
}

/// Decode a `/DCTDecode` image and re-store its samples as `/FlateDecode`.
///
/// # Why this reuses the ordinary decode path
///
/// The samples must be *exactly* the ones `pdfcer-render` would paint, which
/// means Table 13's colour-transform precedence chain, the YCCK inverse, and
/// R29's never-invert rule all have to apply identically. Reimplementing any
/// of that here would create a second opinion about what a JPEG's pixels
/// are, and the two would drift. So this calls
/// [`decode_image_view`](crate::image_codec::decode_image_view) with a
/// synthesised one-entry image dictionary and an **empty object graph** —
/// the graph is consulted only to resolve an indirect
/// `/DecodeParms /ColorTransform`, which a synthesised dictionary does not
/// have.
///
/// The colour model comes from the codec, never from a guess:
/// `Untransformed3` is mapped to `/DeviceRGB` because §7.4.8 defines it as
/// *"the codestream's components already ARE the `/ColorSpace`
/// components"* — which, for a three-component JPEG pdfcer is choosing the
/// colour space for, is the same three components `Passthrough` would have
/// declared.
fn to_lossless(img: &ImportedImage) -> Result<ImportedImage, ImageImportError> {
    use crate::image_codec::{CodecColorModel, decode_image_view};
    use crate::object::{Dict, Name, ObjId, Object};
    use crate::view::DocumentView;

    /// An object graph with nothing in it.
    ///
    /// A standalone image file has no PDF objects, so every lookup honestly
    /// answers "no such object" — which is exactly §7.3.10's outcome for an
    /// unresolvable reference, so the decoder needs no special case.
    struct NoGraph;
    impl crate::graph::ObjectGraph for NoGraph {
        fn value(&self, _id: ObjId) -> Option<&Object> {
            None
        }
        fn trailer_entry(&self, _key: &[u8]) -> Option<&Object> {
            None
        }
    }

    let mut dict = Dict::new();
    dict.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(b"DCTDecode")),
    );
    let graph = NoGraph;
    let view = DocumentView::new(&graph, &[], crate::PdfVersion { major: 1, minor: 7 });
    let coded = decode_image_view(&view, &dict, &img.data, false).map_err(|e| {
        ImageImportError::DecodeFailed {
            detail: e.to_string(),
        }
    })?;

    let color_space = match coded.color_model {
        CodecColorModel::Gray | CodecColorModel::Bilevel => ImportColorSpace::DeviceGray,
        CodecColorModel::Rgb | CodecColorModel::Untransformed3 => ImportColorSpace::DeviceRgb,
        CodecColorModel::Cmyk => ImportColorSpace::DeviceCmyk,
        // `CodecColorModel` is `#[non_exhaustive]`. A model this function
        // does not know how to name is REFUSED rather than guessed at: a
        // wrong `/ColorSpace` renders as the wrong colours with no error.
        _ => {
            return Err(ImageImportError::DecodeFailed {
                detail: "the codestream's colour model has no device colour space".to_owned(),
            });
        }
    };

    let mut notes = img.notes;
    notes.recompressed = Some(RecompressReason::LosslessRequested);
    notes.lossless_from_lossy = true;

    Ok(ImportedImage {
        format: img.format,
        // The CODESTREAM's dimensions, not the ones the earlier marker walk
        // read. They are the same source and should agree — but the decoder
        // is the authority on what it just produced, and sizing a buffer
        // from a second opinion is how a stride bug is born.
        width: coded.width,
        height: coded.height,
        bits_per_component: coded.bits_per_component,
        color_space,
        filter: ImportFilter::Flate,
        data: flate_encode(&coded.samples)?,
        soft_mask: None,
        color_key_mask: None,
        // The EXIF orientation still applies — it is a property of the
        // picture, not of how the picture is stored.
        orientation: img.orientation,
        dpi: img.dpi,
        notes,
    })
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Enforce the same dimension ceilings the decode path uses.
///
/// Deliberately the *decode* ceilings, not a second set: placing an image
/// pdfcer would refuse to render produces a document pdfcer cannot display,
/// which is a worse outcome than the refusal.
pub(crate) fn check_dimensions(
    width: u32,
    height: u32,
    components: u32,
    bits: u32,
) -> Result<(), ImageImportError> {
    if width == 0 || height == 0 {
        return Err(ImageImportError::Empty { width, height });
    }
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(ImageImportError::TooLarge);
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_IMAGE_PIXELS {
        return Err(ImageImportError::TooLarge);
    }
    let bytes = pixels
        .saturating_mul(u64::from(components))
        .saturating_mul(u64::from(bits).div_ceil(8));
    if bytes > MAX_IMAGE_SAMPLE_BYTES as u64 {
        return Err(ImageImportError::TooLarge);
    }
    Ok(())
}

/// The byte-padded row stride for a sample grid (§8.9.3: *"each row … padded
/// to a whole number of bytes"*; §7.4.4.4 says the same for predicted rows).
pub(crate) fn row_bytes(width: u32, components: u32, bits: u32) -> usize {
    let bits = u64::from(width) * u64::from(components) * u64::from(bits);
    usize::try_from(bits.div_ceil(8)).unwrap_or(usize::MAX)
}

/// zlib-compress `data` (§7.4.4.1 delegates FlateDecode to RFC 1950).
///
/// Uses the same pure-Rust `flate2`/`miniz_oxide` backend the decoder does —
/// never a C backend, per the single-static-binary packaging invariant
/// (`ARCHITECTURE.md` §6) and the WASM fork.
///
/// `Compression::best` rather than `default`: this runs once, at import
/// time, on data that will live in the document forever. Trading import
/// milliseconds for permanently smaller documents is the right side of that
/// trade, and it is the *only* branch where pdfcer chooses a compression
/// level at all (the passthrough branches inherit the source's).
pub(crate) fn flate_encode(data: &[u8]) -> Result<Vec<u8>, ImageImportError> {
    use flate2::{Compress, Compression, FlushCompress, Status};

    let mut c = Compress::new(Compression::best(), true);
    // §7.4.4.1 NOTE 2: "Flate encoding expands its input by no more than 11
    // bytes or a factor of 1.003 (whichever is larger)". Start at half the
    // input and let the loop grow it.
    let mut out = vec![0u8; data.len() / 2 + 64];
    let mut consumed = 0usize;
    loop {
        let before_in = c.total_in();
        let written = usize::try_from(c.total_out()).unwrap_or(0);
        if written == out.len() {
            out.resize(out.len() * 2, 0);
        }
        let status = c
            .compress(
                data.get(consumed..).unwrap_or(&[]),
                out.get_mut(written..).unwrap_or(&mut []),
                FlushCompress::Finish,
            )
            .map_err(|e| ImageImportError::Compress(e.to_string()))?;
        consumed += usize::try_from(c.total_in() - before_in).unwrap_or(0);
        match status {
            Status::StreamEnd => {
                out.truncate(usize::try_from(c.total_out()).unwrap_or(0));
                return Ok(out);
            }
            // No progress would spin, so the buffer is grown unconditionally
            // whenever it is full (above) — progress is structural.
            Status::Ok | Status::BufError => {
                out.resize(out.len() * 2, 0);
            }
        }
    }
}

/// Fold a feature's version floor into a running maximum.
pub(crate) fn raise_version(slot: &mut Option<PdfFeature>, feature: PdfFeature) {
    *slot = Some(slot.map_or(feature, |cur| cur.max(feature)));
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
    fn sniff_recognises_the_four_supported_formats() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest").unwrap(), ImageFormat::Png);
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap(), ImageFormat::Jpeg);
        assert_eq!(sniff(b"BM\0\0\0\0").unwrap(), ImageFormat::Bmp);
        // Both byte orders of classic TIFF (TIFF 6.0 §2, version magic 42).
        assert_eq!(sniff(b"II\x2a\x00\x08\0\0\0").unwrap(), ImageFormat::Tiff);
        assert_eq!(sniff(b"MM\x00\x2a\0\0\0\x08").unwrap(), ImageFormat::Tiff);
    }

    /// The load-bearing property of every refusal: it names the format the
    /// operator handed over AND the formats that would have worked.
    #[test]
    fn declined_formats_are_refused_by_name() {
        for (bytes, name) in [
            (b"GIF89a\0\0\0\0".to_vec(), "GIF"),
            // BigTIFF (magic 43) is a DIFFERENT parser, refused under its own
            // name — classic TIFF (magic 42) is placed, so refusing BigTIFF
            // as "TIFF" would produce a message that contradicts itself.
            (b"II\x2b\x00\0\0\0\0".to_vec(), "BigTIFF"),
            (b"MM\x00\x2b\0\0\0\0".to_vec(), "BigTIFF"),
            (b"RIFF\0\0\0\0WEBPVP8 ".to_vec(), "WebP"),
            (b"\0\0\0\x20ftypavif\0\0\0\0".to_vec(), "AVIF"),
            (b"\0\0\0\x20ftypheic\0\0\0\0".to_vec(), "HEIC"),
        ] {
            let err = sniff(&bytes).unwrap_err();
            assert_eq!(
                err,
                ImageImportError::UnsupportedFormat { format: name },
                "{name} must be refused by name"
            );
            let msg = err.to_string();
            assert!(msg.contains(name), "the message names the format: {msg}");
            assert!(
                msg.contains(SUPPORTED_FORMATS),
                "the message names what DOES work: {msg}"
            );
        }
    }

    #[test]
    fn a_text_file_is_not_an_image() {
        assert_eq!(
            sniff(b"This is not an image.").unwrap_err(),
            ImageImportError::NotAnImage
        );
        assert_eq!(sniff(b"").unwrap_err(), ImageImportError::NotAnImage);
    }

    /// Every orientation matrix must map the unit square onto itself — a
    /// cheap total check that catches a transposed sign in any of the eight
    /// entries, which is otherwise only visible as a mirrored photograph.
    #[test]
    fn every_orientation_maps_the_unit_square_onto_itself() {
        let corners = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)];
        for o in [
            Orientation::Identity,
            Orientation::MirrorHorizontal,
            Orientation::Rotate180,
            Orientation::MirrorVertical,
            Orientation::MirrorRotate270,
            Orientation::Rotate90,
            Orientation::MirrorRotate90,
            Orientation::Rotate270,
        ] {
            let [a, b, c, d, e, f] = o.unit_square_matrix();
            let mut seen: Vec<(i64, i64)> = corners
                .iter()
                .map(|&(u, v)| {
                    let x = a * u + c * v + e;
                    let y = b * u + d * v + f;
                    ((x * 1000.0) as i64, (y * 1000.0) as i64)
                })
                .collect();
            seen.sort_unstable();
            assert_eq!(
                seen,
                vec![(0, 0), (0, 1000), (1000, 0), (1000, 1000)],
                "{o:?} must permute the unit square's corners"
            );
        }
    }

    /// Orientation 6 is "rotate 90° clockwise". The stored image's TOP-LEFT
    /// corner must end up at the displayed TOP-RIGHT. Pinned explicitly
    /// because a sign error here produces a picture that is rotated the
    /// wrong way — visually obvious to a human, invisible to a test that
    /// only checks that *some* rotation happened.
    #[test]
    fn orientation_6_turns_the_image_clockwise() {
        let [a, b, c, d, e, f] = Orientation::Rotate90.unit_square_matrix();
        // The stored top-left corner is unit-square (0, 1): u = 0 is the
        // left edge, v = 1 is the top (§8.9.4 puts image row 0 at v = 1).
        let (u, v) = (0.0, 1.0);
        let (x, y) = (a * u + c * v + e, b * u + d * v + f);
        assert!(
            (x - 1.0).abs() < 1e-9 && (y - 1.0).abs() < 1e-9,
            "({x}, {y})"
        );
    }

    #[test]
    fn transposing_orientations_swap_the_displayed_size() {
        assert!(!Orientation::Rotate180.transposes());
        assert!(Orientation::Rotate90.transposes());
        assert!(Orientation::Rotate270.transposes());
        assert!(Orientation::MirrorRotate90.transposes());
        assert!(Orientation::MirrorRotate270.transposes());
    }

    #[test]
    fn indexed_counts_one_component_not_three() {
        let cs = ImportColorSpace::Indexed {
            hival: 5,
            lookup: vec![0; 18],
        };
        assert_eq!(cs.components(), 1);
        assert_eq!(ImportColorSpace::DeviceRgb.components(), 3);
        assert_eq!(ImportColorSpace::DeviceCmyk.components(), 4);
    }

    #[test]
    fn a_flate_round_trip_reproduces_the_input() {
        let data: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
        let packed = flate_encode(&data).unwrap();
        let back = crate::filters::flate::decode(&packed, None).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn the_version_floor_is_the_latest_feature() {
        let mut slot = None;
        raise_version(&mut slot, PdfFeature::ProgressiveJpeg);
        raise_version(&mut slot, PdfFeature::SoftMask);
        raise_version(&mut slot, PdfFeature::ColourKeyMask);
        assert_eq!(slot, Some(PdfFeature::SoftMask));
        raise_version(&mut slot, PdfFeature::BitsPerComponent16);
        assert_eq!(slot, Some(PdfFeature::BitsPerComponent16));
    }

    #[test]
    fn natural_size_uses_the_declared_resolution() {
        let img = ImportedImage {
            format: ImageFormat::Png,
            width: 300,
            height: 150,
            bits_per_component: 8,
            color_space: ImportColorSpace::DeviceRgb,
            filter: ImportFilter::Flate,
            data: Vec::new(),
            soft_mask: None,
            color_key_mask: None,
            orientation: Orientation::Identity,
            dpi: Some((300.0, 300.0)),
            notes: ImportNotes::default(),
        };
        let (w, h) = img.natural_size_pt();
        assert!((w - 72.0).abs() < 1e-9, "300 px at 300 dpi is one inch");
        assert!((h - 36.0).abs() < 1e-9);

        // No declared resolution: one pixel becomes one point.
        let mut plain = img.clone();
        plain.dpi = None;
        assert_eq!(plain.natural_size_pt(), (300.0, 150.0));

        // A transposing orientation swaps both the pixels and the per-axis
        // resolutions.
        let mut turned = img;
        turned.orientation = Orientation::Rotate90;
        turned.dpi = Some((300.0, 150.0));
        let (w, h) = turned.natural_size_pt();
        assert!((w - 72.0).abs() < 1e-9, "150 px at 150 dpi is one inch");
        assert!((h - 72.0).abs() < 1e-9, "300 px at 300 dpi is one inch");
    }
}

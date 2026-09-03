//! # Image codecs (ISO 32000-1 §7.4.6–§7.4.9) — the terminal filter stage
//!
//! The second tier of pdfcer's two-tier filter architecture
//! (`docs/decisions/005-image-codecs.md` §4.6, rule **R23**).
//! [`crate::filters::decode_stream`] handles byte-stream filters —
//! bytes in, bytes out, chainable. This module handles the four
//! **image codecs**, which are a different kind of object:
//!
//! | Filter | Clause | ITU-T | Pass | Status |
//! |---|---|---|---|---|
//! | `DCTDecode` | §7.4.8 | T.81 (JPEG) | 2.1 | **implemented** ([`dct`]) |
//! | `CCITTFaxDecode` | §7.4.6 | T.4 / T.6 | 2.2 | **implemented** ([`ccitt`]) |
//! | `JBIG2Decode` | §7.4.7 | T.88 | 2.2 | **implemented** ([`jbig2`]) |
//! | `JPXDecode` | §7.4.9 | T.800 (JPEG 2000) | 2.3 | **implemented** ([`jpx`]) |
//!
//! ## Why these cannot be `Vec<u8>`-returning filters
//!
//! Three independent, mechanical reasons — any one of them would force
//! a different signature (decision 005 §1.2, §5.2):
//!
//! 1. **§8.9.5 Table 89 inverts the dictionary's authority for JPX.**
//!    `/ColorSpace` becomes optional and, when present, "any colour
//!    space specifications in the JPEG2000 data shall be ignored";
//!    `/BitsPerComponent` is "optional and shall be ignored if present"
//!    because "the bit depth is determined by the conforming reader in
//!    the process of decoding." A function returning bytes has nowhere
//!    to put the bit depth or colour space it just discovered, so the
//!    caller would have to re-parse hostile input to recover what the
//!    decoder already knew.
//! 2. **§7.4.8's DCT samples carry a colour model that depends on the
//!    JPEG's own Adobe APP14 marker**, which Table 13 makes *outrank*
//!    the `/DecodeParms` entry.
//! 3. **JBIG2 needs `/JBIG2Globals`** — a stream *reference*, requiring
//!    a [`Document`] — and **CCITT needs `/Height`** from the image
//!    dictionary when `/Rows` is 0 or absent. Neither is reachable from
//!    a `(dict, raw)` pair.
//!
//! So [`decode_image`] returns a [`CodedImage`]: samples **plus** the
//! geometry and colour model the codestream itself declares.
//!
//! ## Who wins a disagreement is **not** the same for every codec
//!
//! [`CodedImage`] reports what the codestream says. It does not decide
//! whose account is authoritative, because the spec answers that
//! differently per filter, and getting the direction backwards is a
//! wrong-colour bug rather than a crash:
//!
//! | | `DCTDecode` (and the bilevel pair) | `JPXDecode` |
//! |---|---|---|
//! | `/ColorSpace` | **Required.** The dictionary is the only source; a component-count disagreement is a producer bug. | **Optional.** Present → *dictionary wins* and "the colour space specifications in the JPEG2000 data shall be ignored". Absent → *codestream wins*. |
//! | `/BitsPerComponent` | **Required**, and must agree (DCT is always 8). | **"Optional and shall be ignored if present"** — the codestream wins outright, and honouring a stated value is explicitly wrong. |
//! | `/Decode` | Applied (§8.9.5.2) — for DCT it is the *only* sanctioned polarity control (rules R29/R30). | **Ignored** unless `/ImageMask` is true. |
//! | `/Width`, `/Height` | Dictionary is authoritative. | "shall **match**", with **no** conflict rule given. |
//!
//! So "the codestream is authoritative for JPX" is true only where the
//! dictionary is silent or disqualified; a present `/ColorSpace` still
//! wins. In every case the divergence itself is counted
//! ([`CodecNotes::geometry_mismatch`]) and never silently absorbed.
//! `pdfcer-render` implements the per-filter split: the dictionary's
//! `/Width`//`/Height` size the pixmap (§8.9.4 maps the image onto the
//! unit square regardless of sample count), while the codestream's
//! width, component count and bit depth drive the row stride, because
//! that is the physical layout of the bytes in hand.
//!
//! ## The layer boundary (rule R26): this module never decides colour
//!
//! `pdfcer-core` hands `pdfcer-render` the codec's own samples and its
//! declared colour model. Only `pdfcer-render` applies `/Decode`
//! (§8.9.5.2), resolves `/ColorSpace`, and reconciles the two. Nothing
//! here applies a `/Decode` array, an "Adobe CMYK inversion", or any
//! polarity flip of its own — and per decision 006 the "Adobe CMYK
//! inversion" half of that clause is now **permanent and sourced**
//! (rule R29: there is no such inversion to apply; `/Decode` is the
//! sole polarity control). One clarification from the same decision:
//! a codec adapter **may observe** the image dictionary to classify
//! diagnostics (e.g. `/Decode` presence for the R30 counter) —
//! *observing is not applying*. The one named future exception is
//! CCITT's `/BlackIs1`, which is a Table 11 *filter parameter* and
//! therefore genuinely belongs to the adapter.
//!
//! ## Unsupported sub-features are named, never grey boxes (rule R27)
//!
//! Arithmetic-coded JPEG, 12-bit JPEG, lossless JPEG, an unknown Adobe
//! transform byte: each is a distinct
//! [`ImageCodecError::FeatureUnsupported`] with a stable `&'static str`
//! key, counted by name in the renderer's diagnostics. An operator must
//! be able to tell *which* feature is missing without reading the code.
//!
//! ## Resource ceilings are pdfcer's, never the vendor's (rule R25)
//!
//! `zune-core`'s `DecoderOptions::default()` caps dimensions at 16,384
//! pixels. That is sensible for a general image library and a **bug**
//! here: a 20,000-pixel-wide scanned engineering drawing is legal, is
//! under [`MAX_IMAGE_PIXELS`], and would be silently refused. Worse, an
//! inherited bad guard never appears in a diff as a number anyone chose
//! — and this project has already been bitten twice by
//! guard-by-intuition (`MAX_TOKEN_LEN`, `MAX_XOBJECT_DEPTH`). Every
//! ceiling below is therefore set explicitly by pdfcer and documented
//! with its reasoning.

pub mod ccitt;
pub mod dct;
pub mod jbig2;
#[cfg(feature = "jpx")]
pub mod jpx;

/// Shared 1-bit sample packing for the two fax codecs. Private: it is an
/// implementation detail of [`ccitt`] and [`jbig2`], not part of the
/// crate's API surface.
mod bilevel;

#[cfg(test)]
pub(crate) mod fixtures;

#[cfg(test)]
mod fixtures_bilevel;

// Gated with the feature as well as on `test`: these fixtures decode JPX, so
// without the codec they would not compile, and a `--no-default-features
// --tests` build is exactly what CI runs to prove the gate is real.
#[cfg(all(test, feature = "jpx"))]
mod fixtures_jpx;

use crate::document::Document;
use crate::filters::{self, FilterError, FilterNotes};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::settings::CmykJpegPolarity;
use crate::view::DocumentView;

/// Maximum `width × height` accepted for a single decoded image
/// (pdfcer policy, ARCHITECTURE.md §10.1 — no Annex C limit exists).
///
/// 32 Mpx (33,554,432) bounds an RGBA texel buffer at 128 MiB. For
/// scale: a 300 DPI A4 page scanned in full colour is ~8.7 Mpx, and the
/// largest sensible print image at 600 DPI on a 200-inch (Annex C
/// maximum) page edge is still comfortably under this. A crafted
/// 65535 × 65535 image (4.3 Gpx, 17 GiB of RGBA) is refused here rather
/// than after a 17 GiB allocation attempt.
///
/// This is the **single** source of truth for the limit;
/// `pdfcer_render::image::MAX_IMAGE_PIXELS` re-exports it so the codec
/// layer and the rasterizer can never drift apart.
pub const MAX_IMAGE_PIXELS: u64 = 32 * 1024 * 1024;

/// Maximum width or height, in samples, accepted from a codestream.
///
/// Deliberately generous and deliberately *not* a second, tighter
/// aspect-ratio guard: [`MAX_IMAGE_PIXELS`] is the real bound, and a
/// long thin scan (a 30,000-pixel-wide panorama or an unrolled receipt)
/// is a legitimate shape that a 16,384 cap would reject. 65,535 is not
/// an arbitrary round number — it is JPEG's own limit, because T.81's
/// SOF frame header stores X and Y as 16-bit integers, so no JPEG
/// codestream can declare more.
pub const MAX_IMAGE_DIMENSION: u32 = 65_535;

/// Maximum decoded sample-buffer size, in bytes, for a single image.
///
/// [`MAX_IMAGE_PIXELS`] × 4, i.e. the pixel ceiling at the worst
/// component count pdfcer maps (CMYK). Checked against the decoder's own
/// declared output size **before** the buffer is allocated, so a
/// codestream claiming a huge component count cannot turn an
/// in-bounds pixel count into an out-of-bounds allocation.
pub const MAX_IMAGE_SAMPLE_BYTES: usize = 128 * 1024 * 1024;

/// Which terminal image codec a `/Filter` chain ends in.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum Codec {
    /// `DCTDecode` / `DCT` (§7.4.8) — JPEG, ITU-T T.81.
    Dct,
    /// `CCITTFaxDecode` / `CCF` (§7.4.6) — Group 3/4 fax, T.4/T.6.
    Ccitt,
    /// `JBIG2Decode` (§7.4.7) — ITU-T T.88.
    Jbig2,
    /// `JPXDecode` (§7.4.9) — JPEG 2000, ITU-T T.800.
    Jpx,
}

impl Codec {
    /// The canonical (unabbreviated) filter name, for diagnostics.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dct => "DCTDecode",
            Self::Ccitt => "CCITTFaxDecode",
            Self::Jbig2 => "JBIG2Decode",
            Self::Jpx => "JPXDecode",
        }
    }

    /// May this codec appear in an **inline image** (`BI`/`ID`/`EI`)?
    ///
    /// §8.9.7 restricts inline-image filters, and §7.4.7 states the
    /// JBIG2 case outright: JBIG2-encoded data may not be used in an
    /// inline image. JPXDecode is likewise excluded from Table 94's
    /// abbreviation set and from the inline-image filter list. `DCT`
    /// and `CCF` *are* legal inline abbreviations (Table 94), so both
    /// route through the ordinary path.
    #[must_use]
    pub const fn allowed_inline(self) -> bool {
        match self {
            Self::Dct | Self::Ccitt => true,
            Self::Jbig2 | Self::Jpx => false,
        }
    }

    /// Recognize a `/Filter` name, full or Table 94 abbreviation.
    const fn from_name(name: &[u8]) -> Option<Self> {
        match name {
            b"DCTDecode" | b"DCT" => Some(Self::Dct),
            b"CCITTFaxDecode" | b"CCF" => Some(Self::Ccitt),
            b"JBIG2Decode" => Some(Self::Jbig2),
            b"JPXDecode" => Some(Self::Jpx),
            _ => None,
        }
    }
}

/// The colour model of [`CodedImage::samples`], as the **codec**
/// declares it — *not* a PDF colour space.
///
/// Mapping this onto `/ColorSpace` is `pdfcer-render`'s job (rule R26).
/// The distinction matters because a JPEG's component meaning comes
/// from its own APP14 marker under Table 13's precedence rules, which
/// can disagree with the image dictionary, and because for JPX the
/// codestream outranks the dictionary outright (Table 89).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum CodecColorModel {
    /// One component per sample.
    Gray,
    /// Three components, already transformed to RGB by the codec —
    /// Table 13's `ColorTransform 1` for a 3-component image (the
    /// default when no Adobe marker and no `/DecodeParms` entry say
    /// otherwise).
    Rgb,
    /// Three components delivered **untransformed** — Table 13's
    /// `ColorTransform 0`. Per §7.4.8 this does not mean "no colour
    /// space": it means the codestream's components already *are* the
    /// `/ColorSpace` components, so the caller interprets them through
    /// `/ColorSpace` directly.
    Untransformed3,
    /// Four components in CMYK order, **raw**: no `/Decode` applied and
    /// no "Adobe inversion" applied — there is none to apply (decision
    /// 006, rule R29: the four-engine consensus is never-invert, and
    /// `/Decode` is the sole polarity control, applied downstream by
    /// `pdfcer-render`). For YCCK storage the mandated §13.1 inverse
    /// already recovered true ink; for raw-CMYK storage with no
    /// `/Decode` the residual ambiguity is counted by name
    /// ([`CodecNotes::cmyk_polarity_unverifiable`], rule R30).
    Cmyk,
    /// Bit depth 1, one component — where `CCITTFaxDecode` and
    /// `JBIG2Decode` land (§8.9.5 Table 89: both "shall always deliver
    /// 1-bit samples"). The samples are in the **normal PDF convention**
    /// for bilevel data, `0 = black`, which Table 11's `BlackIs1`
    /// description states from the other side. Each adapter has already
    /// translated its codec's native polarity into that convention:
    /// CCITT via `/BlackIs1` → `invert_black`, JBIG2 by the
    /// unconditional inverse of T.88's `1 = black`.
    Bilevel,
    /// **No terminal codec ran.** The stream went through byte-stream
    /// filters only, so the samples are exactly the layout §8.9.3
    /// describes and only the image dictionary describes them. The
    /// geometry fields of [`CodedImage`] are then dictionary echoes,
    /// never independent declarations.
    Unspecified,
    /// The codestream declared a component count pdfcer has no PDF
    /// mapping for. Never rendered; counted by name (rule R27).
    Unknown {
        /// Components per sample as declared by the codestream.
        components: u8,
    },
}

/// Per-image honesty counters produced by the codec layer
/// (decision 005 §6.4; the same disclosure discipline as rule R20).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CodecNotes {
    /// The codestream's geometry disagreed with the image dictionary
    /// (`/Width`, `/Height`, `/BitsPerComponent`, or the component
    /// count implied by `/ColorSpace`). For JPX the codestream *wins*
    /// by Table 89; for DCT a mismatch is a producer bug. Counted
    /// either way, never silent.
    pub geometry_mismatch: bool,
    /// This is a 4-component DCT image whose effective `ColorTransform`
    /// is 1 or 2 (YCCK/YUVK storage) — the **benign census** half of
    /// decision 006 §4.4's split. The mandated YCCK→CMYK inverse ran
    /// (TN #5116 §13.1; it recovers true ink directly), the samples
    /// carry no residual polarity ambiguity, and pdfcer's output
    /// pixel-matches pdfium on every such file in the corpus (9 as of
    /// 2026-07-31 — the earlier "zero exist" claim here was wrong
    /// twice over; see 005's addenda). A pure volume counter: no
    /// warning is warranted (rule R30 covers the shape that does
    /// warrant one, below).
    pub cmyk_image: bool,
    /// This is a 4-component DCT image with effective `ColorTransform`
    /// **0** (raw CMYK storage) and **no `/Decode` array** — the one
    /// shape where the undocumented Photoshop inverted-storage
    /// convention can still produce a photographic negative, because
    /// neither the codestream nor the dictionary disambiguates the
    /// polarity (decision 006 §5.3/§5.4, rule **R30**). Reported,
    /// never repaired: all four reference engines render this shape
    /// un-inverted and share the gap; pdfcer's differentiator is that
    /// it names it. Zero exist in the conformance corpus (the 9
    /// four-component files are all transform 2).
    pub cmyk_polarity_unverifiable: bool,
    /// This JPX image's `/SMaskInData` is **2**: Table 89's "the
    /// image's data stream includes colour channels that have been
    /// **preblended with a background**; the image data also includes an
    /// opacity channel", for which a reader "may create a soft-mask
    /// image **with a `Matte` entry**".
    ///
    /// Recognized and deferred, never approximated (see [`jpx`]). The
    /// colour samples are returned exactly as stored — i.e. already
    /// composited over that backdrop, which is a recognizable picture
    /// rather than a grey box — and the opacity channel is *not*
    /// exposed, because using it without un-premultiplying would
    /// double-darken every partially transparent pixel. Un-premultiplying
    /// needs the `Matte` backdrop colour and clause 11's transparency
    /// model, which decision 005 §7 assigns to `ROADMAP.md` Pass 1.1
    /// item 6.3.
    pub jpx_smask_in_data_preblended: bool,
    /// A JP2 **palette** was left unresolved because the image dictionary
    /// carries its own `/Indexed` colour space (§8.9 Table 89).
    ///
    /// # Why this is worth a field rather than being silent
    ///
    /// Two lookup tables describe the same image — the codestream's
    /// `pclr`/`cmap` boxes and the PDF's `/Indexed` array — and **exactly
    /// one of them may be applied**. Applying both maps a colour value
    /// through a palette a second time and paints whatever entry that
    /// index happens to hit, or black when it hits nothing.
    ///
    /// So this records which authority won, and it is not a shortfall:
    /// Table 89 makes the dictionary's space the answer whenever it is
    /// present. It is disclosed because the decision is **invisible in the
    /// output** — a correct render and a double-resolved one are both just
    /// coloured pixels, and the difference only shows up as the wrong
    /// colour, which nothing downstream can detect.
    pub jpx_palette_left_to_pdf: bool,
    /// LZW framing anomalies seen in the byte-stream prefix of the
    /// chain (see [`FilterNotes::lzw_framing_anomalies`]).
    pub lzw_framing_anomalies: usize,
}

/// A decoded image as the **codec** describes it — before any PDF-level
/// interpretation.
///
/// Nothing here has had `/Decode` applied, and nothing here has been
/// colour-converted beyond the codec's own mandated transform
/// (rule R26). That is `pdfcer-render`'s job.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CodedImage {
    /// Interleaved samples, row-major, row 0 at the top, packed per
    /// [`bits_per_component`](CodedImage::bits_per_component) with rows
    /// padded to a byte boundary (§8.9.3) — the same layout the
    /// byte-stream filters produce, so `pdfcer-render`'s existing
    /// unpacking path is unchanged.
    pub samples: Vec<u8>,
    /// Which codec produced `samples`, or `None` when the chain was
    /// byte-stream filters only. When `None`, every geometry field
    /// below is an echo of the image dictionary and carries no
    /// independent authority.
    pub codec: Option<Codec>,
    /// Width in samples **as the codestream declares it**. May disagree
    /// with `/Width`; the caller decides which wins and
    /// [`CodecNotes::geometry_mismatch`] records the divergence.
    pub width: u32,
    /// Height in samples as the codestream declares it.
    pub height: u32,
    /// Components per sample as the codestream declares it. `0` means
    /// "not declared by any codec" (see [`CodedImage::codec`]).
    pub components: u8,
    /// Bit depth as the codestream declares it. For DCT this is always
    /// 8 (§7.4.8: "each component value shall occupy a byte", and
    /// Table 89's "shall always deliver 8-bit samples"). For JPX it
    /// will be authoritative and `/BitsPerComponent` ignored (Table 89).
    pub bits_per_component: u8,
    /// The colour model the samples are actually in.
    pub color_model: CodecColorModel,
    /// An ICC profile carried inside the codestream, if any (JPEG APP2
    /// / JPX `Icc`). Reconciled against `/ColorSpace` by the caller;
    /// never applied here.
    pub icc_profile: Option<Vec<u8>>,
    /// Alpha carried **inside** the codestream, one 8-bit sample per
    /// pixel in the same row-major order as
    /// [`samples`](CodedImage::samples).
    ///
    /// JPX is the only codec that produces this, and `/SMaskInData`
    /// (Table 89) is what decides whether it is populated: **0 — the
    /// default — means "encoded soft-mask image information shall be
    /// ignored"**, so the common case is `None` even for a codestream
    /// that does carry an opacity channel. `1` fills it in. `2` leaves
    /// it `None` and sets
    /// [`CodecNotes::jpx_smask_in_data_preblended`], because that value
    /// means the colour channels were preblended with a backdrop and
    /// reconstructing them needs clause 11's `Matte` machinery (see
    /// [`jpx`]).
    ///
    /// The opacity channel is **always** removed from `samples`
    /// regardless — it is not a colour component, and leaving it
    /// interleaved would shift every colour one position to the right.
    pub embedded_alpha: Option<Vec<u8>>,
    /// Per-image honesty counters.
    pub notes: CodecNotes,
}

/// Why an image stream could not be turned into samples.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ImageCodecError {
    /// A byte-stream filter in the chain's prefix failed.
    #[error(transparent)]
    Filter(#[from] FilterError),
    /// The codec itself is recognized but not implemented in this
    /// build. Should be **zero** once Pass 2.3 lands; the variant is
    /// kept afterwards so a regression stays visible.
    #[error("{} images are not supported in this build", .codec.name())]
    Unsupported {
        /// Which codec.
        codec: Codec,
    },
    /// A specific codec **sub-feature** is unimplemented (rule R27).
    /// `feature` is a stable diagnostic key such as `"DCT/arithmetic"`
    /// or `"DCT/12-bit"`, suitable for counting by name.
    #[error("unsupported codec feature: {feature}")]
    FeatureUnsupported {
        /// Stable, greppable feature key.
        feature: &'static str,
    },
    /// The codestream is malformed or truncated.
    #[error("corrupt {} data: {detail}", .codec.name())]
    Corrupt {
        /// Which codec reported it.
        codec: Codec,
        /// Human-readable detail from the underlying decoder.
        detail: String,
    },
    /// The codestream's declared geometry crosses a pdfcer ceiling
    /// ([`MAX_IMAGE_PIXELS`], [`MAX_IMAGE_DIMENSION`],
    /// [`MAX_IMAGE_SAMPLE_BYTES`]).
    #[error("image exceeds pdfcer's decode ceilings ({MAX_IMAGE_PIXELS} pixels max)")]
    TooLarge,
    /// The codec is forbidden inside an inline image (§7.4.7, §8.9.7).
    #[error("{} data may not appear in an inline image", .codec.name())]
    NotAllowedInline {
        /// Which codec.
        codec: Codec,
    },
    /// `/BrotliDecode` appeared in an **inline image's** filter chain, which
    /// EXTN-BROTLI-1 §5.2 forbids outright.
    ///
    /// Distinct from [`ImageCodecError::NotAllowedInline`] deliberately: that
    /// one names a terminal **codec** the inline form does not admit, while
    /// this is a **byte-stream filter** that is legal everywhere else a
    /// stream is legal. Collapsing the two would report "codec not allowed
    /// inline" about something that is not a codec, and an operator chasing
    /// that message would look in the wrong half of the file.
    #[error(
        "/BrotliDecode may not appear in an inline image (EXTN-BROTLI-1 \u{a7}5.2); \
         it is legal in any other stream"
    )]
    BrotliNotAllowedInline,
    /// Two or more terminal codecs in one `/Filter` chain, or a codec
    /// followed by further filters. Neither is meaningful: a codec
    /// consumes a codestream and produces samples, so nothing can be
    /// chained after it.
    #[error("/Filter chain has an image codec that is not the final filter")]
    CodecNotTerminal,
}

/// Which terminal codec a `/Filter` chain ends in, if any — so callers
/// can route **without decoding**.
///
/// The inline-image path uses this to reject `JBIG2Decode` and
/// `JPXDecode` outright before any bytes are touched (§7.4.7, §8.9.7).
///
/// # Errors
///
/// [`FilterError::BadFilterEntry`] if `/Filter` is neither a name nor
/// an array of names.
///
/// # Examples
///
/// ```
/// use pdfcer_core::image_codec::{terminal_codec, Codec};
/// use pdfcer_core::object::{Dict, Name, Object};
///
/// let mut dict = Dict::new();
/// dict.insert(Name::from(b"Filter"), Object::Name(Name::from(b"DCTDecode")));
/// assert_eq!(terminal_codec(&dict).unwrap(), Some(Codec::Dct));
///
/// let empty = Dict::new();
/// assert_eq!(terminal_codec(&empty).unwrap(), None);
/// ```
pub fn terminal_codec(dict: &Dict) -> Result<Option<Codec>, FilterError> {
    let names = filters::filter_names(dict)?;
    Ok(names.last().and_then(|n| Codec::from_name(n)))
}

/// Decode an image stream through its `/Filter` chain.
///
/// Runs the byte-stream **prefix** of the chain through
/// [`crate::filters`], then dispatches the single terminal image codec.
/// A chain with no terminal codec still succeeds — it yields a
/// [`CodedImage`] with `codec: None` and
/// [`CodecColorModel::Unspecified`], so the caller has one entry point
/// for every image rather than two code paths that can drift.
///
/// - `doc` resolves indirect values in the image and parameter
///   dictionaries, and will resolve `/JBIG2Globals` (Table 12) and the
///   `/Height` fallback for CCITT's absent `/Rows` when those codecs
///   land.
/// - `dict` is the image dictionary (Table 89) or an inline image's
///   already-normalized parameter dictionary.
/// - `raw` is the still-encoded stream data.
/// - `inline` selects §8.9.7's stricter filter rules.
///
/// This is the **base-revision** entry point, kept with its original
/// `&Document` signature so that every existing caller — the four
/// `image_codec_*` fuzz targets, this module's own tests, the
/// `cmyk_variants` acceptance test — is untouched by decision 018. It is a
/// thin wrapper over [`decode_image_view`]; a caller that must decode an
/// image as an editing session currently has it calls that directly.
///
/// # Errors
///
/// [`ImageCodecError`] — every failure is structured and names the
/// codec, and the specific unsupported feature where one applies
/// (rule R27). This function does not panic on any input; the
/// `image_codec_dct` fuzz target asserts it.
pub fn decode_image(
    doc: &Document,
    dict: &Dict,
    raw: &[u8],
    inline: bool,
) -> Result<CodedImage, ImageCodecError> {
    decode_image_view(&doc.view(), dict, raw, inline)
}

/// [`decode_image`] over an explicit [`DocumentView`] — the general form.
///
/// Exists because `pdfcer-render` renders **whatever view it was handed**
/// (decision 018): rasterizing an editing session must resolve an image
/// dictionary's indirect entries, and a `/JBIG2Globals` payload, against
/// the session rather than the file on disk. Everything else about the
/// decode is identical; see [`decode_image`] for the parameter contract and
/// the error posture.
///
/// # Errors
///
/// As [`decode_image`].
pub fn decode_image_view(
    doc: &DocumentView<'_>,
    dict: &Dict,
    raw: &[u8],
    inline: bool,
) -> Result<CodedImage, ImageCodecError> {
    decode_image_view_with(doc, dict, raw, inline, CmykJpegPolarity::default())
}

/// [`decode_image_view`] with an explicit `DCT-A1` polarity rule (R169).
///
/// ## What `polarity` decides, and what it does not
///
/// **Only** the one configuration nothing in the file disambiguates: a
/// `DCTDecode` image with **four components**, an **effective
/// `ColorTransform` of 0**, an **Adobe APP14 marker present**, and **no
/// `/Decode` array** in the image dictionary. Everything else about the
/// decode is unchanged, and in particular:
///
/// - the YCCK→CMYK inverse for effective transform 1 or 2 is **mandated**
///   by Table 13 (*"transformed … from YUVK to CMYK after decoding"*) and
///   is not a polarity guess — no setting reaches it;
/// - `/Decode` remains the sanctioned polarity control and remains
///   `pdfcer-render`'s to apply (rule R26);
/// - the R30 disclosure
///   ([`CodecNotes::cmyk_polarity_unverifiable`]) is still raised for the
///   ambiguous shape whichever way the setting points, because the shape
///   is a fact about the file and not about pdfcer's configuration.
///
/// The default is [`CmykJpegPolarity::NeverInvert`] — standing rule
/// **R29**, and **evidence tier (c)**: the strongest-sourced default in
/// the whole ambiguity register (Adobe TN #5116 contains `"invert"` zero
/// times; APP14 carries no polarity flag to test; all four reference
/// engines accept the ambiguity rather than inverting on marker presence).
///
/// ## A separate function rather than a changed signature
///
/// [`decode_image_view`] has callers in `image_import`, which this change
/// deliberately leaves alone, and in four fuzz targets. Adding a parameter
/// there would have made an unrelated module's re-decode path carry an
/// opinion it has none.
///
/// # Errors
///
/// As [`decode_image`].
pub fn decode_image_view_with(
    doc: &DocumentView<'_>,
    dict: &Dict,
    raw: &[u8],
    inline: bool,
    polarity: CmykJpegPolarity,
) -> Result<CodedImage, ImageCodecError> {
    let names = filters::filter_names(dict)?;

    // Locate the terminal codec. A codec anywhere but the last position
    // is not a chain pdfcer can make sense of — see `CodecNotTerminal`.
    let codec = names.last().and_then(|n| Codec::from_name(n));
    if names
        .iter()
        .rev()
        .skip(1)
        .any(|n| Codec::from_name(n).is_some())
    {
        return Err(ImageCodecError::CodecNotTerminal);
    }

    if let Some(codec) = codec
        && inline
        && !codec.allowed_inline()
    {
        return Err(ImageCodecError::NotAllowedInline { codec });
    }

    // EXTN-BROTLI-1 §5.2: "`BrotliDecode` SHALL NOT be used for inline
    // images." Checked over the WHOLE chain rather than only the terminal
    // position, because Brotli is a byte-stream filter and its legal place is
    // the prefix -- so `allowed_inline()`, which classifies terminal CODECS,
    // structurally cannot see it. The two checks look alike and are asking
    // different questions.
    //
    // ★ pdfium DECODES Brotli on inline images, which the extension forbids.
    // That is pdfium's behaviour, not the specification, and following it
    // would mean pdfcer reads files no conformant writer produces. Refusing is
    // also the safer asymmetry: a file that should not exist gets a named
    // error rather than a silent, plausible render.
    //
    // There is no abbreviation to check for -- the extension defines none,
    // precisely because Table 92's abbreviations exist for inline images and
    // this filter may not appear in one. (MuPDF accepts a `/Br` alias that
    // does not exist; pdfcer does not, at the dispatch in `filters`.)
    if inline && names.iter().any(|n| n.as_slice() == b"BrotliDecode") {
        return Err(ImageCodecError::BrotliNotAllowedInline);
    }

    // Byte-stream prefix: everything except the terminal codec.
    let prefix_len = names.len() - usize::from(codec.is_some());
    let mut filter_notes = FilterNotes::default();
    let data = filters::decode_prefix(dict, raw, prefix_len, &mut filter_notes)?;

    let mut notes = CodecNotes {
        lzw_framing_anomalies: filter_notes.lzw_framing_anomalies,
        ..CodecNotes::default()
    };

    let Some(codec) = codec else {
        return Ok(uncoded(doc, dict, data, notes));
    };

    let parms = codec_parms(dict, names.len());
    match codec {
        Codec::Dct => dct::decode(doc, &data, parms, dict, polarity, &mut notes),
        Codec::Ccitt => ccitt::decode(doc, &data, parms, dict, &mut notes),
        Codec::Jbig2 => jbig2::decode(doc, &data, parms, dict, &mut notes),
        // No `parms` argument: Table 6 gives `JPXDecode` no parameters
        // (VERIFIED — it is one of the four `no`-parameter filters), so
        // passing one would imply a configurability this filter does not
        // have. Everything configurable lives in the codestream or in
        // the image dictionary.
        #[cfg(feature = "jpx")]
        Codec::Jpx => jpx::decode(doc, &data, dict, &mut notes),
        // A build compiled without the `jpx` feature says so BY NAME rather
        // than returning a blank or a grey box (rule R27). The distinction
        // matters: "this build cannot decode JPEG 2000" is actionable — get
        // the full build — where a silently missing image is a document that
        // looks subtly wrong with no explanation anywhere.
        #[cfg(not(feature = "jpx"))]
        Codec::Jpx => Err(ImageCodecError::FeatureUnsupported {
            feature: "JPX/not-built",
        }),
    }
}

/// Build the [`CodedImage`] for a chain with **no** terminal codec.
///
/// The geometry fields echo the image dictionary so the caller sees one
/// uniform shape, and [`CodecColorModel::Unspecified`] states plainly
/// that no codec declared anything. Missing or nonsensical dictionary
/// entries become `0` rather than an error: validating Table 89 is the
/// renderer's job and it already does it, and duplicating that check
/// here would give two places for the rules to disagree.
fn uncoded(doc: &DocumentView<'_>, dict: &Dict, samples: Vec<u8>, notes: CodecNotes) -> CodedImage {
    let int = |key: &[u8]| -> u32 {
        dict.get(key)
            .map(|o| doc.resolve(o))
            .and_then(Object::as_int)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0)
    };
    CodedImage {
        samples,
        codec: None,
        width: int(b"Width"),
        height: int(b"Height"),
        components: 0,
        bits_per_component: u8::try_from(int(b"BitsPerComponent")).unwrap_or(0),
        color_model: CodecColorModel::Unspecified,
        icc_profile: None,
        embedded_alpha: None,
        notes,
    }
}

/// The `/DecodeParms` entry belonging to the terminal codec.
///
/// Table 5's positional rules apply unchanged: a lone dictionary when
/// the chain has one filter, otherwise the array position matching the
/// codec's index (the last one).
fn codec_parms(dict: &Dict, total: usize) -> Option<&Dict> {
    let parms = dict.get(b"DecodeParms").or_else(|| dict.get(b"DP"))?;
    match parms {
        Object::Dict(d) if total == 1 => Some(d),
        Object::Array(items) => match items.get(total.checked_sub(1)?) {
            Some(Object::Dict(d)) => Some(d),
            _ => None,
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

    fn dict_with(entries: Vec<(&[u8], Object)>) -> Dict {
        let mut d = Dict::new();
        for (k, v) in entries {
            d.insert(Name::from(k), v);
        }
        d
    }

    fn name(n: &[u8]) -> Object {
        Object::Name(Name::from(n))
    }

    #[test]
    fn terminal_codec_recognizes_all_four_and_their_abbreviations() {
        for (spelling, want) in [
            (&b"DCTDecode"[..], Codec::Dct),
            (b"DCT", Codec::Dct),
            (b"CCITTFaxDecode", Codec::Ccitt),
            (b"CCF", Codec::Ccitt),
            (b"JBIG2Decode", Codec::Jbig2),
            (b"JPXDecode", Codec::Jpx),
        ] {
            let d = dict_with(vec![(b"Filter", name(spelling))]);
            assert_eq!(terminal_codec(&d).unwrap(), Some(want), "{spelling:?}");
        }
    }

    #[test]
    fn terminal_codec_is_none_for_byte_stream_chains() {
        let d = dict_with(vec![(b"Filter", name(b"FlateDecode"))]);
        assert_eq!(terminal_codec(&d).unwrap(), None);
        assert_eq!(terminal_codec(&Dict::new()).unwrap(), None);
    }

    #[test]
    fn terminal_codec_sees_through_an_ascii_prefix() {
        // `/Filter [/ASCII85Decode /DCTDecode]` is a real, legal shape.
        let d = dict_with(vec![(
            b"Filter",
            Object::Array(vec![name(b"ASCII85Decode"), name(b"DCTDecode")]),
        )]);
        assert_eq!(terminal_codec(&d).unwrap(), Some(Codec::Dct));
    }

    #[test]
    fn inline_rules_match_the_spec() {
        // §8.9.7 Table 94 gives `DCT` and `CCF` abbreviations; §7.4.7
        // forbids JBIG2 inline, and JPX has no inline form.
        assert!(Codec::Dct.allowed_inline());
        assert!(Codec::Ccitt.allowed_inline());
        assert!(!Codec::Jbig2.allowed_inline());
        assert!(!Codec::Jpx.allowed_inline());
    }

    // -----------------------------------------------------------------
    // End-to-end decodes over real codestreams (`fixtures`)
    // -----------------------------------------------------------------

    /// The smallest document `decode_image` will accept a `&Document`
    /// from. Nothing in the DCT path reads the file body — `doc` is
    /// there for `/JBIG2Globals` and for resolving indirect parameter
    /// values — so a two-object catalog is enough.
    fn empty_document() -> Document {
        let mut buf = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (num, body) in [
            (1u32, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
        ] {
            offsets.push(buf.len());
            buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        let xref_at = buf.len();
        buf.extend_from_slice(b"xref\n0 3\n0000000000 65535 f\r\n");
        for off in offsets {
            buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
        }
        buf.extend_from_slice(
            format!("trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
        );
        Document::from_bytes(buf).unwrap()
    }

    /// An image dictionary declaring `DCTDecode` at the stated geometry.
    fn dct_dict(width: i64, height: i64) -> Dict {
        dict_with(vec![
            (b"Filter", name(b"DCTDecode")),
            (b"Width", Object::Integer(width)),
            (b"Height", Object::Integer(height)),
            (b"BitsPerComponent", Object::Integer(8)),
        ])
    }

    fn decode_fixture(bytes: &[u8], dict: &Dict) -> Result<CodedImage, ImageCodecError> {
        decode_image(&empty_document(), dict, bytes, false)
    }

    #[test]
    fn grayscale_jpeg_decodes_to_one_component_per_pixel() {
        let img = decode_fixture(fixtures::GRAY_2X2, &dct_dict(2, 2)).unwrap();
        assert_eq!(img.codec, Some(Codec::Dct));
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.components, 1);
        assert_eq!(img.bits_per_component, 8);
        assert_eq!(img.color_model, CodecColorModel::Gray);
        assert_eq!(img.samples.len(), 4, "2x2 x 1 component");
        // Source ramp 0, 85, 170, 255 — monotonic and near the originals
        // at quality 90.
        assert!(img.samples[0] < 20, "{:?}", img.samples);
        assert!(img.samples[3] > 235, "{:?}", img.samples);
        assert!(img.samples[0] < img.samples[1]);
        assert!(img.samples[1] < img.samples[2]);
        assert!(!img.notes.cmyk_image);
        assert!(!img.notes.geometry_mismatch);
    }

    #[test]
    fn grayscale_ignores_color_transform_per_table_13() {
        // "This option shall be ignored if the image has one or two
        // colour components" — so `/ColorTransform 1` on a grayscale
        // JPEG is not an error and changes nothing.
        let mut dict = dct_dict(2, 2);
        dict.insert(
            Name::from(b"DecodeParms"),
            Object::Dict(dict_with(vec![(b"ColorTransform", Object::Integer(1))])),
        );
        let with_parm = decode_fixture(fixtures::GRAY_2X2, &dict).unwrap();
        let without = decode_fixture(fixtures::GRAY_2X2, &dct_dict(2, 2)).unwrap();
        assert_eq!(with_parm.samples, without.samples);
        assert_eq!(with_parm.color_model, CodecColorModel::Gray);
    }

    /// Assert a 2x2 RGB decode came back as red / green / blue / white.
    fn assert_rgb_primaries(samples: &[u8]) {
        assert_eq!(samples.len(), 12, "2x2 x 3 components");
        let px = |i: usize| (samples[i * 3], samples[i * 3 + 1], samples[i * 3 + 2]);
        let (r, g, b) = px(0);
        assert!(
            r > 200 && g < 60 && b < 60,
            "pixel 0 should be red: {r},{g},{b}"
        );
        let (r, g, b) = px(1);
        assert!(
            r < 60 && g > 200 && b < 60,
            "pixel 1 should be green: {r},{g},{b}"
        );
        let (r, g, b) = px(2);
        assert!(
            r < 60 && g < 60 && b > 200,
            "pixel 2 should be blue: {r},{g},{b}"
        );
        let (r, g, b) = px(3);
        assert!(
            r > 200 && g > 200 && b > 200,
            "pixel 3 should be white: {r},{g},{b}"
        );
    }

    #[test]
    fn rgb_jpeg_with_no_marker_takes_table_13s_default_transform_of_one() {
        let img = decode_fixture(fixtures::RGB_2X2, &dct_dict(2, 2)).unwrap();
        assert_eq!(img.components, 3);
        assert_eq!(img.color_model, CodecColorModel::Rgb);
        assert_rgb_primaries(&img.samples);
    }

    #[test]
    fn progressive_jpeg_decodes_identically_to_baseline() {
        // §7.4.8: progressive support is required from PDF 1.3, and it
        // is 14% of the measured corpus.
        let img = decode_fixture(fixtures::RGB_2X2_PROGRESSIVE, &dct_dict(2, 2)).unwrap();
        assert_eq!(img.color_model, CodecColorModel::Rgb);
        assert_rgb_primaries(&img.samples);
    }

    #[test]
    fn app14_transform_one_reaches_the_same_result_as_the_default() {
        let img = decode_fixture(fixtures::RGB_2X2_APP14_T1, &dct_dict(2, 2)).unwrap();
        assert_eq!(img.color_model, CodecColorModel::Rgb);
        assert_rgb_primaries(&img.samples);
    }

    #[test]
    fn app14_transform_zero_delivers_untransformed_components() {
        // THE routing regression test. The stored components are YCbCr,
        // so a transform-0 decode hands them back verbatim — visibly
        // different from the transform-1 decode of the very same
        // entropy-coded data.
        let raw = decode_fixture(fixtures::RGB_2X2_APP14_T0, &dct_dict(2, 2)).unwrap();
        let transformed = decode_fixture(fixtures::RGB_2X2, &dct_dict(2, 2)).unwrap();
        assert_eq!(raw.color_model, CodecColorModel::Untransformed3);
        assert_eq!(raw.components, 3);
        assert_eq!(raw.samples.len(), 12);
        assert_ne!(
            raw.samples, transformed.samples,
            "ColorTransform 0 must NOT apply the YCbCr->RGB inverse"
        );
    }

    #[test]
    fn the_marker_outranks_the_dictionary() {
        // Table 13, verbatim: "then the colours shall be transformed, or
        // not … according to the value provided in the encoded data and
        // the value of this dictionary entry shall be ignored."
        // /ColorTransform 1 must NOT override an APP14 saying 0.
        let mut dict = dct_dict(2, 2);
        dict.insert(
            Name::from(b"DecodeParms"),
            Object::Dict(dict_with(vec![(b"ColorTransform", Object::Integer(1))])),
        );
        let img = decode_fixture(fixtures::RGB_2X2_APP14_T0, &dict).unwrap();
        assert_eq!(
            img.color_model,
            CodecColorModel::Untransformed3,
            "the APP14 marker wins"
        );
    }

    #[test]
    fn the_dictionary_wins_when_there_is_no_marker() {
        // Second level of the precedence chain: no Adobe marker, so
        // /ColorTransform 0 applies and suppresses the inverse.
        let mut dict = dct_dict(2, 2);
        dict.insert(
            Name::from(b"DecodeParms"),
            Object::Dict(dict_with(vec![(b"ColorTransform", Object::Integer(0))])),
        );
        let img = decode_fixture(fixtures::RGB_2X2, &dict).unwrap();
        assert_eq!(img.color_model, CodecColorModel::Untransformed3);
        let default = decode_fixture(fixtures::RGB_2X2, &dct_dict(2, 2)).unwrap();
        assert_ne!(img.samples, default.samples);
    }

    #[test]
    fn unknown_adobe_transform_is_a_named_diagnostic_not_a_vendor_error() {
        // zune-jpeg hard-errors on this inside header parsing; the
        // pre-sniff must catch it first (rule R27).
        assert_eq!(
            decode_fixture(fixtures::RGB_2X2_APP14_T3, &dct_dict(2, 2)).unwrap_err(),
            ImageCodecError::FeatureUnsupported {
                feature: "DCT/adobe-transform-3"
            }
        );
    }

    #[test]
    fn cmyk_jpeg_passes_four_raw_components_through_and_is_counted() {
        // This fixture is APP14 transform 0 with no /Decode — exactly
        // decision 006 §4.4's R30 shape, so it must trip the
        // polarity-unverifiable diagnostic and NOT the benign YCCK
        // census (which is reserved for effective transform 1/2).
        let img = decode_fixture(fixtures::CMYK_2X2, &dct_dict(2, 2)).unwrap();
        assert_eq!(img.components, 4);
        assert_eq!(img.color_model, CodecColorModel::Cmyk);
        assert_eq!(img.samples.len(), 16, "2x2 x 4 components");
        assert!(
            img.notes.cmyk_polarity_unverifiable,
            "transform 0 + no /Decode is the R30 shape and must announce itself (decision 006)"
        );
        assert!(
            !img.notes.cmyk_image,
            "the benign census counts YCCK (transform 1/2) only (decision 006 §4.4)"
        );
        // libjpeg writes APP14 transform 0 for CMYK, so Table 13 says no
        // transformation: the samples are exactly what the encoder
        // stored. pdfcer applies NO "Adobe inversion" of its own (R29 —
        // no shipping PDF engine does), so the stored complement
        // survives to `/Decode`.
        let first = &img.samples[0..4];
        assert!(
            first.iter().any(|&v| v > 200),
            "raw stored samples, not inverted: {first:?}"
        );
    }

    #[test]
    fn cmyk_jpeg_with_a_decode_array_is_not_polarity_unverifiable() {
        // Same R30 shape, but the producer DECLARED its polarity via
        // /Decode — the sanctioned mechanism (decision 006 §4.3) — so
        // the residual-ambiguity diagnostic must stay quiet. The array
        // is only OBSERVED here (R26 clarification): applying it is
        // pdfcer-render's job, so the samples must be byte-identical to
        // the no-/Decode decode.
        let mut dict = dct_dict(2, 2);
        dict.insert(
            Name::from(b"Decode"),
            Object::Array(vec![
                Object::Integer(1),
                Object::Integer(0),
                Object::Integer(1),
                Object::Integer(0),
                Object::Integer(1),
                Object::Integer(0),
                Object::Integer(1),
                Object::Integer(0),
            ]),
        );
        let img = decode_fixture(fixtures::CMYK_2X2, &dict).unwrap();
        assert!(!img.notes.cmyk_polarity_unverifiable);
        assert!(!img.notes.cmyk_image);
        let plain = decode_fixture(fixtures::CMYK_2X2, &dct_dict(2, 2)).unwrap();
        assert_eq!(
            img.samples, plain.samples,
            "/Decode is observed for classification, never applied here"
        );
    }

    #[test]
    fn geometry_disagreement_is_counted_and_the_codestream_is_reported() {
        // The dictionary claims 7x9; the codestream says 2x2. Both
        // numbers survive: the codestream's in `CodedImage`, the
        // divergence in `notes`.
        let img = decode_fixture(fixtures::RGB_2X2, &dct_dict(7, 9)).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert!(img.notes.geometry_mismatch);
    }

    #[test]
    fn a_wrong_bits_per_component_is_a_geometry_disagreement() {
        // Table 89: a DCTDecode filter "shall always deliver 8-bit
        // samples", so any other stated value is inconsistent.
        let mut dict = dct_dict(2, 2);
        dict.insert(Name::from(b"BitsPerComponent"), Object::Integer(4));
        let img = decode_fixture(fixtures::RGB_2X2, &dict).unwrap();
        assert_eq!(img.bits_per_component, 8);
        assert!(img.notes.geometry_mismatch);
    }

    #[test]
    fn a_codestream_past_max_image_pixels_is_refused_before_allocating() {
        // 65535 x 65535 = 4.3 Gpx. Refused on the declared geometry, not
        // after a 17 GiB allocation attempt (rule R25).
        assert_eq!(
            decode_fixture(fixtures::RGB_HUGE_DIMS, &dct_dict(65535, 65535)).unwrap_err(),
            ImageCodecError::TooLarge
        );
    }

    #[test]
    fn corrupt_codestream_errs_never_returns_plausible_samples() {
        // The fail-clean contract, at the codec layer.
        let mut truncated = fixtures::RGB_2X2.to_vec();
        truncated.truncate(truncated.len() / 2);
        assert!(matches!(
            decode_fixture(&truncated, &dct_dict(2, 2)),
            Err(ImageCodecError::Corrupt { .. })
        ));
        assert!(matches!(
            decode_fixture(b"not a jpeg", &dct_dict(2, 2)),
            Err(ImageCodecError::Corrupt { .. })
        ));
    }

    #[test]
    fn an_ascii85_prefix_is_run_before_the_codec() {
        // `/Filter [/ASCII85Decode /DCTDecode]` — a real, legal shape.
        // The byte-stream prefix must run first and the codec must see
        // the decoded codestream.
        let armoured = ascii85(fixtures::RGB_2X2);
        let mut dict = dct_dict(2, 2);
        dict.insert(
            Name::from(b"Filter"),
            Object::Array(vec![name(b"ASCII85Decode"), name(b"DCTDecode")]),
        );
        let img = decode_fixture(armoured.as_bytes(), &dict).unwrap();
        assert_eq!(img.codec, Some(Codec::Dct));
        assert_rgb_primaries(&img.samples);
    }

    /// Minimal ASCII85 encoder — test-only, for the chained-filter case.
    fn ascii85(data: &[u8]) -> String {
        let mut out = String::new();
        for chunk in data.chunks(4) {
            let mut word = [0u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            let mut value = u32::from_be_bytes(word);
            let mut group = [0u8; 5];
            for slot in group.iter_mut().rev() {
                *slot = b'!' + (value % 85) as u8;
                value /= 85;
            }
            out.extend(group[..chunk.len() + 1].iter().map(|&b| b as char));
        }
        out.push_str("~>");
        out
    }

    #[test]
    fn a_chain_with_no_codec_yields_an_unspecified_coded_image() {
        // One entry point for every image: a Flate-only image still
        // comes back as a `CodedImage`, just with nothing declared.
        let dict = dict_with(vec![
            (b"Width", Object::Integer(4)),
            (b"Height", Object::Integer(2)),
            (b"BitsPerComponent", Object::Integer(8)),
        ]);
        let img = decode_fixture(b"rawsamples", &dict).unwrap();
        assert_eq!(img.codec, None);
        assert_eq!(img.color_model, CodecColorModel::Unspecified);
        assert_eq!((img.width, img.height), (4, 2));
        assert_eq!(img.components, 0, "no codec declared a component count");
        assert_eq!(img.samples, b"rawsamples");
    }

    #[test]
    fn every_recognized_codec_is_now_implemented() {
        // `ImageCodecError::Unsupported` is retained after Pass 2.3 so a
        // regression stays visible (decision 005 §6.4), but no codec
        // should reach it any more. Each of the four is fed bytes that
        // are certainly not a valid codestream: the failure must be
        // Corrupt/FeatureUnsupported — "these bytes are broken" — never
        // "this codec is not built".
        for filter in [
            &b"DCTDecode"[..],
            b"CCITTFaxDecode",
            b"JBIG2Decode",
            b"JPXDecode",
        ] {
            let dict = dict_with(vec![
                (b"Filter", name(filter)),
                (b"Width", Object::Integer(4)),
                (b"Height", Object::Integer(2)),
            ]);
            let err = decode_fixture(b"not a codestream", &dict).unwrap_err();
            assert!(
                !matches!(err, ImageCodecError::Unsupported { .. }),
                "{}: still reports the codec as unimplemented ({err})",
                String::from_utf8_lossy(filter),
            );
        }
    }

    // -----------------------------------------------------------------
    // JPXDecode (§7.4.9, §8.9.5 Table 89) — Pass 2.3
    // -----------------------------------------------------------------

    /// A JPX image dictionary. `entries` carries whatever Table 89 keys
    /// a given test is about; `/Filter` is added here.
    ///
    /// Note what is NOT added by default: `/ColorSpace` and
    /// `/BitsPerComponent`. Table 89 makes both optional for this
    /// filter, so the *bare* dictionary is the conformant baseline and
    /// every entry beyond `/Width`//`/Height` is something a specific
    /// test chose to say.
    #[cfg(feature = "jpx")]
    fn jpx_dict(entries: Vec<(&[u8], Object)>) -> Dict {
        let mut all: Vec<(&[u8], Object)> = vec![(b"Filter", name(b"JPXDecode"))];
        all.extend(entries);
        dict_with(all)
    }

    /// The bare 16 x 4 grayscale dictionary the two container-shape
    /// fixtures share.
    #[cfg(feature = "jpx")]
    fn jpx_gray_dict() -> Dict {
        jpx_dict(vec![
            (b"Width", Object::Integer(16)),
            (b"Height", Object::Integer(4)),
        ])
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_decodes_a_jp2_container_to_the_expected_samples() {
        let img = decode_fixture(fixtures_jpx::JPX_GRAY_8_JP2, &jpx_gray_dict()).unwrap();
        assert_eq!(img.codec, Some(Codec::Jpx));
        assert_eq!((img.width, img.height), (16, 4));
        assert_eq!(img.components, 1);
        assert_eq!(img.color_model, CodecColorModel::Gray);
        assert_eq!(img.samples, fixtures_jpx::JPX_GRAY_8_SAMPLES);
        assert!(!img.notes.geometry_mismatch);
        assert_eq!(img.embedded_alpha, None);
        assert_eq!(img.icc_profile, None);
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_decodes_a_raw_codestream_identically_to_its_jp2_container() {
        // §7.4.9 says the filter "shall expect to read a full JPX file
        // structure", but real producers embed bare codestreams and
        // `hayro-jpeg2000` sniffs which it has. The two shapes carry the
        // same picture, so they must produce the same bytes — which is
        // what proves pdfcer reads geometry and colour from the
        // codestream rather than from the container.
        let boxed = decode_fixture(fixtures_jpx::JPX_GRAY_8_JP2, &jpx_gray_dict()).unwrap();
        let bare = decode_fixture(fixtures_jpx::JPX_GRAY_8_J2K, &jpx_gray_dict()).unwrap();
        assert_eq!(bare.samples, fixtures_jpx::JPX_GRAY_8_SAMPLES);
        assert_eq!(bare.samples, boxed.samples);
        assert_eq!((bare.width, bare.height), (boxed.width, boxed.height));
        assert_eq!(bare.color_model, boxed.color_model);
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_interleaves_rgb_components_in_order() {
        let dict = jpx_dict(vec![
            (b"Width", Object::Integer(4)),
            (b"Height", Object::Integer(2)),
        ]);
        let img = decode_fixture(fixtures_jpx::JPX_RGB_8_JP2, &dict).unwrap();
        assert_eq!(img.components, 3);
        assert_eq!(img.color_model, CodecColorModel::Rgb);
        assert_eq!(img.samples, fixtures_jpx::JPX_RGB_8_SAMPLES);
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_reports_cmyk_from_enumerated_colour_space_twelve() {
        // §7.4.9 singles this value out: enumerated colour space 12
        // (CMYK) "is part of JPX but not JPX baseline" and "shall be
        // supported in a PDF" regardless.
        let dict = jpx_dict(vec![
            (b"Width", Object::Integer(4)),
            (b"Height", Object::Integer(2)),
        ]);
        let img = decode_fixture(fixtures_jpx::JPX_CMYK_8_JP2, &dict).unwrap();
        assert_eq!(img.components, 4);
        assert_eq!(img.color_model, CodecColorModel::Cmyk);
        assert_eq!(img.samples, fixtures_jpx::JPX_CMYK_8_SAMPLES);
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_absent_colour_space_is_conformant_and_the_codestream_supplies_it() {
        // THE Table 89 inversion, from the side that a conventional
        // reader gets wrong: `/ColorSpace` is "Required for images,
        // EXCEPT those that use the JPXDecode filter". A dictionary with
        // no `/ColorSpace` and no `/BitsPerComponent` is fully
        // conformant, and the codec must supply both.
        let dict = jpx_gray_dict();
        assert!(dict.get(b"ColorSpace").is_none());
        assert!(dict.get(b"BitsPerComponent").is_none());
        let img = decode_fixture(fixtures_jpx::JPX_GRAY_8_JP2, &dict).unwrap();
        assert_eq!(img.color_model, CodecColorModel::Gray);
        assert_eq!(img.bits_per_component, 8);
        assert!(
            !img.notes.geometry_mismatch,
            "absent optional entries are not a disagreement"
        );
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_bits_per_component_is_ignored_and_a_stated_value_is_counted() {
        // Table 89: "this entry is optional and shall be ignored if
        // present. The bit depth is determined by the conforming reader
        // in the process of decoding." So a 16-bit codestream whose
        // dictionary honestly says `/BitsPerComponent 16` still yields
        // 8-bit samples — and the entry's presence is still counted,
        // because it is the one entry a reader is told to ignore.
        let dict = jpx_dict(vec![
            (b"Width", Object::Integer(4)),
            (b"Height", Object::Integer(2)),
            (b"BitsPerComponent", Object::Integer(16)),
        ]);
        let img = decode_fixture(fixtures_jpx::JPX_GRAY_16_JP2, &dict).unwrap();
        assert_eq!(img.bits_per_component, 8);
        assert_eq!(img.samples, fixtures_jpx::JPX_GRAY_16_SAMPLES);
        assert!(img.notes.geometry_mismatch);
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_scales_high_bit_depths_full_range_not_by_high_byte() {
        // The discriminator pixel is 0x00FF: full-range scaling gives 1,
        // a high-byte truncation gives 0. See `jpx`'s module docs.
        let dict = jpx_dict(vec![
            (b"Width", Object::Integer(4)),
            (b"Height", Object::Integer(2)),
        ]);
        let img = decode_fixture(fixtures_jpx::JPX_GRAY_16_JP2, &dict).unwrap();
        assert_eq!(img.samples, fixtures_jpx::JPX_GRAY_16_SAMPLES);
        assert_eq!(img.samples.first().copied(), Some(0x00), "0 stays 0");
        assert_eq!(
            img.samples.get(1).copied(),
            Some(0xFF),
            "the white point 2^16-1 must land on exactly 255"
        );
        assert_eq!(
            img.samples.get(4).copied(),
            Some(0x01),
            "0x00FF must scale to 1, not truncate to 0"
        );
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_dictionary_geometry_disagreement_is_counted_and_the_codestream_reported() {
        // §7.4.9 requires `/Width`//`/Height` to match the codestream
        // but gives no conflict rule. pdfcer reports the codestream's
        // numbers and counts the divergence; the renderer keeps the
        // dictionary's for placement.
        let dict = jpx_dict(vec![
            (b"Width", Object::Integer(999)),
            (b"Height", Object::Integer(7)),
        ]);
        let img = decode_fixture(fixtures_jpx::JPX_GRAY_8_JP2, &dict).unwrap();
        assert_eq!((img.width, img.height), (16, 4));
        assert!(img.notes.geometry_mismatch);
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_colour_space_channel_count_disagreement_is_counted() {
        // §7.4.9: "The number of colour channels in the JPEG2000 data
        // shall match the number of components in the colour space."
        // A `/DeviceRGB` dictionary over a grayscale codestream does
        // not, and the spec gives no recovery rule — so it is counted,
        // not refused, and the codec still reports what it actually has.
        let dict = jpx_dict(vec![
            (b"Width", Object::Integer(16)),
            (b"Height", Object::Integer(4)),
            (b"ColorSpace", name(b"DeviceRGB")),
        ]);
        let img = decode_fixture(fixtures_jpx::JPX_GRAY_8_JP2, &dict).unwrap();
        assert_eq!(img.components, 1);
        assert_eq!(img.color_model, CodecColorModel::Gray);
        assert!(img.notes.geometry_mismatch);
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_matching_colour_space_is_not_a_disagreement() {
        let dict = jpx_dict(vec![
            (b"Width", Object::Integer(4)),
            (b"Height", Object::Integer(2)),
            (b"ColorSpace", name(b"DeviceRGB")),
        ]);
        let img = decode_fixture(fixtures_jpx::JPX_RGB_8_JP2, &dict).unwrap();
        assert!(!img.notes.geometry_mismatch);
        assert_eq!(img.samples, fixtures_jpx::JPX_RGB_8_SAMPLES);
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_smask_in_data_defaults_to_ignoring_the_opacity_channel() {
        // Table 89's default is 0: "If present, encoded soft-mask image
        // information shall be ignored." A decoder that always hands
        // back the alpha it found is wrong. The channel is still lifted
        // OUT of the colour samples — it is not a colour component.
        let dict = jpx_dict(vec![
            (b"Width", Object::Integer(4)),
            (b"Height", Object::Integer(2)),
        ]);
        let img = decode_fixture(fixtures_jpx::JPX_RGBA_8_JP2, &dict).unwrap();
        assert_eq!(img.components, 3);
        assert_eq!(img.embedded_alpha, None, "/SMaskInData 0 means ignore");
        assert_eq!(
            img.samples,
            fixtures_jpx::JPX_RGBA_8_SAMPLES,
            "the opacity channel must not stay interleaved with the colours"
        );
        assert!(!img.notes.jpx_smask_in_data_preblended);
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_smask_in_data_one_exposes_the_opacity_channel() {
        let dict = jpx_dict(vec![
            (b"Width", Object::Integer(4)),
            (b"Height", Object::Integer(2)),
            (b"SMaskInData", Object::Integer(1)),
        ]);
        let img = decode_fixture(fixtures_jpx::JPX_RGBA_8_JP2, &dict).unwrap();
        assert_eq!(img.components, 3);
        assert_eq!(img.samples, fixtures_jpx::JPX_RGBA_8_SAMPLES);
        assert_eq!(
            img.embedded_alpha.as_deref(),
            Some(fixtures_jpx::JPX_RGBA_8_ALPHA)
        );
        assert!(!img.notes.jpx_smask_in_data_preblended);
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_smask_in_data_two_is_deferred_by_name_and_never_applied() {
        // Value 2 means the colour channels were preblended with a
        // backdrop and the opacity channel would need a `Matte` entry
        // to undo. Recognized, counted, and NOT applied: the colours
        // come back as stored (a real picture over that backdrop) and
        // the alpha stays unexposed, because using it without
        // un-premultiplying would double-darken every soft pixel.
        let dict = jpx_dict(vec![
            (b"Width", Object::Integer(4)),
            (b"Height", Object::Integer(2)),
            (b"SMaskInData", Object::Integer(2)),
        ]);
        let img = decode_fixture(fixtures_jpx::JPX_RGBA_8_JP2, &dict).unwrap();
        assert!(img.notes.jpx_smask_in_data_preblended);
        assert_eq!(img.embedded_alpha, None);
        assert_eq!(img.samples, fixtures_jpx::JPX_RGBA_8_SAMPLES);
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_smask_in_data_out_of_range_falls_back_to_the_default() {
        // 3 is undefined. Ignoring the codestream's alpha is the outcome
        // that cannot corrupt what is drawn; refusing the image over one
        // stray integer would be a total loss.
        for value in [-1, 3, 99] {
            let dict = jpx_dict(vec![
                (b"Width", Object::Integer(4)),
                (b"Height", Object::Integer(2)),
                (b"SMaskInData", Object::Integer(value)),
            ]);
            let img = decode_fixture(fixtures_jpx::JPX_RGBA_8_JP2, &dict).unwrap();
            assert_eq!(img.embedded_alpha, None, "SMaskInData {value}");
            assert!(
                !img.notes.jpx_smask_in_data_preblended,
                "SMaskInData {value}"
            );
        }
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_smask_in_data_without_an_opacity_channel_is_a_disagreement() {
        // "If SMaskInData is nonzero, there shall be only one opacity
        // channel in the JPEG2000 data" presupposes one exists. It does
        // not here — counted, not refused, because nothing about the
        // colour samples is wrong.
        let dict = jpx_dict(vec![
            (b"Width", Object::Integer(16)),
            (b"Height", Object::Integer(4)),
            (b"SMaskInData", Object::Integer(1)),
        ]);
        let img = decode_fixture(fixtures_jpx::JPX_GRAY_8_JP2, &dict).unwrap();
        assert_eq!(img.embedded_alpha, None);
        assert!(img.notes.geometry_mismatch);
        assert_eq!(img.samples, fixtures_jpx::JPX_GRAY_8_SAMPLES);
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_ignores_the_decode_array_entirely_at_this_layer() {
        // Table 89: "If the image uses the JPXDecode filter and
        // ImageMask is false, Decode shall be ignored by a conforming
        // reader." Rule R26 already forbids this layer from applying
        // one, so an inverting `/Decode` must change nothing here — and
        // `pdfcer-render` is where the suppression is enforced for real.
        let plain = jpx_dict(vec![
            (b"Width", Object::Integer(16)),
            (b"Height", Object::Integer(4)),
        ]);
        let inverted = jpx_dict(vec![
            (b"Width", Object::Integer(16)),
            (b"Height", Object::Integer(4)),
            (
                b"Decode",
                Object::Array(vec![Object::Integer(1), Object::Integer(0)]),
            ),
        ]);
        let a = decode_fixture(fixtures_jpx::JPX_GRAY_8_JP2, &plain).unwrap();
        let b = decode_fixture(fixtures_jpx::JPX_GRAY_8_JP2, &inverted).unwrap();
        assert_eq!(a.samples, b.samples);
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_corrupt_data_errs_and_never_returns_plausible_samples() {
        // Fail-clean (decision 001 §6.1.4): corrupt in, `Err` out. A
        // truncated codestream is the realistic shape.
        let mut truncated = fixtures_jpx::JPX_GRAY_8_JP2.to_vec();
        truncated.truncate(truncated.len() / 2);
        for data in [&b"not a codestream at all"[..], &truncated, &[]] {
            let err = decode_fixture(data, &jpx_gray_dict()).unwrap_err();
            assert!(
                matches!(
                    err,
                    ImageCodecError::Corrupt {
                        codec: Codec::Jpx,
                        ..
                    } | ImageCodecError::FeatureUnsupported { .. }
                ),
                "unexpected error for {} bytes: {err}",
                data.len()
            );
        }
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_refuses_an_absurd_tile_grid_that_no_pixel_ceiling_can_see() {
        // A CPU-exhaustion vector the `image_codec_jpx` fuzz target
        // found in its first minute: T.800's tile grid is declared
        // independently of the image size, so `XTsiz`/`YTsiz` of 4 x 2
        // over a 512 x 1024 image is **65,536 tiles** from 310 bytes.
        // `hayro-jpeg2000` builds one structure per tile — 32 seconds
        // of work for half a megapixel of output.
        //
        // Neither MAX_IMAGE_PIXELS (512 Kpx is 1/64th of it) nor the
        // sample/working-set ceilings see this, which is exactly why
        // MAX_TILES exists (rule R25).
        //
        // Built by rewriting the fixture's SIZ marker, so the rest of
        // the main header stays valid and the parse genuinely reaches
        // the tile check. Field offsets from SOC (T.800 A.5.1):
        //   8 Xsiz | 12 Ysiz | 24 XTsiz | 28 YTsiz
        let mut shredded = fixtures_jpx::JPX_GRAY_8_J2K.to_vec();
        for (offset, value) in [(8usize, 512u32), (12, 1024), (24, 4), (28, 2)] {
            let Some(slot) = shredded.get_mut(offset..offset + 4) else {
                panic!("fixture is shorter than a SIZ marker segment");
            };
            slot.copy_from_slice(&value.to_be_bytes());
        }
        assert_eq!(
            decode_fixture(&shredded, &jpx_gray_dict()).unwrap_err(),
            ImageCodecError::TooLarge,
            "a 65,536-tile grid must be refused before any tile is built"
        );
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_accepts_a_tile_grid_within_the_ceiling() {
        // The other side of the guard, which is the half that catches an
        // over-tight ceiling: the ordinary single-tile fixture must
        // still decode. Every ceiling in this project gets this pair
        // after MAX_TOKEN_LEN and MAX_XOBJECT_DEPTH both shipped too
        // tight and rejected conformant files.
        let img = decode_fixture(fixtures_jpx::JPX_GRAY_8_J2K, &jpx_gray_dict()).unwrap();
        assert_eq!(img.samples, fixtures_jpx::JPX_GRAY_8_SAMPLES);
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_unsupported_enumerated_colour_space_is_a_named_diagnostic() {
        // §7.4.9 permits enumerated colour space **19 (CIEJab)**
        // explicitly ("limited to the JPX baseline set of features,
        // except for enumerated colour space 19"), and one veraPDF
        // corpus file uses it. `hayro-jpeg2000` does not implement it
        // and refuses during colour resolution, before any sample
        // exists — so pdfcer names the gap rather than reporting a
        // corrupt file (rule R27). See `jpx::map_error` for the
        // recorded divergence from §7.4.9's fallback ladder.
        //
        // Built by rewriting the RGB fixture's `colr` box from 16 (sRGB)
        // to 19: JP2 boxes are length-prefixed, so the edit is in place
        // and every other byte — including the codestream — stays valid.
        // The `colr` payload is METH u8, PREC u8, APPROX u8, then the
        // 4-byte enumerated value when METH is 1.
        let mut ciejab = fixtures_jpx::JPX_RGB_8_JP2.to_vec();
        let tag = ciejab
            .windows(4)
            .position(|w| w == b"colr")
            .expect("the fixture must carry a colr box");
        let at = tag + 4 + 3;
        let Some(slot) = ciejab.get_mut(at..at + 4) else {
            panic!("colr box is truncated");
        };
        assert_eq!(slot, &16u32.to_be_bytes(), "fixture should start as sRGB");
        slot.copy_from_slice(&19u32.to_be_bytes());

        let dict = jpx_dict(vec![
            (b"Width", Object::Integer(4)),
            (b"Height", Object::Integer(2)),
        ]);
        assert_eq!(
            decode_fixture(&ciejab, &dict).unwrap_err(),
            ImageCodecError::FeatureUnsupported {
                feature: "JPX/enumerated-colour-space",
            }
        );
    }

    // Needs the `jpx` codec compiled in; see the feature block in Cargo.toml.
    #[cfg(feature = "jpx")]
    #[test]
    fn jpx_refuses_a_geometry_past_the_pdfcer_ceiling_before_decoding() {
        // A SIZ marker claiming 60000 x 60000 (3.6 Gpx) is inside
        // `hayro-jpeg2000`'s own 60000-per-axis cap, which does not
        // bound the PRODUCT — so pdfcer's ceiling is the only thing
        // between a 40-byte header and a multi-gigabyte allocation
        // (rule R25), and it has to fire between `Image::new` (headers
        // only) and `Image::decode` (which allocates one f32 per sample
        // per component from exactly these numbers).
        //
        // Built by rewriting the *real* fixture's SIZ marker rather than
        // by hand-assembling a codestream: the rest of the main header
        // (COD, QCD) then stays valid, so the parse genuinely reaches
        // the dimension check instead of failing earlier for an
        // unrelated reason — which is how the first draft of this test
        // passed for the wrong cause.
        //
        // SIZ layout (T.800 A.5.1), byte offsets from SOC:
        //   0 SOC | 2 SIZ | 4 Lsiz | 6 Rsiz | 8 Xsiz | 12 Ysiz
        //   16 XOsiz | 20 YOsiz | 24 XTsiz | 28 YTsiz | ...
        // The tile size is enlarged alongside the image so the tile grid
        // stays 1 x 1 and the geometry remains self-consistent.
        let mut huge = fixtures_jpx::JPX_GRAY_8_J2K.to_vec();
        for offset in [8usize, 12, 24, 28] {
            let Some(slot) = huge.get_mut(offset..offset + 4) else {
                panic!("fixture is shorter than a SIZ marker segment");
            };
            slot.copy_from_slice(&60_000u32.to_be_bytes());
        }
        let err = decode_fixture(&huge, &jpx_gray_dict()).unwrap_err();
        assert_eq!(
            err,
            ImageCodecError::TooLarge,
            "a 3.6 Gpx codestream must be refused before allocating"
        );
    }

    // -----------------------------------------------------------------
    // CCITTFaxDecode (§7.4.6, Table 11) — Pass 2.2
    // -----------------------------------------------------------------

    /// An image dictionary declaring `CCITTFaxDecode` over the 16 x 4
    /// fixture geometry, with the given `/DecodeParms` entries.
    fn ccitt_dict(parms: Vec<(&[u8], Object)>) -> Dict {
        dict_with(vec![
            (b"Filter", name(b"CCITTFaxDecode")),
            (b"Width", Object::Integer(16)),
            (b"Height", Object::Integer(4)),
            (b"BitsPerComponent", Object::Integer(1)),
            (b"DecodeParms", Object::Dict(dict_with(parms))),
        ])
    }

    /// The three `K` cases, each with the fixture that matches it.
    fn ccitt_variants() -> [(&'static str, i64, &'static [u8]); 3] {
        [
            ("Group 4 (K < 0)", -1, fixtures_bilevel::CCITT_G4_16X4),
            ("Group 3 1-D (K = 0)", 0, fixtures_bilevel::CCITT_G3_1D_16X4),
            ("Group 3 2-D (K > 0)", 4, fixtures_bilevel::CCITT_G3_2D_16X4),
        ]
    }

    #[test]
    fn every_k_variant_decodes_to_the_same_picture() {
        // The headline CCITT test. Three genuinely different bit
        // streams — 1-D run codes, 2-D mode codes, and the mixed form
        // with a per-line tag bit — must all produce the identical
        // sample bytes, because they encode the identical picture.
        for (label, k, data) in ccitt_variants() {
            let dict = ccitt_dict(vec![
                (b"K", Object::Integer(k)),
                (b"Columns", Object::Integer(16)),
                (b"Rows", Object::Integer(4)),
            ]);
            let img = decode_fixture(data, &dict).unwrap();
            assert_eq!(img.codec, Some(Codec::Ccitt), "{label}");
            assert_eq!((img.width, img.height), (16, 4), "{label}");
            assert_eq!(img.components, 1, "{label}");
            assert_eq!(img.bits_per_component, 1, "{label}");
            assert_eq!(img.color_model, CodecColorModel::Bilevel, "{label}");
            assert_eq!(
                img.samples,
                fixtures_bilevel::BILEVEL_16X4_SAMPLES,
                "{label}: samples must be PDF convention, 0 = black"
            );
            assert!(!img.notes.geometry_mismatch, "{label}");
        }
    }

    #[test]
    fn ccitt_black_is_1_inverts_every_sample() {
        // Table 11: `/BlackIs1 true` means "1 bits shall be interpreted
        // as black pixels … the reverse of the normal PDF convention",
        // so the two decodes must be exact bitwise complements. This is
        // the polarity trap pinned from both sides: a decoder that wired
        // `invert_black` to `!BlackIs1` passes neither half.
        for (label, k, data) in ccitt_variants() {
            let dict = ccitt_dict(vec![
                (b"K", Object::Integer(k)),
                (b"Columns", Object::Integer(16)),
                (b"Rows", Object::Integer(4)),
                (b"BlackIs1", Object::Boolean(true)),
            ]);
            let img = decode_fixture(data, &dict).unwrap();
            assert_eq!(
                img.samples,
                fixtures_bilevel::BILEVEL_16X4_INK,
                "{label}: BlackIs1 true must emit 1 for black"
            );
            let complement: Vec<u8> = fixtures_bilevel::BILEVEL_16X4_SAMPLES
                .iter()
                .map(|b| !b)
                .collect();
            assert_eq!(img.samples, complement, "{label}");
        }
    }

    #[test]
    fn ccitt_black_is_1_false_is_the_same_as_absent() {
        // The default is `false`, so stating it must change nothing.
        let with = ccitt_dict(vec![
            (b"K", Object::Integer(-1)),
            (b"Columns", Object::Integer(16)),
            (b"Rows", Object::Integer(4)),
            (b"BlackIs1", Object::Boolean(false)),
        ]);
        let without = ccitt_dict(vec![
            (b"K", Object::Integer(-1)),
            (b"Columns", Object::Integer(16)),
            (b"Rows", Object::Integer(4)),
        ]);
        let a = decode_fixture(fixtures_bilevel::CCITT_G4_16X4, &with).unwrap();
        let b = decode_fixture(fixtures_bilevel::CCITT_G4_16X4, &without).unwrap();
        assert_eq!(a.samples, b.samples);
    }

    #[test]
    fn ccitt_positive_k_values_are_indistinguishable() {
        // Table 11: the filter "shall not distinguish between different
        // positive K values". 1, 4 and 40 must decode identically.
        let decode_with_k = |k: i64| {
            let dict = ccitt_dict(vec![
                (b"K", Object::Integer(k)),
                (b"Columns", Object::Integer(16)),
                (b"Rows", Object::Integer(4)),
            ]);
            decode_fixture(fixtures_bilevel::CCITT_G3_2D_16X4, &dict)
                .unwrap()
                .samples
        };
        assert_eq!(decode_with_k(1), decode_with_k(4));
        assert_eq!(decode_with_k(4), decode_with_k(40));
        assert_eq!(decode_with_k(4), fixtures_bilevel::BILEVEL_16X4_SAMPLES);
    }

    #[test]
    fn ccitt_rows_zero_falls_back_to_the_dictionary_height() {
        // Table 11: "If the value is 0 or absent, the image's height is
        // not predetermined". `hayro-ccitt` has no unknown-height mode —
        // passing 0 would decode ZERO rows — so the adapter must supply
        // the dictionary's `/Height` (decision 005 §1.2). Tested three
        // ways: absent, explicitly 0, and via the dictionary alone.
        for parms in [
            vec![
                (&b"K"[..], Object::Integer(-1)),
                (b"Columns", Object::Integer(16)),
            ],
            vec![
                (&b"K"[..], Object::Integer(-1)),
                (b"Columns", Object::Integer(16)),
                (b"Rows", Object::Integer(0)),
            ],
        ] {
            let img = decode_fixture(fixtures_bilevel::CCITT_G4_16X4, &ccitt_dict(parms)).unwrap();
            assert_eq!(img.height, 4, "the /Height fallback must supply the bound");
            assert_eq!(img.samples, fixtures_bilevel::BILEVEL_16X4_SAMPLES);
        }
    }

    #[test]
    fn ccitt_end_of_line_is_advisory_not_a_gate() {
        // Table 11: "The CCITTFaxDecode filter shall ALWAYS accept
        // end-of-line bit patterns." libtiff writes EOLs into the Group
        // 3 fixtures, so the same bytes must decode identically whether
        // the flag is absent, true, or false.
        let decode_with = |eol: Option<bool>| {
            let mut parms = vec![
                (&b"K"[..], Object::Integer(0)),
                (b"Columns", Object::Integer(16)),
                (b"Rows", Object::Integer(4)),
            ];
            if let Some(v) = eol {
                parms.push((b"EndOfLine", Object::Boolean(v)));
            }
            decode_fixture(fixtures_bilevel::CCITT_G3_1D_16X4, &ccitt_dict(parms))
                .unwrap()
                .samples
        };
        assert_eq!(decode_with(None), fixtures_bilevel::BILEVEL_16X4_SAMPLES);
        assert_eq!(decode_with(Some(true)), decode_with(None));
        assert_eq!(decode_with(Some(false)), decode_with(None));
    }

    #[test]
    fn ccitt_columns_defaults_to_1728_and_the_default_is_load_bearing() {
        // The verified Table 11 default is the ITU-T A4 scan width, NOT
        // the image's `/Width`. A 16-column stream decoded as 1728
        // columns cannot succeed — which is exactly the observable
        // consequence that proves the default is applied rather than
        // quietly replaced by `/Width`.
        let dict = dict_with(vec![
            (b"Filter", name(b"CCITTFaxDecode")),
            (b"Width", Object::Integer(16)),
            (b"Height", Object::Integer(4)),
            (b"DecodeParms", Object::Dict(dict_with(vec![]))),
        ]);
        assert!(matches!(
            decode_fixture(fixtures_bilevel::CCITT_G4_16X4, &dict),
            Err(ImageCodecError::Corrupt { .. })
        ));
    }

    #[test]
    fn ccitt_geometry_disagreement_is_counted() {
        // `/Columns` and `/Width` disagreeing is the most common
        // real-world fax defect, and it shears rather than fails.
        let mut dict = ccitt_dict(vec![
            (b"K", Object::Integer(-1)),
            (b"Columns", Object::Integer(16)),
            (b"Rows", Object::Integer(4)),
        ]);
        dict.insert(Name::from(b"Width"), Object::Integer(24));
        let img = decode_fixture(fixtures_bilevel::CCITT_G4_16X4, &dict).unwrap();
        assert_eq!(img.width, 16, "the filter's /Columns governs the samples");
        assert!(img.notes.geometry_mismatch);
    }

    #[test]
    fn ccitt_refuses_a_geometry_past_the_pdfcer_ceiling() {
        // Rule R25: the ceiling is pdfcer's, and it is checked on the
        // DECLARED geometry before a byte is decoded.
        let dict = ccitt_dict(vec![
            (b"K", Object::Integer(-1)),
            (b"Columns", Object::Integer(65535)),
            (b"Rows", Object::Integer(65535)),
        ]);
        assert_eq!(
            decode_fixture(fixtures_bilevel::CCITT_G4_16X4, &dict).unwrap_err(),
            ImageCodecError::TooLarge
        );
        // A width past MAX_IMAGE_DIMENSION is refused on its own.
        let wide = ccitt_dict(vec![
            (b"K", Object::Integer(-1)),
            (b"Columns", Object::Integer(70000)),
            (b"Rows", Object::Integer(1)),
        ]);
        assert_eq!(
            decode_fixture(fixtures_bilevel::CCITT_G4_16X4, &wide).unwrap_err(),
            ImageCodecError::TooLarge
        );
    }

    #[test]
    fn ccitt_nonsensical_columns_is_refused_not_defaulted() {
        // A zero or negative `/Columns` is malformed. Silently falling
        // back to 1728 would decode a *different image* from the one the
        // file describes, which is the "plausible garbage" the
        // fail-clean contract forbids.
        for bad in [Object::Integer(0), Object::Integer(-16)] {
            let dict = ccitt_dict(vec![(b"K", Object::Integer(-1)), (b"Columns", bad)]);
            assert!(matches!(
                decode_fixture(fixtures_bilevel::CCITT_G4_16X4, &dict),
                Err(ImageCodecError::Corrupt { .. })
            ));
        }
    }

    #[test]
    fn ccitt_corrupt_data_errs_and_never_returns_a_short_page() {
        // Fail-clean at the codec layer. `hayro-ccitt` documents that
        // some rows may already have been written when it errors; pdfcer
        // deliberately discards them rather than hand back a silently
        // truncated fax page.
        let dict = ccitt_dict(vec![
            (b"K", Object::Integer(-1)),
            (b"Columns", Object::Integer(16)),
            (b"Rows", Object::Integer(4)),
        ]);
        let mut truncated = fixtures_bilevel::CCITT_G4_16X4.to_vec();
        truncated.truncate(3);
        assert!(matches!(
            decode_fixture(&truncated, &dict),
            Err(ImageCodecError::Corrupt { .. })
        ));
        assert!(matches!(
            decode_fixture(&[0x00; 32], &dict),
            Err(ImageCodecError::Corrupt { .. })
        ));
        // NOTE, deliberately recorded rather than asserted away: T.4/T.6
        // carry no checksum and no framing beyond EOL/EOFB, so plenty of
        // arbitrary bytes ARE valid code sequences. `[0xFF; 32]` is a run
        // of V(0) vertical-mode codes and decodes to a real (if
        // meaningless) picture. "Fail-clean" means pdfcer never *invents*
        // samples; it cannot mean pdfcer detects a codec's own
        // undetectable garbage.
        let _ = decode_fixture(&[0xFF; 32], &dict);
    }

    #[test]
    fn ccf_is_a_legal_inline_abbreviation() {
        // Table 94 gives `CCF` for CCITTFaxDecode; unlike JBIG2 and JPX,
        // it is permitted inline.
        let doc = empty_document();
        let mut dict = ccitt_dict(vec![
            (b"K", Object::Integer(-1)),
            (b"Columns", Object::Integer(16)),
            (b"Rows", Object::Integer(4)),
        ]);
        dict.insert(Name::from(b"Filter"), name(b"CCF"));
        let img = decode_image(&doc, &dict, fixtures_bilevel::CCITT_G4_16X4, true).unwrap();
        assert_eq!(img.codec, Some(Codec::Ccitt));
        assert_eq!(img.samples, fixtures_bilevel::BILEVEL_16X4_SAMPLES);
    }

    // -----------------------------------------------------------------
    // JBIG2Decode (§7.4.7, Table 12) — Pass 2.2
    // -----------------------------------------------------------------

    /// An image dictionary declaring `JBIG2Decode` over the fixture
    /// geometry, with the given `/DecodeParms` entries.
    fn jbig2_dict(parms: Vec<(&[u8], Object)>) -> Dict {
        dict_with(vec![
            (b"Filter", name(b"JBIG2Decode")),
            (b"Width", Object::Integer(16)),
            (b"Height", Object::Integer(4)),
            (b"BitsPerComponent", Object::Integer(1)),
            (b"DecodeParms", Object::Dict(dict_with(parms))),
        ])
    }

    /// A document whose object 3 is an unfiltered stream holding
    /// `globals` — the shape `/JBIG2Globals` points at.
    fn document_with_globals(globals: &[u8]) -> Document {
        let mut buf = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (num, body) in [
            (1u32, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>".to_owned()),
        ] {
            offsets.push(buf.len());
            buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        offsets.push(buf.len());
        buf.extend_from_slice(
            format!("3 0 obj\n<< /Length {} >>\nstream\n", globals.len()).as_bytes(),
        );
        buf.extend_from_slice(globals);
        buf.extend_from_slice(b"\nendstream\nendobj\n");
        let xref_at = buf.len();
        buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f\r\n");
        for off in offsets {
            buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
        }
        buf.extend_from_slice(
            format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
        );
        Document::from_bytes(buf).unwrap()
    }

    #[test]
    fn jbig2_decodes_to_the_same_samples_as_ccitt() {
        // The cross-codec assertion. T.88 §6.2.6 fixes MMR black at
        // bitmap value 1; PDF's convention is the opposite and JBIG2 has
        // no polarity parameter to say so, so the adapter's inversion is
        // unconditional. Two completely different routes, one answer.
        let dict = jbig2_dict(vec![]);
        let img = decode_fixture(fixtures_bilevel::JBIG2_MMR_16X4, &dict).unwrap();
        assert_eq!(img.codec, Some(Codec::Jbig2));
        assert_eq!((img.width, img.height), (16, 4));
        assert_eq!(img.components, 1);
        assert_eq!(img.bits_per_component, 1);
        assert_eq!(img.color_model, CodecColorModel::Bilevel);
        assert_eq!(img.samples, fixtures_bilevel::BILEVEL_16X4_SAMPLES);
        assert!(!img.notes.geometry_mismatch);
    }

    #[test]
    fn jbig2_globals_are_resolved_through_the_document() {
        // Table 12's `/JBIG2Globals`: a stream REFERENCE, which is one
        // of the two mechanical reasons the codec layer takes a
        // `&Document` at all (decision 005 §1.2). Split the very same
        // stream in two — page information in the globals, region in the
        // image — and it must still decode to the same picture.
        let doc = document_with_globals(fixtures_bilevel::JBIG2_MMR_16X4_GLOBALS);
        let dict = jbig2_dict(vec![(
            b"JBIG2Globals",
            Object::Reference(crate::object::ObjId::new(3, 0)),
        )]);
        let img = decode_image(&doc, &dict, fixtures_bilevel::JBIG2_MMR_16X4_PAGE, false).unwrap();
        assert_eq!(img.samples, fixtures_bilevel::BILEVEL_16X4_SAMPLES);
    }

    #[test]
    fn jbig2_without_its_globals_fails_cleanly() {
        // The other half of the pair: the region segment alone carries
        // no page information, so a decoder that silently ignored
        // `/JBIG2Globals` would fail here rather than draw something
        // wrong. That is what makes the previous test meaningful.
        let dict = jbig2_dict(vec![]);
        assert!(matches!(
            decode_fixture(fixtures_bilevel::JBIG2_MMR_16X4_PAGE, &dict),
            Err(ImageCodecError::Corrupt { .. })
        ));
    }

    #[test]
    fn jbig2_globals_that_are_not_a_stream_are_treated_as_absent() {
        // A stray non-stream `/JBIG2Globals` carries no segments, so it
        // is ignored rather than turned into a blank page. The image
        // that needs no globals still decodes.
        let dict = jbig2_dict(vec![(b"JBIG2Globals", Object::Integer(7))]);
        let img = decode_fixture(fixtures_bilevel::JBIG2_MMR_16X4, &dict).unwrap();
        assert_eq!(img.samples, fixtures_bilevel::BILEVEL_16X4_SAMPLES);
    }

    #[test]
    fn jbig2_geometry_comes_from_the_page_information_segment() {
        // Unlike CCITT, JBIG2 carries its own geometry, so a dictionary
        // that disagrees is Table 89's "entries inconsistent with each
        // other" — counted, never acted on.
        let mut dict = jbig2_dict(vec![]);
        dict.insert(Name::from(b"Height"), Object::Integer(9));
        let img = decode_fixture(fixtures_bilevel::JBIG2_MMR_16X4, &dict).unwrap();
        assert_eq!(img.height, 4, "the page information segment governs");
        assert!(img.notes.geometry_mismatch);
    }

    #[test]
    fn jbig2_corrupt_segments_err() {
        let dict = jbig2_dict(vec![]);
        for bad in [&b""[..], b"not jbig2 at all", &[0xFF; 40]] {
            assert!(
                matches!(
                    decode_fixture(bad, &dict),
                    Err(ImageCodecError::Corrupt { .. })
                ),
                "{bad:?}"
            );
        }
        let mut truncated = fixtures_bilevel::JBIG2_MMR_16X4.to_vec();
        truncated.truncate(truncated.len() - 4);
        assert!(matches!(
            decode_fixture(&truncated, &dict),
            Err(ImageCodecError::Corrupt { .. })
        ));
    }

    #[test]
    fn inline_images_reject_jbig2_and_jpx_but_allow_dct() {
        // §7.4.7 / §8.9.7. Checked before any bytes are touched.
        let doc = empty_document();
        for (spelling, codec) in [
            (&b"JBIG2Decode"[..], Codec::Jbig2),
            (b"JPXDecode", Codec::Jpx),
        ] {
            let dict = dict_with(vec![(b"Filter", name(spelling))]);
            assert_eq!(
                decode_image(&doc, &dict, b"anything", true).unwrap_err(),
                ImageCodecError::NotAllowedInline { codec }
            );
        }
        // `DCT` is a legal inline abbreviation (Table 94).
        let mut inline = dct_dict(2, 2);
        inline.insert(Name::from(b"Filter"), name(b"DCT"));
        let img = decode_image(&doc, &inline, fixtures::RGB_2X2, true).unwrap();
        assert_eq!(img.codec, Some(Codec::Dct));
    }

    #[test]
    fn a_codec_that_is_not_the_last_filter_is_refused() {
        // Nothing can be chained after a codec: it consumes a
        // codestream and produces samples, not bytes.
        let dict = dict_with(vec![(
            b"Filter",
            Object::Array(vec![name(b"DCTDecode"), name(b"FlateDecode")]),
        )]);
        assert_eq!(
            decode_fixture(fixtures::RGB_2X2, &dict).unwrap_err(),
            ImageCodecError::CodecNotTerminal
        );
    }

    #[test]
    fn lzw_framing_anomalies_survive_into_the_codec_notes() {
        // An LZW-armoured image whose stream lacks a ClearCode still
        // decodes, and the anomaly reaches the renderer's diagnostics
        // through `CodedImage::notes` rather than being swallowed at the
        // filter boundary.
        let dict = dict_with(vec![
            (b"Filter", name(b"LZWDecode")),
            (b"Width", Object::Integer(1)),
            (b"Height", Object::Integer(1)),
        ]);
        let img = decode_fixture(&[0x20, 0xC0, 0x40], &dict).unwrap();
        assert_eq!(img.samples, b"A");
        assert_eq!(img.notes.lzw_framing_anomalies, 1);
    }

    #[test]
    fn codec_parms_follows_table_5_positions() {
        let parms = dict_with(vec![(b"ColorTransform", Object::Integer(0))]);
        // Single filter → a lone dictionary applies to it.
        let single = dict_with(vec![
            (b"Filter", name(b"DCTDecode")),
            (b"DecodeParms", Object::Dict(parms.clone())),
        ]);
        assert!(codec_parms(&single, 1).is_some());
        // Two filters → the array position of the LAST one.
        let chained = dict_with(vec![
            (
                b"Filter",
                Object::Array(vec![name(b"ASCII85Decode"), name(b"DCTDecode")]),
            ),
            (
                b"DecodeParms",
                Object::Array(vec![Object::Null, Object::Dict(parms)]),
            ),
        ]);
        assert!(codec_parms(&chained, 2).is_some());
        assert!(codec_parms(&chained, 1).is_none());
    }

    // -----------------------------------------------------------------
    // A JP2 palette and a PDF `/Indexed` space are the SAME lookup
    // -----------------------------------------------------------------

    /// The one entry both tables would hold in a file built to catch the
    /// double-resolution bug. Arbitrary, but distinctive: no component is 0
    /// or 255, so an off-by-one or a dropped channel is visible in the
    /// assertion rather than coincidentally right.
    /// What palette entry `i` resolves to in
    /// [`fixtures_jpx::jpx_gray_8_jp2_with_palette`]. The constant `13` is a
    /// fingerprint: a resolved sample whose blue is not 13 did not come from
    /// the palette.
    #[cfg(feature = "jpx")]
    fn palette_entry(i: u8) -> [u8; 3] {
        [i, 255 - i, 13]
    }

    /// WITHOUT a PDF `/Indexed` space, the palette is left to the codec and
    /// the disclosure flag stays clear.
    ///
    /// # What this asserts, and what it deliberately does not
    ///
    /// It pins the **decision**, not the decoder: that the flag follows the
    /// dictionary, and that a palette-bearing file still decodes. It does
    /// NOT assert a resolved component count.
    ///
    /// ★ That restraint is deliberate and was arrived at by measurement. The
    /// first draft asserted "1 channel in, 3 out" — reasoned from the `cmap`
    /// box's three entries — and this fixture returns **1**. The behaviour is
    /// pre-existing and untouched by the change under test, so asserting a
    /// number derived from reasoning rather than from a measured run would
    /// pin an expectation, not a fact, and the next person would have to
    /// discover which it was.
    ///
    /// Whether a synthetic `pclr`/`cmap` pair drives the decoder's expansion
    /// identically to a real encoder's output is **unestablished**, and it is
    /// not what this Pass changed. The paired test below carries the claim
    /// that matters.
    #[cfg(feature = "jpx")]
    #[test]
    fn a_jp2_palette_is_left_to_the_codec_when_the_pdf_offers_no_indexed_space() {
        let jp2 = fixtures_jpx::jpx_gray_8_jp2_with_palette();
        let img = decode_fixture(&jp2, &jpx_gray_dict()).unwrap();

        assert!(
            !img.notes.jpx_palette_left_to_pdf,
            "with no /Indexed space in the dictionary, the codec keeps the palette"
        );
        assert!(!img.samples.is_empty(), "the file still decodes");
    }

    /// ★★★ WITH a PDF `/Indexed` space, the palette is LEFT ALONE and the
    /// samples stay indices — because applying both tables is the only way
    /// to be wrong.
    ///
    /// ISO 32000-1 §8.9 Table 89: with `JPXDecode`, if `/ColorSpace` is
    /// present then "colour space specifications in the JPEG2000 data shall
    /// be ignored". A `pclr`/`cmap` pair is such a specification.
    ///
    /// The failure this pins is invisible in any counter. Resolve here and
    /// the samples become `114, 247, 13`; the renderer then reads component
    /// 0 as an index, asks a one-entry table for entry **114**, finds
    /// nothing, and paints **black**. Nothing reports an error — the picture
    /// is simply the wrong colour.
    #[cfg(feature = "jpx")]
    #[test]
    fn a_jp2_palette_is_left_to_the_pdf_when_the_dictionary_carries_indexed() {
        let jp2 = fixtures_jpx::jpx_gray_8_jp2_with_palette();
        let mut dict = jpx_gray_dict();
        // `[/Indexed /DeviceRGB 0 <lookup>]` — the shape a real file uses,
        // with hival 0 and a table holding the SAME bytes as the JP2's.
        dict.insert(
            Name::from(b"ColorSpace"),
            Object::Array(vec![
                Object::Name(Name::from(b"Indexed")),
                Object::Name(Name::from(b"DeviceRGB")),
                Object::Integer(0),
                Object::String(palette_entry(0).to_vec()),
            ]),
        );

        let img = decode_fixture(&jp2, &dict).unwrap();

        assert_eq!(
            img.components, 1,
            "the samples must remain a single INDEX channel"
        );
        assert!(
            img.notes.jpx_palette_left_to_pdf,
            "and the decision must be disclosed -- it is invisible in the output"
        );
        assert_eq!(
            &img.samples[..8],
            &fixtures_jpx::JPX_GRAY_8_SAMPLES[..8],
            "indices pass through untouched, identical to the palette-free fixture"
        );
        let first = fixtures_jpx::JPX_GRAY_8_SAMPLES[0];
        assert_ne!(
            &img.samples[..3],
            &palette_entry(first),
            "if these matched, the palette had been applied after all and the \
             renderer would apply it a SECOND time"
        );
    }

    /// The decision point itself, as a truth table.
    ///
    /// Guards the one case the codec deliberately cannot see: a
    /// `/ColorSpace` written as a NAME resolves through the page's resource
    /// dictionary, which this layer does not have. It answers `false` and
    /// keeps the old behaviour rather than guessing — a named limit, not an
    /// oversight.
    #[cfg(feature = "jpx")]
    #[test]
    fn only_a_visible_indexed_array_suppresses_palette_resolution() {
        let doc = empty_document();
        let with = |cs: Option<Object>| {
            let mut d = Dict::new();
            if let Some(cs) = cs {
                d.insert(Name::from(b"ColorSpace"), cs);
            }
            super::jpx::dict_colorspace_is_indexed(&doc.view(), &d)
        };
        let arr = |first: &[u8]| {
            Object::Array(vec![
                Object::Name(Name::from(first)),
                Object::Name(Name::from(b"DeviceRGB")),
            ])
        };

        assert!(with(Some(arr(b"Indexed"))), "the array form is decidable");
        assert!(!with(Some(arr(b"ICCBased"))));
        assert!(!with(Some(Object::Name(Name::from(b"DeviceRGB")))));
        assert!(
            !with(Some(Object::Name(Name::from(b"CS0")))),
            "a NAMED resource is not visible from the codec layer; answering \
             `false` keeps the pre-existing behaviour instead of guessing"
        );
        assert!(!with(None), "no /ColorSpace at all");
    }
}

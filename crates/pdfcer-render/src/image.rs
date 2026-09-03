//! # Image XObjects and inline images → RGBA pixmaps (ISO 32000-1 §8.9)
//!
//! Turns a PDF *sampled image* — an image XObject (`/Subtype /Image`)
//! or an inline image (`BI`/`ID`/`EI`) — into a [`tiny_skia::Pixmap`]
//! the interpreter can paint through the CTM. Spec sources:
//! `iso32000__s__8.9.md` (Table 89, image space, sample packing,
//! stencil masks), `iso32000__s__8.9.5.2.md` (Table 90 `Decode`
//! defaults, the linear transform, the `[1 0]` inversion, image-mask
//! polarity), `iso32000__s__8.9.7.md` (inline-image abbreviations),
//! `color__indexed.md` (§8.6.6.3 palettes), `color__iccbased.md`
//! (the `N`-component fallback) in the PDF-spec RAG.
//!
//! ## This module does NOT place the image
//!
//! Placement is entirely the CTM's job (§8.9.4): "the unit square of
//! user space… corresponds to the boundary of the image in image
//! space." This module only produces `Width × Height` RGBA texels with
//! **row 0 at the top**, exactly as §8.9.3 orders the samples. The
//! y-flip that §8.9.4's implicit matrix `[1/w 0 0 −1/h 0 1]` describes
//! is applied by the caller ([`crate::interpret`]) when it builds the
//! pattern transform. Keeping the flip out of here means the pixmap is
//! in the same orientation a PNG would be, which is what makes the
//! pixel-level tests readable.
//!
//! ## The decode pipeline, in the order §8.9.5.2 mandates
//!
//! ```text
//! raw stream bytes
//!   → /Filter chain + terminal codec      pdfcer_core::image_codec
//!   → unpack to BitsPerComponent integers §8.9.3 (rows byte-padded)
//!   → Decode transform                    §8.9.5.2 (linear, may invert)
//!   → colour-space conversion             §8.6 / §8.6.6.3
//!   → RGBA texel
//! ```
//!
//! Getting this order wrong is the classic image bug: applying `Decode`
//! after colour conversion silently breaks `Indexed` (whose `Decode`
//! output *is* a palette index, not a colour) and every inverted
//! stencil mask.
//!
//! ## Where the samples come from (decision 005 R23/R26)
//!
//! The first stage is [`pdfcer_core::image_codec::decode_image`], not
//! `filters::decode_stream`. It runs the byte-stream *prefix* of the
//! `/Filter` chain and then dispatches the single terminal image codec
//! (`DCTDecode`, `CCITTFaxDecode`, `JBIG2Decode`, `JPXDecode`), handing
//! back a [`CodedImage`] — samples **plus** the geometry and colour
//! model the codestream itself declares.
//!
//! Everything downstream of that first stage is unchanged and still
//! lives here: sample unpacking, `/Decode`, `/ColorSpace` resolution,
//! `Indexed` palettes, stencil masks, RGBA texels. **Core decodes and
//! models; render paints.** In particular the codec layer applies no
//! `/Decode` array and no "Adobe CMYK inversion" of its own (rules R26
//! and R29 — decision 006 settled that no such inversion exists in any
//! shipping PDF engine), so a CMYK JPEG's polarity is settled here, by
//! `/Decode`, exactly as §8.9.5.2 says it should be. The signed-slope
//! ramp below is therefore load-bearing: `/Decode [1 0 …]` IS the
//! sanctioned inversion mechanism, and it must survive any refactor.
//!
//! ## When the codestream and the dictionary disagree
//!
//! A JPEG whose SOF geometry differs from `/Width`//`/Height` is a
//! producer bug. pdfcer splits the difference along the only seam that
//! neither shears the picture nor moves it:
//!
//! - **the dictionary wins for placement** — the pixmap is `/Width` ×
//!   `/Height`, because §8.9.4 maps the image onto the unit square of
//!   user space regardless of how many samples it turns out to contain;
//! - **the codestream wins for sample reading** — the row stride comes
//!   from the codec's own width, component count and bit depth, because
//!   that is the physical layout of the bytes in hand.
//!
//! The divergence is counted in [`ImageNotes::codec_geometry_mismatch`]
//! and surfaced, never silently absorbed.
//!
//! ## `JPXDecode` inverts three of Table 89's rules, and only three
//!
//! `JPXDecode` is the one filter for which the image dictionary is not
//! simply authoritative, and the three exceptions are exact. They are
//! implemented in [`decode_sampled`], each at the point where the
//! ordinary rule would otherwise apply:
//!
//! | Entry | Ordinary image | `JPXDecode` |
//! |---|---|---|
//! | `/ColorSpace` | **Required**; missing is malformed. | **Optional.** Present → the dictionary still wins and the codestream's colour specifications "shall be ignored". Absent → [`codestream_space`] supplies it from the codec's declared colour model, per §7.4.9's fallback ladder. |
//! | `/BitsPerComponent` | **Required**, one of 1/2/4/8/16. | **"Optional and shall be ignored if present."** The dictionary is not consulted at all; the codec's delivered depth is used. Honouring a stated value is not merely redundant, it is wrong. |
//! | `/Decode` | Applied (§8.9.5.2). | **Ignored** — "except in the case where the image is treated as a mask; that is, when `ImageMask` is true", which is the [`decode_stencil`] branch this function never reaches. |
//!
//! The trap in the *other* direction is worth naming because it looks
//! like the same rule: "the codestream is authoritative for JPX" is
//! **false** as a blanket statement. A present `/ColorSpace` wins. Only
//! where the dictionary is silent (colour) or explicitly disqualified
//! (bit depth, `Decode`) does the codestream take over. Getting that
//! backwards mis-colours precisely the files whose producer bothered to
//! tag them.
//!
//! `/Width` and `/Height` are **not** on that list: §7.4.9 requires them
//! to "match" the codestream but supplies no conflict-resolution rule,
//! so the ordinary dictionary-for-placement / codestream-for-stride
//! split above continues to govern them, with the divergence counted.
//!
//! ## Stencil masks are a separate path on purpose
//!
//! An image with `/ImageMask true` (§8.9.6.2) carries **no colour at
//! all** — its 1-bit samples say only "mark the page with the current
//! non-stroking colour here" or "leave the previous contents alone."
//! `Decode` is a polarity switch for it, not a colour transform, and
//! the default `[0 1]` means **0 = ink** (the opposite of the usual
//! bitmap intuition). Trying to unify this with the ordinary path via a
//! synthetic `DeviceGray` space gets both the polarity and the
//! transparency wrong, so [`decode`] branches early and never mixes
//! them.
//!
//! ## Honesty (`fuzzy, never sneaky`)
//!
//! Nothing here approximates. An image whose data needs a filter pdfcer
//! has not implemented ([`ImageError::UnsupportedFilter`]) or a colour
//! space out of this slice's scope
//! ([`ImageError::UnsupportedColorSpace`]) is **not drawn at all** and
//! is counted by the caller; it is never substituted with a grey box or
//! a guessed colour. Softer divergences that still produce a
//! *recognizable* image — a `/SMask` this slice ignores, a truncated
//! sample array, a palette index past the end of a short lookup table —
//! are drawn and reported through [`ImageNotes`].
//!
//! ## Resource ceiling (ARCHITECTURE.md §10.1 — pdfcer policy)
//!
//! `Width` and `Height` are attacker-controlled integers, and the RGBA
//! buffer is `4 × W × H` bytes, so the product is checked **before any
//! allocation or decode** against [`MAX_IMAGE_PIXELS`].

// decision 018: read paths take a `DocumentView` (graph + byte source), so
// the same code renders a loaded file or an editing session's unsaved state.
use pdfcer_core::filters::{self, FilterError};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::image_codec::{self, Codec, CodecColorModel, CodedImage, ImageCodecError};
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::settings::CmykIntent;
use pdfcer_core::view::DocumentView;
use tiny_skia::{Pixmap, PremultipliedColorU8};

use crate::color::ColorDiagnostics;
use crate::font::RenderPolicy;
use crate::gstate::Rgb;
use crate::mask::{self, AlphaPlane};

/// Maximum `Width × Height` accepted for a single image (pdfcer policy,
/// ARCHITECTURE.md §10.1).
///
/// Re-exported from [`pdfcer_core::image_codec`] rather than restated,
/// so the rasterizer's ceiling and the codec layer's ceiling are the
/// same number by construction. Two independently-maintained copies of
/// a guard is how a guard quietly stops guarding.
pub use pdfcer_core::image_codec::MAX_IMAGE_PIXELS;

/// Where an image came from, which decides §8.9.7's stricter rules.
///
/// An inline image may not use `JBIG2Decode` (§7.4.7 states it
/// outright) or `JPXDecode`; `DCT` and `CCF` *are* legal inline filter
/// abbreviations (Table 94). Passing this in rather than sniffing it
/// keeps the rule where the spec puts it — on the *construct*, not on
/// the data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageOrigin {
    /// An image XObject reached through `Do` (§8.8).
    XObject,
    /// An inline image (`BI`/`ID`/`EI`, §8.9.7).
    Inline,
}

/// Guard on `Indexed` colour-space nesting while resolving a
/// `/ColorSpace` entry.
///
/// A colour space can legitimately nest two deep (`Indexed` over
/// `ICCBased`), and a named resource can add one hop per lookup. A
/// self-referential `/ColorSpace << /CS0 [/Indexed /CS0 …] >>` would
/// otherwise recurse forever (ARCHITECTURE.md §10.1's cycle rule).
const MAX_COLORSPACE_DEPTH: usize = 8;

/// Why an image could not be turned into pixels at all.
///
/// Every variant means **nothing was drawn**. The caller counts these
/// in `Diagnostics::images_unsupported` and carries on with the rest of
/// the page — an image pdfcer cannot decode is a fidelity shortfall, not
/// a reason to abandon a page a reader could otherwise show.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ImageError {
    /// The sample data needs a filter this build does not implement, or
    /// its data is corrupt. The payload names it.
    #[error("image data could not be decoded: {0}")]
    UnsupportedFilter(String),
    /// The image uses a **codec** this build does not implement
    /// (`CCITTFaxDecode`, `JBIG2Decode`, `JPXDecode` until Passes 2.2
    /// and 2.3 land), or one that §8.9.7 forbids in an inline image.
    /// Separate from [`ImageError::UnsupportedFilter`] because "which
    /// codec do I need?" has a specific, actionable answer.
    #[error("{0}")]
    CodecUnsupported(String),
    /// The codec is implemented but a specific **sub-feature** of it is
    /// not — arithmetic-coded JPEG, 12-bit JPEG, an Adobe transform
    /// byte outside 0–2. The payload is a stable diagnostic key such as
    /// `"DCT/arithmetic"` so occurrences can be counted **by name**
    /// (decision 005 rule R27), never rolled into a generic "decode
    /// failed."
    #[error("unsupported codec feature: {0}")]
    CodecFeature(&'static str),
    /// The `/ColorSpace` is outside this slice's scope (`Lab`,
    /// `Separation`, `DeviceN`, `Pattern`, or an unresolvable name).
    /// The payload names it.
    #[error("image colour space {0} is not supported")]
    UnsupportedColorSpace(String),
    /// Table 89's "entries inconsistent with each other" rule: a
    /// missing/zero `Width`/`Height`, a `BitsPerComponent` outside
    /// {1,2,4,8,16}, an image mask with a bit depth other than 1, and
    /// so on. The payload says which.
    #[error("malformed image dictionary: {0}")]
    Malformed(&'static str),
    /// `Width × Height` exceeds [`MAX_IMAGE_PIXELS`] (pdfcer guard).
    #[error("image exceeds MAX_IMAGE_PIXELS ({MAX_IMAGE_PIXELS} pixels)")]
    TooLarge,
}

/// Which of §8.9.6.1's transparency mechanisms supplied an image's alpha.
///
/// Exactly one can be in force per image — `/SMask` and `/Mask` are
/// separate entries and `/Mask` is either a stream or an array, never
/// both — so this is an `Option<MaskApplied>` on [`ImageNotes`] rather
/// than a set of independent flags. The precedence when a document names
/// more than one is documented on [`decode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MaskApplied {
    /// `/SMask` — a separate greyscale image, one continuous alpha per
    /// sample (§8.9.5 Table 89, §11.6.5.3). The mechanism a transparent
    /// PNG's alpha channel becomes.
    SoftMask,
    /// `/Mask` as a stream — a separate 1-bit stencil selecting which
    /// base texels paint (§8.9.6.3). Binary, never partial.
    Stencil,
    /// `/Mask` as an array — ranges of the base image's own
    /// pre-`/Decode` samples that vanish (§8.9.6.4). The mechanism a
    /// single-transparent-colour PNG (`tRNS` on a truecolour image)
    /// becomes.
    ColourKey,
    /// A JPX codestream's own opacity channel, switched on by
    /// `/SMaskInData 1` (Table 89). Not a dictionary entry at all — the
    /// alpha travels inside the image's own bytes.
    EmbeddedAlpha,
}

impl MaskApplied {
    /// A stable, greppable name for the diagnostics surfaces.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::SoftMask => "smask",
            Self::Stencil => "stencil",
            Self::ColourKey => "colour-key",
            Self::EmbeddedAlpha => "jpx-embedded-alpha",
        }
    }
}

/// Divergences that did **not** stop the image from being drawn.
///
/// Distinct from [`ImageError`] because the operator's question is
/// different: an error means "this image is missing from the page", a
/// note means "this image is on the page but is not exactly what the
/// document specifies."
// ★ `Copy` and `Eq` were DROPPED in `Pass 140.2`, when `color` was added.
//
// `ColorDiagnostics` carries a dedup-and-capped `Vec<String>` of notes, so it
// is neither. Keeping the derives would have meant either leaving the field
// off this struct — the arrangement that made an image's broken tint transform
// silent in the first place — or parking it somewhere it does not belong. A
// diagnostics bag that grows is the normal case; a `Copy` bound that decides
// what may be diagnosed is the tail wagging the dog.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ImageNotes {
    /// The sample array was shorter than `stride × Height`; the missing
    /// samples were read as 0. (§8.9.3 gives an exact length; a short
    /// stream is malformed, but refusing to draw the 90% that *is*
    /// present helps nobody.)
    pub truncated: bool,
    /// Which transparency mechanism produced this image's alpha, or
    /// `None` for an image that is opaque because the document says so.
    ///
    /// Census, not shortfall: every variant means pdfcer **did** the
    /// work. The shortfall twin is
    /// [`mask_refused`](ImageNotes::mask_refused).
    pub mask_applied: Option<MaskApplied>,
    /// A `/SMask` or `/Mask` was present and could **not** be turned
    /// into alpha, so the base image was drawn **fully opaque** —
    /// visually wrong wherever the mask would have hidden something.
    /// The payload is [`crate::mask::MaskRefusal::key`], a stable name
    /// so occurrences can be counted **by reason** (rule R27).
    ///
    /// This is the residue of the pre-transparency build's blanket
    /// `mask_deferred`: that note fired for every masked image, because
    /// none were composited. It now fires only for the ones pdfcer
    /// genuinely could not handle.
    pub mask_refused: Option<&'static str>,
    /// The mask's pixel dimensions differed from the base image's, so
    /// its samples were point-sampled across the base's grid
    /// (§8.9.6.3: "need not have the same resolution … their boundaries
    /// on the page will coincide"). Conformant and common; recorded
    /// because a resampled mask cannot be pixel-exact and a parity
    /// investigation should not have to re-derive why.
    pub mask_resampled: bool,
    /// The `/SMask` carried `/Matte` (Table 146) and pdfcer **undid the
    /// preblend** per §11.6.5.3's `c = m + (c′ − m)/α`.
    ///
    /// Census, not shortfall — but recorded, because a `/Matte` image's
    /// partially-transparent samples are reconstructed by a division
    /// that amplifies quantisation error by `1/α`. A parity
    /// investigation that finds a `/Matte` image disagreeing with
    /// another engine in its near-transparent fringes should know that
    /// before spending an afternoon on it.
    pub matte_undone: bool,
    /// A `/Matte` was present and **not** undone, with the reason.
    ///
    /// The alpha is applied either way — that half is conformant
    /// regardless — so the picture's *shape* is right and only the
    /// colours in the partially-transparent regions stay shifted toward
    /// the matte colour. Reasons: `"matte/dimension-mismatch"` (Table
    /// 145 makes equal dimensions a `shall` when `/Matte` is present, so
    /// a mismatch means the file is wrong about one of the two, and
    /// dividing by a resampled α would use the wrong α for every
    /// sample), `"matte/indexed"` (spec ambiguity `SM-A4`: Table 146
    /// counts `n` from the parent's `/ColorSpace`, which for `Indexed`
    /// is **1**, while §11.6.5.3 requires the *colour table* values to
    /// be preblended, which needs the base space's `n` — the two rules
    /// contradict and pdfcer will not pick a side silently), and
    /// `"matte/length"` (the array is not `n` long).
    pub matte_not_undone: Option<&'static str>,
    /// At least one sample indexed past the end of a short `Indexed`
    /// lookup table and was painted black (`color__indexed.md`: real
    /// producers trim trailing unused palette entries).
    pub palette_out_of_range: bool,
    /// The image's colour space was `/Separation /None` or an all-`/None`
    /// `/DeviceN`, so **nothing was painted** — the image is fully
    /// transparent and the page shows through it.
    ///
    /// §8.6.6.4/.5: such a colorant "shall never be painted on the page".
    /// This is pdfcer OBEYING the standard, and it is recorded for R183's
    /// reason: a picture that is correctly absent is otherwise
    /// indistinguishable from one that failed to decode.
    ///
    /// ★ Measured 2026-08-17: **pdfium paints this BLACK.** pdfcer is
    /// deliberately right and the reference renderer is wrong, which is a
    /// finding rather than a failure — but it means any pixel-parity run
    /// containing a `/None` image will show a maximal divergence that is
    /// pdfcer's correctness, not its defect.
    pub colorant_none_suppressed: bool,
    /// The image was decoded through a `crate::color::ColorSpace` that
    /// pdfcer converts by its OWN colorimetry rather than by a colour
    /// management engine — `Lab`, `CalGray` and `CalRGB`, whose XYZ→sRGB
    /// step is documented as pdfcer's engineering choice (Bradford
    /// adaptation to D65, the sRGB matrix and transfer function, no
    /// rendering intent and no gamut mapping).
    ///
    /// Disclosed because it is precisely the kind of divergence that
    /// otherwise lands in a parity harness's *unexplained* bucket and
    /// costs somebody an afternoon: two engines can both be defensible
    /// here and still differ by tens of levels in the saturated corners.
    pub uncalibrated_colorimetry: Option<&'static str>,
    /// The `/Decode` array's length was not `2 × components`, so the
    /// Table 90 default was used instead (`iso32000__s__8.9.5.2.md`
    /// recommends this over truncating, which silently mis-tints).
    pub decode_array_ignored: bool,
    /// The codestream's own geometry disagreed with the image
    /// dictionary (`/Width`, `/Height`, `/BitsPerComponent`, or the
    /// component count implied by `/ColorSpace`). The image was still
    /// drawn — see the module docs for which side wins what — but one
    /// of the two is wrong about the file (decision 005 §6.4).
    pub codec_geometry_mismatch: bool,
    /// This was a 4-component DCT image in YCCK storage (effective
    /// transform 1/2) — the **benign census** half of decision 006
    /// §4.4's split. The mandated YCCK→CMYK inverse recovers true ink
    /// directly (TN #5116 §13.1) and carries no polarity ambiguity;
    /// verified pixel-identical to pdfium across the corpus. Volume,
    /// not shortfall — no warning attaches.
    pub dct_cmyk_image: bool,
    /// This was a 4-component DCT image with effective transform **0**
    /// and **no `/Decode`** — the one shape where the undocumented
    /// Photoshop inverted-storage convention could make it render as
    /// its own negative with nothing to disambiguate (decision 006
    /// rule **R30**). Reported, never repaired: the image was drawn
    /// from the raw samples, exactly as pdfium/pdf.js/MuPDF/Poppler
    /// draw it; pdfcer differs only in *saying so*.
    pub dct_cmyk_polarity_unverifiable: bool,
    /// This JPX image declares `/SMaskInData 2` — Table 89's "colour
    /// channels that have been **preblended with a background**" plus
    /// an opacity channel that would need a `Matte` entry to undo.
    ///
    /// Recognized and deferred, never approximated. The image *was*
    /// drawn, from the preblended colour channels exactly as stored —
    /// which is what it genuinely looks like composited over that
    /// backdrop, so the picture is right wherever it is opaque and
    /// shows the backdrop where it is not.
    ///
    /// **Still deferred after the transparency Pass, and for a reason
    /// that has nothing to do with clause 11.** §11.6.5.3's
    /// un-premultiply is now implemented ([`crate::mask::undo_matte`])
    /// and would apply here unchanged — but it needs the opacity channel
    /// and the matte colour, and neither is available: Table 89's
    /// `/SMaskInData 2` names a *premultiplied* opacity channel type
    /// that `hayro-jpeg2000` does not parse, so the codec layer leaves
    /// `CodedImage::embedded_alpha` as `None`, and a JPX codestream
    /// carries no `/Matte` (that entry lives on a soft-mask image
    /// dictionary, which this construct does not have). The blocker is a
    /// decoder gap, not a spec gap.
    ///
    /// Separate from [`mask_refused`](ImageNotes::mask_refused) because
    /// the divergence is different in kind: a refused mask means
    /// "correct colours, missing transparency", this means "the colours
    /// themselves have a backdrop mixed into them".
    pub jpx_smask_in_data_preblended: bool,
    /// LZW framing anomalies in the byte-stream part of the chain — a
    /// stream with no `ClearCode`, or one that ended with no
    /// `EndOfInformation`. Both recovered, both non-conformant.
    pub lzw_framing_anomalies: usize,
    /// ★★★ THE COLOUR-CONVERSION DIAGNOSTICS OF THIS IMAGE'S OWN TEXELS,
    /// AND THEY REACHED NOTHING AT ALL UNTIL `Pass 140.2`.
    ///
    /// [`decode`] converts every texel through [`crate::color`], which
    /// counts its own shortfalls — a missing or malformed `/tintTransform`,
    /// a `/Separation /All` approximation, an ICC alternate taken. Those
    /// counts went into two local [`ColorDiagnostics`] values that were
    /// **constructed, written to, and dropped at the end of the function**.
    ///
    /// So the visible consequence, measured on an image-only page whose
    /// `/Separation` carries a deliberately malformed transform (wrong
    /// arity, wrong output count):
    ///
    /// ```text
    ///           tint_applied   tint_not_applied
    /// good           0                0
    /// broken         0                0
    /// ```
    ///
    /// **Both zero, both times.** The broken one rendered as
    /// `separation_to_rgb`'s neutral stand-in — whose own note says
    /// *"lightness preserved, hue is not the document's"* — and pdfcer said
    /// **nothing**. That is a **rule 4** violation, not a missing
    /// statistic: pdfcer substituted a colour the document never specified
    /// and the substitution was silent. Rule 4 forbids silence, and an
    /// image is precisely the case where the operator cannot see the
    /// difference, because a plausible grey looks like a grey the file
    /// might have asked for.
    ///
    /// ★★ It also made a CENSUS counter lie by omission, which is how it
    /// was caught. `tint_applied` reads as "how many tint transforms did
    /// this page run", and it counted only paths and shadings — so a page
    /// whose only spot content is an image reported `0` while running one
    /// transform per distinct sample tuple. The engineer read `292` off a
    /// five-colorant `DeviceN` page and attributed it to the image's own
    /// cache; it was the page's path fills, and the image's contribution
    /// was invisible. **A counter that omits one producer is not a smaller
    /// number, it is a different question**, and nothing in the name says
    /// which.
    ///
    /// ⇒ Merged into [`crate::interpret::Diagnostics::color`] by
    /// `note_image_divergence`, on the same terms as every other field
    /// here.
    ///
    /// ★ Why the counts are per DISTINCT SAMPLE TUPLE and not per texel:
    /// [`TintCache`] owns one of the two sources precisely so that a single
    /// broken transform reports once per distinct colour rather than eight
    /// million times. That was always the design; only the delivery was
    /// missing.
    ///
    /// ★ NOT included, and deliberately: an `/Indexed` palette's own
    /// conversions ([`resolve_indexed`]'s `palette_diag`). A palette is
    /// built once, bounded by `hival + 1`, and its shortfall is already
    /// visible as a wrong entry count rather than a counter — that
    /// function says so in its own body. Folding it in here would report a
    /// 256-entry palette's single bad transform as 256 texel conversions.
    pub color: ColorDiagnostics,
}

/// A decoded image: `Width × Height` RGBA texels, row 0 at the **top**
/// (module docs), plus what diverged.
#[derive(Debug)]
pub struct DecodedImage {
    /// The texels. Premultiplied RGBA, as tiny-skia requires.
    pub pixmap: Pixmap,
    /// The image's **authored ink**, texel for texel, when its colour space
    /// is `DeviceCMYK` — `None` for every other space.
    ///
    /// # ★★ WHY A SECOND COPY OF THE SAME PICTURE EXISTS
    ///
    /// [`Self::pixmap`] has already been through `CMYK → sRGB`, and **that
    /// conversion is many-to-one**: different ink mixes produce identical
    /// screen colour, K and CMY trading off against each other. So once a
    /// texel is in `pixmap`, its ink is not recoverable — not approximately,
    /// not with a better inverse. **No inverse exists.**
    ///
    /// That matters because a page whose blending space is subtractive
    /// composites in ink. Feeding it `pixmap` means `CMYK → sRGB → CMYK`,
    /// and the return leg is a *different function* from the outbound one
    /// (a calibrated table out, a naive formula back), so the ink that
    /// arrives is not the ink that left.
    ///
    /// Measured on a print-conformance patch built to catch exactly this: a
    /// red square drawn as a path and the same red drawn as a `DeviceCMYK`
    /// image land on `(238, 29, 35)` and `(225, 63, 50)`. The patch's own
    /// instruction is that no difference should be visible.
    ///
    /// **The confirming experiment was**: set the conversion to `naive` —
    /// where the two legs happen to be exact inverses — and watch the
    /// difference vanish, which is what identified the round trip as the
    /// cause rather than the decode.
    ///
    /// ★ **That recipe is no longer runnable.** `CmykIntent::Naive` was
    /// deleted by operator ruling in `Pass 153.0`, so the conversion has no
    /// exactly-invertible setting left. The **finding stands** — it was
    /// measured, and decision 087 rests on it — but anyone re-deriving it
    /// today needs a different lever, and would have spent a while looking
    /// for a setting that no longer exists.
    ///
    /// Kept rather than deleted because a doc comment recording *how a
    /// conclusion was reached* is worth more than one asserting it, even
    /// when the method has expired. What it owes is to say that it has.
    ///
    /// So the ink is carried forward instead of being reconstructed.
    ///
    /// # Why two pixmaps rather than one four-channel buffer
    ///
    /// Because the compositor's geometry must be **identical** to the sRGB
    /// path's — same transform, same interpolation, same edge coverage — and
    /// the cheapest way to guarantee that is to run the same rasteriser over
    /// the same shape. `tiny_skia` rasterises RGBA, so the four colorants
    /// travel as two RGB triples: `C,M,Y` in one and `K,K,K` in the other,
    /// both carrying the image's own alpha.
    ///
    /// Reconstructing the mapping by inverting the device transform and
    /// sampling per pixel would be a second implementation of the
    /// resampling, and it would disagree with the first at every edge.
    pub ink: Option<CmykTexels>,
    /// Table 149's inputs for a `Separation`/`DeviceN` image — its colorant
    /// names and its **authored process tints, texel for texel**.
    ///
    /// `None` for every other space. Only the `Separation`/`DeviceN` row asks
    /// for a component the source did not name to be taken from the backdrop
    /// in a way this structure can express, so only it needs anything carried
    /// here.
    ///
    /// ★★★ THE JUSTIFICATION THAT USED TO SIT HERE WAS FALSE, and is quoted
    /// rather than deleted because the same sentence was written in three
    /// places and believed for many Passes:
    ///
    ///   "§11.7.4.3's Table 149 gives a *process* colour space `c_s` in all
    ///    three columns, so painting a `DeviceGray`, `DeviceRGB` or
    ///    `DeviceCMYK` image normally under `/OP true` **is** the conforming
    ///    result."
    ///
    /// Table 149's "any process colour space" row has TWO sub-rows. The
    /// *process component* one reads `c_s` throughout, as quoted. The *spot
    /// colorant* one reads `c_b` under `OP true` — so such an image IS owed
    /// preservation of a spot colorant in the backdrop, and painting it
    /// normally is conforming only when there is no spot beneath it.
    ///
    /// `None` here was therefore a REAL SHORTFALL rather than a
    /// correctly-empty case, and was disclosed as one
    /// (`Diagnostics::overprint_process_images_unsupported`) until
    /// `Pass 238.0` closed it — not by putting anything in this structure,
    /// but by telling the compositor to leave the spot planes alone
    /// (`SpotSource::Preserve`) when a process-space image paints under
    /// `/OP true`. A process source states no spot tint, so there is nothing
    /// per-texel to carry; the whole of its spot behaviour is one policy. The
    /// counter is kept on the metrics line, at zero, for script stability.
    ///
    /// # ★ Why this is a SECOND set of planes rather than [`Self::ink`]
    ///
    /// The two answer different questions and disagree on exactly the case
    /// that matters. [`Self::ink`] is *"what ink does this texel put on the
    /// sheet"* — the value a normal paint lays down, which for a
    /// `Separation` is its tint transform's output. This is *"which process
    /// tints did the file's own operands state"*, read straight out of the
    /// operands in `names` order per §8.6.6.5 and with a spot colorant
    /// contributing **nothing** (it has no process channel to contribute
    /// to). Reusing one for the other would make a `/Separation /PANTONE-185`
    /// image paint nothing at all, because its authored process tints are
    /// all zero while the ink it lays down plainly is not.
    ///
    /// See [`crate::overprint::authored_tints`], which is the single
    /// implementation of the read and is shared with the path painter so the
    /// two cannot come to disagree about the same space.
    pub overprint: Option<OverprintSource>,
    /// Whether this image's samples were **colour-managed** through its
    /// embedded ICC profile (`Pass 214.0`).
    ///
    /// ★ It exists so the disclosure counters can tell the two cases apart.
    /// `Pass 207.0` added `icc_unmanaged_paints` and counted every `ICCBased`
    /// image on a subtractive page, because at that point none of them COULD
    /// be managed. Now some are — and a counter that kept reporting them as
    /// unmanaged would be wrong in the opposite direction from the defect it
    /// was written to fix, which is the more embarrassing half of the same
    /// mistake.
    pub icc_managed: bool,
    /// Divergences that still produced pixels.
    pub notes: ImageNotes,
}

/// A `Separation`/`DeviceN` image's §11.7.4.3 inputs.
///
/// Produced by [`decode`] only for a space that classifies into Table 149's
/// third row (directly or through an `/Indexed` base), and consumed only by
/// the overprint path — an image painted with `/OP false` never reads it.
#[derive(Debug)]
pub struct OverprintSource {
    /// The colorants the image's space names, as
    /// [`crate::overprint::classify`] read them. This decides the four
    /// [`crate::overprint::ComponentRule`]s **once for the whole image**:
    /// row 3's selection depends on *which* colorants are named and never on
    /// their tints, so there is no per-texel rule evaluation to do.
    pub kind: crate::overprint::SourceKind,
    /// The authored process tints, packed exactly as [`CmykTexels`] packs
    /// ink so the same rasteriser can carry both.
    pub tints: CmykTexels,
    /// The authored **spot** tints, one plane per spot colorant the space
    /// names, in the space's declaration order (`Pass 238.0`).
    ///
    /// # Why these live here and not on [`DecodedImage::ink`]
    ///
    /// Together with [`Self::tints`] they are the image's colour **as the
    /// file stated it**: process tints in the four process channels, each
    /// spot in its own plane. [`DecodedImage::ink`] is the same colour
    /// **flattened** — the tint transform's output, which already contains
    /// every spot's contribution as process ink. A spot colorant's ink can
    /// arrive by exactly one of those two routes; carrying both at once lays
    /// it down twice. So the compositor uses `tints` + `spots` when every
    /// spot got a plane, and `ink` alone when any was refused — the same
    /// all-or-nothing rule `cmyk_paint` applies to a fill, and the reason a
    /// spot fill and a spot image of the same tint agree.
    ///
    /// Empty when the space names no spot colorant (a `DeviceN` of process
    /// names only), and empty under the composite device model — the
    /// interpreter decides that, not the decoder, by ignoring these.
    pub spots: Vec<SpotTexel>,
}

pub(crate) use crate::overprint::SpotColorant;

/// One spot colorant's per-texel tint, packed for rasterisation.
///
/// The tint is replicated across the red, green and blue channels and
/// premultiplied by the image's own alpha — the identical packing
/// [`CmykTexels::k`] uses, for the identical reason: any channel is the
/// right channel, so the writer and the reader cannot disagree about which.
#[derive(Debug)]
pub struct SpotTexel {
    /// The colorant name, `#xx`-decoded — the comparison form §7.3.5
    /// specifies, and the key `CmykBuffer::spot_index` allocates planes by.
    pub colorant: std::sync::Arc<[u8]>,
    /// This colorant alone on white, sampled across the tint range, from
    /// the image's own space through [`crate::overprint::spot_lut`].
    pub(crate) lut: std::sync::Arc<crate::cmyk_buffer::SpotLut>,
    /// The tint plane.
    pub tint: Pixmap,
}

/// A `DeviceCMYK` image's colorants, packed for rasterisation.
///
/// Both planes carry the image's own alpha, so either one un-premultiplied
/// by that alpha yields the authored tint. See [`DecodedImage::ink`].
#[derive(Debug)]
pub struct CmykTexels {
    /// `C`, `M`, `Y` in the red, green and blue channels.
    pub cmy: Pixmap,
    /// `K`, replicated across all three channels so any one of them reads it.
    pub k: Pixmap,
}

/// Decode an image XObject or inline image into RGBA texels.
///
/// - `dict` is the image dictionary (Table 89) or the inline image's
///   already-normalized parameter dictionary (Table 93 abbreviations
///   expanded by `pdfcer_core::content`, so exactly one key spelling
///   reaches this function).
/// - `raw` is the **still-encoded** sample data; the `/Filter` chain and
///   any terminal image codec are run here so that `/DecodeParms`
///   predictors and codec dispatch are handled by the one
///   implementation in `pdfcer-core`.
/// - `resources` is the current resource dictionary, needed for a
///   `/ColorSpace` given as a *name* referring to the `/ColorSpace`
///   subdictionary (§8.9.7 permits this for inline images from PDF 1.2,
///   and image XObjects have always permitted it).
/// - `fill` is the current **non-stroking** colour, used only by the
///   stencil-mask path (§8.9.6.2) and ignored otherwise.
/// - `origin` selects §8.9.7's stricter inline-image filter rules.
///
/// ## Transparency precedence when a document names more than one
///
/// §8.9.6 is silent on this, and `iso32000__s__8.9.5.2.md` carried it as
/// an open gap until 2026-08-08 — but the answer is **normative and
/// verbatim**, it simply lives in Table 89's `SMask` row rather than in
/// §8.9.6 where a reader would look for it:
///
/// > "shall **override** the current soft mask in the graphics state,
/// > **as well as the image's `Mask` entry, if any**. However, the other
/// > transparency-related graphics state parameters — blend mode and
/// > alpha constant — shall remain in effect."
///
/// §11.6.4.3 says the same thing independently. So co-presence is
/// **legal, not an error**: the loser is ignored, and — R34 — must still
/// round-trip byte-identical, which it does here because nothing in this
/// path writes.
///
/// The ladder pdfcer walks:
///
/// 1. **`/ImageMask true`** short-circuits everything — such an image
///    has no colour, so there is nothing for a mask to make transparent
///    (and §8.9.6.2 forbids it carrying `/Mask` at all).
/// 2. **`/SMask`** (and `/SMaskInData` ≠ 0), by the quotation above.
/// 3. **`/Mask`** — stream (stencil) or array (colour key), dispatched
///    on the resolved type as §8.9.6.1 requires.
/// 4. **The JPX codestream's own opacity channel**, when
///    `/SMaskInData 1` switched it on (Table 89).
///
/// Below all of that sits the ExtGState soft mask and then 1.0, neither
/// of which this Pass implements — see the module docs of
/// [`crate::mask`] and `iso32000__ref__image_transparency.md`.
///
/// A mechanism that is present but refused (see
/// [`crate::mask::MaskRefusal`]) does **not** fall through to the next
/// one: the document named a mechanism, pdfcer could not honour it, and
/// quietly substituting a different one would be exactly the kind of
/// plausible-looking guess `fuzzy, never sneaky` forbids. The refusal is
/// recorded in [`ImageNotes::mask_refused`] and the image draws opaque.
///
/// # Errors
///
/// [`ImageError`] — see its variants. Every one means "nothing drawn".
/// A mask that cannot be decoded is **not** one of them: the picture is
/// still drawn, and the shortfall is a note rather than an error.
// EIGHT parameters, one over clippy's threshold, and the eighth is the one
// that makes an ICC image render correctly. The alternative -- bundling the
// existing seven into a struct -- would touch every caller and every test that
// names them, to satisfy a count rather than a reader. The parameters are all
// distinct types with distinct roles and none of them is a boolean.
#[allow(clippy::too_many_arguments)]
pub fn decode(
    doc: &DocumentView<'_>,
    dict: &Dict,
    raw: &[u8],
    resources: &Dict,
    fill: Rgb,
    origin: ImageOrigin,
    policy: RenderPolicy,
    icc: IccContext<'_>,
) -> Result<DecodedImage, ImageError> {
    let width = positive_dimension(doc, dict, b"Width")?;
    let height = positive_dimension(doc, dict, b"Height")?;

    // Ceiling FIRST — before the filter chain runs and before any
    // pixmap allocation (module docs / ARCHITECTURE.md §10.1). The
    // codec layer applies the same ceiling to the *codestream's* own
    // declared geometry; this one covers the dictionary's, which is
    // what sizes the pixmap.
    if u64::from(width).saturating_mul(u64::from(height)) > MAX_IMAGE_PIXELS {
        return Err(ImageError::TooLarge);
    }

    // `DCT-A1` (R169) travels with the decode rather than being read
    // from anywhere ambient: the polarity rule changes the SAMPLES, so a
    // cached or re-run decode under a different setting must be a
    // different call, not the same call with a different global.
    let coded = image_codec::decode_image_view_with(
        doc,
        dict,
        raw,
        origin == ImageOrigin::Inline,
        policy.cmyk_jpeg_polarity,
    )
    .map_err(map_codec_error)?;

    let mut notes = ImageNotes {
        codec_geometry_mismatch: coded.notes.geometry_mismatch,
        dct_cmyk_image: coded.notes.cmyk_image,
        dct_cmyk_polarity_unverifiable: coded.notes.cmyk_polarity_unverifiable,
        jpx_smask_in_data_preblended: coded.notes.jpx_smask_in_data_preblended,
        lzw_framing_anomalies: coded.notes.lzw_framing_anomalies,
        ..ImageNotes::default()
    };
    // §8.9.6.2: an image mask is a completely different object — no
    // colour space, no colour conversion, `Decode` is a polarity bit.
    // Step 1 of the precedence ladder in this function's docs: it
    // short-circuits before any mask is even looked for.
    if matches!(
        dict.get(b"ImageMask").map(|o| doc.resolve(o)),
        Some(Object::Boolean(true))
    ) {
        return decode_stencil(dict, &coded, width, height, fill, notes);
    }

    let mut tr = resolve_transparency(doc, dict, &coded, resources, &mut notes);
    if let Some(plane) = &tr.alpha
        && plane.dimensions() != (width, height)
    {
        notes.mask_resampled = true;
        // Table 145's `Width` row: "**If a `Matte` entry … is present,
        // shall be the same as the `Width` value of the parent image**;
        // otherwise independent of it." A mismatched `/Matte` mask is
        // therefore non-conformant, and un-premultiplying with a
        // resampled α would divide each sample by an alpha that is not
        // its own — recovering colours from the wrong equation. The
        // ALPHA is still honoured (that half is conformant either way);
        // only the colour correction is dropped, by name.
        if tr.matte.is_some() {
            tr.matte = None;
            notes.matte_not_undone = Some("matte/dimension-mismatch");
        }
    }

    decode_sampled(
        doc,
        dict,
        &coded,
        width,
        height,
        resources,
        notes,
        tr.alpha.as_ref(),
        tr.colour_key,
        tr.matte.as_deref(),
        policy,
        icc,
    )
}

/// Whichever alpha source won [`decode`]'s precedence ladder, plus the
/// `/Matte` that may travel with a soft mask.
///
/// A struct rather than a tuple because the three fields are not
/// interchangeable and two of them are `Option`s of the same shape —
/// exactly the situation where a positional return silently swaps under
/// a later edit.
struct Transparency<'a> {
    /// Per-texel alpha, for the three mechanisms that have one.
    alpha: Option<AlphaPlane>,
    /// The un-parsed colour-key `/Mask` array (§8.9.6.4), which cannot
    /// become a plane — see [`resolve_transparency`].
    colour_key: Option<&'a Object>,
    /// `/Matte` (Table 146), in the **parent image's** colour space.
    matte: Option<Vec<f32>>,
}

/// Walk the precedence ladder documented on [`decode`] and produce
/// whichever alpha source wins.
///
/// Returns the resolved [`AlphaPlane`] for the three per-sample
/// mechanisms, **or** the un-parsed `/Mask` array object for colour-key
/// masking — which cannot become a plane here, because §8.9.6.4's ranges
/// are tested against the base image's own pre-`/Decode` samples and
/// those only exist inside [`decode_sampled`]'s pixel loop.
///
/// Every refusal is recorded in `notes` and none of them falls through
/// to the next mechanism (see [`decode`]'s precedence section for why).
fn resolve_transparency<'a>(
    doc: &DocumentView<'a>,
    dict: &'a Dict,
    coded: &CodedImage,
    resources: &Dict,
    notes: &mut ImageNotes,
) -> Transparency<'a> {
    let none = Transparency {
        alpha: None,
        colour_key: None,
        matte: None,
    };

    // Rung 2 — `/SMask`.
    if let Some(entry) = dict.get(b"SMask") {
        return match mask::soft_mask_plane(doc, entry, resources) {
            Ok(soft) => {
                notes.mask_applied = Some(MaskApplied::SoftMask);
                Transparency {
                    alpha: Some(soft.plane),
                    colour_key: None,
                    matte: soft.matte,
                }
            }
            Err(err) => {
                notes.mask_refused = Some(err.key());
                none
            }
        };
    }

    // Rung 3 — `/Mask`, dispatched on its resolved type (§8.9.6.1: a
    // stream is an explicit mask, an array is a colour-key mask).
    if let Some(entry) = dict.get(b"Mask") {
        return match doc.resolve(entry) {
            Object::Array(_) => {
                // Parsed in `decode_sampled`, where the component count
                // is known; the entry is carried, not the ranges.
                notes.mask_applied = Some(MaskApplied::ColourKey);
                Transparency {
                    colour_key: Some(entry),
                    ..none
                }
            }
            _ => match mask::stencil_plane(doc, entry) {
                Ok(plane) => {
                    notes.mask_applied = Some(MaskApplied::Stencil);
                    Transparency {
                        alpha: Some(plane),
                        ..none
                    }
                }
                Err(err) => {
                    notes.mask_refused = Some(err.key());
                    none
                }
            },
        };
    }

    // Rung 4 — the JPX codestream's own opacity channel. Present only
    // when `/SMaskInData` is 1 (Table 89: the default of 0 means the
    // channel "shall be ignored", so a JPX image with alpha inside it
    // and no `/SMaskInData` is CORRECTLY drawn opaque and is not a
    // shortfall). `/SMaskInData 2` leaves this `None` and is reported
    // through `jpx_smask_in_data_preblended` instead — those colour
    // samples carry a backdrop that needs `/Matte` to undo.
    if let Some(bytes) = &coded.embedded_alpha {
        match AlphaPlane::from_bytes(coded.width, coded.height, bytes.clone()) {
            Some(plane) => {
                notes.mask_applied = Some(MaskApplied::EmbeddedAlpha);
                return Transparency {
                    alpha: Some(plane),
                    ..none
                };
            }
            None => {
                // A short opacity channel is a codec bug, not a
                // document defect; named all the same.
                notes.mask_refused = Some("mask/short-embedded-alpha");
            }
        }
    }

    none
}

/// The physical layout of the sample bytes in hand.
///
/// Distinct from the *image's* `/Width`//`/Height`//`/BitsPerComponent`
/// because a codec declares its own geometry and the two can disagree
/// (module docs). This struct is what the row-stride arithmetic uses;
/// the dictionary's numbers size the pixmap.
#[derive(Debug, Clone, Copy)]
struct SampleLayout {
    /// Samples per row, from the codestream when one exists.
    width: u32,
    /// Components per sample, from the codestream when one exists.
    components: usize,
    /// Bits per component, from the codestream when one exists.
    bits: u32,
}

impl SampleLayout {
    /// Resolve the layout, preferring the codec's declaration and
    /// falling back to the PDF-declared one.
    ///
    /// `codec: None` means no codec ran, so the dictionary is the only
    /// description there is and its values are used verbatim — which is
    /// exactly the pre-Pass-2.1 behaviour, unchanged.
    fn resolve(
        coded: &CodedImage,
        dict_width: u32,
        dict_components: usize,
        dict_bits: u32,
    ) -> Self {
        match coded.codec {
            None => Self {
                width: dict_width,
                components: dict_components,
                bits: dict_bits,
            },
            Some(_) => Self {
                width: if coded.width > 0 {
                    coded.width
                } else {
                    dict_width
                },
                components: if coded.components > 0 {
                    usize::from(coded.components)
                } else {
                    dict_components
                },
                bits: if coded.bits_per_component > 0 {
                    u32::from(coded.bits_per_component)
                } else {
                    dict_bits
                },
            },
        }
    }
}

/// Read a required positive integer dimension (`Width`/`Height`).
///
/// Table 89 marks both Required; zero or negative is Table 89's
/// "entries inconsistent with each other" case (an image with no
/// samples cannot be painted, and a zero stride would divide by zero
/// downstream).
fn positive_dimension(doc: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Result<u32, ImageError> {
    let raw = dict
        .get(key)
        .map(|o| doc.resolve(o))
        .and_then(Object::as_int)
        .ok_or(ImageError::Malformed("missing /Width or /Height"))?;
    u32::try_from(raw)
        .ok()
        .filter(|&v| v > 0)
        .ok_or(ImageError::Malformed("/Width or /Height is not positive"))
}

/// Turn a filter failure into an image failure, preserving the filter
/// name so the diagnostics can say *which* codec is missing (the
/// operator's next question is always "so what do I need?").
fn map_filter_error(err: FilterError) -> ImageError {
    ImageError::UnsupportedFilter(err.to_string())
}

/// Turn a codec failure into an image failure, keeping the three
/// distinctions the diagnostics need to count separately (decision 005
/// §6.4): "this codec is not built", "this *feature* of this codec is
/// not built", and "these bytes are broken."
fn map_codec_error(err: ImageCodecError) -> ImageError {
    match err {
        ImageCodecError::Filter(inner) => map_filter_error(inner),
        ImageCodecError::FeatureUnsupported { feature } => ImageError::CodecFeature(feature),
        ImageCodecError::TooLarge => ImageError::TooLarge,
        // `Unsupported` and `NotAllowedInline` are both "pdfcer will not
        // decode this codec here"; the message already says which and
        // why, and an operator's next action is the same either way.
        other
        @ (ImageCodecError::Unsupported { .. } | ImageCodecError::NotAllowedInline { .. }) => {
            ImageError::CodecUnsupported(other.to_string())
        }
        other => ImageError::UnsupportedFilter(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Stencil masks (§8.9.6.2)
// ---------------------------------------------------------------------------

/// Build the RGBA texels for an image mask: opaque `fill` where the
/// sample says "mark", fully transparent where it says "leave alone".
///
/// §8.9.6.2's three normative rules are enforced here:
/// 1. no `/ColorSpace` (ignored if present — `iso32000__s__8.9.5.2.md`
///    recommends honouring `ImageMask` over a non-conformant
///    `ColorSpace`);
/// 2. `/BitsPerComponent` shall be 1 (forced, and a stated other value
///    is a hard inconsistency rather than something to guess around) —
///    **except for `JPXDecode`**, where Table 89's "optional and shall
///    be ignored if present" overrides §8.9.6.2's requirement, being
///    the more specific rule;
/// 3. `/Decode [0 1]` (the **default**) means **0 marks the page**;
///    `[1 0]` reverses it. This is the *one* case where a JPX image's
///    `/Decode` is honoured: §7.4.9 says it "shall be ignored, except
///    in the case where the image is treated as a mask; that is, when
///    `ImageMask` is true", which is precisely this function.
///
/// ## Why the sample is thresholded rather than read as a raw bit
///
/// Every other stencil codec delivers genuine 1-bit samples, so the
/// mask sample *is* the bit. JPX does not: §7.4.9 requires the
/// codestream to "provide a single colour channel with 1-bit samples"
/// for a mask, but pdfcer's JPX adapter normalizes every depth to 8 bits
/// (Table 89 makes the delivered depth the reader's choice), so those
/// 1-bit samples arrive as 0 and 255. Reading them at one bit per
/// sample would unpack eight neighbouring pixels out of every one and
/// shear the mask beyond recognition.
///
/// So the row stride and the sample width come from the codec's own
/// declared depth, and the result is compared against zero. For a 1-bit
/// codec that is exactly the old behaviour (`0`/`1` are the only
/// values); for JPX it is exact for conformant data (`0`/`255`); and
/// for a non-conformant deeper mask "any non-zero marks" is a stated
/// fail-soft rather than an invented threshold.
fn decode_stencil(
    dict: &Dict,
    coded: &CodedImage,
    width: u32,
    height: u32,
    fill: Rgb,
    mut notes: ImageNotes,
) -> Result<DecodedImage, ImageError> {
    let data = &coded.samples;
    if let Some(bpc) = dict.get(b"BitsPerComponent").and_then(Object::as_int)
        && bpc != 1
        && coded.codec != Some(Codec::Jpx)
    {
        return Err(ImageError::Malformed(
            "/ImageMask true requires /BitsPerComponent 1",
        ));
    }

    // Polarity: the sample value that MARKS the page. Default `[0 1]`
    // → 0 marks. `[1 0]` → 1 marks. Anything else is not a legal image
    // mask `Decode`; fall back to the default rather than inventing a
    // meaning.
    let ink_sample: u32 = match decode_pairs(dict) {
        Some(pairs) => match pairs.as_slice() {
            [(a, b)] if *a > *b => 1,
            [(_, _)] => 0,
            _ => {
                notes.decode_array_ignored = true;
                0
            }
        },
        None => 0,
    };

    // §8.9.6.2 forces one component, so the component count is fixed by
    // the construct; the width and the *delivered* bit depth come from
    // the codec, because they describe the bytes actually in hand (see
    // the threshold note in this function's docs).
    let layout = SampleLayout::resolve(coded, width, 1, 1);
    let stride = row_stride(layout.width, 1, layout.bits)?;
    if data.len() < stride.saturating_mul(height as usize) {
        notes.truncated = true;
    }

    let mut pixmap = Pixmap::new(width, height).ok_or(ImageError::TooLarge)?;
    let ink = premultiplied(fill, 255);
    // A fully transparent texel must have zero colour too: tiny-skia
    // stores PREMULTIPLIED components, and `r > a` is an invalid
    // premultiplied colour it will refuse to construct.
    let clear = PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap_or(ink);

    let texels = pixmap.pixels_mut();
    for y in 0..height as usize {
        let row_bit_base = y.saturating_mul(stride).saturating_mul(8);
        for x in 0..width as usize {
            let raw = read_sample(data, row_bit_base + x * layout.bits as usize, layout.bits);
            let sample = u32::from(raw != 0);
            let Some(slot) = texels.get_mut(y * width as usize + x) else {
                continue;
            };
            *slot = if sample == ink_sample { ink } else { clear };
        }
    }

    // A stencil mask paints the CURRENT FILL COLOUR through a
    // 1-bit shape; it carries no colorants of its own, so there
    // is no authored ink to preserve. (`ink` in this scope is
    // that fill colour, not the CMYK planes -- a collision the
    // type checker caught and a rename would only paper over.)
    // A stencil mask likewise states no colorants of its own for Table 149
    // to read: §8.9.6.2 makes it "a region of the page to be painted with
    // the current colour", so the SOURCE COLOUR is the graphics state's,
    // not the image's, and the path painter's `paint_overprint` is what
    // governs a fill of that colour. Overprinting a stencil is therefore
    // not an image question at all, and `None` says so.
    Ok(DecodedImage {
        pixmap,
        ink: None,
        overprint: None,
        // A stencil mask has no colour space of its own -- it paints the
        // fill colour through a 1-bit shape -- so there is nothing here to
        // colour-manage and `false` is a fact rather than a default.
        icc_managed: false,
        notes,
    })
}

// ---------------------------------------------------------------------------
// Ordinary sampled images (§8.9.3, §8.9.5.2, §8.6)
// ---------------------------------------------------------------------------

/// Build the RGBA texels for a colour image.
///
/// `alpha` is a resolved soft/stencil/embedded alpha plane, sampled per
/// texel across the base's grid; `colour_key_entry` is the un-parsed
/// `/Mask` array for §8.9.6.4 masking. At most one of them is ever
/// `Some` — [`resolve_transparency`] enforces the precedence.
///
/// Both are applied **in the existing pixel loop**, not in a second pass
/// over the pixmap. That is a deliberate performance choice as much as a
/// tidiness one: the loop already reads every raw sample (which is what
/// colour-key masking needs) and already writes every texel (which is
/// what alpha needs), so transparency costs one array index and one
/// multiply per texel rather than a whole extra traversal of a
/// potentially 40-megapixel buffer.
#[allow(clippy::too_many_arguments)] // Each argument is a distinct input
// the loop genuinely needs; bundling them into a struct would move the
// same seven values behind one name without removing any of them.
fn decode_sampled(
    doc: &DocumentView<'_>,
    dict: &Dict,
    coded: &CodedImage,
    width: u32,
    height: u32,
    resources: &Dict,
    mut notes: ImageNotes,
    alpha: Option<&AlphaPlane>,
    colour_key_entry: Option<&Object>,
    matte: Option<&[f32]>,
    policy: RenderPolicy,
    icc: IccContext<'_>,
) -> Result<DecodedImage, ImageError> {
    // Two independent operator choices ride in on `policy` here, and they
    // touch different halves of the loop below: `cmyk_intent` decides
    // COLOUR (§8.6.4.4) and `mask_resample` decides ALPHA (`SM-A1`).
    let intent = policy.cmyk_intent;
    let data = &coded.samples;
    // Table 89 makes this filter — and only this filter — able to
    // supply its own colour space, bit depth and (non-)`Decode`.
    let jpx = coded.codec == Some(Codec::Jpx);

    let space = match dict.get(b"ColorSpace").map(|o| doc.resolve(o)) {
        // `/ColorSpace` present: the DICTIONARY wins, for every filter
        // including JPX. Table 89 is explicit — "If ColorSpace is
        // present, any colour space specifications in the JPEG2000 data
        // shall be ignored." Reading "the codestream is authoritative
        // for JPX" as a blanket rule and overriding a stated
        // `/ColorSpace` here is the inverted-inversion bug, and it would
        // produce wrong colour on exactly the files a producer took the
        // trouble to tag.
        Some(obj) => resolve_space(doc, obj, resources, 0, intent, icc)?,
        // `/ColorSpace` absent. Required for every other image (Table
        // 89: "Required for images, except those that use the JPXDecode
        // filter"), so this is malformed unless the codestream can
        // supply it.
        None if jpx => codestream_space(coded, icc)?,
        None => return Err(ImageError::Malformed("image has no /ColorSpace")),
    };
    let components = space.components();

    // `/BitsPerComponent` is Required (Table 89) — except for JPX,
    // where it is "optional and shall be ignored if present. The bit
    // depth is determined by the conforming reader in the process of
    // decoding." Note the two-step: for JPX a stated value is not
    // merely redundant, honouring it is *wrong*, so the dictionary is
    // not consulted at all. For DCT the codestream is likewise
    // authoritative (always 8, "each component value shall occupy a
    // byte") but the entry is still Required, so an absent one is
    // tolerated rather than mandated away.
    let declared_bpc = dict
        .get(b"BitsPerComponent")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_int);
    let bpc = match declared_bpc {
        // The Table 89 override: the codestream's depth, whatever the
        // dictionary said. The divergence is already counted by the
        // codec layer.
        _ if jpx => u32::from(coded.bits_per_component).max(1),
        Some(v @ (1 | 2 | 4 | 8 | 16)) => v as u32,
        Some(_) if coded.codec.is_none() => {
            return Err(ImageError::Malformed(
                "/BitsPerComponent is not 1, 2, 4, 8, or 16",
            ));
        }
        None if coded.codec.is_none() => {
            return Err(ImageError::Malformed("image has no /BitsPerComponent"));
        }
        // A codec ran and the dictionary is absent or nonsense; the
        // codestream's own depth is the truth (and the disagreement is
        // already counted by the codec layer).
        _ => u32::from(coded.bits_per_component).max(1),
    };

    // The physical byte layout, which is the codec's when there is one
    // (module docs: "the codestream wins for sample reading").
    let layout = SampleLayout::resolve(coded, width, components, bpc);
    // §8.9.5.2's domain: raw samples run 0 … 2ⁿ − 1, at the depth the
    // samples are ACTUALLY packed at.
    let max_sample = f32::from(u16::MAX).min(((1u32 << layout.bits.min(16)) - 1) as f32);

    // §8.9.5.2 + Table 90. `Decode` maps each raw integer linearly into
    // the colour space's component range; the DEFAULT is colour-space
    // dependent and is emphatically not always `[0 1]`.
    //
    // JPX is the one filter that bypasses this entirely: Table 89 says
    // "If the image uses the JPXDecode filter and ImageMask is false,
    // Decode shall be ignored by a conforming reader", and §7.4.9 says
    // the same from the filter's side ("shall be ignored, except in the
    // case where the image is treated as a mask"). The `ImageMask true`
    // half of that exception is honoured by `decode` branching to
    // `decode_stencil` before it ever reaches here, where `Decode` is a
    // polarity switch rather than a colour transform — so suppressing it
    // in this function is exactly the right scope. This is the one place
    // a shared "apply Decode" helper silently corrupts JPX output.
    let decode = match decode_pairs(dict) {
        _ if jpx => space.default_decode(max_sample),
        Some(pairs) if pairs.len() == components => pairs,
        Some(_) => {
            notes.decode_array_ignored = true;
            space.default_decode(max_sample)
        }
        None => space.default_decode(max_sample),
    };
    // Precompute (offset, slope) per component so the inner loop is one
    // multiply-add: y = Dmin + x·(Dmax − Dmin)/(2ⁿ − 1). A NEGATIVE
    // slope is the `[1 0]` inversion and must survive — this is exactly
    // where a `min`/`max` "normalization" would destroy it.
    let ramp: Vec<(f32, f32)> = decode
        .iter()
        .map(|&(dmin, dmax)| (dmin, (dmax - dmin) / max_sample))
        .collect();

    let stride = row_stride(layout.width, layout.components, layout.bits)?;
    if data.len() < stride.saturating_mul(height as usize) {
        notes.truncated = true;
    }
    // A codestream that declares a different component count from the
    // one `/ColorSpace` implies is Table 89's "entries inconsistent with
    // each other" case. Real files do it, so it is counted rather than
    // refused; only `layout.components` of them are read, so the rows
    // stay aligned either way.
    if coded.codec.is_some() && layout.components != components {
        notes.codec_geometry_mismatch = true;
    }
    let readable = components.min(layout.components);

    // `color__indexed.md`'s named fast path: convert the ≤256-entry
    // palette once, then the per-pixel loop is a table lookup with no
    // colour maths at all.
    let palette = space.palette();
    // Borrowed ONCE, beside the palette and for the same reason: the texel
    // loop cannot re-match on `space` per pixel without moving out of it, and
    // an index is only meaningful while the table it indexes is in scope.
    let palette_ink_table: Option<&Vec<[f32; 4]>> = match &space {
        Space::Indexed { ink, .. } => ink.as_ref(),
        _ => None,
    };
    // Table 149's row for this image, and — for an `/Indexed` image — the
    // per-entry authored tints it selects over. Hoisted here for exactly the
    // reason the two tables above are: the texel loop cannot re-match on
    // `space` without moving out of it.
    //
    // Two shapes reach row 3: a DIRECT `Separation`/`DeviceN` image, whose
    // tints are the texel's own components, and an `/Indexed` image over such
    // a base, whose tints come from the palette. Both are common in print
    // files — the suite's own overprint patches use the second exclusively.
    //
    // Every other row is deliberately dropped to `None` here rather than
    // carried: a process source is `c_s` in all three of Table 149's columns,
    // so an ordinary paint of it already IS the overprint result and there is
    // nothing for the compositor to select between.
    let op_kind: Option<crate::overprint::SourceKind> = match &space {
        Space::Indexed {
            base_icc_managed: false,
            overprint: Some(o),
            ..
        } => Some(o.kind.clone()),
        Space::Special { cs, .. } => match crate::overprint::classify(
            cs,
            true,
            // ★ NOT a policy read, deliberately, and this is the one place
            // in the codebase where passing a literal is MORE honest than
            // threading the operator's setting through.
            //
            // `in_image_sample` is `true` here, and `classify` refuses to
            // upgrade a sampled image under ANY scope — Table 149 gives
            // `DeviceCmykDirect` the qualifier "and not in a sampled image",
            // so a CMYK image already falls to `OtherProcess` where `OPM 0`
            // and `OPM 1` are identical. Reading `policy.overprint_zero_tint_scope`
            // here would imply the operator's choice reaches this call. It
            // cannot, and a reader would have to go and prove that.
            //
            // Pinned by `a_grey_image_is_never_upgraded_whatever_the_scope`,
            // so if the `!in_image_sample` guard is ever removed this becomes
            // a failing test rather than a stale comment.
            pdfcer_core::settings::OverprintZeroTintScope::DeviceCmykOnly,
        ) {
            Some(k @ crate::overprint::SourceKind::SeparationOrDeviceN { .. }) => Some(k),
            _ => None,
        },
        _ => None,
    };
    let palette_op_table: Option<&Vec<[f32; 4]>> = match &space {
        Space::Indexed {
            base_icc_managed: false,
            overprint: Some(o),
            ..
        } => Some(&o.entries),
        _ => None,
    };
    // The spot colorants this image can deposit (`Pass 238.0`): for an
    // `/Indexed` image the palette-build already resolved names, curves and
    // per-entry tints; for a direct `Separation`/`DeviceN` image the tints
    // are the texel's own components at the resolved component indices, and
    // the curves are built here, once per image.
    let palette_spot_table: Option<&Vec<Vec<f32>>> = match &space {
        Space::Indexed {
            base_icc_managed: false,
            overprint: Some(o),
            ..
        } => Some(&o.spot_entries),
        _ => None,
    };
    let (spot_colorants, spot_components): (Vec<SpotColorant>, Vec<usize>) =
        match (&space, &op_kind) {
            (
                Space::Indexed {
                    overprint: Some(o), ..
                },
                Some(_),
            ) => (o.spot_colorants.clone(), Vec::new()),
            (Space::Special { cs, .. }, Some(kind)) => {
                let slots = crate::overprint::authored_spots(kind, &vec![0.0_f32; readable]);
                let colorants = slots
                    .iter()
                    .map(|(component, name, _)| {
                        (
                            std::sync::Arc::from(*name),
                            std::sync::Arc::new(crate::overprint::spot_lut(
                                cs, *component, readable, intent,
                            )),
                        )
                    })
                    .collect();
                (colorants, slots.iter().map(|(c, _, _)| *c).collect())
            }
            _ => (Vec::new(), Vec::new()),
        };

    // §8.9.6.4's ranges are counted against the IMAGE's colour space, so
    // the component count is the one resolved above — 1 for `Indexed`
    // (the index), not the base space's width. A length mismatch drops
    // the mask by name rather than masking the wrong colours.
    let colour_key = match colour_key_entry {
        Some(entry) => match mask::ColourKey::parse(doc, entry, components) {
            Ok(key) => Some(key),
            Err(err) => {
                notes.mask_applied = None;
                notes.mask_refused = Some(err.key());
                None
            }
        },
        None => None,
    };

    // §11.6.5.3's un-premultiply, validated once rather than per texel.
    // Two ways it is dropped here, both named:
    //
    // - `Indexed`: spec ambiguity SM-A4. Table 146 counts `/Matte`'s `n`
    //   from the parent's `/ColorSpace`, which for `Indexed` is 1 (the
    //   index); §11.6.5.3 says "the colour values in the colour table
    //   (not the index values themselves) shall be preblended", which
    //   needs the BASE space's `n`. Those cannot both be satisfied, and
    //   un-premultiplying a palette index is meaningless in any reading.
    // - a length that is not `n`: Table 146 is exact about it, and a
    //   short array would apply the matte to a prefix of the components,
    //   producing a colour cast on some channels and not others.
    let matte = match matte {
        None => None,
        Some(_) if palette.is_some() => {
            notes.matte_not_undone = Some("matte/indexed");
            None
        }
        Some(m) if m.len() != components => {
            notes.matte_not_undone = Some("matte/length");
            None
        }
        Some(m) => {
            notes.matte_undone = true;
            Some(m)
        }
    };

    // Hoisted out of the pixel loop so the overwhelmingly common
    // no-colour-key case pays a perfectly-predicted loop-invariant
    // branch rather than two stack stores per component per texel. On a
    // 40-megapixel CMYK image that is 320 million stores avoided; this
    // project has already had one render-performance emergency and the
    // cheapest time to not create the next one is now.
    let keying = colour_key.is_some();

    // A `Space::Special` conversion runs the document's own function per
    // distinct sample tuple, and the two ICC variants run a profile chain
    // per distinct tuple (`Pass 240.0`); everything else is closed-form
    // arithmetic and wants no cache at all. `tinting` is the loop-invariant
    // branch, in the same spirit as `keying` above it.
    //
    // ★★ `Icc` and `IccRgb` are on this route for CORRECTNESS, not only for
    // cost, and the omission of `Icc` was a shipped defect. The ink arm of
    // the texel loop reads its colorants from `texel_cmyk` when `tinting`
    // holds and from `last_comps` -- the RAW components -- when it does not.
    // A direct `ICCBased` `N 4` image therefore wrote its unmanaged samples
    // as ink under an `icc_managed: true` flag from `Pass 214.0` until this
    // one; the bridge ran exactly once, inside the `yields_cmyk` probe, and
    // never for a texel. (The `/Indexed` shape was unaffected: its palette
    // is built through `to_cmyk` at table time, which is why the conformance
    // patch that shipped the Pass looked right.) What the cache bounds is
    // the chain evaluation, which on a 16-bit photograph would otherwise run
    // per texel.
    let tinting = matches!(
        &space,
        Space::Special { .. } | Space::Icc { .. } | Space::IccRgb { .. }
    );
    // §8.6.6.4/.5: a `/None` colorant "shall never be painted on the
    // page". The whole image is therefore transparent — NOT white.
    //
    // The first version of this Pass returned white from the conversion
    // instead, which looks identical on a blank page and is wrong the
    // moment anything is underneath: an opaque white image ERASES the
    // backdrop the standard requires to show through. Caught by a fixture
    // whose divergence from pdfium was maximal in both directions.
    let suppressed = matches!(&space, Space::Special { cs, .. } if !cs.paints());
    if suppressed {
        notes.colorant_none_suppressed = true;
    }
    if let Space::Special { cs, .. } = &space {
        notes.uncalibrated_colorimetry = match &**cs {
            crate::color::ColorSpace::Lab { .. } => Some("Lab"),
            crate::color::ColorSpace::CalGray { .. } => Some("CalGray"),
            crate::color::ColorSpace::CalRgb { .. } => Some("CalRGB"),
            _ => None,
        };
    }
    let mut tint_cache = tinting.then(|| TintCache::new(layout.bits, readable));
    let mut scratch_diag = ColorDiagnostics::default();

    // Per-component clamp bounds. Only `Lab` differs from 0–1, and it
    // differs enough to matter — see the `default_decode` note.
    let clamp_range: Vec<(f32, f32)> = match &space {
        Space::Special { cs, .. } => (0..cs.components())
            .map(|i| cs.component_range(i))
            .collect(),
        _ => vec![(0.0, 1.0); components],
    };

    let mut pixmap = Pixmap::new(width, height).ok_or(ImageError::TooLarge)?;
    // The ink planes exist only for a `DeviceCMYK` image. Every other space
    // either has no colorants to preserve (`DeviceRGB`, `Lab`) or resolves
    // through a tint transform whose output is already the alternate's, and
    // in both cases the sRGB texels lose nothing a subtractive page wanted.
    //
    // ★ Allocated unconditionally for CMYK rather than only when the page
    // turns out to be subtractive, because THAT IS NOT KNOWN HERE — the
    // blending space is a page-level decision made after the image is
    // decoded, and an image is cached across pages. Two texel-sized pixmaps
    // is the price; the alternative is threading a page property into the
    // codec layer to save memory on a case that is already rare.
    // FOUR shapes carry ink, and this list has been wrong by omission twice
    // — `R219`'s shape, which is why `Pass 140.0` enumerated every route in
    // one Pass rather than fixing the one that was reported:
    //
    //   1. a direct `DeviceCMYK` image                        (`Pass 130.1`)
    //   2. an `/Indexed` image whose BASE is `DeviceCMYK`     (`Pass 130.1`)
    //   3. a direct `Separation`/`DeviceN` image over a
    //      `DeviceCMYK` alternate                             (`Pass 140.0`)
    //   4. an `/Indexed` image over such a base               (`Pass 140.0`)
    //
    // (2) is not a special case but the commoner one in print files, where a
    // flat colour ships as a one-entry palette rather than as four planes of
    // identical samples. (4) is the same shape one level in — a duotone.
    //
    // Rows 1 and 3 are both asked THROUGH `Space::yields_cmyk`, so the direct
    // route has one predicate rather than an enumeration that can fall
    // behind the conversion it is supposed to describe. Rows 2 and 4 are
    // resolved at palette-build time by `resolve_indexed`, through the very
    // same `Space::to_cmyk` applied to the base.
    let carries_ink = space.yields_cmyk(&mut scratch_diag)
        || matches!(space, Space::Indexed { ink: Some(_), .. });
    let mut ink = if carries_ink {
        match (Pixmap::new(width, height), Pixmap::new(width, height)) {
            (Some(cmy), Some(k)) => Some(CmykTexels { cmy, k }),
            // Out of memory for the extra planes is not a reason to fail the
            // image: the sRGB path still works and merely bridges, which is
            // what happened before this existed.
            _ => None,
        }
    } else {
        None
    };
    // The overprint planes, on the same terms as the ink planes above: two
    // texel-sized pixmaps, allocated whenever the space is row 3, because
    // whether the PAGE composites in ink is a decision made after the decode
    // and an image is cached across pages. A failed allocation is not a
    // failed image — the sRGB path still paints, and the overprint path then
    // discloses that it could not run rather than painting silently wrongly.
    let mut op_planes = if op_kind.is_some() {
        match (Pixmap::new(width, height), Pixmap::new(width, height)) {
            (Some(cmy), Some(k)) => Some(CmykTexels { cmy, k }),
            _ => None,
        }
    } else {
        None
    };
    // One tint plane per spot colorant, on the same all-or-nothing terms:
    // if any plane cannot be allocated none are kept, and the image then
    // flattens every spot exactly as it did before this Pass. A partial set
    // would deposit some colorants and flatten others, and the flattened
    // ink already contains the deposited ones -- double ink, the defect the
    // fill path's agreement tests caught.
    let mut spot_planes: Vec<Pixmap> = Vec::with_capacity(spot_colorants.len());
    if op_planes.is_some() {
        for _ in &spot_colorants {
            match Pixmap::new(width, height) {
                Some(p) => spot_planes.push(p),
                None => {
                    spot_planes.clear();
                    break;
                }
            }
        }
    }
    let spots_carried = !spot_colorants.is_empty() && spot_planes.len() == spot_colorants.len();
    let mut out_of_range = false;
    // ★ ALL-OR-NOTHING, and it is the safety net under `Space::yields_cmyk`'s
    // probe. Set the moment a texel's `to_cmyk` answers `None` on an image
    // whose space claimed it would not; the planes are then dropped for the
    // WHOLE image after the loop and it bridges uniformly.
    //
    // A partial plane would be worse than no plane: half the image
    // composited in ink and half through sRGB is a seam along a contour of
    // the tint transform's domain — a boundary no file asked for, appearing
    // in the middle of a photograph. `ColorRamp::new` refuses the same thing
    // for the same reason, one line of its own.
    let mut ink_incomplete = false;

    for y in 0..height as usize {
        let row_bit_base = y.saturating_mul(stride).saturating_mul(8);
        for x in 0..width as usize {
            let first = x.saturating_mul(layout.components);
            // Reused across texels; see the `None` arm below for why the ink
            // write needs it and why it must not be a per-arm local.
            let mut last_comps = [0.0f32; MAX_IMAGE_COMPONENTS];
            // Set by the palette arm below when the base is ink; `None` for
            // a direct image, whose colorants are `last_comps` instead.
            let mut palette_ink: Option<[f32; 4]> = None;
            // The same, for Table 149's authored tints. Separate from
            // `palette_ink` because the two tables answer different questions
            // — see `Space::Indexed::overprint`.
            let mut palette_op: Option<[f32; 4]> = None;
            // And the palette entry's spot tints, one per colorant.
            let mut palette_spots: Option<&Vec<f32>> = None;
            // This texel's own colorants, for a DIRECT `Separation`/`DeviceN`
            // image (row 3 of `carries_ink`'s list). Produced by the same
            // `TintCache` entry as the sRGB beside it, so the two cannot come
            // from different evaluations of the same tint transform.
            //
            // `None` for every other space, including `DeviceCMYK`: that one
            // reads its colorants straight out of `last_comps` below, because
            // they ARE the components and no transform is involved.
            let mut texel_cmyk: Option<[f32; 4]> = None;
            // Read the plane BEFORE the colour work: §11.6.5.3's
            // un-premultiply divides by this very value, and it must be
            // applied before the colour-space conversion below.
            // `SM-A1` (R169): the mask→base resampling filter. Passed
            // per call, not stored on the plane, so the same decoded mask
            // can be sampled two ways in one session without a rebuild.
            let plane_alpha = alpha.map_or(255u8, |p| {
                p.at(x as u32, y as u32, width, height, policy.mask_resample)
            });
            // The pre-`/Decode` integers, kept alive across the colour
            // conversion because §8.9.6.4 tests THESE, not the colours
            // they become ("representing colour values BEFORE decoding
            // with the `Decode` array"). Filling it is skipped entirely
            // when no colour-key mask is in force.
            let mut raw_comps = [0u32; MAX_IMAGE_COMPONENTS];
            let rgb = match &palette {
                Some(table) => {
                    // Indexed: one component, and after the (default:
                    // identity) Decode transform it IS the palette
                    // index. §8.6.6.3's clamp is normative.
                    let raw = read_sample(
                        data,
                        row_bit_base + first * layout.bits as usize,
                        layout.bits,
                    );
                    if keying && let Some(slot) = raw_comps.first_mut() {
                        *slot = raw;
                    }
                    let (dmin, slope) = ramp.first().copied().unwrap_or((0.0, 1.0));
                    let value = dmin + raw as f32 * slope;
                    let index = value.round().max(0.0) as usize;
                    // The SAME index into the parallel ink table. Resolved
                    // here rather than after the match because this is the
                    // only point where the index still exists — one line
                    // later it has become a colour.
                    palette_ink = palette_ink_table.and_then(|t| t.get(index).copied());
                    palette_op = palette_op_table.and_then(|t| t.get(index).copied());
                    palette_spots = palette_spot_table.and_then(|t| t.get(index));
                    match table.get(index) {
                        Some(&c) => c,
                        None => {
                            out_of_range = true;
                            Rgb::BLACK
                        }
                    }
                }
                None => {
                    // ★ The OUTER buffer, not a fresh local. The ink write
                    // below needs this tuple, and it is only valid here: by
                    // the next statement the colour has been converted and
                    // `comps` would be the previous texel's if it were not
                    // shared. Reusing one array also keeps the loop
                    // allocation-free, which it was before.
                    let comps = &mut last_comps;
                    for c in 0..readable {
                        let raw = read_sample(
                            data,
                            row_bit_base + (first + c) * layout.bits as usize,
                            layout.bits,
                        );
                        // Filled unconditionally when the space needs a
                        // cache key, not only when colour-keying: the key
                        // IS the raw tuple, so `tinting` joins `keying` as
                        // a reason to keep these.
                        if (keying || tinting)
                            && let Some(slot) = raw_comps.get_mut(c)
                        {
                            *slot = raw;
                        }
                        let (dmin, slope) = ramp.get(c).copied().unwrap_or((0.0, 1.0));
                        // §8.9.5.2's output clamping: "if an output
                        // value falls outside the range allowed for a
                        // component it shall be adjusted to the nearest
                        // allowed value."
                        //
                        // The allowed range is the SPACE's, not 0–1 — the
                        // distinction only bites for `Lab`, whose L is
                        // 0–100 and whose a/b are routinely negative.
                        // Clamping those to 0–1 would flatten the image to
                        // near-black.
                        let (lo, hi) = clamp_range.get(c).copied().unwrap_or((0.0, 1.0));
                        if let Some(slot) = comps.get_mut(c) {
                            *slot = (dmin + raw as f32 * slope).clamp(lo, hi);
                        }
                    }
                    // §11.6.5.3: "If a colour conversion is required,
                    // inversion of the preblending shall precede the
                    // colour conversion", and it is done "in the colour
                    // space specified by the parent image's ColorSpace
                    // entry" — which is exactly the state `comps` is in
                    // on this line and nowhere after it.
                    if let Some(m) = matte {
                        mask::undo_matte(comps, components.min(4), m, plane_alpha);
                    }
                    let (rgb, cmyk) = match &mut tint_cache {
                        Some(cache) => {
                            cache.lookup(&space, intent, &raw_comps[..readable], &comps[..readable])
                        }
                        // No cache means the space is not `Special`, so
                        // there is no tint transform to bound and no ink
                        // answer this arm could give — a `DeviceCMYK`
                        // image's colorants are its components, read from
                        // `last_comps` at the write below.
                        None => (space.to_rgb(intent, comps, &mut scratch_diag), None),
                    };
                    texel_cmyk = cmyk;
                    rgb
                }
            };
            // Alpha, in the order the precedence ladder resolved: a
            // colour-key hit is absolute (0 or 255, §8.9.6.4 has no
            // partial state), otherwise the plane's sample, otherwise
            // opaque.
            let a = match &colour_key {
                _ if suppressed => 0,
                Some(key) if key.masks(&raw_comps[..readable]) => 0,
                _ => plane_alpha,
            };
            let at = y * width as usize + x;
            if let Some(slot) = pixmap.pixels_mut().get_mut(at) {
                *slot = premultiplied(rgb, a);
            }
            // The authored ink, written texel-for-texel beside the sRGB.
            // `comps` is still in the image's own colour space here and
            // nowhere after this point.
            if let Some(planes) = ink.as_mut() {
                // Three sources, exactly one of which is meaningful for any
                // given image — the three `carries_ink` rows that reach a
                // texel:
                //
                //   * `/Indexed`: the palette entry's colorants, already
                //     resolved through the base at table-build time.
                //   * `Separation`/`DeviceN`: this texel's tint-transform
                //     output, cached beside its sRGB.
                //   * `DeviceCMYK`: the texel's own components, which ARE
                //     the colorants.
                //
                // ★ `tinting` is what separates the last two, and it must:
                // a `Separation`'s `last_comps` holds TINTS, not process
                // components. Falling through to the third arm would write a
                // five-colorant `DeviceN`'s first four tints as if they were
                // C, M, Y and K — plausible-looking ink of entirely the
                // wrong colour, which is worse than the bridge it replaced.
                let tint = match (palette_ink, tinting) {
                    (Some(t), _) => t,
                    (None, true) => match texel_cmyk {
                        Some(t) => t,
                        // The probe said this space yields colorants and this
                        // texel disagreed. Drop the whole plane after the
                        // loop rather than leave a seam; see `ink_incomplete`.
                        None => {
                            ink_incomplete = true;
                            [0.0; 4]
                        }
                    },
                    (None, false) => [
                        last_comps.first().copied().unwrap_or(0.0),
                        last_comps.get(1).copied().unwrap_or(0.0),
                        last_comps.get(2).copied().unwrap_or(0.0),
                        last_comps.get(3).copied().unwrap_or(0.0),
                    ],
                };
                write_ink(planes, at, tint, a);
            }
            // Table 149's source tints, written texel-for-texel beside both.
            // Same packing, same premultiply, same rasteriser downstream —
            // the whole reason `CmykTexels` is reused rather than a third
            // layout invented.
            if let (Some(planes), Some(kind)) = (op_planes.as_mut(), op_kind.as_ref()) {
                let tint = match palette_op {
                    Some(t) => t,
                    // A DIRECT `Separation`/`DeviceN` image: the operands are
                    // the texel's own components, still in the image's colour
                    // space on this line and nowhere after it.
                    None => crate::overprint::authored_tints(kind, &last_comps[..readable])
                        .unwrap_or([0.0; 4]),
                };
                write_ink(planes, at, tint, a);
            }
            // The spot tints, texel for texel, one plane per colorant. Read
            // from the palette row for an `/Indexed` image and from the
            // texel's own components for a direct one -- the same two
            // sources the process tints above come from.
            if spots_carried {
                for (i, plane) in spot_planes.iter_mut().enumerate() {
                    let tint = match palette_spots {
                        Some(row) => row.get(i).copied().unwrap_or(0.0),
                        None => spot_components
                            .get(i)
                            .and_then(|c| last_comps.get(*c))
                            .copied()
                            .unwrap_or(0.0),
                    };
                    write_tint(plane, at, tint, a);
                }
            }
        }
    }
    notes.palette_out_of_range = out_of_range;
    // All-or-nothing, discharged. See `ink_incomplete`'s declaration for why
    // a partial plane is worse than none.
    if ink_incomplete {
        ink = None;
    }
    // ★ `Pass 140.2`: hand the colour work's own account to the caller.
    //
    // ★★ THE TWO SOURCES ARE NOT SYMMETRIC, AND THE FIRST DRAFT OF THIS
    // COMMENT CLAIMED THEY WERE. It said merging only the cache "would leave
    // every non-`Special` image silent". That is **false**, and a sabotage
    // proved it: deleting the `scratch_diag` merge changes no test, because
    //
    //   * `tint_cache` is `Some` exactly when the space is `Space::Special`,
    //     `Space::Icc` or `Space::IccRgb` (`tinting`), so the `None` arm of
    //     the texel loop's match is reached only by `Gray`, `Rgb`, `Cmyk`
    //     and `Indexed` — and `Space::to_rgb` for those four is closed-form
    //     arithmetic that records nothing at all. There is no shortfall for
    //     such an image to report. (The two ICC variants record nothing
    //     either -- a bridge that refuses falls back silently -- but they
    //     are on the cached route for the ink arm's sake, see `tinting`.)
    //   * So `scratch_diag`'s ONLY possible contribution is the
    //     `yields_cmyk` probe, and the probe records something only in one
    //     narrow case: a `/tintTransform` that LOADS successfully and then
    //     fails to EVALUATE at an all-zero operand. A transform that fails to
    //     load leaves `tint: None`, and `tint_to_cmyk` returns early on that
    //     without counting anything.
    //
    // ⇒ The merge is kept as belt-and-braces for that narrow case, and it is
    // recorded here as **deliberately uncovered** rather than left to read as
    // tested. The fixtures' broken transform fails at LOAD, which is the
    // commoner malformation and the one worth pinning; a load-fine-eval-fail
    // fixture would exercise this line and does not exist.
    //
    // The cache's own diagnostics are the half that carries every real
    // count, and they are covered.
    notes.color.merge(scratch_diag);
    if let Some(cache) = tint_cache {
        notes.color.merge(cache.diag);
    }

    Ok(DecodedImage {
        pixmap,
        ink,
        // Both shapes count: a direct `ICCBased` image, and an `/Indexed`
        // one whose BASE was managed at palette-build time. The second is the
        // common shape in the conformance corpus, and omitting it is what made
        // the first cut of this flag under-report.
        icc_managed: matches!(space, Space::Icc { .. } | Space::IccRgb { .. })
            || matches!(
                space,
                Space::Indexed {
                    base_icc_managed: true,
                    ..
                }
            ),
        // Both halves or neither: a row-3 classification with no planes
        // behind it would let the caller compute rules and then read tints
        // that do not exist.
        overprint: op_kind.zip(op_planes).map(|(kind, tints)| OverprintSource {
            kind,
            tints,
            spots: if spots_carried {
                spot_colorants
                    .into_iter()
                    .zip(spot_planes)
                    .map(|((colorant, lut), tint)| SpotTexel {
                        colorant,
                        lut,
                        tint,
                    })
                    .collect()
            } else {
                Vec::new()
            },
        }),
        notes,
    })
}

/// Write one texel's spot tint into its plane — [`write_ink`]'s packing for
/// a single channel, replicated across all three and premultiplied by the
/// same alpha, so the compositor reads it back with the identical
/// un-premultiply it applies to the process planes.
fn write_tint(plane: &mut Pixmap, at: usize, tint: f32, alpha: u8) {
    let t = tint.clamp(0.0, 1.0);
    if let Some(slot) = plane.pixels_mut().get_mut(at) {
        *slot = premultiplied(Rgb::from_rgb(t, t, t), alpha);
    }
}

/// Write one texel's authored `DeviceCMYK` tint into the ink planes.
///
/// `comps` is in the image's own colour space and `alpha` is the texel's
/// resolved alpha, exactly as the sRGB write beside this one uses. Both
/// planes are **premultiplied by that same alpha**, because `tiny_skia`
/// rasterises premultiplied and the compositor un-premultiplies by the alpha
/// it reads back — the identical dance `composite_srgb` already performs.
///
/// ★ `K` is replicated across all three channels rather than parked in one.
/// A single channel would work and would also make a silent failure possible:
/// if the packing and the unpacking ever disagreed about WHICH channel, two
/// out of three reads would return zero — a plausible-looking lighter image
/// rather than an obvious fault. Replicating means any channel is the right
/// channel, so the two sides cannot disagree.
fn write_ink(planes: &mut CmykTexels, at: usize, tint: [f32; 4], alpha: u8) {
    let q = |v: f32| -> u8 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            (v.clamp(0.0, 1.0) * 255.0).round() as u8
        }
    };
    let c = |i: usize| tint.get(i).copied().unwrap_or(0.0);
    let cmy = premultiplied(Rgb::from_rgb(c(0), c(1), c(2)), alpha);
    let k = premultiplied(Rgb::from_rgb(c(3), c(3), c(3)), alpha);
    let _ = q;
    if let Some(slot) = planes.cmy.pixels_mut().get_mut(at) {
        *slot = cmy;
    }
    if let Some(slot) = planes.k.pixels_mut().get_mut(at) {
        *slot = k;
    }
}

/// Row stride in bytes: `ceil(Width × components × BitsPerComponent / 8)`.
///
/// §8.9.3: "each row of the image shall begin on a byte boundary",
/// padded with trailing zero bits. The `ceil` is **per row**, not per
/// image — a 3-pixel-wide 1-bpc image has a 1-byte stride with 5
/// padding bits on every row, and computing it image-wide instead
/// shears the picture diagonally.
///
/// `pub(crate)` for [`crate::mask`], which unpacks `/SMask` and `/Mask`
/// samples under exactly the same §8.9.3 rules. Sharing the function is
/// the point: a mask whose stride was computed differently from the base
/// image's would shear against it, and that is the sort of divergence
/// two copies of "the same" arithmetic produce.
pub(crate) fn row_stride(width: u32, components: usize, bpc: u32) -> Result<usize, ImageError> {
    let bits = u64::from(width)
        .checked_mul(components as u64)
        .and_then(|v| v.checked_mul(u64::from(bpc)))
        .ok_or(ImageError::TooLarge)?;
    usize::try_from(bits.div_ceil(8)).map_err(|_| ImageError::TooLarge)
}

/// Read one `bpc`-wide sample at `bit_offset` bits into `data`.
///
/// §8.9.3 packs "from high-order to low-order bits", and because `bpc`
/// is always 1, 2, 4, 8 or 16 while rows start on byte boundaries, a
/// sample can never straddle a byte boundary for `bpc < 8` — which is
/// what makes this a shift-and-mask rather than a bit-stream reader.
///
/// Out-of-range reads return **0** rather than failing: the caller has
/// already flagged the stream as truncated, and returning a value keeps
/// the surviving majority of the image on the page.
///
/// `pub(crate)` for [`crate::mask`] — see [`row_stride`] for why the
/// mask path shares this rather than restating it.
pub(crate) fn read_sample(data: &[u8], bit_offset: usize, bpc: u32) -> u32 {
    let byte_index = bit_offset / 8;
    let at = |i: usize| u32::from(data.get(i).copied().unwrap_or(0));
    match bpc {
        16 => (at(byte_index) << 8) | at(byte_index + 1),
        8 => at(byte_index),
        // 1, 2, 4: `bit_offset % 8` is always a multiple of `bpc`.
        b => {
            let shift = 8u32.saturating_sub(b + (bit_offset % 8) as u32);
            let mask = (1u32 << b) - 1;
            (at(byte_index) >> shift) & mask
        }
    }
}

/// Read `/Decode` as `(Dmin, Dmax)` pairs, or `None` when absent.
///
/// **Pairs are never normalized.** `Dmin > Dmax` is §8.9.5.2's
/// inversion idiom, not a malformed rectangle — the exact opposite of
/// §7.9.5's rule for `/BBox` and `/MediaBox`, and confusing the two is
/// the named trap in `iso32000__s__8.9.5.2.md`.
///
/// `pub(crate)` for [`crate::mask`]: a soft mask's `/Decode` inverts
/// alpha by the same `Dmin > Dmax` idiom, and a stencil mask's is a
/// polarity switch read from the same pair.
pub(crate) fn decode_pairs(dict: &Dict) -> Option<Vec<(f32, f32)>> {
    let items = dict.get(b"Decode")?.as_array()?;
    if items.len() < 2 {
        return None;
    }
    Some(
        items
            .chunks_exact(2)
            .map(|pair| {
                let lo = pair.first().and_then(Object::as_number).unwrap_or(0.0) as f32;
                let hi = pair.get(1).and_then(Object::as_number).unwrap_or(1.0) as f32;
                (lo, hi)
            })
            .collect(),
    )
}

/// Pack an [`Rgb`] plus alpha into a tiny-skia premultiplied texel.
///
/// ## This is a MULTIPLY, and it has to be
///
/// tiny-skia stores colours **premultiplied**: the stored component is
/// `colour × alpha`, not the colour with an alpha stapled beside it.
/// Before transparency landed, every texel this module produced had
/// `alpha == 255` (a stencil mask's transparent texels being explicitly
/// `TRANSPARENT` rather than routed through here), so the function could
/// get away with `min(round(v × 255), alpha)` — a *clamp*, which is
/// exactly right at `alpha == 255` and exactly right at the extremes
/// `v == 0` and `v == 1`, and wrong everywhere else.
///
/// That is a genuinely nasty shape of bug: mid-grey at half alpha would
/// have come out as `min(128, 128) = 128` — full-strength grey, twice as
/// bright as the correct `0.5 × 128 = 64` — while every pure black, pure
/// white and fully-opaque pixel stayed right. A test over a black-and-
/// white checkerboard would have passed. The fixtures therefore use a
/// deliberately mid-toned ramp (`tools/gen-image-fixtures.py`'s
/// `alpha_at`), which is the only kind of data that can catch it.
///
/// Multiplying by `alpha` (0–255) rather than by 255 and then scaling
/// keeps the type's own invariant — `component ≤ alpha` — true by
/// construction rather than by a `min` that hides the arithmetic error.
/// At `alpha == 255` the result is bit-identical to the old code, so no
/// opaque image moved a single pixel when this changed.
fn premultiplied(c: Rgb, alpha: u8) -> PremultipliedColorU8 {
    let a = f32::from(alpha);
    let q = |v: f32| (v.clamp(0.0, 1.0) * a).round() as u8;
    PremultipliedColorU8::from_rgba(q(c.r), q(c.g), q(c.b), alpha)
        .unwrap_or(PremultipliedColorU8::TRANSPARENT)
}

// ---------------------------------------------------------------------------
// Colour spaces (§8.6, §8.6.6.3)
// ---------------------------------------------------------------------------

/// The image colour spaces this slice converts.
///
/// Deliberately *not* a general colour-space model — that arrives with
/// the `cs`/`sc`/`scn` operators in a later Pass. This is the minimum
/// that covers the overwhelming majority of real images: the three
/// device spaces, their CIE-based aliases handled by the same maths,
/// `ICCBased` through its `N`-component fallback, and `Indexed` over
/// any of those.
///
/// `pub(crate)` for [`crate::mask`], which needs exactly one thing from
/// it: [`Space::components`], to enforce that an `/SMask`'s colour space
/// carries one component per sample.
/// Ceiling on an image colour space's component count.
///
/// Bounds two things at once: the per-pixel component buffer, and a
/// malformed file's ability to make the row-stride arithmetic enormous.
/// A real `DeviceN` is a duotone (2), a hexachrome (6), or occasionally a
/// packaging file with a dozen inks; 32 is comfortably past any of them
/// and matches the guard `crate::shading` puts on `/Function` outputs.
pub(crate) const MAX_IMAGE_COMPONENTS: usize = 32;

/// Memoises [`Space::Special`] conversions on the **pre-`/Decode`
/// integer samples**.
///
/// # Why a cache is load-bearing rather than an optimisation
///
/// A `Separation` or `DeviceN` conversion runs the document's
/// `/tintTransform` — a §7.10 function, which for `FunctionType 4` is a
/// PostScript calculator interpreted per call. A 40-megapixel duotone
/// would run it 40 million times to produce, at most, 65 536 distinct
/// answers. Without memoisation this Pass would trade "the image is
/// missing" for "the page takes a minute", which is not obviously the
/// better failure.
///
/// # Why the key is the RAW samples
///
/// The raw integers are the natural quantisation: two texels with
/// identical samples have identical colour by definition, so the cache is
/// **exact** — it changes speed and nothing else. Keying on the decoded
/// floats instead would need an epsilon, and an epsilon here would be a
/// silent colour approximation of exactly the kind rule 4 forbids.
///
/// The key packs each component into `bits` and requires
/// `components × bits ≤ 64`. That covers 8 channels at 8 bits and 4 at
/// 16 — every duotone, every hexachrome at 8 bits, every `Lab` image.
/// Wider inputs fall back to computing per pixel (correct, slower), which
/// is the honest degradation: a cache that dropped precision to fit would
/// change the picture.
struct TintCache {
    /// Distinct sample tuples seen, and **both** answers each produced:
    /// the sRGB the screen path paints, and the `DeviceCMYK` colorants a
    /// subtractive page composites in, when the space has any.
    ///
    /// ★ Both, from one entry, for the reason [`crate::shading::ColorRamp`]
    /// and [`crate::mesh`]'s vertex reader both state: a `/tintTransform`
    /// may be arbitrary PostScript, and **nothing forces two evaluations of
    /// it to agree**. Caching them separately, or computing one here and
    /// the other in a second pass, would let the same texel's screen colour
    /// and its ink describe different points of the transform.
    ///
    /// It is also what bounds the cost of `Pass 140.0`. The ink answer is a
    /// second §7.10 function evaluation, and on a photograph that would be
    /// per-texel — but this cache already keys on the raw sample tuple, so
    /// it is per **distinct tuple** exactly as the sRGB answer has always
    /// been. Measured on the five-colorant `DeviceN` patch that opened the
    /// Pass: 292 distinct tuples behind 25,870 texels.
    seen: std::collections::HashMap<u64, (Rgb, Option<[f32; 4]>)>,
    /// Bits per component, for packing the key.
    bits: u32,
    /// How many components participate in the key.
    components: usize,
    /// Whether the key fits in 64 bits at all.
    packable: bool,
    /// Diagnostics from the DISTINCT conversions, not from the texels.
    ///
    /// This is why the cache owns them: routed straight from the pixel
    /// loop, `tint_transform_not_applied` would report once per texel and
    /// a shell would print "8 million spot-colour conversions had no tint
    /// transform" for one broken image.
    diag: ColorDiagnostics,
}

impl TintCache {
    fn new(bits: u32, components: usize) -> Self {
        let packable = components > 0 && bits > 0 && (components as u32).saturating_mul(bits) <= 64;
        Self {
            seen: std::collections::HashMap::new(),
            bits,
            components,
            packable,
            diag: ColorDiagnostics::default(),
        }
    }

    /// Pack the raw samples into a key, or `None` when they do not fit.
    fn key(&self, raw: &[u32]) -> Option<u64> {
        if !self.packable {
            return None;
        }
        let mut k = 0u64;
        for v in raw.iter().take(self.components) {
            k = (k << self.bits) | u64::from(*v);
        }
        Some(k)
    }

    /// Both of one texel's colours, computed once per distinct sample
    /// tuple: the sRGB it paints, and the `DeviceCMYK` colorants it lays
    /// down where the space has any (`None` otherwise — see
    /// [`Space::to_cmyk`]).
    fn lookup(
        &mut self,
        space: &Space,
        intent: CmykIntent,
        raw: &[u32],
        comps: &[f32],
    ) -> (Rgb, Option<[f32; 4]>) {
        match self.key(raw) {
            Some(k) => {
                if let Some(hit) = self.seen.get(&k) {
                    return *hit;
                }
                let both = Self::convert(space, intent, comps, &mut self.diag);
                self.seen.insert(k, both);
                both
            }
            None => Self::convert(space, intent, comps, &mut self.diag),
        }
    }

    /// The uncached conversion, in ONE place so the cache-hit path and the
    /// unpackable-key path cannot come to compute different things.
    ///
    /// Both answers are taken from the **same `comps`**, in the same call
    /// — see [`Self::seen`] for why that is a correctness requirement and
    /// not tidiness.
    fn convert(
        space: &Space,
        intent: CmykIntent,
        comps: &[f32],
        diag: &mut ColorDiagnostics,
    ) -> (Rgb, Option<[f32; 4]>) {
        let rgb = space.to_rgb(intent, comps, diag);
        (rgb, space.to_cmyk(comps, diag))
    }
}

/// An `/Indexed` palette's §11.7.4.3 inputs, when its base is a
/// `Separation`/`DeviceN`.
///
/// Held beside the palette rather than derived later because a palette entry
/// is resolved to [`Rgb`] when the table is BUILT — by the time a texel looks
/// one up, the operands the tints must be read from are gone. That is the
/// same argument [`Space::Indexed::ink`] makes, applied to a different read
/// of the same operands.
#[derive(Debug, Clone)]
pub(crate) struct IndexedOverprint {
    /// The base's Table 149 row, classified once at palette-build time.
    pub kind: crate::overprint::SourceKind,
    /// Authored process tints, one per palette entry.
    pub entries: Vec<[f32; 4]>,
    /// The base's spot colorants — name and tint curve — in declaration
    /// order (`Pass 238.0`). Empty when the base names none.
    pub spot_colorants: Vec<(
        std::sync::Arc<[u8]>,
        std::sync::Arc<crate::cmyk_buffer::SpotLut>,
    )>,
    /// Authored spot tints, one row per palette entry, one column per
    /// `spot_colorants` entry. Same index discipline as `entries`.
    pub spot_entries: Vec<Vec<f32>>,
}

#[derive(Debug, Clone)]
pub(crate) enum Space {
    /// `DeviceGray` / `CalGray` / `ICCBased` with `N 1`.
    Gray,
    /// `DeviceRGB` / `CalRGB` / `ICCBased` with `N 3`.
    Rgb,
    /// `DeviceCMYK` / `ICCBased` with `N 4`.
    Cmyk,
    /// `[/Indexed base hival lookup]` (§8.6.6.3). The palette is
    /// resolved to RGB at construction — see [`Space::palette`].
    /// `/Indexed`: the resolved palette, plus the base's **authored ink**
    /// when that base is `DeviceCMYK`.
    ///
    /// ★ The second table exists for the same reason
    /// [`DecodedImage::ink`] does, one level further in. A palette entry is
    /// resolved to `Rgb` when the table is BUILT, so by the time a texel
    /// looks one up the colorants are already gone — and `CMYK -> sRGB` is
    /// many-to-one, so they cannot be recovered from the `Rgb`.
    ///
    /// This is not a hypothetical arrangement: a print-conformance patch
    /// ships `[/Indexed /DeviceCMYK 0 <lookup>]` for a JPEG 2000 image, and
    /// on a subtractive page the same red drawn as a path and as that image
    /// landed two visibly different colours until the ink was carried here.
    ///
    /// `None` for every other base, where there are no colorants to keep.
    Indexed {
        /// Whether the BASE space was colour-managed when the palette was
        /// built (`Pass 214.0`).
        ///
        /// ★ Recorded here because it cannot be recovered later. This variant
        /// keeps resolved TABLES, not the base space, so by the time anything
        /// asks "was this image managed?" the base is gone — and an
        /// `/Indexed` over `ICCBased` is exactly the shape the conformance
        /// corpus uses. Without this the disclosure counter reported such an
        /// image as UNMANAGED while its palette had in fact been converted
        /// through the profile, which is the same blind spot `Pass 207.0`
        /// fixed one level up, one level further in.
        base_icc_managed: bool,
        /// The palette, resolved to sRGB — what the screen path reads.
        table: Vec<Rgb>,
        /// The same entries' `DeviceCMYK` colorants, index for index.
        ink: Option<Vec<[f32; 4]>>,
        /// The same entries' §11.7.4.3 **authored process tints**, index for
        /// index, when the base is a `Separation`/`DeviceN`.
        ///
        /// A third parallel table rather than a reuse of `ink`, for the
        /// reason [`DecodedImage::overprint`] states at length: "what ink
        /// does this entry lay down" and "which process tints did the file's
        /// operands name" are different questions with different answers for
        /// a spot colorant, and collapsing them makes a spot image paint
        /// nothing. Built in the same loop as the other two so all three
        /// cannot come to disagree about which index means what.
        overprint: Option<IndexedOverprint>,
    },
    /// Any space this rasterizer does not decode itself, delegated whole
    /// to [`crate::color::ColorSpace`]: `Separation`, `DeviceN`, `Lab`,
    /// `CalGray` and `CalRGB`.
    ///
    /// # Why delegate rather than add four more arms here
    ///
    /// [`crate::color`] already parses every one of these, already
    /// evaluates a `/tintTransform` through [`pdfcer_core::function`], and
    /// already knows `/All` and `/None`. Re-implementing them here would
    /// put a **second** answer to "what colour is this tint?" in the
    /// binary — and the two would be reached by different content (a
    /// filled rectangle versus an image), so a divergence would show up
    /// as *the same spot colour printing two different ways on one page*.
    /// That is the exact failure `pdfcer_core::function` was centralised to
    /// prevent, stated in [`crate::color`]'s own module docs.
    ///
    /// The cost is that conversion is no longer a closed-form arithmetic
    /// step — a `Separation` runs a §7.10 function per distinct sample
    /// tuple — which is what [`TintCache`] exists to bound.
    Special {
        /// The delegated space.
        cs: std::sync::Arc<crate::color::ColorSpace>,
        /// The output intent's separation engine, when the space is a CIE
        /// family (`Lab`, `CalRGB`, `CalGray`) and the document names an
        /// output device (`Pass 242.0`). `None` for every other delegated
        /// space, and for a CIE space on a document with no intent.
        ///
        /// ★ Resolved at construction rather than looked up per texel for
        /// the same reason `Icc`'s bridge is: the cache lives on the
        /// interpreter and a `Space` outlives the decode call. With it, a
        /// `Lab` image and a `Lab` fill of one colour separate through ONE
        /// chain -- the fill through `Interpreter::authored_cmyk`, the image
        /// through [`Space::to_cmyk`] -- rather than one through the
        /// profile and the other through `rgb_to_cmyk`.
        pcs: Option<std::sync::Arc<crate::icc::PcsBridge>>,
    },
    /// `[/ICCBased stream]` where the document ALSO named an output device,
    /// so the samples can actually be colour-managed (`Pass 214.0`).
    ///
    /// # Why this is a distinct variant rather than a field on the others
    ///
    /// [`Self::Gray`], [`Self::Rgb`] and [`Self::Cmyk`] each document
    /// themselves as *"`ICCBased` with `N 1`/`N 3`/`N 4`"* — the profile was
    /// parsed for its `/N` and thrown away, and every `ICCBased` image in the
    /// corpus took the unmanaged path as a result. Widening those three
    /// variants would have touched every match arm in this file for a case
    /// that only arises when a bridge could be built; a separate variant
    /// leaves all of them exactly as they were.
    ///
    /// ★ IT IS ONLY CONSTRUCTED WHEN A BRIDGE EXISTS. No output intent, an
    /// unparseable profile, or a destination that is not four-component, and
    /// resolution falls back to the device space by `/N` — today's behaviour,
    /// bit for bit. So this variant cannot regress a file it does not apply
    /// to.
    ///
    /// # What it changes, and what it deliberately does not
    ///
    /// [`Self::to_cmyk`] runs the transform. [`Self::to_rgb`] does **not** —
    /// it delegates to the device fallback, so the sRGB path does not move.
    /// That is the same split `Pass 199.2` used for path fills, and it is what
    /// keeps the change confined to pages that composite in ink.
    Icc {
        /// Table 66 `/N`. Drives the sample width exactly as before.
        n: usize,
        /// The built source-to-`/OutputIntent` transform.
        bridge: std::sync::Arc<crate::icc::IccBridge>,
    },
    /// `[/ICCBased stream]` with `N 3`, colour-managed **to the screen**
    /// through its own embedded profile (`Pass 240.0`).
    ///
    /// # Why a third variant, and why it is not [`Self::Icc`] with `n: 3`
    ///
    /// [`Self::Icc`] carries one bridge, to the document's `/OutputIntent`,
    /// and its [`Self::to_rgb`] deliberately does not move — a CMYK image's
    /// screen answer stays `Rgb::from_cmyk`. This variant carries **two**:
    /// a display bridge to iccce's constructed sRGB, which is always built
    /// and answers [`Self::to_rgb`]; and an ink bridge to the output intent,
    /// built only when the document names one, which answers
    /// [`Self::to_cmyk`]. On an additive page the first is the whole story.
    /// On a subtractive page the second deposits the same ink the VECTOR
    /// path deposits for the same colour through the same profile
    /// (`Interpreter::authored_cmyk`, `Pass 199.2`) — which is what makes a
    /// fill and an image of one colour land on one pixel value, and is the
    /// conformance patch's exact pass criterion.
    ///
    /// ★★ ON THE "MEASURED NEGATIVE" THAT SAID NOT TO DO THIS. The 2026-09-02
    /// hand-off (`docs/NEXT_SESSION.md` §D item 1) recorded routing an `N 3`
    /// image onto the ink path as **3× worse**, and that number is real —
    /// but it measured a DEFECT, not the route. Until this Pass a direct
    /// [`Self::Icc`] image was outside the `tinting` route, so the texel
    /// loop's ink arm wrote `last_comps` — the image's RAW components — as
    /// C, M, Y, K. For `N 4` that was silently unmanaged ink under a
    /// `icc_managed: true` flag; for `N 3` it was three RGB values written as
    /// three inks, which is the 3×. The bridge's output was computed by the
    /// `yields_cmyk` probe and never once per texel. With `Icc` and this
    /// variant both on the cached route, the ink written IS the bridge's,
    /// and the same image measured on the same patch lands on its vector
    /// twin. The negative is retracted by the measurement that made it.
    ///
    /// # What it fixes
    ///
    /// A conformance patch draws one colour four ways through one embedded
    /// RGB profile: as a vector fill and as an `/Indexed`-over-`ICCBased`
    /// image, each in RGB and in CMYK. Before this variant the RGB IMAGE
    /// cell fell to [`Self::Rgb`] — its profile parsed for `/N` and thrown
    /// away — and rendered a red trap X against the vector cell's green.
    /// The profile is deliberately not sRGB, so "reinterpret as
    /// `DeviceRGB`" is exactly the wrong answer the patch exists to catch.
    ///
    /// # What is deliberately NOT managed this way
    ///
    /// * `N 4` on an additive page: `Rgb::from_cmyk` with the operator's
    ///   `cmyk_intent` stays. Rewiring a CMYK→sRGB terminal conversion to a
    ///   profile chain was measured worse (§D item 2 of the same note), and
    ///   an embedded CMYK profile to sRGB is the same class of transform.
    /// * `N 1`: unmeasured. A gray profile's TRC against `Rgb::from_gray`
    ///   would move every ICC-gray image on every additive page by an
    ///   unquantified amount, and no patch fails on it. It is a one-line
    ///   widening of the `components == 3` test in [`resolve_space_array`] when a
    ///   measurement asks for it.
    ///
    /// Only constructed when the profile parses and models; otherwise the
    /// `/N` fallback runs exactly as before, so a file this does not apply
    /// to renders as it did.
    IccRgb {
        /// The built source-to-sRGB transform. Three components in, three
        /// encoded sRGB components out. Always present: the variant is not
        /// constructed without it.
        display: std::sync::Arc<crate::icc::IccBridge>,
        /// The built source-to-`/OutputIntent` transform, when the document
        /// named an output device. `None` on an ordinary document, where
        /// [`Self::to_cmyk`] answers `None` and a subtractive page bridges
        /// the managed sRGB with `rgb_to_cmyk` exactly as it does for
        /// `DeviceRGB`.
        ink: Option<std::sync::Arc<crate::icc::IccBridge>>,
    },
}

impl Space {
    /// Number of colour components a *sample* carries.
    ///
    /// For `Indexed` this is **1** — the index — not the base space's
    /// count. That distinction drives the row stride, the `Decode`
    /// array length, and the predictor's `/Colors`, and getting it
    /// wrong shears the image (`color__indexed.md`).
    pub(crate) fn components(&self) -> usize {
        match self {
            Self::Gray | Self::Indexed { .. } => 1,
            Self::Rgb => 3,
            Self::Cmyk => 4,
            // A `DeviceN` carries one component per colorant name, so this
            // is the ONLY space whose sample width is not fixed by its
            // family. It drives the row stride and the `/Decode` length,
            // so a wrong answer here shears the image rather than
            // discolouring it.
            Self::Special { cs, .. } => cs.components(),
            Self::Icc { n, .. } => *n,
            Self::IccRgb { .. } => 3,
        }
    }

    /// Table 90's default `Decode` array for this space.
    ///
    /// `[0 1]` per component for the device spaces; **`[0 2ⁿ−1]`** for
    /// `Indexed`, which makes the transform the identity so raw samples
    /// pass through as palette indices unchanged (§8.9.5.2 NOTE 2).
    /// `ICCBased`'s true default is the profile's `Range`, which pdfcer
    /// does not parse; `[0 1]` per component is the documented
    /// `N`-fallback approximation (`color__iccbased.md`) and is correct
    /// for every profile whose range is the usual 0–1.
    fn default_decode(&self, max_sample: f32) -> Vec<(f32, f32)> {
        match self {
            Self::Indexed { .. } => vec![(0.0, max_sample)],
            // ★ NOT `[0 1]` per component. Table 90's default is the
            // space's own component RANGE, and `Lab` is the case where
            // that is not 0–1: its L runs 0–100 and its a/b run over the
            // `/Range` array's values, which are routinely negative.
            // Defaulting a `Lab` image to `[0 1]` would collapse every
            // sample into the darkest corner of the space and paint a
            // near-black picture — plausible enough to be mistaken for a
            // badly exposed scan rather than for a decode bug.
            Self::Special { cs, .. } => (0..cs.components())
                .map(|i| cs.component_range(i))
                .collect(),
            _ => vec![(0.0, 1.0); self.components()],
        }
    }

    /// Each component's `(lo, hi)` range — what a palette byte or a decoded
    /// sample is scaled into. `0..1` for every family but the delegated
    /// ones, where `Lab`'s L\* is `0..100` and its a\*/b\* follow `/Range`.
    fn component_ranges(&self) -> Vec<(f32, f32)> {
        match self {
            Self::Special { cs, .. } => (0..cs.components())
                .map(|i| cs.component_range(i))
                .collect(),
            _ => vec![(0.0, 1.0); self.components()],
        }
    }

    /// The resolved palette, for `Indexed` only.
    fn palette(&self) -> Option<&[Rgb]> {
        match self {
            Self::Indexed { table, .. } => Some(table),
            _ => None,
        }
    }

    /// Convert decoded components (already clamped into range) to RGB.
    ///
    /// `diag` is threaded because [`Self::Special`] delegates to
    /// [`crate::color::ColorSpace::to_rgb`], which counts its own
    /// shortfalls (a missing `/tintTransform`, a `/Separation /All`
    /// approximation). Callers in a per-pixel loop must route through
    /// [`TintCache`] rather than calling this directly — otherwise those
    /// counters would tick once per texel and report millions.
    fn to_rgb(&self, intent: CmykIntent, comps: &[f32], diag: &mut ColorDiagnostics) -> Rgb {
        let c = |i: usize| comps.get(i).copied().unwrap_or(0.0);
        match self {
            // `None` here means the space paints nothing at all —
            // `/Separation /None`, or an all-`/None` `DeviceN`
            // (§8.6.6.4/.5, "shall never be painted on the page").
            //
            // The colour returned is irrelevant BECAUSE THE ALPHA IS
            // ZERO: the decoder sets `suppressed` from the same
            // `paints()` query and forces every texel transparent. Black
            // is chosen over white deliberately — if the alpha path were
            // ever bypassed, a black block is an obvious defect that gets
            // reported, whereas white is invisible on the blank page a
            // test most likely uses and silently erases content on a real
            // one. Fail loudly, not plausibly.
            Self::Special { cs, .. } => cs.to_rgb(comps, intent, diag).unwrap_or(Rgb::BLACK),
            // An Indexed space never reaches here — the palette path
            // short-circuits it — but returning grey rather than
            // panicking keeps this total.
            Self::Gray | Self::Indexed { .. } => Rgb::from_gray(c(0)),
            Self::Rgb => Rgb::from_rgb(c(0), c(1), c(2)),
            // The same calibrated conversion the `k`/`K` operators use —
            // one function in `pdfcer_core::color`, reached through the
            // same `Rgb` constructor — so an image and a filled rectangle
            // of the "same" CMYK agree on screen by construction rather
            // than by two formulas being kept in step (gstate.rs docs).
            Self::Cmyk => Rgb::from_cmyk(intent, c(0), c(1), c(2), c(3)),
            // ★ THE sRGB PATH DELIBERATELY DOES NOT MOVE. An `Icc` space
            // renders on screen exactly as the device space its `/N` implies,
            // which is what it did before this variant existed.
            //
            // Only `to_cmyk` is colour-managed. That confines the change to
            // pages that composite in ink -- where a wrong conversion is
            // measurably wrong against a reference -- and keeps every additive
            // page byte-identical, including the parity fixtures that pin the
            // quantised sRGB output. Same split `Pass 199.2` used for path
            // fills, and for the same reason: two coexisting answers, neither
            // derived from the other.
            Self::Icc { n, .. } => match n {
                1 => Rgb::from_gray(c(0)),
                4 => Rgb::from_cmyk(intent, c(0), c(1), c(2), c(3)),
                _ => Rgb::from_rgb(c(0), c(1), c(2)),
            },
            // ★ THE DISPLAY ROUTE (`Pass 240.0`): the profile's own answer
            // for what these three numbers look like on an sRGB screen. The
            // fallback is Table 66's reinterpretation, reached only if the
            // bridge refuses the width -- which `resolve_space_array` made
            // impossible by building it from the same `/N`.
            //
            // Per-texel cost is bounded by `TintCache`: `tinting` is true for
            // this variant, so the chain runs once per DISTINCT sample tuple.
            Self::IccRgb { display, .. } => display
                .convert_to_rgb(comps)
                .unwrap_or_else(|| Rgb::from_rgb(c(0), c(1), c(2))),
        }
    }

    /// The **`DeviceCMYK` colorants** these components lay down, when this
    /// space has any — the ink answer that sits beside [`Self::to_rgb`]'s
    /// screen answer.
    ///
    /// # Why this exists at all, and what it is NOT
    ///
    /// A subtractive page composites in a four-colorant buffer
    /// ([`crate::cmyk_buffer`]). Anything that reaches that buffer as sRGB
    /// has been through a **many-to-one** conversion and its colorants can
    /// no longer be recovered — that is what `cmyk_bridged_pixels` counts,
    /// and a bridged image comes out visibly desaturated against a reader
    /// that kept the ink. `Pass 130.1` gave a `DeviceCMYK` image its
    /// colorants; this method is what lets a `Separation`/`DeviceN` image
    /// keep its own (`Pass 140.0`).
    ///
    /// ★★ It is **not** Table 149's question and must never be confused
    /// with it. [`crate::overprint::authored_tints`] answers *"which
    /// components did the source SPECIFY"* — a question about the operands
    /// — and returns `None` for a spot-only `DeviceN`, because a spot
    /// colorant specifies no process component at all. This method answers
    /// *"what does the tint transform PRODUCE in the alternate space"*, and
    /// for that same spot-only `DeviceN` it returns the flattened process
    /// approximation, which is exactly what a plain (non-overprinting)
    /// render must paint. Reusing the overprint planes here would paint
    /// bare white paper; see this Pass's `ROADMAP.md` entry, where the trap
    /// is written out at length.
    ///
    /// # The arms
    ///
    /// * [`Self::Cmyk`] — the components themselves.
    /// * [`Self::Special`] — delegated whole to
    ///   [`crate::color::ColorSpace::to_cmyk`], which returns `Some` only
    ///   for a `Separation`/`DeviceN` whose alternate **is** `DeviceCMYK`,
    ///   and takes the tint transform's own output before anything converts
    ///   it. `Lab`, `CalGray` and `CalRGB` have no colorants and answer
    ///   `None`.
    /// * [`Self::Gray`] / [`Self::Rgb`] — `None`, and deliberately: a grey
    ///   *could* be mapped to `K`, but the file did not say so, and
    ///   claiming components a document never named is the exact error
    ///   [`crate::color::ColorSpace::to_cmyk`] refuses to make.
    /// * [`Self::Indexed`] — `None`. §8.6.6.3 puts the colour values in the
    ///   BASE space, so asking the *index* for colorants is a category
    ///   error. The palette carries its entries' ink in
    ///   [`Self::Indexed::ink`], built by [`resolve_indexed`] **through
    ///   this same method applied to the base**, so both routes have one
    ///   answer rather than two.
    ///
    /// `None` is an honest answer, never a failure: the caller falls back
    /// to the [`Self::to_rgb`] bridge and the shortfall is disclosed by
    /// `cmyk_bridged_pixels`.
    fn to_cmyk(&self, comps: &[f32], diag: &mut ColorDiagnostics) -> Option<[f32; 4]> {
        let c = |i: usize| comps.get(i).copied().unwrap_or(0.0);
        match self {
            Self::Cmyk => Some([c(0), c(1), c(2), c(3)]),
            // A `Separation`/`DeviceN` answers through its alternate; a CIE
            // space answers `None` there and takes the PCS route when the
            // document named an output device (`Pass 242.0`). Both in one
            // arm so the two cannot come to disagree about which applies.
            Self::Special { cs, pcs } => cs.to_cmyk(comps, diag).or_else(|| {
                let bridge = pcs.as_ref()?;
                bridge.to_ink(cs.to_pcs_xyz(comps)?)
            }),
            // ★★★ THE COLOUR-MANAGED ROUTE, `Pass 214.0`.
            //
            // This one arm is the whole image fix, and it is one arm because
            // BOTH image routes come through here: a direct `ICCBased` image
            // asks per sample, and an `/Indexed` one asks once per palette
            // entry at table-build time via the base space. Putting the
            // transform anywhere else would have needed it in two places.
            //
            // Returning `Some` also flips `yields_cmyk`, which is the gate
            // that allocates `DecodedImage::ink` -- so an ICC image starts
            // carrying authored ink and stops being bridged through sRGB by
            // `rgb_to_cmyk`, for the same reason and by the same mechanism a
            // `DeviceCMYK` image did in `Pass 130.1`.
            //
            // `None` on a width or destination mismatch, which falls back to
            // the unmanaged path rather than writing a wrong-width result.
            Self::Icc { bridge, .. } => bridge.convert_components(comps),
            // The ink half of `IccRgb` (`Pass 240.0`): the output-intent
            // bridge when the document named one, so an RGB image deposits
            // the same managed ink its vector twin does. `None` otherwise,
            // and then the page bridges the managed sRGB like `DeviceRGB`.
            Self::IccRgb { ink, .. } => ink.as_ref().and_then(|b| b.convert_components(comps)),
            Self::Gray | Self::Rgb | Self::Indexed { .. } => None,
        }
    }

    /// Whether this space can produce colorants **at all** — the
    /// allocation gate for [`DecodedImage::ink`]'s two texel-sized planes.
    ///
    /// # ★ Why this is a PROBE and not a structural predicate
    ///
    /// The obvious implementation is a `matches!` over the space's shape:
    /// *"`Cmyk`, or a `Separation`/`DeviceN` with a `DeviceCMYK`
    /// alternate."* That would be a **second answer** to a question
    /// [`Self::to_cmyk`] already answers, reached by a different caller,
    /// and this module's whole colour design exists to avoid exactly that
    /// (see [`Self::Special`]'s own docs: one answer to "what colour is
    /// this tint?", or the same spot colour prints two ways on one page).
    /// Two predicates that disagree would allocate planes nothing fills,
    /// or skip planes a texel then wants.
    ///
    /// So it asks the real function, once, with an all-zero operand. The
    /// question is **structural** — a `Separation` over `DeviceCMYK` has a
    /// CMYK answer for every input or for none — so any operand serves,
    /// and zeros need no knowledge of the space's component ranges.
    ///
    /// # What a wrong answer costs, in each direction
    ///
    /// * **False negative** (a transform that happens to fail at zero):
    ///   the image bridges through sRGB exactly as it did before this
    ///   Pass, and `cmyk_bridged_pixels` says so. Status quo, disclosed.
    /// * **False positive**: the planes are allocated and the per-texel
    ///   [`Self::to_cmyk`] returns `None` somewhere, which the decode loop
    ///   treats as all-or-nothing — it drops the planes for the WHOLE
    ///   image rather than leaving a seam where half of it kept its ink.
    ///   That is [`crate::shading::ColorRamp`]'s rule, applied here for the
    ///   same reason: a boundary no file asked for is worse than a uniform
    ///   approximation.
    ///
    /// Neither direction can paint the wrong colour, which is what makes a
    /// probe acceptable where a guess would not be.
    fn yields_cmyk(&self, diag: &mut ColorDiagnostics) -> bool {
        let probe = vec![0.0f32; self.components()];
        self.to_cmyk(&probe, diag).is_some()
    }
}

/// Both bridges for a three-component profile: the display one, without
/// which the caller does not build [`Space::IccRgb`] at all, and the ink one,
/// which is `None` on a document with no output intent.
///
/// One function for the two places a profile can arrive from — the
/// dictionary's `[/ICCBased stream]` and a JPX codestream's own `colr` box —
/// so the two cannot come to build different pairs. The ink half rides on
/// the same profile bytes and the same intent; `IccBridgeCache::get` refuses
/// without a destination, so nothing is invented on an ordinary document.
type IccRgbBridges = (
    std::sync::Arc<crate::icc::IccBridge>,
    Option<std::sync::Arc<crate::icc::IccBridge>>,
);
fn icc_rgb_bridges(
    cache: &crate::icc::IccBridgeCache,
    profile: &[u8],
    intent: pdfcer_core::color::RenderingIntent,
) -> Option<IccRgbBridges> {
    let profile: std::sync::Arc<[u8]> = std::sync::Arc::from(profile);
    let display = cache.get_srgb(&profile, 3, intent)?;
    let ink = cache.get(&profile, 3, intent);
    Some((display, ink))
}

/// The colour space a JPX codestream supplies when `/ColorSpace` is
/// absent — §7.4.9's fallback ladder, terminal rung.
///
/// Table 89: "If `ColorSpace` is absent, the colour space
/// specifications in the JPEG2000 data shall be used." §7.4.9 spells out
/// what "used" means when the codestream's specification is not one a
/// reader supports: "the next lower colour space … shall be used", and
/// "**if no supported colour space is found, the colour space used shall
/// be `DeviceGray`, `DeviceRGB`, or `DeviceCMYK`, depending on …
/// whether the number of channels in the JPEG2000 data is 1, 3, or
/// 4**."
///
/// `pdfcer_core::image_codec::jpx` walks the upper rungs (enumerated
/// spaces, and an embedded ICC profile's own data-colour-space
/// signature) and reports the result as a [`CodecColorModel`]. This
/// function is the last step: turning that into one of the three device
/// spaces this rasterizer converts.
///
/// # Errors
///
/// [`ImageError::UnsupportedColorSpace`] for a channel count with no PDF
/// device-space mapping, and for an ICC profile whose colour space pdfcer
/// cannot approximate as a device space (a `Lab ` profile's samples are
/// not device components; painting them as RGB would be a plausible-
/// looking, entirely wrong picture). Refusing is the `fuzzy, never
/// sneaky` outcome — nothing is drawn and the caller counts it.
fn codestream_space(coded: &CodedImage, icc: IccContext<'_>) -> Result<Space, ImageError> {
    match coded.color_model {
        CodecColorModel::Gray => Ok(Space::Gray),
        // ★ A three-channel codestream that carries its OWN ICC profile is
        // §7.4.9's higher rung, not the `DeviceRGB` terminal one: "the colour
        // space specifications in the JPEG2000 data shall be used", and an
        // embedded profile IS such a specification. Managed to the screen
        // exactly as a dictionary `[/ICCBased <N 3>]` is (`Pass 240.0`);
        // when the profile will not model, or the caller declined
        // management, the terminal rung below runs as before.
        //
        // ★ Deliberately unmeasured against the conformance suite: every
        // JPX patch there names a `/ColorSpace`, which Table 89 makes win
        // over the codestream, so this rung is reached by none of them.
        // It is here because the rule is the same rule, not because a
        // patch asked for it.
        CodecColorModel::Rgb
            if let Some(cache) = icc.cache
                && let Some(profile) = coded.icc_profile.as_deref()
                && let Some((display, ink)) = icc_rgb_bridges(cache, profile, icc.intent) =>
        {
            Ok(Space::IccRgb { display, ink })
        }
        CodecColorModel::Rgb | CodecColorModel::Untransformed3 => Ok(Space::Rgb),
        CodecColorModel::Cmyk => Ok(Space::Cmyk),
        // Neither can reach here: `Bilevel` belongs to the fax codecs,
        // whose `/ColorSpace` is Required, and `Unspecified` means no
        // codec ran at all. Mapped rather than unreachable! so this
        // stays total (`pdfcer-core` denies `panic!` for the same
        // reason).
        CodecColorModel::Bilevel => Ok(Space::Gray),
        // `Unspecified`, `Unknown { .. }`, and — because
        // `CodecColorModel` is `#[non_exhaustive]` — any model a future
        // codec adds before this function learns about it. Refusing an
        // unrecognized model is the only safe default: the alternative
        // is painting samples of unknown meaning as if they were RGB.
        _ => Err(ImageError::UnsupportedColorSpace(
            "the JPEG2000 codestream's colour space".into(),
        )),
    }
}
/// What an image decode needs in order to colour-manage an `ICCBased` source.
///
/// # Why a struct rather than two parameters
///
/// Because they are one dependency, not two, and separating them invites the
/// bug. The cache decides *whether* a transform can be built; the rendering
/// intent decides *which* transform — and a cache lookup keyed on the wrong
/// intent returns a valid `Chain` that silently answers a different question.
/// Standing rule `R237` is exactly that defect in a different memo, so the two
/// travel together and cannot be passed independently.
///
/// `None` for the cache is the ordinary case and means "do not colour-manage",
/// which is what every caller did before `Pass 214.0`.
#[derive(Clone, Copy)]
pub struct IccContext<'a> {
    /// The page's built transforms, or `None` to decline colour management.
    ///
    /// PRIVATE, and that is the honest surface rather than an oversight: an
    /// outside caller has no cache to supply — one is built per page by the
    /// interpreter — so [`Self::unmanaged`] is the only context it could
    /// construct anyway. Exposing the field would publish
    /// `IccBridgeCache` for no reachable benefit.
    cache: Option<&'a crate::icc::IccBridgeCache>,
    /// §8.6.5.8's intent, from the GRAPHICS STATE rather than the profile —
    /// `ri` and `/RI` override a profile's default, so reading the profile's
    /// would make the operator's `ri` a no-op.
    intent: pdfcer_core::color::RenderingIntent,
}

impl<'a> IccContext<'a> {
    /// Build a context that WILL colour-manage, from a page's cache.
    ///
    /// Crate-internal because the cache is: only the interpreter has one.
    pub(crate) fn managed(
        cache: &'a crate::icc::IccBridgeCache,
        intent: pdfcer_core::color::RenderingIntent,
    ) -> Self {
        Self {
            cache: Some(cache),
            intent,
        }
    }

    /// Every bridge `space` can have under this context — empty when the
    /// context declines management (`Pass 243.0`, for the shading and mesh
    /// readers, which convert through the bundle rather than the bare
    /// space).
    #[must_use]
    pub fn bridges_for(&self, space: &crate::color::ColorSpace) -> crate::icc::ColorBridges {
        match self.cache {
            Some(cache) => cache.bridges_for(space, self.intent),
            None => crate::icc::ColorBridges::none(),
        }
    }

    /// The context that declines colour management, for callers that have no
    /// document-level destination — tests, and the geometry-only paths.
    pub fn unmanaged() -> Self {
        Self {
            cache: None,
            intent: pdfcer_core::color::RenderingIntent::RelativeColorimetric,
        }
    }
}

/// Resolve a `/ColorSpace` value to a [`Space`].
///
/// Handles the four shapes §8.6/§8.9.7 allow in an image: a device
/// name, a name referring to the resource dictionary's `/ColorSpace`
/// subdictionary, an `[/ICCBased stream]` array, and an
/// `[/Indexed base hival lookup]` array. `depth` guards the nesting
/// (`Indexed` over `ICCBased` is two levels; a self-referential named
/// resource is unbounded).
///
/// `pub(crate)` for [`crate::mask`]: a soft mask has its own
/// `/ColorSpace`, resolved by the same rules (including the named-
/// resource hop), and only then checked for the single-component
/// constraint §8.9.5 Table 89 puts on it.
pub(crate) fn resolve_space(
    doc: &DocumentView<'_>,
    obj: &Object,
    resources: &Dict,
    depth: usize,
    intent: CmykIntent,
    icc: IccContext<'_>,
) -> Result<Space, ImageError> {
    if depth > MAX_COLORSPACE_DEPTH {
        return Err(ImageError::UnsupportedColorSpace(
            "colour space nested too deeply".into(),
        ));
    }
    match obj {
        Object::Name(n) => match n.as_bytes() {
            b"DeviceGray" | b"CalGray" | b"G" => Ok(Space::Gray),
            b"DeviceRGB" | b"CalRGB" | b"RGB" => Ok(Space::Rgb),
            b"DeviceCMYK" | b"CMYK" => Ok(Space::Cmyk),
            // §7.8.3: any other name is a key in the resource
            // dictionary's `/ColorSpace` subdictionary.
            other => {
                let entry = resources
                    .get(b"ColorSpace")
                    .map(|o| doc.resolve(o))
                    .and_then(Object::as_dict)
                    .and_then(|cs| cs.get(other))
                    .map(|o| doc.resolve(o));
                match entry {
                    Some(inner) => resolve_space(doc, inner, resources, depth + 1, intent, icc),
                    None => Err(ImageError::UnsupportedColorSpace(format!(
                        "/{}",
                        String::from_utf8_lossy(other)
                    ))),
                }
            }
        },
        Object::Array(items) => resolve_space_array(doc, items, resources, depth, intent, icc),
        _ => Err(ImageError::UnsupportedColorSpace(
            "/ColorSpace is neither a name nor an array".into(),
        )),
    }
}

/// The array forms of `/ColorSpace`.
fn resolve_space_array(
    doc: &DocumentView<'_>,
    items: &[Object],
    resources: &Dict,
    depth: usize,
    intent: CmykIntent,
    icc: IccContext<'_>,
) -> Result<Space, ImageError> {
    let family = items
        .first()
        .map(|o| doc.resolve(o))
        .and_then(Object::as_name)
        .map(|n| n.as_bytes().to_vec())
        .unwrap_or_default();

    match family.as_slice() {
        // A one-element array is just the name (`[/DeviceRGB]`), which
        // real producers emit.
        _ if items.len() == 1 => {
            let name = Object::Name(pdfcer_core::object::Name(family));
            resolve_space(doc, &name, resources, depth + 1, intent, icc)
        }
        // `[/ICCBased stream]` — §8.6.5.5. pdfcer does not parse ICC
        // profiles; the spec's own fallback is the stream's `/N`
        // (1 → Gray, 3 → RGB, 4 → CMYK), which is exactly what
        // `/Alternate` would default to (`color__iccbased.md`).
        b"ICCBased" => {
            let n = items
                .get(1)
                .map(|o| doc.resolve(o))
                .and_then(Object::as_dict)
                .and_then(|d| d.get(b"N"))
                .map(|o| doc.resolve(o))
                .and_then(Object::as_int);
            // ★★★ COLOUR-MANAGE IT WHEN BOTH ENDS EXIST, `Pass 214.0`.
            //
            // Before this, an `ICCBased` image was collapsed to a device space
            // by `/N` and its profile discarded -- so every ICC image in the
            // corpus rendered UNMANAGED while the graphics-state path beside
            // it was managed. Measured on a conformance patch that draws the
            // same colour through the same embedded profile four ways: the two
            // VECTOR cells landed within a level of correct and the two IMAGE
            // cells reproduced the unmanaged answer bit-for-bit.
            //
            // The fallback below is untouched and still runs whenever a bridge
            // cannot be built -- no output intent, an unparseable profile, a
            // non-CMYK destination. So a file this does not apply to renders
            // exactly as it did.
            if let Some(cache) = icc.cache
                && let Some(components) = n.and_then(|v| usize::try_from(v).ok())
                && components == 4
                && let Object::Stream(st) = doc.resolve(&items[1])
                && let Some(raw) = doc.slice(st.data_span)
                && let Ok(profile) = pdfcer_core::filters::decode_stream(&st.dict, raw)
                && let Some(bridge) =
                    cache.get(&std::sync::Arc::from(profile), components, icc.intent)
            {
                return Ok(Space::Icc {
                    n: components,
                    bridge,
                });
            }
            // ★★★ AND THE DISPLAY ROUTE FOR `N 3`, `Pass 240.0`.
            //
            // A three-component profile goes to the SCREEN, not to the output
            // intent -- `Space::IccRgb`'s docs carry the measurement that
            // chose the route. Two things differ from the arm above, both
            // deliberate:
            //
            //   * no `has_destination` requirement. The destination is sRGB by
            //     construction, so this manages on an ordinary document with
            //     no `/OutputIntent` at all -- which is where an RGB photo
            //     tagged with a non-sRGB profile actually lives.
            //   * the `/Indexed` base takes the same arm, because
            //     `resolve_indexed` resolves its base through this function
            //     and builds the palette with `to_rgb`. The conformance
            //     patch's image is exactly that shape.
            //
            // The `cache` gate is kept: `IccContext::unmanaged()` means the
            // caller declined colour management, and that is honoured on this
            // route as on the other.
            if let Some(cache) = icc.cache
                && let Some(components) = n.and_then(|v| usize::try_from(v).ok())
                && components == 3
                && let Object::Stream(st) = doc.resolve(&items[1])
                && let Some(raw) = doc.slice(st.data_span)
                && let Ok(profile) = pdfcer_core::filters::decode_stream(&st.dict, raw)
                && let Some((display, ink)) = icc_rgb_bridges(cache, &profile, icc.intent)
            {
                return Ok(Space::IccRgb { display, ink });
            }
            match n {
                Some(1) => Ok(Space::Gray),
                Some(3) => Ok(Space::Rgb),
                Some(4) => Ok(Space::Cmyk),
                _ => Err(ImageError::UnsupportedColorSpace(
                    "/ICCBased without a usable /N".into(),
                )),
            }
        }
        // `[/Indexed base hival lookup]` — §8.6.6.3. Kept HERE rather
        // than delegated, because an image's `Indexed` space is not a
        // per-sample conversion at all: the sample IS the palette index,
        // the width of a sample is the index width and not the base
        // space's component count, and the whole palette is resolved once
        // at construction. `crate::color`'s `Indexed` answers a different
        // question (what colour is index N) and would give the row stride
        // the wrong number of components.
        b"Indexed" | b"I" => resolve_indexed(doc, items, resources, depth, intent, icc),
        // Everything else `crate::color` knows how to parse — the two
        // this Pass exists for (`Separation`, `DeviceN`) and the three
        // that came free with them (`Lab`, `CalGray`, `CalRGB`).
        //
        // Before this, EVERY one of these was an outright refusal and the
        // image was dropped from the raster entirely. On the operator's
        // suite X-4 file that was 18 pictures across three pages, which is
        // the largest single hole this crate had.
        b"Separation" | b"DeviceN" | b"Lab" | b"CalGray" | b"CalRGB" => {
            let obj = Object::Array(items.to_vec());
            let mut scratch = ColorDiagnostics::default();
            match crate::color::resolve_object(doc, &obj, resources, depth, &mut scratch) {
                Some(cs) if cs.components() > 0 && cs.components() <= MAX_IMAGE_COMPONENTS => {
                    // The PCS route for the three CIE families, resolved
                    // once here. `pcs_bridge` answers `None` without a
                    // destination, so an ordinary document is unaffected.
                    let pcs = match (icc.cache, cs.to_pcs_xyz(&vec![0.0; cs.components()])) {
                        (Some(cache), Some(_)) => cache.pcs_bridge(icc.intent),
                        _ => None,
                    };
                    Ok(Space::Special { cs, pcs })
                }
                // A space that resolves to zero components, or to more
                // than the guard allows, is refused rather than clamped:
                // the component count sets the row stride, so a wrong one
                // does not discolour the image, it shears it.
                Some(cs) => Err(ImageError::UnsupportedColorSpace(format!(
                    "/{} with {} component(s)",
                    String::from_utf8_lossy(&family),
                    cs.components()
                ))),
                None => Err(ImageError::UnsupportedColorSpace(format!(
                    "/{} (did not resolve)",
                    String::from_utf8_lossy(&family)
                ))),
            }
        }
        other => Err(ImageError::UnsupportedColorSpace(format!(
            "/{}",
            String::from_utf8_lossy(other)
        ))),
    }
}

/// One `/Indexed` palette entry, as **exactly `m`** normalised components.
///
/// # Why this is a named function rather than three lines inline
///
/// Because the three lines it replaces were wrong for two years in a way
/// that produced a plausible picture, and a named function can be tested.
/// They built a fixed `[0.0f32; 4]` and passed the whole thing on, which:
///
/// - **broke arity** for a `Separation`/`DeviceN` base, whose tint
///   transform is a §7.10 function with a declared input count. Four
///   inputs into a two-input function makes the evaluator refuse, and the
///   caller falls back to a neutral — a grey palette, a rendered image,
///   and no counter anywhere saying the document's own transform never
///   ran; and
/// - **truncated** any base with more than four components. `DeviceN` is
///   the only PDF colour space whose component count is not fixed by its
///   family, and the print-conformance suite ships five- and six-colorant patches.
///
/// Both were found on 2026-08-21 by an operator reading a test patch's own
/// caption, not by any gate this project owns.
///
/// Short entries pad with the component's MINIMUM rather than failing: a
/// lookup table one byte short is a malformed file, and §8.6.6.3 gives no
/// recovery, so the choice is between a darkest-value component and refusing
/// the whole image. The caller already reports a short table by stopping the
/// palette early.
///
/// # ★ The range is the BASE's, not `0..1` (`Pass 242.0`)
///
/// §8.6.6.3: each byte "shall be scaled to the range of the corresponding
/// colour component in the base colour space". This function divided by 255
/// and stopped, which is right for every device, `ICCBased`, `Separation`
/// and `DeviceN` base and WRONG for a `Lab` base, whose L\* runs 0–100 and
/// whose a\*/b\* run over its `/Range` — routinely negative. An `/Indexed`
/// over `Lab` therefore decoded `L\* = 60` (byte 153) as `L\* = 0.6`, and the
/// whole palette came out near-black: on the three-ways fixture the palette
/// image probed at `(0, 0.50, 0.75, 0.98)` ink beside a fill at
/// `(0, 0, 0, 0.43)`. The graphics-state twin, `color::indexed_to_rgb`, had
/// read `component_range` correctly all along, so a `Lab` palette FILL was
/// right and a `Lab` palette IMAGE was wrong on the same page — the route
/// twin shape this project keeps recording, caught by the agreement test
/// the same day the fixture was written.
///
/// `ranges` is the base's per-component `(lo, hi)`, from
/// [`Space::component_ranges`]; a `(0.0, 1.0)` entry reproduces the old
/// arithmetic exactly, so no non-`Lab` palette moves.
fn palette_entry(entry: &[u8], ranges: &[(f32, f32)]) -> Vec<f32> {
    ranges
        .iter()
        .enumerate()
        .map(|(c, &(lo, hi))| {
            let byte = f32::from(entry.get(c).copied().unwrap_or(0));
            lo + byte / 255.0 * (hi - lo)
        })
        .collect()
}

/// Build an [`Space::Indexed`] palette from `[/Indexed base hival lookup]`.
///
/// Per §8.6.6.3: the table is `m × (hival + 1)` bytes where `m` is the
/// **base** space's component count; "each byte shall be an unsigned
/// integer in the range 0 to 255 that shall be scaled to the range of
/// the corresponding colour component" — i.e. the table is always
/// 8-bit-per-component regardless of the image's own
/// `BitsPerComponent`, which governs only the width of the *index*.
///
/// A short table is tolerated (producers trim unused trailing entries);
/// the palette simply ends early and out-of-range indices paint black
/// with [`ImageNotes::palette_out_of_range`] set.
fn resolve_indexed(
    doc: &DocumentView<'_>,
    items: &[Object],
    resources: &Dict,
    depth: usize,
    intent: CmykIntent,
    icc: IccContext<'_>,
) -> Result<Space, ImageError> {
    // The palette is built ONCE, at construction, so its conversions are
    // bounded by `hival + 1` and want no cache. The diagnostics are
    // scratch for the same reason: a shortfall in a 256-entry palette is
    // reported by the entry count, not by a counter.
    let mut palette_diag = ColorDiagnostics::default();
    let base_obj =
        items
            .get(1)
            .map(|o| doc.resolve(o))
            .ok_or(ImageError::UnsupportedColorSpace(
                "/Indexed without a base space".into(),
            ))?;
    let base = resolve_space(doc, base_obj, resources, depth + 1, intent, icc)?;
    if matches!(base, Space::Indexed { .. }) {
        // §8.6.6.3: the base "shall not be … another Indexed space".
        return Err(ImageError::UnsupportedColorSpace(
            "/Indexed over /Indexed".into(),
        ));
    }
    let m = base.components();
    // The base's component ranges, read once: what each palette byte is
    // scaled INTO (§8.6.6.3). `0..1` for everything but `Lab`.
    let base_ranges = base.component_ranges();

    // `hival` is a MAXIMUM INDEX, not a count — the table has
    // `hival + 1` entries. Normative ceiling: 255.
    let hival = items
        .get(2)
        .map(|o| doc.resolve(o))
        .and_then(Object::as_int)
        .filter(|&v| (0..=255).contains(&v))
        .ok_or(ImageError::UnsupportedColorSpace(
            "/Indexed /hival missing or outside 0..=255".into(),
        ))? as usize;

    // The lookup may be a byte STRING (PDF 1.2, and the form §8.6.6.3's
    // own example uses) or a STREAM. A reader that handles only the
    // stream case fails on the spec's own example.
    let lookup_obj =
        items
            .get(3)
            .map(|o| doc.resolve(o))
            .ok_or(ImageError::UnsupportedColorSpace(
                "/Indexed without a lookup table".into(),
            ))?;
    let lookup: Vec<u8> = match lookup_obj {
        Object::String(bytes) => bytes.clone(),
        Object::Stream(stream) => {
            // `doc.slice`, not `span.slice(doc.bytes())`: on a session
            // view the payload may live in the R45 staging half and
            // there is no single buffer to index (decision 018 §4).
            let raw = doc
                .slice(stream.data_span)
                .ok_or(ImageError::UnsupportedColorSpace(
                    "/Indexed lookup stream is out of bounds".into(),
                ))?;
            filters::decode_stream(&stream.dict, raw).map_err(map_filter_error)?
        }
        _ => {
            return Err(ImageError::UnsupportedColorSpace(
                "/Indexed lookup is neither a string nor a stream".into(),
            ));
        }
    };

    let mut table = Vec::with_capacity(hival + 1);
    // The base's colorants, kept whenever the base HAS any. Built in the
    // same loop as the sRGB entries, so the two tables cannot come to
    // disagree about which index means what.
    //
    // ★★ THIS READ `matches!(base, Space::Cmyk)` UNTIL `Pass 140.0`, and the
    // omission was the fourth route of the same defect: a DUOTONE — an
    // `/Indexed` over a `[/DeviceN [...] /DeviceCMYK <tint>]` — has
    // colorants behind every palette entry and lost all of them on the way
    // to a subtractive page. The suite ships exactly that shape (`PCS 8.2`,
    // whose caption calls a grey rendering of it an error), so this is a
    // measured population rather than a hypothetical one.
    //
    // Built OPTIMISTICALLY and dropped whole if any entry has no colorants,
    // which costs nothing here: a palette is at most 256 entries, so unlike
    // the per-texel route there is no allocation to gate. `Space::to_cmyk`
    // is the single answer both routes use.
    let mut ink_table = Some(Vec::with_capacity(hival + 1));
    // Set when an entry's base yields no colorants. All-or-nothing for the
    // same reason the texel loop is: an ink table that is right for some
    // indices and absent for others would composite parts of one image two
    // different ways.
    let mut ink_incomplete = false;
    // The base's Table 149 row, resolved ONCE here rather than per entry.
    //
    // Only the `Separation`/`DeviceN` row is kept, because it is the only one
    // whose shortfall this structure can express.
    //
    // ★ This comment used to justify that with "every other row paints `c_s`
    // in all three columns, so an overprinting image in it is already rendered
    // correctly by an ordinary paint and has nothing to carry". That is a
    // correct reading of the process-component sub-row and drops the
    // spot-colorant sub-row, which reads `c_b` under `OP true`. A process
    // image over a spot backdrop IS rendered incorrectly by an ordinary paint;
    // pdfcer simply has no plane to fix it with, and now says so rather than
    // claiming there was nothing owed. See
    // `Diagnostics::overprint_process_images_unsupported`.
    let op_kind = match &base {
        Space::Special { cs, .. } => match crate::overprint::classify(
            cs,
            true,
            // ★ NOT a policy read, deliberately, and this is the one place
            // in the codebase where passing a literal is MORE honest than
            // threading the operator's setting through.
            //
            // `in_image_sample` is `true` here, and `classify` refuses to
            // upgrade a sampled image under ANY scope — Table 149 gives
            // `DeviceCmykDirect` the qualifier "and not in a sampled image",
            // so a CMYK image already falls to `OtherProcess` where `OPM 0`
            // and `OPM 1` are identical. Reading `policy.overprint_zero_tint_scope`
            // here would imply the operator's choice reaches this call. It
            // cannot, and a reader would have to go and prove that.
            //
            // Pinned by `a_grey_image_is_never_upgraded_whatever_the_scope`,
            // so if the `!in_image_sample` guard is ever removed this becomes
            // a failing test rather than a stale comment.
            pdfcer_core::settings::OverprintZeroTintScope::DeviceCmykOnly,
        ) {
            Some(k @ crate::overprint::SourceKind::SeparationOrDeviceN { .. }) => Some(k),
            _ => None,
        },
        _ => None,
    };
    let mut op_table = op_kind
        .as_ref()
        .map(|_| Vec::<[f32; 4]>::with_capacity(hival + 1));
    // The base's spot colorants, resolved ONCE: which components they are
    // and what each looks like alone on white. Per-entry tints are read in
    // the loop below by component index, so no per-entry name search.
    let spot_slots: Vec<(usize, std::sync::Arc<[u8]>)> = match (&op_kind, &base) {
        (Some(kind), Space::Special { .. }) => {
            crate::overprint::authored_spots(kind, &vec![0.0_f32; m])
                .into_iter()
                .map(|(component, name, _)| (component, std::sync::Arc::from(name)))
                .collect()
        }
        _ => Vec::new(),
    };
    let spot_colorants: Vec<SpotColorant> = match &base {
        Space::Special { cs, .. } => spot_slots
            .iter()
            .map(|(component, name)| {
                (
                    std::sync::Arc::clone(name),
                    std::sync::Arc::new(crate::overprint::spot_lut(cs, *component, m, intent)),
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut spot_table: Vec<Vec<f32>> =
        Vec::with_capacity(if spot_slots.is_empty() { 0 } else { hival + 1 });
    // ★ EXACTLY `m` COMPONENTS, AND THE BUFFER IS SIZED FROM `m`.
    //
    // This was a fixed `[0.0f32; 4]` passed whole to `to_rgb`, and it was
    // wrong in two independent ways that both produced a PLAUSIBLE picture
    // rather than a broken one — which is why it survived until an
    // operator read a test patch's own caption on 2026-08-21.
    //
    // 1. **Arity.** `Space::Special` hands the slice straight to
    //    `ColorSpace::to_rgb`, and a `Separation`/`DeviceN` tint transform
    //    is a §7.10 function with a declared input count. Handing a
    //    two-colorant `DeviceN` four inputs makes `PdfFunction::eval`
    //    refuse, `tint_through` return `None`, and `device_n_to_rgb` fall
    //    back to its max-tint NEUTRAL. The palette comes out grey, the
    //    image renders, and nothing anywhere says the document's own
    //    transform never ran.
    //
    //    Measured on suite `PCS 8.2`, whose image space is
    //    `[/Indexed [/DeviceN [/Cyan /Black] /DeviceCMYK <tint>] 255 …]`:
    //    pdfcer rendered a neutral-grey manta ray where the duotone's cyan
    //    should be, and the patch's own caption calls that exact outcome
    //    an ERROR.
    //
    // 2. **Width.** Four slots cannot hold a five- or six-colorant
    //    `DeviceN`, and the suite ships both (`PCS 8.1`, `PCS 8.01`). The
    //    trailing colorants were silently dropped before any conversion
    //    was attempted.
    for i in 0..=hival {
        let base_off = i.saturating_mul(m);
        let Some(entry) = lookup.get(base_off..base_off + m) else {
            // Short table: stop here. Indices past the end paint black
            // and set `palette_out_of_range`.
            break;
        };
        let comps = palette_entry(entry, &base_ranges);
        // ★ `comps` is the entry in the BASE space, so the base is what is
        // asked — a `DeviceCMYK` base returns the components themselves, a
        // `Separation`/`DeviceN` base runs its tint transform, and an
        // additive base answers `None` and takes the whole table with it.
        // One function, both routes; see `Space::to_cmyk`.
        if let Some(ink) = ink_table.as_mut() {
            match base.to_cmyk(&comps, &mut palette_diag) {
                Some(t) => ink.push(t),
                None => ink_incomplete = true,
            }
        }
        // ★ `comps` is the entry in the BASE space — one operand per declared
        // colorant, in `names` order — which is precisely what
        // `authored_tints` is written to read. Read here, at the one point
        // where the operands still exist; one line later the entry is an
        // `Rgb` and §8.6.6.5's ordering has been erased.
        if let (Some(op), Some(kind)) = (op_table.as_mut(), op_kind.as_ref()) {
            op.push(crate::overprint::authored_tints(kind, &comps).unwrap_or([0.0; 4]));
        }
        if !spot_slots.is_empty() {
            spot_table.push(
                spot_slots
                    .iter()
                    .map(|(component, _)| comps.get(*component).copied().unwrap_or(0.0))
                    .collect(),
            );
        }
        table.push(base.to_rgb(intent, &comps, &mut palette_diag));
    }
    // All-or-nothing, discharged. A short lookup table breaks the loop early
    // and leaves FEWER ink entries than palette entries, which would
    // mis-index every colour after the break — so the length is checked as
    // well as the per-entry answer, and the two failures are one outcome.
    if ink_incomplete || ink_table.as_ref().is_some_and(|t| t.len() != table.len()) {
        ink_table = None;
    }
    Ok(Space::Indexed {
        base_icc_managed: matches!(base, Space::Icc { .. } | Space::IccRgb { .. }),
        table,
        ink: ink_table,
        overprint: op_kind
            .zip(op_table)
            .map(|(kind, entries)| IndexedOverprint {
                kind,
                entries,
                spot_colorants,
                spot_entries: spot_table,
            }),
    })
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

    /// ★ THE REGRESSION GUARD FOR A BUG AN OPERATOR FOUND AND NO GATE DID.
    ///
    /// A palette entry must be exactly as wide as its base space, because
    /// `Space::Special` hands the slice straight to a tint transform whose
    /// input count the document declares. The previous code passed four
    /// components always; a two-colorant `DeviceN` therefore got four,
    /// its transform refused, and the palette silently came out neutral
    /// grey. suite `PCS 8.2`'s duotone rendered as a greyscale manta ray
    /// where it should have been cyan, and the harness called it clean.
    #[test]
    fn a_palette_entry_is_exactly_as_wide_as_its_base_space() {
        let entry = [10u8, 20, 30, 40, 50, 60];
        let unit = |m: usize| vec![(0.0_f32, 1.0_f32); m];
        for m in 1..=6 {
            assert_eq!(
                palette_entry(&entry, &unit(m)).len(),
                m,
                "a {m}-component base must receive {m} components, not 4"
            );
        }
        // And the values are the entry's own bytes, in order, normalised.
        let two = palette_entry(&entry, &unit(2));
        assert!((two[0] - 10.0 / 255.0).abs() < 1e-6);
        assert!((two[1] - 20.0 / 255.0).abs() < 1e-6);
        // Six colorants are NOT truncated to four -- `DeviceN` is the one
        // space whose width its family does not fix, and the print-conformance suite
        // ships a six-colorant patch.
        let six = palette_entry(&entry, &unit(6));
        assert!((six[5] - 60.0 / 255.0).abs() < 1e-6);
    }

    /// §8.6.6.3 scales each byte into the BASE's component range. For a
    /// `Lab` base that is `0..100` for L\* and the `/Range` for a\*/b\*, so
    /// byte 153 is `L\* = 60` and byte 128 under `[-128 127]` is exactly 0 —
    /// not 0.6 and 0.502, which is what dividing by 255 alone produced and
    /// what painted every `/Indexed`-over-`Lab` image near-black.
    #[test]
    fn a_palette_entry_is_scaled_into_the_bases_range_not_into_the_unit_interval() {
        let lab = vec![(0.0_f32, 100.0_f32), (-128.0, 127.0), (-128.0, 127.0)];
        let got = palette_entry(&[153u8, 128, 128], &lab);
        assert!((got[0] - 60.0).abs() < 1e-4, "L* {}", got[0]);
        assert!(got[1].abs() < 1e-4, "a* {}", got[1]);
        assert!(got[2].abs() < 1e-4, "b* {}", got[2]);
        // Byte 0 is the range's floor and byte 255 its ceiling, whatever
        // the range is.
        let ends = palette_entry(&[0u8, 255, 0], &lab);
        assert!(
            (ends[0]).abs() < 1e-6
                && (ends[1] - 127.0).abs() < 1e-4
                && (ends[2] + 128.0).abs() < 1e-4
        );
    }

    /// A short lookup pads with the component's floor rather than
    /// panicking. §8.6.6.3 gives no recovery for a truncated table, and a
    /// darkest-value component is a visible defect while a panic is a dead
    /// renderer.
    #[test]
    fn a_short_palette_entry_pads_with_the_floor() {
        let unit = |m: usize| vec![(0.0_f32, 1.0_f32); m];
        assert_eq!(palette_entry(&[255u8], &unit(3)), vec![1.0, 0.0, 0.0]);
        assert_eq!(palette_entry(&[], &unit(2)), vec![0.0, 0.0]);
        // For a signed range the floor is the range's minimum, not zero.
        let lab = vec![(0.0_f32, 100.0_f32), (-128.0, 127.0), (-128.0, 127.0)];
        assert_eq!(palette_entry(&[], &lab), vec![0.0, -128.0, -128.0]);
    }

    #[test]
    fn row_stride_rounds_up_per_row() {
        // §8.9.3: a 3-pixel 1-bpc row is 1 byte with 5 padding bits.
        assert_eq!(row_stride(3, 1, 1).unwrap(), 1);
        assert_eq!(row_stride(9, 1, 1).unwrap(), 2);
        assert_eq!(row_stride(2, 3, 8).unwrap(), 6);
        assert_eq!(row_stride(2, 1, 4).unwrap(), 1);
        assert_eq!(row_stride(3, 1, 4).unwrap(), 2);
    }

    #[test]
    fn sub_byte_samples_unpack_high_order_first() {
        // 0b01_10_11_00 at 2 bpc → 1, 2, 3, 0.
        let data = [0b0110_1100u8];
        let got: Vec<u32> = (0..4).map(|i| read_sample(&data, i * 2, 2)).collect();
        assert_eq!(got, vec![1, 2, 3, 0]);
        // 1 bpc.
        let data = [0b1010_0000u8];
        let got: Vec<u32> = (0..4).map(|i| read_sample(&data, i, 1)).collect();
        assert_eq!(got, vec![1, 0, 1, 0]);
    }

    #[test]
    fn sixteen_bit_samples_are_big_endian() {
        assert_eq!(read_sample(&[0x12, 0x34], 0, 16), 0x1234);
    }

    #[test]
    fn reads_past_the_end_are_zero_not_a_panic() {
        assert_eq!(read_sample(&[], 0, 8), 0);
        assert_eq!(read_sample(&[0xFF], 8, 16), 0);
    }

    #[test]
    fn indexed_default_decode_is_the_identity() {
        // Table 90 / §8.9.5.2 NOTE 2: [0 2ⁿ−1], so y = x.
        let space = Space::Indexed {
            base_icc_managed: false,
            table: vec![Rgb::BLACK],
            ink: None,
            overprint: None,
        };
        assert_eq!(space.default_decode(255.0), vec![(0.0, 255.0)]);
        assert_eq!(space.components(), 1, "the sample is ONE index");
    }

    #[test]
    fn device_default_decode_is_zero_to_one_per_component() {
        assert_eq!(Space::Rgb.default_decode(255.0), vec![(0.0, 1.0); 3]);
        assert_eq!(Space::Cmyk.default_decode(15.0), vec![(0.0, 1.0); 4]);
    }

    #[test]
    fn decode_pairs_are_not_normalized() {
        // `[1 0]` is inversion (§8.9.5.2 NOTE 3), NOT a malformed
        // rectangle — the named trap.
        let mut d = Dict::new();
        d.insert(
            pdfcer_core::object::Name::from(b"Decode"),
            Object::Array(vec![Object::Integer(1), Object::Integer(0)]),
        );
        assert_eq!(decode_pairs(&d), Some(vec![(1.0, 0.0)]));
    }

    /// ★ THE ARMS THE FIXTURES CANNOT REACH, AND WHY THEY ARE ASSERTED HERE.
    ///
    /// `tests/devicen_image_ink.rs` exercises the two spaces that DO carry ink
    /// through whole rendered pages, which is the right level for those. It
    /// cannot cheaply build a page for every space that must answer **`None`**
    /// — and `None` is the load-bearing half: a space that wrongly claims
    /// colorants would have four numbers written into the ink planes and
    /// composited as authored ink, which paints a plausible colour that the
    /// document never asked for.
    ///
    /// `DeviceGray` is the one to watch. Mapping a grey to `K` is arithmetically
    /// obvious, reads as helpful, and is exactly the claim
    /// [`crate::color::ColorSpace::to_cmyk`] refuses to make: the file did not
    /// say `K`, it said grey, and a renderer that decides otherwise has
    /// invented a colorant. (It is also, separately, what Acrobat does under
    /// `/OPM 1` — see the `Pass 143.0` Backlog entry, where that divergence is
    /// a deliberate open question rather than an oversight here.)
    #[test]
    fn only_a_space_with_real_colorants_answers_to_cmyk() {
        let mut d = ColorDiagnostics::default();
        // Carries ink: the components ARE the colorants.
        assert_eq!(
            Space::Cmyk.to_cmyk(&[0.1, 0.2, 0.3, 0.4], &mut d),
            Some([0.1, 0.2, 0.3, 0.4])
        );
        assert!(Space::Cmyk.yields_cmyk(&mut d));

        // Does not, and each for its own reason.
        for (label, space) in [
            (
                "DeviceGray states no colorant, however tempting K is",
                Space::Gray,
            ),
            ("DeviceRGB is additive", Space::Rgb),
            (
                "Lab is colorimetric, not a colorant space",
                Space::Special {
                    cs: std::sync::Arc::new(crate::color::ColorSpace::Lab {
                        white: [0.9642, 1.0, 0.8249],
                        range: [-100.0, 100.0, -100.0, 100.0],
                    }),
                    // No output intent on this page, so no PCS route: the
                    // colorimetric space stays colorant-less.
                    pcs: None,
                },
            ),
            (
                "CalRGB likewise",
                Space::Special {
                    cs: std::sync::Arc::new(crate::color::ColorSpace::CalRgb {
                        white: [0.9505, 1.0, 1.089],
                        gamma: [1.0, 1.0, 1.0],
                        matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                    }),
                    pcs: None,
                },
            ),
        ] {
            assert_eq!(
                space.to_cmyk(&[0.5, 0.5, 0.5, 0.5], &mut d),
                None,
                "{label}"
            );
            assert!(!space.yields_cmyk(&mut d), "{label}");
        }
    }

    /// §8.6.6.3: an `/Indexed` operand is an **index**, not a colour, so the
    /// space itself must refuse the colorant question outright.
    ///
    /// ★ Answering it would be worse than merely wrong. `Space::to_cmyk` takes
    /// raw components, so a permissive arm would read the index `3` as a cyan
    /// value of 3.0, clamp it to full ink, and paint the whole image solid.
    /// The palette's colorants live in `Space::Indexed::ink`, built by
    /// [`resolve_indexed`] from the BASE, and the texel loop reads that table
    /// instead — which is why this returning `None` is not a gap.
    #[test]
    fn an_indexed_space_refuses_the_colorant_question_and_carries_a_table_instead() {
        let mut d = ColorDiagnostics::default();
        let indexed = Space::Indexed {
            base_icc_managed: false,
            table: vec![Rgb::BLACK; 2],
            ink: Some(vec![[0.1, 0.2, 0.3, 0.4], [0.5, 0.6, 0.7, 0.8]]),
            overprint: None,
        };
        assert_eq!(indexed.to_cmyk(&[1.0], &mut d), None);
        assert!(!indexed.yields_cmyk(&mut d));
        // ...and the decode loop's gate still recognises it, through the OTHER
        // half of the `carries_ink` test. Both halves are needed; asserted
        // together so a "simplification" that drops one is caught here.
        assert!(matches!(indexed, Space::Indexed { ink: Some(_), .. }));
    }
}

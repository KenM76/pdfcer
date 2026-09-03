//! # JPXDecode (ISO 32000-1 §7.4.9, §8.9.5 Table 89; ITU-T T.800) — the JPEG 2000 adapter
//!
//! Spec source: `filters/filter__jpx.md` in the PDF-spec RAG, VERIFIED
//! 2026-07-30 against the staged `PDF32000_2008.pdf` (printed pp. 35–37
//! for §7.4.9, pp. 206–208 for Table 89). The codec itself is ITU-T
//! T.800 (published identically as ISO/IEC 15444-1); the PDF clause
//! defines only the *embedding*.
//!
//! Crate choice is decision 005 §4.5: **`hayro-jpeg2000`**, with
//! `simd` off (it pulls `fearless_simd`, which is where the crate's only
//! unsafe would live) and `image` off (it pulls the `image` crate and
//! `moxcms`, neither of which belongs in `pdfcer-core`). It is the only
//! credible pure-Rust JPEG 2000 decoder — 550k downloads against ~1.9k
//! for the nearest pure-Rust rival, with everything else in the space
//! wrapping OpenJPEG in C. That mattered: a C binding would have
//! triggered both a `LEGAL.md` §6.2 licence review of the underlying
//! library *and* reopened the single-static-binary packaging question
//! (`ARCHITECTURE.md` §6), and the mature JPEG 2000 implementations are
//! GPL.
//!
//! ## This filter INVERTS the usual dictionary rules — the whole point
//!
//! For every other image filter the image dictionary is authoritative
//! and the codestream must agree with it. Table 89 turns that around,
//! and §7.4.9 states the same rules a second time from the filter's
//! side. Both citations are valid; §7.4.9 is the fuller statement.
//!
//! | Entry | Ordinary image | **JPXDecode** |
//! |---|---|---|
//! | `/ColorSpace` | Required | **Optional.** Present → it wins and "the colour space specifications in the JPEG2000 data shall be ignored". Absent → "the colour space information in the JPEG2000 data shall be used". |
//! | `/BitsPerComponent` | Required | **"Optional and shall be ignored if present. The bit depth is determined by the conforming reader in the process of decoding."** |
//! | `/Decode` | Applied (§8.9.5.2) | **"Shall be ignored, except in the case where the image is treated as a mask; that is, when `ImageMask` is true."** |
//! | `/Width`, `/Height` | Authoritative | "shall **match** … the JPEG2000 data" — with **no** conflict-resolution rule given. |
//! | `/SMaskInData` | meaningless | Selects whether the codestream's own opacity channel is used at all. Default **0** = ignore it. |
//!
//! Two consequences are easy to get backwards and both are load-bearing:
//!
//! 1. **A present `/ColorSpace` WINS.** The trap is to read "the
//!    codestream is authoritative for JPX" as a blanket rule and let the
//!    codestream override a stated `/ColorSpace`. It does not. The
//!    codestream wins only where the dictionary is *silent* (colour) or
//!    *disqualified* (bit depth, `Decode`). Applying it the other way
//!    round produces wrong colour on exactly the files a producer took
//!    the trouble to tag.
//! 2. **A present `/BitsPerComponent` is WRONG to honour.** It is not
//!    merely optional; a reader that uses it "will get the wrong depth,
//!    because it shall be ignored if present". This module reports the
//!    depth of the samples it actually delivers and the renderer takes
//!    that in preference to the dictionary (see
//!    `pdfcer_render::image`).
//!
//! This module owns half of that: it produces the samples, the
//! codestream-declared geometry, the codestream-declared colour model
//! and any embedded ICC profile. `pdfcer-render` owns the other half —
//! choosing between dictionary and codestream, and suppressing
//! `/Decode`.
//!
//! ## Bit depth: pdfcer delivers 8, range-scaled — a decision, documented
//!
//! §7.4.9 allows "any value from 1 to 38" bits per component, and
//! permits **different components to use different depths**. The
//! [`CodedImage`] contract has one uniform `bits_per_component`, because
//! §8.9.3's sample layout — which `pdfcer-render`'s unpacking path
//! implements — has no way to express a per-component depth.
//!
//! Table 89 resolves the tension for us in one clause: "the bit depth is
//! determined by the **conforming reader** in the process of decoding."
//! pdfcer determines **8**, and scales each component from its own
//! declared depth:
//!
//! ```text
//! out = round(sample / (2^d − 1) × 255)      clamped to 0..=255
//! ```
//!
//! **Full-range scaling, not high-byte truncation.** The distinction is
//! real: a 16-bit sample of `0x00FF` scales to 1 and truncates to 0, and
//! more importantly the codestream's white point `2^d − 1` maps onto
//! exactly 255 for *every* depth under scaling, while truncation only
//! gets that right when `d` is a multiple of 8. `fixtures_jpx.rs`'s
//! 16-bit fixture carries a `0x00FF` pixel precisely to pin this.
//!
//! Per-component depths are handled naturally because each component is
//! scaled by its own `bit_depth()` on the way into the interleave. This
//! is also the reason the adapter does its own interleaving rather than
//! calling `hayro-jpeg2000`'s `DecodedImage::data_u8()`: that helper
//! interleaves the opacity channel *into* the colour samples, which
//! `/SMaskInData` requires us to keep separate, and its scaling path
//! computes `1 << bit_depth` on a depth that a JP2 palette box may
//! legally declare as large as 128 — a shift overflow. pdfcer's own loop
//! rejects any depth outside 1..=31 by name (rule R27) and never
//! performs that shift.
//!
//! ## `/SMaskInData` (Table 89, VERIFIED): 0 ignore, 1 opacity, 2 preblended
//!
//! - **0 (the default)** — "If present, encoded soft-mask image
//!   information **shall be ignored**." So a decoder that always hands
//!   back the alpha it found is *wrong*. The opacity channel is still
//!   removed from the colour samples (it is not a colour component),
//!   but it is not exposed.
//! - **1** — "The image's data stream includes encoded soft-mask
//!   values." This is §7.4.9's *ordinary opacity* channel type. The
//!   channel is lifted out into [`CodedImage::embedded_alpha`], one
//!   8-bit sample per pixel.
//! - **2** — "the image's data stream includes colour channels that have
//!   been **preblended with a background**; the image data also includes
//!   an opacity channel," and a reader "may create a soft-mask image
//!   **with a `Matte` entry**". This is §7.4.9's *premultiplied
//!   opacity* channel type.
//!
//! **Value 2 is recognize-and-defer, deliberately.** Reconstructing the
//! unblended colour needs the `Matte` backdrop and clause 11's
//! un-premultiplication, and clause 11 is out of scope for this Pass —
//! decision 005 §7 assigns `/SMask`, `/Mask` and colour-key masking to
//! `ROADMAP.md` Pass 1.1 item 6.3, and the spec RAG marks the `Matte`
//! interaction a GAP (clause 11 not yet ingested). So pdfcer:
//!
//! - decodes and returns the colour channels **as stored**, i.e.
//!   preblended — which is what the picture genuinely looks like
//!   composited over that backdrop, and therefore a recognizable image
//!   rather than a grey box;
//! - does **not** expose the opacity channel, because using it without
//!   un-premultiplying would double-darken every partially transparent
//!   pixel;
//! - sets [`CodecNotes::jpx_smask_in_data_preblended`], which the
//!   renderer and both front ends surface by name.
//!
//! That is the `fuzzy, never sneaky` shape (CLAUDE.md rule 4): visible,
//! counted, and not silently approximated. There is also a hard limit
//! underneath it — `hayro-jpeg2000`'s `cdef` parser accepts only channel
//! types 0 (colour) and 1 (opacity), so a file that declares a genuine
//! *premultiplied* channel type fails at parse with an invalid-box
//! error rather than reaching this branch at all.
//!
//! A nonzero `/SMaskInData` on an image whose codestream carries no
//! opacity channel is a malformed dictionary — "there shall be only one
//! opacity channel in the JPEG2000 data and it shall apply to all
//! colour channels" presupposes one exists. It is counted as a geometry
//! disagreement rather than refused: nothing about the colour samples is
//! wrong.
//!
//! Note the exclusion rule this module does **not** enforce: "If this
//! entry has a nonzero value, `SMask` shall not be specified." That is a
//! validation rule about the image dictionary, and `pdfcer-render` is
//! where image-dictionary consistency is judged.
//!
//! ## Colour: the §7.4.9 fallback ladder, and where ICC stops
//!
//! §7.4.9 specifies a *ladder*, not a lookup, for the `/ColorSpace`-absent
//! case: the codestream's own specification, preferring "the one with
//! the highest precedence and best approximation value"; on an
//! unsupported ICC profile "the next lower colour space … shall be
//! used"; and finally "**if no supported colour space is found, the
//! colour space used shall be `DeviceGray`, `DeviceRGB`, or
//! `DeviceCMYK`, depending on … whether the number of channels in the
//! JPEG2000 data is 1, 3, or 4**". A decoder that errors out on an
//! unsupported ICC profile is non-conformant.
//!
//! `hayro-jpeg2000` walks the top of that ladder itself and hands back
//! `ColorSpace::{Gray, RGB, CMYK, Icc{profile, n}, Unknown{n}}`. This
//! module walks the rest:
//!
//! | Vendor `ColorSpace` | [`CodecColorModel`] | `icc_profile` |
//! |---|---|---|
//! | `Gray` | `Gray` | none |
//! | `RGB` | `Rgb` | none |
//! | `CMYK` | `Cmyk` | none |
//! | `Icc { profile, n }` | by the profile's own **data colour space** signature, else by `n` | the profile, verbatim |
//! | `Unknown { n }` | `Unknown { n }` | none |
//!
//! The ICC row is the interesting one and the reason a four-byte header
//! read earns its place. pdfcer applies no colour management: the
//! renderer's `ICCBased` path already uses the spec's own
//! `N`-component fallback, treating an ICC RGB profile as `DeviceRGB`.
//! That approximation is fine for `GRAY`/`RGB `/`CMYK` profiles and
//! **wrong** for a `Lab ` profile, whose samples are not device
//! components at all — and `hayro-jpeg2000` returns exactly that shape
//! for a JPX enumerated CIELab image (it substitutes a bundled LAB
//! profile and leaves the samples as scaled Lab). Mapping such a
//! profile by channel count would paint L\*a\*b\* as if it were RGB:
//! plausible-looking, entirely wrong colour. So a profile whose data
//! colour space is not one pdfcer can approximate becomes
//! [`CodecColorModel::Unknown`], and the renderer refuses the image
//! rather than inventing colour for it.
//!
//! Note what is *not* here: this module never picks a PDF colour space,
//! never applies `/Decode`, and never applies an ICC transform
//! (rule R26). It reports what the samples are; the renderer decides
//! what they mean.
//!
//! ## Forbidden in inline images
//!
//! §7.4.9: "This filter shall only be applied to image XObjects, and not
//! to inline images." §8.9.7 says it from the other side —
//! `JBIG2Decode` and `JPXDecode` are absent from Table 94's
//! abbreviation list "because those filters shall not be used with
//! inline images." Enforced before any bytes are touched, in
//! [`super::decode_image`] via [`super::Codec::allowed_inline`], so this
//! module is never reached from the inline path.
//!
//! ## No `/DecodeParms` (Table 6, VERIFIED)
//!
//! `JPXDecode` is one of the four `no`-parameter filters in Table 6
//! (with `ASCIIHexDecode`, `ASCII85Decode` and `RunLengthDecode`), which
//! is why [`decode`] takes no parameter dictionary at all — a difference
//! from the other three codecs that is worth seeing in the signature.
//! Everything configurable lives in the codestream or in the *image*
//! dictionary.
//!
//! ## Resource ceilings are pdfcer's, never the vendor's (rule R25)
//!
//! `hayro-jpeg2000` caps dimensions at 60,000 and does not bound the
//! *product*, so its own guards permit a 60,000 × 60,000 image — 3.6
//! Gpx. Worse, its `DecoderContext` allocates one `f32` per sample per
//! component **from the codestream's declared geometry, before any
//! entropy decoding**, so a few hundred header bytes can ask for
//! arbitrary memory. The ceilings below are therefore applied between
//! `Image::new` (which parses headers only) and `Image::decode` (which
//! allocates), which is the one place they can work.
//!
//! ### The tile-count ceiling, and the fuzz finding behind it
//!
//! Pixels and bytes are not the only unbounded quantity a SIZ marker
//! can declare. The **tile grid** is independent of them: T.800's
//! `XTsiz`//`YTsiz` may be as small as 1, so a 512 × 1024 image tiled at
//! 4 × 2 declares **65,536 tiles**, and a decoder that materializes and
//! walks one structure per tile does 65,536 tiles' worth of setup for
//! half a megapixel of output.
//!
//! That is not hypothetical. `fuzz_targets/image_codec_jpx.rs` produced
//! exactly that codestream — **310 bytes, 32 seconds** — within the
//! first minute of its first campaign. It is a CPU-exhaustion vector,
//! not a memory one, so neither [`MAX_IMAGE_PIXELS`] nor
//! [`MAX_WORKING_BYTES`] sees it: 512 Kpx is two orders of magnitude
//! inside both. [`MAX_TILES`] is the guard that does, and it is a
//! textbook case for rule R25 — a ceiling nobody would have thought to
//! write until a fuzzer wrote the input that needs it.
//!
//! ## Upstream's admitted gaps become named diagnostics (rule R27)
//!
//! `hayro-jpeg2000`'s own documentation states it has "some missing
//! pieces for some 'obscure' features, like for example support for
//! progression order changes in tile-parts". Mechanically, an unhandled
//! marker segment is a single `MarkerError::Unsupported` from either the
//! main-header or the tile-part-header parser, with no indication of
//! *which* marker. A bare "decode failed" there would be exactly the
//! grey box rule R27 forbids, so [`decode`] runs a bounded marker walk
//! **on the error path only** and reports
//! `"JPX/progression-order-change"` when a `POC` marker (T.800 Table
//! A.2, code `0xFF5F`) is present and `"JPX/unsupported-marker"`
//! otherwise. The walk never influences accept/reject — only the name of
//! the diagnostic — so it cannot drift into a second, disagreeing
//! parser.

use hayro_jpeg2000::{ColorSpace, DecodeSettings, DecoderContext, Image};

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

/// Ceiling on `hayro-jpeg2000`'s internal working set, in bytes.
///
/// The decoder holds one `f32` per sample per component — four bytes for
/// every one byte that reaches [`CodedImage::samples`] — and allocates
/// all of it up front from the codestream's declared geometry. Setting
/// this to [`MAX_IMAGE_PIXELS`] × 4 components × 4 bytes makes it
/// **exactly as permissive as [`MAX_IMAGE_PIXELS`] already is**: no
/// image that the project-wide pixel ceiling admits is refused here.
///
/// It exists anyway, rather than being left implicit, for the reason
/// rule R25 was written: a bound that is only the accidental product of
/// other constants is a bound nobody chose and nobody will notice
/// changing. This one is written down, and it is the number the fuzz
/// target's `-rss_limit_mb` is reasoned against.
const MAX_WORKING_BYTES: u64 = MAX_IMAGE_PIXELS * 4 * 4;

/// Maximum number of T.800 tiles pdfcer will decode in one image.
///
/// A tile grid is declared independently of the image size (`XTsiz` and
/// `YTsiz` may each be as small as 1), and `hayro-jpeg2000` builds and
/// walks one structure per tile, so the tile count — not the pixel
/// count — is what bounds the decode's *work*. A 310-byte codestream
/// declaring 65,536 tiles over a 512 × 1024 image took **32 seconds**
/// when the fuzz target found it; see the module docs.
///
/// 4,096 is set from what real encoders emit rather than from a round
/// number. OpenJPEG's default is a single tile; Kakadu's is
/// 1024 × 1024, which is 30 tiles at pdfcer's 32 Mpx ceiling; the most
/// aggressive tiling seen in practice is 256 × 256, which is 512. So
/// this admits eight times the most aggressive real tiling, and lets a
/// full 32 Mpx image be tiled as finely as 91 × 91 — while bounding the
/// pathological case at roughly half a second instead of eight.
///
/// Like every ceiling in this module it is pdfcer's own number, not a
/// vendor default (rule R25), and it is validated against the corpus's
/// veraPDF implementation-limits files before shipping.
const MAX_TILES: u64 = 4096;

/// Highest per-component bit depth pdfcer will scale from.
///
/// §7.4.9 allows 1..=38. `hayro-jpeg2000` refuses anything above 31 in
/// the SIZ marker (its bit-plane coder is `u32`-based), so for ordinary
/// components this is never the binding limit — but a JP2 **palette**
/// box may declare a column depth up to 128, and after palette
/// resolution that depth becomes the component's. Scaling from it would
/// mean evaluating `1 << 128`. Refused by name instead.
const MAX_COMPONENT_BIT_DEPTH: u8 = 31;

/// Table 89's `/SMaskInData` code, as an exhaustive three-way choice.
///
/// Modelled as an enum rather than an integer so that every use site has
/// to say what it does about the preblended case; the whole risk in this
/// entry is that value 2 gets quietly handled like value 1.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum SMaskInData {
    /// **0**, the default — "encoded soft-mask image information shall
    /// be ignored".
    Ignore,
    /// **1** — an ordinary opacity channel, usable as a soft mask.
    Opacity,
    /// **2** — colour channels preblended with a background, plus an
    /// opacity channel that would need a `Matte` entry. Recognized and
    /// deferred (module docs).
    Preblended,
}

/// Decode a `JPXDecode` codestream (§7.4.9).
///
/// `data` is the JP2/JPX file structure — or a bare T.800 codestream;
/// `hayro-jpeg2000` sniffs which — after any byte-stream filter prefix.
/// `dict` is the image dictionary, read **only** for `/SMaskInData` and
/// for the geometry-disagreement counters; nothing in it changes how the
/// samples are produced. There is no `parms` argument because Table 6
/// gives this filter no parameters.
///
/// # Errors
///
/// - [`ImageCodecError::TooLarge`] when the codestream's declared
///   geometry crosses [`MAX_IMAGE_PIXELS`], [`MAX_IMAGE_DIMENSION`],
///   [`MAX_IMAGE_SAMPLE_BYTES`] or [`MAX_WORKING_BYTES`];
/// - [`ImageCodecError::FeatureUnsupported`] for the two named gaps —
///   an unhandled marker segment (`"JPX/progression-order-change"` /
///   `"JPX/unsupported-marker"`) and a component bit depth pdfcer will
///   not scale from (`"JPX/bit-depth"`);
/// - [`ImageCodecError::Corrupt`] for everything else, carrying the
///   vendor's own `Display` text so the detail names the actual fault.
///
/// Nothing here can panic: every fallible step is checked, every slice
/// access is bounds-checked, and the `image_codec_jpx` fuzz target
/// asserts it over arbitrary bytes.
pub(super) fn decode(
    doc: &DocumentView<'_>,
    data: &[u8],
    dict: &Dict,
    notes: &mut CodecNotes,
) -> Result<CodedImage, ImageCodecError> {
    // ★★ A JP2 PALETTE AND A PDF `/Indexed` SPACE ARE THE SAME LOOKUP, AND
    // APPLYING BOTH PAINTS THE WRONG COLOUR.
    //
    // This was unconditionally `true`, on reasoning that was right for the
    // common case and wrong for exactly one shape. The prior comment read:
    //
    //   "JP2 palette boxes are resolved to real component values here rather
    //    than surfaced as indices. PDF has its own `Indexed` colour space and
    //    §7.4.9 permits one, but a JPX *internal* palette is not that:
    //    leaving it unresolved would hand the renderer grayscale index values
    //    with nothing to look them up in."
    //
    // Every clause of that holds **when the image dictionary supplies no
    // `/Indexed` space**. When it does, the renderer has exactly what to look
    // them up in, and resolving here means the lookup happens TWICE.
    //
    // What that does, concretely, on a file built to catch it: a JP2 with a
    // one-entry `pclr` palette of `(114, 247, 13)` and a PDF `/ColorSpace` of
    // `[/Indexed <ICCBased N=3> 0 <3-byte lookup>]` holding **the same three
    // bytes**. Resolve here and the samples become `114, 247, 13`; the
    // renderer then reads component 0 as an INDEX, asks a one-entry table for
    // entry 114, gets nothing, and paints **black**. Apply either lookup
    // alone and the answer is green. Applying both is the only way to be
    // wrong, which is precisely why a conformance suite ships the pair.
    //
    // §8.9 Table 89 decides it: with `JPXDecode`, if `/ColorSpace` is present
    // then "colour space specifications in the JPEG2000 data shall be
    // ignored". A `pclr`/`cmap` pair is such a specification — it is how the
    // codestream says what its samples MEAN — so the dictionary's space wins
    // and the samples must stay indices.
    //
    // ★ THE DEFAULT IS UNCHANGED, DELIBERATELY. This disables resolution only
    // where an `/Indexed` array is actually VISIBLE from here. A `/ColorSpace`
    // naming a resource (`/CS0`) needs the page's resource dictionary, which
    // this function does not have, so it cannot be inspected — and there the
    // old behaviour stands. Changing an unreadable case on a guess would
    // trade a rare wrong render for a common one.
    let pdf_supplies_indexed = dict_colorspace_is_indexed(doc, dict);
    if pdf_supplies_indexed {
        notes.jpx_palette_left_to_pdf = true;
    }
    let settings = DecodeSettings {
        resolve_palette_indices: !pdf_supplies_indexed,
        // Non-strict, on upstream's own recommendation ("leave this flag
        // disabled unless you have a specific reason not to"). Strict
        // mode rejects a missing EOC marker and other end-of-stream
        // sloppiness that real producers emit; refusing those files
        // would be a fidelity loss with no safety gain, since every
        // buffer here is bounds-checked regardless.
        strict: false,
        // No resolution reduction. `target_resolution` asks the decoder
        // to stop at a lower wavelet resolution level, which is a
        // performance/quality trade the renderer has not asked for and
        // which would silently make `CodedImage::width` disagree with
        // the codestream's actual geometry — the one field callers are
        // told is the codestream's own declaration.
        target_resolution: None,
    };

    let image = Image::new(data, &settings).map_err(|e| map_error(e, data))?;
    let (width, height) = (image.width(), image.height());
    let color = image.color_space().clone();
    let colour_channels = u64::from(color.num_channels());
    let has_alpha = image.has_alpha();
    let total_channels = colour_channels + u64::from(has_alpha);

    // The ONE correct place for the ceilings: after the SIZ marker has
    // declared the geometry, before `decode()` allocates from it (module
    // docs, rule R25).
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION || pixels > MAX_IMAGE_PIXELS {
        return Err(ImageCodecError::TooLarge);
    }
    if pixels.saturating_mul(colour_channels) > MAX_IMAGE_SAMPLE_BYTES as u64
        || pixels
            .saturating_mul(total_channels)
            .saturating_mul(4)
            .max(1)
            > MAX_WORKING_BYTES
    {
        return Err(ImageCodecError::TooLarge);
    }
    // A zero-channel colour space would make the output buffer empty and
    // every stride zero. `hayro-jpeg2000` rejects `Csiz == 0` in the SIZ
    // marker, but `ColorSpace::Unknown { num_channels: 0 }` is
    // constructible from a JP2 box set, so the guard is stated here too.
    if colour_channels == 0 {
        return Err(corrupt("codestream declares no colour channels"));
    }
    // The tile grid is declared independently of the image size, so this
    // is a THIRD ceiling and not a restatement of the first two: the
    // fuzz-found 512 x 1024 / 65,536-tile codestream is two orders of
    // magnitude inside both of the above. `hayro-jpeg2000` keeps the
    // tile parameters private, so they are re-read from the SIZ marker
    // here — a bounded, fixed-offset read on a segment `Image::new` has
    // already validated. `None` means the marker could not be located,
    // which cannot happen after a successful parse; the guard declines
    // to refuse an image on the strength of its own second-guessing.
    if tile_count(data).is_some_and(|tiles| tiles > MAX_TILES) {
        return Err(ImageCodecError::TooLarge);
    }

    let smask = smask_in_data(doc, dict);

    let mut context = DecoderContext::default();
    let decoded = image.decode(&mut context).map_err(|e| map_error(e, data))?;
    let components = decoded.components();

    // The vendor sorts colour channels by their `cdef` association and
    // places an opacity channel last, so "the first N are colour, the
    // last is alpha" is its contract rather than our assumption. A
    // shorter list than the colour space calls for is a corrupt file.
    let colour_len = color.num_channels() as usize;
    let Some(colour_planes) = components.get(..colour_len) else {
        return Err(corrupt(
            "fewer decoded components than the colour space requires",
        ));
    };

    // Every component's declared depth is checked BEFORE any scaling, so
    // the `1 << depth` below can never overflow (module docs).
    for plane in components {
        let depth = plane.bit_depth();
        if depth == 0 || depth > MAX_COMPONENT_BIT_DEPTH {
            return Err(ImageCodecError::FeatureUnsupported {
                feature: "JPX/bit-depth",
            });
        }
    }

    // Row count is derived from the samples actually present rather than
    // from the declared height, exactly as the bilevel adapters do:
    // reporting the emitted count is what lets `pdfcer-render` mark a
    // short image `truncated` instead of reading past the buffer.
    let plane_len = colour_planes.first().map_or(0, |p| p.samples().len());
    let width_usize = width as usize;
    // `checked_div` rather than an `if width == 0` guard: a zero width
    // is already impossible here (the SIZ marker forbids a zero
    // reference grid and the ceiling check ran above), but the division
    // must be visibly total either way, and the `None` branch folds
    // into the `rows == 0` refusal below.
    let rows = plane_len
        .checked_div(width_usize)
        .map_or(0, |r| r.min(height as usize));
    if rows == 0 {
        return Err(corrupt("no image rows decoded"));
    }
    let pixel_count = rows.saturating_mul(width_usize);

    let mut samples = vec![0u8; pixel_count.saturating_mul(colour_len)];
    for (index, plane) in colour_planes.iter().enumerate() {
        interleave(
            plane.samples(),
            plane.bit_depth(),
            index,
            colour_len,
            &mut samples,
        );
    }

    // §7.4.9's opacity channel, gated by Table 89's `/SMaskInData`.
    let alpha_plane = if has_alpha {
        components.get(colour_len)
    } else {
        None
    };
    let embedded_alpha = match (smask, alpha_plane) {
        (SMaskInData::Opacity, Some(plane)) => {
            let mut alpha = vec![0u8; pixel_count];
            interleave(plane.samples(), plane.bit_depth(), 0, 1, &mut alpha);
            Some(alpha)
        }
        // Value 2: recognized, counted, and NOT applied — the colour
        // samples above are the preblended ones and are returned as
        // such (module docs).
        (SMaskInData::Preblended, _) => {
            notes.jpx_smask_in_data_preblended = true;
            None
        }
        // Value 0 is the default and means "shall be ignored"; a
        // nonzero value with no opacity channel to point at is a
        // dictionary that disagrees with its own codestream.
        (SMaskInData::Opacity, None) => {
            notes.geometry_mismatch = true;
            None
        }
        (SMaskInData::Ignore, _) => None,
    };

    let (color_model, icc_profile) = color_model(&color);

    notes.geometry_mismatch |= geometry_disagrees(doc, dict, width, height, colour_len);

    Ok(CodedImage {
        samples,
        codec: Some(Codec::Jpx),
        width,
        height: u32::try_from(rows).unwrap_or(height),
        components: u8::try_from(colour_len).unwrap_or(u8::MAX),
        // The depth pdfcer DELIVERS, which Table 89 makes the conforming
        // reader's choice — not the depth the codestream stored.
        bits_per_component: 8,
        color_model,
        icc_profile,
        embedded_alpha,
        notes: *notes,
    })
}

/// Scale one planar component to 8 bits and write it into an interleaved
/// buffer at `index`, striding by `stride` components per pixel.
///
/// The scale is the module docs' `round(v / (2^d − 1) × 255)`, clamped:
/// the inverse wavelet transform can legitimately produce values a
/// little outside `0..=2^d − 1` at sharp edges (T.800 Annex F's lifting
/// steps are not range-preserving), and §8.9.5.2's own instruction for
/// an out-of-range value is to adjust it "to the nearest allowed value".
///
/// `bit_depth` is guaranteed in `1..=31` by the caller's check, so the
/// shift cannot overflow and the divisor cannot be zero.
fn interleave(plane: &[f32], bit_depth: u8, index: usize, stride: usize, out: &mut [u8]) {
    let span = ((1u32 << bit_depth) - 1) as f32;
    let scale = 255.0 / span;
    for (slot, &sample) in out.iter_mut().skip(index).step_by(stride).zip(plane) {
        *slot = (sample * scale + 0.5).clamp(0.0, 255.0) as u8;
    }
}

/// Read `/SMaskInData` (Table 89), defaulting to `0`.
///
/// Values outside 0..=2 are undefined by the spec. They are treated as
/// the default — "shall be ignored" — rather than guessed at: the two
/// alternatives are to refuse the image (a total loss over one stray
/// integer) or to pick one of the defined meanings (inventing a rule the
/// spec does not have). Ignoring the codestream's alpha is the outcome
/// that cannot corrupt what is drawn.
fn smask_in_data(doc: &DocumentView<'_>, dict: &Dict) -> SMaskInData {
    match dict
        .get(b"SMaskInData")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_int)
    {
        Some(1) => SMaskInData::Opacity,
        Some(2) => SMaskInData::Preblended,
        _ => SMaskInData::Ignore,
    }
}

/// Map the vendor's colour space onto pdfcer's codec-level colour model,
/// carrying any embedded ICC profile out untouched.
///
/// The ICC branch walks the tail of §7.4.9's fallback ladder; see the
/// module docs for why the profile's own data-colour-space signature is
/// consulted rather than the channel count alone.
fn color_model(space: &ColorSpace) -> (CodecColorModel, Option<Vec<u8>>) {
    match space {
        ColorSpace::Gray => (CodecColorModel::Gray, None),
        ColorSpace::RGB => (CodecColorModel::Rgb, None),
        ColorSpace::CMYK => (CodecColorModel::Cmyk, None),
        ColorSpace::Unknown { num_channels } => (
            CodecColorModel::Unknown {
                components: *num_channels,
            },
            None,
        ),
        ColorSpace::Icc {
            profile,
            num_channels,
        } => {
            let model = match icc_data_color_space(profile) {
                // The profile says what its own samples are. These three
                // are the ones pdfcer's `N`-component fallback
                // approximates correctly.
                Some(b"GRAY") => CodecColorModel::Gray,
                Some(b"RGB ") => CodecColorModel::Rgb,
                Some(b"CMYK") => CodecColorModel::Cmyk,
                // A profile pdfcer cannot approximate as a device space —
                // `Lab `, `YCbr`, `2CLR`… Refused rather than painted as
                // if it were RGB (module docs).
                Some(_) => CodecColorModel::Unknown {
                    components: *num_channels,
                },
                // No readable profile header: fall to §7.4.9's terminal
                // rule, "DeviceGray, DeviceRGB, or DeviceCMYK, depending
                // on whether the number of channels … is 1, 3, or 4".
                None => match num_channels {
                    1 => CodecColorModel::Gray,
                    3 => CodecColorModel::Rgb,
                    4 => CodecColorModel::Cmyk,
                    n => CodecColorModel::Unknown { components: *n },
                },
            };
            (model, Some(profile.clone()))
        }
    }
}

/// The four-character *data colour space* signature of an ICC profile.
///
/// ICC.1 fixes the profile header at 128 bytes with the data colour
/// space at offset 16 — `'GRAY'`, `'RGB '`, `'CMYK'`, `'Lab '` and so
/// on. Reading four bytes at a fixed offset is the whole of the parse;
/// pdfcer does not otherwise interpret ICC profiles, and this is used
/// only to decide whether the device-space approximation is honest.
/// Returns `None` for anything too short to carry a header.
fn icc_data_color_space(profile: &[u8]) -> Option<&[u8; 4]> {
    profile.get(16..20)?.try_into().ok()
}

/// Does the image dictionary disagree with the codestream?
///
/// §7.4.9 requires `/Width` and `/Height` to "match the corresponding
/// width and height values in the JPEG2000 data" but gives **no**
/// conflict-resolution rule, and Table 89 makes `/BitsPerComponent`
/// "optional and … ignored if present". So all three are compared and
/// none is acted on here: the divergence is counted, and
/// `pdfcer-render` keeps the dictionary's numbers for placement and the
/// codestream's for reading, which is the only split that neither
/// shears the picture nor moves it.
///
/// `/BitsPerComponent` counts as a disagreement whenever it is present
/// and is not the 8 pdfcer delivers — including the common, superficially
/// harmless case of a producer writing `16` for a 16-bit codestream.
/// That is not pedantry: it is the one entry a reader is actively told
/// to ignore, so a file carrying it is a file whose other entries
/// deserve a second look.
///
/// An **absent** entry is never a disagreement — Table 89 makes
/// `/ColorSpace` and `/BitsPerComponent` optional for this filter, which
/// is the entire point of the audit that preceded this Pass.
fn geometry_disagrees(
    doc: &DocumentView<'_>,
    dict: &Dict,
    width: u32,
    height: u32,
    components: usize,
) -> bool {
    let int = |key: &[u8]| -> Option<i64> {
        dict.get(key)
            .map(|o| doc.resolve(o))
            .and_then(Object::as_int)
    };
    let differs = |key: &[u8], actual: u32| -> bool {
        int(key).is_some_and(|v| u32::try_from(v).map(|v| v != actual).unwrap_or(true))
    };
    differs(b"Width", width)
        || differs(b"Height", height)
        || differs(b"BitsPerComponent", 8)
        // §7.4.9: "The number of colour channels in the JPEG2000 data
        // shall match the number of components in the colour space."
        // Only checkable here for the device-space names; the general
        // case (ICCBased, Indexed, a named resource) needs the resource
        // dictionary and is checked by the renderer.
        || dict_colorspace_components(doc, dict)
            .is_some_and(|n| n != components)
}

/// Whether the image dictionary's `/ColorSpace` is an `/Indexed` array.
///
/// # Why this is a separate question from the component count
///
/// `dict_colorspace_components` answers *"how many channels does the PDF
/// expect?"*, which for `/Indexed` is always 1 and says nothing about who
/// owns the palette. This answers *"does the PDF carry its own lookup
/// table?"* — and that decides whether the JPEG 2000 decoder should resolve
/// the codestream's own palette or leave the samples as indices.
///
/// # What it deliberately cannot see
///
/// A `/ColorSpace` written as a **name** (`/CS0`) resolves through the page's
/// resource dictionary, which the codec layer does not have and should not
/// acquire — the codec's job is bytes, not page structure. Such a name
/// returns `false`, keeping the pre-existing behaviour rather than guessing.
///
/// That is a real, named limit rather than an oversight: a JPX image with a
/// palette AND a named `/Indexed` resource would still double-resolve. No
/// such file is known, the shape is checkable if one turns up, and the
/// alternative — plumbing resources into the codec to catch a hypothetical —
/// costs more than it protects.
pub(super) fn dict_colorspace_is_indexed(doc: &DocumentView<'_>, dict: &Dict) -> bool {
    let Some(cs) = dict.get(b"ColorSpace").map(|o| doc.resolve(o)) else {
        return false;
    };
    // Only the array form is decidable here. A bare name is either a device
    // space (never Indexed) or a resource lookup (not visible from here).
    cs.as_array()
        .and_then(|items| items.first())
        .map(|o| doc.resolve(o))
        .and_then(Object::as_name)
        .is_some_and(|n| n.as_bytes() == b"Indexed")
}

/// The component count implied by a `/ColorSpace` given as a device
/// name, or `None` when the entry is absent or is any other shape.
///
/// Deliberately narrow. Resolving `[/ICCBased …]`, `[/Indexed …]` or a
/// named resource needs the page's resource dictionary, which the codec
/// layer does not have and should not acquire — `pdfcer-render` already
/// performs the full comparison once it has resolved the space. This
/// covers the case a JPX image actually hits in practice (a bare
/// `/DeviceRGB` beside a grayscale codestream) without duplicating the
/// colour-space resolver.
fn dict_colorspace_components(doc: &DocumentView<'_>, dict: &Dict) -> Option<usize> {
    match dict.get(b"ColorSpace").map(|o| doc.resolve(o))? {
        Object::Name(name) => match name.as_bytes() {
            b"DeviceGray" | b"CalGray" | b"G" => Some(1),
            b"DeviceRGB" | b"CalRGB" | b"RGB" => Some(3),
            b"DeviceCMYK" | b"CMYK" => Some(4),
            _ => None,
        },
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

/// Wrap a `hayro-jpeg2000` failure as a structured pdfcer error, naming
/// the one class of failure that is a *missing feature* rather than a
/// broken file.
///
/// `MarkerError::Unsupported` is raised by both the main-header and the
/// tile-part-header parsers for any marker segment the decoder does not
/// handle, and it carries no marker code. Upstream names progression
/// order changes in tile-parts as its known gap, and `POC` (T.800 Table
/// A.2, `0xFF5F`) is precisely the marker that expresses one — so the
/// codestream is walked once, here, to turn "unsupported marker" into
/// the name an operator can act on (rule R27).
///
/// Every other variant is a genuine corrupt/invalid-input report and
/// keeps the vendor's own `Display` text, which names the actual fault
/// (`"missing COD marker"`, `"invalid tile offsets"`, …) rather than
/// collapsing to "decode failed".
fn map_error(err: hayro_jpeg2000::DecodeError, data: &[u8]) -> ImageCodecError {
    use hayro_jpeg2000::{DecodeError, FormatError, MarkerError};
    match err {
        DecodeError::Marker(MarkerError::Unsupported) => ImageCodecError::FeatureUnsupported {
            feature: unsupported_marker_feature(data),
        },
        // `FormatError::Unsupported` has exactly one origin in
        // `hayro-jpeg2000` 0.4.0: the `colr` box declares an
        // **enumerated colour space** the crate does not implement. Its
        // supported set is CMYK (12), CIELab (14), sRGB (16), greyscale
        // (17), sYCC (18), e-sRGB (20) and ROMM-RGB (21). The gap that
        // matters for PDF is **19, CIEJab**, which §7.4.9 singles out as
        // permitted ("limited to the JPX baseline set of features,
        // except for enumerated colour space 19 (CIEJab)") — one
        // veraPDF corpus file uses it.
        //
        // KNOWN DIVERGENCE, recorded rather than papered over: §7.4.9's
        // colour ladder ends "if no supported colour space is found, the
        // colour space used shall be DeviceGray, DeviceRGB, or
        // DeviceCMYK, depending on … whether the number of channels …
        // is 1, 3, or 4", so a fully conformant reader would fall back
        // and draw the image with (wrong) device colours. pdfcer cannot
        // reach that rung: `Image::new` fails during colour resolution,
        // before any sample exists, and there is no vendor setting that
        // defers it. Refusing with a name is the honest position —
        // nothing drawn, the reason greppable — where the alternative
        // is silently painting CIEJab coordinates as if they were RGB.
        // Revisit when upstream can decode-then-report an unsupported
        // enumerated space.
        DecodeError::Format(FormatError::Unsupported) => ImageCodecError::FeatureUnsupported {
            feature: "JPX/enumerated-colour-space",
        },
        other => ImageCodecError::Corrupt {
            codec: Codec::Jpx,
            detail: other.to_string(),
        },
    }
}

/// A corrupt-stream error raised by pdfcer's own checks.
fn corrupt(detail: &str) -> ImageCodecError {
    ImageCodecError::Corrupt {
        codec: Codec::Jpx,
        detail: detail.to_owned(),
    }
}

/// T.800 Table A.2 marker code for a progression order change.
const MARKER_POC: u8 = 0x5F;
/// Start of tile-part.
const MARKER_SOT: u8 = 0x90;
/// Start of data — after this the tile-part carries entropy-coded
/// packets, not marker segments.
const MARKER_SOD: u8 = 0x93;
/// End of codestream.
const MARKER_EOC: u8 = 0xD9;
/// Start of codestream.
const MARKER_SOC: u8 = 0x4F;

/// Upper bound on marker segments walked while naming a diagnostic.
///
/// The walk is a diagnostic refinement on an already-failing path, so it
/// must be cheap and unconditionally terminating. Every step either
/// advances by at least two bytes or stops, which already bounds it by
/// the input length; this second bound keeps a pathological
/// many-tiny-segments codestream from turning an error report into a
/// measurable cost.
const MAX_MARKERS_WALKED: usize = 4096;

/// Name the missing feature behind a `MarkerError::Unsupported`.
///
/// Walks the codestream's **header** marker segments — the main header
/// up to the first `SOT`, then each tile-part header up to its `SOD` —
/// looking for `POC`. Entropy-coded packet data is skipped using the
/// tile-part length `Psot` from each `SOT` segment (T.800 A.4.2), so no
/// packet byte is ever mistaken for a marker.
///
/// Returns `"JPX/progression-order-change"` when a `POC` marker is
/// found and `"JPX/unsupported-marker"` otherwise, which is also the
/// answer for a JP2 box container whose codestream cannot be located or
/// for a walk that runs off the end. It never changes whether an image
/// is accepted — only what the refusal is called — so a disagreement
/// with the vendor's parser costs a less precise diagnostic and nothing
/// else.
fn unsupported_marker_feature(data: &[u8]) -> &'static str {
    const GENERIC: &str = "JPX/unsupported-marker";
    const POC: &str = "JPX/progression-order-change";

    let Some(mut pos) = codestream_start(data) else {
        return GENERIC;
    };
    pos += 2; // past SOC

    let be16 = |at: usize| -> Option<usize> {
        let bytes: [u8; 2] = data.get(at..at + 2)?.try_into().ok()?;
        Some(u16::from_be_bytes(bytes) as usize)
    };
    let be32 = |at: usize| -> Option<u64> {
        let bytes: [u8; 4] = data.get(at..at + 4)?.try_into().ok()?;
        Some(u32::from_be_bytes(bytes).into())
    };

    for _ in 0..MAX_MARKERS_WALKED {
        if data.get(pos) != Some(&0xFF) {
            return GENERIC;
        }
        let Some(&code) = data.get(pos + 1) else {
            return GENERIC;
        };
        match code {
            MARKER_POC => return POC,
            MARKER_EOC | MARKER_SOD => return GENERIC,
            // A.4.1: markers 0xFF30..=0xFF3F and SOC carry no segment.
            MARKER_SOC | 0x30..=0x3F => pos += 2,
            MARKER_SOT => {
                // SOT segment (A.4.2): Lsot u16, Isot u16, Psot u32.
                // `Psot` counts from the first byte of the SOT MARKER to
                // the end of the tile-part; 0 means "to the end of the
                // codestream", i.e. this is the last tile-part.
                let (Some(lsot), Some(psot)) = (be16(pos + 2), be32(pos + 6)) else {
                    return GENERIC;
                };
                let tile_part = pos;
                // Scan this tile-part's header for POC before skipping
                // its packet data.
                let mut inner = pos + 2 + lsot;
                for _ in 0..MAX_MARKERS_WALKED {
                    if data.get(inner) != Some(&0xFF) {
                        break;
                    }
                    let Some(&inner_code) = data.get(inner + 1) else {
                        break;
                    };
                    match inner_code {
                        MARKER_POC => return POC,
                        MARKER_SOD | MARKER_EOC => break,
                        0x30..=0x3F => inner += 2,
                        _ => match be16(inner + 2) {
                            Some(len) if len >= 2 => inner += 2 + len,
                            _ => break,
                        },
                    }
                }
                if psot == 0 {
                    return GENERIC;
                }
                let Ok(step) = usize::try_from(psot) else {
                    return GENERIC;
                };
                let Some(next) = tile_part.checked_add(step) else {
                    return GENERIC;
                };
                pos = next;
            }
            // Every other marker is a segment: two-byte length, which
            // includes itself, so a value below 2 is malformed and would
            // not advance the walk.
            _ => match be16(pos + 2) {
                Some(len) if len >= 2 => pos += 2 + len,
                _ => return GENERIC,
            },
        }
    }
    GENERIC
}

/// The number of tiles the SIZ marker declares, per T.800 equation B-5.
///
/// ```text
/// numXtiles = ceil((Xsiz − XTOsiz) / XTsiz)
/// numYtiles = ceil((Ysiz − YTOsiz) / YTsiz)
/// ```
///
/// Re-read from the codestream because `hayro-jpeg2000` keeps its
/// `Header` private and exposes only the image dimensions — and because
/// the tile count is the quantity that bounds *decode work*, which no
/// pixel or byte ceiling can see (see [`MAX_TILES`]).
///
/// This is a fixed-offset read into a segment `Image::new` has already
/// parsed and validated, so it is a second look rather than a second
/// parser: SIZ is at a known position (T.800 A.5.1) and every field is
/// a fixed-width big-endian integer.
///
/// ```text
/// +0 SOC | +2 SIZ | +4 Lsiz | +6 Rsiz | +8 Xsiz | +12 Ysiz
/// +16 XOsiz | +20 YOsiz | +24 XTsiz | +28 YTsiz | +32 XTOsiz | +36 YTOsiz
/// ```
///
/// Returns `None` when the codestream cannot be located or the segment
/// is short — cases that cannot follow a successful parse, and which the
/// caller deliberately treats as "no opinion" rather than as a refusal.
fn tile_count(data: &[u8]) -> Option<u64> {
    let soc = codestream_start(data)?;
    if data.get(soc + 2..soc + 4) != Some(&[0xFF, 0x51]) {
        return None;
    }
    let be32 = |offset: usize| -> Option<u64> {
        let at = soc.checked_add(offset)?;
        let bytes: [u8; 4] = data.get(at..at + 4)?.try_into().ok()?;
        Some(u32::from_be_bytes(bytes).into())
    };
    let (x_size, y_size) = (be32(8)?, be32(12)?);
    let (x_tile, y_tile) = (be32(24)?, be32(28)?);
    let (x_tile_offset, y_tile_offset) = (be32(32)?, be32(36)?);
    // A zero tile size and an offset past the image are both rejected by
    // `size_marker` upstream, but this function must be total on its own
    // terms: it also runs on inputs the fuzzer builds by hand.
    let across = x_size.checked_sub(x_tile_offset)?.div_ceil(x_tile.max(1));
    let down = y_size.checked_sub(y_tile_offset)?.div_ceil(y_tile.max(1));
    Some(across.saturating_mul(down))
}

/// Locate the `SOC` marker that starts the T.800 codestream.
///
/// Two shapes reach here. A **bare codestream** starts with
/// `FF 4F FF 51` at offset 0. A **JP2 container** wraps it in a
/// `jp2c` box, so the box list is walked to find it — the same walk
/// `hayro-jpeg2000` does, but tolerating any failure by returning
/// `None`, because this is only ever refining a diagnostic.
fn codestream_start(data: &[u8]) -> Option<usize> {
    if data.starts_with(&[0xFF, MARKER_SOC, 0xFF, 0x51]) {
        return Some(0);
    }
    // JP2 box structure (ISO/IEC 15444-1 Annex I): LBox u32, TBox u32,
    // then an optional XLBox u64 when LBox == 1. LBox == 0 means "to the
    // end of the file".
    let mut pos = 0usize;
    for _ in 0..MAX_MARKERS_WALKED {
        let lbox: [u8; 4] = data.get(pos..pos + 4)?.try_into().ok()?;
        let kind = data.get(pos + 4..pos + 8)?;
        let mut length = u64::from(u32::from_be_bytes(lbox));
        let mut header = 8usize;
        if length == 1 {
            let xl: [u8; 8] = data.get(pos + 8..pos + 16)?.try_into().ok()?;
            length = u64::from_be_bytes(xl);
            header = 16;
        } else if length == 0 {
            length = (data.len() - pos) as u64;
        }
        if kind == b"jp2c" {
            return Some(pos + header);
        }
        let step = usize::try_from(length).ok()?.max(header);
        pos = pos.checked_add(step)?;
    }
    None
}

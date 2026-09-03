//! # DCTDecode (ISO 32000-1 §7.4.8, Table 13; ITU-T T.81) — the JPEG adapter
//!
//! Spec source: `filters/filter__dct.md` in the PDF-spec RAG, whose
//! Table 13 quotation was **verified against the source PDF on
//! 2026-07-30** (printed pp. 34–35). The codec itself is ITU-T T.81.
//! Crate choice and the colour-routing design are
//! `docs/decisions/005-image-codecs.md` §4.1.
//!
//! ## What this module is, and is not
//!
//! It is an **adapter**, not a decoder. `zune-jpeg` does the Huffman
//! decoding, the IDCT, the upsampling, the restart-interval handling
//! and the progressive scan merging. Everything here is glue, and the
//! glue is where pdfcer's bugs would live (decision 005 §6.5) — so the
//! glue is what carries the doc comments, the ceilings and the fuzz
//! target.
//!
//! Four jobs, in order:
//!
//! 1. **Pre-sniff the codestream's markers** so unsupported features
//!    become *named* diagnostics (rule R27) instead of an opaque vendor
//!    error string, and so the Adobe APP14 transform byte is available
//!    to Table 13's precedence rule.
//! 2. **Apply Table 13's colour contract** by choosing what to ask
//!    `zune-jpeg` to output — see the routing table below.
//! 3. **Impose pdfcer's own resource ceilings** (rule R25), never the
//!    vendor defaults.
//! 4. **Report the codestream's own geometry and colour model** back to
//!    the caller, applying no `/Decode` and no polarity flip of its own
//!    (rule R26).
//!
//! ## Table 13, verbatim, as a decision procedure
//!
//! > "**If the encoding algorithm has inserted the Adobe-defined marker
//! > code in the encoded data** indicating the `ColorTransform` value,
//! > then the colours shall be transformed, or not, after the DCT
//! > decoding has been performed **according to the value provided in
//! > the encoded data** and **the value of this dictionary entry shall
//! > be ignored**. If the Adobe-defined marker code … is **not
//! > present** then the value specified in this dictionary entry will
//! > be used. If [neither] is present … the **default value of
//! > `ColorTransform` shall be 1 if the image has three components and
//! > 0 otherwise**."
//!
//! ```text
//! if APP14 "Adobe" marker present:      transform = the MARKER's byte
//! elif /ColorTransform in /DecodeParms: transform = the DICTIONARY's value
//! else:                                 transform = 1 if components == 3 else 0
//! ```
//!
//! Two consequences that are easy to get backwards, both called out in
//! `filter__dct.md`: the **marker outranks the dictionary
//! unconditionally** (pdfcer must never let `/DecodeParms` override an
//! APP14 value), and the fallback default is **component-count
//! dependent** — a 4-component JPEG with neither defaults to `0`, i.e.
//! *no* transform, not to `1`.
//!
//! Table 13 also says the parameter "**shall be ignored if the image
//! has one or two colour components**", so a grayscale JPEG carrying
//! `/ColorTransform 1` is not an error — the value is simply not
//! consulted.
//!
//! ## The colour-routing table (decision 005 §4.1)
//!
//! | Components | Effective transform | Asked of `zune-jpeg` | [`CodecColorModel`] |
//! |---|---|---|---|
//! | 1 | *ignored* | `Luma` | `Gray` |
//! | 3 | 1 | **`RGB`** — zune applies the T.81 YCbCr→RGB inverse | `Rgb` |
//! | 3 | 0 | the codestream's own model (passthrough) | `Untransformed3` |
//! | 4 | 0 | `CMYK` (passthrough) | `Cmyk` |
//! | 4 | 1 or 2 | passthrough, then **pdfcer's own** YCC→CMY inverse | `Cmyk` |
//! | any | > 2 | — | named diagnostic, nothing decoded |
//!
//! `zune-jpeg` has a `YCCK → RGB` arm but **no `YCCK → CMYK` arm**
//! (verified in `zune-jpeg-0.5.15/src/worker.rs`), and its
//! `YCCK → RGB` additionally composites K assuming the Adobe inverted
//! convention. Neither is what §7.4.8 asks for, so pdfcer requests a raw
//! passthrough and does the ~20-line inverse itself — which is the
//! right place for it anyway under rule R26.
//!
//! ## The 3-component/transform-0 fixup, and why it is handled here
//!
//! `zune-jpeg` maps an APP14 transform byte of `0` to `CMYK`
//! unconditionally (`headers.rs:485-514`), then corrects it to `RGB`
//! once the SOF component count is known — but that correction lives in
//! `misc.rs:209`, which runs inside `decode_into`, **not** inside
//! `decode_headers`. So `input_colorspace()` reports `CMYK` for a
//! 3-component transform-0 image at the moment pdfcer has to choose an
//! output colourspace. Asking for `CMYK` there would allocate a
//! 4-component buffer and then fail with an unimplemented
//! `(RGB, CMYK)` mapping. [`passthrough_target`] encodes the fixup on
//! pdfcer's side so the request is right the first time.
//!
//! ## The CMYK-inversion question is SETTLED: never invert (R29)
//!
//! Decision 006 (`docs/decisions/006-cmyk-jpeg-inversion.md`) closed
//! decision 005 §5.5's deliberately-open question. The rule is the
//! null rule, now standing rule **R29**: pdfcer **never** applies an
//! "Adobe CMYK inversion" — not on APP14 presence, not on
//! transform-byte value, not on component count, not on producer
//! sniffing. The APP14 transform byte is consumed for exactly one
//! purpose, the one this module already implements: selecting the
//! Table 13 colour transform. `/Decode` is the sole polarity control,
//! applied by `pdfcer-render` (§8.9.5.2) — `/Decode [1 0 1 0 1 0 1 0]`
//! *is* the sanctioned mechanism by which a producer declares inverted
//! storage.
//!
//! Why this is sourced fact rather than caution: pdf.js, pdfium, MuPDF
//! (PDF path) and Poppler all implement exactly this; marker-gated
//! inversion has been shipped and reverted twice upstream (cairo issue
//! 156, Firefox bug 674619); and the normative-by-reference primary,
//! Adobe TN #5116 (ISO 32000-1 §7.4.8 footnote *a*), contains the word
//! "invert" **zero times** — its §13.1 defines the CMYK→YCCK forward
//! transform on *true ink values* (`R = 255−C` etc., `K` untouched),
//! so [`ycck_to_cmyk_in_place`]'s inverse recovers true ink directly
//! and no further polarity step exists to take. Empirically, pdfcer
//! pixel-matches pdfium on every four-component JPEG in the corpus
//! (decision 006 §3.2) — the plausible "invert on APP14" guess that
//! §5.5 declined to make would have broken all of them.
//!
//! The one residual risk — a 4-component stream with effective
//! transform 0 **and** no `/Decode`, where the undocumented Photoshop
//! inverted-storage convention has nothing in the codestream or the
//! dictionary to disambiguate it — is **reported, never repaired**
//! (rule R30): [`CodecNotes::cmyk_polarity_unverifiable`] names it,
//! distinct from the benign YCCK census
//! ([`CodecNotes::cmyk_image`]). Observing the dictionary's `/Decode`
//! for that classification is permitted by R26's decision-006
//! clarification (*observing is not applying*); actually applying
//! `/Decode` remains `pdfcer-render`'s job alone.
//!
//! ## R169: the operator gets an escape hatch, R29 keeps the default
//!
//! Standing rule **R169** (2026-08-08) says a genuine spec ambiguity
//! becomes an operator setting whose installed default is the best guess
//! at what is usually followed. `DCT-A1` is exactly that shape, and its
//! best guess is R29 — so [`CmykJpegPolarity::NeverInvert`] is the
//! default and the paragraph above still describes what pdfcer does out of
//! the box. What R169 adds is [`CmykJpegPolarity::InvertOnApp14`], for the
//! operator who *knows* their corpus is old Photoshop output: it
//! complements all four components for the ambiguous shape alone, and
//! [`complement_in_place`] documents why that is a convention rather than
//! a transform. The R30 note is raised either way — it describes the file,
//! not the configuration. R29 is narrowed from "pdfcer never inverts" to
//! "pdfcer never inverts **unless the operator explicitly asked, for the
//! one shape nothing can decide**", which is a disclosure gain, not a
//! weakening: the alternative to a named setting is an operator silently
//! living with negatives.

use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

use super::{
    Codec, CodecColorModel, CodecNotes, CodedImage, ImageCodecError, MAX_IMAGE_DIMENSION,
    MAX_IMAGE_PIXELS, MAX_IMAGE_SAMPLE_BYTES,
};
use crate::settings::CmykJpegPolarity;
// decision 018: the codecs resolve indirect entries through a `DocumentView`
// rather than a `&Document`, so an image whose dictionary lives in an
// editing session decodes as the operator currently has it. `Document` is
// still named by the back-compat `decode_image` wrapper in `mod.rs`.
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::view::DocumentView;

/// Ceiling on progressive scans, against the progressive-scan bomb.
///
/// A conformant progressive JPEG needs roughly two scans per component
/// plus refinements — ten or so for a 3-component image, and the
/// corpus's progressive files sit well inside that. 100 is generous
/// enough that no legitimate encoder is refused while still bounding
/// the work a crafted file can demand. Set **explicitly** even though
/// it happens to equal `zune-core`'s current default, because rule R25
/// is about the number being one pdfcer chose and can be seen to have
/// chosen — an inherited default never appears in a diff.
const MAX_PROGRESSIVE_SCANS: usize = 100;

/// Decode a `DCTDecode` codestream.
///
/// `data` is the codestream *after* any byte-stream filter prefix has
/// been removed; `parms` is the codec's own `/DecodeParms` entry;
/// `dict` is the image dictionary, consulted only to compare geometry
/// (rule R26 keeps colour decisions out of here); `notes` accumulates
/// the honesty counters.
///
/// # Errors
///
/// [`ImageCodecError::FeatureUnsupported`] for arithmetic-coded,
/// lossless, differential or non-8-bit JPEG and for an Adobe transform
/// byte outside 0–2; [`ImageCodecError::TooLarge`] when a pdfcer ceiling
/// is crossed; [`ImageCodecError::Corrupt`] for anything `zune-jpeg`
/// rejects.
pub(super) fn decode(
    doc: &DocumentView<'_>,
    data: &[u8],
    parms: Option<&Dict>,
    dict: &Dict,
    polarity: CmykJpegPolarity,
    notes: &mut CodecNotes,
) -> Result<CodedImage, ImageCodecError> {
    let frame = sniff(data)?;

    // Table 13's three-level precedence chain, in the spec's own order.
    let effective_transform = match frame.adobe_transform {
        Some(marker) => marker,
        None => match dict_color_transform(doc, parms) {
            Some(value) => value,
            // "1 if the image has three components and 0 otherwise."
            None => u8::from(frame.components == 3),
        },
    };

    let (request, model) = route(&frame, effective_transform)?;

    // --- pdfcer's ceilings, set explicitly (rule R25) ------------------
    let options = DecoderOptions::default()
        .set_max_width(MAX_IMAGE_DIMENSION as usize)
        .set_max_height(MAX_IMAGE_DIMENSION as usize)
        .jpeg_set_max_scans(MAX_PROGRESSIVE_SCANS)
        .jpeg_set_out_colorspace(request);

    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(data), options);
    decoder.decode_headers().map_err(corrupt)?;

    let info = decoder
        .info()
        .ok_or_else(|| corrupt_detail("headers decoded but no image info"))?;
    let width = u32::from(info.width);
    let height = u32::from(info.height);

    // Geometry ceiling BEFORE any allocation: `output_buffer_size()` is
    // the decoder's own claim, so it is checked rather than trusted.
    if u64::from(width).saturating_mul(u64::from(height)) > MAX_IMAGE_PIXELS {
        return Err(ImageCodecError::TooLarge);
    }
    let buffer_size = decoder
        .output_buffer_size()
        .filter(|&n| n <= MAX_IMAGE_SAMPLE_BYTES)
        .ok_or(ImageCodecError::TooLarge)?;

    let mut samples = vec![0u8; buffer_size];
    decoder.decode_into(&mut samples).map_err(corrupt)?;

    // The one colour operation this module performs, and only because
    // zune-jpeg has no YCCK→CMYK arm (module docs). It is the inverse
    // of the transform Table 13 names ("YUVK to CMYK after decoding"),
    // not a polarity guess.
    if model == CodecColorModel::Cmyk && needs_ycck_inverse(effective_transform) {
        ycck_to_cmyk_in_place(&mut samples);
    }

    let components = out_components(request);
    // Decision 006 §4.4: two diagnostics, not one. The benign census
    // (effective transform 1/2 — the YCCK inverse is mandated and
    // verified against pdfium) is disjoint from the R30 shape
    // (transform 0 AND no /Decode — the one place the undocumented
    // Photoshop polarity convention can still bite, because nothing in
    // the codestream or the dictionary disambiguates it). Reading the
    // dictionary's /Decode PRESENCE here is diagnostic observation,
    // permitted by R26's decision-006 clarification; APPLYING /Decode
    // remains pdfcer-render's job alone.
    if model == CodecColorModel::Cmyk {
        if needs_ycck_inverse(effective_transform) {
            notes.cmyk_image = true;
        } else if !has_decode_array(doc, dict) {
            notes.cmyk_polarity_unverifiable = true;
            // `DCT-A1` (R169). THIS, and only this, is the configurable
            // case: four components, effective transform 0, an Adobe
            // marker present, and nothing in the dictionary to say which
            // way round the ink is stored. The note above is raised first
            // and unconditionally — the ambiguity is a property of the
            // FILE, so an operator who set a polarity still gets told the
            // file could not settle it (R30).
            //
            // `frame.adobe_transform.is_some()` is the whole test the
            // option's name promises, and it is deliberately a presence
            // test: APP14 carries **no polarity flag**, so there is no bit
            // to consult. That absence is exactly why R29 makes
            // `NeverInvert` the default and why the alternative can only
            // ever be a blunt instrument for a known-bad corpus.
            if polarity == CmykJpegPolarity::InvertOnApp14 && frame.adobe_transform.is_some() {
                complement_in_place(&mut samples);
            }
        }
    }
    notes.geometry_mismatch = geometry_disagrees(doc, dict, width, height);

    Ok(CodedImage {
        samples,
        codec: Some(Codec::Dct),
        width,
        height,
        components,
        // §7.4.8: "Each component value shall occupy a byte"; Table 89:
        // a DCTDecode filter "shall always deliver 8-bit samples". The
        // sniff already refused any other precision, so this is a fact
        // rather than an assumption.
        bits_per_component: 8,
        color_model: model,
        icc_profile: decoder.icc_profile(),
        // JPEG has no in-codestream alpha. `/SMaskInData` is "meaningless"
        // for anything but JPXDecode (Table 89), and a `/SMask` stream is
        // a separate image the renderer resolves.
        embedded_alpha: None,
        notes: *notes,
    })
}

// ---------------------------------------------------------------------------
// Colour routing (Table 13 → a zune-jpeg request)
// ---------------------------------------------------------------------------

/// What pdfcer asks `zune-jpeg` to hand back, and what that means.
///
/// Split out from [`decode`] so the module's routing table is one
/// readable `match` that a reviewer can compare against §4.1 line by
/// line, rather than being interleaved with buffer management.
fn route(frame: &Frame, transform: u8) -> Result<(ColorSpace, CodecColorModel), ImageCodecError> {
    // Table 13: an Adobe transform byte outside 0–2 has no defined
    // meaning. zune-jpeg treats it as a hard error deep inside header
    // parsing; pdfcer pre-sniffs precisely so it becomes a named,
    // countable diagnostic instead (rule R27).
    if let Some(marker) = frame.adobe_transform
        && marker > 2
    {
        return Err(ImageCodecError::FeatureUnsupported {
            feature: match marker {
                3 => "DCT/adobe-transform-3",
                _ => "DCT/adobe-transform-unknown",
            },
        });
    }

    match frame.components {
        // "This option shall be ignored if the image has one or two
        // colour components" — so `transform` is deliberately unread.
        1 => Ok((ColorSpace::Luma, CodecColorModel::Gray)),
        // Legal per §7.4.8 ("one, two, three, or four colour
        // components") but unsupported by zune-jpeg: its colour matrix
        // has no arm reaching a 2-component output, and its default
        // input model for 2 components is the 3-component YCbCr. Named
        // rather than mis-decoded.
        2 => Err(ImageCodecError::FeatureUnsupported {
            feature: "DCT/2-component",
        }),
        3 => match transform {
            // Passthrough. The request is RGB, NOT YCbCr, because of
            // the deferred fixup — see [`passthrough_target`].
            0 => Ok((
                passthrough_target(3, frame.adobe_transform),
                CodecColorModel::Untransformed3,
            )),
            1 => Ok((ColorSpace::RGB, CodecColorModel::Rgb)),
            // Transform 2 is YCCK, which is a 4-component model; on a
            // 3-component frame it is incoherent and zune has no path
            // for it. Named, not guessed.
            _ => Err(ImageCodecError::FeatureUnsupported {
                feature: "DCT/ycck-3-component",
            }),
        },
        // Table 13 for four components: transform 1 means "YUVK values
        // … transformed … from YUVK to CMYK after decoding", and the
        // APP14 byte 2 (YCCK) means the same storage. Both therefore
        // need the inverse; only transform 0 is genuinely raw CMYK.
        // Either way the request is a passthrough and the model is
        // CMYK — `needs_ycck_inverse` decides what happens after.
        4 => Ok((
            passthrough_target(4, frame.adobe_transform),
            CodecColorModel::Cmyk,
        )),
        // §7.4.8 allows only one, two, three or four components, so
        // anything else is a malformed frame header rather than an
        // exotic image. Named and counted (R27), never rendered.
        _ => Err(ImageCodecError::FeatureUnsupported {
            feature: "DCT/component-count",
        }),
    }
}

/// The `ColorSpace` to request when pdfcer wants the codestream's own
/// samples untouched.
///
/// `zune-jpeg` short-circuits to a padding-removing copy when
/// `input == output` and the component count is 3 or 4
/// (`worker.rs:36-44`), which is a genuine raw passthrough — exactly
/// what §7.4.8's `ColorTransform 0` requires. Getting there means
/// naming the model zune will *believe the input is* at decode time,
/// which is not always what `input_colorspace()` reports at header
/// time:
///
/// **3 components** split into two sub-cases that look alike and are
/// not, which is exactly why decision 005 §4.1 gave them separate rows:
///
/// - **APP14 present, transform 0.** zune sets its input model to
///   `CMYK` when it parses the marker and only corrects it to `RGB`
///   once the SOF component count is known — and that correction lives
///   in `misc.rs:209`, which runs inside `decode_into`, **not** inside
///   `decode_headers`. Asking for `CMYK` here would size a 4-component
///   buffer and then fail on an unimplemented `(RGB, CMYK)` mapping.
///   pdfcer already knows the component count from its own SOF sniff, so
///   it applies the fixup's conclusion up front and asks for **`RGB`**.
/// - **No APP14; `/ColorTransform 0` came from `/DecodeParms`.** No
///   marker means zune's input model is still its default `YCbCr`, so
///   the passthrough is reached by asking for **`YCbCr`**. Asking for
///   `RGB` here would make zune *apply* the inverse — the precise bug
///   this function exists to prevent, and the one the
///   `the_dictionary_wins_when_there_is_no_marker` test pins.
///
/// **4 components**: `YCCK` (APP14 transform 2) is what zune will
/// believe, and asking for it preserves all four channels. Every other
/// four-component case lands on `CMYK` — what zune infers for four
/// components with no APP14 at all, and what it corrects an APP14
/// transform byte of 0 or 1 to once the SOF component count is known
/// (`headers.rs:256`).
const fn passthrough_target(components: u8, adobe_transform: Option<u8>) -> ColorSpace {
    match components {
        1 => ColorSpace::Luma,
        3 => match adobe_transform {
            Some(0) => ColorSpace::RGB,
            _ => ColorSpace::YCbCr,
        },
        4 => match adobe_transform {
            Some(2) => ColorSpace::YCCK,
            _ => ColorSpace::CMYK,
        },
        // Unreachable: `route` refuses every other component count
        // before calling this. Luma is the least surprising fallback
        // for a total function.
        _ => ColorSpace::Luma,
    }
}

/// Does the effective transform mean the four stored components are
/// **YCCK** rather than raw CMYK?
///
/// Table 13: for a four-component image, `ColorTransform 1` means
/// "CMYK values shall be transformed to YUVK before encoding and from
/// YUVK to CMYK after decoding". The Adobe APP14 byte `2` denotes the
/// same YCCK storage. Only `0` — "no transformation" — is raw CMYK.
const fn needs_ycck_inverse(transform: u8) -> bool {
    transform == 1 || transform == 2
}

/// Component count implied by the colourspace pdfcer requested.
const fn out_components(space: ColorSpace) -> u8 {
    match space {
        ColorSpace::Luma => 1,
        ColorSpace::RGB | ColorSpace::YCbCr => 3,
        ColorSpace::CMYK | ColorSpace::YCCK => 4,
        _ => 0,
    }
}

/// In-place YCCK → CMYK, four interleaved 8-bit components per pixel.
///
/// The inverse of the forward transform Table 13 names for four
/// components ("CMYK values shall be transformed to YUVK before
/// encoding"). Forward: `R = 255−C, G = 255−M, B = 255−Y`, then
/// `RGB → YCbCr`, with `K` carried through untouched. Inverse,
/// therefore:
///
/// ```text
/// (R, G, B) = YCbCr→RGB(Y, Cb, Cr)
/// C = 255 − R,  M = 255 − G,  Y = 255 − B,  K unchanged
/// ```
///
/// The `YCbCr → RGB` coefficients are CCIR recommendation 601-1's, as
/// §7.4.8 NOTE 4 requires: "The RGB-to-YUV conversion provided by the
/// filters … conforms to CCIR recommendation 601-1." (`filter__dct.md`
/// records that ISO 32000-1's "YUV/YUVK" is what JPEG/JFIF calls
/// YCbCr/YCCK.) They are the same coefficients `zune-jpeg` uses for the
/// 3-component case, so a YCCK image and an otherwise identical YCbCr
/// image agree on the chroma maths.
///
/// **This is not the "Adobe inversion"** (module docs / decision 005
/// §5.5). The `255 − x` here is part of the YCCK definition itself
/// (TN #5116 §13.1 defines the forward transform on true ink values)
/// and applies to every YCCK file; settled by decision 006: there is
/// no Adobe inversion; `/Decode` is the sole polarity control (R29).
fn ycck_to_cmyk_in_place(samples: &mut [u8]) {
    for pixel in samples.chunks_exact_mut(4) {
        let (Some(&y), Some(&cb), Some(&cr)) = (pixel.first(), pixel.get(1), pixel.get(2)) else {
            continue;
        };
        let y = f32::from(y);
        let cb = f32::from(cb) - 128.0;
        let cr = f32::from(cr) - 128.0;
        let r = y + 1.402 * cr;
        let g = y - 0.344_136 * cb - 0.714_136 * cr;
        let b = y + 1.772 * cb;
        let invert = |v: f32| 255 - (v.clamp(0.0, 255.0).round() as u8);
        if let Some(slot) = pixel.first_mut() {
            *slot = invert(r);
        }
        if let Some(slot) = pixel.get_mut(1) {
            *slot = invert(g);
        }
        if let Some(slot) = pixel.get_mut(2) {
            *slot = invert(b);
        }
        // pixel[3] — K — is carried through untouched.
    }
}

/// Complement every byte: `x → 255 − x` (`DCT-A1`,
/// [`CmykJpegPolarity::InvertOnApp14`]).
///
/// ## This is NOT a colour transform, and the distinction is the point
///
/// [`ycck_to_cmyk_in_place`] above implements a transform Table 13
/// **mandates** — it is compliance, and it runs whatever the operator
/// configured. This function implements a **convention no document
/// defines**: some 1990s Photoshop output stores four-component JPEG
/// samples complemented, declares nothing, and leaves a reader with
/// nothing in the codestream or the dictionary to detect it by. It runs
/// only when the operator has explicitly asked, only for the one shape
/// that is genuinely undecidable, and never by default (R29).
///
/// All **four** components are complemented, K included — unlike the YCCK
/// inverse, where K is already true ink and is carried through untouched.
/// The convention being undone complemented the whole pixel.
///
/// Operating over the flat buffer rather than `chunks_exact(4)` is
/// deliberate: the operation is per-byte, so a buffer whose length is not
/// a multiple of four (which the ceilings above should already have made
/// impossible) still comes out consistently complemented rather than with
/// a differently-treated tail.
fn complement_in_place(samples: &mut [u8]) {
    for byte in samples.iter_mut() {
        *byte = 255 - *byte;
    }
}

// ---------------------------------------------------------------------------
// Marker pre-sniff (ITU-T T.81 §B.1)
// ---------------------------------------------------------------------------

/// What the marker walk learned about the codestream.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Frame {
    /// Components per sample from the SOF header.
    components: u8,
    /// The Adobe APP14 transform byte, if the marker is present.
    adobe_transform: Option<u8>,
}

/// Walk the codestream's marker chain to SOS, refusing unsupported
/// frame types **by name**.
///
/// This is the same walk decision 005 §3.2 used to measure the corpus's
/// 70 JPEG codestreams, and it exists for three reasons that a
/// "just call the decoder and map its error" approach cannot serve:
///
/// 1. `zune-jpeg` reports arithmetic-coded, lossless and 12-bit JPEG
///    through a single `UnsupportedSchemes` error whose text is not a
///    stable key; rule R27 requires a stable, countable name per
///    feature.
/// 2. An Adobe transform byte outside 0–2 is a **hard error** in
///    `zune-jpeg`'s header parser, so the value would never reach
///    pdfcer's Table 13 logic at all.
/// 3. Table 13's own default rule needs the component count *before*
///    deciding what to ask the decoder for.
///
/// T.81 §B.1.1.3: markers are `0xFF` followed by a non-zero, non-`0xFF`
/// byte; `0xFF00` is a stuffed data byte and repeated `0xFF`s are fill.
/// Standalone markers (SOI, EOI, TEM, RST0–7) carry no length; every
/// other marker segment starts with a 2-byte big-endian length that
/// *includes* those two bytes.
fn sniff(data: &[u8]) -> Result<Frame, ImageCodecError> {
    let mut frame = Frame::default();
    let mut i = 0usize;
    let mut seen_sof = false;

    // T.81 §B.2.1: the codestream begins with SOI.
    if data.first() != Some(&0xFF) || data.get(1) != Some(&0xD8) {
        return Err(corrupt_detail("codestream does not begin with SOI"));
    }
    i += 2;

    loop {
        // Skip fill bytes; find the next marker prefix.
        while data.get(i) == Some(&0xFF) && data.get(i + 1) == Some(&0xFF) {
            i += 1;
        }
        let (Some(&0xFF), Some(&marker)) = (data.get(i), data.get(i + 1)) else {
            // Ran out of markers without ever reaching a scan. If a SOF
            // was seen we already know everything this walk is for;
            // otherwise the decoder will reject it in a moment anyway.
            return if seen_sof {
                Ok(frame)
            } else {
                Err(corrupt_detail("no SOF marker before end of data"))
            };
        };
        i += 2;

        match marker {
            // Standalone markers: no length field (T.81 §B.1.1.3).
            0xD8 | 0x01 | 0xD0..=0xD7 => continue,
            // Start of scan — everything this walk needs precedes it.
            0xDA => {
                return if seen_sof {
                    Ok(frame)
                } else {
                    Err(corrupt_detail("SOS before any SOF marker"))
                };
            }
            0xD9 => {
                return if seen_sof {
                    Ok(frame)
                } else {
                    Err(corrupt_detail("EOI before any SOF marker"))
                };
            }
            _ => {}
        }

        // Every other marker introduces a segment: a 2-byte length that
        // counts itself, then `length - 2` payload bytes.
        let (Some(&hi), Some(&lo)) = (data.get(i), data.get(i + 1)) else {
            return Err(corrupt_detail("truncated marker segment length"));
        };
        let length = usize::from(u16::from_be_bytes([hi, lo]));
        let Some(payload_len) = length.checked_sub(2) else {
            return Err(corrupt_detail("marker segment length below 2"));
        };
        let Some(payload) = data.get(i + 2..i + 2 + payload_len) else {
            return Err(corrupt_detail("marker segment runs past end of data"));
        };
        i += 2 + payload_len;

        match marker {
            // SOF markers. The gaps are real: 0xC4 is DHT, 0xC8 is a
            // reserved JPG extension, 0xCC is DAC — none of them frame
            // headers, and treating them as such would read a component
            // count out of a Huffman table.
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                if seen_sof {
                    // A second frame header means a hierarchical or
                    // multi-frame stream; the first one is what the
                    // decoder will use.
                    continue;
                }
                seen_sof = true;
                if let Some(feature) = unsupported_sof(marker) {
                    return Err(ImageCodecError::FeatureUnsupported { feature });
                }
                // SOF payload: precision(1), height(2), width(2),
                // component count(1), then per-component triples.
                let (Some(&precision), Some(&components)) = (payload.first(), payload.get(5))
                else {
                    return Err(corrupt_detail("truncated SOF header"));
                };
                if precision != 8 {
                    return Err(ImageCodecError::FeatureUnsupported {
                        feature: match precision {
                            12 => "DCT/12-bit",
                            16 => "DCT/16-bit",
                            _ => "DCT/precision-unsupported",
                        },
                    });
                }
                frame.components = components;
            }
            // APP14. Only an "Adobe"-identified segment carries a
            // transform byte; any other APP14 is a different vendor's
            // and is ignored (T.81 leaves APPn contents to the
            // application).
            0xEE => {
                // Adobe layout: "Adobe"(5), version(2), flags0(2),
                // flags1(2), transform(1) = 12 payload bytes.
                if payload.starts_with(b"Adobe")
                    && let Some(&transform) = payload.get(11)
                {
                    frame.adobe_transform = Some(transform);
                }
            }
            _ => {}
        }
    }
}

/// Map an unsupported SOF marker to its stable diagnostic key (R27).
///
/// Returns `None` for the three frame types `zune-jpeg` implements:
/// SOF0 (baseline sequential), SOF1 (extended sequential) and SOF2
/// (progressive) — which between them cover 100% of the 70 codestreams
/// measured in decision 005 §3.2 (77% SOF0, 14% SOF2, 9% SOF0
/// grayscale).
const fn unsupported_sof(marker: u8) -> Option<&'static str> {
    match marker {
        // SOF0, SOF1, SOF2.
        0xC0..=0xC2 => None,
        // Lossless: SOF3 (huffman), SOF7 (differential), SOF11
        // (arithmetic), SOF15 (differential arithmetic). Named
        // "lossless" first because that is the property that makes them
        // a different codec, not merely a different entropy coder.
        0xC3 | 0xC7 | 0xCB | 0xCF => Some("DCT/lossless"),
        // Arithmetic entropy coding: SOF9, SOF10, SOF13, SOF14.
        0xC9 | 0xCA | 0xCD | 0xCE => Some("DCT/arithmetic"),
        // Differential (hierarchical) huffman: SOF5, SOF6.
        0xC5 | 0xC6 => Some("DCT/differential"),
        _ => Some("DCT/unsupported-frame"),
    }
}

// ---------------------------------------------------------------------------
// Dictionary reconciliation
// ---------------------------------------------------------------------------

/// Read `/ColorTransform` from the codec's `/DecodeParms`.
///
/// Table 13 enumerates only 0 and 1 for the dictionary entry. A value
/// outside that range is meaningless, and is treated as absent rather
/// than as an error — falling back to the component-count default,
/// which is what a reader with no `/DecodeParms` at all would do.
fn dict_color_transform(doc: &DocumentView<'_>, parms: Option<&Dict>) -> Option<u8> {
    let value = parms
        .and_then(|d| d.get(b"ColorTransform"))
        .map(|o| doc.resolve(o))
        .and_then(Object::as_int)?;
    match value {
        0 => Some(0),
        1 => Some(1),
        _ => None,
    }
}

/// Does the image dictionary carry a usable `/Decode` array?
///
/// Consulted **only** to classify the R30 diagnostic
/// ([`CodecNotes::cmyk_polarity_unverifiable`]) — a producer that
/// declares its polarity via `/Decode` has said what its samples mean,
/// so the residual-ambiguity warning does not apply. Presence means
/// "resolves to an array": a `/Decode` that resolves to anything else
/// is not a polarity declaration (and `pdfcer-render` will ignore it
/// with its own `decode_array_ignored` note). This function OBSERVES
/// the entry; it never applies it — R26 as clarified by decision 006.
fn has_decode_array(doc: &DocumentView<'_>, dict: &Dict) -> bool {
    matches!(
        dict.get(b"Decode").map(|o| doc.resolve(o)),
        Some(Object::Array(_))
    )
}

/// Does the image dictionary disagree with the codestream?
///
/// For DCT this is a **producer bug** either way — unlike JPX, where
/// Table 89 makes the codestream authoritative by design — so it is
/// counted and reported rather than acted on. The caller keeps the
/// dictionary's `/Width` and `/Height` for placement (§8.9.4 maps the
/// image onto the unit square regardless of sample count) and reads
/// samples with the codestream's stride, which is the only combination
/// that neither shears the picture nor moves it.
///
/// `/BitsPerComponent` is included: Table 89's opening rule makes an
/// entry inconsistent with the filter an error, and a DCTDecode image
/// "shall always deliver 8-bit samples", so any other stated value is a
/// disagreement worth surfacing.
fn geometry_disagrees(doc: &DocumentView<'_>, dict: &Dict, width: u32, height: u32) -> bool {
    let int = |key: &[u8]| -> Option<i64> {
        dict.get(key)
            .map(|o| doc.resolve(o))
            .and_then(Object::as_int)
    };
    let differs = |key: &[u8], actual: u32| -> bool {
        int(key).is_some_and(|v| u32::try_from(v).map(|v| v != actual).unwrap_or(true))
    };
    differs(b"Width", width) || differs(b"Height", height) || differs(b"BitsPerComponent", 8)
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

/// Wrap a `zune-jpeg` failure as a structured pdfcer error.
///
/// Every `zune-jpeg` failure mode is an `Err`, never a panic
/// (`worker.rs:125-133` returns `Err` for unimplemented colourspace
/// pairs rather than unwrapping), so this mapping is total.
fn corrupt(err: zune_jpeg::errors::DecodeErrors) -> ImageCodecError {
    ImageCodecError::Corrupt {
        codec: Codec::Dct,
        detail: err.to_string(),
    }
}

/// A corrupt-codestream error raised by pdfcer's own marker walk.
fn corrupt_detail(detail: &str) -> ImageCodecError {
    ImageCodecError::Corrupt {
        codec: Codec::Dct,
        detail: detail.to_owned(),
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

    /// A minimal codestream: SOI, one marker segment, SOF, SOS.
    ///
    /// Only the marker *chain* matters to [`sniff`]; the entropy-coded
    /// data after SOS is never reached, so these fixtures stop there.
    fn codestream(segments: &[(u8, Vec<u8>)], sof: Option<(u8, u8, u8)>) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        for (marker, payload) in segments {
            out.extend_from_slice(&[0xFF, *marker]);
            let len = u16::try_from(payload.len() + 2).unwrap();
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(payload);
        }
        if let Some((marker, precision, components)) = sof {
            let payload = vec![precision, 0, 8, 0, 8, components];
            out.extend_from_slice(&[0xFF, marker]);
            let len = u16::try_from(payload.len() + 2).unwrap();
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(&payload);
        }
        out.extend_from_slice(&[0xFF, 0xDA]);
        out
    }

    fn adobe(transform: u8) -> (u8, Vec<u8>) {
        let mut payload = b"Adobe".to_vec();
        payload.extend_from_slice(&[0x00, 0x64, 0, 0, 0, 0]); // version, flags0, flags1
        payload.push(transform);
        (0xEE, payload)
    }

    #[test]
    fn sniff_reads_component_count_and_app14() {
        let data = codestream(&[adobe(1)], Some((0xC0, 8, 3)));
        let frame = sniff(&data).unwrap();
        assert_eq!(frame.components, 3);
        assert_eq!(frame.adobe_transform, Some(1));
    }

    #[test]
    fn sniff_ignores_a_non_adobe_app14() {
        let data = codestream(&[(0xEE, b"SomeoneElse\0\0".to_vec())], Some((0xC0, 8, 3)));
        assert_eq!(sniff(&data).unwrap().adobe_transform, None);
    }

    #[test]
    fn sniff_names_unsupported_frame_types() {
        for (marker, feature) in [
            (0xC3u8, "DCT/lossless"),
            (0xC9, "DCT/arithmetic"),
            (0xCA, "DCT/arithmetic"),
            (0xC5, "DCT/differential"),
        ] {
            let data = codestream(&[], Some((marker, 8, 3)));
            assert_eq!(
                sniff(&data).unwrap_err(),
                ImageCodecError::FeatureUnsupported { feature },
                "marker {marker:#X}"
            );
        }
    }

    #[test]
    fn sniff_names_unsupported_precision() {
        let data = codestream(&[], Some((0xC0, 12, 3)));
        assert_eq!(
            sniff(&data).unwrap_err(),
            ImageCodecError::FeatureUnsupported {
                feature: "DCT/12-bit"
            }
        );
    }

    #[test]
    fn sniff_does_not_mistake_dht_or_dac_for_a_frame_header() {
        // 0xC4 (DHT) and 0xCC (DAC) sit inside the SOF marker range and
        // are NOT frame headers. A walk that treats them as such reads
        // a component count out of a Huffman table.
        let data = codestream(
            &[(0xC4, vec![0u8; 20]), (0xCC, vec![0u8; 4])],
            Some((0xC0, 8, 1)),
        );
        let frame = sniff(&data).unwrap();
        assert_eq!(frame.components, 1);
    }

    #[test]
    fn sniff_refuses_a_stream_that_is_not_a_jpeg() {
        assert!(matches!(
            sniff(b"not a jpeg at all"),
            Err(ImageCodecError::Corrupt { .. })
        ));
        assert!(matches!(sniff(&[]), Err(ImageCodecError::Corrupt { .. })));
    }

    #[test]
    fn sniff_refuses_a_segment_length_that_runs_past_the_end() {
        // Length 0xFFFF with two payload bytes present.
        let data = [0xFF, 0xD8, 0xFF, 0xE0, 0xFF, 0xFF, 0x00, 0x00];
        assert!(matches!(sniff(&data), Err(ImageCodecError::Corrupt { .. })));
    }

    #[test]
    fn table_13_default_is_component_count_dependent() {
        // The verified rule: "1 if the image has three components and 0
        // otherwise". Expressed here exactly as `decode` computes it.
        for (components, want) in [(1u8, 0u8), (3, 1), (4, 0)] {
            assert_eq!(u8::from(components == 3), want, "{components} components");
        }
    }

    #[test]
    fn routing_matches_the_decision_005_table() {
        let frame = |components, adobe_transform| Frame {
            components,
            adobe_transform,
        };
        // 1 component: transform ignored entirely.
        assert_eq!(
            route(&frame(1, Some(1)), 1).unwrap(),
            (ColorSpace::Luma, CodecColorModel::Gray)
        );
        // 3 components, transform 1 → zune applies YCbCr→RGB.
        assert_eq!(
            route(&frame(3, None), 1).unwrap(),
            (ColorSpace::RGB, CodecColorModel::Rgb)
        );
        // 3 components, transform 0 from an APP14 marker → passthrough,
        // and the request is RGB because of the deferred misc.rs:209
        // fixup.
        assert_eq!(
            route(&frame(3, Some(0)), 0).unwrap(),
            (ColorSpace::RGB, CodecColorModel::Untransformed3)
        );
        // 3 components, transform 0 from /DecodeParms with NO marker →
        // passthrough, but the request is YCbCr, because with no marker
        // that is what zune believes the input is. Asking for RGB here
        // would make zune APPLY the inverse.
        assert_eq!(
            route(&frame(3, None), 0).unwrap(),
            (ColorSpace::YCbCr, CodecColorModel::Untransformed3)
        );
        // 4 components → CMYK either way; the YCCK inverse is selected
        // separately by the transform value.
        assert_eq!(
            route(&frame(4, None), 0).unwrap(),
            (ColorSpace::CMYK, CodecColorModel::Cmyk)
        );
        assert!(!needs_ycck_inverse(0));
        assert!(needs_ycck_inverse(1));
        assert!(needs_ycck_inverse(2));
    }

    #[test]
    fn adobe_transform_above_two_is_a_named_diagnostic() {
        let f = Frame {
            components: 3,
            adobe_transform: Some(3),
        };
        assert_eq!(
            route(&f, 3).unwrap_err(),
            ImageCodecError::FeatureUnsupported {
                feature: "DCT/adobe-transform-3"
            }
        );
        let f = Frame {
            components: 3,
            adobe_transform: Some(9),
        };
        assert_eq!(
            route(&f, 9).unwrap_err(),
            ImageCodecError::FeatureUnsupported {
                feature: "DCT/adobe-transform-unknown"
            }
        );
    }

    #[test]
    fn two_component_jpeg_is_named_not_misdecoded() {
        let f = Frame {
            components: 2,
            adobe_transform: None,
        };
        assert_eq!(
            route(&f, 0).unwrap_err(),
            ImageCodecError::FeatureUnsupported {
                feature: "DCT/2-component"
            }
        );
    }

    #[test]
    fn ycck_inverse_round_trips_a_known_pixel() {
        // Forward: pure cyan CMYK (255, 0, 0, 0) → RGB'(0, 255, 255) →
        // YCbCr. Feed that YCbCr back and the cyan must return.
        let (r, g, b) = (0.0f32, 255.0f32, 255.0f32);
        let y = 0.299 * r + 0.587 * g + 0.114 * b;
        let cb = 128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b;
        let cr = 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b;
        let mut samples = [y.round() as u8, cb.round() as u8, cr.round() as u8, 42];
        ycck_to_cmyk_in_place(&mut samples);
        assert!(samples[0] > 250, "C ≈ 255, got {}", samples[0]);
        assert!(samples[1] < 5, "M ≈ 0, got {}", samples[1]);
        assert!(samples[2] < 5, "Y ≈ 0, got {}", samples[2]);
        assert_eq!(samples[3], 42, "K is carried through UNTOUCHED");
    }

    #[test]
    fn ycck_inverse_tolerates_a_ragged_tail() {
        // A truncated final pixel must not panic (the fuzz invariant).
        let mut samples = [0u8, 1, 2, 3, 4, 5];
        ycck_to_cmyk_in_place(&mut samples);
        assert_eq!(&samples[4..], &[4, 5]);
    }
}

//! # Re-encoding an imported image as `/DCTDecode` — the JPEG **writer**
//!
//! The implementation of
//! [`ImageCompression::Jpeg`](super::ImageCompression::Jpeg). Everything else in
//! [`image_import`](super) exists to *avoid* re-encoding; this
//! module is the one place the operator can ask for it explicitly, and it is
//! the first and only encoder in the whole project.
//!
//! ## Why this needed a decision before it needed code
//!
//! `ROADMAP.md` **R28** — *"Read-compat only: pdfcer writes none of these
//! codecs. No image encoder enters any pdfcer crate without a new decision
//! record."* — held this variant at a refusal for the whole of its first
//! life. The blocker was never difficulty; it was that every credible JPEG
//! encoder carries a licence consequence:
//!
//! | Candidate | Why not |
//! |---|---|
//! | `jpegli-rs`, `zenjpeg` | **AGPL.** Categorically impossible for an MIT project (`LEGAL.md` §6.1) — not a judgement call. |
//! | `mozjpeg`, `turbojpeg` | Need a C toolchain, cannot target `wasm32`, and their permissive licence covers only the *bindings* — the linked libjpeg-turbo is still IJG, so the obligation is hidden rather than escaped. Also breaks `ARCHITECTURE.md` §6's single-static-binary packaging. |
//! | `image`'s own encoder | **Cannot encode CMYK at all**, which is the one colour space a PDF-editing tool most needs to preserve. |
//! | **`jpeg-encoder` 0.7.1** | Chosen. Pure Rust, zero dependencies, `no_std`, `forbid(unsafe_code)` with `simd` off, `wasm32`-capable, full CMYK/YCCK. |
//!
//! `jpeg-encoder`'s SPDX expression is `(MIT OR Apache-2.0) AND IJG`, and the
//! `AND` is **conjunctive**: the IJG terms bind *in addition to* the
//! permissive ones. IJG imposes no source-disclosure obligation (so pdfcer's
//! own MIT licence is untouched) but does require that binary redistribution
//! carry the sentence *"this software is based in part on the work of the
//! Independent JPEG Group"*. The operator accepted that on 2026-08-08. The
//! sentence lives in `about.hbs` — the `cargo-about` template — so it is
//! re-emitted into `THIRD_PARTY_LICENSES.md` on every regeneration and
//! cannot be lost to a dependency-set change. See `crates/pdfcer-core/Cargo.toml`
//! for the full adoption note.
//!
//! ## What re-encoding costs, stated plainly
//!
//! **A JPEG re-encode is a generation loss, and re-encoding a source that
//! was already a JPEG compounds it.** The DCT is not idempotent: decoding a
//! lossy codestream yields samples that already carry ringing, blocking and
//! chroma bleed, and quantising *those* introduces a second, independent set
//! of the same artefacts on top. There is no quality setting that makes this
//! free — quality 100 is still a quantisation, merely a fine one, and it
//! produces a *larger* file than the source it replaced while looking
//! slightly worse.
//!
//! That is why the two acts are disclosed **separately**:
//!
//! | Source | Disclosure | What actually happened |
//! |---|---|---|
//! | PNG / BMP (lossless) | [`ImportNotes::recompressed`](super::ImportNotes::recompressed) = [`RecompressReason::JpegRequested`](super::RecompressReason::JpegRequested) | One lossy encode of exact pixels. The loss is real but bounded and predictable. |
//! | JPEG (already lossy) | the above **plus** [`ImportNotes::jpeg_from_lossy`](super::ImportNotes::jpeg_from_lossy) | A *second* lossy encode over artefacts. Compounding, and invisible at editing zoom. |
//!
//! They are not the same act and the project's rule 4 posture ("fuzzy, never
//! sneaky") does not let them share one sentence. The size change is
//! reported unconditionally, for every policy, as
//! [`ImportNotes::source_bytes`](super::ImportNotes::source_bytes) / [`ImportNotes::stored_bytes`](super::ImportNotes::stored_bytes) — the
//! operator asked for a smaller file, so the answer to "did it get smaller?"
//! must not require diffing the output.
//!
//! ## THE CMYK POLARITY TRAP — read this before touching the 4-component path
//!
//! `docs/decisions/006-cmyk-jpeg-inversion.md` exists because getting this
//! wrong produces a **photographic negative that looks deliberate**: it
//! renders, it does not warn, and a reviewer who has not seen the original
//! cannot tell. R29 is the standing rule that came out of it: *"pdfcer never
//! applies an 'Adobe CMYK inversion'. `/Decode` is the sole polarity
//! control."* Four independent production engines (pdf.js, pdfium, MuPDF,
//! Poppler) implement exactly that, and marker-gated inversion has been
//! shipped and reverted twice upstream (cairo issue 156, Firefox bug 674619).
//!
//! R29 is a rule about the **read** path. This module is the write path, and
//! it inherits the trap from the other side. The decisive facts, read out of
//! the pinned `jpeg-encoder` 0.7.1 source rather than inferred from its docs
//! (a hand-off note claiming the opposite is what prompted the audit):
//!
//! - `ColorType::Cmyk` (`image_buffer.rs:236-256`) writes an Adobe APP14 with
//!   **transform byte 0** and stores `255 - input` on all four channels.
//! - `ColorType::CmykAsYcck` (`image_buffer.rs:259-286`) writes APP14 with
//!   **transform byte 2** and stores
//!   `cmyk_to_ycck(p) = (rgb_to_ycbcr(p₀, p₁, p₂), 255 − p₃)`.
//!
//! Run both through a never-inverting reader and **both decode to
//! `255 − input`.** (`jpeg-encoder`'s own round-trip tests do not reveal
//! this: they check against the `jpeg-decoder` crate, which — like Pillow,
//! and exactly as **R31** warns — applies its own unconditional inversion to
//! four-component JPEGs. Verifying a reference decoder's conventions before
//! trusting it is the rule that nearly got skipped here twice.)
//!
//! So pdfcer feeds the encoder the **complement of its true-ink CMYK
//! samples**, through `CmykAsYcck`. Substituting `p = 255 − ink`:
//!
//! ```text
//! stored luma/chroma = rgb_to_ycbcr(255 − C, 255 − M, 255 − Y)
//! stored K           = 255 − (255 − K) = K
//! ```
//!
//! which is **Adobe TN #5116 §13.1's forward CMYK→YCCK transform, exactly**:
//! RGB→YCC applied to `R = 255 − C`, `G = 255 − M`, `B = 255 − Y`, with `K`
//! passed through. The complement is not a polarity flip pdfcer invented — it
//! is §13.1's own `255 −`, which `jpeg-encoder` parameterises onto the `K`
//! input instead of the C/M/Y inputs. Decoding it with pdfcer's
//! `ycck_to_cmyk_in_place` (a faithful reimplementation of libjpeg's
//! `ycck_cmyk_convert`) recovers **true ink**.
//!
//! Three properties fall out, and all three are why this option was chosen
//! over the alternatives:
//!
//! 1. **No `/Decode` array is needed or written.** R29 stays intact: pdfcer
//!    applies no inversion on read, and writes nothing that requires one.
//! 2. **The output is the transform-2 (YCCK) shape** — decision 006 §4.4's
//!    *benign census*, the shape all 9 four-component files in the
//!    conformance corpus have, and the one pdfcer's render was verified
//!    pixel-perfect against pdfium on.
//! 3. **It is NOT R30's shape.** The alternative — `ColorType::Cmyk` fed the
//!    complement, giving true-ink bytes under transform 0 with no `/Decode` —
//!    would have been byte-correct *and* would have made pdfcer emit the exact
//!    one shape it warns operators about
//!    ([`ImportNotes::cmyk_polarity_unverifiable`](super::ImportNotes::cmyk_polarity_unverifiable), R30). A writer that
//!    manufactures its own diagnostic's trigger condition is a writer that
//!    has misread its own rules.
//!
//! `crates/pdfcer-core/tests/image_placement.rs` asserts this in **sample
//! values** — decode the written codestream and check the ink at named
//! pixels — not merely in the presence or absence of a `/Decode` array,
//! because a polarity bug is invisible to every structural assertion.
//!
//! ## What is refused, and why each refusal is by name
//!
//! | Refusal | Why |
//! |---|---|
//! | quality outside 1–100 ([`ImageImportError::InvalidQuality`](super::ImageImportError::InvalidQuality)) | A clamp would be pdfcer choosing an encoder setting the operator did not choose, and then storing the result permanently. Rule 4 wants inferences *disclosed*; the cheaper honest answer for a plainly out-of-range number is to say so and change nothing. |
//! | a colour-key `/Mask` ([`ImageImportError::CompressionRefused`](super::ImageImportError::CompressionRefused)) | §8.9.6.4 masks by **exact sample ranges**, and DCT quantisation moves sample values. The stated range would then miss pixels it should hide and catch pixels it should not — speckled holes in the picture and speckled opacity in the background, with no diagnostic. Not repairable after the fact, so it is refused before. |
//! | a colour model with no device colour space | Same posture as `image_import`'s own `to_lossless`: a wrong `/ColorSpace` renders as the wrong colours with no error. |
//!
//! An **`/SMask` is not refused** — it is kept, untouched, as its own
//! `/FlateDecode` `DeviceGray` image. §8.9.5 Table 89 makes the soft mask a
//! *separate* image XObject, so nothing requires it to share the base
//! image's filter, and re-encoding an alpha channel lossily would produce
//! exactly the halo artefacts JPEG is worst at. The base image goes lossy;
//! the opacity stays exact.
//!
//! ## Spec sources
//!
//! - `filter__dct.md` — §7.4.8, Table 13 (`ColorTransform` precedence), the
//!   APP14 layout, R29/R30
//! - `iso32000__s__8.9.md` — §8.9.3 sample layout, §8.9.5 Table 89
//!   (`/BitsPerComponent` **shall be 8** for `/DCTDecode`)
//! - `color__indexed.md` — §8.6.6.3 `[/Indexed base hival lookup]`, and the
//!   out-of-range rule this module relies on when expanding a palette
//! - Adobe TN #5116 §13.1 (the CMYK→YCCK forward transform), normative by
//!   reference from ISO 32000-1 §7.4.8 footnote *a*

use jpeg_encoder::{ColorType, Encoder, SamplingFactor};

use super::{
    ImageImportError, ImportColorSpace, ImportFilter, ImportNotes, ImportedImage, PdfFeature,
    RecompressReason, row_bytes,
};

/// The lowest quality [`ImageCompression::Jpeg`](super::ImageCompression::Jpeg)
/// accepts.
///
/// 1 rather than 0 because libjpeg's own quality scale — which
/// `jpeg-encoder`'s `QuantizationTable::new_with_quality` reproduces — is
/// defined on 1..=100, and 0 is not "maximum compression" there but an
/// out-of-domain value that divides by zero in the scale-factor formula.
pub(crate) const MIN_QUALITY: u8 = 1;

/// The highest quality accepted.
///
/// 100 is *not* lossless. It is the finest quantisation the scale defines,
/// and it still discards information; a source stored at quality 100 is
/// typically **larger** than the lossless `/FlateDecode` of the same pixels
/// for synthetic content. Said here so the constant is not mistaken for an
/// escape hatch.
pub(crate) const MAX_QUALITY: u8 = 100;

/// Re-encode an already-imported image as a baseline `/DCTDecode` stream.
///
/// # Contract
///
/// Takes the [`ImportedImage`] one of the three format importers produced —
/// so the source's own decisions about colour space, palette and bit depth
/// are already resolved — and returns a *new* one whose
/// [`data`](ImportedImage::data) is a JPEG codestream and whose
/// [`filter`](ImportedImage::filter) is [`ImportFilter::DctDecode`].
///
/// Preserved unchanged: [`soft_mask`](ImportedImage::soft_mask) (see the
/// module docs), [`orientation`](ImportedImage::orientation) (a property of
/// the picture, not of its storage) and [`dpi`](ImportedImage::dpi).
///
/// Cleared, because they described the *source* and are no longer true of
/// what is stored: `color_key_mask` (refused outright, so always `None`
/// here), `progressive_jpeg` (this writer emits baseline only) and
/// `cmyk_polarity_unverifiable` (the output is transform 2, which is never
/// the ambiguous shape — see the module docs).
///
/// # Errors
///
/// - [`ImageImportError::CompressionRefused`](super::ImageImportError::CompressionRefused) — the image carries a
///   colour-key `/Mask`, which lossy encoding would corrupt.
/// - [`ImageImportError::DecodeFailed`] — the source's own samples could not
///   be recovered, or its colour model has no JPEG component layout.
/// - [`ImageImportError::TooLarge`] — the samples do not fit the encoder's
///   `u16` dimension API. Unreachable in practice:
///   [`MAX_IMAGE_DIMENSION`](crate::image_codec::MAX_IMAGE_DIMENSION) is
///   65 535, which is `u16::MAX`, so [`check_dimensions`](super::check_dimensions)
///   has already rejected anything larger. Checked anyway rather than cast,
///   because a silent truncation here would produce a plausible-looking
///   image of the wrong size.
/// - [`ImageImportError::Compress`] — the encoder itself failed.
pub(crate) fn to_jpeg(img: &ImportedImage, quality: u8) -> Result<ImportedImage, ImageImportError> {
    // §8.9.6.4 masks by exact sample values; DCT moves them. Refused BEFORE
    // any work, so the operator learns the real blocker rather than watching
    // a decode succeed and then being told no.
    if img.color_key_mask.is_some() {
        return Err(ImageImportError::CompressionRefused {
            policy: "jpeg",
            reason: "this image's transparency is a single fully-transparent COLOUR, matched \
                     against exact sample values (ISO 32000-1 §8.9.6.4). JPEG is lossy, so \
                     re-encoding would shift those values: some transparent pixels would turn \
                     opaque and some opaque pixels would turn transparent, in a speckle no \
                     diagnostic can catch afterwards. Use `passthrough` or `lossless`",
        });
    }

    let source = source_samples(img)?;

    // The u16 API is a real edge, not a formality — see the Errors section.
    let (Ok(w16), Ok(h16)) = (u16::try_from(source.width), u16::try_from(source.height)) else {
        return Err(ImageImportError::TooLarge);
    };

    let expected = (source.width as usize)
        .checked_mul(source.height as usize)
        .and_then(|px| px.checked_mul(usize::from(source.components)));
    if expected != Some(source.samples.len()) {
        // The decoder and the geometry disagree. Refused rather than
        // encoded from a truncated or over-long buffer, which would either
        // panic inside the encoder or shear the image.
        return Err(ImageImportError::DecodeFailed {
            detail: format!(
                "the decoded sample buffer is {} bytes but {}×{}×{} components needs {}",
                source.samples.len(),
                source.width,
                source.height,
                source.components,
                expected.map_or_else(|| "more than fits in memory".to_owned(), |n| n.to_string()),
            ),
        });
    }

    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, quality);

    // Chroma subsampling, chosen per component count rather than left to the
    // library default, because the default is quality-conditioned
    // (`SamplingFactor::F_2_2` below quality 90, `F_1_1` at or above) and
    // that coupling is invisible to the operator.
    //
    // For 1 and 3 components the default IS the right answer and is adopted
    // deliberately: 4:2:0 on YCbCr is what every photographic encoder does,
    // and it is what "quality 75" means to anyone who has used one.
    //
    // For 4 components it is NOT. `jpeg-encoder`'s YCCK component table
    // (`encoder.rs:631-648`) gives the sampling factor to Y **and K** while
    // pinning Cb/Cr at 1×1 — so a subsampled configuration halves the C/M/Y
    // chroma resolution of ink channels that, on a CAD export or a print
    // proof, carry hard edges rather than smooth gradients. Forced to 1:1.
    if source.components == 4 {
        encoder.set_sampling_factor(SamplingFactor::F_1_1);
    }

    // THE POLARITY DECISION. See the module docs for the derivation; the
    // one-line version is that `CmykAsYcck` fed the COMPLEMENT of true ink
    // reproduces TN #5116 §13.1 exactly and lands on the benign transform-2
    // shape, while every other combination either inverts the picture or
    // manufactures R30's ambiguous shape.
    let (payload, color_type) = match source.components {
        1 => (source.samples, ColorType::Luma),
        3 => (source.samples, ColorType::Rgb),
        4 => (
            source.samples.iter().map(|&v| 255 - v).collect(),
            ColorType::CmykAsYcck,
        ),
        // `source_samples` only ever produces 1, 3 or 4; a fourth value
        // would be a bug in this module, not in the file.
        n => {
            return Err(ImageImportError::DecodeFailed {
                detail: format!("{n} components have no /DCTDecode layout"),
            });
        }
    };

    encoder
        .encode(&payload, w16, h16, color_type)
        .map_err(|e| ImageImportError::Compress(e.to_string()))?;

    // Recomputed from what is actually stored rather than inherited: the
    // source's floor may have been `/BitsPerComponent 16` (PDF 1.5) or a
    // colour-key `/Mask` (1.3), and neither survives into this output.
    // Baseline `/DCTDecode` itself is PDF 1.0, so only the soft mask can
    // raise the floor at all.
    let mut requires_pdf_version = None;
    if let Some(mask) = &img.soft_mask {
        super::raise_version(&mut requires_pdf_version, PdfFeature::SoftMask);
        if mask.bits_per_component == 16 {
            super::raise_version(&mut requires_pdf_version, PdfFeature::BitsPerComponent16);
        }
    }

    Ok(ImportedImage {
        format: img.format,
        width: source.width,
        height: source.height,
        // Table 89: `/DCTDecode` "shall always deliver 8-bit samples", and
        // §7.4.8 says each component value occupies a byte. There is no
        // other legal value here.
        bits_per_component: 8,
        color_space: source.space,
        filter: ImportFilter::DctDecode,
        data: out,
        soft_mask: img.soft_mask.clone(),
        color_key_mask: None,
        orientation: img.orientation,
        dpi: img.dpi,
        notes: ImportNotes {
            recompressed: Some(RecompressReason::JpegRequested),
            // The distinction rule 4 insists on: a second lossy pass over an
            // already-lossy source is a different act from one lossy pass
            // over exact pixels, and compounds rather than adds.
            jpeg_from_lossy: img.filter == ImportFilter::DctDecode,
            jpeg_quality: Some(quality),
            // Baseline output — whatever the source was.
            progressive_jpeg: false,
            // Transform 2 is never R30's shape; see the module docs.
            cmyk_polarity_unverifiable: false,
            requires_pdf_version,
            // `source_bytes`/`stored_bytes` are deliberately NOT set here:
            // `import_with` sets them last, for every policy, so no branch in
            // this module can forget one and no two branches can disagree.
            //
            // A re-encode cannot restore a profile the importer dropped, and
            // it does not drop one that survived — this field is a property
            // of the SOURCE and travels unchanged.
            ..img.notes
        },
    })
}

/// 8-bit interleaved samples plus the `/ColorSpace` they land in.
///
/// A struct rather than a tuple because the three fields must agree — a
/// component count that disagrees with the colour space is precisely the bug
/// that renders as plausible-looking wrong colours.
struct SourceSamples {
    /// Interleaved, row-major, row 0 at the top, exactly
    /// `width × height × components` bytes with **no row padding** (the
    /// encoder's own buffer contract; §8.9.3's padding has been undone).
    samples: Vec<u8>,
    width: u32,
    height: u32,
    /// 1, 3 or 4 — the only component counts `/DCTDecode` expresses.
    components: u8,
    /// The `/ColorSpace` the re-encoded image will declare, which is what
    /// the components *mean*.
    space: ImportColorSpace,
}

/// Recover the source's pixels as 8-bit interleaved samples.
///
/// # Why this goes back through the ordinary decode path
///
/// Same argument as [`to_lossless`](super::to_lossless): the samples must be
/// exactly the ones `pdfcer-render` would paint, which means Table 13's
/// colour-transform precedence chain, the YCCK inverse and R29's never-invert
/// rule all have to apply identically. A second opinion about what a JPEG's
/// pixels are is a second opinion that will drift.
///
/// The non-DCT branch reuses the same entry point for a less obvious reason:
/// an [`ImportedImage`]'s `data` **is** a conforming PDF image stream by
/// construction, so a synthesised dictionary describing it feeds the real
/// filter chain — including §7.4.4.4's PNG predictor, which the verbatim
/// `IDAT` passthrough depends on. Re-implementing an un-predictor here would
/// mean two of them.
fn source_samples(img: &ImportedImage) -> Result<SourceSamples, ImageImportError> {
    use crate::image_codec::{CodecColorModel, decode_image_view};
    use crate::object::{Dict, Name, ObjId, Object};
    use crate::view::DocumentView;

    /// An object graph with nothing in it — a standalone image file has no
    /// PDF objects, so every lookup honestly answers "no such object", which
    /// is §7.3.10's outcome for an unresolvable reference.
    struct NoGraph;
    impl crate::graph::ObjectGraph for NoGraph {
        fn value(&self, _id: ObjId) -> Option<&Object> {
            None
        }
        fn trailer_entry(&self, _key: &[u8]) -> Option<&Object> {
            None
        }
    }

    let n = usize::from(img.color_space.components());
    let bpc = u32::from(img.bits_per_component);

    let mut dict = Dict::new();
    dict.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(match img.filter {
            ImportFilter::DctDecode => b"DCTDecode".as_slice(),
            ImportFilter::Flate | ImportFilter::FlatePngPredictor { .. } => b"FlateDecode",
        })),
    );
    if let ImportFilter::FlatePngPredictor {
        colors,
        bits_per_component,
        columns,
    } = img.filter
    {
        let mut parms = Dict::new();
        parms.insert(Name::from(b"Predictor"), Object::Integer(15));
        parms.insert(Name::from(b"Colors"), Object::Integer(i64::from(colors)));
        parms.insert(
            Name::from(b"BitsPerComponent"),
            Object::Integer(i64::from(bits_per_component)),
        );
        parms.insert(Name::from(b"Columns"), Object::Integer(i64::from(columns)));
        dict.insert(Name::from(b"DecodeParms"), Object::Dict(parms));
    }

    let graph = NoGraph;
    let view = DocumentView::new(&graph, &[], crate::PdfVersion { major: 1, minor: 7 });
    let coded = decode_image_view(&view, &dict, &img.data, false).map_err(|e| {
        ImageImportError::DecodeFailed {
            detail: e.to_string(),
        }
    })?;

    // --- The DCT branch: the codec already delivered 8-bit samples --------
    if img.filter == ImportFilter::DctDecode {
        let (components, space) = match coded.color_model {
            CodecColorModel::Gray | CodecColorModel::Bilevel => (1, ImportColorSpace::DeviceGray),
            // `Untransformed3` is mapped to `/DeviceRGB` for the reason
            // §7.4.8 gives: transform 0 means "the codestream's components
            // already ARE the /ColorSpace components", which for a
            // three-component JPEG pdfcer is choosing the space for is the
            // same three components passthrough would have declared.
            CodecColorModel::Rgb | CodecColorModel::Untransformed3 => {
                (3, ImportColorSpace::DeviceRgb)
            }
            CodecColorModel::Cmyk => (4, ImportColorSpace::DeviceCmyk),
            // REFUSED rather than guessed at: a wrong `/ColorSpace` renders
            // as the wrong colours with no error at all.
            _ => {
                return Err(ImageImportError::DecodeFailed {
                    detail: "the codestream's colour model has no device colour space".to_owned(),
                });
            }
        };
        return Ok(SourceSamples {
            samples: coded.samples,
            // The CODESTREAM's geometry, not the marker walk's. They are the
            // same source and should agree, but the decoder is the authority
            // on what it just produced, and sizing a buffer from a second
            // opinion is how a stride bug is born.
            width: coded.width,
            height: coded.height,
            components,
            space,
        });
    }

    // --- The lossless branch: §8.9.3-packed rows, still at the source's
    //     bit depth, still indexed if the source had a palette -------------
    let packed = coded.samples;
    let stride = row_bytes(img.width, n as u32, bpc);
    let rows = img.height as usize;
    if packed.len() < stride.saturating_mul(rows) {
        return Err(ImageImportError::DecodeFailed {
            detail: format!(
                "the decompressed image is {} bytes but {rows} rows of {stride} bytes need {}",
                packed.len(),
                stride.saturating_mul(rows),
            ),
        });
    }

    // One 8-bit value per component per pixel, row padding removed.
    let flat = unpack_to_bytes(
        &packed,
        img.width,
        img.height,
        n,
        img.bits_per_component,
        stride,
    );

    match &img.color_space {
        // §8.6.6.3: the stored value is an INDEX, not a colour. It is looked
        // up, never scaled — scaling an index is the classic way to turn a
        // 4-bit palette image into noise. JPEG has no palette, so the only
        // faithful encoding is the expanded RGB.
        ImportColorSpace::Indexed { hival, lookup } => {
            let mut rgb = Vec::with_capacity(flat.len().saturating_mul(3));
            for &idx in &flat {
                // §8.6.6.3 clamps an out-of-range index to `hival` rather
                // than failing; a palette image with a stray index is a
                // producer bug pdfcer reads the same way every viewer does.
                let i = usize::from(idx.min(*hival));
                let base = i.saturating_mul(3);
                rgb.extend_from_slice(lookup.get(base..base + 3).unwrap_or(&[0, 0, 0]));
            }
            Ok(SourceSamples {
                samples: rgb,
                width: img.width,
                height: img.height,
                components: 3,
                space: ImportColorSpace::DeviceRgb,
            })
        }
        space => Ok(SourceSamples {
            samples: flat,
            width: img.width,
            height: img.height,
            // `components()` is 1/3/4 for the three device spaces, which is
            // exactly the set `/DCTDecode` expresses.
            components: img.color_space.components(),
            space: space.clone(),
        }),
    }
}

/// Unpack §8.9.3-packed samples into one byte per component, dropping row
/// padding.
///
/// # The three cases, and why each scales the way it does
///
/// - **8 bits** — already one byte per component; only the row padding
///   (which is zero for 8-bit data anyway) is stepped over.
/// - **16 bits** — the high byte is taken. This is a real loss of precision
///   and it is unavoidable: §7.4.8 and Table 89 both fix `/DCTDecode` at
///   8-bit samples, so there is no 16-bit JPEG to write. Taking the high
///   byte rather than rounding is deliberate — `(v >> 8)` is exactly
///   `floor(v × 255 / 65535)` to within one unit and needs no arithmetic
///   that could overflow, and the difference is far below the quantisation
///   the encoder is about to apply anyway.
/// - **1, 2 or 4 bits** — samples are packed *"from high-order to low-order
///   bits"* (§7.4.4.4, §8.9.3). A colour value is scaled onto 0..=255 by
///   `v × 255 / (2ⁿ − 1)`, which maps the extremes exactly (0→0, max→255) —
///   the naive `v << (8 − n)` would map 4-bit 15 to 240 and quietly darken
///   every white pixel. **Indexed images are handled by the caller**, which
///   must NOT scale: there the value is a palette index.
///
/// Everything past the last full row, and every bit past the last sample in
/// a row, is padding the caller never sees.
fn unpack_to_bytes(
    packed: &[u8],
    width: u32,
    height: u32,
    components: usize,
    bits: u8,
    stride: usize,
) -> Vec<u8> {
    let per_row = (width as usize).saturating_mul(components);
    let mut out = Vec::with_capacity(per_row.saturating_mul(height as usize));

    for y in 0..height as usize {
        let row = packed
            .get(y.saturating_mul(stride)..)
            .and_then(|r| r.get(..stride))
            .unwrap_or(&[]);
        match bits {
            8 => out.extend_from_slice(row.get(..per_row).unwrap_or(row)),
            16 => {
                for i in 0..per_row {
                    out.push(row.get(i.saturating_mul(2)).copied().unwrap_or(0));
                }
            }
            b @ (1 | 2 | 4) => {
                let n = u32::from(b);
                let max = (1u32 << n) - 1;
                let per_byte = 8 / n as usize;
                let mask = max as u8;
                for i in 0..per_row {
                    let byte = row.get(i / per_byte).copied().unwrap_or(0);
                    // High-order first: sample 0 of a byte lives in its top
                    // `n` bits.
                    let shift = 8 - n as usize - (i % per_byte) * n as usize;
                    let v = u32::from((byte >> shift) & mask);
                    // Exact endpoint mapping; see the doc comment.
                    out.push(((v * 255) / max) as u8);
                }
            }
            // Table 89 admits only 1, 2, 4, 8 and 16. Anything else has
            // already been refused by the format importers, so this arm is
            // unreachable; emitting the row verbatim keeps the function
            // total without inventing a scaling rule for a depth that
            // cannot occur.
            _ => out.extend_from_slice(row.get(..per_row).unwrap_or(row)),
        }
    }
    out
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

    /// The endpoint-exactness property the doc comment claims: a 4-bit
    /// sample of 15 must become 255, not 240. Getting this wrong darkens
    /// every white pixel in a low-depth image by 6 %, which is visible and
    /// which no structural assertion would catch.
    #[test]
    fn sub_byte_samples_scale_onto_the_full_range() {
        // Two 4-bit greyscale pixels per byte: 0x0F then 0xF0 -> 0, 255,
        // 255, 0.
        let packed = [0x0F, 0xF0];
        let out = unpack_to_bytes(&packed, 2, 2, 1, 4, 1);
        assert_eq!(out, vec![0, 255, 255, 0]);

        // 1-bit: 0b1010_0000 with width 4 -> 255, 0, 255, 0.
        let out = unpack_to_bytes(&[0b1010_0000], 4, 1, 1, 1, 1);
        assert_eq!(out, vec![255, 0, 255, 0]);

        // 2-bit: 0b11_10_01_00 -> 255, 170, 85, 0.
        let out = unpack_to_bytes(&[0b1110_0100], 4, 1, 1, 2, 1);
        assert_eq!(out, vec![255, 170, 85, 0]);
    }

    /// Row padding is stepped over, not emitted. A 3-pixel 4-bit row
    /// occupies 2 bytes and the low nibble of the second is padding
    /// (§8.9.3: *"each row … padded to a whole number of bytes"*).
    #[test]
    fn row_padding_never_reaches_the_output() {
        // Row 0: 0x12, 0x3F (the F is padding). Row 1: 0x45, 0x6F.
        let packed = [0x12, 0x3F, 0x45, 0x6F];
        let out = unpack_to_bytes(&packed, 3, 2, 1, 4, 2);
        let scale = |v: u32| ((v * 255) / 15) as u8;
        assert_eq!(
            out,
            vec![scale(1), scale(2), scale(3), scale(4), scale(5), scale(6)]
        );
    }

    /// 16-bit samples keep their high byte, and the low byte is dropped
    /// rather than mixed in.
    #[test]
    fn sixteen_bit_samples_keep_the_high_byte() {
        // Two RGB pixels, big-endian 16-bit per component (§8.9.3).
        let packed = [
            0xAB, 0xCD, 0x00, 0x11, 0xFF, 0xFF, // pixel 0
            0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, // pixel 1
        ];
        let out = unpack_to_bytes(&packed, 2, 1, 3, 16, 12);
        assert_eq!(out, vec![0xAB, 0x00, 0xFF, 0x12, 0x56, 0x9A]);
    }
}

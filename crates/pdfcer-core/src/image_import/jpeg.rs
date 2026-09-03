//! # JPEG import — the verbatim `/DCTDecode` passthrough
//!
//! Turns a JPEG file into an image XObject **without decoding a single
//! coefficient**. The codestream is copied byte for byte into a
//! `/DCTDecode` stream; this module's entire job is to read enough of the
//! marker chain to fill in `/Width`, `/Height`, `/ColorSpace` and
//! `/BitsPerComponent`, and to refuse — by name — the frame types
//! `/DCTDecode` does not cover.
//!
//! ## Why verbatim is not merely an optimisation here
//!
//! JPEG is lossy. Decoding and re-encoding a scan, a photograph or a CAD
//! export loses quality *every time the image is placed*, and there is no
//! quality setting that makes it free — a "95%" re-encode of an already-95%
//! image is visibly worse than the original and larger. For the documents
//! this project exists to handle (drawings, scans, photographs dropped onto
//! a page) that difference is the whole feature.
//!
//! It is also the only permitted path. `ROADMAP.md` **R28** — *"Read-compat
//! only: pdfcer writes none of these codecs. No image encoder enters any
//! pdfcer crate without a new decision record"* — means a JPEG *encoder*
//! cannot be added by a feature Pass at all. Passthrough introduces none:
//! the bytes were encoded by whatever produced the file, and pdfcer only
//! writes a dictionary describing them.
//!
//! ## The dictionary is derived, never guessed
//!
//! ISO 32000-1 §7.4.8: the JPEG parameters *"are entirely under the control
//! of the encoder and shall be stored in the encoded data"* — with exactly
//! one exception, `ColorTransform`, and even that one defers to the
//! codestream when the Adobe APP14 marker is present. So every value this
//! module writes comes from the SOF marker it just read:
//!
//! | PDF entry | Source |
//! |---|---|
//! | `/Width`, `/Height` | SOF frame header (T.81 §B.2.2) |
//! | `/BitsPerComponent` | always **8** — Table 89 couples `/DCTDecode` to 8-bit samples, and §7.4.8 says *"Each component value shall occupy a byte"* |
//! | `/ColorSpace` | component count: 1 → `/DeviceGray`, 3 → `/DeviceRGB`, 4 → `/DeviceCMYK` |
//! | `/Filter` | `/DCTDecode` |
//! | `/DecodeParms` | **absent** — see below |
//! | `/Decode` | **absent** — see below |
//!
//! ### Why no `/DecodeParms /ColorTransform`
//!
//! Table 13: *"If the encoding algorithm has inserted the Adobe-defined
//! marker code in the encoded data indicating the `ColorTransform` value,
//! then the colours shall be transformed … according to the value provided
//! in the encoded data and **the value of this dictionary entry shall be
//! ignored**."* Since the codestream travels verbatim, its APP14 marker
//! travels with it, so on a marked JPEG the entry would be inert. On an
//! unmarked one the default rule already applies — *"`ColorTransform` shall
//! be 1 if the image has three components and 0 otherwise"* — which is
//! exactly what the encoder assumed when it wrote the file. Writing the
//! entry could only ever agree redundantly or disagree wrongly.
//!
//! ### Why no `/Decode`, especially on CMYK
//!
//! **R29**: *"pdfcer never applies an 'Adobe CMYK inversion.' … `/Decode` is
//! the sole polarity control for every image, in every colour space, at
//! every bit depth."* Decision 006 established this from four independent
//! production engines (pdf.js, pdfium, MuPDF's PDF path, Poppler — none
//! inverts), from the negative result that the word "invert" appears zero
//! times in Adobe TN #5116, and from the revert trail of the two projects
//! that tried marker-gated inversion and backed it out. Emitting a
//! `/Decode [1 0 1 0 1 0 1 0]` on import would invert nine of nine
//! known-good corpus files.
//!
//! **R30**'s residual shape is *reported*: four components, effective
//! `ColorTransform` 0 (transform byte 0, or no Adobe marker), no `/Decode`.
//! There, nothing in the file declares polarity — the undocumented
//! pre-1994 Photoshop convention could apply and no bit says so — and
//! [`ImportNotes::cmyk_polarity_unverifiable`](super::ImportNotes::cmyk_polarity_unverifiable)
//! says exactly that. It is never repaired: a silent polarity flip is
//! high-impact, invisible when wrong, and undetectable by an operator who
//! does not have the original.
//!
//! ## Progressive JPEG: accepted, and disclosed
//!
//! §7.4.8, verbatim: *"beginning with PDF 1.3, the `DCTDecode` filter shall
//! support the progressive JPEG extension. This extension does not add any
//! entries to the `DCTDecode` parameter dictionary; the distinction between
//! baseline and progressive JPEG shall be represented in the encoded
//! data."* So a progressive JPEG is legal and needs no dictionary change —
//! but NOTE 5 is equally explicit that *"there is no benefit to using
//! progressive JPEG for stream data that is embedded in a PDF file.
//! Decoding progressive JPEG is slower and consumes more memory."*
//!
//! pdfcer therefore **embeds it as-is and says so**. The alternative — a
//! baseline transcode — would be both a generation loss and an encoder R28
//! forbids, to buy a decode-speed property the operator did not ask for.
//! Telling them, so they can re-save as baseline if the document will be
//! opened often, is the honest version of the same advice.
//!
//! ## EXIF orientation: applied to the placement, not to the pixels
//!
//! A phone or camera JPEG is very often stored sideways with an EXIF
//! `Orientation` tag saying how to turn it. Ignoring the tag places a
//! rotated picture — which reads as a pdfcer bug, not as a property of the
//! file. Rotating the pixels means a decode and re-encode, which is the
//! generation loss and the forbidden encoder again.
//!
//! So pdfcer reads the tag and folds the rotation into the `cm` matrix
//! §8.9.4 already requires. All eight EXIF orientations are isometries of
//! the unit square, so this is **exact** — see
//! [`Orientation::unit_square_matrix`](super::Orientation::unit_square_matrix).
//! The bytes stay verbatim, the picture comes out the right way up, and the
//! operator is told it happened.
//!
//! ## What is refused, by name
//!
//! | Frame / property | Feature key | Why |
//! |---|---|---|
//! | SOF3/7/11/15 | `JPEG/lossless` | A different codec in JPEG's marker syntax |
//! | SOF9/10/13/14 | `JPEG/arithmetic` | Arithmetic entropy coding |
//! | SOF5/6 | `JPEG/differential` | Hierarchical/differential |
//! | 12- or 16-bit precision | `JPEG/12-bit`, `JPEG/16-bit` | Table 89 couples `/DCTDecode` to 8-bit samples |
//! | 2 components | `JPEG/2-component` | §7.4.8 permits two components, but §8.6 has no two-component device space to name in `/ColorSpace` |
//! | APP14 transform ≥ 3 | `JPEG/adobe-transform-N` | Outside Table 13's 0–2; pdfcer's own decoder rejects it, so placing one would make a document pdfcer cannot display |
//!
//! The keys deliberately mirror `crate::image_codec::dct`'s R27 diagnostic
//! names, so a refusal on import and a refusal on render read the same.
//!
//! ## Spec sources
//!
//! - ISO 32000-1 §7.4.8 + Table 13 + footnote *a* (`filter__dct.md`)
//! - ISO 32000-1 §8.9.5 Table 89 (`iso32000__s__8.9.md`)
//! - ITU-T T.81 §B.1 (marker syntax), §B.2.2 (frame header)
//! - Adobe TN #5116 §18 (the APP14 layout) — reported, never quoted
//! - `docs/decisions/006-cmyk-jpeg-inversion.md` (R29, R30)

use super::{
    DpiSource, ImageFormat, ImageImportError, ImportColorSpace, ImportFilter, ImportNotes,
    ImportedImage, Orientation, PdfFeature, check_dimensions, raise_version,
};

/// What the marker walk learned. Everything the PDF dictionary needs comes
/// from here; nothing is inferred from the file name or guessed.
#[derive(Debug, Default, Clone, Copy)]
struct Frame {
    width: u32,
    height: u32,
    /// Sample precision in bits, from the SOF header.
    precision: u8,
    /// Components per sample, from the SOF header.
    components: u8,
    /// SOF2 — the PDF 1.3 progressive extension.
    progressive: bool,
    /// The Adobe APP14 transform byte, when an `Adobe`-identified APP14 is
    /// present.
    adobe_transform: Option<u8>,
    /// An ICC profile was found in APP2.
    icc: bool,
    /// EXIF `Orientation` (IFD0 tag 0x0112).
    orientation: Orientation,
    /// A declared resolution, and where it came from.
    dpi: Option<(f64, f64)>,
    dpi_source: DpiSource,
}

/// Parse a JPEG into a PDF-ready image XObject payload.
///
/// # Errors
///
/// See [`ImageImportError`]. Every unsupported frame type or precision is
/// [`ImageImportError::Unsupported`] with a stable key, never a generic
/// failure.
pub fn import(data: &[u8]) -> Result<ImportedImage, ImageImportError> {
    let frame = walk(data)?;

    // §8.6.4 has device colour spaces at 1, 3 and 4 components. §7.4.8
    // permits a two-component JPEG, but there is no two-component space to
    // put in `/ColorSpace`, so it is refused rather than mapped onto
    // something it is not.
    let color_space = match frame.components {
        1 => ImportColorSpace::DeviceGray,
        3 => ImportColorSpace::DeviceRgb,
        4 => ImportColorSpace::DeviceCmyk,
        2 => {
            return Err(ImageImportError::Unsupported {
                feature: "JPEG/2-component",
            });
        }
        _ => {
            return Err(ImageImportError::Corrupt {
                detail: format!(
                    "the JPEG declares {} colour components; 1, 3 or 4 are placeable",
                    frame.components
                ),
            });
        }
    };

    check_dimensions(
        frame.width,
        frame.height,
        u32::from(frame.components),
        u32::from(frame.precision),
    )?;

    // Table 13's precedence chain, for the R30 diagnostic ONLY — this value
    // is never used to transform anything, because nothing is decoded here.
    // Marker outranks `/DecodeParms` (which pdfcer does not write) outranks
    // the component-count default.
    let effective_transform =
        frame
            .adobe_transform
            .unwrap_or(if frame.components == 3 { 1 } else { 0 });

    let mut notes = ImportNotes {
        progressive_jpeg: frame.progressive,
        colour_profile_dropped: frame.icc,
        exif_orientation: frame.orientation.exif_value(),
        dpi_source: frame.dpi_source,
        // R30: four components, effective ColorTransform 0, and no `/Decode`
        // — which pdfcer never writes (R29), so the third condition is
        // structurally always true on this path.
        cmyk_polarity_unverifiable: frame.components == 4 && effective_transform == 0,
        ..ImportNotes::default()
    };
    if frame.progressive {
        raise_version(&mut notes.requires_pdf_version, PdfFeature::ProgressiveJpeg);
    }

    Ok(ImportedImage {
        format: ImageFormat::Jpeg,
        width: frame.width,
        height: frame.height,
        // Table 89 couples `/DCTDecode` to 8-bit delivered samples, and
        // §7.4.8 says "Each component value shall occupy a byte". `walk`
        // has already refused any other precision by name, so this is a
        // constant rather than a copy of `frame.precision`.
        bits_per_component: 8,
        color_space,
        filter: ImportFilter::DctDecode,
        // THE PASSTHROUGH. The whole file, markers and all — not the
        // entropy-coded scan alone. §7.4.8 delegates to the JPEG format,
        // and a `/DCTDecode` stream is a complete codestream: stripping the
        // APP14 marker would silently change Table 13's answer.
        data: data.to_vec(),
        soft_mask: None,
        color_key_mask: None,
        orientation: frame.orientation,
        dpi: frame.dpi,
        notes,
    })
}

// ---------------------------------------------------------------------------
// The marker walk (ITU-T T.81 §B.1)
// ---------------------------------------------------------------------------

/// Walk the marker chain to SOS, refusing unsupported frames by name.
///
/// T.81 §B.1.1.3: a marker is `0xFF` followed by a non-zero, non-`0xFF`
/// byte; `0xFF00` is a stuffed data byte and repeated `0xFF`s are fill.
/// Standalone markers (SOI, EOI, TEM, RST0–7) carry no length; every other
/// marker segment begins with a 2-byte big-endian length that **includes
/// those two bytes**.
///
/// # Why this is a separate walk from `image_codec::dct::sniff`
///
/// The decode-side sniff answers *"can `zune-jpeg` handle this, and what
/// colour transform applies"*. This one answers *"what does the PDF
/// dictionary have to say"* — it needs width, height, precision, the
/// progressive flag, EXIF orientation and the resolution, none of which the
/// decode path cares about; and it must not refuse things the decode path
/// refuses for decoder-capability reasons that do not apply to a
/// passthrough. Merging them would mean one function serving two different
/// definitions of "supported", which is how the wrong refusal reaches an
/// operator.
fn walk(data: &[u8]) -> Result<Frame, ImageImportError> {
    let corrupt = |detail: &str| ImageImportError::Corrupt {
        detail: detail.to_owned(),
    };

    let mut frame = Frame::default();
    let mut seen_sof = false;
    let mut i = 0usize;

    if data.first() != Some(&0xFF) || data.get(1) != Some(&0xD8) {
        return Err(corrupt(
            "this JPEG does not begin with a start-of-image marker",
        ));
    }
    i += 2;

    loop {
        // Skip fill bytes; find the next marker prefix.
        while data.get(i) == Some(&0xFF) && data.get(i + 1) == Some(&0xFF) {
            i += 1;
        }
        let (Some(&0xFF), Some(&marker)) = (data.get(i), data.get(i + 1)) else {
            return if seen_sof {
                Ok(frame)
            } else {
                Err(corrupt("no frame header before the end of the file"))
            };
        };
        i += 2;

        match marker {
            // Standalone markers: no length field.
            0xD8 | 0x01 | 0xD0..=0xD7 => continue,
            // Start of scan / end of image — everything needed precedes them.
            0xDA | 0xD9 => {
                return if seen_sof {
                    Ok(frame)
                } else {
                    Err(corrupt("the scan begins before any frame header"))
                };
            }
            _ => {}
        }

        let (Some(&hi), Some(&lo)) = (data.get(i), data.get(i + 1)) else {
            return Err(corrupt(
                "a marker segment's length runs past the end of the file",
            ));
        };
        let length = usize::from(u16::from_be_bytes([hi, lo]));
        let Some(payload_len) = length.checked_sub(2) else {
            return Err(corrupt("a marker segment declares a length below 2"));
        };
        let Some(payload) = data.get(i + 2..i + 2 + payload_len) else {
            return Err(corrupt("a marker segment runs past the end of the file"));
        };
        i += 2 + payload_len;

        match marker {
            // SOF markers. The gaps are real: 0xC4 is DHT, 0xC8 a reserved
            // JPG extension, 0xCC is DAC — none of them frame headers, and
            // treating one as such would read a component count out of a
            // Huffman table.
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                if seen_sof {
                    // A second frame header means a hierarchical or
                    // multi-frame stream; the first is what a decoder uses.
                    continue;
                }
                seen_sof = true;
                if let Some(feature) = unsupported_sof(marker) {
                    return Err(ImageImportError::Unsupported { feature });
                }
                frame.progressive = marker == 0xC2;
                // SOF payload: precision(1), height(2), width(2),
                // component count(1), then per-component triples.
                let (Some(&precision), Some(h), Some(w), Some(&components)) = (
                    payload.first(),
                    payload.get(1..3).and_then(|b| <[u8; 2]>::try_from(b).ok()),
                    payload.get(3..5).and_then(|b| <[u8; 2]>::try_from(b).ok()),
                    payload.get(5),
                ) else {
                    return Err(corrupt("the frame header is truncated"));
                };
                if precision != 8 {
                    return Err(ImageImportError::Unsupported {
                        feature: match precision {
                            12 => "JPEG/12-bit",
                            16 => "JPEG/16-bit",
                            _ => "JPEG/precision-unsupported",
                        },
                    });
                }
                frame.precision = precision;
                frame.height = u32::from(u16::from_be_bytes(h));
                frame.width = u32::from(u16::from_be_bytes(w));
                frame.components = components;
            }
            // APP0 — JFIF density (JFIF 1.02 §5).
            0xE0 => read_jfif(&mut frame, payload),
            // APP1 — EXIF orientation and resolution.
            0xE1 => read_exif(&mut frame, payload),
            // APP2 — an ICC profile, if it is identified as one.
            0xE2 => {
                if payload.starts_with(b"ICC_PROFILE\0") {
                    frame.icc = true;
                }
            }
            // APP14 — Adobe's colour-transform marker. TN #5116 §18: the
            // segment is 12 payload bytes, "Adobe"(5) + version(2) +
            // flags0(2) + flags1(2) + transform(1), and "a decoder shall
            // skip any APPE segment that does not begin with `Adobe`" —
            // which is why the prefix test is load-bearing rather than
            // defensive: other vendors use APP14 too.
            0xEE => {
                if payload.starts_with(b"Adobe")
                    && let Some(&transform) = payload.get(11)
                {
                    if transform > 2 {
                        // Outside Table 13's 0/1/2. pdfcer's own decoder
                        // refuses it (R27), so placing one would produce a
                        // document pdfcer cannot render — the one outcome
                        // worse than a refusal.
                        return Err(ImageImportError::Unsupported {
                            feature: match transform {
                                3 => "JPEG/adobe-transform-3",
                                _ => "JPEG/adobe-transform-unknown",
                            },
                        });
                    }
                    frame.adobe_transform = Some(transform);
                }
            }
            _ => {}
        }
    }
}

/// Map an unsupported SOF marker to its stable diagnostic key.
///
/// `None` for SOF0 (baseline sequential), SOF1 (extended sequential) and
/// SOF2 (progressive) — the three §7.4.8 covers between *"the JPEG baseline
/// format"* and the PDF 1.3 progressive extension.
const fn unsupported_sof(marker: u8) -> Option<&'static str> {
    match marker {
        0xC0..=0xC2 => None,
        // SOF3/7/11/15. Named "lossless" first because that is the property
        // that makes them a different codec, not merely a different entropy
        // coder.
        0xC3 | 0xC7 | 0xCB | 0xCF => Some("JPEG/lossless"),
        // SOF9/10/13/14.
        0xC9 | 0xCA | 0xCD | 0xCE => Some("JPEG/arithmetic"),
        // SOF5/6.
        0xC5 | 0xC6 => Some("JPEG/differential"),
        _ => Some("JPEG/unsupported-frame"),
    }
}

/// Read a JFIF APP0 density (JFIF 1.02 §5): `"JFIF\0"`, version(2),
/// units(1), Xdensity(2), Ydensity(2).
///
/// `units` 0 means *"no units; Xdensity and Ydensity specify the pixel
/// aspect ratio"* — an aspect ratio is not a resolution, and reading it as
/// one would invent a physical size the file explicitly declined to state.
fn read_jfif(frame: &mut Frame, payload: &[u8]) {
    if !payload.starts_with(b"JFIF\0") || frame.dpi.is_some() {
        return;
    }
    let (Some(&units), Some(x), Some(y)) = (
        payload.get(7),
        payload.get(8..10).and_then(|b| <[u8; 2]>::try_from(b).ok()),
        payload
            .get(10..12)
            .and_then(|b| <[u8; 2]>::try_from(b).ok()),
    ) else {
        return;
    };
    let x = f64::from(u16::from_be_bytes(x));
    let y = f64::from(u16::from_be_bytes(y));
    if x <= 0.0 || y <= 0.0 {
        return;
    }
    let scale = match units {
        1 => 1.0,    // dots per inch
        2 => 2.54,   // dots per cm
        _ => return, // 0 = aspect ratio only
    };
    frame.dpi = Some((x * scale, y * scale));
    frame.dpi_source = DpiSource::JfifDensity;
}

/// Read the EXIF IFD0 tags pdfcer uses: `Orientation` (0x0112),
/// `XResolution`/`YResolution` (0x011A/0x011B) and `ResolutionUnit`
/// (0x0128).
///
/// Structure: `"Exif\0\0"` then a whole TIFF file — a byte-order mark
/// (`II` little-endian or `MM` big-endian), the magic 42, and the offset of
/// IFD0 **from the start of the TIFF header**. An IFD is a 2-byte entry
/// count followed by 12-byte entries (tag, type, count, value-or-offset),
/// where a value of 4 bytes or fewer is stored inline.
///
/// Everything here is best-effort: a malformed EXIF block leaves the frame
/// untouched rather than failing the import. The picture is fine; only the
/// metadata is not, and refusing to place a good photograph over a bad IFD
/// would be the wrong trade.
fn read_exif(frame: &mut Frame, payload: &[u8]) {
    let Some(tiff) = payload.strip_prefix(b"Exif\0\0") else {
        return;
    };
    let big_endian = match tiff.get(0..2) {
        Some(b"MM") => true,
        Some(b"II") => false,
        _ => return,
    };
    let u16_at = |o: usize| -> Option<u16> {
        let b: [u8; 2] = tiff.get(o..o + 2)?.try_into().ok()?;
        Some(if big_endian {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    };
    let u32_at = |o: usize| -> Option<u32> {
        let b: [u8; 4] = tiff.get(o..o + 4)?.try_into().ok()?;
        Some(if big_endian {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    };

    if u16_at(2) != Some(42) {
        return;
    }
    let Some(ifd0) = u32_at(4).and_then(|v| usize::try_from(v).ok()) else {
        return;
    };
    let Some(count) = u16_at(ifd0) else { return };

    let mut x_res = None;
    let mut y_res = None;
    let mut unit = 2u16; // EXIF default: inches.
    for n in 0..usize::from(count) {
        let at = ifd0 + 2 + n * 12;
        let (Some(tag), Some(kind)) = (u16_at(at), u16_at(at + 2)) else {
            return;
        };
        match (tag, kind) {
            // SHORT, stored inline in the first 2 bytes of the value field.
            (0x0112, 3) => {
                if let Some(v) = u16_at(at + 8) {
                    frame.orientation = Orientation::from_exif(v);
                }
            }
            (0x0128, 3) => {
                if let Some(v) = u16_at(at + 8) {
                    unit = v;
                }
            }
            // RATIONAL (two u32s) — 8 bytes, so the value field holds an
            // OFFSET to them rather than the value itself.
            (0x011A | 0x011B, 5) => {
                let Some(off) = u32_at(at + 8).and_then(|v| usize::try_from(v).ok()) else {
                    continue;
                };
                let (Some(num), Some(den)) = (u32_at(off), u32_at(off + 4)) else {
                    continue;
                };
                if den == 0 {
                    continue;
                }
                let value = f64::from(num) / f64::from(den);
                if tag == 0x011A {
                    x_res = Some(value);
                } else {
                    y_res = Some(value);
                }
            }
            _ => {}
        }
    }

    // Unit 1 is "no absolute unit" — a ratio, like JFIF's units 0.
    let scale = match unit {
        2 => 1.0,
        3 => 2.54,
        _ => return,
    };
    if let (Some(x), Some(y)) = (x_res, y_res)
        && x > 0.0
        && y > 0.0
    {
        frame.dpi = Some((x * scale, y * scale));
        frame.dpi_source = DpiSource::ExifResolution;
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

    /// Assemble a codestream: SOI, the given segments, a SOF, then SOS.
    fn codestream(segments: &[(u8, Vec<u8>)], sof: (u8, u8, u16, u16, u8)) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        for (marker, payload) in segments {
            out.push(0xFF);
            out.push(*marker);
            out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
            out.extend_from_slice(payload);
        }
        let (marker, precision, height, width, components) = sof;
        let mut payload = vec![precision];
        payload.extend_from_slice(&height.to_be_bytes());
        payload.extend_from_slice(&width.to_be_bytes());
        payload.push(components);
        for c in 0..components {
            payload.extend_from_slice(&[c + 1, 0x11, 0]);
        }
        out.push(0xFF);
        out.push(marker);
        out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&payload);
        // A minimal SOS, then some entropy-coded bytes.
        out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
        out.extend_from_slice(&[0x12, 0x34, 0xFF, 0xD9]);
        out
    }

    fn adobe(transform: u8) -> (u8, Vec<u8>) {
        let mut p = b"Adobe".to_vec();
        p.extend_from_slice(&[0x00, 0x65, 0, 0, 0, 0, transform]);
        (0xEE, p)
    }

    /// THE headline property: the stream payload is the source file, byte
    /// for byte. Nothing is stripped, nothing is re-framed.
    #[test]
    fn the_codestream_is_embedded_byte_for_byte() {
        let data = codestream(&[], (0xC0, 8, 40, 60, 3));
        let img = import(&data).unwrap();
        assert_eq!(
            img.data, data,
            "verbatim, including SOI/EOI and every marker"
        );
        assert_eq!(img.filter, ImportFilter::DctDecode);
        assert_eq!((img.width, img.height), (60, 40));
        assert_eq!(img.bits_per_component, 8);
        assert_eq!(img.color_space, ImportColorSpace::DeviceRgb);
    }

    #[test]
    fn component_counts_map_to_device_colour_spaces() {
        for (n, want) in [
            (1u8, ImportColorSpace::DeviceGray),
            (3, ImportColorSpace::DeviceRgb),
            (4, ImportColorSpace::DeviceCmyk),
        ] {
            let data = codestream(&[], (0xC0, 8, 4, 4, n));
            assert_eq!(import(&data).unwrap().color_space, want);
        }
    }

    #[test]
    fn a_two_component_jpeg_is_refused_by_name() {
        let data = codestream(&[], (0xC0, 8, 4, 4, 2));
        assert_eq!(
            import(&data).unwrap_err(),
            ImageImportError::Unsupported {
                feature: "JPEG/2-component"
            }
        );
    }

    #[test]
    fn progressive_is_accepted_and_disclosed() {
        let data = codestream(&[], (0xC2, 8, 4, 4, 3));
        let img = import(&data).unwrap();
        assert!(img.notes.progressive_jpeg);
        assert_eq!(img.data, data, "still verbatim — never transcoded");
        assert_eq!(
            img.notes.requires_pdf_version,
            Some(PdfFeature::ProgressiveJpeg),
            "§7.4.8 puts the progressive extension at PDF 1.3"
        );
    }

    #[test]
    fn unsupported_frame_types_are_refused_by_name() {
        for (marker, feature) in [
            (0xC3u8, "JPEG/lossless"),
            (0xC5, "JPEG/differential"),
            (0xC9, "JPEG/arithmetic"),
            (0xCE, "JPEG/arithmetic"),
            (0xCF, "JPEG/lossless"),
        ] {
            let data = codestream(&[], (marker, 8, 4, 4, 3));
            assert_eq!(
                import(&data).unwrap_err(),
                ImageImportError::Unsupported { feature },
                "marker {marker:#04x}"
            );
        }
    }

    #[test]
    fn twelve_bit_precision_is_refused_by_name() {
        let data = codestream(&[], (0xC0, 12, 4, 4, 3));
        assert_eq!(
            import(&data).unwrap_err(),
            ImageImportError::Unsupported {
                feature: "JPEG/12-bit"
            }
        );
    }

    /// R30, mirrored onto the write direction: the ONE shape where a
    /// four-component JPEG's polarity is undeclared gets a named
    /// disclosure — and the YCCK shape, which decision 006 proved benign,
    /// does not.
    #[test]
    fn cmyk_polarity_is_disclosed_only_for_the_undeclared_shape() {
        // Transform 0: nothing declares polarity.
        let t0 = codestream(&[adobe(0)], (0xC0, 8, 4, 4, 4));
        assert!(import(&t0).unwrap().notes.cmyk_polarity_unverifiable);

        // No Adobe marker at all: Table 13's default for 4 components is
        // also 0, so the same hazard applies.
        let none = codestream(&[], (0xC0, 8, 4, 4, 4));
        assert!(import(&none).unwrap().notes.cmyk_polarity_unverifiable);

        // Transform 2 (YCCK): the transform's own definition recovers true
        // ink, verified against pdfium on nine corpus files. Benign — and a
        // warning here would cry wolf on every real CMYK JPEG.
        let t2 = codestream(&[adobe(2)], (0xC0, 8, 4, 4, 4));
        assert!(!import(&t2).unwrap().notes.cmyk_polarity_unverifiable);

        // Three components default to transform 1 and are never in scope.
        let rgb = codestream(&[], (0xC0, 8, 4, 4, 3));
        assert!(!import(&rgb).unwrap().notes.cmyk_polarity_unverifiable);
    }

    #[test]
    fn an_adobe_transform_outside_table_13_is_refused_by_name() {
        let data = codestream(&[adobe(3)], (0xC0, 8, 4, 4, 4));
        assert_eq!(
            import(&data).unwrap_err(),
            ImageImportError::Unsupported {
                feature: "JPEG/adobe-transform-3"
            }
        );
    }

    /// Another vendor's APP14 must be skipped, not misread as Adobe's —
    /// TN #5116 says so explicitly, and byte 11 of an arbitrary APP14 is
    /// arbitrary.
    #[test]
    fn a_non_adobe_app14_is_ignored() {
        let seg = (0xEEu8, b"SomeoneElse\x09extra".to_vec());
        let data = codestream(&[seg], (0xC0, 8, 4, 4, 4));
        let img = import(&data).unwrap();
        assert!(
            img.notes.cmyk_polarity_unverifiable,
            "with no Adobe marker the default transform is 0 for 4 components"
        );
    }

    #[test]
    fn a_jfif_density_in_dpi_is_read() {
        let mut p = b"JFIF\0".to_vec();
        p.extend_from_slice(&[1, 2, 1]); // version 1.2, units 1 = dpi
        p.extend_from_slice(&300u16.to_be_bytes());
        p.extend_from_slice(&150u16.to_be_bytes());
        p.extend_from_slice(&[0, 0]); // no thumbnail
        let data = codestream(&[(0xE0, p)], (0xC0, 8, 4, 4, 3));
        let img = import(&data).unwrap();
        assert_eq!(img.dpi, Some((300.0, 150.0)));
        assert_eq!(img.notes.dpi_source, DpiSource::JfifDensity);
    }

    #[test]
    fn a_jfif_aspect_ratio_is_not_a_resolution() {
        let mut p = b"JFIF\0".to_vec();
        p.extend_from_slice(&[1, 2, 0]); // units 0 = aspect ratio only
        p.extend_from_slice(&1u16.to_be_bytes());
        p.extend_from_slice(&1u16.to_be_bytes());
        p.extend_from_slice(&[0, 0]);
        let data = codestream(&[(0xE0, p)], (0xC0, 8, 4, 4, 3));
        let img = import(&data).unwrap();
        assert!(img.dpi.is_none());
        assert_eq!(img.notes.dpi_source, DpiSource::Assumed);
    }

    fn exif_ifd0(entries: &[(u16, u16, u32)], big_endian: bool, trailer: &[u8]) -> Vec<u8> {
        let mut tiff = if big_endian {
            let mut t = b"MM".to_vec();
            t.extend_from_slice(&42u16.to_be_bytes());
            t.extend_from_slice(&8u32.to_be_bytes());
            t
        } else {
            let mut t = b"II".to_vec();
            t.extend_from_slice(&42u16.to_le_bytes());
            t.extend_from_slice(&8u32.to_le_bytes());
            t
        };
        let put16 = |v: u16| {
            if big_endian {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let put32 = |v: u32| {
            if big_endian {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        tiff.extend_from_slice(&put16(entries.len() as u16));
        for &(tag, kind, value) in entries {
            tiff.extend_from_slice(&put16(tag));
            tiff.extend_from_slice(&put16(kind));
            tiff.extend_from_slice(&put32(1));
            if kind == 3 {
                // A SHORT lives in the first two bytes of the value field.
                tiff.extend_from_slice(&put16(value as u16));
                tiff.extend_from_slice(&[0, 0]);
            } else {
                tiff.extend_from_slice(&put32(value));
            }
        }
        tiff.extend_from_slice(&put32(0)); // no next IFD
        tiff.extend_from_slice(trailer);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        app1
    }

    #[test]
    fn exif_orientation_is_read_in_both_byte_orders() {
        for big_endian in [false, true] {
            let app1 = exif_ifd0(&[(0x0112, 3, 6)], big_endian, &[]);
            let data = codestream(&[(0xE1, app1)], (0xC0, 8, 40, 60, 3));
            let img = import(&data).unwrap();
            assert_eq!(img.orientation, Orientation::Rotate90, "{big_endian}");
            assert_eq!(img.notes.exif_orientation, Some(6));
            // A transposing orientation swaps the DISPLAYED size while the
            // stored /Width and /Height stay as the file wrote them.
            assert_eq!((img.width, img.height), (60, 40));
            assert_eq!(img.display_size_px(), (40, 60));
        }
    }

    #[test]
    fn orientation_1_is_not_reported_as_a_change() {
        let app1 = exif_ifd0(&[(0x0112, 3, 1)], false, &[]);
        let data = codestream(&[(0xE1, app1)], (0xC0, 8, 4, 4, 3));
        let img = import(&data).unwrap();
        assert_eq!(img.orientation, Orientation::Identity);
        assert_eq!(img.notes.exif_orientation, None);
    }

    #[test]
    fn exif_resolution_rationals_become_dpi() {
        // The IFD is 2 + 3*12 + 4 = 42 bytes long and starts at offset 8,
        // so the rationals go at 50 and 58.
        let mut trailer = Vec::new();
        trailer.extend_from_slice(&600u32.to_le_bytes());
        trailer.extend_from_slice(&2u32.to_le_bytes()); // 600/2 = 300
        trailer.extend_from_slice(&150u32.to_le_bytes());
        trailer.extend_from_slice(&1u32.to_le_bytes()); // 150/1 = 150
        let app1 = exif_ifd0(
            &[(0x011A, 5, 50), (0x011B, 5, 58), (0x0128, 3, 2)],
            false,
            &trailer,
        );
        let data = codestream(&[(0xE1, app1)], (0xC0, 8, 4, 4, 3));
        let img = import(&data).unwrap();
        assert_eq!(img.dpi, Some((300.0, 150.0)));
        assert_eq!(img.notes.dpi_source, DpiSource::ExifResolution);
    }

    #[test]
    fn an_icc_profile_in_app2_is_disclosed() {
        let mut p = b"ICC_PROFILE\0".to_vec();
        p.extend_from_slice(&[1, 1, 0, 0]);
        let data = codestream(&[(0xE2, p)], (0xC0, 8, 4, 4, 3));
        assert!(import(&data).unwrap().notes.colour_profile_dropped);
    }

    #[test]
    fn a_truncated_codestream_is_corrupt_not_a_panic() {
        let data = codestream(&[], (0xC0, 8, 4, 4, 3));
        for cut in 2..data.len() {
            // Every prefix either parses (the SOF may already be complete)
            // or errors — never panics, and never silently yields a frame
            // with a zero dimension.
            if let Ok(img) = import(data.get(..cut).unwrap()) {
                assert!(img.width > 0 && img.height > 0, "cut at {cut}");
            }
        }
    }

    #[test]
    fn a_zero_sized_frame_is_refused() {
        let data = codestream(&[], (0xC0, 8, 0, 4, 3));
        assert!(matches!(import(&data), Err(ImageImportError::Empty { .. })));
    }
}

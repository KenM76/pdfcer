//! # PNG import (RFC 2083) — the verbatim-`IDAT` passthrough and its exceptions
//!
//! Turns a PNG file into an image XObject. The headline is that **most PNGs
//! are not decoded at all**: their `IDAT` payload is copied byte for byte
//! into a `/FlateDecode` stream with `/Predictor 15`. The argument for why
//! that is sound — four separately-sourced facts about ISO 32000-1 §7.4.4.4
//! and RFC 2083 §6 lining up exactly — is in the parent module's docs and is
//! not repeated here.
//!
//! This file is about the *bookkeeping*: which PNGs qualify, what the PDF
//! dictionary has to say about them, and what the three cases that do not
//! qualify cost.
//!
//! ## The decision table
//!
//! | Colour type | Channels | Interlace | Branch | Result |
//! |---|---|---|---|---|
//! | 0 (greyscale) | 1 | none | **verbatim** | `/DeviceGray`, `/Colors 1` |
//! | 2 (truecolour) | 3 | none | **verbatim** | `/DeviceRGB`, `/Colors 3` |
//! | 3 (indexed) | 1 | none | **verbatim** | `[/Indexed /DeviceRGB hival …]`, `/Colors 1` |
//! | 0 or 2 + `tRNS` | as above | none | **verbatim** + `/Mask` | one transparent colour costs nothing (§8.9.6.4) |
//! | 3 + `tRNS` | 1 | none | **verbatim base** + decoded `/SMask` | palette alpha; the *image* still passes through |
//! | 4 (grey+alpha) | 2 | none | decode + re-deflate | `/DeviceGray` + `/SMask` |
//! | 6 (truecolour+alpha) | 4 | none | decode + re-deflate | `/DeviceRGB` + `/SMask` |
//! | any | any | **Adam7** | **refused** `PNG/interlaced` | PDF has no interlacing |
//!
//! ## Why alpha forces a decode, and why that is not a failure of nerve
//!
//! PNG stores opacity **interleaved with colour, sample by sample**: an RGBA
//! row is `R G B A R G B A …`. PDF stores it in a **separate image**
//! (§8.9.5 Table 89 gives an image exactly one `/ColorSpace` and one
//! `/BitsPerComponent`; opacity travels as `/SMask`, a whole second image
//! XObject). There is no `/DecodeParms` entry, no colour space, and no
//! filter parameter that says "channel 4 of this stream is the alpha of the
//! other three". So the samples must be pulled apart, and pulling them apart
//! means undoing the per-row prediction, which means the rows have to be
//! re-deflated on the way out.
//!
//! **The re-deflate is lossless.** Every sample is bit-identical; only the
//! deflate encoding differs, and the parent module's
//! [`flate_encode`](super::flate_encode) uses maximum compression, so the
//! result is usually *smaller* than the source. Nothing here is a quality
//! trade — which is exactly why this branch is acceptable while a JPEG
//! transcode would not be.
//!
//! ### Why not flatten alpha against white instead
//!
//! Because it is irreversible and usually wrong. A logo composited against
//! white and then placed on a coloured page shows a white box around it, and
//! nothing in the document records that the operator's file had an alpha
//! channel at all. The whole point of the `/SMask` is that the page decides
//! what shows through.
//!
//! ### The disclosure that comes with it
//!
//! `pdfcer-render` does **not yet composite `/SMask`** — it counts the
//! entry as a deferred feature and paints the base image opaque. So a
//! correctly-authored transparent PNG looks opaque in pdfcer's own preview
//! while looking right in Acrobat. That gap is disclosed by
//! [`EditSession::add_image`](crate::edit::EditSession::add_image) rather
//! than hidden, because an operator who is not told will reasonably conclude
//! the transparency was lost.
//!
//! ## The chunks this reader looks at, and the ones it deliberately ignores
//!
//! Read: `IHDR` (geometry), `PLTE` (the `/Indexed` lookup), `tRNS`
//! (transparency), `pHYs` (resolution), `IDAT` (the payload, concatenated in
//! order), and the *presence* of `iCCP`/`sRGB`/`gAMA`/`cHRM` (a colour claim
//! pdfcer cannot carry over, so it is disclosed).
//!
//! Ignored: `bKGD`, `tEXt`/`zTXt`/`iTXt`, `tIME`, `sBIT`, `hIST`, `sPLT`,
//! and every ancillary chunk. RFC 2083 §3.3 makes ancillary chunks
//! optional-to-honour by construction (the lowercase-first-letter rule), and
//! none of them changes the pixels.
//!
//! ## Spec sources
//!
//! - RFC 2083 §§3, 4.1.1 (`IHDR`), 4.1.2 (`PLTE`), 4.2.4 (`tRNS`),
//!   4.2.4.4 (`pHYs`), 6 (filters), 7.2 (bit packing), 10.1 (zlib framing)
//! - ISO 32000-1 §7.4.4 + Table 8, §7.4.4.4 + Table 10, §8.6.6.3
//!   (`/Indexed`), §8.9.3 (sample layout), §8.9.5 Table 89, §8.9.6.4
//!   (colour-key `/Mask`)

use super::{
    DpiSource, ImageFormat, ImageImportError, ImportColorSpace, ImportFilter, ImportNotes,
    ImportedImage, Orientation, PdfFeature, RecompressReason, SoftMask, check_dimensions,
    flate_encode, raise_version, row_bytes,
};
use crate::filters::{flate, predictor};

/// PNG's 8-byte signature (RFC 2083 §3.1).
const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// What the chunk walk collected before any decision was made.
///
/// A separate struct from [`ImportedImage`] because the walk must finish
/// before the branch can be chosen: `tRNS` may legally appear after `PLTE`
/// but before `IDAT`, and the colour-type branch needs both.
#[derive(Debug, Default)]
struct Png {
    width: u32,
    height: u32,
    bit_depth: u8,
    colour_type: u8,
    interlaced: bool,
    palette: Vec<u8>,
    trns: Option<Vec<u8>>,
    phys: Option<(f64, f64)>,
    colour_claim: bool,
    idat: Vec<u8>,
}

impl Png {
    /// Channels per sample for this colour type (RFC 2083 §4.1.1 Table).
    const fn channels(&self) -> u32 {
        match self.colour_type {
            0 | 3 => 1,
            4 => 2,
            2 => 3,
            _ => 4,
        }
    }

    /// Whether the colour type interleaves an alpha channel with the colour
    /// channels — the property that forces the decode branch.
    const fn has_interleaved_alpha(&self) -> bool {
        matches!(self.colour_type, 4 | 6)
    }
}

/// Parse a PNG into a PDF-ready image XObject payload.
///
/// # Errors
///
/// See [`ImageImportError`]; in particular
/// [`ImageImportError::Unsupported`] with feature `"PNG/interlaced"` for an
/// Adam7 file, which pdfcer declines rather than mis-decoding.
pub fn import(data: &[u8]) -> Result<ImportedImage, ImageImportError> {
    let png = walk(data)?;

    if png.interlaced {
        // Adam7 splits the image into seven passes, each with its own row
        // width and its own `Prior(x) = 0` first row. ISO 32000-1 §7.4.4.4
        // has one image, one first row, and no notion of a pass — so the
        // byte stream simply does not mean the same thing in the two
        // formats, and reusing it would produce stripes. pdfcer has no
        // de-interlacer, and inventing half of one is worse than saying so.
        return Err(ImageImportError::Unsupported {
            feature: "PNG/interlaced",
        });
    }

    let channels = png.channels();
    check_dimensions(png.width, png.height, channels, u32::from(png.bit_depth))?;

    let mut notes = ImportNotes {
        colour_profile_dropped: png.colour_claim,
        dpi_source: if png.phys.is_some() {
            DpiSource::PngPhys
        } else {
            DpiSource::Assumed
        },
        ..ImportNotes::default()
    };
    if png.bit_depth == 16 {
        raise_version(
            &mut notes.requires_pdf_version,
            PdfFeature::BitsPerComponent16,
        );
    }

    if png.has_interleaved_alpha() {
        return split_alpha(&png, notes);
    }
    passthrough(&png, notes)
}

// ---------------------------------------------------------------------------
// The chunk walk
// ---------------------------------------------------------------------------

/// Walk the chunk stream, collecting everything a decision could need.
///
/// Chunk framing (RFC 2083 §3.2): a 4-byte big-endian length (of the *data*
/// only), a 4-byte type, `length` data bytes, and a 4-byte CRC-32.
///
/// # Why the CRC is not verified
///
/// Deliberate, and worth stating so it is not read as an oversight. The CRC
/// protects against transmission damage, and a damaged `IDAT` will fail
/// inflate (on the decode branch) or produce a stream the reader rejects (on
/// the passthrough branch) — either way it is caught. Meanwhile real-world
/// PNGs written by sloppy tools do occasionally carry wrong CRCs on
/// *ancillary* chunks pdfcer does not even read, and refusing to place an
/// otherwise-perfect image over a bad `tEXt` checksum would be a worse
/// outcome than placing it. This is the same posture
/// `crate::filters::flate` takes toward zlib's Adler-32: fail on the data,
/// not on the wrapper.
fn walk(data: &[u8]) -> Result<Png, ImageImportError> {
    let corrupt = |detail: &str| ImageImportError::Corrupt {
        detail: detail.to_owned(),
    };

    let mut png = Png::default();
    let mut seen_ihdr = false;
    let mut i = SIGNATURE.len();
    if !data.starts_with(SIGNATURE) {
        return Err(corrupt("not a PNG signature"));
    }

    // Running out of chunks without an IEND is tolerated as long as the
    // image is complete: truncated-but-usable PNGs are common (a file copied
    // while it was still being written), and IEND carries no data.
    while let Some(header) = data.get(i..i + 8) {
        let (Some(len_bytes), Some(kind)) = (
            header.get(0..4).and_then(|b| <[u8; 4]>::try_from(b).ok()),
            header.get(4..8),
        ) else {
            break;
        };
        let len = usize::try_from(u32::from_be_bytes(len_bytes))
            .map_err(|_| corrupt("chunk length out of range"))?;
        let Some(payload) = data.get(i + 8..i + 8 + len) else {
            return Err(corrupt("chunk runs past the end of the file"));
        };
        // 8 header bytes + payload + 4 CRC bytes.
        i += 12 + len;

        match kind {
            b"IHDR" => {
                if seen_ihdr {
                    return Err(corrupt("more than one IHDR chunk"));
                }
                seen_ihdr = true;
                read_ihdr(&mut png, payload)?;
            }
            _ if !seen_ihdr => return Err(corrupt("the first chunk is not IHDR")),
            b"PLTE" => {
                if !len.is_multiple_of(3) || len == 0 || len > 256 * 3 {
                    return Err(corrupt("PLTE is not 1..=256 RGB triples"));
                }
                png.palette = payload.to_vec();
            }
            b"tRNS" => png.trns = Some(payload.to_vec()),
            b"pHYs" => {
                // RFC 2083 §4.2.4.2: x(4), y(4), unit(1). Unit 1 = metre;
                // unit 0 means "aspect ratio only, no physical size", which
                // is a ratio and NOT a resolution — reading it as dpi would
                // invent a size the file explicitly declined to state.
                if let (Some(x), Some(y), Some(&1)) = (
                    payload.get(0..4).and_then(be_u32),
                    payload.get(4..8).and_then(be_u32),
                    payload.get(8),
                ) && x > 0
                    && y > 0
                {
                    // pixels/metre → pixels/inch.
                    png.phys = Some((f64::from(x) * 0.0254, f64::from(y) * 0.0254));
                }
            }
            b"iCCP" | b"sRGB" | b"gAMA" | b"cHRM" => png.colour_claim = true,
            b"IDAT" => png.idat.extend_from_slice(payload),
            b"IEND" => break,
            _ => {}
        }
    }

    if !seen_ihdr {
        return Err(corrupt("no IHDR chunk"));
    }
    if png.idat.is_empty() {
        return Err(corrupt("no IDAT image data"));
    }
    if png.colour_type == 3 && png.palette.is_empty() {
        // RFC 2083 §4.1.2 makes PLTE mandatory for colour type 3, and
        // without it there is no way to know what any index means.
        return Err(corrupt("an indexed PNG with no PLTE chunk"));
    }
    Ok(png)
}

fn be_u32(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(0..4)?.try_into().ok()?))
}

/// Read `IHDR` and validate the combinations RFC 2083 §4.1.1 permits.
///
/// The bit-depth/colour-type matrix is validated here rather than trusted,
/// because an out-of-table combination would otherwise produce a PDF
/// dictionary describing a stream whose real geometry is different — the
/// worst kind of failure, since it renders as garbage rather than erroring.
fn read_ihdr(png: &mut Png, payload: &[u8]) -> Result<(), ImageImportError> {
    let corrupt = |detail: &str| ImageImportError::Corrupt {
        detail: detail.to_owned(),
    };
    // Destructured rather than indexed: IHDR is thirteen fixed bytes in a
    // fixed order (RFC 2083 §4.1.1), and naming them here lets a reader
    // check the layout against the RFC without counting offsets.
    let [
        w0,
        w1,
        w2,
        w3,
        h0,
        h1,
        h2,
        h3,
        bit_depth,
        colour_type,
        compression,
        filter,
        interlace,
    ] = payload
        .get(0..13)
        .and_then(|s| <[u8; 13]>::try_from(s).ok())
        .ok_or_else(|| corrupt("IHDR is shorter than 13 bytes"))?;
    png.width = u32::from_be_bytes([w0, w1, w2, w3]);
    png.height = u32::from_be_bytes([h0, h1, h2, h3]);
    png.bit_depth = bit_depth;
    png.colour_type = colour_type;
    // Compression method 0 (deflate) and filter method 0 (adaptive, the
    // five RFC 2083 §6 filters) are the ONLY values RFC 2083 defines. A
    // future method would mean the IDAT is not a zlib stream and the row
    // tags are not §7.4.4.4's tags, which is precisely what the passthrough
    // assumes.
    if compression != 0 {
        return Err(corrupt("unknown PNG compression method"));
    }
    if filter != 0 {
        return Err(corrupt("unknown PNG filter method"));
    }
    png.interlaced = match interlace {
        0 => false,
        1 => true,
        _ => return Err(corrupt("unknown PNG interlace method")),
    };

    let ok = match png.colour_type {
        0 => matches!(png.bit_depth, 1 | 2 | 4 | 8 | 16),
        3 => matches!(png.bit_depth, 1 | 2 | 4 | 8),
        2 | 4 | 6 => matches!(png.bit_depth, 8 | 16),
        _ => return Err(corrupt("unknown PNG colour type")),
    };
    if !ok {
        return Err(corrupt("bit depth not allowed for this PNG colour type"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Branch 1: verbatim passthrough (colour types 0, 2, 3)
// ---------------------------------------------------------------------------

/// Describe the source `IDAT` as a PDF stream without touching a byte of it.
///
/// The entire "conversion" is choosing four Table 8 numbers and a colour
/// space. [`ImportedImage::data`](super::ImportedImage::data) is
/// `png.idat` moved, not copied-and-transformed.
fn passthrough(png: &Png, mut notes: ImportNotes) -> Result<ImportedImage, ImageImportError> {
    let channels = png.channels();
    let color_space = match png.colour_type {
        0 => ImportColorSpace::DeviceGray,
        2 => ImportColorSpace::DeviceRgb,
        _ => {
            // §8.6.6.3: `hival` is the maximum valid index and "shall be no
            // greater than 255", which PLTE's own 256-entry cap guarantees.
            let entries = png.palette.len() / 3;
            let hival = u8::try_from(entries.saturating_sub(1)).unwrap_or(255);
            ImportColorSpace::Indexed {
                hival,
                lookup: png.palette.clone(),
            }
        }
    };

    // Transparency, which takes one of two shapes depending on what `tRNS`
    // means for this colour type (RFC 2083 §4.2.4).
    let mut color_key_mask = None;
    let mut soft_mask = None;
    if let Some(trns) = png.trns.as_ref() {
        match png.colour_type {
            0 | 2 => {
                if let Some(range) = colour_key_from_trns(png, trns) {
                    color_key_mask = Some(range);
                    notes.transparent_colour_to_mask = true;
                    raise_version(&mut notes.requires_pdf_version, PdfFeature::ColourKeyMask);
                }
            }
            3 => {
                if let Some(mask) = palette_alpha_mask(png, trns)? {
                    soft_mask = Some(mask);
                    notes.palette_alpha_to_soft_mask = true;
                    raise_version(&mut notes.requires_pdf_version, PdfFeature::SoftMask);
                }
            }
            _ => {}
        }
    }

    Ok(ImportedImage {
        format: ImageFormat::Png,
        width: png.width,
        height: png.height,
        bits_per_component: png.bit_depth,
        color_space,
        filter: ImportFilter::FlatePngPredictor {
            // §7.4.4.4's `/Colors` is PNG's channel count, `/Columns` its
            // width, `/BitsPerComponent` its bit depth. `/Predictor 15` is
            // Table 10's "PNG prediction (on encoding, PNG optimum)", which
            // is exactly what a PNG encoder's per-row filter choice is.
            colors: u8::try_from(channels).unwrap_or(1),
            bits_per_component: png.bit_depth,
            columns: png.width,
        },
        // THE PASSTHROUGH. No arithmetic has been performed on these bytes.
        data: png.idat.clone(),
        soft_mask,
        color_key_mask,
        orientation: Orientation::Identity,
        dpi: png.phys,
        notes,
    })
}

/// Turn a `tRNS` chunk on a greyscale or truecolour PNG into a §8.9.6.4
/// colour-key `/Mask` array.
///
/// RFC 2083 §4.2.4.1/§4.2.4.2: for colour type 0 the chunk is a single
/// 2-byte greyscale sample; for colour type 2 it is three 2-byte samples.
/// **The values are always stored as 16 bits regardless of the image's bit
/// depth** — at depth 8 only the low byte is significant.
///
/// §8.9.6.4 wants `[min₁ max₁ … minₙ maxₙ]` with each integer *"in the range
/// 0 to 2^BitsPerComponent − 1, representing colour values BEFORE decoding
/// with the `Decode` array"*, and masks a sample when **every** component
/// falls in its range. One transparent colour is therefore the degenerate
/// range `min = max = that value`, per component. An exact, lossless
/// translation — which is why this case keeps the verbatim passthrough while
/// palette alpha does not.
///
/// Returns `None` (rather than an error) for a malformed or out-of-range
/// `tRNS`: the image itself is fine, and silently dropping a transparency
/// pdfcer cannot express would be wrong, but so would refusing to place a
/// good picture. The caller only sets the disclosure when a mask is actually
/// produced, so `None` means "no transparency claim was made", which is what
/// an unusable chunk amounts to.
fn colour_key_from_trns(png: &Png, trns: &[u8]) -> Option<Vec<i64>> {
    let want = if png.colour_type == 0 { 1 } else { 3 };
    if trns.len() < want * 2 {
        return None;
    }
    let max = (1u32 << png.bit_depth) - 1;
    let mut out = Vec::with_capacity(want * 2);
    for pair in trns.chunks_exact(2).take(want) {
        let [hi, lo] = *pair else { return None };
        let v = u32::from(u16::from_be_bytes([hi, lo]));
        if v > max {
            return None;
        }
        out.push(i64::from(v));
        out.push(i64::from(v));
    }
    Some(out)
}

/// Build an 8-bit `/SMask` from a palette `tRNS` chunk, leaving the indexed
/// image data itself untouched.
///
/// # Why this branch exists at all
///
/// RFC 2083 §4.2.4.3 gives colour type 3 a `tRNS` that is an **array of
/// per-entry alpha bytes**, one per palette entry, entries past the end of
/// the array being fully opaque. That is a genuine 256-level alpha channel
/// expressed as a lookup — and PDF has no indexed-with-alpha colour space to
/// map it onto. So the alpha must become a real `/SMask` image, which means
/// resolving every pixel's index.
///
/// # Why the base image still passes through
///
/// Because only the *mask* needs the indices. The indexed samples themselves
/// are still a legal `/FlateDecode`+`/Predictor 15` stream describing exactly
/// the same picture, so they are left alone. pdfcer decodes to *read* the
/// indices and re-encodes only the mask it derived — the image the operator
/// supplied is still in the document byte for byte.
///
/// Returns `None` when every entry is fully opaque (a `tRNS` that says
/// nothing), so an all-255 chunk does not add a pointless mask object.
///
/// # Errors
///
/// [`ImageImportError::Corrupt`] if the `IDAT` does not inflate or does not
/// contain a whole number of predicted rows.
fn palette_alpha_mask(png: &Png, trns: &[u8]) -> Result<Option<SoftMask>, ImageImportError> {
    if trns.iter().all(|&a| a == 255) {
        return Ok(None);
    }
    let indices = decode_samples(png)?;
    let stride = row_bytes(png.width, 1, u32::from(png.bit_depth));
    let depth = u32::from(png.bit_depth);

    let mut alpha = Vec::with_capacity((png.width as usize) * (png.height as usize));
    for y in 0..png.height as usize {
        let row =
            indices
                .get(y * stride..(y + 1) * stride)
                .ok_or_else(|| ImageImportError::Corrupt {
                    detail: "the PNG has fewer rows than its IHDR declares".to_owned(),
                })?;
        for x in 0..png.width as usize {
            let idx = read_packed_sample(row, x, depth) as usize;
            // RFC 2083 §4.2.4.3: "Alpha values for the remaining palette
            // entries are assumed to be 255."
            alpha.push(trns.get(idx).copied().unwrap_or(255));
        }
    }

    Ok(Some(SoftMask {
        width: png.width,
        height: png.height,
        // Always 8, whatever the index depth: the alpha VALUES in `tRNS` are
        // bytes (RFC 2083 §4.2.4.3), so an 8-bit mask is exact and a
        // narrower one would quantize.
        bits_per_component: 8,
        data: flate_encode(&alpha)?,
    }))
}

/// Read the `n`-th sample of a byte-packed row at 1, 2, 4, 8 or 16 bits.
///
/// §8.9.3 and RFC 2083 §7.2 agree: samples pack *"from high-order to
/// low-order bits"*, so sample 0 of a 4-bit row is the **high** nibble of
/// byte 0. Getting this backwards mirrors the palette in a way that looks
/// like a corrupt file rather than like a bug.
///
/// Out-of-range reads yield 0, which matches §7.4.4.4's own rule for samples
/// outside the image.
fn read_packed_sample(row: &[u8], index: usize, bits: u32) -> u16 {
    match bits {
        16 => {
            let hi = row.get(index * 2).copied().unwrap_or(0);
            let lo = row.get(index * 2 + 1).copied().unwrap_or(0);
            u16::from_be_bytes([hi, lo])
        }
        8 => u16::from(row.get(index).copied().unwrap_or(0)),
        _ => {
            let per_byte = (8 / bits) as usize;
            let byte = row.get(index / per_byte).copied().unwrap_or(0);
            let slot = index % per_byte;
            // Slot 0 occupies the HIGH bits.
            let shift = 8 - bits * (slot as u32 + 1);
            let mask = (1u16 << bits) - 1;
            (u16::from(byte) >> shift) & mask
        }
    }
}

// ---------------------------------------------------------------------------
// Branch 2: decode + split (colour types 4 and 6)
// ---------------------------------------------------------------------------

/// Inflate and un-predict the `IDAT` into raw, byte-padded sample rows.
///
/// Reuses `crate::filters` rather than reimplementing: the inflate is the
/// same ceiling-bounded one every PDF stream goes through, and the
/// un-prediction is the same `unpredict` that has the two classic PNG bugs
/// (Average's non-modular inner sum, Paeth's normative tie-break order)
/// already pinned by tests. Rewriting either here would mean maintaining a
/// second copy of the tricky arithmetic.
///
/// The `Params` handed over describe the source exactly, and `predictor: 15`
/// is not a guess — §7.4.4.4 makes every value ≥ 10 mean *"read each row's
/// own tag"*, which is what a PNG's rows carry.
fn decode_samples(png: &Png) -> Result<Vec<u8>, ImageImportError> {
    let params = predictor::Params {
        predictor: 15,
        colors: png.channels(),
        bits_per_component: u32::from(png.bit_depth),
        columns: png.width,
    };
    let raw = flate::decode(&png.idat, None).map_err(|e| ImageImportError::Corrupt {
        detail: format!("the PNG image data could not be decompressed: {e}"),
    })?;
    predictor::unpredict(raw, &params).map_err(|e| ImageImportError::Corrupt {
        detail: format!("the PNG rows could not be un-filtered: {e}"),
    })
}

/// Split an interleaved-alpha PNG into a base image and a soft mask.
///
/// Both halves are plain `/FlateDecode` with **no predictor**. That is a
/// deliberate simplification and it is worth naming why it costs nothing
/// real: re-applying a PNG predictor would mean *choosing* a filter per row,
/// which is an encoder heuristic pdfcer would then own forever, and the
/// compression gain over max-level deflate on already-split channels is
/// small. Plain Flate is exact, has no parameters to get wrong, and is what
/// `/DecodeParms`-free streams mean everywhere else in this codebase.
fn split_alpha(png: &Png, mut notes: ImportNotes) -> Result<ImportedImage, ImageImportError> {
    let samples = decode_samples(png)?;
    let depth_bytes = usize::from(png.bit_depth / 8); // 1 or 2 — PNG forbids
    // alpha below depth 8 (RFC 2083 §4.1.1), which `read_ihdr` enforced.
    let colour_channels = if png.colour_type == 4 { 1 } else { 3 };
    let px_in = (colour_channels + 1) * depth_bytes;
    let px_colour = colour_channels * depth_bytes;
    let stride = row_bytes(png.width, png.channels(), u32::from(png.bit_depth));

    let w = png.width as usize;
    let h = png.height as usize;
    let mut base = Vec::with_capacity(w * h * px_colour);
    let mut alpha = Vec::with_capacity(w * h * depth_bytes);
    for y in 0..h {
        let row =
            samples
                .get(y * stride..(y + 1) * stride)
                .ok_or_else(|| ImageImportError::Corrupt {
                    detail: "the PNG has fewer rows than its IHDR declares".to_owned(),
                })?;
        for x in 0..w {
            let at = x * px_in;
            let px = row.get(at..at + px_in).unwrap_or(&[]);
            base.extend_from_slice(px.get(..px_colour).unwrap_or(&[]));
            alpha.extend_from_slice(px.get(px_colour..).unwrap_or(&[]));
        }
    }

    notes.recompressed = Some(RecompressReason::AlphaSplit);
    notes.alpha_to_soft_mask = true;
    raise_version(&mut notes.requires_pdf_version, PdfFeature::SoftMask);

    Ok(ImportedImage {
        format: ImageFormat::Png,
        width: png.width,
        height: png.height,
        bits_per_component: png.bit_depth,
        color_space: if png.colour_type == 4 {
            ImportColorSpace::DeviceGray
        } else {
            ImportColorSpace::DeviceRgb
        },
        filter: ImportFilter::Flate,
        data: flate_encode(&base)?,
        soft_mask: Some(SoftMask {
            width: png.width,
            height: png.height,
            // The mask keeps the source's precision. A 16-bit RGBA PNG has
            // 16-bit alpha, and quantizing it to 8 would be pdfcer discarding
            // data the operator supplied, silently.
            bits_per_component: png.bit_depth,
            data: flate_encode(&alpha)?,
        }),
        color_key_mask: None,
        orientation: Orientation::Identity,
        dpi: png.phys,
        notes,
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

    /// Build a minimal PNG from raw (unfiltered) rows, all filter tag 0.
    fn png_bytes(
        width: u32,
        height: u32,
        colour_type: u8,
        bit_depth: u8,
        rows: &[Vec<u8>],
        extra: &[(&[u8; 4], Vec<u8>)],
    ) -> Vec<u8> {
        fn chunk(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
            let mut crc = crc32(kind);
            crc = crc32_continue(crc, payload);
            let mut out = (payload.len() as u32).to_be_bytes().to_vec();
            out.extend_from_slice(kind);
            out.extend_from_slice(payload);
            out.extend_from_slice(&crc.to_be_bytes());
            out
        }
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[bit_depth, colour_type, 0, 0, 0]);

        let mut raw = Vec::new();
        for r in rows {
            raw.push(0u8); // filter type None
            raw.extend_from_slice(r);
        }
        let idat = {
            use flate2::{Compression, write::ZlibEncoder};
            use std::io::Write;
            let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
            e.write_all(&raw).unwrap();
            e.finish().unwrap()
        };

        let mut out = SIGNATURE.to_vec();
        out.extend_from_slice(&chunk(b"IHDR", &ihdr));
        for (kind, payload) in extra {
            out.extend_from_slice(&chunk(kind, payload));
        }
        out.extend_from_slice(&chunk(b"IDAT", &idat));
        out.extend_from_slice(&chunk(b"IEND", &[]));
        out
    }

    fn crc32(bytes: &[u8]) -> u32 {
        crc32_continue(0, bytes)
    }
    fn crc32_continue(prev: u32, bytes: &[u8]) -> u32 {
        // Bitwise CRC-32 (RFC 2083 §15). Slow and obviously correct; only
        // test fixtures go through it.
        let mut c = prev ^ 0xFFFF_FFFF;
        for &b in bytes {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        c ^ 0xFFFF_FFFF
    }

    #[test]
    fn a_truecolour_png_passes_its_idat_through_byte_for_byte() {
        let rows = vec![vec![1, 2, 3, 4, 5, 6], vec![7, 8, 9, 10, 11, 12]];
        let bytes = png_bytes(2, 2, 2, 8, &rows, &[]);
        let img = import(&bytes).unwrap();

        // The IDAT payload, extracted independently of the importer.
        let idat = extract_idat(&bytes);
        assert_eq!(
            img.data, idat,
            "the stream payload IS the source IDAT, not a re-encoding of it"
        );
        assert_eq!(
            img.filter,
            ImportFilter::FlatePngPredictor {
                colors: 3,
                bits_per_component: 8,
                columns: 2
            }
        );
        assert_eq!(img.color_space, ImportColorSpace::DeviceRgb);
        assert!(img.notes.recompressed.is_none());
    }

    fn extract_idat(png: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 8;
        while i + 8 <= png.len() {
            let len = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
            if &png[i + 4..i + 8] == b"IDAT" {
                out.extend_from_slice(&png[i + 8..i + 8 + len]);
            }
            i += 12 + len;
        }
        out
    }

    #[test]
    fn an_indexed_png_becomes_an_indexed_colour_space() {
        let palette = vec![0, 0, 0, 255, 0, 0, 0, 255, 0];
        let rows = vec![vec![0, 1], vec![2, 0]];
        let bytes = png_bytes(2, 2, 3, 8, &rows, &[(b"PLTE", palette.clone())]);
        let img = import(&bytes).unwrap();
        assert_eq!(
            img.color_space,
            ImportColorSpace::Indexed {
                hival: 2,
                lookup: palette
            }
        );
        assert_eq!(img.data, extract_idat(&bytes), "still a passthrough");
    }

    /// One transparent colour becomes a colour-key `/Mask` — and the image
    /// data is STILL the source IDAT. This is the case that would be lost if
    /// transparency were treated as "always means decode".
    #[test]
    fn trns_on_truecolour_becomes_a_colour_key_mask_without_recompressing() {
        let rows = vec![vec![0, 0, 255, 5, 5, 5]];
        let trns = vec![0, 0, 0, 0, 0, 255]; // R=0 G=0 B=255, as 16-bit each
        let bytes = png_bytes(2, 1, 2, 8, &rows, &[(b"tRNS", trns)]);
        let img = import(&bytes).unwrap();
        assert_eq!(img.color_key_mask, Some(vec![0, 0, 0, 0, 255, 255]));
        assert!(img.notes.transparent_colour_to_mask);
        assert!(img.soft_mask.is_none());
        assert_eq!(img.data, extract_idat(&bytes));
        assert_eq!(
            img.notes.requires_pdf_version,
            Some(PdfFeature::ColourKeyMask)
        );
    }

    #[test]
    fn palette_alpha_becomes_a_soft_mask_while_the_image_still_passes_through() {
        let palette = vec![0, 0, 0, 255, 0, 0, 0, 255, 0];
        let rows = vec![vec![0, 1], vec![2, 0]];
        let bytes = png_bytes(
            2,
            2,
            3,
            8,
            &rows,
            &[(b"PLTE", palette), (b"tRNS", vec![0, 128])],
        );
        let img = import(&bytes).unwrap();
        assert_eq!(img.data, extract_idat(&bytes), "the base image is verbatim");
        let mask = img.soft_mask.expect("palette alpha becomes a soft mask");
        assert_eq!(
            (mask.width, mask.height, mask.bits_per_component),
            (2, 2, 8)
        );
        let alpha = flate::decode(&mask.data, None).unwrap();
        // index 0 -> 0, index 1 -> 128, index 2 -> (absent) 255.
        assert_eq!(alpha, vec![0, 128, 255, 0]);
        assert!(img.notes.palette_alpha_to_soft_mask);
    }

    #[test]
    fn an_all_opaque_trns_adds_no_mask() {
        let palette = vec![0, 0, 0, 255, 0, 0];
        let rows = vec![vec![0, 1]];
        let bytes = png_bytes(
            2,
            1,
            3,
            8,
            &rows,
            &[(b"PLTE", palette), (b"tRNS", vec![255, 255])],
        );
        let img = import(&bytes).unwrap();
        assert!(img.soft_mask.is_none());
        assert!(!img.notes.palette_alpha_to_soft_mask);
    }

    #[test]
    fn rgba_splits_into_a_base_image_and_a_soft_mask() {
        let rows = vec![vec![10, 20, 30, 40, 50, 60, 70, 80]];
        let bytes = png_bytes(2, 1, 6, 8, &rows, &[]);
        let img = import(&bytes).unwrap();
        assert_eq!(img.color_space, ImportColorSpace::DeviceRgb);
        assert_eq!(img.filter, ImportFilter::Flate);
        assert_eq!(
            flate::decode(&img.data, None).unwrap(),
            vec![10, 20, 30, 50, 60, 70],
            "the colour channels, de-interleaved"
        );
        let mask = img.soft_mask.unwrap();
        assert_eq!(
            flate::decode(&mask.data, None).unwrap(),
            vec![40, 80],
            "the alpha channel, on its own"
        );
        assert_eq!(img.notes.recompressed, Some(RecompressReason::AlphaSplit));
        assert!(img.notes.alpha_to_soft_mask);
    }

    #[test]
    fn sixteen_bit_alpha_keeps_sixteen_bits() {
        // One pixel: R=0x0102 G=0x0304 B=0x0506 A=0x0708.
        let rows = vec![vec![1, 2, 3, 4, 5, 6, 7, 8]];
        let bytes = png_bytes(1, 1, 6, 16, &rows, &[]);
        let img = import(&bytes).unwrap();
        assert_eq!(img.bits_per_component, 16);
        let mask = img.soft_mask.unwrap();
        assert_eq!(mask.bits_per_component, 16);
        assert_eq!(flate::decode(&mask.data, None).unwrap(), vec![7, 8]);
        assert_eq!(
            flate::decode(&img.data, None).unwrap(),
            vec![1, 2, 3, 4, 5, 6]
        );
        assert_eq!(
            img.notes.requires_pdf_version,
            Some(PdfFeature::BitsPerComponent16),
            "16 bpc is the later floor of {{1.4 SMask, 1.5 bpc16}}"
        );
    }

    #[test]
    fn greyscale_alpha_splits_too() {
        let rows = vec![vec![90, 10, 200, 250]];
        let bytes = png_bytes(2, 1, 4, 8, &rows, &[]);
        let img = import(&bytes).unwrap();
        assert_eq!(img.color_space, ImportColorSpace::DeviceGray);
        assert_eq!(flate::decode(&img.data, None).unwrap(), vec![90, 200]);
        assert_eq!(
            flate::decode(&img.soft_mask.unwrap().data, None).unwrap(),
            vec![10, 250]
        );
    }

    #[test]
    fn an_interlaced_png_is_refused_by_name() {
        let rows = vec![vec![1, 2, 3]];
        let mut bytes = png_bytes(1, 1, 2, 8, &rows, &[]);
        // Flip IHDR's interlace byte (the 13th of the payload, at offset
        // 8 + 8 + 12) and leave the CRC stale — this reader does not check
        // CRCs, by design (see `walk`).
        bytes[8 + 8 + 12] = 1;
        assert_eq!(
            import(&bytes).unwrap_err(),
            ImageImportError::Unsupported {
                feature: "PNG/interlaced"
            }
        );
    }

    #[test]
    fn a_phys_chunk_in_metres_becomes_dpi() {
        let rows = vec![vec![1, 2, 3]];
        // 11811 px/m ≈ 300 dpi.
        let mut phys = 11811u32.to_be_bytes().to_vec();
        phys.extend_from_slice(&11811u32.to_be_bytes());
        phys.push(1);
        let bytes = png_bytes(1, 1, 2, 8, &rows, &[(b"pHYs", phys)]);
        let img = import(&bytes).unwrap();
        let (dx, _) = img.dpi.expect("pHYs in metres is a resolution");
        assert!((dx - 300.0).abs() < 0.5, "{dx}");
        assert_eq!(img.notes.dpi_source, DpiSource::PngPhys);
    }

    /// Unit 0 means "aspect ratio only" — a ratio, not a resolution. Reading
    /// it as dpi would invent a physical size the file declined to state.
    #[test]
    fn a_phys_chunk_with_no_unit_is_not_a_resolution() {
        let rows = vec![vec![1, 2, 3]];
        let mut phys = 1u32.to_be_bytes().to_vec();
        phys.extend_from_slice(&1u32.to_be_bytes());
        phys.push(0);
        let bytes = png_bytes(1, 1, 2, 8, &rows, &[(b"pHYs", phys)]);
        let img = import(&bytes).unwrap();
        assert!(img.dpi.is_none());
        assert_eq!(img.notes.dpi_source, DpiSource::Assumed);
    }

    #[test]
    fn an_embedded_colour_profile_is_disclosed_not_carried() {
        let rows = vec![vec![1, 2, 3]];
        let bytes = png_bytes(1, 1, 2, 8, &rows, &[(b"sRGB", vec![0])]);
        assert!(import(&bytes).unwrap().notes.colour_profile_dropped);
    }

    #[test]
    fn sub_byte_samples_read_high_bits_first() {
        // 4-bit: byte 0xAB is sample 0 = 0xA, sample 1 = 0xB.
        assert_eq!(read_packed_sample(&[0xAB], 0, 4), 0xA);
        assert_eq!(read_packed_sample(&[0xAB], 1, 4), 0xB);
        // 2-bit: 0b11_10_01_00.
        assert_eq!(read_packed_sample(&[0b1110_0100], 0, 2), 0b11);
        assert_eq!(read_packed_sample(&[0b1110_0100], 3, 2), 0b00);
        // 1-bit.
        assert_eq!(read_packed_sample(&[0b1000_0000], 0, 1), 1);
        assert_eq!(read_packed_sample(&[0b1000_0000], 1, 1), 0);
        // 16-bit is big-endian, matching both RFC 2083 and §8.9.3.
        assert_eq!(read_packed_sample(&[0x12, 0x34], 0, 16), 0x1234);
    }

    #[test]
    fn four_bit_palette_alpha_reads_the_right_indices() {
        let palette = vec![0, 0, 0, 255, 0, 0, 0, 255, 0];
        // Two 4-bit samples in one byte: index 2 then index 1.
        let rows = vec![vec![0x21]];
        let bytes = png_bytes(
            2,
            1,
            3,
            4,
            &rows,
            &[(b"PLTE", palette), (b"tRNS", vec![10, 20, 30])],
        );
        let img = import(&bytes).unwrap();
        let mask = img.soft_mask.unwrap();
        assert_eq!(flate::decode(&mask.data, None).unwrap(), vec![30, 20]);
    }

    /// Any cut that removes part of the IMAGE DATA must be refused, and must
    /// be refused as a diagnosis rather than a panic.
    #[test]
    fn a_truncated_chunk_is_corrupt_not_a_panic() {
        let rows = vec![vec![1, 2, 3]];
        let bytes = png_bytes(1, 1, 2, 8, &rows, &[]);
        // Everything up to the end of the IDAT PAYLOAD. Past that point
        // only the IDAT CRC (4 bytes, deliberately unverified — see `walk`)
        // and the 12-byte IEND remain, and neither carries image data.
        let idat_end = bytes.len() - 16;
        for cut in 1..idat_end {
            let err = import(bytes.get(..cut).unwrap_or(&bytes));
            assert!(err.is_err(), "a PNG cut at {cut} must not be accepted");
        }
    }

    /// The deliberate tolerance, pinned so it is not "fixed" later: a file
    /// whose trailing CRC or IEND is missing — a copy taken while the file
    /// was still being written — still has a complete image, and neither
    /// carries pixels. Refusing it would lose a picture for the sake of a
    /// terminator this reader does not check anyway.
    #[test]
    fn a_missing_iend_is_tolerated() {
        let rows = vec![vec![1, 2, 3]];
        let bytes = png_bytes(1, 1, 2, 8, &rows, &[]);
        let idat_end = bytes.len() - 16;
        for cut in idat_end..=bytes.len() {
            let img = import(bytes.get(..cut).unwrap_or(&bytes))
                .unwrap_or_else(|e| panic!("cut at {cut}: {e}"));
            assert_eq!(img.data, extract_idat(&bytes));
        }
    }

    #[test]
    fn an_indexed_png_with_no_palette_is_corrupt() {
        let rows = vec![vec![0]];
        let bytes = png_bytes(1, 1, 3, 8, &rows, &[]);
        assert!(matches!(
            import(&bytes),
            Err(ImageImportError::Corrupt { .. })
        ));
    }

    #[test]
    fn a_zero_pixel_png_is_refused() {
        let bytes = png_bytes(0, 1, 2, 8, &[vec![]], &[]);
        assert!(matches!(
            import(&bytes),
            Err(ImageImportError::Empty { .. })
        ));
    }
}

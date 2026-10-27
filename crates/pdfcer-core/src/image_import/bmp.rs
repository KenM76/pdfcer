//! # BMP import — the one format with nothing to pass through
//!
//! Windows Bitmap is included for one reason: on Windows it is what a
//! screenshot, a clipboard paste and half the CAD-adjacent tooling produce,
//! and refusing the operator's most reachable image format would be a poor
//! trade for a container that takes two hundred lines to read.
//!
//! It is also the format where this module's governing rule — pass bytes
//! through, never re-encode — has nothing to work with. `BI_RGB` is
//! *uncompressed*, and PDF has no "raw samples" image filter worth using, so
//! every BMP is decoded and deflated. That costs nothing in quality (the
//! samples are bit-identical) and usually shrinks the file dramatically,
//! since a 24-bit BMP is one of the least compact ways ever devised to store
//! a picture. The re-compression is still **disclosed**
//! ([`RecompressReason::NoCompressedSource`]), because the operator's file
//! and the embedded stream are no longer the same bytes and rule 4 does not
//! carve out an exception for changes that happen to be improvements.
//!
//! ## What is read
//!
//! `BITMAPFILEHEADER` (14 bytes: `"BM"`, file size, two reserved words, the
//! offset of the pixel data) followed by a `BITMAPINFOHEADER` whose first
//! field is its own size. Headers of 40 bytes (`BITMAPINFOHEADER`), 108
//! (`BITMAPV4HEADER`) and 124 (`BITMAPV5HEADER`) are all accepted — the
//! later ones only *append* fields, so the first 40 bytes mean the same
//! thing and the extra colour-management fields are exactly the kind of
//! claim pdfcer discloses rather than carries.
//!
//! ## The traps
//!
//! **Row order.** BMP stores rows **bottom-up** — the first row in the file
//! is the bottom row of the picture — unless `biHeight` is **negative**,
//! which means top-down. §8.9.4 puts PDF's image row 0 at the *top*
//! (*"The coordinate origin (0, 0) is at the upper-left corner of the
//! image"*, and the samples are *"ordered by row"*). So the common case
//! needs the rows reversed, and a reader that ignores the sign of
//! `biHeight` silently mirrors half the world's BMPs vertically. Both
//! directions are tested.
//!
//! **The fourth byte of a 32-bit pixel is NOT alpha.** A 32-bit `BI_RGB`
//! bitmap is documented as `BGRX`: the high byte is padding, and a great
//! many writers leave it **zero**. Treating it as opacity would make such an
//! image entirely invisible — the most spectacular available way to get BMP
//! wrong, and the reason
//! [`ImportNotes::bmp_fourth_byte_ignored`](super::ImportNotes::bmp_fourth_byte_ignored)
//! exists as a disclosure rather than as a silent decision. (Alpha in a BMP
//! requires `BI_BITFIELDS` with an explicit alpha mask, or a
//! `BITMAPV4HEADER`'s `bV4AlphaMask` — both of which this reader refuses by
//! name rather than half-supports.)
//!
//! **A pixel offset of zero.** Some producers write `bfOffBits = 0`, or the
//! header size alone with the colour table forgotten. Any declared offset at
//! or short of the end of the headers is replaced by the computed one
//! (headers, bit masks, colour table) when the pixel array fits after it,
//! and disclosed as
//! [`ImportNotes::bmp_pixel_offset_repaired`](super::ImportNotes::bmp_pixel_offset_repaired).
//!
//! ## What is refused, by name
//!
//! | Property | Feature key |
//! |---|---|
//! | `BI_RLE8` / `BI_RLE4` | `BMP/rle8`, `BMP/rle4` |
//! | `BI_BITFIELDS` / `BI_ALPHABITFIELDS` | `BMP/bitfields` |
//! | `BI_JPEG` / `BI_PNG` (a wrapped file) | `BMP/embedded-codec` |
//! | 16-bit (5-5-5 or 5-6-5 packed) | `BMP/16-bit` |
//! | `BITMAPCOREHEADER` (OS/2 1.x, 12-byte header) | `BMP/core-header` |
//!
//! ## Spec sources
//!
//! - Microsoft `BITMAPFILEHEADER` / `BITMAPINFOHEADER` / `BITMAPV4HEADER`
//!   (the Windows GDI documentation; no ISO standard exists for BMP)
//! - ISO 32000-1 §8.9.3 (sample layout), §8.9.4 (row order and the image
//!   coordinate system), §8.6.6.3 (`/Indexed`)

use super::{
    DpiSource, ImageFormat, ImageImportError, ImportColorSpace, ImportFilter, ImportNotes,
    ImportedImage, Orientation, RecompressReason, check_dimensions, flate_encode, row_bytes,
};

/// Parse a BMP into a PDF-ready image XObject payload.
///
/// # Errors
///
/// See [`ImageImportError`]. Every compression method and bit depth pdfcer
/// declines is [`ImageImportError::Unsupported`] with a stable key.
pub fn import(data: &[u8]) -> Result<ImportedImage, ImageImportError> {
    let h = read_header(data)?;
    let (color_space, components, out_bits) = colour_space(data, &h)?;
    check_dimensions(h.width, h.height, components, out_bits)?;

    // BMP rows are padded to a 4-byte boundary; PDF rows are padded to a
    // whole byte (§8.9.3). They agree only by accident, so the payload is
    // repacked row by row rather than copied.
    let src_stride = ((h.width as usize * usize::from(h.bpp)).div_ceil(8)).next_multiple_of(4);
    let dst_stride = row_bytes(h.width, components, out_bits);
    let pixel_bytes = src_stride * h.height as usize;
    let repaired = repaired_pixel_offset(&h, pixel_bytes, data.len());
    let pixels = usize::try_from(repaired.unwrap_or(h.pixel_offset))
        .ok()
        .and_then(|o| data.get(o..))
        .ok_or_else(|| corrupt("the BMP pixel data starts past the end of the file"))?;
    if pixels.len() < pixel_bytes {
        return Err(corrupt(
            "the BMP pixel data is shorter than its header declares",
        ));
    }
    let out = repack(pixels, &h, src_stride, dst_stride)?;

    // biXPelsPerMeter / biYPelsPerMeter. Zero is what most writers emit
    // when they have nothing to say, and it is not a resolution.
    let dpi = match (le32(data, 38), le32(data, 42)) {
        (Some(x), Some(y)) if x > 0 && y > 0 => {
            Some((f64::from(x) * 0.0254, f64::from(y) * 0.0254))
        }
        _ => None,
    };

    Ok(ImportedImage {
        format: ImageFormat::Bmp,
        width: h.width,
        height: h.height,
        bits_per_component: u8::try_from(out_bits).unwrap_or(8),
        color_space,
        filter: ImportFilter::Flate,
        data: flate_encode(&out)?,
        soft_mask: None,
        color_key_mask: None,
        orientation: Orientation::Identity,
        dpi,
        notes: ImportNotes {
            recompressed: Some(RecompressReason::NoCompressedSource),
            bmp_fourth_byte_ignored: h.bpp == 32 && h.height > 0,
            bmp_pixel_offset_repaired: repaired.map(|used| (h.pixel_offset, used)),
            dpi_source: if dpi.is_some() {
                DpiSource::BmpPelsPerMeter
            } else {
                DpiSource::Assumed
            },
            ..ImportNotes::default()
        },
    })
}

/// The `BITMAPFILEHEADER` and `BITMAPINFOHEADER` fields this reader uses.
struct Header {
    /// `bfOffBits` as declared.
    pixel_offset: u32,
    /// `biSize`.
    header_size: usize,
    width: u32,
    height: u32,
    /// `biHeight` was negative: rows are stored top-down.
    top_down: bool,
    bpp: u16,
    /// `biCompression`.
    compression: u32,
    /// `biClrUsed`; 0 when absent.
    colours_used: u32,
}

fn corrupt(detail: &str) -> ImageImportError {
    ImageImportError::Corrupt {
        detail: detail.to_owned(),
    }
}

fn le32(data: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(o..o.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn le16(data: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(o..o.checked_add(2)?)?.try_into().ok()?,
    ))
}

/// Read the headers, refusing what this reader declines by name.
fn read_header(data: &[u8]) -> Result<Header, ImageImportError> {
    // BITMAPFILEHEADER: "BM"(2), bfSize(4), reserved(4), bfOffBits(4).
    let Some(pixel_offset) = le32(data, 10) else {
        return Err(corrupt("the BMP file header is truncated"));
    };
    let Some(header_size) = le32(data, 14).and_then(|v| usize::try_from(v).ok()) else {
        return Err(corrupt("the BMP info header is truncated"));
    };
    if header_size == 12 {
        // BITMAPCOREHEADER: different field widths (16-bit dimensions,
        // 3-byte palette entries), so it is a different parse, not a subset.
        return Err(ImageImportError::Unsupported {
            feature: "BMP/core-header",
        });
    }
    if header_size < 40 {
        return Err(corrupt("unrecognised BMP info-header size"));
    }
    let (Some(width_raw), Some(height_raw), Some(bpp), Some(compression)) = (
        le32(data, 18).map(|v| v as i32),
        le32(data, 22).map(|v| v as i32),
        le16(data, 28),
        le32(data, 30),
    ) else {
        return Err(corrupt("the BMP info header is truncated"));
    };
    if let Some(feature) = unsupported_compression(compression) {
        return Err(ImageImportError::Unsupported { feature });
    }
    // A negative biHeight means the rows are stored TOP-DOWN. A negative
    // width has no meaning.
    let Ok(width) = u32::try_from(width_raw) else {
        return Err(corrupt("the BMP declares a negative width"));
    };
    Ok(Header {
        pixel_offset,
        header_size,
        width,
        height: height_raw.unsigned_abs(),
        top_down: height_raw < 0,
        bpp,
        compression,
        colours_used: le32(data, 46).unwrap_or(0),
    })
}

/// The PDF colour space, component count and bits per component.
fn colour_space(data: &[u8], h: &Header) -> Result<(ImportColorSpace, u32, u32), ImageImportError> {
    match h.bpp {
        1 | 4 | 8 => {
            let max = 1usize << h.bpp;
            let entries = usize::try_from(h.colours_used)
                .ok()
                .filter(|&n| n > 0)
                .unwrap_or(max)
                .min(max);
            let start = h.header_size.saturating_add(14);
            let table = data
                .get(start..start.saturating_add(entries * 4))
                .ok_or_else(|| corrupt("the BMP colour table is truncated"))?;
            // BMP palette entries are BGR + one reserved byte; §8.6.6.3
            // wants consecutive RGB triples.
            let mut lookup = Vec::with_capacity(entries * 3);
            for e in table.chunks_exact(4) {
                if let [b, g, r, _] = *e {
                    lookup.extend_from_slice(&[r, g, b]);
                }
            }
            let hival = u8::try_from(entries.saturating_sub(1)).unwrap_or(255);
            Ok((
                ImportColorSpace::Indexed { hival, lookup },
                1,
                u32::from(h.bpp),
            ))
        }
        24 | 32 => Ok((ImportColorSpace::DeviceRgb, 3, 8)),
        16 => Err(ImageImportError::Unsupported {
            feature: "BMP/16-bit",
        }),
        _ => Err(corrupt("unsupported BMP bit depth")),
    }
}

/// The pixel array's offset when the declared `bfOffBits` points inside or
/// exactly at the end of the headers, or `None` to keep the declared one.
///
/// An offset strictly past the header end is used as given, even inside the
/// table or masks: a file whose `biClrUsed` overstates its table can still
/// point at its real pixels.
///
/// The computed offset is the end of the headers, plus the
/// `BI_BITFIELDS` (12) or `BI_ALPHABITFIELDS` (16) masks a 40-byte header
/// leaves outside itself, plus the colour table (`biClrUsed` entries, or
/// `1 << bpp` at 1, 4 and 8 bits when it is 0; 4 bytes each, 3 after a
/// 12-byte core header). It is used only when the whole pixel array fits
/// after it, and differs from the declared one.
fn repaired_pixel_offset(h: &Header, pixel_bytes: usize, file_len: usize) -> Option<u32> {
    let headers_end = h.header_size.checked_add(14)?;
    let declared = usize::try_from(h.pixel_offset).ok()?;
    if declared > headers_end {
        return None;
    }
    let masks = match (h.header_size, h.compression) {
        (40, 3) => 12,
        (40, 6) => 16,
        _ => 0,
    };
    let entries = match usize::try_from(h.colours_used).ok()? {
        0 if matches!(h.bpp, 1 | 4 | 8) => 1usize << h.bpp,
        n => n,
    };
    let entry_size = if h.header_size == 12 { 3 } else { 4 };
    let computed = headers_end
        .checked_add(masks)?
        .checked_add(entries.checked_mul(entry_size)?)?;
    if computed == declared || computed.checked_add(pixel_bytes)? > file_len {
        return None;
    }
    u32::try_from(computed).ok()
}

/// Repack the rows into PDF order (row 0 at the top, §8.9.4) and padding.
fn repack(
    pixels: &[u8],
    h: &Header,
    src_stride: usize,
    dst_stride: usize,
) -> Result<Vec<u8>, ImageImportError> {
    let height = h.height as usize;
    let mut out = vec![0u8; dst_stride * height];
    for y in 0..height {
        // The source row for PDF row `y` (which is the TOP row when y = 0).
        let src_y = if h.top_down { y } else { height - 1 - y };
        let src = pixels
            .get(src_y * src_stride..src_y * src_stride + src_stride)
            .ok_or_else(|| corrupt("a BMP row runs past the end of the file"))?;
        let dst = out
            .get_mut(y * dst_stride..(y + 1) * dst_stride)
            .ok_or_else(|| corrupt("row buffer overflow"))?;
        match h.bpp {
            24 => {
                for (x, px) in dst.chunks_exact_mut(3).enumerate() {
                    // BGR -> RGB. A short source row cannot happen (the
                    // stride check rules it out) but is left black rather
                    // than allowed to panic.
                    if let Some(&[b, g, r]) = src
                        .get(x * 3..x * 3 + 3)
                        .and_then(|s| <&[u8; 3]>::try_from(s).ok())
                    {
                        px.copy_from_slice(&[r, g, b]);
                    }
                }
            }
            32 => {
                for (x, px) in dst.chunks_exact_mut(3).enumerate() {
                    // BGRX -> RGB. The fourth byte is DROPPED, not read as
                    // alpha — see the module docs.
                    if let Some(&[b, g, r, _]) = src
                        .get(x * 4..x * 4 + 4)
                        .and_then(|s| <&[u8; 4]>::try_from(s).ok())
                    {
                        px.copy_from_slice(&[r, g, b]);
                    }
                }
            }
            // 1/4/8-bit indices are already packed high-order-bit-first,
            // exactly as §8.9.3 wants, so only the stride changes.
            _ => dst.copy_from_slice(src.get(..dst_stride).unwrap_or(&[])),
        }
    }
    Ok(out)
}

/// Map a `biCompression` value pdfcer declines to its stable feature key.
///
/// `None` for `BI_RGB` (0), the only uncompressed form — which is the only
/// form with a straightforward sample layout, and therefore the only one
/// worth half of this module.
const fn unsupported_compression(value: u32) -> Option<&'static str> {
    match value {
        0 => None,
        1 => Some("BMP/rle8"),
        2 => Some("BMP/rle4"),
        3 | 6 => Some("BMP/bitfields"),
        4 | 5 => Some("BMP/embedded-codec"),
        _ => Some("BMP/unknown-compression"),
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
    use crate::filters::flate;

    /// Build a BITMAPINFOHEADER bitmap. `rows` are given in FILE order.
    fn bmp(
        width: i32,
        height: i32,
        bpp: u16,
        rows: &[Vec<u8>],
        palette: &[u8],
        compression: u32,
    ) -> Vec<u8> {
        let stride =
            ((width.unsigned_abs() as usize * usize::from(bpp)).div_ceil(8)).next_multiple_of(4);
        let mut body = Vec::new();
        for r in rows {
            let mut row = r.clone();
            row.resize(stride, 0);
            body.extend_from_slice(&row);
        }
        let offset = 14 + 40 + palette.len();
        let mut out = b"BM".to_vec();
        out.extend_from_slice(&((offset + body.len()) as u32).to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&bpp.to_le_bytes());
        out.extend_from_slice(&compression.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&2835u32.to_le_bytes());
        out.extend_from_slice(&2835u32.to_le_bytes());
        out.extend_from_slice(&((palette.len() / 4) as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(palette);
        out.extend_from_slice(&body);
        out
    }

    /// The row-order trap, pinned in both directions: the SAME picture
    /// stored bottom-up and top-down must import to the same PDF samples.
    #[test]
    fn bottom_up_and_top_down_produce_the_same_image() {
        // Two rows, one pixel each: top is red, bottom is blue. In BGR.
        let top = vec![0, 0, 255];
        let bottom = vec![255, 0, 0];

        let up = bmp(1, 2, 24, &[bottom.clone(), top.clone()], &[], 0);
        let down = bmp(1, -2, 24, &[top.clone(), bottom.clone()], &[], 0);

        let a = flate::decode(&import(&up).unwrap().data, None).unwrap();
        let b = flate::decode(&import(&down).unwrap().data, None).unwrap();
        assert_eq!(a, b);
        assert_eq!(
            a,
            vec![255, 0, 0, 0, 0, 255],
            "PDF row 0 is the TOP row (§8.9.4): red first, then blue"
        );
    }

    #[test]
    fn a_32_bit_bmp_ignores_its_fourth_byte_and_says_so() {
        // A fully-opaque red pixel whose padding byte is ZERO — the shape
        // that renders invisible if the byte is mistaken for alpha.
        let file = bmp(1, 1, 32, &[vec![0, 0, 255, 0]], &[], 0);
        let img = import(&file).unwrap();
        assert!(img.notes.bmp_fourth_byte_ignored);
        assert!(img.soft_mask.is_none(), "a BI_RGB BMP has no alpha channel");
        assert_eq!(flate::decode(&img.data, None).unwrap(), vec![255, 0, 0]);
    }

    #[test]
    fn a_palette_bmp_becomes_an_indexed_colour_space() {
        // Two entries, stored BGRX: black then red.
        let palette = vec![0, 0, 0, 0, 0, 0, 255, 0];
        let file = bmp(2, 1, 8, &[vec![1, 0]], &palette, 0);
        let img = import(&file).unwrap();
        assert_eq!(
            img.color_space,
            ImportColorSpace::Indexed {
                hival: 1,
                lookup: vec![0, 0, 0, 255, 0, 0],
            }
        );
        assert_eq!(img.bits_per_component, 8);
        assert_eq!(flate::decode(&img.data, None).unwrap(), vec![1, 0]);
    }

    /// BMP pads rows to four bytes; PDF pads to one (§8.9.3). A 3-pixel
    /// 24-bit row is 9 bytes in PDF and 12 in the file, so a straight copy
    /// would smear the padding into the next row.
    #[test]
    fn row_padding_is_repacked_not_copied() {
        let row = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 0xFF, 0xFF, 0xFF];
        let file = bmp(3, 1, 24, &[row], &[], 0);
        let img = import(&file).unwrap();
        let out = flate::decode(&img.data, None).unwrap();
        assert_eq!(out.len(), 9, "no BMP stride padding survives");
        assert_eq!(out, vec![3, 2, 1, 6, 5, 4, 9, 8, 7], "BGR became RGB");
    }

    #[test]
    fn sub_byte_indices_keep_their_packing_but_lose_the_stride() {
        // 4-bit, 3 pixels: nibbles 1, 2, 3 → 2 bytes in PDF, 4 in the file.
        let palette: Vec<u8> = (0u8..4).flat_map(|i| [i, i, i, 0]).collect();
        let file = bmp(3, 1, 4, &[vec![0x12, 0x30, 0, 0]], &palette, 0);
        let img = import(&file).unwrap();
        assert_eq!(img.bits_per_component, 4);
        assert_eq!(flate::decode(&img.data, None).unwrap(), vec![0x12, 0x30]);
    }

    #[test]
    fn compressed_and_packed_variants_are_refused_by_name() {
        for (compression, bpp, feature) in [
            (1u32, 8u16, "BMP/rle8"),
            (2, 4, "BMP/rle4"),
            (3, 32, "BMP/bitfields"),
            (4, 24, "BMP/embedded-codec"),
        ] {
            let palette = if bpp <= 8 {
                vec![0u8; 4 * 256]
            } else {
                Vec::new()
            };
            let file = bmp(1, 1, bpp, &[vec![0; 4]], &palette, compression);
            assert_eq!(
                import(&file).unwrap_err(),
                ImageImportError::Unsupported { feature },
                "compression {compression}"
            );
        }
        let file = bmp(1, 1, 16, &[vec![0; 4]], &[], 0);
        assert_eq!(
            import(&file).unwrap_err(),
            ImageImportError::Unsupported {
                feature: "BMP/16-bit"
            }
        );
    }

    #[test]
    fn an_os2_core_header_is_refused_by_name() {
        let mut file = bmp(1, 1, 24, &[vec![0, 0, 0]], &[], 0);
        file[14..18].copy_from_slice(&12u32.to_le_bytes());
        assert_eq!(
            import(&file).unwrap_err(),
            ImageImportError::Unsupported {
                feature: "BMP/core-header"
            }
        );
    }

    #[test]
    fn pels_per_metre_becomes_dpi() {
        let file = bmp(1, 1, 24, &[vec![0, 0, 0]], &[], 0);
        let img = import(&file).unwrap();
        let (dx, _) = img.dpi.expect("2835 px/m is a real resolution");
        assert!((dx - 72.0).abs() < 0.5, "{dx}");
        assert_eq!(img.notes.dpi_source, DpiSource::BmpPelsPerMeter);
    }

    #[test]
    fn a_truncated_bmp_is_corrupt_not_a_panic() {
        let file = bmp(4, 4, 24, &vec![vec![0; 12]; 4], &[], 0);
        for cut in 0..file.len() {
            assert!(
                import(file.get(..cut).unwrap()).is_err(),
                "a BMP cut at {cut} must not be accepted"
            );
        }
    }

    #[test]
    fn every_bmp_discloses_that_it_was_recompressed() {
        let file = bmp(1, 1, 24, &[vec![0, 0, 0]], &[], 0);
        assert_eq!(
            import(&file).unwrap().notes.recompressed,
            Some(RecompressReason::NoCompressedSource)
        );
    }

    fn with_offset(mut file: Vec<u8>, offset: u32) -> Vec<u8> {
        file[10..14].copy_from_slice(&offset.to_le_bytes());
        file
    }

    fn samples(file: &[u8]) -> Vec<u8> {
        flate::decode(&import(file).unwrap().data, None).unwrap()
    }

    /// Red then blue, in BGR, 24-bit: pixels at 54.
    fn red_blue() -> Vec<u8> {
        bmp(2, 1, 24, &[vec![0, 0, 255, 255, 0, 0]], &[], 0)
    }

    #[test]
    fn a_pixel_offset_inside_the_headers_is_computed_and_disclosed() {
        let intact = red_blue();
        assert_eq!(
            import(&intact).unwrap().notes.bmp_pixel_offset_repaired,
            None
        );
        for declared in [0, 14, 53] {
            let file = with_offset(intact.clone(), declared);
            assert_eq!(samples(&file), samples(&intact), "offset {declared}");
            assert_eq!(
                import(&file).unwrap().notes.bmp_pixel_offset_repaired,
                Some((declared, 54))
            );
        }
    }

    #[test]
    fn an_offset_at_the_header_end_skips_a_following_table() {
        let palette = vec![0, 0, 0, 0, 0, 0, 255, 0];
        let intact = bmp(2, 1, 8, &[vec![1, 0]], &palette, 0);
        let file = with_offset(intact.clone(), 54);
        assert_eq!(samples(&file), samples(&intact));
        assert_eq!(
            import(&file).unwrap().notes.bmp_pixel_offset_repaired,
            Some((54, 62))
        );
        // Inside the table: used as given.
        let inside = with_offset(intact, 58);
        assert_eq!(
            import(&inside).unwrap().notes.bmp_pixel_offset_repaired,
            None
        );
        // Nothing follows the header: the declared offset is already right.
        assert_eq!(
            import(&with_offset(red_blue(), 54))
                .unwrap()
                .notes
                .bmp_pixel_offset_repaired,
            None
        );
        let mut masks = header(40, 32, 3, 0);
        masks.pixel_offset = 54;
        assert_eq!(repaired_pixel_offset(&masks, 4, 1000), Some(66));
    }

    #[test]
    fn a_zero_offset_skips_the_colour_table() {
        let palette = vec![0, 0, 0, 0, 0, 0, 255, 0];
        let intact = bmp(2, 1, 8, &[vec![1, 0]], &palette, 0);
        let file = with_offset(intact.clone(), 0);
        assert_eq!(samples(&file), samples(&intact));
        assert_eq!(
            import(&file).unwrap().notes.bmp_pixel_offset_repaired,
            Some((0, 62))
        );
    }

    fn header(header_size: usize, bpp: u16, compression: u32, colours_used: u32) -> Header {
        Header {
            pixel_offset: 0,
            header_size,
            width: 1,
            height: 1,
            top_down: false,
            bpp,
            compression,
            colours_used,
        }
    }

    #[test]
    fn bit_masks_outside_a_40_byte_header_are_skipped() {
        assert_eq!(
            repaired_pixel_offset(&header(40, 32, 3, 0), 4, 1000),
            Some(66)
        );
        assert_eq!(
            repaired_pixel_offset(&header(40, 32, 6, 0), 4, 1000),
            Some(70)
        );
        // A V4 header carries its masks inside itself.
        assert_eq!(
            repaired_pixel_offset(&header(108, 32, 3, 0), 4, 1000),
            Some(122)
        );
        // A core header's palette entries are 3 bytes.
        assert_eq!(
            repaired_pixel_offset(&header(12, 8, 0, 0), 4, 1000),
            Some(26 + 768)
        );
    }

    #[test]
    fn a_computed_offset_that_does_not_fit_keeps_the_declared_one() {
        assert_eq!(repaired_pixel_offset(&header(40, 24, 0, 0), 8, 61), None);
        assert_eq!(
            repaired_pixel_offset(&header(40, 8, 0, u32::MAX), 4, 1000),
            None
        );
        // Too short for the pixels after 54: read from 0, as before.
        let mut file = with_offset(red_blue(), 0);
        file.pop();
        let img = import(&file).unwrap();
        assert_eq!(img.notes.bmp_pixel_offset_repaired, None);
        let d = &file;
        assert_eq!(
            flate::decode(&img.data, None).unwrap(),
            vec![d[2], d[1], d[0], d[5], d[4], d[3]]
        );
    }
}

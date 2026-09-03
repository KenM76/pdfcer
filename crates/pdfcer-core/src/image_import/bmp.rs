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
//! ## The two traps
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
    let corrupt = |detail: &str| ImageImportError::Corrupt {
        detail: detail.to_owned(),
    };
    let le32 = |o: usize| -> Option<u32> {
        Some(u32::from_le_bytes(data.get(o..o + 4)?.try_into().ok()?))
    };
    let le16 = |o: usize| -> Option<u16> {
        Some(u16::from_le_bytes(data.get(o..o + 2)?.try_into().ok()?))
    };

    // BITMAPFILEHEADER: "BM"(2), bfSize(4), reserved(4), bfOffBits(4).
    let Some(pixel_offset) = le32(10).and_then(|v| usize::try_from(v).ok()) else {
        return Err(corrupt("the BMP file header is truncated"));
    };
    let Some(header_size) = le32(14).and_then(|v| usize::try_from(v).ok()) else {
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
        le32(18).map(|v| v as i32),
        le32(22).map(|v| v as i32),
        le16(28),
        le32(30),
    ) else {
        return Err(corrupt("the BMP info header is truncated"));
    };

    if let Some(feature) = unsupported_compression(compression) {
        return Err(ImageImportError::Unsupported { feature });
    }
    // A negative biHeight means the rows are stored TOP-DOWN. A negative
    // width has no meaning.
    let top_down = height_raw < 0;
    let Ok(width) = u32::try_from(width_raw) else {
        return Err(corrupt("the BMP declares a negative width"));
    };
    let height = height_raw.unsigned_abs();

    let (color_space, components, out_bits) = match bpp {
        1 | 4 | 8 => {
            let entries = le32(46)
                .and_then(|v| usize::try_from(v).ok())
                .filter(|&n| n > 0)
                .unwrap_or(1usize << bpp)
                .min(1usize << bpp);
            let table = data
                .get(14 + header_size..14 + header_size + entries * 4)
                .ok_or_else(|| corrupt("the BMP colour table is truncated"))?;
            // BMP palette entries are BGR + one reserved byte; §8.6.6.3
            // wants consecutive RGB triples.
            let mut lookup = Vec::with_capacity(entries * 3);
            for e in table.chunks_exact(4) {
                // BGRX -> RGB.
                if let [b, g, r, _] = *e {
                    lookup.extend_from_slice(&[r, g, b]);
                }
            }
            let hival = u8::try_from(entries.saturating_sub(1)).unwrap_or(255);
            (
                ImportColorSpace::Indexed { hival, lookup },
                1u32,
                bpp as u32,
            )
        }
        24 | 32 => (ImportColorSpace::DeviceRgb, 3, 8),
        16 => {
            return Err(ImageImportError::Unsupported {
                feature: "BMP/16-bit",
            });
        }
        _ => return Err(corrupt("unsupported BMP bit depth")),
    };

    check_dimensions(width, height, components, out_bits)?;

    // BMP rows are padded to a 4-byte boundary; PDF rows are padded to a
    // whole byte (§8.9.3). They agree only by accident, so the payload is
    // repacked row by row rather than copied.
    let src_stride = ((width as usize * usize::from(bpp)).div_ceil(8)).next_multiple_of(4);
    let dst_stride = row_bytes(width, components, out_bits);
    let pixels = data
        .get(pixel_offset..)
        .ok_or_else(|| corrupt("the BMP pixel data starts past the end of the file"))?;
    if pixels.len() < src_stride * height as usize {
        return Err(corrupt(
            "the BMP pixel data is shorter than its header declares",
        ));
    }

    let mut out = vec![0u8; dst_stride * height as usize];
    let mut fourth_byte_seen = false;
    for y in 0..height as usize {
        // The source row for PDF row `y` (which is the TOP row when y = 0).
        let src_y = if top_down { y } else { height as usize - 1 - y };
        let src = pixels
            .get(src_y * src_stride..src_y * src_stride + src_stride)
            .ok_or_else(|| corrupt("a BMP row runs past the end of the file"))?;
        let dst = out
            .get_mut(y * dst_stride..(y + 1) * dst_stride)
            .ok_or_else(|| corrupt("row buffer overflow"))?;
        match bpp {
            24 => {
                for (x, px) in dst.chunks_exact_mut(3).enumerate() {
                    // BGR -> RGB. A short source row cannot happen (the
                    // stride check above rules it out) but is left black
                    // rather than allowed to panic.
                    if let Some(&[b, g, r]) = src
                        .get(x * 3..x * 3 + 3)
                        .and_then(|s| <&[u8; 3]>::try_from(s).ok())
                    {
                        px.copy_from_slice(&[r, g, b]);
                    }
                }
            }
            32 => {
                fourth_byte_seen = true;
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

    // biXPelsPerMeter / biYPelsPerMeter. Zero is what most writers emit
    // when they have nothing to say, and it is not a resolution.
    let dpi = match (le32(38), le32(42)) {
        (Some(x), Some(y)) if x > 0 && y > 0 => {
            Some((f64::from(x) * 0.0254, f64::from(y) * 0.0254))
        }
        _ => None,
    };

    Ok(ImportedImage {
        format: ImageFormat::Bmp,
        width,
        height,
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
            bmp_fourth_byte_ignored: fourth_byte_seen,
            dpi_source: if dpi.is_some() {
                DpiSource::BmpPelsPerMeter
            } else {
                DpiSource::Assumed
            },
            ..ImportNotes::default()
        },
    })
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
}

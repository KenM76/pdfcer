//! Bitmap records → image XObjects ([MS-EMF] §2.3.1: EMR_BITBLT 2.3.1.2,
//! EMR_STRETCHBLT 2.3.1.6, EMR_STRETCHDIBITS 2.3.1.7, EMR_ALPHABLEND
//! 2.3.1.1).
//!
//! The DIB is decoded by the image importer (wrapped as a BMP file, or a
//! JPEG/PNG payload as is). Only a copy (SRCCOPY) of a bitmap is drawn; the
//! three constant-colour ROPs with no bitmap become fills. Other raster
//! operations combine with the destination, which a PDF page cannot do.

use crate::image_import::{
    self, ImageFormat, ImportColorSpace, ImportFilter, ImportNotes, ImportedImage, Orientation,
    PdfFeature, SoftMask,
};

use super::EmfImportError;
use super::dc::Affine;
use super::draw::{Player, name, num};
use super::reader::Rec;
use super::shapes::rect;

const SRCCOPY: u32 = 0x00CC_0020;
const PATCOPY: u32 = 0x00F0_0021;
const BLACKNESS: u32 = 0x0000_0042;
const WHITENESS: u32 = 0x00FF_0062;
/// BLENDFUNCTION AlphaFormat: the source has per-pixel premultiplied alpha.
const AC_SRC_ALPHA: u8 = 0x01;

/// The fields every bitmap record shares, normalised.
struct Blit {
    dest: [f64; 4],
    src: [f64; 4],
    rop: u32,
    usage: u32,
    off_bmi: usize,
    cb_bmi: usize,
    off_bits: usize,
    cb_bits: usize,
    /// ALPHABLEND: (SourceConstantAlpha, AC_SRC_ALPHA set).
    blend: Option<(u8, bool)>,
}

impl Player {
    /// Bitmap records; `Ok(false)` when `rec` is not one.
    pub(super) fn raster_record(&mut self, rec: &Rec<'_>) -> Result<bool, EmfImportError> {
        let blit = match rec.kind {
            0x51 => stretch_dibits(rec),
            0x4C | 0x4D | 0x72 => bitblt(rec),
            _ => return Ok(false),
        };
        let label = name(rec.kind);
        let Some(b) = blit else {
            self.notes.skip(&format!("{label} (malformed)"));
            return Ok(true);
        };
        if b.cb_bmi == 0 {
            self.constant_fill(&b, &label);
            return Ok(true);
        }
        if b.blend.is_none() && b.rop != SRCCOPY {
            self.notes.skip(&format!("{label} ROP 0x{:08X}", b.rop));
            return Ok(true);
        }
        if b.usage != 0 {
            self.notes.skip(&format!("{label} with palette indices"));
            return Ok(true);
        }
        match decode(rec, &b)? {
            Some(img) => self.place(&b, img, &label),
            None => self.notes.skip(&format!("{label} (bitmap not decodable)")),
        }
        Ok(true)
    }

    /// A ROP with no source bitmap: PATCOPY fills with the brush,
    /// BLACKNESS and WHITENESS with their colour.
    fn constant_fill(&mut self, b: &Blit, label: &str) {
        let colour = match b.rop {
            PATCOPY => self.dc.brush,
            BLACKNESS => Some([0, 0, 0]),
            WHITENESS => Some([255, 255, 255]),
            rop => {
                self.notes.skip(&format!("{label} ROP 0x{rop:08X}"));
                return;
            }
        };
        let Some(c) = colour else {
            return;
        };
        let [x, y, w, h] = b.dest;
        let ops = self.ops(&rect([x, y, x + w, y + h]));
        self.sync_clip();
        self.set_fill(c);
        self.out.extend_from_slice(&ops);
        self.out.extend_from_slice(b"f\n");
    }

    fn place(&mut self, b: &Blit, img: ImportedImage, label: &str) {
        let (w, h) = (f64::from(img.width), f64::from(img.height));
        let [xd, yd, cxd, cyd] = b.dest;
        let [xs, ys, mut cxs, mut cys] = b.src;
        if cxs == 0.0 || cys == 0.0 {
            (cxs, cys) = (w, h);
        }
        let (kx, ky) = (cxd / cxs, cyd / cys);
        // Image space (row 0 at v = 1) → logical.
        let to_logical = Affine {
            a: w * kx,
            b: 0.0,
            c: 0.0,
            d: -h * ky,
            e: xd - xs * kx,
            f: yd + (h - ys) * ky,
        };
        let m = to_logical.then(&self.xf());
        let partial = xs != 0.0 || ys != 0.0 || cxs != w || cys != h;
        if let Some((sca, _)) = b.blend
            && sca < 255
            && !img.soft_mask.is_some()
        {
            self.notes
                .approximate(&format!("{label} constant alpha (drawn opaque)"));
        }
        let res = format!("Im{}", self.images.len() + 1);
        self.images.push((res.clone(), img));
        self.sync_clip();
        let mut s = b"q\n".to_vec();
        if partial {
            s.extend_from_slice(&self.ops(&rect([xd, yd, xd + cxd, yd + cyd])));
            s.extend_from_slice(b"W n\n");
        }
        for v in [m.a, m.b, m.c, m.d, m.e, m.f] {
            num(&mut s, v);
        }
        s.extend_from_slice(format!("cm\n/{res} Do\n").as_bytes());
        self.out.extend_from_slice(&s);
        self.pop_state();
    }
}

/// EMR_STRETCHDIBITS: xDest @24, yDest @28, xSrc @32, ySrc @36, cxSrc @40,
/// cySrc @44, offBmiSrc @48, cbBmiSrc @52, offBitsSrc @56, cbBitsSrc @60,
/// UsageSrc @64, BitBltRasterOperation @68, cxDest @72, cyDest @76.
fn stretch_dibits(rec: &Rec<'_>) -> Option<Blit> {
    let i = |off| rec.i32(off).map(f64::from);
    let u = |off| rec.u32(off).map(|v| v as usize);
    Some(Blit {
        dest: [i(24)?, i(28)?, i(72)?, i(76)?],
        src: [i(32)?, i(36)?, i(40)?, i(44)?],
        rop: rec.u32(68)?,
        usage: rec.u32(64)?,
        off_bmi: u(48)?,
        cb_bmi: u(52)?,
        off_bits: u(56)?,
        cb_bits: u(60)?,
        blend: None,
    })
}

/// EMR_BITBLT / EMR_STRETCHBLT / EMR_ALPHABLEND: xDest @24, yDest @28,
/// cxDest @32, cyDest @36, ROP (ALPHABLEND: BLENDFUNCTION) @40, xSrc @44,
/// ySrc @48, XformSrc @52, BkColorSrc @76, UsageSrc @80, offBmiSrc @84,
/// cbBmiSrc @88, offBitsSrc @92, cbBitsSrc @96, then (STRETCHBLT,
/// ALPHABLEND) cxSrc @100, cySrc @104. BITBLT's source size is the
/// destination size.
fn bitblt(rec: &Rec<'_>) -> Option<Blit> {
    let i = |off| rec.i32(off).map(f64::from);
    let u = |off| rec.u32(off).map(|v| v as usize);
    let (cxd, cyd) = (i(32)?, i(36)?);
    let (cxs, cys) = if rec.kind == 0x4C {
        (cxd, cyd)
    } else {
        (i(100)?, i(104)?)
    };
    let blend = (rec.kind == 0x72).then(|| {
        let f: [u8; 4] = rec.array(40).unwrap_or([0, 0, 255, 0]);
        (f[2], f[3] & AC_SRC_ALPHA != 0)
    });
    Some(Blit {
        dest: [i(24)?, i(28)?, cxd, cyd],
        src: [i(44)?, i(48)?, cxs, cys],
        rop: rec.u32(40)?,
        usage: rec.u32(80)?,
        off_bmi: u(84)?,
        cb_bmi: u(88)?,
        off_bits: u(92)?,
        cb_bits: u(96)?,
        blend,
    })
}

/// The record's DIB as an image; `Ok(None)` when it does not decode.
fn decode(rec: &Rec<'_>, b: &Blit) -> Result<Option<ImportedImage>, EmfImportError> {
    let (Some(bmi), Some(bits)) = (
        rec.bytes(b.off_bmi, b.cb_bmi),
        rec.bytes(b.off_bits, b.cb_bits),
    ) else {
        return Ok(None);
    };
    if let Some((sca, true)) = b.blend {
        return Ok(premultiplied(bmi, bits, sca));
    }
    // BitmapInfoHeader Compression @16: BI_JPEG 4 and BI_PNG 5 carry a
    // whole image file as the bits.
    let compression = le::<4>(bmi, 16).map(u32::from_le_bytes);
    if matches!(compression, Some(4 | 5)) {
        return Ok(image_import::import(bits).ok());
    }
    let header_len = 14 + bmi.len();
    let total = header_len + bits.len();
    let (Ok(total32), Ok(off32)) = (u32::try_from(total), u32::try_from(header_len)) else {
        return Ok(None);
    };
    let mut file = Vec::with_capacity(total);
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&total32.to_le_bytes());
    file.extend_from_slice(&[0; 4]);
    file.extend_from_slice(&off32.to_le_bytes());
    file.extend_from_slice(bmi);
    file.extend_from_slice(bits);
    Ok(image_import::import(&file).ok())
}

/// `N` bytes of a BitmapInfoHeader at `off`.
fn le<const N: usize>(bmi: &[u8], off: usize) -> Option<[u8; N]> {
    bmi.get(off..off.checked_add(N)?)?.try_into().ok()
}

/// A 32-bpp BI_RGB DIB of premultiplied BGRA (AC_SRC_ALPHA), un-premultiplied
/// into DeviceRGB plus a soft mask scaled by SourceConstantAlpha.
fn premultiplied(bmi: &[u8], bits: &[u8], sca: u8) -> Option<ImportedImage> {
    let rd = |o: usize| le::<4>(bmi, o).map(i32::from_le_bytes);
    let (w, h) = (rd(4)?, rd(8)?);
    let bpp = le::<2>(bmi, 14).map(u16::from_le_bytes)?;
    if bpp != 32 || rd(16)? != 0 || w <= 0 || h == 0 {
        return None;
    }
    let (width, height) = (w.unsigned_abs() as usize, h.unsigned_abs() as usize);
    let row = width * 4;
    if bits.len() < row.checked_mul(height)? {
        return None;
    }
    let mut rgb = Vec::with_capacity(width * height * 3);
    let mut alpha = Vec::with_capacity(width * height);
    for y in 0..height {
        let src = if h > 0 { height - 1 - y } else { y };
        let line = bits.get(src * row..(src + 1) * row)?;
        for px in line.as_chunks::<4>().0 {
            let a = px[3];
            for c in [px[2], px[1], px[0]] {
                let v = if a == 0 {
                    0
                } else {
                    (u32::from(c) * 255 / u32::from(a)).min(255) as u8
                };
                rgb.push(v);
            }
            alpha.push((u32::from(a) * u32::from(sca) / 255) as u8);
        }
    }
    let mut notes = ImportNotes {
        alpha_to_soft_mask: true,
        ..ImportNotes::default()
    };
    image_import::raise_version(&mut notes.requires_pdf_version, PdfFeature::SoftMask);
    Some(ImportedImage {
        format: ImageFormat::Bmp,
        width: u32::try_from(width).ok()?,
        height: u32::try_from(height).ok()?,
        bits_per_component: 8,
        color_space: ImportColorSpace::DeviceRgb,
        filter: ImportFilter::Flate,
        data: image_import::flate_encode(&rgb).ok()?,
        soft_mask: Some(SoftMask {
            width: u32::try_from(width).ok()?,
            height: u32::try_from(height).ok()?,
            bits_per_component: 8,
            data: image_import::flate_encode(&alpha).ok()?,
        }),
        color_key_mask: None,
        orientation: Orientation::Identity,
        dpi: None,
        notes,
    })
}

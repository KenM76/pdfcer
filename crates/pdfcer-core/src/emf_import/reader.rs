//! Record framing, the header, and the ceiling pre-scan that runs before any
//! drawing ([MS-EMF] §2.3: every record is `Type u32, Size u32`, `Size` a
//! multiple of 4 counting the 8 header bytes).

use super::{
    EmfImportError, MAX_BITMAP_PIXELS, MAX_INPUT_BYTES, MAX_POINTS, MAX_RECORDS, MAX_SAVE_DEPTH,
};

pub(super) const EMR_HEADER: u32 = 0x01;
pub(super) const EMR_EOF: u32 = 0x0E;
pub(super) const EMR_COMMENT: u32 = 0x46;
/// " EMF" read as a little-endian u32 ([MS-EMF] §2.2.9 Signature).
const ENHMETA_SIGNATURE: u32 = 0x464D_4520;
/// "EMF+" — EMR_COMMENT_EMFPLUS identifier ([MS-EMF] §2.3.3.2).
const EMFPLUS_IDENTIFIER: u32 = 0x2B46_4D45;
/// EmfPlusHeader record type ([MS-EMFPLUS] §2.3.3.3).
const EMFPLUS_HEADER: u16 = 0x4001;
/// EmfPlusHeader Flags bit D: the EMF records render the picture too.
const EMFPLUS_DUAL: u16 = 0x0001;

/// One record: its type and its bytes from offset 0 (the `Type` field).
#[derive(Debug, Clone, Copy)]
pub(super) struct Rec<'a> {
    pub(super) kind: u32,
    pub(super) data: &'a [u8],
}

impl<'a> Rec<'a> {
    /// `N` bytes at `off`, or `None` past the end of the record.
    pub(super) fn array<const N: usize>(&self, off: usize) -> Option<[u8; N]> {
        self.bytes(off, N)?.try_into().ok()
    }

    /// Little-endian field at `off`; `None` past the record end.
    pub(super) fn u8(&self, off: usize) -> Option<u8> {
        self.data.get(off).copied()
    }

    /// Little-endian field at `off`; `None` past the record end.
    pub(super) fn u16(&self, off: usize) -> Option<u16> {
        self.array(off).map(u16::from_le_bytes)
    }

    /// Little-endian field at `off`; `None` past the record end.
    pub(super) fn u32(&self, off: usize) -> Option<u32> {
        self.array(off).map(u32::from_le_bytes)
    }

    /// Little-endian field at `off`; `None` past the record end.
    pub(super) fn i32(&self, off: usize) -> Option<i32> {
        self.u32(off).map(|v| i32::from_le_bytes(v.to_le_bytes()))
    }

    /// Little-endian field at `off`; `None` past the record end.
    pub(super) fn i16(&self, off: usize) -> Option<i16> {
        self.array(off).map(i16::from_le_bytes)
    }

    /// Little-endian field at `off`; `None` past the record end.
    pub(super) fn f32(&self, off: usize) -> Option<f32> {
        self.u32(off).map(f32::from_bits)
    }

    /// A RectL `[left, top, right, bottom]`.
    pub(super) fn rectl(&self, off: usize) -> Option<[i32; 4]> {
        Some([
            self.i32(off)?,
            self.i32(off + 4)?,
            self.i32(off + 8)?,
            self.i32(off + 12)?,
        ])
    }

    /// `len` bytes at `off`, or `None` past the record.
    pub(super) fn bytes(&self, off: usize, len: usize) -> Option<&'a [u8]> {
        self.data.get(off..off.checked_add(len)?)
    }

    /// Point `i` of a PointL (`wide`) or PointS array starting at `off`.
    pub(super) fn point(&self, off: usize, i: usize, wide: bool) -> Option<(f64, f64)> {
        if wide {
            let at = off + i * 8;
            Some((f64::from(self.i32(at)?), f64::from(self.i32(at + 4)?)))
        } else {
            let at = off + i * 4;
            Some((f64::from(self.i16(at)?), f64::from(self.i16(at + 2)?)))
        }
    }
}

/// Iterate the records of `data`, stopping at EMR_EOF or the first record
/// whose framing is broken (the caller has already pre-scanned).
pub(super) fn records(data: &[u8]) -> impl Iterator<Item = Rec<'_>> {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        let rec = frame(data, at)?;
        at += rec.data.len();
        if rec.kind == EMR_EOF {
            at = data.len();
        }
        Some(rec)
    })
}

/// The record at `at`, when its Type/Size frame fits ([MS-EMF] §2.3: Size is
/// a multiple of 4 and at least 8).
fn frame(data: &[u8], at: usize) -> Option<Rec<'_>> {
    let head = Rec {
        kind: 0,
        data: data.get(at..)?,
    };
    let size = head.u32(4)? as usize;
    if size < 8 || !size.is_multiple_of(4) {
        return None;
    }
    Some(Rec {
        kind: head.u32(0)?,
        data: head.data.get(..size)?,
    })
}

/// The header fields the importer needs ([MS-EMF] §2.2.9).
#[derive(Debug, Clone, Copy)]
pub(super) struct Header {
    /// Inclusive-inclusive device-unit bounds.
    pub(super) bounds: [i32; 4],
    /// Inclusive-inclusive picture frame, 0.01 mm.
    pub(super) frame: [i32; 4],
    /// Millimetres per device pixel, per axis.
    pub(super) mm_per_px: (f64, f64),
}

/// What the pre-scan found.
#[derive(Debug)]
pub(super) struct Scan {
    pub(super) header: Header,
    pub(super) records: usize,
    /// An EMF+ header marked dual was found (the EMF records are a full
    /// rendering and are drawn; the EMF+ records are not).
    pub(super) emf_plus_dual: bool,
}

/// Validate framing and every ceiling before any drawing work.
pub(super) fn scan(data: &[u8]) -> Result<Scan, EmfImportError> {
    if data.len() > MAX_INPUT_BYTES {
        return Err(EmfImportError::TooLarge {
            limit: MAX_INPUT_BYTES,
        });
    }
    let header = header(data)?;
    let mut tally = Tally::default();
    let mut emf_plus_dual = false;
    let mut at = 0usize;
    while at < data.len() {
        let Some(rec) = frame(data, at) else {
            return Err(EmfImportError::Corrupt {
                detail: format!("record at byte {at} has a broken Type/Size frame"),
            });
        };
        let (kind, size) = (rec.kind, rec.data.len());
        if tally.records == 1 && kind == EMR_COMMENT {
            emf_plus_dual = emf_plus(&rec)?;
        }
        tally.count(&rec)?;
        at += size;
        if kind == EMR_EOF {
            break;
        }
    }
    Ok(Scan {
        header,
        records: tally.records,
        emf_plus_dual,
    })
}

fn header(data: &[u8]) -> Result<Header, EmfImportError> {
    let rec = frame(data, 0).ok_or(EmfImportError::NotEmf)?;
    if rec.kind != EMR_HEADER || rec.u32(40) != Some(ENHMETA_SIGNATURE) {
        return Err(EmfImportError::NotEmf);
    }
    let field = |off| rec.i32(off).ok_or(EmfImportError::NotEmf);
    let bounds = rec.rectl(8).ok_or(EmfImportError::NotEmf)?;
    let frame = rec.rectl(24).ok_or(EmfImportError::NotEmf)?;
    let device = (f64::from(field(72)?), f64::from(field(76)?));
    let mut mm = (f64::from(field(80)?), f64::from(field(84)?));
    // §2.2.9 Micrometers (header extension 2) is the finer measure when set.
    if rec.data.len() >= 108 {
        let um = (f64::from(field(100)?), f64::from(field(104)?));
        if um.0 > 0.0 && um.1 > 0.0 {
            mm = (um.0 / 1000.0, um.1 / 1000.0);
        }
    }
    if device.0 <= 0.0 || device.1 <= 0.0 || mm.0 <= 0.0 || mm.1 <= 0.0 {
        return Err(EmfImportError::Corrupt {
            detail: "the header's reference device size is zero".to_owned(),
        });
    }
    Ok(Header {
        bounds,
        frame,
        mm_per_px: (mm.0 / device.0, mm.1 / device.1),
    })
}

/// Classify the EMF+ header in the comment right after EMR_HEADER: `Ok(true)`
/// for dual mode, refusal for EMF+ only, `Ok(false)` for a plain comment.
fn emf_plus(rec: &Rec<'_>) -> Result<bool, EmfImportError> {
    // EMR_COMMENT: DataSize @8, then the data @12 ([MS-EMF] §2.3.3.1).
    if rec.u32(12) != Some(EMFPLUS_IDENTIFIER) {
        return Ok(false);
    }
    let (kind, flags) = (rec.u16(16), rec.u16(18));
    match (kind, flags) {
        (Some(EMFPLUS_HEADER), Some(f)) if f & EMFPLUS_DUAL != 0 => Ok(true),
        (Some(EMFPLUS_HEADER), Some(_)) => Err(EmfImportError::EmfPlusOnly),
        _ => Ok(false),
    }
}

#[derive(Debug, Default)]
struct Tally {
    records: usize,
    points: usize,
    depth: usize,
}

impl Tally {
    fn count(&mut self, rec: &Rec<'_>) -> Result<(), EmfImportError> {
        self.records += 1;
        if self.records > MAX_RECORDS {
            return Err(EmfImportError::TooManyRecords { limit: MAX_RECORDS });
        }
        self.points = self.points.saturating_add(points_in(rec));
        if self.points > MAX_POINTS {
            return Err(EmfImportError::TooManyPoints { limit: MAX_POINTS });
        }
        match rec.kind {
            0x21 => {
                self.depth += 1;
                if self.depth > MAX_SAVE_DEPTH {
                    return Err(EmfImportError::SaveDepth {
                        limit: MAX_SAVE_DEPTH,
                    });
                }
            }
            0x22 => {
                let n = rec.i32(8).unwrap_or(-1);
                let back = if n < 0 { n.unsigned_abs() as usize } else { 1 };
                self.depth = self.depth.saturating_sub(back);
            }
            _ => {}
        }
        if let Some(px) = bitmap_pixels(rec)
            && px > MAX_BITMAP_PIXELS
        {
            return Err(EmfImportError::BitmapTooLarge {
                limit: MAX_BITMAP_PIXELS,
            });
        }
        Ok(())
    }
}

/// The declared point count of a poly record (0 for anything else).
fn points_in(rec: &Rec<'_>) -> usize {
    match rec.kind {
        // POLYBEZIER, POLYGON, POLYLINE, POLYBEZIERTO, POLYLINETO and their
        // 16-bit forms: Count @24. POLYPOLY*: total Count @28.
        0x02..=0x06 | 0x55..=0x59 => rec.u32(24).unwrap_or(0) as usize,
        0x07 | 0x08 | 0x5A | 0x5B => rec.u32(28).unwrap_or(0) as usize,
        _ => 0,
    }
}

/// Offset of the source BitmapInfoHeader's `offBmiSrc` field, per record.
pub(super) fn bmi_field(kind: u32) -> Option<usize> {
    match kind {
        0x51 => Some(48),               // STRETCHDIBITS
        0x4C | 0x4D | 0x72 => Some(84), // BITBLT, STRETCHBLT, ALPHABLEND
        _ => None,
    }
}

/// Pixel count of a raster record's source bitmap, when it has one.
fn bitmap_pixels(rec: &Rec<'_>) -> Option<usize> {
    let off = rec.u32(bmi_field(rec.kind)?)? as usize;
    if rec.u32(bmi_field(rec.kind)? + 4)? == 0 {
        return None;
    }
    let w = rec.i32(off + 4)?.unsigned_abs() as usize;
    let h = rec.i32(off + 8)?.unsigned_abs() as usize;
    Some(w.saturating_mul(h))
}

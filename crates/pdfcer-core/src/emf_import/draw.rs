//! The record player: DC state records, painting and the clip
//! ([MS-EMF] §3.1 playback, §2.3.11 state records, §2.3.10 path brackets,
//! §2.3.2 clipping).
//!
//! Every coordinate is transformed into form space as its record is
//! played (GDI stores paths and clips in device space, so a later
//! transform change must not move them). The clip is an intersection list:
//! narrowing is emitted inline; widening (RGN_COPY, EMR_RESTOREDC) closes
//! the clip group with `Q q` and replays the list before the next paint.

use crate::fontdata::Std14;
use crate::image_import::ImportedImage;
use crate::writer::content::emit_number;

use super::dc::{Affine, ClipOp, Dc, Rgb, device_to_form};
use super::objects::Objects;
use super::reader::{Header, Rec};
use super::{EmfImportError, EmfImportNotes, ImportedEmf, MAX_CONTENT_BYTES};

/// An open BEGINPATH bracket.
#[derive(Debug, Default)]
pub(super) struct Bracket {
    pub(super) ops: Vec<u8>,
    pub(super) figure_open: bool,
}

pub(super) struct Player {
    pub(super) notes: EmfImportNotes,
    pub(super) out: Vec<u8>,
    pub(super) dc: Dc,
    saved: Vec<Dc>,
    objects: Objects,
    header: Header,
    to_form: Affine,
    pub(super) size_pt: (f64, f64),
    pub(super) bracket: Option<Bracket>,
    /// The path ENDPATH closed, waiting for FILLPATH & co.
    pub(super) path: Option<Vec<u8>>,
    pub(super) widened: bool,
    clip_dirty: bool,
    /// Last fill / stroke state emitted in this `q` level.
    fill_state: Option<Vec<u8>>,
    stroke_state: Option<Vec<u8>>,
    pub(super) images: Vec<(String, ImportedImage)>,
    pub(super) fonts: Vec<(String, Std14)>,
}

impl Player {
    /// A player for `header`, refused when the frame has no size.
    pub(super) fn new(header: &Header) -> Result<Self, EmfImportError> {
        let frame = frame_hmm(header).ok_or(EmfImportError::EmptyFrame)?;
        let k = 72.0 / 2540.0;
        let size_pt = ((frame[2] - frame[0]) * k, (frame[3] - frame[1]) * k);
        Ok(Self {
            notes: EmfImportNotes::default(),
            out: b"q\n".to_vec(),
            dc: Dc::default(),
            saved: Vec::new(),
            objects: Objects::default(),
            header: *header,
            to_form: device_to_form(header, frame, size_pt.1),
            size_pt,
            bracket: None,
            path: None,
            widened: false,
            clip_dirty: false,
            fill_state: None,
            stroke_state: None,
            images: Vec::new(),
            fonts: Vec::new(),
        })
    }

    /// Close the content and hand over the picture.
    pub(super) fn finish(mut self, records: usize) -> ImportedEmf {
        self.out.extend_from_slice(b"Q\n");
        ImportedEmf {
            content: self.out,
            size_pt: self.size_pt,
            images: self.images,
            fonts: self.fonts,
            notes: self.notes,
            records,
        }
    }

    /// Logical units → form points under the current DC.
    pub(super) fn xf(&self) -> Affine {
        self.dc
            .world
            .then(&self.dc.map.page_to_device(self.header.mm_per_px))
            .then(&self.to_form)
    }

    /// Draw one record, or note why it was not drawn.
    pub(super) fn play(&mut self, rec: &Rec<'_>) -> Result<(), EmfImportError> {
        let handled = self.state(rec)
            || self.objects.play(rec, &mut self.dc, &mut self.notes)
            || self.geometry(rec)
            || self.path_record(rec)
            || self.clip_record(rec)
            || self.text_record(rec)
            || self.raster_record(rec)?
            || silent(rec.kind);
        if !handled {
            self.notes.skip(&name(rec.kind));
        }
        if self.out.len() > MAX_CONTENT_BYTES {
            return Err(EmfImportError::TooComplex {
                limit: MAX_CONTENT_BYTES,
            });
        }
        Ok(())
    }

    /// DC state records; `false` when `rec` is not one.
    fn state(&mut self, rec: &Rec<'_>) -> bool {
        let pair = |off| -> Option<(f64, f64)> {
            Some((f64::from(rec.i32(off)?), f64::from(rec.i32(off + 4)?)))
        };
        let map = &mut self.dc.map;
        match rec.kind {
            0x09 => map.win_ext = pair(8).unwrap_or(map.win_ext),
            0x0A => map.win_org = pair(8).unwrap_or(map.win_org),
            0x0B => map.vp_ext = pair(8).unwrap_or(map.vp_ext),
            0x0C => map.vp_org = pair(8).unwrap_or(map.vp_org),
            0x1F => map.vp_ext = scale_ext(rec, map.vp_ext),
            0x20 => map.win_ext = scale_ext(rec, map.win_ext),
            0x11 => map.mode = rec.u32(8).unwrap_or(1),
            0x13 => self.dc.polyfill_winding = rec.u32(8) == Some(2),
            0x16 => self.dc.text_align = rec.u32(8).unwrap_or(0),
            0x18 => self.dc.text_color = super::dc::colorref(rec, 8).unwrap_or([0; 3]),
            0x19 => self.dc.bk_color = super::dc::colorref(rec, 8).unwrap_or([255; 3]),
            0x3A => self.dc.miter = miter(rec),
            0x14 => {
                // R2_COPYPEN is 13; any other mix is drawn as a copy.
                if rec.u32(8) != Some(13) {
                    self.notes
                        .approximate("EMR_SETROP2 mix mode (drawn as copy)");
                }
            }
            0x21 => self.saved.push(self.dc.clone()),
            0x22 => self.restore(rec.i32(8).unwrap_or(-1)),
            0x23 => {
                if let Some(m) = Affine::read(rec, 8) {
                    self.dc.world = m;
                }
            }
            0x24 => self.modify_world(rec),
            _ => return false,
        }
        true
    }

    /// EMR_MODIFYWORLDTRANSFORM (§2.3.12.1): MWT_IDENTITY 1,
    /// MWT_LEFTMULTIPLY 2 (the new transform applies first), MWT_RIGHTMULTIPLY
    /// 3, MWT_SET 4.
    fn modify_world(&mut self, rec: &Rec<'_>) {
        let Some(x) = Affine::read(rec, 8) else {
            return;
        };
        let w = self.dc.world;
        self.dc.world = match rec.u32(32) {
            Some(1) => Affine::IDENTITY,
            Some(2) => x.then(&w),
            Some(3) => w.then(&x),
            Some(4) => x,
            _ => w,
        };
    }

    /// EMR_RESTOREDC (§2.3.11.6): a negative count is relative to the top,
    /// a positive one an absolute instance number.
    fn restore(&mut self, n: i32) {
        let target = if n < 0 {
            self.saved.len().checked_sub(n.unsigned_abs() as usize)
        } else {
            (n as usize).checked_sub(1)
        };
        let Some(t) = target.filter(|&t| t < self.saved.len()) else {
            return;
        };
        self.saved.truncate(t + 1);
        let Some(dc) = self.saved.pop() else {
            return;
        };
        if dc.clip != self.dc.clip {
            self.clip_dirty = true;
        }
        self.dc = dc;
    }

    /// Paint path operators with the current brush (`fill`) and/or pen
    /// (`stroke`). A pen of no paint or a null brush drops its half.
    pub(super) fn paint(&mut self, ops: &[u8], fill: bool, stroke: bool) {
        let brush = if fill { self.dc.brush } else { None };
        let pen = if stroke { self.dc.pen.paint() } else { None };
        if brush.is_none() && pen.is_none() {
            return;
        }
        self.sync_clip();
        if let Some(c) = brush {
            self.set_fill(c);
        }
        if let Some(c) = pen {
            self.set_stroke(c);
        }
        self.out.extend_from_slice(ops);
        let eo = !self.dc.polyfill_winding;
        let op: &[u8] = match (brush.is_some(), pen.is_some(), eo) {
            (true, true, true) => b"B*\n",
            (true, true, false) => b"B\n",
            (true, false, true) => b"f*\n",
            (true, false, false) => b"f\n",
            _ => b"S\n",
        };
        self.out.extend_from_slice(op);
    }

    /// Emit `rg` for `c` unless it is already the fill colour.
    pub(super) fn set_fill(&mut self, c: Rgb) {
        let mut s = Vec::new();
        for v in c {
            num(&mut s, f64::from(v) / 255.0);
        }
        s.extend_from_slice(b"rg\n");
        if self.fill_state.as_ref() != Some(&s) {
            self.out.extend_from_slice(&s);
            self.fill_state = Some(s);
        }
    }

    fn set_stroke(&mut self, c: Rgb) {
        let s = self.stroke_ops(c);
        if self.stroke_state.as_ref() != Some(&s) {
            self.out.extend_from_slice(&s);
            self.stroke_state = Some(s);
        }
    }

    /// Emit `Q` and forget the fill and stroke state it discards.
    pub(super) fn pop_state(&mut self) {
        self.out.extend_from_slice(b"Q\n");
        self.fill_state = None;
        self.stroke_state = None;
    }

    /// Replay the clip when it widened since the last paint.
    pub(super) fn sync_clip(&mut self) {
        if !self.clip_dirty {
            return;
        }
        self.pop_state();
        self.out.extend_from_slice(b"q\n");
        for op in &self.dc.clip {
            emit_clip(&mut self.out, op);
        }
        self.clip_dirty = false;
    }

    /// Narrow the clip by `op`.
    pub(super) fn push_clip(&mut self, op: ClipOp) {
        if !self.clip_dirty {
            emit_clip(&mut self.out, &op);
        }
        self.dc.clip.push(op);
    }

    /// Replace the clip (empty = no clip).
    pub(super) fn reset_clip(&mut self) {
        self.dc.clip.clear();
        self.clip_dirty = true;
    }

    /// One reference-device pixel, in points.
    pub(super) fn px_pt(&self) -> f64 {
        self.header.mm_per_px.0 * 72.0 / 25.4
    }

    /// A box far outside the frame, for "everything except" clips.
    pub(super) fn outer_box(&self) -> Vec<u8> {
        let (w, h) = self.size_pt;
        let mut s = Vec::new();
        for v in [-10.0 * w, -10.0 * h, 21.0 * w, 21.0 * h] {
            num(&mut s, v);
        }
        s.extend_from_slice(b"re\n");
        s
    }
}

fn emit_clip(out: &mut Vec<u8>, op: &ClipOp) {
    out.extend_from_slice(&op.path);
    out.extend_from_slice(if op.even_odd { b"W* n\n" } else { b"W n\n" });
}

/// The picture frame in 0.01 mm as an exclusive rectangle (the header's
/// Frame and Bounds are inclusive-inclusive, [MS-EMF] §2.2.9), falling back
/// to the device bounds.
fn frame_hmm(h: &Header) -> Option<[f64; 4]> {
    let f = h.frame.map(f64::from);
    if f[2] >= f[0] && f[3] >= f[1] && f != [0.0; 4] {
        return Some([f[0], f[1], f[2] + 1.0, f[3] + 1.0]);
    }
    let b = h.bounds.map(f64::from);
    let (mx, my) = (h.mm_per_px.0 * 100.0, h.mm_per_px.1 * 100.0);
    let g = [b[0] * mx, b[1] * my, (b[2] + 1.0) * mx, (b[3] + 1.0) * my];
    (b[2] >= b[0] && b[3] >= b[1] && g[2] > g[0] && g[3] > g[1]).then_some(g)
}

/// EMR_SCALEVIEWPORTEXTEX / EMR_SCALEWINDOWEXTEX (§2.3.11.13): xNum, xDenom,
/// yNum, yDenom.
fn scale_ext(rec: &Rec<'_>, ext: (f64, f64)) -> (f64, f64) {
    let v = |off| rec.i32(off).map(f64::from);
    match (v(8), v(12), v(16), v(20)) {
        (Some(xn), Some(xd), Some(yn), Some(yd)) if xd != 0.0 && yd != 0.0 => {
            (ext.0 * xn / xd, ext.1 * yn / yd)
        }
        _ => ext,
    }
}

/// EMR_SETMITERLIMIT: the spec types it u32, Windows writes f32 bits
/// ([MS-EMF] Appendix A <90>). A value small as an integer is an integer.
fn miter(rec: &Rec<'_>) -> f64 {
    let v = rec.u32(8).unwrap_or(10);
    let m = if v <= 0x1_0000 {
        f64::from(v)
    } else {
        f64::from(f32::from_bits(v))
    };
    if m.is_finite() && m >= 1.0 { m } else { 10.0 }
}

/// A number operand, rounded to 4 decimals, then a space.
pub(super) fn num(out: &mut Vec<u8>, v: f64) {
    let r = (v * 10_000.0).round() / 10_000.0;
    emit_number(out, r);
    out.push(b' ');
}

/// Records that put no paint on the page: header, end, palettes, colour
/// management, comments (EMF+ is disclosed separately), brush origin,
/// stretch mode, mapper flags, meta region, arc direction (arcs are
/// skipped by name), BkMode (only used with ETO_OPAQUE, which reads
/// BkColor), UFI mapping and layout.
fn silent(kind: u32) -> bool {
    matches!(
        kind,
        0x01 | 0x0D
            | 0x0E
            | 0x10
            | 0x12
            | 0x15
            | 0x17
            | 0x1C
            | 0x30..=0x34
            | 0x39
            | 0x41
            | 0x46
            | 0x62..=0x65
            | 0x6D
            | 0x6F
            | 0x70
            | 0x71
            | 0x73
            | 0x7A
    )
}

/// The [MS-EMF] §2.1.1 name of a record type, for the notes.
pub(super) fn name(kind: u32) -> String {
    let n = match kind {
        0x0F => "EMR_SETPIXELV",
        0x1A => "EMR_OFFSETCLIPRGN",
        0x29 => "EMR_ANGLEARC",
        0x2D => "EMR_ARC",
        0x2E => "EMR_CHORD",
        0x2F => "EMR_PIE",
        0x35 => "EMR_EXTFLOODFILL",
        0x37 => "EMR_ARCTO",
        0x38 => "EMR_POLYDRAW",
        0x42 => "EMR_WIDENPATH",
        0x47 => "EMR_FILLRGN",
        0x48 => "EMR_FRAMERGN",
        0x49 => "EMR_INVERTRGN",
        0x4A => "EMR_PAINTRGN",
        0x4C => "EMR_BITBLT",
        0x4D => "EMR_STRETCHBLT",
        0x4E => "EMR_MASKBLT",
        0x4F => "EMR_PLGBLT",
        0x50 => "EMR_SETDIBITSTODEVICE",
        0x51 => "EMR_STRETCHDIBITS",
        0x53 => "EMR_EXTTEXTOUTA",
        0x54 => "EMR_EXTTEXTOUTW",
        0x5C => "EMR_POLYDRAW16",
        0x5D => "EMR_CREATEMONOBRUSH",
        0x5E => "EMR_CREATEDIBPATTERNBRUSHPT",
        0x60 => "EMR_POLYTEXTOUTA",
        0x61 => "EMR_POLYTEXTOUTW",
        0x66 => "EMR_GLSRECORD",
        0x67 => "EMR_GLSBOUNDEDRECORD",
        0x68 => "EMR_PIXELFORMAT",
        0x69 => "EMR_DRAWESCAPE",
        0x6A => "EMR_EXTESCAPE",
        0x6C => "EMR_SMALLTEXTOUT",
        0x6E => "EMR_NAMEDESCAPE",
        0x72 => "EMR_ALPHABLEND",
        0x74 => "EMR_TRANSPARENTBLT",
        0x76 => "EMR_GRADIENTFILL",
        0x77 => "EMR_SETLINKEDUFIS",
        0x78 => "EMR_SETTEXTJUSTIFICATION",
        0x79 => "EMR_COLORMATCHTOTARGETW",
        _ => return format!("EMR_0x{kind:04X}"),
    };
    n.to_owned()
}

//! EMR_EXTTEXTOUTW → real PDF text in a standard-14 substitute face
//! ([MS-EMF] §2.3.5.8, EmrText §2.2.5, LogFont [MS-WMF] §2.2.1.2).
//!
//! The EMF names a face it does not embed; the substitute is chosen by
//! family and pitch, disclosed per face. With a `Dx` array each glyph lands
//! where GDI put it (a `TJ` adjustment absorbs the width difference);
//! without one the standard-14 widths are used and that is disclosed.

use crate::fontdata::{
    BaseEncoding, Std14, encoding_glyph_name, std14_base_font_name, std14_styled, std14_width,
};
use crate::vartext::encode_winansi;

use super::dc::Font;
use super::draw::{Player, num};
use super::reader::Rec;
use super::shapes::{P, rect};

const ETO_OPAQUE: u32 = 0x0002;
const ETO_CLIPPED: u32 = 0x0004;
const ETO_GLYPH_INDEX: u32 = 0x0010;
const ETO_NO_RECT: u32 = 0x0100;
const ETO_PDY: u32 = 0x2000;
/// Typical ascent and descent of a Latin face, in em: used to move a
/// TA_TOP / TA_BOTTOM reference to the baseline.
const ASCENT: f64 = 0.905;
const DESCENT: f64 = 0.212;
/// The em height of a cell-height (positive lfHeight) font, as a fraction
/// of the cell: ascent + descent of the same typical face.
const CELL_TO_EM: f64 = 1.0 / (ASCENT + DESCENT);

/// One decoded EMR_EXTTEXTOUTW.
struct Run {
    reference: P,
    options: u32,
    rectangle: Option<[f64; 4]>,
    units: Vec<u16>,
    /// Per UTF-16 unit; empty when the record carries none.
    dx: Vec<f64>,
}

impl Player {
    /// EMR_EXTTEXTOUTW; `false` when `rec` is not one.
    pub(super) fn text_record(&mut self, rec: &Rec<'_>) -> bool {
        if rec.kind != 0x54 {
            return false;
        }
        let Some(run) = read_run(rec) else {
            self.notes.skip("EMR_EXTTEXTOUTW (malformed)");
            return true;
        };
        if run.options & ETO_GLYPH_INDEX != 0 {
            self.notes.skip("EMR_EXTTEXTOUTW with glyph indices");
            return true;
        }
        if run.options & ETO_PDY != 0 && run.dx.as_chunks::<2>().0.iter().any(|p| p[1] != 0.0) {
            self.notes
                .approximate("text with vertical Dx offsets (ignored)");
        }
        if run.options & ETO_OPAQUE != 0
            && let Some(r) = run.rectangle
        {
            let ops = self.ops(&rect(r));
            let fill = self.dc.bk_color;
            self.sync_clip();
            self.set_fill(fill);
            self.out.extend_from_slice(&ops);
            self.out.extend_from_slice(b"f\n");
        }
        if run.units.is_empty() {
            return true;
        }
        let clipped = run.options & ETO_CLIPPED != 0 && run.rectangle.is_some();
        self.sync_clip();
        if clipped {
            let ops = self.ops(&rect(run.rectangle.unwrap_or_default()));
            self.out.extend_from_slice(b"q\n");
            self.out.extend_from_slice(&ops);
            self.out.extend_from_slice(b"W n\n");
        }
        self.draw_run(&run);
        if clipped {
            self.pop_state();
        }
        true
    }

    fn draw_run(&mut self, run: &Run) {
        let font = self.dc.font.clone();
        let face = self.substitute(&font);
        let em = self.em_size(&font);
        let text = String::from_utf16_lossy(&run.units);
        let (codes, miss) = encode_winansi(&text);
        self.notes.characters_replaced += miss;
        let dx = per_char_dx(run, &text, run.options & ETO_PDY != 0);
        if dx.is_none() {
            self.notes
                .approximate("text without Dx (standard-14 widths)");
        }
        let widths: Vec<f64> = codes.iter().map(|&c| glyph_width(face, c)).collect();
        let advance: f64 = match &dx {
            Some(d) => d.iter().sum(),
            None => widths.iter().map(|w| w * em / 1000.0).sum(),
        };
        let (r, u) = self.text_axes(&font);
        let origin = self.text_origin(run, em, advance, (r, u));
        let resource = self.font_resource(face);
        self.set_fill(self.dc.text_color);
        let mut s = b"BT\n/".to_vec();
        s.extend_from_slice(resource.as_bytes());
        s.extend_from_slice(b" 1 Tf\n");
        for v in [r.0 * em, r.1 * em, u.0 * em, u.1 * em, origin.0, origin.1] {
            num(&mut s, v);
        }
        s.extend_from_slice(b"Tm\n");
        show(&mut s, &codes, &widths, dx.as_deref(), em);
        s.extend_from_slice(b"ET\n");
        self.out.extend_from_slice(&s);
        if self.dc.text_align & 1 != 0 {
            self.dc.cur.0 += advance;
        }
    }

    /// The baseline direction and the upright direction per logical unit,
    /// in form space, turned by the font's escapement.
    fn text_axes(&self, font: &Font) -> (P, P) {
        let m = self.xf();
        let r = m.vector((1.0, 0.0));
        let mut u = m.vector((0.0, 1.0));
        if r.0 * u.1 - r.1 * u.0 < 0.0 {
            u = (-u.0, -u.1);
        }
        let t = (font.escapement / 10.0).to_radians();
        let (s, c) = t.sin_cos();
        (
            (c * r.0 + s * u.0, c * r.1 + s * u.1),
            (-s * r.0 + c * u.0, -s * r.1 + c * u.1),
        )
    }

    /// The baseline start in form space, from the reference point and the
    /// text alignment (EMR_SETTEXTALIGN, [MS-WMF] §2.1.2.18).
    fn text_origin(&mut self, run: &Run, em: f64, adv: f64, axes: (P, P)) -> P {
        let (r, u) = axes;
        let align = self.dc.text_align;
        let reference = if align & 1 != 0 {
            self.dc.cur
        } else {
            run.reference
        };
        let o = self.xf().apply(reference);
        let along = match align & 6 {
            6 => -adv / 2.0,
            2 => -adv,
            _ => 0.0,
        };
        let up = match align & 0x18 {
            0x18 => 0.0,
            8 => DESCENT * em,
            _ => -ASCENT * em,
        };
        if align & 0x18 != 0x18 {
            self.notes
                .approximate("text aligned top or bottom (typical ascent/descent)");
        }
        (o.0 + r.0 * along + u.0 * up, o.1 + r.1 * along + u.1 * up)
    }

    /// The em height in logical units (LogFont Height: negative = em,
    /// positive = cell, 0 = the default 12).
    fn em_size(&mut self, font: &Font) -> f64 {
        if font.height < 0.0 {
            -font.height
        } else if font.height > 0.0 {
            self.notes
                .approximate("font cell height (em size estimated)");
            font.height * CELL_TO_EM
        } else {
            self.notes.approximate("font of default height");
            12.0
        }
    }

    /// The standard-14 face for `font`, recorded in the notes.
    fn substitute(&mut self, font: &Font) -> Std14 {
        let base = base_face(font);
        let face = std14_styled(base, font.weight >= 600, font.italic).unwrap_or(base);
        let name = std14_base_font_name(face);
        let key = if font.stock || font.face.is_empty() {
            "(stock font)".to_owned()
        } else {
            font.face.clone()
        };
        if key != name {
            self.notes.fonts_substituted.insert(key, name.to_owned());
        }
        face
    }

    fn font_resource(&mut self, face: Std14) -> String {
        if let Some((n, _)) = self.fonts.iter().find(|(_, f)| *f == face) {
            return n.clone();
        }
        let n = format!("F{}", self.fonts.len() + 1);
        self.fonts.push((n.clone(), face));
        n
    }
}

/// Fixed pitch or FF_MODERN → Courier; FF_ROMAN or a serif face name →
/// Times; anything else → Helvetica (LogFont PitchAndFamily: pitch in the
/// low 2 bits, family in the high nibble).
fn base_face(font: &Font) -> Std14 {
    let name = font.face.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if font.pitch_family & 3 == 1
        || font.pitch_family >> 4 == 3
        || has(&["courier", "consolas", "mono", "lucida console"])
    {
        Std14::Courier
    } else if font.pitch_family >> 4 == 1
        || (has(&["times", "cambria", "georgia", "garamond", "roman", "serif"])
            && !name.contains("sans serif"))
    {
        Std14::TimesRoman
    } else {
        Std14::Helvetica
    }
}

fn glyph_width(face: Std14, code: u8) -> f64 {
    encoding_glyph_name(BaseEncoding::WinAnsi, code)
        .and_then(|g| std14_width(face, g))
        .map_or(0.0, f64::from)
}

/// `Dx` per character (a character outside the BMP spans two units).
fn per_char_dx(run: &Run, text: &str, pdy: bool) -> Option<Vec<f64>> {
    if run.dx.is_empty() {
        return None;
    }
    let step = if pdy { 2 } else { 1 };
    let mut at = 0usize;
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let mut d = 0.0;
        for _ in 0..ch.len_utf16() {
            d += run.dx.get(at * step).copied().unwrap_or(0.0);
            at += 1;
        }
        out.push(d);
    }
    Some(out)
}

/// `Tj`, or `TJ` with one adjustment per glyph that moves it by `dx`.
fn show(s: &mut Vec<u8>, codes: &[u8], widths: &[f64], dx: Option<&[f64]>, em: f64) {
    let Some(dx) = dx.filter(|_| em > 0.0) else {
        crate::writer::content::emit_literal_string(s, codes);
        s.extend_from_slice(b" Tj\n");
        return;
    };
    s.push(b'[');
    for (i, (&c, &w)) in codes.iter().zip(widths).enumerate() {
        crate::writer::content::emit_literal_string(s, &[c]);
        let adj = w - dx.get(i).copied().unwrap_or(0.0) * 1000.0 / em;
        if adj.abs() > 1e-4 {
            s.push(b' ');
            num(s, adj);
        }
    }
    s.extend_from_slice(b"] TJ\n");
}

/// EMR_EXTTEXTOUTW: EmrText @36 — Reference @36, Chars @44, offString @48,
/// Options @52, Rectangle @56 unless ETO_NO_RECT, then offDx.
fn read_run(rec: &Rec<'_>) -> Option<Run> {
    let reference = rec.point(36, 0, true)?;
    let n = rec.u32(44)? as usize;
    let off_string = rec.u32(48)? as usize;
    let options = rec.u32(52)?;
    let (rectangle, dx_at) = if options & ETO_NO_RECT != 0 {
        (None, 56)
    } else {
        (Some(rec.rectl(56)?.map(f64::from)), 72)
    };
    let raw = rec.bytes(off_string, n.checked_mul(2)?)?;
    let units = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| u16::from_le_bytes(b))
        .collect();
    let off_dx = rec.u32(dx_at)? as usize;
    let per = if options & ETO_PDY != 0 { 2 } else { 1 };
    let dx = if off_dx == 0 {
        Vec::new()
    } else {
        let bytes = rec.bytes(off_dx, n.checked_mul(4 * per)?)?;
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&b| f64::from(i32::from_le_bytes(b)))
            .collect()
    };
    let rectangle = rectangle.filter(|r| r[2] > r[0] && r[3] > r[1]);
    Some(Run {
        reference,
        options,
        rectangle,
        units,
        dx,
    })
}

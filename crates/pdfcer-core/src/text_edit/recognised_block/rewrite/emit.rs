//! One rewritten show operator: kept codes at their original positions,
//! each replacement at its old word's origin under its fitted `Tz`, and a
//! final move so the text matrix ends where the original left it (§9.4.4).

use crate::text_edit::BlockEditError;
use crate::text_edit::cause::UnsupportedCause;
use crate::text_edit::edit::block_encode::BlockEncoding;
use crate::text_edit::edit::{ShowData, ShowElem, glyph_advance_with, resolve_font};
use crate::text_edit::reflow_style::code_bytes;
use crate::text_extract::font::ExtractFont;
use crate::writer::content::emit_number;

use super::{Act, Ctx, Rep, not_found};

/// `codes`' advance in `enc`'s font under `s`'s size, `Tc` and `Tw`, at a
/// horizontal scaling of 100%.
pub(super) fn natural_width(s: &ShowData, enc: &BlockEncoding, codes: &[u32]) -> f64 {
    advance(&enc.font, s, codes, 1.0)
}

fn advance(font: &ExtractFont, s: &ShowData, codes: &[u32], th: f64) -> f64 {
    let single = font.bytes_per_code() == 1;
    codes
        .iter()
        .map(|&c| glyph_advance_with(font, c, s.tf_size, s.tc(), s.tw(), th, single))
        .sum()
}

/// The bytes replacing operator `s`.
///
/// # Errors
///
/// The operator's font cannot be resolved, or an action names a missing
/// replacement.
pub(super) fn op(
    ctx: &Ctx<'_, '_>,
    s: &ShowData,
    acts: &[Act],
    reps: &[Rep],
    encs: &[BlockEncoding],
) -> Result<Vec<u8>, BlockEditError> {
    let font = resolve_font(ctx.doc, &ctx.page.resources, &s.font_name).ok_or(
        BlockEditError::unsupported(UnsupportedCause::FontUnresolvable),
    )?;
    let (starts, advs, end) = replay(&font, s);
    let mut e = Emitter::new(s);
    for (k, slot) in s.slots.iter().enumerate() {
        let (Some(&start), Some(&adv)) = (starts.get(k), advs.get(k)) else {
            continue;
        };
        match acts.get(k).copied().unwrap_or(Act::Keep) {
            Act::Keep => {
                let Some(ShowElem::Str(b)) = s.elems.get(slot.elem) else {
                    continue;
                };
                let bytes = b.get(slot.byte_in_elem..slot.byte_in_elem + usize::from(slot.width));
                e.move_to(start);
                e.code(bytes.unwrap_or_default());
                e.pen += adv;
            }
            Act::Skip => {}
            Act::Insert(r) => {
                let rep = reps.get(r).ok_or(not_found())?;
                let enc = encs.get(rep.enc).ok_or(not_found())?;
                e.insert(s, rep, enc, start);
            }
        }
    }
    e.move_to(end);
    Ok(e.finish())
}

/// Each slot's start along the text line, its advance, and the operator's
/// total displacement, in text space after scaling (§9.4.4).
pub(super) fn replay(font: &ExtractFont, s: &ShowData) -> (Vec<f64>, Vec<f64>, f64) {
    let (tfs, th) = (s.tf_size, s.th());
    let mut starts = Vec::with_capacity(s.slots.len());
    let mut advs = Vec::with_capacity(s.slots.len());
    let mut u = 0.0;
    let mut k = 0;
    for (ei, el) in s.elems.iter().enumerate() {
        match el {
            ShowElem::Num(n) => u -= n / 1000.0 * tfs * th,
            ShowElem::Str(_) => {
                while let Some(slot) = s.slots.get(k).filter(|sl| sl.elem == ei) {
                    let a = glyph_advance_with(
                        font,
                        slot.code,
                        tfs,
                        s.tc(),
                        s.tw(),
                        th,
                        slot.width == 1,
                    );
                    starts.push(u);
                    advs.push(a);
                    u += a;
                    k += 1;
                }
            }
        }
    }
    (starts, advs, u)
}

/// Writes `[…] TJ` segments, tracking where the text matrix is.
struct Emitter {
    out: Vec<u8>,
    seg: Vec<u8>,
    /// Displacement from the operator's start, text space after scaling.
    pen: f64,
    tfs: f64,
    th: f64,
}

impl Emitter {
    fn new(s: &ShowData) -> Self {
        Self {
            out: Vec::new(),
            seg: Vec::new(),
            pen: 0.0,
            tfs: s.tf_size,
            th: s.th(),
        }
    }

    /// A `TJ` number moving the pen to `t`.
    fn move_to(&mut self, t: f64) {
        let scale = self.tfs * self.th;
        let d = t - self.pen;
        if d.abs() < 1e-9 || scale.abs() < f64::EPSILON {
            return;
        }
        if !self.seg.is_empty() {
            self.seg.push(b' ');
        }
        emit_number(&mut self.seg, -d * 1000.0 / scale);
        self.pen = t;
    }

    fn code(&mut self, bytes: &[u8]) {
        self.seg.push(b'<');
        for b in bytes {
            self.seg.extend_from_slice(format!("{b:02X}").as_bytes());
        }
        self.seg.push(b'>');
    }

    fn flush(&mut self) {
        if self.seg.is_empty() {
            return;
        }
        self.out.push(b'[');
        self.out.append(&mut self.seg);
        self.out.extend_from_slice(b"] TJ\n");
    }

    /// `rep` at `start`, under its own `Tz` (and font, when the encoder
    /// chose a sibling), then the operator's own `Tz` and font back.
    fn insert(&mut self, s: &ShowData, rep: &Rep, enc: &BlockEncoding, start: f64) {
        self.flush();
        let other_font = enc.font_resource != s.font_name;
        self.scaling(rep.th);
        if other_font {
            self.tf(&enc.font_resource);
        }
        self.move_to(start);
        for &c in &rep.codes {
            self.code(&code_bytes(c, enc.font.bytes_per_code()));
        }
        self.pen += advance(&enc.font, s, &rep.codes, rep.th);
        self.flush();
        self.scaling(s.th());
        if other_font {
            self.tf(&s.font_name);
        }
    }

    fn scaling(&mut self, th: f64) {
        emit_number(&mut self.out, th * 100.0);
        self.out.extend_from_slice(b" Tz\n");
        self.th = th;
    }

    fn tf(&mut self, name: &[u8]) {
        self.out.push(b'/');
        self.out.extend_from_slice(name);
        self.out.push(b' ');
        emit_number(&mut self.out, self.tfs);
        self.out.extend_from_slice(b" Tf\n");
    }

    /// The operator's replacement, led by a space so its first token never
    /// runs into the operator before it.
    fn finish(mut self) -> Vec<u8> {
        self.flush();
        if !self.out.is_empty() {
            self.out.insert(0, b' ');
        }
        self.out
    }
}

//! Convert a Type 1 font program to a bare CFF program, so SVG export can
//! keep text drawn from a Type 1 font as `<text>` (through [`super::webfont`]).
//!
//! The outlines come from skrifa's Type 1 interpreter (the same one the
//! renderer draws with) and are re-encoded as Type 2 charstrings
//! (`rmoveto`, `rlineto`, `rrcurveto`, `endchar`; Adobe TN5177). Hints are
//! dropped. Glyph ids are kept: glyph `n` of the CFF is glyph `n` of the
//! Type 1 font, and skrifa always puts `.notdef` at 0, as CFF requires.
//! Only the glyphs in `used` (and `.notdef`) get outlines; every other
//! charstring is a bare `endchar`, since the caller subsets straight after.
//!
//! Layout (Adobe TN5176): header, Name INDEX, Top DICT INDEX, String INDEX,
//! an empty Global Subr INDEX, a format-0 charset, the CharStrings INDEX and
//! an empty Private DICT (default and nominal widths 0). Top DICT offsets
//! are always written as 5-byte integers, so one layout pass is enough.
//!
//! Refused (`None`): a font whose em is not 1000 units or whose
//! `FontMatrix` is not a plain scale — the CFF written here uses the default
//! matrix — and a glyph that does not evaluate.

use skrifa::GlyphId;
use skrifa::outline::pen::OutlinePen;
use skrifa::raw::ps::transform::FontMatrix;
use skrifa::raw::ps::type1::Type1Font;

/// First string id available to a font's own strings (TN5176 appendix A).
const FIRST_CUSTOM_SID: usize = 391;

/// Build a bare CFF program from `font`, with outlines for `.notdef` and
/// every glyph in `used`.
pub(crate) fn convert(font: &Type1Font, used: impl IntoIterator<Item = u16>) -> Option<Vec<u8>> {
    if font.upem() != 1000 || font.matrix() != FontMatrix::IDENTITY {
        return None;
    }
    let count = u16::try_from(font.num_glyphs()).ok().filter(|&n| n > 0)?;
    let mut charstrings = vec![vec![14u8]; usize::from(count)];
    for gid in std::iter::once(0).chain(used) {
        let slot = charstrings.get_mut(usize::from(gid))?;
        *slot = charstring(font, gid)?;
    }

    let names: Vec<&str> = (1..count)
        .map(|g| font.glyph_name(GlyphId::new(g.into())).unwrap_or(".notdef"))
        .collect();
    let font_name = font
        .name()
        .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_graphic()))
        .unwrap_or("Type1Font");

    let name_index = index(&[font_name.as_bytes()]);
    let string_index = index(&names.iter().map(|n| n.as_bytes()).collect::<Vec<_>>());
    let global_subrs = index(&[]);
    let mut charset = vec![0u8];
    for i in 0..names.len() {
        let sid = u16::try_from(FIRST_CUSTOM_SID + i).ok()?;
        charset.extend_from_slice(&sid.to_be_bytes());
    }
    let charstrings_index = index(&charstrings.iter().map(Vec::as_slice).collect::<Vec<_>>());

    // The Top DICT's size does not depend on the offsets it carries.
    let top_dict_len = top_dict(0, 0, 0).len();
    let top_index_len = index(&[&vec![0; top_dict_len]]).len();
    let charset_at = 4 + name_index.len() + top_index_len + string_index.len() + global_subrs.len();
    let charstrings_at = charset_at + charset.len();
    let private_at = charstrings_at + charstrings_index.len();
    let top = top_dict(
        i32::try_from(charset_at).ok()?,
        i32::try_from(charstrings_at).ok()?,
        i32::try_from(private_at).ok()?,
    );

    let mut out = vec![1, 0, 4, 4];
    out.extend_from_slice(&name_index);
    out.extend_from_slice(&index(&[&top]));
    out.extend_from_slice(&string_index);
    out.extend_from_slice(&global_subrs);
    out.extend_from_slice(&charset);
    out.extend_from_slice(&charstrings_index);
    Some(out)
}

/// The Top DICT: `charset`, `CharStrings`, and an empty `Private` at
/// `private` (size 0, then offset).
fn top_dict(charset: i32, charstrings: i32, private: i32) -> Vec<u8> {
    let mut d = Vec::new();
    for (operands, op) in [
        (&[charset][..], 15u8),
        (&[charstrings][..], 17),
        (&[0, private][..], 18),
    ] {
        for &v in operands {
            d.push(0x1d);
            d.extend_from_slice(&v.to_be_bytes());
        }
        d.push(op);
    }
    d
}

/// A CFF INDEX: count, offset size, 1-based offsets, data. Empty is `00 00`.
fn index(items: &[&[u8]]) -> Vec<u8> {
    if items.is_empty() {
        return vec![0, 0];
    }
    let total: usize = items.iter().map(|i| i.len()).sum::<usize>() + 1;
    let off_size: u8 = match total {
        0..=0xFF => 1,
        0x100..=0xFFFF => 2,
        0x1_0000..=0xFF_FFFF => 3,
        _ => 4,
    };
    let mut out = (items.len() as u16).to_be_bytes().to_vec();
    out.push(off_size);
    let mut offset = 1usize;
    let push = |out: &mut Vec<u8>, v: usize| {
        out.extend_from_slice(&(v as u32).to_be_bytes()[4 - usize::from(off_size)..]);
    };
    push(&mut out, offset);
    for item in items {
        offset += item.len();
        push(&mut out, offset);
    }
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// Glyph `gid` as a Type 2 charstring.
fn charstring(font: &Type1Font, gid: u16) -> Option<Vec<u8>> {
    let mut pen = CharstringPen::default();
    let width = font.draw(GlyphId::new(gid.into()), None, &mut pen).ok()?;
    let width = fixed(width.unwrap_or(0.0));
    let mut out = Vec::new();
    let mut ops = pen.ops.into_iter();
    // The advance is an extra first operand of the first stack-clearing
    // operator when it differs from nominalWidthX (0).
    match ops.next() {
        Some((operands, op)) => {
            if width != 0 {
                number(&mut out, width);
            }
            for v in operands {
                number(&mut out, v);
            }
            out.push(op);
        }
        None => {
            if width != 0 {
                number(&mut out, width);
            }
        }
    }
    for (operands, op) in ops {
        for v in operands {
            number(&mut out, v);
        }
        out.push(op);
    }
    out.push(14);
    Some(out)
}

/// `v` in 16.16 fixed point.
fn fixed(v: f32) -> i32 {
    (f64::from(v) * 65536.0).round() as i32
}

/// Append a charstring number: an integer form when `v` is whole and fits
/// 16 bits, otherwise `ff` and the 16.16 value.
fn number(out: &mut Vec<u8>, v: i32) {
    if v & 0xFFFF == 0 {
        let n = v >> 16;
        match n {
            -107..=107 => out.push((n + 139) as u8),
            108..=1131 => {
                let n = n - 108;
                out.extend_from_slice(&[(n / 256 + 247) as u8, (n % 256) as u8]);
            }
            -1131..=-108 => {
                let n = -n - 108;
                out.extend_from_slice(&[(n / 256 + 251) as u8, (n % 256) as u8]);
            }
            _ => {
                out.push(28);
                out.extend_from_slice(&(n as i16).to_be_bytes());
            }
        }
    } else {
        out.push(0xff);
        out.extend_from_slice(&v.to_be_bytes());
    }
}

/// Records drawing commands as relative Type 2 operators, tracking the
/// current point in 16.16 so rounding never accumulates.
#[derive(Default)]
struct CharstringPen {
    ops: Vec<(Vec<i32>, u8)>,
    at: (i32, i32),
    open: bool,
}

impl CharstringPen {
    fn rel(&mut self, x: f32, y: f32) -> [i32; 2] {
        let (x, y) = (fixed(x), fixed(y));
        let d = [x.wrapping_sub(self.at.0), y.wrapping_sub(self.at.1)];
        self.at = (x, y);
        d
    }

    /// A drawing operator needs a preceding moveto.
    fn ensure_open(&mut self) {
        if !self.open {
            self.ops.push((vec![0, 0], 21));
            self.open = true;
        }
    }
}

impl OutlinePen for CharstringPen {
    fn move_to(&mut self, x: f32, y: f32) {
        let d = self.rel(x, y);
        self.ops.push((d.to_vec(), 21));
        self.open = true;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.ensure_open();
        let d = self.rel(x, y);
        self.ops.push((d.to_vec(), 5));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        // Degree elevation; the absolute start is the tracked point.
        let (sx, sy) = (self.at.0 as f32 / 65536.0, self.at.1 as f32 / 65536.0);
        let c1 = (sx + 2.0 / 3.0 * (cx0 - sx), sy + 2.0 / 3.0 * (cy0 - sy));
        let c2 = (x + 2.0 / 3.0 * (cx0 - x), y + 2.0 / 3.0 * (cy0 - y));
        self.curve_to(c1.0, c1.1, c2.0, c2.1, x, y);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.ensure_open();
        let a = self.rel(cx0, cy0);
        let b = self.rel(cx1, cy1);
        let c = self.rel(x, y);
        self.ops.push((vec![a[0], a[1], b[0], b[1], c[0], c[1]], 8));
    }
    fn close(&mut self) {
        // Type 2 closes every contour implicitly.
        self.open = false;
    }
}

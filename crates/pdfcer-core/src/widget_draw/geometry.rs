//! Where a text or choice field's characters land on the page, for an
//! editor's caret and selection.
//!
//! The layout is the one [`WidgetAppearances::appearance`] draws (§12.7.3.3
//! variable text; §12.7.4.3 Table 228 for Multiline, Password, Comb and
//! `/MaxLen`; §12.5.6.19 Table 189 `/MK /R` for rotation), so a caret placed
//! from it sits on the glyphs a reader paints from the same field.

use std::borrow::Cow;

use super::{WidgetAppearances, comb_cells, displayed_text};
use crate::edit::EditSession;
use crate::forms::{Field, FieldFlags, FieldType};
use crate::object::ObjId;
use crate::page_tree::Rect;
use crate::vartext::{self, CharSpan, TextLayout};

/// A point in page space (PDF user space, y up).
pub type PagePoint = [f64; 2];

/// One laid-out line of a field's text.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FieldTextLine {
    /// The line's box, descent to ascent, from the left of its first drawn
    /// glyph to the right of its last, in page space. Corners in the order
    /// lower-left, lower-right, upper-right, upper-left of the unrotated
    /// field, so a rotated field gives a rotated quadrilateral.
    pub corners: [PagePoint; 4],
    /// Byte offset of the line's first character in the input string.
    pub start: usize,
    /// Byte offset one past its last character.
    pub end: usize,
}

/// One character of the input string.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FieldTextChar {
    /// Byte offset of the character in the input string.
    pub offset: usize,
    /// Byte offset one past it.
    pub end: usize,
    /// Index into [`FieldTextGeometry::lines`].
    pub line: usize,
    /// Its advance's start, on the baseline, in page space.
    pub start: PagePoint,
    /// Its advance's end, on the baseline, in page space. Equal to `start`
    /// for a character that is not drawn (a collapsed space, a line break,
    /// a comb character past `/MaxLen`).
    pub stop: PagePoint,
}

/// A caret position: a string offset and the segment it is drawn as.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FieldCaret {
    /// Byte offset in the input string.
    pub offset: usize,
    /// Index into [`FieldTextGeometry::lines`].
    pub line: usize,
    /// The caret's foot, at the descent, in page space.
    pub bottom: PagePoint,
    /// The caret's head, at the ascent, in page space.
    pub top: PagePoint,
}

/// A field's text laid out as its appearance draws it.
///
/// Built by [`WidgetAppearances::text_geometry`]. Offsets are UTF-8 byte
/// offsets into the string passed there, never into a masked or flattened
/// copy of it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FieldTextGeometry {
    /// The point size the text is drawn at, after any auto-size.
    pub font_size: f64,
    /// Lines, top to bottom. Never empty.
    pub lines: Vec<FieldTextLine>,
    /// Every character of the input, in string order.
    pub chars: Vec<FieldTextChar>,
    ascent: f64,
    descent: f64,
    baselines: Vec<f64>,
    local: Vec<Vec<CharSpan>>,
    line_x: Vec<f64>,
    to_page: [f64; 6],
}

impl WidgetAppearances {
    /// The field that owns widget `widget_id`, if the form has it.
    #[must_use]
    pub fn field(&self, widget_id: ObjId) -> Option<&Field> {
        let &(fi, _) = self.index.get(&widget_id)?;
        self.fields.get(fi)
    }

    /// Lay `text` out in widget `widget_id` exactly as its appearance would
    /// draw it, and say where every character lands on the page.
    ///
    /// `text` is any string, not necessarily the field's value: an editor
    /// passes what the operator is typing. The field's `/DA` font and size
    /// (auto size included), `/Q`, Multiline, Comb with `/MaxLen`, Password
    /// masking and `/MK /R` all apply. A character the font cannot encode is
    /// measured as `?`, as it is drawn.
    ///
    /// `None` for a widget not in the form, one with no `/Rect`, a field
    /// that is not a text or choice field, or a `/DA` the layout refuses
    /// (the same refusals as drawing it).
    #[must_use]
    pub fn text_geometry(&self, widget_id: ObjId, text: &str) -> Option<FieldTextGeometry> {
        let &(fi, wi) = self.index.get(&widget_id)?;
        let field = self.fields.get(fi)?;
        let widget = field.widgets.get(wi)?;
        let rect = widget.rect?;
        let multiline = match field.field_type? {
            FieldType::Text => field.flags.has(FieldFlags::MULTILINE),
            FieldType::Choice => !field.flags.has(FieldFlags::COMBO),
            FieldType::Button | FieldType::Signature => return None,
        };
        let quarter = widget.rotation.unwrap_or(0).rem_euclid(360);
        let (w, h) = if quarter == 90 || quarter == 270 {
            (rect.height(), rect.width())
        } else {
            (rect.width(), rect.height())
        };
        let bbox = Rect {
            llx: 0.0,
            lly: 0.0,
            urx: w.max(1.0),
            ury: h.max(1.0),
        };
        let da = field
            .default_appearance
            .as_deref()
            .unwrap_or(&self.default_da);
        let shown = displayed_text(field, text);
        let laid = match comb_cells(field, multiline) {
            Some(cells) => vartext::layout_comb_text(bbox, &shown, da, cells, &self.fonts),
            None => vartext::layout_variable_text(
                bbox,
                &shown,
                da,
                field.quadding,
                multiline,
                &self.fonts,
            ),
        }
        .ok()?;
        let laid = match shown {
            Cow::Owned(_) => unmask(laid, text),
            Cow::Borrowed(_) => laid,
        };
        let turn =
            EditSession::quarter_turn_matrix(quarter).unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        Some(FieldTextGeometry::new(laid, place(turn, bbox, rect)))
    }
}

/// Map a password field's masked layout (one `*` per non-line-break
/// character) back onto the offsets of the string the caller passed.
fn unmask(mut laid: TextLayout, text: &str) -> TextLayout {
    let originals: Vec<(usize, usize)> = text
        .char_indices()
        .map(|(i, c)| (i, i + c.len_utf8()))
        .collect();
    let mut src = originals.iter().peekable();
    for line in &mut laid.lines {
        let mut mapped = Vec::new();
        for c in &line.chars {
            // A stripped line break gets a zero-width span where it fell.
            while let Some(&&(o, e)) = src.peek() {
                if matches!(text.get(o..e), Some("\r" | "\n")) {
                    mapped.push(CharSpan::new(o, e, c.x0, c.x0));
                    src.next();
                } else {
                    break;
                }
            }
            if let Some(&(o, e)) = src.next() {
                mapped.push(CharSpan::new(o, e, c.x0, c.x1));
            }
        }
        line.chars = mapped;
    }
    if let Some(last) = laid.lines.last_mut() {
        let x = last.chars.last().map_or(last.x1, |c| c.x1);
        for &(o, e) in src {
            last.chars.push(CharSpan::new(o, e, x, x));
        }
    }
    for line in &mut laid.lines {
        line.start = line.chars.first().map_or(line.start, |c| c.offset);
        line.end = line.chars.last().map_or(line.end, |c| c.end);
    }
    if laid.lines.iter().all(|l| l.chars.is_empty()) {
        for line in &mut laid.lines {
            line.start = 0;
            line.end = 0;
        }
    }
    laid
}

/// Local appearance space to page space: the `/MK /R` quarter turn as the
/// form's `/Matrix`, then §12.5.5's fit of the transformed `/BBox` to the
/// `/Rect`.
fn place(turn: [f64; 6], bbox: Rect, rect: Rect) -> [f64; 6] {
    let corners = [
        (bbox.llx, bbox.lly),
        (bbox.urx, bbox.lly),
        (bbox.urx, bbox.ury),
        (bbox.llx, bbox.ury),
    ]
    .map(|(x, y)| apply(turn, [x, y]));
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for [x, y] in corners {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let sx = if x1 > x0 {
        rect.width() / (x1 - x0)
    } else {
        1.0
    };
    let sy = if y1 > y0 {
        rect.height() / (y1 - y0)
    } else {
        1.0
    };
    let fit = [sx, 0.0, 0.0, sy, rect.llx - x0 * sx, rect.lly - y0 * sy];
    concat(turn, fit)
}

/// `a` then `b`, as PDF matrices compose (§8.3.4).
fn concat(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn apply(m: [f64; 6], [x, y]: PagePoint) -> PagePoint {
    [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]]
}

fn invert(m: [f64; 6]) -> Option<[f64; 6]> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-12 {
        return None;
    }
    let (a, b, c, d) = (m[3] / det, -m[1] / det, -m[2] / det, m[0] / det);
    Some([a, b, c, d, -(m[4] * a + m[5] * c), -(m[4] * b + m[5] * d)])
}

impl FieldTextGeometry {
    fn new(laid: TextLayout, to_page: [f64; 6]) -> Self {
        let p = |x: f64, y: f64| apply(to_page, [x, y]);
        let (asc, desc) = (laid.ascent, laid.descent);
        let mut lines = Vec::new();
        let mut chars = Vec::new();
        for (i, l) in laid.lines.iter().enumerate() {
            let (lo, hi) = (l.baseline - desc, l.baseline + asc);
            lines.push(FieldTextLine {
                corners: [p(l.x0, lo), p(l.x1, lo), p(l.x1, hi), p(l.x0, hi)],
                start: l.start,
                end: l.end,
            });
            chars.extend(l.chars.iter().map(|c| FieldTextChar {
                offset: c.offset,
                end: c.end,
                line: i,
                start: p(c.x0, l.baseline),
                stop: p(c.x1, l.baseline),
            }));
        }
        Self {
            font_size: laid.font_size,
            lines,
            chars,
            ascent: asc,
            descent: desc,
            baselines: laid.lines.iter().map(|l| l.baseline).collect(),
            line_x: laid.lines.iter().map(|l| l.x0).collect(),
            local: laid.lines.into_iter().map(|l| l.chars).collect(),
            to_page,
        }
    }

    /// Every caret position, line by line: before each character, and after
    /// the last character of each line. An offset at a soft wrap appears
    /// twice, at the end of one line and the start of the next.
    #[must_use]
    pub fn carets(&self) -> Vec<FieldCaret> {
        let mut out = Vec::new();
        for (line, spans) in self.local.iter().enumerate() {
            for c in spans {
                out.push(self.caret(line, c.x0, c.offset));
            }
            match spans.last() {
                Some(c) => out.push(self.caret(line, c.x1, c.end)),
                None => {
                    let x = self.line_x.get(line).copied().unwrap_or(0.0);
                    let start = self.lines.get(line).map_or(0, |l| l.start);
                    out.push(self.caret(line, x, start));
                }
            }
        }
        out
    }

    /// The caret segment for `offset`: before the character that starts
    /// there (so an offset at a soft wrap goes to the start of the next
    /// line), else after the character that ends there. `None` for an
    /// offset past the end or inside a multi-byte character.
    #[must_use]
    pub fn caret_segment(&self, offset: usize) -> Option<[PagePoint; 2]> {
        let found = self
            .local
            .iter()
            .enumerate()
            .find_map(|(l, s)| s.iter().find(|c| c.offset == offset).map(|c| (l, c.x0)))
            .or_else(|| {
                self.local
                    .iter()
                    .enumerate()
                    .find_map(|(l, s)| s.iter().find(|c| c.end == offset).map(|c| (l, c.x1)))
            });
        let (line, x) = match found {
            Some(hit) => hit,
            None if offset == 0 && self.chars.is_empty() => {
                (0, self.line_x.first().copied().unwrap_or(0.0))
            }
            None => return None,
        };
        let c = self.caret(line, x, offset);
        Some([c.bottom, c.top])
    }

    /// The caret offset nearest page point `(x, y)`: the nearest line by
    /// height, then the nearest caret on it.
    #[must_use]
    pub fn caret_at_point(&self, x: f64, y: f64) -> usize {
        let Some(inv) = invert(self.to_page) else {
            return 0;
        };
        let [lx, ly] = apply(inv, [x, y]);
        let line = (0..self.baselines.len())
            .min_by(|&a, &b| {
                self.band_distance(a, ly)
                    .total_cmp(&self.band_distance(b, ly))
            })
            .unwrap_or(0);
        self.carets()
            .into_iter()
            .filter(|c| c.line == line)
            .min_by(|a, b| {
                let da = (self.local_x(a) - lx).abs();
                let db = (self.local_x(b) - lx).abs();
                da.total_cmp(&db)
            })
            .map_or(0, |c| c.offset)
    }

    fn band_distance(&self, line: usize, ly: f64) -> f64 {
        let base = self.baselines.get(line).copied().unwrap_or(0.0);
        let (lo, hi) = (base - self.descent, base + self.ascent);
        if ly < lo {
            lo - ly
        } else if ly > hi {
            ly - hi
        } else {
            0.0
        }
    }

    fn local_x(&self, c: &FieldCaret) -> f64 {
        invert(self.to_page).map_or(0.0, |inv| apply(inv, c.bottom)[0])
    }

    fn caret(&self, line: usize, x: f64, offset: usize) -> FieldCaret {
        let base = self.baselines.get(line).copied().unwrap_or(0.0);
        FieldCaret {
            offset,
            line,
            bottom: apply(self.to_page, [x, base - self.descent]),
            top: apply(self.to_page, [x, base + self.ascent]),
        }
    }
}

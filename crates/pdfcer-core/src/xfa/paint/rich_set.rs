//! Setting a [`Rich`] value: lines broken at spaces, tabs advanced to
//! their stops, each run in its own face. Rich-text layout is the
//! processor's choice; where a style cannot be honoured all the text is
//! still shown (Ch.5 p.222; slice `adobe_ext__xfa_rich_text.md` §6).

use std::cell::Cell;

use super::rich::{Base, Para, Piece, Rich, RunStyle, read, rich_value};
use super::text::{metrics, plain_text};
use super::{FontWeight, LayoutItem, LayoutRect, PaintNoteKind, Painter, XfaNode};
use crate::vartext::{encode_winansi, text_width};
use crate::writer::content::Paint;

/// One run placed on a line, `x` from the line's left edge.
struct Placed {
    x: f64,
    text: String,
    style: RunStyle,
}

/// One set line.
struct Line {
    pieces: Vec<Placed>,
    /// Left edge from the region's left: margin plus any first-line indent.
    left: f64,
    width: f64,
    ascent: f64,
    height: f64,
    /// The free width alignment shares out; 0 when unwrapped.
    free: f64,
    align: f64,
}

/// A rich value set at one width.
pub(super) struct Block {
    lines: Vec<Line>,
    /// The space above each line (paragraph spacing).
    above: Vec<f64>,
    pub(super) width: f64,
    pub(super) height: f64,
    /// A word was wider than its line, or a character was not encodable.
    pub(super) approximated: bool,
}

/// `rich` set in `width` (unwrapped when not positive).
pub(super) fn lay_out(rich: &Rich, width: f64) -> Block {
    let mut b = Block {
        lines: Vec::new(),
        above: Vec::new(),
        width: 0.0,
        height: 0.0,
        approximated: false,
    };
    for p in &rich.paras {
        let before = b.lines.len();
        let mut s = Setter::new(p, width);
        s.run();
        b.approximated |= s.approximated;
        for (i, line) in s.lines.into_iter().enumerate() {
            let gap = if i == 0 { p.space_before } else { 0.0 };
            b.width = b.width.max(line.left + line.width + p.margin_right);
            b.height += gap + line.height;
            b.above.push(gap);
            b.lines.push(line);
        }
        if b.lines.len() > before {
            b.height += p.space_after;
            if let Some(last) = b.lines.last_mut() {
                last.height += p.space_after;
            }
        }
    }
    b
}

/// Breaks one paragraph into lines.
struct Setter<'p> {
    p: &'p Para,
    /// Room for a line's content; infinite when unwrapped.
    room: f64,
    wrap: bool,
    lines: Vec<Line>,
    cur: Line,
    x: f64,
    /// Spaces since the last word: width, and whether they are kept.
    pending: Vec<(f64, bool)>,
    has_content: bool,
    approximated: bool,
}

impl<'p> Setter<'p> {
    fn new(p: &'p Para, width: f64) -> Self {
        let wrap = width > 0.0;
        let room = if wrap {
            (width - p.margin_left - p.margin_right).max(0.0)
        } else {
            f64::INFINITY
        };
        Self {
            p,
            room,
            wrap,
            lines: Vec::new(),
            cur: Self::line(p, true),
            x: 0.0,
            pending: Vec::new(),
            has_content: false,
            approximated: false,
        }
    }

    fn line(p: &Para, first: bool) -> Line {
        Line {
            pieces: Vec::new(),
            left: p.margin_left + if first { p.indent } else { 0.0 },
            width: 0.0,
            ascent: 0.0,
            height: 0.0,
            free: 0.0,
            align: p.align,
        }
    }

    fn run(&mut self) {
        let pieces = &self.p.pieces;
        let text = |p: &Piece| matches!(p, Piece::Text(..));
        for chunk in pieces.chunk_by(|a, b| text(a) && text(b)) {
            for piece in chunk {
                match piece {
                    Piece::Text(..) => {
                        self.word(chunk);
                        break;
                    }
                    Piece::Space(st, kept) => self.pending.push((width(st, " "), *kept)),
                    Piece::Tab(_) => self.tab(),
                    Piece::Break(_) => self.end_line(true),
                }
            }
        }
        if self.has_content || (self.lines.is_empty() && !pieces.is_empty()) {
            self.end_line(true);
        }
    }

    /// Room left on the current line, measured from its left edge.
    fn room_here(&self) -> f64 {
        self.room - (self.cur.left - self.p.margin_left)
    }

    fn word(&mut self, word: &[Piece]) {
        let w: f64 = word.iter().map(piece_width).sum();
        let spaces = self.spaces();
        if self.wrap && self.has_content && self.x + spaces + w > self.room_here() + 0.01 {
            self.end_line(false);
        }
        self.x += self.spaces();
        self.pending.clear();
        if self.x + w > self.room_here() + 0.01 && self.wrap {
            self.approximated = true;
        }
        for piece in word {
            if let Piece::Text(text, style) = piece {
                self.approximated |= encode_winansi(text).1 > 0;
                self.cur.pieces.push(Placed {
                    x: self.x,
                    text: text.clone(),
                    style: style.clone(),
                });
                self.x += width(style, text);
            }
        }
        self.has_content = true;
    }

    /// The pending spaces' width: all of them after content, only kept
    /// ones at a line start.
    fn spaces(&self) -> f64 {
        self.pending
            .iter()
            .filter(|(_, kept)| self.has_content || *kept)
            .map(|(w, _)| w)
            .sum()
    }

    /// Advance to the next tab stop past the pen, measured from the
    /// paragraph's margin edge; past the last stop, to the next multiple
    /// of the tab interval.
    fn tab(&mut self) {
        self.x += self.spaces();
        self.pending.clear();
        let indent = self.cur.left - self.p.margin_left;
        let at = indent + self.x;
        let next = self
            .p
            .tab_stops
            .iter()
            .copied()
            .find(|s| *s > at + 0.01)
            .unwrap_or_else(|| {
                ((at + 0.01) / self.p.tab_interval)
                    .floor()
                    .mul_add(self.p.tab_interval, self.p.tab_interval)
            });
        self.x = next - indent;
        self.has_content = true;
    }

    /// Close the current line; `forced` keeps an empty one (a `br`).
    fn end_line(&mut self, forced: bool) {
        let mut line = std::mem::replace(&mut self.cur, Self::line(self.p, false));
        let used: Vec<&RunStyle> = line.pieces.iter().map(|p| &p.style).collect();
        let (ascent, natural) = if used.is_empty() {
            line_metrics(&[&self.p.base])
        } else {
            line_metrics(&used)
        };
        line.width = line
            .pieces
            .last()
            .map_or(self.x, |p| p.x + width(&p.style, &p.text))
            .max(self.x.min(0.0));
        line.ascent = ascent;
        line.height = self.p.line_height.unwrap_or(natural);
        if self.wrap {
            line.free = (self.room - (line.left - self.p.margin_left) - line.width).max(0.0);
        }
        if forced || !line.pieces.is_empty() {
            self.lines.push(line);
        }
        self.x = 0.0;
        self.pending.clear();
        self.has_content = false;
    }
}

/// A line's ascent and natural height: the tallest face on it.
fn line_metrics(styles: &[&RunStyle]) -> (f64, f64) {
    let mut ascent = 0.0_f64;
    let mut descent = 0.0_f64;
    for s in styles {
        let (a, h) = metrics(s.face, s.size, None);
        ascent = ascent.max(a + s.shift.max(0.0));
        descent = descent.max(h - a - s.shift.min(0.0));
    }
    (ascent, ascent + descent)
}

fn width(style: &RunStyle, text: &str) -> f64 {
    text_width(style.face, style.size, text)
}

fn piece_width(p: &Piece) -> f64 {
    match p {
        Piece::Text(t, s) => width(s, t),
        _ => 0.0,
    }
}

impl Painter<'_> {
    /// Draw `rich` in `r`, wrapped at its width, `v_align` of the spare
    /// height above it, clipped to `r` when it has area.
    pub(super) fn set_rich(&mut self, item: &LayoutItem, rich: &Rich, v_align: f64, r: LayoutRect) {
        if rich.substituted {
            self.note(item, PaintNoteKind::Font);
        }
        let block = lay_out(rich, r.w);
        if rich.approximated || block.approximated || (r.h > 0.0 && block.height > r.h + 0.01) {
            self.note(item, PaintNoteKind::Text);
        }
        self.b.save_state();
        if r.w > 0.0 && r.h > 0.0 {
            self.b.rect(r.x, self.height - r.y - r.h, r.w, r.h);
            self.b.clip_nonzero();
            self.b.paint(Paint::NoPaint);
        }
        let mut top = self.height - r.y - (r.h - block.height).max(0.0) * v_align;
        for (line, above) in block.lines.iter().zip(&block.above) {
            top -= above;
            let baseline = top - line.ascent;
            let left = r.x + line.left + line.free * line.align;
            self.rich_line(line, left, baseline);
            top -= line.height;
        }
        self.b.restore_state();
    }

    fn rich_line(&mut self, line: &Line, left: f64, baseline: f64) {
        if line.pieces.is_empty() {
            return;
        }
        self.b.begin_text();
        for p in &line.pieces {
            let name = self.font_name(p.style.face);
            let [r, g, b] = p.style.rgb;
            self.b.set_fill_rgb(r, g, b);
            self.b.set_font(name.as_bytes(), p.style.size);
            let y = baseline + p.style.shift;
            self.b.set_text_matrix(1.0, 0.0, 0.0, 1.0, left + p.x, y);
            self.b.show_text(&encode_winansi(&p.text).0);
        }
        self.b.end_text();
        for p in line
            .pieces
            .iter()
            .filter(|p| p.style.underline || p.style.strike)
        {
            let s = &p.style;
            let [r, g, b] = s.rgb;
            self.b.set_fill_rgb(r, g, b);
            let w = width(s, &p.text);
            let y = baseline + s.shift;
            let t = s.size * 0.05;
            if s.underline {
                self.b.rect(left + p.x, y - s.size * 0.12 - t, w, t);
            }
            if s.strike {
                self.b.rect(left + p.x, y + s.size * 0.25, w, t);
            }
            self.b.paint(Paint::Fill);
        }
    }
}

/// The starting style for a rich value in `font`/`para` (font pp.741-744,
/// para pp.800-803), text in `rgb`, a `font` naming no weight in `weight`.
pub(super) fn base(
    font: Option<&XfaNode>,
    para: Option<&XfaNode>,
    weight: FontWeight,
    rgb: [f64; 3],
) -> Base {
    let f = |a| font.and_then(|n| n.attr(a));
    let p = |a| para.and_then(|n| super::length(n, a)).unwrap_or(0.0);
    Base {
        family: f("typeface").unwrap_or("Courier").to_owned(),
        bold: f("weight").map_or(weight == FontWeight::Bold, |w| w == "bold"),
        italic: f("posture") == Some("italic"),
        size: font
            .and_then(|n| super::length(n, "size"))
            .filter(|s| *s > 0.0)
            .unwrap_or(10.0),
        rgb,
        align: match para.and_then(|n| n.attr("hAlign")) {
            Some("center") => 0.5,
            Some("right") => 1.0,
            _ => 0.0,
        },
        indent: p("textIndent"),
        margin_left: p("marginLeft"),
        margin_right: p("marginRight"),
        space_before: p("spaceAbove"),
        space_after: p("spaceBelow"),
        line_height: para
            .and_then(|n| super::length(n, "lineHeight"))
            .filter(|h| *h > 0.0),
    }
}

/// The width and height `holder`'s rich value takes in `font`/`para` at the
/// paint defaults, wrapped at `wrap_at` when positive; `None` when its value
/// is not rich text. Floating fields measure empty.
pub(in crate::xfa) fn rich_extent(
    holder: &XfaNode,
    font: Option<&XfaNode>,
    para: Option<&XfaNode>,
    wrap_at: f64,
) -> Option<(f64, f64)> {
    let ex = rich_value(holder)?;
    let rich = read(ex, &base(font, para, FontWeight::Normal, [0.0; 3]), &|_| {
        None
    });
    let block = lay_out(&rich, wrap_at);
    Some((block.width, block.height))
}

/// The plain value of the node in `root` whose `id` is `key` (a leading
/// `#` ignored): what a floating field shows (Ch.5 p.219).
fn floating(
    root: &XfaNode,
    key: &str,
    page: (usize, usize),
    scripted: &Cell<bool>,
) -> Option<String> {
    let key = key.trim().trim_start_matches('#');
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if n.attr("id") == Some(key) {
            if let Some(v) = super::page_script::value(n, page.0, page.1) {
                scripted.set(true);
                return Some(v);
            }
            return Some(plain_text(n).unwrap_or_default().to_owned());
        }
        stack.extend(n.children.iter());
    }
    None
}

impl Painter<'_> {
    /// Read `holder`'s rich value against `font`/`para`.
    pub(super) fn rich(
        &mut self,
        item: &LayoutItem,
        holder: &XfaNode,
        font: Option<&XfaNode>,
        para: Option<&XfaNode>,
    ) -> Option<Rich> {
        let ex = rich_value(holder)?;
        let rgb = match font.and_then(|f| f.child("fill")) {
            Some(fill) => self.color(item, fill, [0.0; 3]),
            None => [0.0; 3],
        };
        let base = base(font, para, self.opts.font_weight, rgb);
        let (root, page, scripted) = (self.root, self.page, Cell::new(false));
        let rich = read(ex, &base, &|key| floating(root, key, page, &scripted));
        if scripted.get() {
            self.note(item, PaintNoteKind::Script);
        }
        Some(rich)
    }

    /// Draw `holder`'s rich value in `r`; `false` when it has none.
    pub(super) fn set_rich_value(
        &mut self,
        item: &LayoutItem,
        holder: &XfaNode,
        font: Option<&XfaNode>,
        para: Option<&XfaNode>,
        r: LayoutRect,
    ) -> bool {
        let Some(rich) = self.rich(item, holder, font, para) else {
            return false;
        };
        let v_align = match para.and_then(|p| p.attr("vAlign")) {
            Some("middle") => 0.5,
            Some("bottom") => 1.0,
            _ => 0.0,
        };
        self.set_rich(item, &rich, v_align, r);
        true
    }
}

//! Styled variable text: the appearance body of a rich-text field
//! (ISO 32000-1 §12.7.3.4), laid out with the plain multiline rules.
//!
//! §12.7.3.4 names the inputs (`/RV`, `/DS`; RT-M6) and fixes no layout, so
//! this module reuses the plain generator's: the same [`greedy_pack`]
//! breaker, padding, leading, quadding and auto-size, measured per span
//! with each span's own face and size. Whitespace (`U+0020`, tab, CR, LF,
//! FF) separates words and collapses, as XHTML text renders; a word may
//! cross spans when no whitespace divides them. A single-line field puts
//! every paragraph on one line, separated by a space.
//!
//! [`greedy_pack`]: crate::linebreak::greedy_pack

use std::ops::Range;

use pdfcer_model::object::{Dict, Name, Object};
use pdfcer_model::page_tree::Rect;
use pdfcer_model::writer::content::{ContentBuilder, Paint};

use super::{
    Face, FontResource, LINE_FACTOR, Quadding, TEXT_PAD, TextColor, VarTextAppearance,
    VarTextError, align_x, auto_size, measure, standard14_font_dict,
};
use crate::fontdata::Std14;

/// One styled stretch of text for [`build_rich_text`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RichSpan {
    /// The text, untrimmed; whitespace inside it separates words.
    pub text: String,
    /// The font resource name (no leading `/`), looked up in the
    /// `resources` passed to [`build_rich_text`].
    pub font: Vec<u8>,
    /// The point size; `0.0` or less means auto-size ([`auto_size`]).
    pub size: f64,
    /// The fill colour.
    pub color: TextColor,
    /// Draw an underline beneath the span.
    pub underline: bool,
    /// Draw a line through the span.
    pub strikethrough: bool,
    /// The paragraph the span belongs to, counting from zero. A change of
    /// paragraph between consecutive spans is a line break.
    pub paragraph: usize,
}

impl RichSpan {
    /// A span with no decoration.
    #[must_use]
    pub fn new(
        text: impl Into<String>,
        font: impl Into<Vec<u8>>,
        size: f64,
        color: TextColor,
        paragraph: usize,
    ) -> Self {
        Self {
            text: text.into(),
            font: font.into(),
            size,
            color,
            underline: false,
            strikethrough: false,
            paragraph,
        }
    }

    /// This span with underline and line-through set as given.
    #[must_use]
    pub fn with_decoration(mut self, underline: bool, strikethrough: bool) -> Self {
        self.underline = underline;
        self.strikethrough = strikethrough;
        self
    }
}

/// Generate the `/Tx BMC … EMC` body for styled `spans` in `bbox`.
///
/// `aligns[p]` is paragraph `p`'s quadding; a paragraph past the end of
/// `aligns` is left-aligned. A single-line field (`multiline = false`) uses
/// `aligns[0]` for its one line and does not wrap. Underline and
/// line-through are filled bars in the span's colour, drawn after the text
/// inside the same clip.
///
/// [`VarTextAppearance::applied_autosize`] reports the size chosen for any
/// span of size `0`; [`VarTextAppearance::used_font`] is the first span's
/// face and [`VarTextAppearance::used_size`] the largest size drawn.
///
/// # Errors
///
/// [`VarTextError::FontUnresolved`] when a span's font is not in
/// `resources`; [`VarTextError::SymbolicFont`] when it is Symbol or
/// ZapfDingbats.
///
/// # Examples
///
/// ```
/// use pdfcer_fonts::fontdata::Std14;
/// use pdfcer_fonts::vartext::{FontResource, Quadding, RichSpan, TextColor, build_rich_text};
/// use pdfcer_model::page_tree::Rect;
///
/// let font = |name: &[u8], font| FontResource {
///     name: name.to_vec(), font, widths: None, ascent: None, descent: None,
///     codes: None, cid: None,
/// };
/// let res = [font(b"Helv", Std14::Helvetica), font(b"HeBo", Std14::HelveticaBold)];
/// let black = TextColor::Gray(0.0);
/// let spans = [
///     RichSpan::new("plain ", "Helv", 10.0, black, 0),
///     RichSpan::new("bold", "HeBo", 10.0, black, 0),
/// ];
/// let bbox = Rect { llx: 0.0, lly: 0.0, urx: 200.0, ury: 20.0 };
/// let ap = build_rich_text(bbox, &spans, &[Quadding::Left], false, &res)?;
/// let body = String::from_utf8_lossy(&ap.content);
/// assert!(body.contains("/Helv 10 Tf") && body.contains("/HeBo 10 Tf"));
/// # Ok::<(), pdfcer_fonts::vartext::VarTextError>(())
/// ```
pub fn build_rich_text(
    bbox: Rect,
    spans: &[RichSpan],
    aligns: &[Quadding],
    multiline: bool,
    resources: &[FontResource],
) -> Result<VarTextAppearance, VarTextError> {
    let (w, h) = (bbox.width(), bbox.height());
    let auto = auto_size(h);
    let faces = span_faces(spans, resources, auto)?;
    let (words, paras, unencodable_chars) = tokenise(spans, &faces, aligns, multiline);
    let max_width = (w - 2.0 * TEXT_PAD).max(0.0);
    let lines = break_lines(&words, &paras, &faces, max_width, multiline);

    let mut b = ContentBuilder::new();
    b.begin_marked_content(b"Tx");
    b.save_state();
    b.rect(0.0, 0.0, w, h);
    b.clip_nonzero();
    b.paint(Paint::NoPaint);
    if !lines.is_empty() {
        let bars = emit_lines(&mut b, spans, &faces, &words, &lines, w, h, multiline);
        emit_bars(&mut b, &bars);
    }
    b.restore_state();
    b.end_marked_content();

    let used_size = faces.iter().map(|f| f.size).fold(0.0, f64::max);
    let applied_autosize = spans.iter().any(|s| s.size <= 0.0).then_some(auto);
    Ok(VarTextAppearance {
        content: b.into_bytes(),
        resources: span_resources(spans, &faces),
        applied_autosize,
        applied_autosize_bound: None,
        da_colour_unmodelled: false,
        unencodable_chars,
        used_font: faces.first().map_or(Std14::Helvetica, |f| f.face.font),
        used_size,
    })
}

/// A span's resolved face and size.
#[derive(Clone, Copy)]
struct SpanFace<'a> {
    face: Face<'a>,
    size: f64,
}

/// Resolve every span's font name and size, refusing an absent or symbolic
/// font exactly as the plain generator's `/DA` lookup does.
fn span_faces<'r>(
    spans: &[RichSpan],
    resources: &'r [FontResource],
    auto: f64,
) -> Result<Vec<SpanFace<'r>>, VarTextError> {
    spans
        .iter()
        .map(|s| {
            let name = || String::from_utf8_lossy(&s.font).into_owned();
            let face = resources
                .iter()
                .find(|r| r.name == s.font)
                .map(Face::of)
                .ok_or_else(|| VarTextError::FontUnresolved(name()))?;
            if matches!(face.font, Std14::Symbol | Std14::ZapfDingbats) {
                return Err(VarTextError::SymbolicFont(name()));
            }
            let size = if s.size > 0.0 { s.size } else { auto };
            Ok(SpanFace { face, size })
        })
        .collect()
}

/// A same-span stretch of one word.
struct Piece {
    span: usize,
    codes: Vec<u16>,
    width: f64,
}

/// A whitespace-delimited word, and the span of the whitespace before it.
struct Word {
    pieces: Vec<Piece>,
    gap: Option<usize>,
}

/// A paragraph's words and alignment; `span` sizes it when it is blank.
struct Para {
    words: Range<usize>,
    quad: Quadding,
    span: usize,
}

/// XHTML whitespace. `U+00A0` is deliberately not in it: a no-break space
/// is a glyph.
const fn is_space(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{0C}')
}

/// Accumulates words and paragraphs while the spans are scanned.
struct Tokens<'f, 'a> {
    faces: &'f [SpanFace<'a>],
    words: Vec<Word>,
    paras: Vec<Para>,
    cur: Vec<Piece>,
    gap_before: Option<usize>,
    unencodable: usize,
}

impl Tokens<'_, '_> {
    fn push_piece(&mut self, span: usize, text: &str) {
        let Some(f) = self.faces.get(span) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let (codes, miss) = f.face.encode(text);
        self.unencodable += miss;
        let width = measure(f.face, f.size, &codes);
        self.cur.push(Piece { span, codes, width });
    }

    fn finish_word(&mut self) {
        if !self.cur.is_empty() {
            let pieces = std::mem::take(&mut self.cur);
            let gap = self.gap_before.take();
            self.words.push(Word { pieces, gap });
        }
    }

    fn whitespace(&mut self, span: usize) {
        self.finish_word();
        self.gap_before.get_or_insert(span);
    }

    fn close_para(&mut self, quad: Quadding, span: usize) {
        self.finish_word();
        let start = self.paras.last().map_or(0, |p| p.words.end);
        let words = start..self.words.len();
        self.paras.push(Para { words, quad, span });
        self.gap_before = None;
    }
}

/// Split `spans` into words and paragraphs, encoding and measuring each
/// piece with its span's face.
fn tokenise(
    spans: &[RichSpan],
    faces: &[SpanFace<'_>],
    aligns: &[Quadding],
    multiline: bool,
) -> (Vec<Word>, Vec<Para>, usize) {
    let mut t = Tokens {
        faces,
        words: Vec::new(),
        paras: Vec::new(),
        cur: Vec::new(),
        gap_before: None,
        unencodable: 0,
    };
    let quad_of = |p: usize| aligns.get(p).copied().unwrap_or_default();
    let mut open: Option<(usize, usize)> = None; // (paragraph, first span)
    for (i, s) in spans.iter().enumerate() {
        match open {
            Some((p, first)) if p != s.paragraph => {
                if multiline {
                    t.close_para(quad_of(p), first);
                    open = Some((s.paragraph, i));
                } else {
                    t.whitespace(i);
                }
            }
            None => open = Some((s.paragraph, i)),
            Some(_) => {}
        }
        let mut buf = String::new();
        for ch in s.text.chars() {
            if is_space(ch) {
                t.push_piece(i, &buf);
                buf.clear();
                t.whitespace(i);
            } else {
                buf.push(ch);
            }
        }
        t.push_piece(i, &buf);
    }
    if let Some((p, first)) = open {
        let quad = if multiline { quad_of(p) } else { quad_of(0) };
        t.close_para(quad, first);
    }
    (t.words, t.paras, t.unencodable)
}

/// One laid-out line.
struct Line {
    words: Range<usize>,
    quad: Quadding,
    width: f64,
    size: f64,
    ascent: f64,
}

/// The width of a gap shown in span `g`'s face.
fn gap_width(faces: &[SpanFace<'_>], g: usize) -> f64 {
    faces
        .get(g)
        .map_or(0.0, |f| measure(f.face, f.size, &[f.face.space()]))
}

/// The width of `words[r]` laid on one line: each word, plus the gap
/// before every word but the first.
fn run_width(words: &[Word], faces: &[SpanFace<'_>], r: Range<usize>) -> f64 {
    words
        .get(r)
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .fold(0.0, |acc, (k, word)| {
            let gap = match word.gap {
                Some(g) if k > 0 => gap_width(faces, g),
                _ => 0.0,
            };
            acc + gap + word.pieces.iter().map(|p| p.width).sum::<f64>()
        })
}

/// The largest size and ascent among the spans `words[r]` draws in, else
/// span `blank`'s for an empty line.
fn line_metrics(
    words: &[Word],
    faces: &[SpanFace<'_>],
    r: Range<usize>,
    blank: usize,
) -> (f64, f64) {
    let spans = words
        .get(r)
        .unwrap_or(&[])
        .iter()
        .flat_map(|w| w.pieces.iter().map(|p| p.span));
    let mut best = (0.0f64, 0.0f64);
    for f in spans
        .chain(std::iter::once(blank))
        .filter_map(|s| faces.get(s))
    {
        best.0 = best.0.max(f.size);
        best.1 = best.1.max(f.face.ascent_at(f.size));
    }
    best
}

/// Break each paragraph into lines with the shared greedy breaker.
fn break_lines(
    words: &[Word],
    paras: &[Para],
    faces: &[SpanFace<'_>],
    max_width: f64,
    multiline: bool,
) -> Vec<Line> {
    let mut lines = Vec::new();
    for para in paras {
        let n = para.words.len();
        let ranges: Vec<Range<usize>> = if n == 0 || !multiline {
            vec![para.words.clone()]
        } else {
            let base = para.words.start;
            crate::linebreak::greedy_pack(n, max_width, |s, e| {
                run_width(words, faces, base + s..base + e)
            })
            .into_iter()
            .map(|r| base + r.start..base + r.end)
            .collect()
        };
        for r in ranges {
            let blank = if r.is_empty() { para.span } else { usize::MAX };
            let (size, ascent) = line_metrics(words, faces, r.clone(), blank);
            lines.push(Line {
                width: run_width(words, faces, r.clone()),
                words: r,
                quad: para.quad,
                size,
                ascent,
            });
        }
    }
    lines
}

/// A decoration bar: `x0..x1` at baseline `y`, for text of `size`.
struct Bar {
    x0: f64,
    x1: f64,
    y: f64,
    size: f64,
    color: TextColor,
    underline: bool,
    strikethrough: bool,
}

/// The text object's current font and colour, so each is set only on change.
struct Pen<'s> {
    font: Option<(&'s [u8], f64)>,
    color: Option<TextColor>,
}

impl<'s> Pen<'s> {
    fn select(&mut self, b: &mut ContentBuilder, span: &'s RichSpan, size: f64) {
        let font = (span.font.as_slice(), size);
        if self.font != Some(font) {
            b.set_font(font.0, size);
            self.font = Some(font);
        }
        if self.color != Some(span.color) {
            span.color.apply_fill(b);
            self.color = Some(span.color);
        }
    }
}

/// The first baseline: below the top padding for a multiline field,
/// centred on the largest face for a single line.
fn first_baseline(faces: &[SpanFace<'_>], lines: &[Line], h: f64, multiline: bool) -> f64 {
    let top = lines.first().map_or(0.0, |l| l.ascent);
    if multiline {
        return h - TEXT_PAD - top;
    }
    faces
        .iter()
        .copied()
        .max_by(|a, b| a.size.total_cmp(&b.size))
        .map_or(h / 2.0, |f| f.face.centred_baseline(f.size, h))
}

/// Emit the `BT … ET` text object; returns the decoration bars to draw.
#[allow(clippy::too_many_arguments)] // the layout and its inputs, one each.
fn emit_lines(
    b: &mut ContentBuilder,
    spans: &[RichSpan],
    faces: &[SpanFace<'_>],
    words: &[Word],
    lines: &[Line],
    w: f64,
    h: f64,
    multiline: bool,
) -> Vec<Bar> {
    let mut bars = Vec::new();
    let mut pen = Pen {
        font: None,
        color: None,
    };
    let mut y = first_baseline(faces, lines, h, multiline);
    let mut prev: Option<(f64, f64)> = None;
    b.begin_text();
    for (li, line) in lines.iter().enumerate() {
        if li > 0 {
            y -= line.size * LINE_FACTOR;
        }
        let x = align_x(line.quad, w, line.width);
        match prev {
            None => b.set_text_matrix(1.0, 0.0, 0.0, 1.0, x, y),
            Some((px, py)) => b.text_move(x - px, y - py),
        }
        prev = Some((x, y));
        let mut cursor = x;
        let line_words = words.get(line.words.clone()).unwrap_or(&[]);
        for (k, word) in line_words.iter().enumerate() {
            let gap = word.gap.filter(|_| k > 0);
            let shown = gap
                .into_iter()
                .map(|g| (g, None))
                .chain(word.pieces.iter().map(|p| (p.span, Some(p))));
            for (si, piece) in shown {
                let (Some(span), Some(f)) = (spans.get(si), faces.get(si)) else {
                    continue;
                };
                pen.select(b, span, f.size);
                let (codes, width) = match piece {
                    Some(p) => (p.codes.clone(), p.width),
                    None => (vec![f.face.space()], gap_width(faces, si)),
                };
                b.show_text(&f.face.show_bytes(&codes));
                if span.underline || span.strikethrough {
                    bars.push(bar(span, cursor, cursor + width, y, f.size));
                }
                cursor += width;
            }
        }
    }
    b.end_text();
    bars
}

const fn bar(span: &RichSpan, x0: f64, x1: f64, y: f64, size: f64) -> Bar {
    Bar {
        x0,
        x1,
        y,
        size,
        color: span.color,
        underline: span.underline,
        strikethrough: span.strikethrough,
    }
}

/// Underline at 0.12 em below the baseline and line-through at 0.28 em
/// above it, each 0.05 em thick — pdfcer's choice; Table 225 names the
/// decoration and fixes no geometry.
fn emit_bars(b: &mut ContentBuilder, bars: &[Bar]) {
    for bar in bars {
        let t = 0.05 * bar.size;
        bar.color.apply_fill(b);
        for (on, dy) in [(bar.underline, -0.12), (bar.strikethrough, 0.28)] {
            if on {
                b.rect(bar.x0, bar.y + dy * bar.size - t / 2.0, bar.x1 - bar.x0, t);
                b.paint(Paint::Fill);
            }
        }
    }
}

/// `<< /Font << /name <standard-14 dict> … >> >>` for every font drawn.
fn span_resources(spans: &[RichSpan], faces: &[SpanFace<'_>]) -> Dict {
    let mut fonts = Dict::new();
    for (s, f) in spans.iter().zip(faces) {
        if fonts.get(&s.font).is_none() {
            fonts.insert(
                Name(s.font.clone()),
                Object::Dict(standard14_font_dict(f.face.font)),
            );
        }
    }
    let mut res = Dict::new();
    res.insert(Name::from(b"Font"), Object::Dict(fonts));
    res
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn res() -> Vec<FontResource> {
        [
            (b"Helv".as_slice(), Std14::Helvetica),
            (b"HeBo", Std14::HelveticaBold),
            (b"HeOb", Std14::HelveticaOblique),
        ]
        .iter()
        .map(|&(n, font)| FontResource {
            name: n.to_vec(),
            font,
            widths: None,
            ascent: None,
            descent: None,
            codes: None,
            cid: None,
        })
        .collect()
    }

    fn bbox(w: f64, h: f64) -> Rect {
        Rect {
            llx: 0.0,
            lly: 0.0,
            urx: w,
            ury: h,
        }
    }

    const BLACK: TextColor = TextColor::Gray(0.0);

    /// Each span selects its own font, and a colour change is emitted
    /// before the span that carries it.
    #[test]
    fn each_span_selects_its_font_and_colour() {
        let spans = [
            RichSpan::new("plain ", "Helv", 10.0, BLACK, 0),
            RichSpan::new("bold", "HeBo", 10.0, BLACK, 0),
            RichSpan::new(" red", "Helv", 10.0, TextColor::Rgb(1.0, 0.0, 0.0), 0),
        ];
        let ap = build_rich_text(bbox(300.0, 20.0), &spans, &[], false, &res()).unwrap();
        let body = String::from_utf8_lossy(&ap.content);
        let bold = body.find("/HeBo 10 Tf").unwrap();
        let red = body.find("1 0 0 rg").unwrap();
        assert!(bold < body.find("(bold) Tj").unwrap());
        assert!(red < body.find("(red) Tj").unwrap() && red > bold);
    }

    /// A paragraph break wraps a multiline field onto a new line, and a
    /// word wider than the box still wraps by the shared breaker.
    #[test]
    fn paragraphs_and_wrapping_make_lines() {
        let spans = [
            RichSpan::new("one two three", "Helv", 10.0, BLACK, 0),
            RichSpan::new("four", "Helv", 10.0, BLACK, 1),
        ];
        let ap = build_rich_text(bbox(40.0, 80.0), &spans, &[], true, &res()).unwrap();
        let body = String::from_utf8_lossy(&ap.content);
        assert!(
            body.contains(
                "Td
(three) Tj
0 -11.5 Td
(four) Tj"
            ),
            "{body}"
        );
    }

    /// Underline draws a filled bar after the text object.
    #[test]
    fn underline_draws_a_bar() {
        let spans = [RichSpan::new("u", "Helv", 10.0, BLACK, 0).with_decoration(true, false)];
        let ap = build_rich_text(bbox(100.0, 20.0), &spans, &[], false, &res()).unwrap();
        let body = String::from_utf8_lossy(&ap.content);
        let et = body.find("ET").unwrap();
        assert!(body[et..].contains(" re\nf"), "{body}");
    }

    /// An unknown font is the plain generator's named refusal.
    #[test]
    fn an_unknown_font_is_refused() {
        let spans = [RichSpan::new("x", "Nope", 10.0, BLACK, 0)];
        let err = build_rich_text(bbox(100.0, 20.0), &spans, &[], false, &res()).unwrap_err();
        assert_eq!(err, VarTextError::FontUnresolved("Nope".into()));
    }
}

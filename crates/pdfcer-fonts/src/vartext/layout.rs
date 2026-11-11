//! Where each character of a variable-text value lands, laid out exactly as
//! [`build_variable_text`] and [`build_comb_text`] draw it (§12.7.3.3).
//!
//! The size comes from running the builder itself, so an auto-sized field
//! (`0 Tf`) is measured at the size it is drawn at. Coordinates are the
//! appearance's local `/BBox` space, origin at the box's lower-left corner.

use super::{
    Face, LINE_FACTOR, Quadding, TEXT_PAD, VarTextError, align_x, build_comb_text,
    build_variable_text, join_words, measure, parse_default_appearance,
};
use crate::fontdata::Std14;
use pdfcer_model::page_tree::Rect;

use super::FontResource;

/// One character's horizontal span on its line.
///
/// A character the builder does not draw (a collapsed space, a line break, a
/// comb character past `/MaxLen`) has `x0 == x1`, at the pen position where
/// it was dropped.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct CharSpan {
    /// Byte offset of the character in the laid-out string.
    pub offset: usize,
    /// Byte offset one past the character.
    pub end: usize,
    /// Left edge of its advance, local x.
    pub x0: f64,
    /// Right edge of its advance, local x.
    pub x1: f64,
}

/// One drawn line.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LineLayout {
    /// Byte offset of the line's first character (the paragraph start for a
    /// line with none).
    pub start: usize,
    /// Byte offset one past its last character.
    pub end: usize,
    /// Baseline, local y.
    pub baseline: f64,
    /// Left edge of the drawn text, local x.
    pub x0: f64,
    /// Right edge of the drawn text, local x.
    pub x1: f64,
    /// The line's characters in string order.
    pub chars: Vec<CharSpan>,
}

/// A laid-out variable-text value.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TextLayout {
    /// The point size drawn at, after any auto-size.
    pub font_size: f64,
    /// Ascent above the baseline at that size, points.
    pub ascent: f64,
    /// Descent below the baseline at that size, points (positive).
    pub descent: f64,
    /// Lines, top to bottom. Never empty.
    pub lines: Vec<LineLayout>,
    /// Characters drawn as `?` because the font has no code for them.
    pub unencodable_chars: usize,
}

/// Lay `text` out as [`build_variable_text`] draws it.
///
/// # Errors
///
/// Exactly the errors [`build_variable_text`] returns for the same inputs.
///
/// # Examples
///
/// ```
/// use pdfcer_fonts::fontdata::Std14;
/// use pdfcer_fonts::vartext::{layout_variable_text, FontResource, Quadding};
/// use pdfcer_model::page_tree::Rect;
///
/// let helv = FontResource {
///     name: b"Helv".to_vec(),
///     font: Std14::Helvetica,
///     widths: None,
///     ascent: None,
///     descent: None,
///     codes: None,
///     cid: None,
/// };
/// let bbox = Rect { llx: 0.0, lly: 0.0, urx: 200.0, ury: 20.0 };
/// let laid = layout_variable_text(bbox, "Hi", b"/Helv 12 Tf", Quadding::Left, false, &[helv])?;
/// assert_eq!(laid.lines[0].chars.len(), 2);
/// assert_eq!(laid.lines[0].x0, 2.0);
/// # Ok::<(), pdfcer_fonts::vartext::VarTextError>(())
/// ```
pub fn layout_variable_text(
    bbox: Rect,
    text: &str,
    da: &[u8],
    quad: Quadding,
    multiline: bool,
    resources: &[FontResource],
) -> Result<TextLayout, VarTextError> {
    let built = build_variable_text(bbox, text, da, quad, multiline, resources)?;
    let face = face_for(da, resources)?;
    let size = built.used_size;
    let (w, h) = (bbox.width(), bbox.height());
    let mut lines = if multiline {
        multiline_lines(face, size, text, quad, w)
    } else {
        vec![single_line(face, size, text, quad, w)]
    };
    let first = if multiline {
        h - TEXT_PAD - face.ascent_at(size)
    } else {
        face.centred_baseline(size, h)
    };
    for (i, line) in lines.iter_mut().enumerate() {
        line.baseline = first - i as f64 * size * LINE_FACTOR;
    }
    Ok(TextLayout {
        font_size: size,
        ascent: face.ascent_at(size),
        descent: face.descent_at(size),
        lines,
        unencodable_chars: built.unencodable_chars,
    })
}

/// Lay `text` out as [`build_comb_text`] draws it: one character per cell,
/// centred in it.
///
/// # Errors
///
/// Exactly the errors [`build_comb_text`] returns for the same inputs.
pub fn layout_comb_text(
    bbox: Rect,
    text: &str,
    da: &[u8],
    cells: usize,
    resources: &[FontResource],
) -> Result<TextLayout, VarTextError> {
    let built = build_comb_text(bbox, text, da, cells, resources)?;
    let face = face_for(da, resources)?;
    let size = built.used_size;
    let cells = cells.max(1);
    let cell_w = bbox.width() / cells as f64;
    let mut chars = Vec::new();
    let mut drawn = 0usize;
    let mut pen = 0.0;
    for (offset, ch) in text.char_indices() {
        let end = offset + ch.len_utf8();
        if matches!(ch, '\r' | '\n') || drawn >= cells {
            chars.push(span(offset, end, pen, pen));
            continue;
        }
        let code = face.encode(ch.encode_utf8(&mut [0; 4])).0;
        let gw = measure(face, size, &code);
        let x0 = drawn as f64 * cell_w + (cell_w - gw) / 2.0;
        pen = x0 + gw;
        chars.push(span(offset, end, x0, pen));
        drawn += 1;
    }
    let mut line = line_of(chars, 0, 0.0);
    line.baseline = face.centred_baseline(size, bbox.height());
    Ok(TextLayout {
        font_size: size,
        ascent: face.ascent_at(size),
        descent: face.descent_at(size),
        lines: vec![line],
        unencodable_chars: built.unencodable_chars,
    })
}

fn face_for<'a>(da: &[u8], resources: &'a [FontResource]) -> Result<Face<'a>, VarTextError> {
    let parsed = parse_default_appearance(da)?;
    resources
        .iter()
        .find(|r| r.name == parsed.font_name)
        .map(Face::of)
        .filter(|f| !matches!(f.font, Std14::Symbol | Std14::ZapfDingbats))
        .ok_or_else(|| {
            VarTextError::FontUnresolved(String::from_utf8_lossy(&parsed.font_name).into_owned())
        })
}

impl CharSpan {
    /// A span from its offsets and edges.
    #[must_use]
    pub const fn new(offset: usize, end: usize, x0: f64, x1: f64) -> Self {
        Self {
            offset,
            end,
            x0,
            x1,
        }
    }
}

const fn span(offset: usize, end: usize, x0: f64, x1: f64) -> CharSpan {
    CharSpan::new(offset, end, x0, x1)
}

/// A line from its characters; `start` and `x` are used when it has none.
fn line_of(chars: Vec<CharSpan>, start: usize, x: f64) -> LineLayout {
    let drawn = || chars.iter().filter(|c| c.x1 > c.x0);
    LineLayout {
        start: chars.first().map_or(start, |c| c.offset),
        end: chars.last().map_or(start, |c| c.end),
        baseline: 0.0,
        x0: drawn().map(|c| c.x0).reduce(f64::min).unwrap_or(x),
        x1: drawn().map(|c| c.x1).reduce(f64::max).unwrap_or(x),
        chars,
    }
}

/// A single-line field: line breaks become spaces, nothing wraps or
/// collapses, the whole string is one `Tj`.
fn single_line(face: Face<'_>, size: f64, text: &str, quad: Quadding, w: f64) -> LineLayout {
    let flat: String = text
        .chars()
        .map(|c| if matches!(c, '\n' | '\r') { ' ' } else { c })
        .collect();
    let codes = face.encode(&flat).0;
    let mut pen = align_x(quad, w, measure(face, size, &codes));
    let start = pen;
    let chars = text
        .char_indices()
        .zip(&codes)
        .map(|((offset, ch), &code)| {
            let x0 = pen;
            pen += measure(face, size, &[code]);
            span(offset, offset + ch.len_utf8(), x0, pen)
        })
        .collect();
    line_of(chars, 0, start)
}

/// One character of a paragraph, with its code (`None` for a stripped `\r`).
struct Item {
    offset: usize,
    end: usize,
    code: Option<u16>,
}

/// A multiline field: paragraphs split on `\n`, words greedily packed, runs
/// of spaces collapsed to one, exactly as `wrap_lines` does.
fn multiline_lines(
    face: Face<'_>,
    size: f64,
    text: &str,
    quad: Quadding,
    w: f64,
) -> Vec<LineLayout> {
    let mut out = Vec::new();
    let mut para_start = 0usize;
    for para in text.split('\n') {
        let mut lines = paragraph_lines(face, size, para, para_start, quad, w);
        let next = para_start + para.len();
        if next < text.len() {
            // The `\n` itself: zero width at the end of the paragraph.
            if let Some(last) = lines.last_mut() {
                let x = last.chars.last().map_or(last.x1, |c| c.x1);
                last.chars.push(span(next, next + 1, x, x));
                last.end = next + 1;
                if last.chars.len() == 1 {
                    last.start = next;
                }
            }
        }
        out.append(&mut lines);
        para_start = next + 1;
    }
    out
}

fn paragraph_items(face: Face<'_>, para: &str, base: usize) -> Vec<Item> {
    let kept: String = para.chars().filter(|&c| c != '\r').collect();
    let mut codes = face.encode(&kept).0.into_iter();
    para.char_indices()
        .map(|(i, ch)| Item {
            offset: base + i,
            end: base + i + ch.len_utf8(),
            code: if ch == '\r' { None } else { codes.next() },
        })
        .collect()
}

fn paragraph_lines(
    face: Face<'_>,
    size: f64,
    para: &str,
    base: usize,
    quad: Quadding,
    w: f64,
) -> Vec<LineLayout> {
    let space = face.space();
    let items = paragraph_items(face, para, base);
    // Words as item-index ranges, and each word's codes.
    let mut words: Vec<(usize, usize)> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let is_word = it.code.is_some_and(|c| c != space);
        let joins = words.last().is_some_and(|&(_, e)| e == i);
        match (is_word, joins) {
            (true, true) => {
                if let Some(last) = words.last_mut() {
                    last.1 = i + 1;
                }
            }
            (true, false) => words.push((i, i + 1)),
            (false, _) => {}
        }
    }
    let word_codes: Vec<Vec<u16>> = words
        .iter()
        .map(|&(s, e)| {
            items
                .get(s..e)
                .unwrap_or(&[])
                .iter()
                .filter_map(|it| it.code)
                .collect()
        })
        .collect();
    let max_width = (w - 2.0 * TEXT_PAD).max(0.0);
    let joined = |s: usize, e: usize| {
        let refs: Vec<&[u16]> = word_codes
            .get(s..e)
            .unwrap_or(&[])
            .iter()
            .map(Vec::as_slice)
            .collect();
        join_words(&refs, space)
    };
    let ranges = crate::linebreak::greedy_pack(words.len(), max_width, |s, e| {
        measure(face, size, &joined(s, e))
    });
    if ranges.is_empty() {
        let x = align_x(quad, w, 0.0);
        let chars = items
            .iter()
            .map(|it| span(it.offset, it.end, x, x))
            .collect();
        return vec![line_of(chars, base, x)];
    }
    place_words(face, size, &items, &words, &ranges, &joined, quad, w, base)
}

#[allow(clippy::too_many_arguments)] // the wrap's inputs, one each.
fn place_words(
    face: Face<'_>,
    size: f64,
    items: &[Item],
    words: &[(usize, usize)],
    ranges: &[std::ops::Range<usize>],
    joined: &dyn Fn(usize, usize) -> Vec<u16>,
    quad: Quadding,
    w: f64,
    base: usize,
) -> Vec<LineLayout> {
    let starts: Vec<f64> = ranges
        .iter()
        .map(|r| align_x(quad, w, measure(face, size, &joined(r.start, r.end))))
        .collect();
    let line_of_word = |k: usize| ranges.iter().position(|r| r.contains(&k)).unwrap_or(0);
    let mut per_line: Vec<Vec<CharSpan>> = vec![Vec::new(); ranges.len()];
    let (mut cur, mut pen) = (0usize, starts.first().copied().unwrap_or(TEXT_PAD));
    let mut prev_word: Option<usize> = None;
    let mut gap_drawn = false;
    for (i, it) in items.iter().enumerate() {
        let word = words.iter().position(|&(s, e)| (s..e).contains(&i));
        let adv = match (word, it.code) {
            (Some(k), Some(code)) => {
                let line = line_of_word(k);
                if line != cur {
                    (cur, pen) = (line, starts.get(line).copied().unwrap_or(pen));
                }
                prev_word = Some(k);
                gap_drawn = false;
                measure(face, size, &[code])
            }
            (None, Some(code)) => {
                let inner = prev_word
                    .is_some_and(|k| k + 1 < words.len() && line_of_word(k) == line_of_word(k + 1));
                if inner && !gap_drawn {
                    gap_drawn = true;
                    measure(face, size, &[code])
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        if let Some(chars) = per_line.get_mut(cur) {
            chars.push(span(it.offset, it.end, pen, pen + adv));
        }
        pen += adv;
    }
    per_line
        .into_iter()
        .zip(starts)
        .map(|(chars, x)| line_of(chars, base, x))
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests {
    use super::*;

    fn helv() -> Vec<FontResource> {
        vec![FontResource {
            name: b"Helv".to_vec(),
            font: Std14::Helvetica,
            widths: None,
            ascent: None,
            descent: None,
            codes: None,
            cid: None,
        }]
    }

    fn bbox(w: f64, h: f64) -> Rect {
        Rect {
            llx: 0.0,
            lly: 0.0,
            urx: w,
            ury: h,
        }
    }

    #[test]
    fn hello_spans_are_contiguous_and_sum_to_the_advance() {
        let laid = layout_variable_text(
            bbox(200.0, 20.0),
            "Hello",
            b"/Helv 12 Tf",
            Quadding::Left,
            false,
            &helv(),
        )
        .unwrap();
        let line = &laid.lines[0];
        assert_eq!(line.chars.len(), 5);
        assert_eq!(line.x0, TEXT_PAD);
        let width = crate::vartext::text_width(Std14::Helvetica, 12.0, "Hello");
        assert!((line.x1 - line.x0 - width).abs() < 1e-9);
        for pair in line.chars.windows(2) {
            assert_eq!(pair[0].x1, pair[1].x0);
        }
    }

    #[test]
    fn a_collapsed_space_run_draws_one_space() {
        let laid = layout_variable_text(
            bbox(400.0, 40.0),
            "a   b", // string-gap-exempt: the space run is the input under test.
            b"/Helv 12 Tf",
            Quadding::Left,
            true,
            &helv(),
        )
        .unwrap();
        let line = &laid.lines[0];
        let widths: Vec<f64> = line.chars.iter().map(|c| c.x1 - c.x0).collect();
        assert!(widths[1] > 0.0);
        assert_eq!(widths[2], 0.0);
        assert_eq!(widths[3], 0.0);
        let expect = crate::vartext::text_width(Std14::Helvetica, 12.0, "a b");
        assert!((line.x1 - line.x0 - expect).abs() < 1e-9);
    }

    #[test]
    fn a_newline_starts_a_new_line_and_is_itself_zero_width() {
        let laid = layout_variable_text(
            bbox(400.0, 60.0),
            "ab\ncd",
            b"/Helv 12 Tf",
            Quadding::Left,
            true,
            &helv(),
        )
        .unwrap();
        assert_eq!(laid.lines.len(), 2);
        let nl = laid.lines[0].chars[2];
        assert_eq!((nl.offset, nl.x0, nl.x1), (2, nl.x1, laid.lines[0].x1));
        assert_eq!(laid.lines[1].start, 3);
        assert!(laid.lines[1].baseline < laid.lines[0].baseline);
    }

    #[test]
    fn comb_spans_stay_in_their_cells() {
        let laid =
            layout_comb_text(bbox(100.0, 20.0), "ABCDEFG", b"/Helv 12 Tf", 5, &helv()).unwrap();
        let chars = &laid.lines[0].chars;
        for (i, c) in chars.iter().take(5).enumerate() {
            assert!(c.x0 >= i as f64 * 20.0 && c.x1 <= (i + 1) as f64 * 20.0);
        }
        assert_eq!(chars[5].x0, chars[5].x1);
    }
}

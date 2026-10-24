//! Turning an edit script over words into [`Change`]s with page places.

use std::collections::HashMap;
use std::ops::Range;

use pdfcer_model::page_tree::Rect;

use super::diff::Op;
use super::words::{Style, Word, union};
use super::{Change, ChangeKind, FormattingDelta, Place, Side};
use crate::text_extract::TextColor;

/// Size differences up to this many points are not formatting changes.
const SIZE_TOLERANCE: f32 = 0.05;
/// Colour components closer than this are the same colour.
const COLOR_TOLERANCE: f32 = 0.01;

/// Map each distinct word text to one id, shared by both sides.
pub(super) fn intern(a: &[Word], b: &[Word]) -> (Vec<u32>, Vec<u32>) {
    let mut ids: HashMap<&str, u32> = HashMap::new();
    let mut ia = Vec::with_capacity(a.len());
    let mut ib = Vec::with_capacity(b.len());
    for (words, out) in [(a, &mut ia), (b, &mut ib)] {
        for w in words {
            let next = u32::try_from(ids.len()).unwrap_or(u32::MAX);
            out.push(*ids.entry(w.text.as_str()).or_insert(next));
        }
    }
    (ia, ib)
}

/// Group the script's non-equal ops into changes, in order.
pub(super) fn build(ops: &[Op], a: &[Word], b: &[Word], formatting: bool) -> Vec<Change> {
    let mut out = Vec::new();
    let (mut oi, mut ni) = (0, 0);
    let mut hunk: Option<(Range<usize>, Range<usize>)> = None;
    for op in ops {
        match op {
            Op::Equal { old, new, len } => {
                if let Some((ro, rn)) = hunk.take() {
                    out.push(text_change(a, b, ro, rn));
                }
                if formatting {
                    formatting_changes(a, b, *old, *new, *len, &mut out);
                }
                (oi, ni) = (old + len, new + len);
            }
            Op::Delete { old } => {
                hunk.get_or_insert((oi..oi, ni..ni)).0.end = old.end;
                oi = old.end;
            }
            Op::Insert { new } => {
                hunk.get_or_insert((oi..oi, ni..ni)).1.end = new.end;
                ni = new.end;
            }
        }
    }
    if let Some((ro, rn)) = hunk {
        out.push(text_change(a, b, ro, rn));
    }
    out
}

fn text_change(a: &[Word], b: &[Word], old: Range<usize>, new: Range<usize>) -> Change {
    let (kind, anchor) = match (old.is_empty(), new.is_empty()) {
        (true, _) => (
            ChangeKind::Inserted,
            anchor_at(a, old.start).map(|p| (Side::Old, p)),
        ),
        (_, true) => (
            ChangeKind::Deleted,
            anchor_at(b, new.start).map(|p| (Side::New, p)),
        ),
        _ => (ChangeKind::Replaced, None),
    };
    change(kind, a, b, old, new, anchor, None)
}

fn change(
    kind: ChangeKind,
    a: &[Word],
    b: &[Word],
    old: Range<usize>,
    new: Range<usize>,
    anchor: Option<(Side, Place)>,
    formatting: Option<FormattingDelta>,
) -> Change {
    Change {
        kind,
        old_text: join(span(a, &old)),
        new_text: join(span(b, &new)),
        old_places: places(span(a, &old)),
        new_places: places(span(b, &new)),
        old_words: old,
        new_words: new,
        anchor,
        formatting,
    }
}

/// `words[r]`, or empty when out of bounds (crate lint policy: no
/// panicking index on paths fed by untrusted input).
fn span<'w>(words: &'w [Word], r: &Range<usize>) -> &'w [Word] {
    words.get(r.clone()).unwrap_or(&[])
}

/// Emit one formatting change per maximal run of equal words whose style
/// differs in the same way.
fn formatting_changes(
    a: &[Word],
    b: &[Word],
    old: usize,
    new: usize,
    len: usize,
    out: &mut Vec<Change>,
) {
    let delta_at = |i: usize| {
        let style = |w: &[Word], at: usize| w.get(at).and_then(|w| w.style);
        style_delta(style(a, old + i), style(b, new + i))
    };
    let mut i = 0;
    while i < len {
        let Some(delta) = delta_at(i) else {
            i += 1;
            continue;
        };
        let start = i;
        while i < len && delta_at(i) == Some(delta) {
            i += 1;
        }
        let (ro, rn) = (old + start..old + i, new + start..new + i);
        out.push(change(
            ChangeKind::Formatting,
            a,
            b,
            ro,
            rn,
            None,
            Some(delta),
        ));
    }
}

fn style_delta(a: Option<Style>, b: Option<Style>) -> Option<FormattingDelta> {
    let (a, b) = (a?, b?);
    let delta = FormattingDelta {
        size: ((a.size - b.size).abs() > SIZE_TOLERANCE).then_some((a.size, b.size)),
        weight: (a.weight != b.weight).then_some((a.weight, b.weight)),
        color: (!same_color(a.color, b.color)).then_some((a.color, b.color)),
    };
    (delta.size.is_some() || delta.weight.is_some() || delta.color.is_some()).then_some(delta)
}

fn same_color(a: Option<TextColor>, b: Option<TextColor>) -> bool {
    let close = |x: &[f32], y: &[f32]| {
        x.iter()
            .zip(y)
            .all(|(p, q)| (p - q).abs() < COLOR_TOLERANCE)
    };
    // No colour set is the initial DeviceGray black (ISO 32000-2 Table 51).
    let a = a.unwrap_or(TextColor::Gray(0.0));
    let b = b.unwrap_or(TextColor::Gray(0.0));
    match (a, b) {
        (TextColor::Gray(x), TextColor::Gray(y)) => close(&[x], &[y]),
        (TextColor::Rgb(r, g, bl), TextColor::Rgb(r2, g2, b2)) => close(&[r, g, bl], &[r2, g2, b2]),
        (TextColor::Cmyk(c, m, y, k), TextColor::Cmyk(c2, m2, y2, k2)) => {
            close(&[c, m, y, k], &[c2, m2, y2, k2])
        }
        (TextColor::Other, TextColor::Other) => true,
        _ => false,
    }
}

fn join(words: &[Word]) -> String {
    let mut s = String::new();
    for w in words {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&w.text);
    }
    s
}

/// The words' rectangles, merged per line: a word joins the previous place
/// when it is on the same page and overlaps it vertically by at least half
/// the shorter height.
fn places(words: &[Word]) -> Vec<Place> {
    let mut out: Vec<Place> = Vec::new();
    for w in words {
        let Some(rect) = w.rect else { continue };
        if let Some(last) = out.last_mut()
            && last.page == w.page
            && same_line(&last.rect, &rect)
        {
            last.rect = union(Some(last.rect), Some(rect)).unwrap_or(rect);
            continue;
        }
        out.push(Place { page: w.page, rect });
    }
    out
}

fn same_line(a: &Rect, b: &Rect) -> bool {
    let overlap = a.ury.min(b.ury) - a.lly.max(b.lly);
    overlap >= 0.5 * a.height().min(b.height())
}

/// A zero-width place at the left edge of word `at`, else at the right
/// edge of the nearest positioned word before it.
fn anchor_at(words: &[Word], at: usize) -> Option<Place> {
    let edge = |w: &Word, x: f64| {
        w.rect.map(|r| Place {
            page: w.page,
            rect: Rect {
                llx: x,
                urx: x,
                ..r
            },
        })
    };
    if let Some(w) = words.get(at)
        && let Some(r) = w.rect
    {
        return edge(w, r.llx);
    }
    words
        .iter()
        .take(at)
        .rev()
        .find_map(|w| w.rect.and_then(|r| edge(w, r.urx)))
}

//! The page's invisible text and a recognised block's words.

use std::collections::HashMap;

use crate::page_tree::Rect;
use crate::text_edit::edit::{OpRec, Rec};
use crate::text_edit::model::{
    Block, EditableTextModel, GlyphRef, derived_space_between, glyph_text,
};
use crate::text_extract::{PageText, TextOrigin, TextRun};

/// One word of a block: its glyphs (in content order) and where they sit.
pub(super) struct Word {
    pub(super) text: String,
    pub(super) bbox: Rect,
    /// The block line (0 = top) it is on.
    pub(super) line: usize,
    pub(super) glyphs: Vec<GlyphRef>,
}

/// `page` with only its invisible text: a glyph run is kept when every glyph
/// is invisible (render mode 3 or 7), a derived space or line break when the
/// glyph runs either side of it are kept. Visible text cannot join a
/// recognised block, so a word over a printed caption stays a word.
pub(super) fn invisible_only(page: &PageText) -> PageText {
    let glyph_kept: Vec<Option<bool>> = page
        .runs
        .iter()
        .map(|r| (!r.glyphs.is_empty()).then(|| r.glyphs.iter().all(|g| g.invisible)))
        .collect();
    let mut out = page.clone();
    out.runs = page
        .runs
        .iter()
        .enumerate()
        .filter(|(i, r)| match glyph_kept.get(*i).copied().flatten() {
            Some(k) => k,
            None => derived(r) && neighbours_kept(&glyph_kept, *i),
        })
        .map(|(_, r)| r.clone())
        .collect();
    out
}

fn derived(r: &TextRun) -> bool {
    matches!(
        r.origin,
        TextOrigin::DerivedWordSpace | TextOrigin::DerivedLineBreak
    )
}

fn neighbours_kept(kept: &[Option<bool>], i: usize) -> bool {
    let before = kept.get(..i).and_then(|k| k.iter().rev().find_map(|x| *x));
    let after = kept.get(i + 1..).and_then(|k| k.iter().find_map(|x| *x));
    before == Some(true) && after == Some(true)
}

/// `block`'s words in reading order. A word ends at whitespace, at a space
/// the extractor derived from a gap, and where its show operators are
/// separated by an explicit position (`Tm`, `Td`) or `ET`: an OCR layer
/// places each word itself, so two words whose boxes touch stay two.
pub(super) fn block_words(
    model: &EditableTextModel<'_>,
    page: &PageText,
    block: &Block,
    recs: &[OpRec],
) -> Vec<Word> {
    let by_end: HashMap<usize, usize> = recs
        .iter()
        .enumerate()
        .filter(|(_, r)| matches!(r.rec, Rec::Show(_)))
        .map(|(i, r)| (r.end, i))
        .collect();
    let mut out = Vec::new();
    for (line_no, &li) in block.line_indices.iter().enumerate() {
        let Some(line) = model.lines().get(li) else {
            continue;
        };
        let mut cur: Option<Word> = None;
        let mut prev: Option<GlyphRef> = None;
        for &g in &line.glyphs {
            let t = glyph_text(page, g);
            let split = t.trim().is_empty()
                || prev.is_some_and(|p| {
                    derived_space_between(page, p, g)
                        || positioned_apart(model, recs, &by_end, p, g)
                });
            if split {
                out.extend(cur.take());
            }
            prev = Some(g);
            if t.trim().is_empty() {
                continue;
            }
            let Some(cell) = model
                .glyph(g)
                .map(crate::text_extract::ExtractedGlyph::cell)
            else {
                continue;
            };
            let w = cur.get_or_insert_with(|| Word {
                text: String::new(),
                bbox: cell,
                line: line_no,
                glyphs: Vec::new(),
            });
            w.text.push_str(t);
            w.bbox = union(w.bbox, cell);
            w.glyphs.push(g);
        }
        out.extend(cur);
    }
    out
}

/// Whether an explicit position or `ET` lies between the operators showing
/// `a` and `b`.
fn positioned_apart(
    model: &EditableTextModel<'_>,
    recs: &[OpRec],
    by_end: &HashMap<usize, usize>,
    a: GlyphRef,
    b: GlyphRef,
) -> bool {
    let (Some(pa), Some(pb)) = (model.provenance(a), model.provenance(b)) else {
        return true;
    };
    if pa.content_stream != pb.content_stream {
        return true;
    }
    let (ea, eb) = (pa.operator_span.end(), pb.operator_span.end());
    if ea == eb {
        return false;
    }
    let (Some(&ia), Some(&ib)) = (by_end.get(&ea), by_end.get(&eb)) else {
        return true;
    };
    if ib < ia {
        return true;
    }
    recs.get(ia + 1..ib).is_none_or(|between| {
        between
            .iter()
            .any(|r| matches!(r.rec, Rec::Tm(_) | Rec::Td { .. } | Rec::EndText))
    })
}

fn union(a: Rect, b: Rect) -> Rect {
    Rect {
        llx: a.llx.min(b.llx),
        lly: a.lly.min(b.lly),
        urx: a.urx.max(b.urx),
        ury: a.ury.max(b.ury),
    }
}

/// The words as one string: single spaces within a line, `\n` between.
pub(super) fn joined(words: &[Word]) -> String {
    let mut out = String::new();
    let mut line = None;
    for w in words {
        match line {
            Some(l) if l == w.line => out.push(' '),
            Some(_) => out.push('\n'),
            None => {}
        }
        out.push_str(&w.text);
        line = Some(w.line);
    }
    out
}

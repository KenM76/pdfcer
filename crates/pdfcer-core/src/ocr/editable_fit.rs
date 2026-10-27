//! Word boxes for the editable output, refitted to the measured ink.
//!
//! An engine box is padded and clips glyphs, so sizing text from it writes
//! letters too tall and then squeezed to width. Each line's visible words
//! take their x-extent from their own ink and share one baseline and cap
//! height measured across the line (`ocr__editable_text_and_images_mode.md`).

use super::OcrPage;
use super::editable::Analysis;
use super::layer::{HELVETICA_ASCENT_FRAC, HELVETICA_DESCENT_FRAC};
use super::layer_content::WordStyle;
use super::structure::resolve;
use crate::page_tree::Rect;

/// A fitted size outside this band of the box-derived size is a measurement
/// gone wrong (a stray mark joined to the ink), so the line keeps its boxes.
/// Low because engines pad: PaddleOCR's boxes on 12 pt text imply 22 pt.
const SIZE_BAND: (f64, f64) = (0.4, 1.2);

/// `page` with each visible word's box replaced by its ink-fitted one, or
/// `None` when nothing was refitted. Unrotated pages only: a rotated page's
/// lines run along another axis.
pub(super) fn fitted(a: &Analysis, page: &OcrPage, rotate: u16) -> Option<OcrPage> {
    if rotate != 0 {
        return None;
    }
    let (blocks, _) = resolve(page);
    let mut out = page.clone();
    let mut changed = false;
    for line in blocks.iter().flat_map(|b| b.lines.iter()) {
        changed |= fit_line(a, page, line, &mut out);
    }
    changed.then_some(out)
}

fn fit_line(a: &Analysis, page: &OcrPage, line: &[usize], out: &mut OcrPage) -> bool {
    let em = HELVETICA_ASCENT_FRAC + HELVETICA_DESCENT_FRAC;
    let inks: Vec<(usize, Rect)> = line
        .iter()
        .filter(|&&w| matches!(a.styles.get(w), Some(WordStyle::Visible(_))))
        .filter_map(|&w| a.ink_rect(w).map(|r| (w, r)))
        .collect();
    if inks.is_empty() {
        return false;
    }
    // Words whose letters reach cap height and none descends measure the
    // size directly; each is measured on its own, so a skewed scan's drift
    // along the line cancels.
    let mut sizes: Vec<f64> = Vec::new();
    let mut bottoms: Vec<f64> = Vec::new();
    for (w, r) in &inks {
        let Some(word) = page.words.get(*w) else {
            continue;
        };
        if !word.text.chars().any(descends) {
            bottoms.push(r.lly);
        }
        if word.text.chars().any(reaches_cap) && !word.text.chars().any(descends) {
            sizes.push((r.ury - r.lly) / HELVETICA_ASCENT_FRAC);
        }
    }
    let (Some(size), Some(baseline)) = (median(&mut sizes), median(&mut bottoms)) else {
        return false;
    };
    let mut boxed: Vec<f64> = line
        .iter()
        .filter_map(|&w| page.words.get(w))
        .map(|w| (w.rect.ury - w.rect.lly) / em)
        .collect();
    let Some(boxed) = median(&mut boxed) else {
        return false;
    };
    if !(size >= SIZE_BAND.0 * boxed && size <= SIZE_BAND.1 * boxed) {
        return false;
    }
    let (lly, ury) = (
        baseline - HELVETICA_DESCENT_FRAC * size,
        baseline + HELVETICA_ASCENT_FRAC * size,
    );
    for &w in line {
        let Some(word) = out.words.get_mut(w) else {
            continue;
        };
        let (llx, urx) = inks
            .iter()
            .find(|(i, _)| *i == w)
            .map_or((word.rect.llx, word.rect.urx), |(_, r)| (r.llx, r.urx));
        word.rect = Rect::from_corners(llx, lly, urx, ury);
    }
    true
}

/// Letters whose Helvetica outline tops out at the cap height.
fn reaches_cap(c: char) -> bool {
    c.is_ascii_uppercase() && c != 'Q' || "bdfhikl".contains(c)
}

/// Letters and marks drawn below the baseline.
fn descends(c: char) -> bool {
    "gjpqyQ,;()[]{}|/_@$".contains(c) || !c.is_ascii()
}

fn median(v: &mut [f64]) -> Option<f64> {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied()
}

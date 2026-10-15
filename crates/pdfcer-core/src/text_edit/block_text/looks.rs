//! Which look each character of a block's new text is set in. A character
//! that survives the edit keeps its own look; an inserted one takes the look
//! of the character before it, or after it at the start. Survival is a diff
//! of the old and new texts' non-space characters (G199).

use crate::span::ByteSpan;
use crate::text_edit::model::{Block, EditableTextModel, glyph_text};
use crate::text_edit::reflow_style::style_of;
use crate::text_edit::reflow_walk::{BlockRegion, SpanStyle};

use super::BlockEditError;

/// Above this many cells (old × new changed characters) the middle of the
/// diff is not aligned and every changed character inherits instead. Bounds
/// the table at 16 MB.
const MAX_DIFF_CELLS: usize = 4_000_000;

/// The block's distinct looks and the look of each new character.
pub(super) struct Looks {
    /// Distinct looks in reading order; index 0 is the first glyph's.
    pub(super) styles: Vec<SpanStyle>,
    /// Each look's first show operator in the block.
    pub(super) anchors: Vec<ByteSpan>,
    /// Per character of the new flat text, an index into `styles`.
    pub(super) of_char: Vec<usize>,
    /// New non-space characters with no counterpart in the old text, set
    /// in a neighbour's look.
    pub(super) inherited: usize,
}

/// The block's non-space characters in reading order, each with its look
/// index, and the looks with their first show operators.
struct OldText {
    chars: Vec<(char, usize)>,
    styles: Vec<SpanStyle>,
    anchors: Vec<ByteSpan>,
}

fn old_chars(
    model: &EditableTextModel<'_>,
    block: &Block,
    region: &BlockRegion,
) -> Result<OldText, BlockEditError> {
    let (mut chars, mut styles, mut anchors) = (Vec::new(), Vec::<SpanStyle>::new(), Vec::new());
    let lines = block
        .line_indices
        .iter()
        .filter_map(|&li| model.lines().get(li));
    for gref in lines.flat_map(|l| l.glyphs.iter().copied()) {
        let text = glyph_text(model.sourced_view(), gref);
        if text.trim().is_empty() {
            continue;
        }
        let style = style_of(model, region, gref)?;
        let look = match styles.iter().position(|s| s.same_look(style)) {
            Some(i) => i,
            None => {
                let span = model
                    .provenance(gref)
                    .ok_or(crate::text_edit::ReflowApplyError::NoProvenance)?
                    .operator_span;
                styles.push(style.clone());
                anchors.push(span);
                styles.len() - 1
            }
        };
        chars.extend(
            text.chars()
                .filter(|c| !c.is_whitespace())
                .map(|c| (c, look)),
        );
    }
    Ok(OldText {
        chars,
        styles,
        anchors,
    })
}

/// Assign a look to every character of `flat` (words joined by single
/// spaces). A space takes the look of the character before it.
///
/// # Errors
///
/// A block glyph whose show operator the region walk did not record.
pub(super) fn assign(
    model: &EditableTextModel<'_>,
    block: &Block,
    region: &BlockRegion,
    flat: &[char],
) -> Result<Looks, BlockEditError> {
    let OldText {
        chars: old,
        styles,
        anchors,
    } = old_chars(model, block, region)?;
    let new: Vec<char> = flat.iter().copied().filter(|c| *c != ' ').collect();
    let old_text: Vec<char> = old.iter().map(|&(c, _)| c).collect();
    let matched = align(&old_text, &new);
    let inherited = matched.iter().filter(|m| m.is_none()).count();
    let known: Vec<Option<usize>> = matched
        .iter()
        .map(|m| m.and_then(|i| old.get(i)).map(|&(_, look)| look))
        .collect();
    let looks = fill(&known);
    let mut of_char = Vec::with_capacity(flat.len());
    let mut next = looks.iter();
    let mut last = looks.first().copied().unwrap_or(0);
    for &c in flat {
        if c != ' ' {
            last = next.next().copied().unwrap_or(last);
        }
        of_char.push(last);
    }
    Ok(Looks {
        styles,
        anchors,
        of_char,
        inherited,
    })
}

/// Each unknown look takes the previous known one, or the next at the
/// start; with none known, look 0.
fn fill(known: &[Option<usize>]) -> Vec<usize> {
    let first = known.iter().flatten().next().copied().unwrap_or(0);
    let mut cur = first;
    known
        .iter()
        .map(|k| {
            if let Some(l) = k {
                cur = *l;
            }
            cur
        })
        .collect()
}

/// For each character of `new`, the index of the `old` character it
/// survives from: common prefix and suffix, then a longest common
/// subsequence of the middle when it fits [`MAX_DIFF_CELLS`].
fn align(old: &[char], new: &[char]) -> Vec<Option<usize>> {
    let mut out = vec![None; new.len()];
    let pre = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let (old_rest, new_rest) = (old.get(pre..).unwrap_or(&[]), new.get(pre..).unwrap_or(&[]));
    let suf = old_rest
        .iter()
        .rev()
        .zip(new_rest.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    for (i, slot) in out.iter_mut().enumerate().take(pre) {
        *slot = Some(i);
    }
    let (old_suf, new_suf) = (old.len() - suf, new.len() - suf);
    for (k, slot) in out.iter_mut().enumerate().skip(new_suf) {
        *slot = Some(old_suf + (k - new_suf));
    }
    let o = old.get(pre..old_suf).unwrap_or(&[]);
    let n = new.get(pre..new_suf).unwrap_or(&[]);
    if o.len().saturating_mul(n.len()) <= MAX_DIFF_CELLS {
        for (oi, ni) in lcs(o, n) {
            if let Some(slot) = out.get_mut(pre + ni) {
                *slot = Some(pre + oi);
            }
        }
    }
    out
}

/// The matched index pairs of a longest common subsequence of `a` and `b`.
fn lcs(a: &[char], b: &[char]) -> Vec<(usize, usize)> {
    let w = b.len() + 1;
    let mut t = vec![0u32; (a.len() + 1) * w];
    let at = |t: &[u32], i: usize, j: usize| t.get(i * w + j).copied().unwrap_or(0);
    for (i, ca) in a.iter().enumerate().rev() {
        for (j, cb) in b.iter().enumerate().rev() {
            let v = if ca == cb {
                at(&t, i + 1, j + 1) + 1
            } else {
                at(&t, i + 1, j).max(at(&t, i, j + 1))
            };
            if let Some(cell) = t.get_mut(i * w + j) {
                *cell = v;
            }
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while let (Some(ca), Some(cb)) = (a.get(i), b.get(j)) {
        if ca == cb {
            out.push((i, j));
            i += 1;
            j += 1;
        } else if at(&t, i + 1, j) >= at(&t, i, j + 1) {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn surviving_characters_keep_their_old_index() {
        let m = align(&chars("abXcd"), &chars("abYYcd"));
        assert_eq!(m, [Some(0), Some(1), None, None, Some(3), Some(4)]);
    }

    #[test]
    fn a_moved_character_matches_once() {
        let m = align(&chars("abc"), &chars("cab"));
        assert_eq!(m, [None, Some(0), Some(1)]);
    }

    #[test]
    fn an_insertion_takes_the_look_before_it_or_after_it_at_the_start() {
        assert_eq!(fill(&[None, Some(2), None, Some(1)]), [2, 2, 2, 1]);
        assert_eq!(fill(&[None, None]), [0, 0]);
    }
}

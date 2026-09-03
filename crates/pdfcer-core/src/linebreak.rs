//! # `linebreak` — the shared greedy first-fit line breaker
//!
//! ## Why this module exists (one breaker, two callers)
//!
//! Two very different subsystems in `pdfcer-core` need to pack a stream of
//! words into lines that fit a width:
//!
//! 1. **[`crate::vartext`]** — variable-text appearance generation
//!    (§12.7.3.3): it packs `WinAnsi` bytes measured by **standard-14 AFM
//!    advance widths** into a widget/FreeText appearance box.
//! 2. **[`crate::text_edit::reflow`]** — within-block offline reflow, FF-A
//!    (`docs/decisions/015-ffa-within-block-offline-reflow.md` §3.2): it
//!    re-wraps a recognized [`Block`](crate::text_edit::Block)'s words
//!    measured by the **real, per-glyph §9.4.4 advances carried in
//!    provenance** (embedded/supplied font `Widths`, not AFM estimates).
//!
//! The *measurement* differs (AFM bytes vs. provenance advances), but the
//! *packing algorithm* is identical, and decision 015 §3.2 (option
//! **VT-extend**) is explicit that there must be **one** greedy breaker
//! taking a **width-measuring closure**, not two hand-rolled copies that
//! can drift. This module is that single breaker; [`greedy_pack`] is the
//! packing core, factored verbatim out of `vartext::wrap_lines`.
//!
//! ## The algorithm — greedy / first-fit (ISO context, not ISO mandate)
//!
//! Greedy first-fit: walk the words left to right, keep adding the next
//! word to the current line for as long as the whole line still measures
//! `≤ max_width`; the first word that would overflow starts a new line.
//! This is **first-fit**, not Knuth-Plass total-fit — a deliberate,
//! low-cost choice (decision 015 §3.2, **LB-greedy**): Acrobat publishes
//! no line-breaking algorithm, so pdfcer is matching no documented
//! behaviour and greedy is the honest, cheap option. Knuth-Plass is a
//! named deferral (decision 015 §7).
//!
//! Two invariants make it total and panic-free:
//!
//! - **The current line always holds at least one word.** The breaker
//!   never tests "does word *i* alone fit?" before placing it — the first
//!   word of every line is placed unconditionally, then growth is tested.
//! - **An oversized single word becomes its own overflowing line.** A word
//!   wider than `max_width` cannot be split (no hyphenation — decision 015
//!   §3.2, whitespace-U+0020 breaks only), so it is emitted alone on one
//!   line that overflows the box. The caller discloses the overflow
//!   (`vartext` clips to the `/BBox`; reflow discloses it as a reviewable
//!   condition — decision 015 §3.5 / rule 4). Emitting it alone rather than
//!   looping forever is what keeps the loop total.
//!
//! ## Measurement contract (§9.4.4)
//!
//! The caller supplies `line_width(start, end)`: the natural width, in the
//! caller's own length unit, of the line formed by **`words[start..end]`
//! joined by single inter-word gaps**. "Natural" means at the source
//! spacing — before any justified slack redistribution (that is the
//! reflow engine's later concern, decision 015 §3.1, and does not change
//! *where* the greedy breaks fall). The breaker treats the returned number
//! as opaque: it only ever compares it to `max_width`, so the unit is
//! whatever the caller measures in (text-space points for both current
//! callers). Per §9.4.4 an advance is `Σ width/1000 × size` (plus `Tc`/
//! `Tw`/`Tz`); the closure owns that arithmetic, the breaker owns only the
//! fit decision.
//!
//! ## What this module is NOT
//!
//! No paragraph splitting (the caller splits on `\n` first — a `Block` is
//! one paragraph, and `vartext` splits its own multiline value), no
//! hyphenation, no CJK per-glyph breaking, no bidi. Pure index arithmetic
//! over a word count; it never sees the words themselves, only their
//! measured widths — which is exactly what lets the same code serve bytes
//! and glyph runs.

use core::ops::Range;

/// Greedily pack `word_count` words into first-fit lines under `max_width`.
///
/// Returns one [`Range<usize>`] per output line: `start..end` are the
/// half-open word indices on that line, in order, covering `0..word_count`
/// exactly once with no gaps and no overlaps. An empty input
/// (`word_count == 0`) returns no lines — the caller decides whether an
/// empty paragraph should still emit a blank line (`vartext` does; reflow
/// refuses an empty block earlier).
///
/// `line_width(start, end)` measures the natural width of
/// `words[start..end]` joined by single inter-word gaps (see the module
/// docs' measurement contract). It is called `O(word_count)` times in the
/// common case and at most `O(word_count²)` on a pathological all-overflow
/// input — acceptable for the small word counts both callers see (an
/// appearance value, a single recognized paragraph), and never a source of
/// non-termination because each iteration advances `end`.
///
/// # Algorithm
///
/// The current line is the half-open range `start..end`, always holding at
/// least one word (`end > start`). For each next word at index `end`:
///
/// - if `line_width(start, end + 1) ≤ max_width`, the word fits — extend
///   the line (`end += 1`);
/// - otherwise the word overflows — close the current line (`push
///   start..end`), and begin a fresh line at that word (`start = end; end =
///   start + 1`), which places it unconditionally (so an oversized single
///   word lands alone on its own overflowing line).
///
/// The final in-progress line is always pushed. This is byte-for-byte the
/// packing `vartext::wrap_lines` performed inline before the factor-out, so
/// the standard-14 appearance path is unchanged (its tests pass verbatim).
///
/// # Examples
///
/// ```
/// use pdfcer_core::linebreak::greedy_pack;
///
/// // Five unit-width words, single-space gaps, box wide enough for two
/// // words plus their gap (width 3) but not three (width 5).
/// let widths = [1.0_f64; 5];
/// let lines = greedy_pack(widths.len(), 3.0, |s, e| {
///     let words: f64 = widths[s..e].iter().sum();
///     let gaps = (e - s - 1) as f64; // one unit gap between neighbours
///     words + gaps
/// });
/// assert_eq!(lines, vec![0..2, 2..4, 4..5]);
/// ```
///
/// ```
/// use pdfcer_core::linebreak::greedy_pack;
///
/// // A single word wider than the box lands alone on an overflowing line.
/// let widths = [10.0_f64, 1.0];
/// let lines = greedy_pack(widths.len(), 3.0, |s, e| {
///     let words: f64 = widths[s..e].iter().sum();
///     words + (e - s - 1) as f64
/// });
/// assert_eq!(lines, vec![0..1, 1..2]);
/// ```
#[must_use]
pub fn greedy_pack<F>(word_count: usize, max_width: f64, mut line_width: F) -> Vec<Range<usize>>
where
    F: FnMut(usize, usize) -> f64,
{
    let mut lines: Vec<Range<usize>> = Vec::new();
    if word_count == 0 {
        return lines;
    }
    // The current line is words[start..end]; it always holds ≥ 1 word.
    let mut start = 0usize;
    let mut end = 1usize;
    while end < word_count {
        // Would adding words[end] keep the whole line within the box?
        if line_width(start, end + 1) <= max_width {
            end += 1;
        } else {
            lines.push(start..end);
            start = end;
            end = start + 1;
        }
    }
    lines.push(start..end);
    lines
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests {
    use super::*;

    /// A measurer for unit-width words joined by unit gaps.
    fn unit(widths: &[f64]) -> impl FnMut(usize, usize) -> f64 + '_ {
        move |s: usize, e: usize| {
            let w: f64 = widths[s..e].iter().sum();
            w + (e - s).saturating_sub(1) as f64
        }
    }

    #[test]
    fn empty_input_makes_no_lines() {
        assert!(greedy_pack(0, 100.0, |_, _| 0.0).is_empty());
    }

    #[test]
    fn a_single_word_is_one_line_even_if_it_overflows() {
        let widths = [999.0];
        let lines = greedy_pack(1, 3.0, unit(&widths));
        assert_eq!(lines, vec![0..1]);
    }

    #[test]
    fn greedy_breaks_at_the_first_word_that_overflows() {
        // widths 1 each, gaps 1: two words = 3 (fits 3), three = 5 (no).
        let widths = [1.0; 5];
        let lines = greedy_pack(5, 3.0, unit(&widths));
        assert_eq!(lines, vec![0..2, 2..4, 4..5]);
    }

    #[test]
    fn an_oversized_interior_word_lands_alone() {
        // "a BIG b": the wide middle word cannot share a line.
        let widths = [1.0, 10.0, 1.0];
        let lines = greedy_pack(3, 3.0, unit(&widths));
        assert_eq!(lines, vec![0..1, 1..2, 2..3]);
    }

    #[test]
    fn everything_fits_on_one_line_when_the_box_is_wide() {
        let widths = [1.0; 4];
        let lines = greedy_pack(4, 1000.0, unit(&widths));
        assert_eq!(lines, vec![0..4]);
    }

    #[test]
    fn output_ranges_tile_the_whole_word_range() {
        // Whatever the widths, the union of the ranges is exactly 0..n with
        // no gaps or overlaps — the property every caller relies on.
        let widths = [2.0, 2.0, 5.0, 1.0, 1.0, 9.0, 1.0];
        let lines = greedy_pack(widths.len(), 4.0, unit(&widths));
        let mut next = 0usize;
        for r in &lines {
            assert_eq!(r.start, next, "no gap/overlap: {lines:?}");
            assert!(r.end > r.start, "no empty line: {lines:?}");
            next = r.end;
        }
        assert_eq!(next, widths.len(), "covers every word: {lines:?}");
    }
}

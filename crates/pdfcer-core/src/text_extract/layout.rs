//! # Derived layout — where pdfcer guesses, visibly
//!
//! Turns the flat, geometry-carrying item list produced by
//! [`super::page`] into [`super::TextRun`]s, inserting the word spaces
//! and line breaks that the page's *appearance* implies and the page's
//! *content* does not state.
//!
//! **Everything this module decides is DERIVED.** That is not modesty;
//! it is the sourced position of ISO 32000-1, stated across six negative
//! results in `iso32000__s__14.8.md`:
//!
//! - **S1** — the term "word break" appears only in §14.8/§14.9. Clause
//!   9, which defines everything about showing text, never mentions
//!   words at all.
//! - **S2** — there is **no interword-space guarantee outside Tagged
//!   PDF**. §14.8.2.5's guarantee ("the spacing characters that would be
//!   present to separate words in a pure text representation shall be
//!   present") is a *Tagged PDF writer* obligation, and its NOTE 2
//!   states the payoff as "the conforming reader does not need to guess
//!   about word breaks" — i.e. in an untagged file, guessing is exactly
//!   what is left.
//! - **S3** — a space may be a glyph, a `TJ` offset, a `Td`/`Tm` jump, a
//!   new `BT` block, or nothing at all. The standard assigns word-break
//!   meaning to **none** of them.
//! - **S4** — `TJ` negative-offset thresholds are reader heuristics with
//!   **zero** spec basis. Table 109 defines the numbers purely as a text
//!   matrix translation in thousandths of a text-space unit, and the
//!   standard's own illustration of them is *kerning*
//!   (`[(A) 120 (W) 120 …]`), an intra-word use of the same mechanism
//!   this module reads as an inter-word signal.
//! - **S5** — **no line or paragraph markers exist in a content
//!   stream**, in a tagged document either.
//! - **S9** — for an untagged document, no definition of *word*, *line*,
//!   *paragraph*, *column* or *reading order* exists anywhere in the
//!   standard.
//!
//! So this module does the only thing left: it measures gaps, applies
//! three ratios with no provenance beyond "these work", **counts every
//! decision**, and marks every character it invents with a
//! [`super::TextOrigin`] variant that says so. A caller who wants none
//! of it calls [`super::ExtractedText::sourced_text`] and gets exactly
//! the characters the file provides.
//!
//! ## The four rules, in evaluation order
//!
//! Between two consecutive glyphs with geometry:
//!
//! 0. **Direction changed** by more than about two degrees ⇒ derived
//!    line break. Text that runs a different way is a different line,
//!    whatever the gap; and this is what guarantees that every glyph in
//!    a run shares one direction, which is what makes
//!    [`super::TextRun::direction`] answerable at all.
//! 1. **Baseline moved** — the component of the step *perpendicular to
//!    the writing direction* — by more than `line_gap_ratio` ×
//!    effective font size ⇒ derived line break.
//! 2. **Backward jump** along the writing direction larger than
//!    `backward_jump_ratio` × size ⇒ derived line break. Without this a
//!    two-column page whose columns share baselines runs the columns
//!    together with no separator at all; §14.8.2.3.1 makes column
//!    ordering derived in an untagged file, so this is a guess about
//!    something the standard declines to define.
//! 3. **Forward gap** along the writing direction larger than
//!    `word_gap_ratio` × size ⇒ derived word space.
//!
//! ## ★ The frame those rules are measured in (`Pass 139.1`)
//!
//! Rules 1–3 are measured **in the line's own frame**, not in the
//! page's. Until `Pass 139.1` they were stated in page axes — `|Δy|`
//! for the baseline, `Δx` for the two gaps — and that assumption is
//! true of virtually every word-processor page and **false of every CAD
//! title block**, which stamps its source path with a rotated `Tm`.
//!
//! A rotated line failed *both* clauses, independently: at 90° the whole
//! advance lands in `Δy` and trips the baseline test, and at 180° the
//! step is in `−x` while [`super::ExtractedGlyph::advance`] is a
//! positive magnitude, so the backward-jump test sees `≈ −2·advance`.
//! Either way the verdict was a line break **between every letter** —
//! measured at 71 breaks across one 82-glyph line, which pasted into an
//! editor as one character per line.
//!
//! No ratio changed and no threshold was retuned. The step from the
//! previous glyph's end is resolved into the frame as
//! `along = d · dir`, `perp = d × dir`; for `dir = (1, 0)` those are the
//! previous expressions term for term, so a horizontal page's output is
//! **byte-identical**.
//!
//! The one thing that does *not* generalise is the `/ActualText`
//! boundary test, which stays in page axes because a replacement
//! publishes an axis-aligned box and no baseline at all — named at its
//! call site rather than left to be discovered.
//!
//! Note what is *not* in that list: `Tw`. §9.3.3 applies word spacing
//! only to the single-byte code 32, so it is **inert under
//! `Identity-H`** — that is, inert in every modern subsetted document
//! (**S6**). Reading `Tw` as a word-break signal would work on 1997
//! files and silently fail on everything since.
//!
//! ## Why a real space glyph is never doubled
//!
//! When a document *does* encode its spaces as glyphs — the common case
//! for a simple font, and available to a composite font too since a
//! `ToUnicode` destination may legitimately be U+0020 — a derived space
//! next to it would produce `Hello  world`. Rule 3 is therefore
//! suppressed whenever either side already ends or begins with
//! whitespace. This is a heuristic guarding a heuristic, and it is why
//! the sourced/derived split is kept at the *run* level rather than
//! being reconstructed after the fact from the output string.
//!
//! ## `/ActualText` boundaries
//!
//! §14.9.4 NOTE 2 makes `ActualText` "a **character** substitution",
//! contrasted explicitly with `Alt`'s "whole word or phrase
//! substitution", and §14.9.4 requires that two consecutive
//! `ActualText` sequences "be treated as if **no word break** is present
//! between them". Both point the same way: an `ActualText` run sits
//! *inside* a word — the standard's own examples are a ligature and a
//! hyphenation that changes spelling — so **no derived word space is
//! ever inserted adjacent to one**. Derived *line* breaks still are: a
//! baseline that moved really did move, and §14.9.4's `Drucker` example
//! is precisely a word split across two lines.
//!
//! That combination is what makes the `Drucker` example come out right
//! in both accessors: `sourced_text()` is `Drucker` exactly as the
//! standard glosses it, while `plain_text()` is `Dru` `c` ⏎ `ker` —
//! the same characters plus one clearly-labelled derived line break.

use crate::page_tree::Rect;

use super::page::{GlyphItem, Item};
use super::{ArtifactKind, ExtractOptions, ExtractedGlyph, TextDiagnostics, TextOrigin, TextRun};

/// The geometry of the last emitted glyph, for gap analysis.
#[derive(Debug, Clone, Copy)]
struct Cursor {
    /// x of the **end** of the previous glyph — its origin advanced by
    /// one `advance` **along [`Self::dir`]**, not along the page x axis.
    end_x: f32,
    /// y of the end of the previous glyph, likewise.
    end_y: f32,
    /// Effective font size of the previous glyph.
    size: f32,
    /// Writing direction of the previous glyph, as a unit vector in
    /// default user space. The frame every threshold below is measured
    /// in (`Pass 139.1`).
    dir: (f32, f32),
}

/// What the gap between two items implies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Break {
    /// The glyphs are adjacent; emit nothing.
    None,
    /// A word space, if word spaces are allowed at this boundary.
    Word,
    /// A line break.
    Line,
}

/// Build the run list, inserting derived whitespace.
pub(super) fn assemble(
    items: Vec<Item>,
    options: &ExtractOptions,
    diagnostics: &mut TextDiagnostics,
) -> Vec<TextRun> {
    let mut builder = Builder {
        runs: Vec::new(),
        open: None,
        cursor: None,
        at_actual_text_boundary: false,
        options,
    };

    for item in items {
        match item {
            Item::Glyph(g) => builder.push_glyph(g, diagnostics),
            Item::Replacement(r) => {
                builder.push_replacement(r, diagnostics);
            }
        }
    }
    builder.finish(diagnostics)
}

/// An in-progress [`TextOrigin::Glyphs`] run.
struct OpenRun {
    text: String,
    glyphs: Vec<ExtractedGlyph>,
    artifact: Option<ArtifactKind>,
    mcid: Option<u32>,
    llx: f32,
    lly: f32,
    urx: f32,
    ury: f32,
}

struct Builder<'a> {
    runs: Vec<TextRun>,
    open: Option<OpenRun>,
    cursor: Option<Cursor>,
    /// `true` immediately after an `/ActualText` replacement, and set
    /// again just before one: the boundary where derived word spaces are
    /// suppressed (see the module docs).
    at_actual_text_boundary: bool,
    options: &'a ExtractOptions,
}

impl Builder<'_> {
    /// Add one glyph, inserting a derived break first if the geometry
    /// calls for one.
    fn push_glyph(&mut self, g: GlyphItem, diagnostics: &mut TextDiagnostics) {
        let brk = self.classify(&g);
        match brk {
            Break::Line => {
                self.close_run();
                self.emit_derived('\n', TextOrigin::DerivedLineBreak);
                diagnostics.lines_derived += 1;
            }
            Break::Word => {
                // Suppressed at an /ActualText boundary (§14.9.4's
                // character-substitution semantics) and wherever a real
                // space glyph already exists on either side.
                let doubled = self
                    .open
                    .as_ref()
                    .and_then(|r| r.text.chars().next_back())
                    .is_some_and(char::is_whitespace)
                    || g.chars.starts_with(char::is_whitespace);
                if !self.at_actual_text_boundary && !doubled {
                    self.close_run();
                    self.emit_derived(' ', TextOrigin::DerivedWordSpace);
                    diagnostics.spaces_derived += 1;
                }
            }
            Break::None => {}
        }
        self.at_actual_text_boundary = false;

        // A change of marked-content context starts a new run even
        // without a geometric break: an artifact's characters and real
        // content's characters must never share a run, or the caller
        // cannot filter one without the other.
        if self
            .open
            .as_ref()
            .is_some_and(|r| r.artifact != g.artifact || r.mcid != g.mcid)
        {
            self.close_run();
        }

        let run = self.open.get_or_insert_with(|| OpenRun {
            text: String::new(),
            glyphs: Vec::new(),
            artifact: g.artifact.clone(),
            mcid: g.mcid,
            llx: f32::MAX,
            lly: f32::MAX,
            urx: f32::MIN,
            ury: f32::MIN,
        });

        let text_start = u32::try_from(run.text.len()).unwrap_or(u32::MAX);
        run.text.push_str(&g.chars);
        let text_len = u32::try_from(g.chars.len()).unwrap_or(0);
        run.glyphs.push(ExtractedGlyph {
            code: g.code,
            rung: g.rung,
            text_start,
            text_len,
            x: g.x,
            y: g.y,
            advance: g.advance,
            size: g.size,
            direction: g.direction,
            invisible: g.invisible,
            // Carry the walk's provenance straight through; layout adds no
            // provenance of its own (it only segments). `None` when
            // capture was off.
            provenance: g.provenance,
        });
        // `Pass 139.1`: the cell is taken in the glyph's OWN frame by
        // the one shared function, not restated here in page axes. For a
        // 90° glyph at (100, 300) with advance 8.7 and size 12 the old
        // expression gave x ∈ 100..108.7 while the ink is at
        // x ∈ 91..103 — a box overlapping its glyph by about a third and
        // hung off the wrong corner, which is what made a click on the
        // middle of a rotated letter land outside every line box.
        let cell = super::glyph_cell(g.x, g.y, g.advance, g.size, g.direction);
        run.llx = run.llx.min(cell.llx as f32);
        run.urx = run.urx.max(cell.urx as f32);
        run.lly = run.lly.min(cell.lly as f32);
        run.ury = run.ury.max(cell.ury as f32);

        self.cursor = Some(Cursor {
            end_x: g.x + g.advance * g.direction.0,
            end_y: g.y + g.advance * g.direction.1,
            size: g.size,
            dir: g.direction,
        });
    }

    /// Add an `/ActualText` replacement run.
    fn push_replacement(
        &mut self,
        r: super::page::ReplacementItem,
        diagnostics: &mut TextDiagnostics,
    ) {
        // A baseline change across the replacement is still a real
        // baseline change; a word gap is not (module docs).
        //
        // ★ This comparison stays in **page axes** (`Pass 139.1`), and
        // that is a limit rather than an oversight. An `/ActualText`
        // replacement publishes an axis-aligned box and nothing else —
        // §14.9.4 N4 forbids per-character correspondence, so there is
        // no baseline, no origin and no direction to resolve into a
        // frame. Recovering a baseline from `bbox.lly` is already an
        // approximation that only holds for horizontal text; rotating it
        // would be inventing a second one on top. A rotated
        // `/ActualText` sequence is therefore segmented as it was before
        // this Pass. None exists in the corpus, and the fix, if one is
        // ever needed, is for `page::extend_covered` to carry the
        // covered glyphs' direction out with the box.
        if let (Some(cursor), Some(bbox)) = (self.cursor, r.bbox) {
            let size = cursor.size.max(1e-6);
            // The replacement's own baseline is unknown; its box's
            // bottom plus the quarter-em descender assumption recovers
            // it closely enough for a threshold comparison.
            let y = bbox.lly as f32 + size * 0.25;
            if (y - cursor.end_y).abs() > self.options.line_gap_ratio * size {
                self.close_run();
                self.emit_derived('\n', TextOrigin::DerivedLineBreak);
                diagnostics.lines_derived += 1;
            }
        }
        self.close_run();
        self.runs.push(TextRun {
            text: r.text,
            origin: TextOrigin::ActualText,
            glyphs: Vec::new(),
            artifact: r.artifact,
            mcid: r.mcid,
            bbox: r.bbox,
        });
        // The cursor moves to the right edge of what the replacement
        // covered, so the NEXT glyph's break test measures from there.
        if let Some(bbox) = r.bbox {
            let size = self.cursor.map_or(1.0, |c| c.size);
            // The direction is CARRIED from the previous glyph rather
            // than defaulted to `(1, 0)`: a replacement inside a rotated
            // run must not make the glyph after it look like a change of
            // direction and earn a spurious break. The end POINT is
            // still read off the axis-aligned box, per the note above.
            let dir = self.cursor.map_or((1.0, 0.0), |c| c.dir);
            self.cursor = Some(Cursor {
                end_x: bbox.urx as f32,
                end_y: bbox.lly as f32 + size * 0.25,
                size,
                dir,
            });
        }
        self.at_actual_text_boundary = true;
    }

    /// What the gap between the cursor and `g` implies.
    ///
    /// # `Pass 139.1`: measured in the line's own frame, not the page's
    ///
    /// Every threshold here used to be stated in page axes — `|Δy|`
    /// against the line-gap ratio, `Δx` against the backward-jump and
    /// word-gap ratios. That is right for the direction those axes
    /// assume and **catastrophic for any other**, in two independent
    /// ways:
    ///
    /// | text | what broke | which clause |
    /// |---|---|---|
    /// | 90° / 270° | one advance lands entirely in `Δy`, which exceeds `line_gap_ratio × size` for any glyph wider than a third of an em | the baseline clause |
    /// | 180° | the step is in `−x` while `advance` is a positive magnitude, so `Δx − advance ≈ −2·advance` | the backward-jump clause |
    ///
    /// Both produce **one derived line break between every letter**. On
    /// a SOLIDWORKS title block's vertically-stamped file path that was
    /// 82 glyphs in 72 runs with 71 breaks, for one line of text, which
    /// pasted into an editor as one character per line.
    ///
    /// The generalisation introduces **no new ratio**. `d` is taken from
    /// the previous glyph's *end* (already advanced along its own
    /// direction), and resolved into the frame:
    ///
    /// ```text
    /// along = d · dir      (the gap, signed forward)
    /// perp  = d × dir      (the baseline displacement)
    /// ```
    ///
    /// For `dir = (1, 0)` that is `along = Δx − advance` and
    /// `|perp| = |Δy|` — the previous expressions exactly, which is why
    /// this changes nothing on a horizontal page.
    ///
    /// # The one rule that is genuinely new
    ///
    /// A **change of direction is itself a line break**, tested first.
    /// Without it, a horizontal run ending where a vertical run begins
    /// has `along = 0` and `perp = 0` and would be merged — and the
    /// merged run would then publish one direction
    /// ([`TextRun::direction`](super::TextRun::direction)) for glyphs
    /// that do not share one. The guarantee that a run has a single
    /// direction is worth more than the merge.
    fn classify(&self, g: &GlyphItem) -> Break {
        let Some(cursor) = self.cursor else {
            // The first glyph on the page: nothing to break from.
            return Break::None;
        };
        // A zero or absurd font size (a malformed `Tf 0`, or a degenerate
        // CTM) would make every ratio comparison meaningless; fall back
        // to a nominal 1 unit so the thresholds stay finite.
        let size = cursor.size.max(g.size).max(1e-6);
        if !size.is_finite() || !g.x.is_finite() || !g.y.is_finite() {
            return Break::None;
        }

        let dir = cursor.dir;
        // Different direction ⇒ different line, whatever the gap.
        if dir.0 * g.direction.0 + dir.1 * g.direction.1 < self.options.same_direction_cos {
            return Break::Line;
        }

        let (dx, dy) = (g.x - cursor.end_x, g.y - cursor.end_y);
        // The z component of `d × dir`: for `dir = (1, 0)` this is `−Δy`,
        // and only its magnitude is compared, so the historical
        // `(g.y − cursor.y).abs()` is reproduced sign and all.
        let perp = dx * dir.1 - dy * dir.0;
        if perp.abs() > self.options.line_gap_ratio * size {
            return Break::Line;
        }
        // The forward gap, signed along the writing direction. The
        // subtraction of `advance` the page-axis version had to do
        // explicitly is already folded into the cursor's end point.
        let gap = dx * dir.0 + dy * dir.1;
        if gap < -self.options.backward_jump_ratio * size {
            return Break::Line;
        }
        if gap > self.options.word_gap_ratio * size {
            return Break::Word;
        }
        Break::None
    }

    /// Emit a one-character derived run.
    fn emit_derived(&mut self, ch: char, origin: TextOrigin) {
        self.runs.push(TextRun {
            text: ch.to_string(),
            origin,
            glyphs: Vec::new(),
            artifact: None,
            mcid: None,
            bbox: None,
        });
    }

    /// Close the in-progress glyph run, if any.
    fn close_run(&mut self) {
        let Some(run) = self.open.take() else {
            return;
        };
        // A run with no characters carries nothing a caller can use —
        // its geometry already lives in the cursor — and leaving it in
        // would make a trailing derived break unreachable by the
        // drop-trailing-whitespace pass in `finish`.
        if run.text.is_empty() {
            return;
        }
        let bbox = (run.llx <= run.urx).then(|| {
            Rect::from_corners(
                f64::from(run.llx),
                f64::from(run.lly),
                f64::from(run.urx),
                f64::from(run.ury),
            )
        });
        self.runs.push(TextRun {
            text: run.text,
            origin: TextOrigin::Glyphs,
            glyphs: run.glyphs,
            artifact: run.artifact,
            mcid: run.mcid,
            bbox,
        });
    }

    /// Close the last run and post-process.
    fn finish(mut self, diagnostics: &mut TextDiagnostics) -> Vec<TextRun> {
        self.close_run();
        // A trailing derived break contributes nothing but a stray
        // character at the end of every page's text.
        while self.runs.last().is_some_and(|r| !r.origin.is_sourced()) {
            match self.runs.pop().map(|r| r.origin) {
                Some(TextOrigin::DerivedLineBreak) => {
                    diagnostics.lines_derived = diagnostics.lines_derived.saturating_sub(1);
                }
                Some(TextOrigin::DerivedWordSpace) => {
                    diagnostics.spaces_derived = diagnostics.spaces_derived.saturating_sub(1);
                }
                _ => break,
            }
        }

        // Right-to-left detection. R17 permits `unicode-bidi` in this
        // path and this path only, but the reordering itself is deferred
        // (see the module docs on `super`): ISO 32000-1's own position is
        // that visual-to-logical reordering is specified nowhere (B3)
        // and that its only RTL mechanism is `ReversedChars` (B2), which
        // pdfcer implements. Detecting and naming is the honest interim.
        let rtl_runs = self
            .runs
            .iter()
            .filter(|r| r.text.chars().any(is_rtl))
            .count();
        if rtl_runs > 0 {
            diagnostics.rtl_runs += rtl_runs as u64;
            diagnostics.note(
                "text: right-to-left characters present — logical-order (bidi) reordering is \
                 DEFERRED; runs are in page content order, with /ReversedChars honoured where \
                 the file declares it (ISO 32000-1 §14.8.2.3.3)"
                    .to_string(),
            );
        }

        self.runs
    }
}

/// Whether a character belongs to a right-to-left script.
///
/// A deliberately coarse range test over the RTL blocks, not a Unicode
/// bidirectional-category lookup: the answer feeds a *diagnostic count*
/// and a deferral note, not a reordering decision, so a table-driven
/// implementation (and the dependency that would come with it) would buy
/// precision nothing consumes. When bidi reordering lands, this is
/// replaced wholesale rather than extended.
fn is_rtl(ch: char) -> bool {
    matches!(u32::from(ch),
        0x0590..=0x05FF   // Hebrew
        | 0x0600..=0x06FF // Arabic
        | 0x0700..=0x074F // Syriac
        | 0x0750..=0x077F // Arabic Supplement
        | 0x0780..=0x07BF // Thaana
        | 0x07C0..=0x08FF // NKo, Samaritan, Mandaic, Arabic Extended-A
        | 0xFB1D..=0xFDFF // Hebrew/Arabic presentation forms A
        | 0xFE70..=0xFEFF // Arabic presentation forms B
    )
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::text_extract::font::LadderRung;

    /// A glyph at `(x, y)` advancing `advance`, size 10.
    fn glyph(chars: &str, x: f32, y: f32, advance: f32) -> Item {
        Item::Glyph(GlyphItem {
            chars: chars.to_string(),
            code: 0,
            rung: LadderRung::ToUnicode,
            x,
            y,
            advance,
            size: 10.0,
            direction: (1.0, 0.0),
            invisible: false,
            artifact: None,
            mcid: None,
            provenance: None,
        })
    }

    fn run(items: Vec<Item>) -> (Vec<TextRun>, TextDiagnostics) {
        let mut diagnostics = TextDiagnostics::default();
        let runs = assemble(items, &ExtractOptions::default(), &mut diagnostics);
        (runs, diagnostics)
    }

    fn text(runs: &[TextRun]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    fn sourced(runs: &[TextRun]) -> String {
        runs.iter()
            .filter(|r| r.is_sourced())
            .map(|r| r.text.as_str())
            .collect()
    }

    #[test]
    fn adjacent_glyphs_produce_no_derived_whitespace() {
        let (runs, d) = run(vec![
            glyph("H", 0.0, 100.0, 6.0),
            glyph("i", 6.0, 100.0, 3.0),
        ]);
        assert_eq!(text(&runs), "Hi");
        assert_eq!(d.spaces_derived, 0);
        assert_eq!(runs.len(), 1, "one uninterrupted run");
    }

    #[test]
    fn a_wide_gap_derives_a_word_space_and_counts_it() {
        let (runs, d) = run(vec![
            glyph("a", 0.0, 100.0, 6.0),
            // 4 units of gap on a size-10 glyph = 0.4 em > the 0.20
            // default ratio.
            glyph("b", 10.0, 100.0, 6.0),
        ]);
        assert_eq!(text(&runs), "a b");
        assert_eq!(sourced(&runs), "ab", "the space is NOT in the file");
        assert_eq!(d.spaces_derived, 1);
    }

    #[test]
    fn a_baseline_change_derives_a_line_break() {
        let (runs, d) = run(vec![
            glyph("a", 0.0, 100.0, 6.0),
            glyph("b", 0.0, 88.0, 6.0),
        ]);
        assert_eq!(text(&runs), "a\nb");
        assert_eq!(sourced(&runs), "ab");
        assert_eq!(d.lines_derived, 1);
        assert_eq!(d.spaces_derived, 0);
    }

    #[test]
    fn a_backward_jump_on_one_baseline_is_a_line_break() {
        // The two-column case: same baseline, second column starts far
        // to the LEFT of where the first ended.
        let (runs, d) = run(vec![
            glyph("a", 200.0, 100.0, 6.0),
            glyph("b", 20.0, 100.0, 6.0),
        ]);
        assert_eq!(text(&runs), "a\nb");
        assert_eq!(d.lines_derived, 1);
    }

    #[test]
    fn a_real_space_glyph_is_never_doubled() {
        // A file that DOES encode its space: the gap is wide (the space
        // glyph occupies it), but a derived space on top would give
        // "a  b".
        let (runs, d) = run(vec![
            glyph("a", 0.0, 100.0, 6.0),
            glyph(" ", 6.0, 100.0, 5.0),
            glyph("b", 11.0, 100.0, 6.0),
        ]);
        assert_eq!(text(&runs), "a b");
        assert_eq!(d.spaces_derived, 0, "the space is SOURCED, not derived");
        assert_eq!(sourced(&runs), "a b");
    }

    #[test]
    fn trailing_derived_whitespace_is_dropped_and_uncounted() {
        // Nothing follows the break, so it is noise; and dropping it
        // must also drop its count, or the diagnostics would report a
        // derivation that is not in the output.
        let mut items = vec![glyph("a", 0.0, 100.0, 6.0)];
        items.push(Item::Glyph(GlyphItem {
            chars: String::new(),
            code: 0,
            rung: LadderRung::ToUnicode,
            x: 0.0,
            y: 50.0,
            advance: 0.0,
            size: 10.0,
            direction: (1.0, 0.0),
            invisible: false,
            artifact: None,
            mcid: None,
            provenance: None,
        }));
        let (runs, d) = run(items);
        assert_eq!(text(&runs), "a");
        assert_eq!(d.lines_derived, 0);
    }

    #[test]
    fn an_artifact_boundary_starts_a_new_run() {
        let mut items = vec![glyph("a", 0.0, 100.0, 6.0)];
        items.push(Item::Glyph(GlyphItem {
            chars: "1".to_string(),
            code: 0,
            rung: LadderRung::ToUnicode,
            x: 6.0,
            y: 100.0,
            advance: 6.0,
            size: 10.0,
            direction: (1.0, 0.0),
            invisible: false,
            artifact: Some(ArtifactKind::Pagination),
            mcid: None,
            provenance: None,
        }));
        let (runs, _) = run(items);
        assert_eq!(runs.len(), 2);
        assert!(runs[0].artifact.is_none());
        assert_eq!(runs[1].artifact, Some(ArtifactKind::Pagination));
    }

    #[test]
    fn glyph_text_ranges_survive_one_to_many_mappings() {
        // A ligature code producing three characters: the range must be
        // the three bytes, not one.
        let (runs, _) = run(vec![
            glyph("f", 0.0, 100.0, 3.0),
            glyph("ffl", 3.0, 100.0, 9.0),
        ]);
        let g = &runs[0].glyphs[1];
        assert_eq!(g.text_start, 1);
        assert_eq!(g.text_len, 3);
        let slice = &runs[0].text[g.text_start as usize..(g.text_start + g.text_len) as usize];
        assert_eq!(slice, "ffl");
    }

    #[test]
    fn rtl_text_is_detected_counted_and_named() {
        let (_runs, d) = run(vec![glyph("\u{05D0}", 0.0, 100.0, 6.0)]);
        assert_eq!(d.rtl_runs, 1);
        assert!(d.notes.iter().any(|n| n.contains("bidi")));
    }

    #[test]
    fn no_items_is_no_runs() {
        let (runs, d) = run(Vec::new());
        assert!(runs.is_empty());
        assert_eq!(d.spaces_derived, 0);
    }
}

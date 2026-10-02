//! Hit-testing and caret navigation over a recognized [`EditableTextModel`].

use super::cells::x_distance;
use super::{EditableTextModel, GlyphRef, Line, TextPosition};
use crate::text_extract::ExtractedGlyph;

impl<'a> EditableTextModel<'a> {
    /// Map a page-space point to a caret [`TextPosition`].
    ///
    /// Finds the line whose box contains `(x, y)` — or, if none does, the
    /// nearest line **within reach** — then the glyph on that line whose
    /// extent along the line contains the point (or the nearest), and
    /// resolves to the glyph's leading or trailing boundary by which half
    /// of the glyph it fell in. Pure geometry over the borrowed page;
    /// introduces no GUI type.
    ///
    /// # `None` means "no text here" — the reach is one line-height
    ///
    /// A line is *in reach* when the point lies inside its box **inflated
    /// by one line-height on every side** (the larger of the line's font
    /// size and its box height). That keeps the gesture every editor
    /// relies on — clicking just past the last character puts the caret
    /// after it, clicking a little above or below a line still lands on
    /// it — and makes a click on blank paper answer `None`.
    ///
    /// **Until 2026-09-05 this fell back to the nearest line at ANY
    /// distance**, so `None` was reachable only on a page with no text at
    /// all; the doc comment said so and read as a description rather than
    /// the defect it was. `pdfcer-gui` measured it: a point 100 000 pt to
    /// the right of a 612 pt page, and one a billion points away, both
    /// resolved to a run, which made its *click-in-space-to-add-text*
    /// gesture dead code on every real document — it was asking a
    /// placement-that-never-fails and reading the answer as a presence
    /// test. The bound is derived from the line rather than taken as a
    /// parameter (their shape (a)) so that every caller, including ones not
    /// yet written, gets the presence semantics without a second opinion
    /// about this crate's geometry. The bound is axis-aligned in page
    /// space, which for a rotated line is its page-space box inflated the
    /// same way — generous, never tighter than the line itself.
    #[must_use]
    pub fn hit_test(&self, x: f64, y: f64) -> Option<TextPosition> {
        // Pick the line: a containing box wins outright; otherwise the
        // nearest-by-baseline line whose INFLATED box contains the point.
        let mut chosen: Option<&Line> = None;
        let mut best_dy = f64::MAX;
        for line in &self.lines {
            let b = line.bbox;
            if y >= b.lly && y <= b.ury && x >= b.llx && x <= b.urx {
                return self.hit_in_line(line, x, y);
            }
            let reach = f64::from(line.size).max(b.ury - b.lly).max(0.0);
            let in_reach = x >= b.llx - reach
                && x <= b.urx + reach
                && y >= b.lly - reach
                && y <= b.ury + reach;
            if !in_reach {
                continue;
            }
            let dy = (y - f64::from(line.baseline_y)).abs();
            if dy < best_dy {
                best_dy = dy;
                chosen = Some(line);
            }
        }
        chosen.and_then(|line| self.hit_in_line(line, x, y))
    }

    /// Resolve a page-space point to a caret within one line: the glyph
    /// whose extent **along the line's own writing direction** contains it
    /// (leading/trailing half), else clamp to the line ends.
    ///
    /// # `Pass 139.2`: projected onto the line, not onto the page x axis
    ///
    /// Every comparison here used to be against `x` alone — `g.x` versus
    /// `g.x + g.advance` — which is right for a horizontal line and
    /// meaningless for any other. On a line stamped at 90° all of its
    /// glyphs share one `x`, so the first glyph "contained" every click
    /// and the caret never moved; on a 180° line the extents ran the wrong
    /// way and both ends collapsed onto one slot. Driven by the consuming
    /// shell before the fix: a sweep down a six-letter 90° string selected
    /// **five** of them, and a sweep along an eight-letter 180° string
    /// selected **nothing at all**.
    ///
    /// The generalisation is one projection. `t` is the point's distance
    /// along the line's direction from the line's own origin, and each
    /// glyph's extent is `[t0, t0 + advance]` in the same coordinate. For
    /// `direction = (1, 0)` and a line whose origin is its leftmost glyph,
    /// `t` is `x − origin.x` and every comparison below reduces term for
    /// term to the ones it replaced.
    ///
    /// The perpendicular component is deliberately **discarded**: the
    /// caller has already chosen the line (by box containment or by
    /// nearest baseline), so how far off the baseline the click was is no
    /// longer a question this function answers.
    fn hit_in_line(&self, line: &Line, x: f64, y: f64) -> Option<TextPosition> {
        let (dx, dy) = (f64::from(line.direction.0), f64::from(line.direction.1));
        // The point, projected onto the line's direction. The origin the
        // projection is measured from cancels out of every comparison
        // below, so any fixed point on the line would do; the first
        // glyph's is used because it is what `RawLine` already records.
        let origin = self
            .glyph(*line.glyphs.first()?)
            .map(|g| (f64::from(g.x), f64::from(g.y)))?;
        let along = |px: f64, py: f64| (px - origin.0) * dx + (py - origin.1) * dy;
        let t = along(x, y);

        let mut best: Option<(GlyphRef, &ExtractedGlyph, f64, f64)> = None;
        for &gref in &line.glyphs {
            let g = self.glyph(gref)?;
            let t0 = along(f64::from(g.x), f64::from(g.y));
            let t1 = t0 + f64::from(g.advance);
            let (lo, hi) = (t0.min(t1), t0.max(t1));
            if t >= lo && t <= hi {
                let mid = (lo + hi) / 2.0;
                return Some(self.boundary(gref, g, t > mid));
            }
            // Track the nearest glyph for the clamp-to-end fallback.
            let dist = if t < lo { lo - t } else { t - hi };
            if best.is_none_or(|(_, _, d, _)| dist < d) {
                best = Some((gref, g, dist, t0));
            }
        }
        best.map(|(gref, g, _, t0)| {
            // Clamp: before the nearest glyph's midpoint ALONG THE LINE ⇒
            // its leading edge; after ⇒ trailing. "Before" and "after" are
            // the line's own sense of the words, not the page's — which is
            // the whole point of the projection, and is why a 180° line
            // used to clamp both ends to the same slot.
            let trailing = t > t0 + f64::from(g.advance) / 2.0;
            self.boundary(gref, g, trailing)
        })
    }

    /// The caret position at a glyph's leading (`trailing == false`) or
    /// trailing edge, as a byte offset into its run's text.
    fn boundary(&self, gref: GlyphRef, g: &ExtractedGlyph, trailing: bool) -> TextPosition {
        let offset = if trailing {
            (g.text_start + g.text_len) as usize
        } else {
            g.text_start as usize
        };
        TextPosition::new(gref.run, offset)
    }

    // -- Boundary lookups (Pass 14.3 GUI: double/triple-click, Home/End) --
    //
    // The GUI's word/line selection and Home/End caret navigation need the
    // Line a caret sits on, and the word/line span around it. Per decision
    // 014 §4.1 ("core owns the derived structure") and Pass 14.3 UI spec
    // §4.3, these live HERE — reusing the exact `text_start`/`text_len`
    // glyph-boundary matching `hit_test`/`hit_in_line` already encode —
    // rather than being re-derived (and possibly diverging) in `pdfce-gui`.
    // All three are pure index/range arithmetic over the borrowed page; they
    // add NO GUI type (the load-bearing GUI-core separation, §3).

    /// The [`Self::lines`] index of the line containing caret `pos`, or
    /// `None` if no line holds a glyph of `pos.run` whose byte range brackets
    /// `pos.byte_offset` (a stale reference, or a `pos.run` that carries no
    /// clustered glyph — an `/ActualText`/whitespace run).
    ///
    /// This is the reverse of `hit_test`'s internal line-then-glyph walk:
    /// where `hit_test` maps a *point* to a `(run, offset)`, this maps a
    /// `(run, offset)` back to the *line* it was clustered into. A run's
    /// glyphs may be split across lines by a baseline jump (module docs,
    /// Stage 1), so the match is per-glyph, not per-run: the first line (in
    /// content order) carrying a `pos.run` glyph whose
    /// `[text_start, text_start+text_len]` closed interval contains
    /// `pos.byte_offset` wins. The interval is closed so a caret exactly on a
    /// glyph's trailing boundary resolves (it is a valid caret slot).
    #[must_use]
    pub fn line_at(&self, pos: TextPosition) -> Option<usize> {
        for (li, line) in self.lines.iter().enumerate() {
            for &gref in &line.glyphs {
                if gref.run != pos.run {
                    continue;
                }
                let Some(g) = self.glyph(gref) else { continue };
                let lo = g.text_start as usize;
                let hi = lo + g.text_len as usize;
                if pos.byte_offset >= lo && pos.byte_offset <= hi {
                    return Some(li);
                }
            }
        }
        None
    }

    /// The [`Self::blocks`] index of the block (paragraph) containing caret
    /// `pos`, or `None` when [`Self::line_at`] finds no line for `pos`.
    ///
    /// Sugar over [`Self::line_at`] then [`Line::block`] — the same "core owns
    /// the derived structure" spirit as `line_at`/`word_range_at`/
    /// `line_range_at` themselves (Pass 14.3 UI spec §4.3), so the three-line
    /// composition does not reappear at every call site (CLI, GUI, tests).
    /// Pure index arithmetic over the borrowed page; adds NO GUI type (the
    /// load-bearing GUI-core separation, §3). Pass 15.2's reflow sub-mode
    /// resolves "which paragraph is the caret in" through this against a model
    /// built with [`reflow_recognition_options`](crate::text_edit::reflow_recognition_options).
    #[must_use]
    pub fn block_at(&self, pos: TextPosition) -> Option<usize> {
        let li = self.line_at(pos)?;
        self.lines.get(li).map(|l| l.block)
    }

    /// The first and last caret positions of the line containing `pos` — the
    /// two ends Home/End move to (Pass 14.3 UI spec §4.5). `None` when
    /// [`Self::line_at`] finds no line for `pos`.
    ///
    /// The ends are the leading boundary of the line's first glyph and the
    /// trailing boundary of its last — each a real [`TextPosition`] on a
    /// glyph boundary. A line may draw glyphs from more than one run (a
    /// derived word space between two runs stays within the line), so the two
    /// returned positions can name different runs; that is fine for caret
    /// navigation (Home/End never commits an edit — a *selection* spanning
    /// >1 run is refused separately, §4.4/UI spec).
    #[must_use]
    pub fn line_range_at(&self, pos: TextPosition) -> Option<(TextPosition, TextPosition)> {
        let li = self.line_at(pos)?;
        let line = self.lines.get(li)?;
        let first = *line.glyphs.first()?;
        let last = *line.glyphs.last()?;
        let fg = self.glyph(first)?;
        let lg = self.glyph(last)?;
        let start = TextPosition::new(first.run, fg.text_start as usize);
        let end = TextPosition::new(last.run, (lg.text_start + lg.text_len) as usize);
        Some((start, end))
    }

    // -- Caret navigation geometry (Pass 14.4 GUI: arrows / Up-Down) ------
    //
    // Pass 14.4 completes the caret model with keyboard navigation (14.3 UI
    // spec §4.5). Left/Right/Up/Down are pure traversals over structure this
    // model already owns, so — like `line_at`/`word_range_at`/`line_range_at`
    // in Pass 14.3 — they live HERE, not re-derived in `pdfce-gui`: the GUI's
    // `PageText`/`TextRun`/`ExtractedGlyph` are `#[non_exhaustive]` and so
    // cannot be constructed in a `pdfce-gui` unit test, which means core is
    // also the only place these can be *headless-tested* (decision 014 §4.1's
    // "core owns the derived structure" argument, reinforced by the crate
    // boundary). All add NO GUI type (the load-bearing GUI-core separation,
    // §3). Home/End need no new method — they are exactly
    // [`Self::line_range_at`]'s two ends.

    /// The page-space x of caret `pos` — the leading edge of the glyph that
    /// begins at `pos.byte_offset`, or the trailing edge of the glyph that ends
    /// there (Pass 14.4 Up/Down "nearest-x", UI spec §4.5). `None` when no
    /// glyph in `pos.run` has a boundary exactly at `pos.byte_offset` (a stale
    /// position, or a run — derived whitespace / `/ActualText` — carrying no
    /// clustered glyph).
    ///
    /// This is the x-half of the vertical segment the GUI draws for a caret,
    /// exposed so vertical navigation can compute a "desired column" through
    /// the SAME glyph-boundary matching [`Self::hit_test`] / [`Self::line_range_at`]
    /// already encode, rather than the GUI re-deriving glyph x-positions.
    /// **A page-axis answer, and on a rotated line it is the wrong
    /// question** (`Pass 139.2`). Every glyph of a 90° line shares one `x`,
    /// so this returns the same number for every caret slot on it. The
    /// signature is the limit — a scalar cannot name a point on a line
    /// that is not horizontal — which is the same shape of defect
    /// `PickedLine::object_index` had in `Pass 138.0`: an answer made
    /// unrepresentable by its own return type.
    ///
    /// It is kept, un-deprecated, because for horizontal text it is
    /// exactly right and is what "desired column" for Up/Down navigation
    /// means. Use [`Self::caret_point`] when the line may be rotated.
    #[must_use]
    pub fn caret_x(&self, pos: TextPosition) -> Option<f32> {
        self.caret_point(pos).map(|(x, _)| x)
    }

    /// **The page-space point of caret `pos`** — the origin of the glyph
    /// that begins at `pos.byte_offset`, or the
    /// [`advance_end`](crate::text_extract::ExtractedGlyph::advance_end) of
    /// the glyph that ends there (`Pass 139.2`).
    ///
    /// `None` under exactly the same conditions as [`Self::caret_x`]: no
    /// glyph in `pos.run` has a boundary at that offset, because the
    /// position is stale or the run carries no clustered glyph (derived
    /// whitespace, `/ActualText`).
    ///
    /// # Why this exists beside [`Self::caret_x`]
    ///
    /// A caret is a *point on a baseline*, and a baseline has a direction.
    /// `caret_x` returns the x half of one, which is complete for
    /// horizontal text and degenerate for anything else — on a 90° line
    /// every slot has the same `x`. Pair this with the line's
    /// [`Line::direction`] and a shell has everything it needs to draw the
    /// caret *along* the text rather than always vertically.
    #[must_use]
    pub fn caret_point(&self, pos: TextPosition) -> Option<(f32, f32)> {
        let run = self.page.runs.get(pos.run)?;
        for g in &run.glyphs {
            let lo = g.text_start as usize;
            let hi = lo + g.text_len as usize;
            if pos.byte_offset == lo {
                return Some((g.x, g.y));
            }
            if pos.byte_offset == hi {
                return Some(g.advance_end());
            }
        }
        None
    }

    /// The caret on line `line_index` whose x-extent is nearest page-space `x`
    /// — the same within-line resolution [`Self::hit_test`] performs
    /// internally, exposed for ONE explicit line so vertical caret navigation
    /// (Pass 14.4 Up/Down, UI spec §4.5) can land on the geometrically nearest
    /// slot of the adjacent line without a third re-implementation of
    /// nearest-glyph matching in the GUI (§3). `None` for an out-of-range
    /// `line_index` or a line whose glyphs are all stale.
    ///
    /// **Page-axis, and it stays that way on purpose** (`Pass 139.2`).
    /// `x` alone cannot name a slot on a line that is not horizontal, so
    /// this delegates with the line's own `baseline_y` as the second
    /// coordinate — which is exact for horizontal text and, for a rotated
    /// line, resolves as though the caller had clicked on its baseline at
    /// that `x`. Use [`Self::caret_on_line_nearest_point`] when the line
    /// may be rotated. Not deprecated: this *is* the right shape for the
    /// Up/Down "desired column" it was built for.
    #[must_use]
    pub fn caret_on_line_nearest_x(&self, line_index: usize, x: f64) -> Option<TextPosition> {
        let line = self.lines.get(line_index)?;
        let y = f64::from(line.baseline_y);
        self.hit_in_line(line, x, y)
    }

    /// The caret on line `line_index` nearest a page-space **point**,
    /// resolved along that line's own writing direction (`Pass 139.2`).
    ///
    /// The two-coordinate twin of [`Self::caret_on_line_nearest_x`], and
    /// the same body — [`Self::hit_test`] calls it too, so there is one
    /// implementation of within-line resolution rather than three. `None`
    /// for an out-of-range `line_index` or a line whose glyphs are all
    /// stale.
    #[must_use]
    pub fn caret_on_line_nearest_point(
        &self,
        line_index: usize,
        x: f64,
        y: f64,
    ) -> Option<TextPosition> {
        let line = self.lines.get(line_index)?;
        self.hit_in_line(line, x, y)
    }

    /// Move the caret one glyph boundary left (Pass 14.4, UI spec §4.5).
    ///
    /// Steps within the run and, at a run's/line's start, across to the
    /// previous run's last slot — a new line begins at a new run after a
    /// [`TextOrigin::DerivedLineBreak`](crate::text_extract::TextOrigin::DerivedLineBreak), so this glides across line boundaries
    /// for free. At the document's very first slot it stays put (clamped, never
    /// wraps). Empty runs (derived word-space / line-break / `/ActualText`)
    /// carry no glyph and so contribute no slot — which is exactly why the step
    /// skips over them.
    #[must_use]
    pub fn caret_left(&self, pos: TextPosition) -> TextPosition {
        let key = pos.key();
        self.caret_slots()
            .into_iter()
            .rev()
            .find(|p| p.key() < key)
            .unwrap_or(pos)
    }

    /// Move the caret one glyph boundary right (Pass 14.4, UI spec §4.5). The
    /// mirror of [`Self::caret_left`]; clamps at the document's last slot.
    #[must_use]
    pub fn caret_right(&self, pos: TextPosition) -> TextPosition {
        let key = pos.key();
        self.caret_slots()
            .into_iter()
            .find(|p| p.key() > key)
            .unwrap_or(pos)
    }

    /// Move the caret to the geometrically nearest slot on the line
    /// immediately ABOVE the current one (UI spec §4.5). `desired_x` is the
    /// page-space column to preserve, normally [`Self::caret_x`] of the
    /// current caret. Stays put when there is nowhere to go or the caret is
    /// not on a recognized line.
    ///
    /// Outside a table the target is the nearest line above in the same
    /// column band; among lines on nearly the same baseline (within a
    /// quarter em) the one nearest `desired_x` wins, so a caret above a
    /// table row enters the cell under it. Inside a table cell the target is
    /// the next line up in the same cell, else the bottom line of the cell
    /// above (the one under `desired_x`, else one sharing the current
    /// cell's columns; rows of empty cells are skipped), else the nearest
    /// line above the table.
    ///
    /// "Above" is a LARGER baseline y: default user space y increases UP the
    /// page (§9.4.4), the opposite of screen space.
    #[must_use]
    pub fn caret_up(&self, pos: TextPosition, desired_x: f32) -> TextPosition {
        self.caret_vertical(pos, desired_x, true)
    }

    /// Move the caret to the nearest slot on the line immediately BELOW the
    /// current one. The mirror of [`Self::caret_up`]: "below" is a SMALLER
    /// baseline y, and leaving a cell downward lands on the top line of the
    /// cell below.
    #[must_use]
    pub fn caret_down(&self, pos: TextPosition, desired_x: f32) -> TextPosition {
        self.caret_vertical(pos, desired_x, false)
    }

    fn caret_vertical(&self, pos: TextPosition, desired_x: f32, up: bool) -> TextPosition {
        let Some(cur_idx) = self.line_at(pos) else {
            return pos;
        };
        let Some(cur) = self.lines.get(cur_idx) else {
            return pos;
        };
        let target = match cur.cell {
            Some(ci) => self.vertical_from_cell(cur_idx, ci, desired_x, up),
            // §14.8.2.3.1 reading order: never cross column bands.
            None => self.nearest_line(cur_idx, desired_x, up, |l| l.column == cur.column),
        };
        target
            .and_then(|li| self.caret_on_line_nearest_x(li, f64::from(desired_x)))
            .unwrap_or(pos)
    }

    /// The `eligible` line nearest line `cur_idx` on the requested side by
    /// baseline distance; lines within a quarter em of that distance are
    /// tied and the one nearest `desired_x` wins.
    pub(super) fn nearest_line(
        &self,
        cur_idx: usize,
        desired_x: f32,
        up: bool,
        eligible: impl Fn(&Line) -> bool,
    ) -> Option<usize> {
        let cur = self.lines.get(cur_idx)?;
        let candidates: Vec<(usize, f32, f64)> = self
            .lines
            .iter()
            .enumerate()
            .filter(|&(li, l)| {
                let dy = l.baseline_y - cur.baseline_y;
                li != cur_idx && eligible(l) && if up { dy > 0.0 } else { dy < 0.0 }
            })
            .map(|(li, l)| {
                let dy = (l.baseline_y - cur.baseline_y).abs();
                (li, dy, x_distance(l.bbox, f64::from(desired_x)))
            })
            .collect();
        let nearest = candidates.iter().map(|c| c.1).fold(f32::INFINITY, f32::min);
        let tie = 0.25 * cur.size.max(1.0);
        candidates
            .iter()
            .filter(|c| c.1 <= nearest + tie)
            .min_by(|a, b| a.2.total_cmp(&b.2).then(a.1.total_cmp(&b.1)))
            .map(|c| c.0)
    }

    /// Every caret slot on the page — the leading and trailing byte boundary of
    /// each clustered glyph — in `(run, byte_offset)` content order, de-duped.
    /// The ordered spine [`Self::caret_left`] / [`Self::caret_right`] step
    /// along.
    fn caret_slots(&self) -> Vec<TextPosition> {
        let mut slots = Vec::new();
        for (ri, run) in self.page.runs.iter().enumerate() {
            for g in &run.glyphs {
                let lo = g.text_start as usize;
                let hi = lo + g.text_len as usize;
                slots.push(TextPosition::new(ri, lo));
                slots.push(TextPosition::new(ri, hi));
            }
        }
        slots.sort_by_key(|p| p.key());
        slots.dedup();
        slots
    }

    /// The word boundaries around caret `pos`, split on Unicode whitespace
    /// **within `pos.run`'s own text** — what double-click selects (Pass 14.3
    /// UI spec §4.3).
    ///
    /// A DERIVED judgement with the same honesty posture as every other
    /// boundary in this model: word boundaries do not exist in an untagged
    /// content stream any more than lines do (S1–S9), so this is a
    /// whitespace split over the run's decoded text, not a sourced fact.
    /// Both returned positions name `pos.run` (a word never spans runs), so
    /// the result is always an editable single-run selection. When `pos.run`
    /// is out of range the position is returned collapsed (`(pos, pos)`),
    /// never a panic.
    #[must_use]
    pub fn word_range_at(&self, pos: TextPosition) -> (TextPosition, TextPosition) {
        let Some(run) = self.page.runs.get(pos.run) else {
            return (pos, pos);
        };
        let (lo, hi) = word_bounds(&run.text, pos.byte_offset);
        (
            TextPosition::new(pos.run, lo),
            TextPosition::new(pos.run, hi),
        )
    }

    /// The glyphs covered by the selection between two caret positions.
    ///
    /// Order-insensitive: the two positions are sorted, then every glyph
    /// whose byte range intersects the covered span — from `start.byte_offset`
    /// in the start run, through whole intervening runs, to `end.byte_offset`
    /// in the end run — is returned, in content order. `/ActualText` and
    /// derived-whitespace runs contribute no glyphs (they carry none), so a
    /// selection across them yields exactly the real glyphs it covers.
    #[must_use]
    pub fn resolve_range(&self, a: TextPosition, b: TextPosition) -> Vec<GlyphRef> {
        let (start, end) = if a.key() <= b.key() { (a, b) } else { (b, a) };
        let mut covered = Vec::new();
        let last = self.page.runs.len().saturating_sub(1);
        for ri in start.run..=end.run.min(last) {
            let Some(run) = self.page.runs.get(ri) else {
                break;
            };
            // The byte window of this run that the selection covers.
            let lo = if ri == start.run {
                start.byte_offset
            } else {
                0
            };
            let hi = if ri == end.run {
                end.byte_offset
            } else {
                run.text.len()
            };
            for (gi, g) in run.glyphs.iter().enumerate() {
                let g0 = g.text_start as usize;
                let g1 = g0 + g.text_len as usize;
                // Intersection of [g0, g1) with [lo, hi); a zero-width caret
                // window (lo == hi) selects nothing, which is correct.
                if g0 < hi && g1 > lo {
                    covered.push(GlyphRef::new(ri, gi));
                }
            }
        }
        covered
    }
}

/// The `[start, end)` byte range of the whitespace-delimited word of `text`
/// that contains byte offset `off` (Pass 14.3 double-click, UI spec §4.3).
///
/// `start` is just past the last Unicode-whitespace char strictly before
/// `off` (or 0), and `end` is the first whitespace char at or after `off`
/// (or `text.len()`). Both bounds are UTF-8 char boundaries, so they are
/// valid caret slots; in a simple font each code is one glyph whose text is
/// one code point, so a whitespace boundary is also a glyph boundary. `off`
/// is clamped into range first, so an out-of-range offset never panics. When
/// `off` sits on a whitespace char the returned range is the preceding word
/// (its `end` collapses onto `off`), which is the intuitive double-click
/// result on inter-word space.
fn word_bounds(text: &str, off: usize) -> (usize, usize) {
    let off = off.min(text.len());
    let mut start = 0usize;
    let mut end = text.len();
    for (i, c) in text.char_indices() {
        if i < off {
            if c.is_whitespace() {
                start = i + c.len_utf8();
            }
        } else if c.is_whitespace() {
            end = i;
            break;
        }
    }
    (start, end)
}

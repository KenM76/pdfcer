//! Splitting a text line into words at the valley of its gap distribution.
//!
//! # Contract
//!
//! [`split`] takes a [`TextLine`] and the components it was grouped from, and
//! returns the words on it, left to right. The threshold between an
//! intra-word and an inter-word gap is found per line, from that line's own
//! gaps, rather than being a fixed multiple of x-height (`ARCHITECTURE.md`
//! section 6): in print the two populations separate cleanly, and where they
//! separate depends on the face's set width and tracking, which the engine
//! does not know.
//!
//! Deterministic: integer gaps, an `f64` objective, ties stated.

use crate::image::components::Component;
use crate::layout::lines::TextLine;

/// Word-splitting parameters.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Fewest gaps a line must have before its distribution is worth
    /// analysing. Two gaps cannot show a valley; three is the least that
    /// can.
    pub min_gaps: usize,
    /// How separable the two gap populations must be before the valley is
    /// believed, as Otsu's `eta`: between-class variance over total
    /// variance, in `0..=1`.
    ///
    /// Otsu returns a threshold for any distribution, including the unimodal
    /// one a single-word line or a monospaced run produces, so something has
    /// to decide when the split is real. `eta` is the right quantity to
    /// decide it on — it is exactly "how much of the spread is explained by
    /// splitting here".
    ///
    /// Swept end to end; `model/params.tsv` carries the result and its
    /// reasoning. What the level is worth depends entirely on where falling
    /// back lands, which is why it moved when
    /// [`Params::no_valley_x_heights`] separated from
    /// [`Params::lone_gap_x_heights`]: a demanding gate is only affordable
    /// once the fallback is a credible threshold rather than a ceiling.
    pub min_separability: f64,
    /// A gap this many x-heights wide is a space, whatever the distribution
    /// says: the ceiling on a valley the line's own gaps produced.
    ///
    /// This is a ceiling and nothing else. What a line falls back to when it
    /// has no believable valley is [`Params::no_valley_x_heights`], which is
    /// a different claim and a different number.
    ///
    /// The claim is that inter-letter gaps inside a word in a printed face do
    /// not approach the x-height, so a gap this wide is a space even without a
    /// distribution to confirm it. Swept end to end; `model/params.tsv`
    /// carries the measured level and the per-block check behind it.
    ///
    /// # Why it caps the valley and does not merely stand in for it
    ///
    /// Otsu splits a distribution into two classes, and a line that sets its
    /// words apart by *several* spaces has three: intra-letter, one space, and
    /// the wide run. One wide outlier carries enough between-class variance to
    /// pull the threshold above the ordinary spaces, and the line then comes
    /// back as two words — measured on a CAD title block reading
    /// `PART NO. 71-4820-B   REV C`, where the valley landed at 11 px with the
    /// single spaces at 8 to 11 px and the wide run at 27 px.
    ///
    /// Capping fixes that without a second parameter, because the claim in the
    /// paragraph above is already the claim needed: no valley may sit where a
    /// gap this wide is called intra-word.
    ///
    /// The limitation this admits, stated rather than discovered: text that is
    /// deliberately letterspaced beyond the x-height — a widely tracked title
    /// — splits into single letters. That was already true of the fallback.
    pub lone_gap_x_heights: f32,
    /// The threshold for a line whose gaps produced no believable valley —
    /// too few of them, or too little separation between them.
    ///
    /// # Why this is not the same number as the ceiling
    ///
    /// The two answer different questions. The ceiling asks *how wide may a
    /// gap be and still be called intra-word*, and being generous costs it
    /// nothing, because a measured valley is what usually decides. This asks
    /// *where is the boundary, when nothing about this line will say* — and
    /// the honest answer is the middle of where such boundaries are observed
    /// to sit, not their upper edge. Sharing one number made every
    /// fallen-back line use a ceiling as an estimate, which deletes every
    /// space narrower than it.
    ///
    /// Measured rather than guessed: over `bench/pages-cov`, the
    /// best-possible single threshold for a page sits at 0.41 to 0.47
    /// x-heights at the median across the five render sizes, and the
    /// 0.7-x-height ceiling is *above* the best possible threshold on 65% to
    /// 89% of pages depending on size. `model/params.tsv` carries the sweep.
    pub no_valley_x_heights: f32,
    /// The narrowest gap a believed valley may call a word space, as a
    /// fraction of the x-height: 0 removes the floor.
    ///
    /// [`Params::lone_gap_x_heights`] is the ceiling on a valley — no valley
    /// may sit where a gap that wide is called intra-word. This is the same
    /// claim from the other side: no valley may sit where a gap that *narrow*
    /// is called a word space. An inter-word space is about a quarter to a
    /// third of an em and an x-height is about half an em, so a word space is
    /// roughly half to two-thirds of an x-height; tight tracking narrows it
    /// but does not take it near zero.
    ///
    /// # What has no floor gets wrong
    ///
    /// A valley search given only intra-word gaps still finds a valley among
    /// them, and Otsu separability on `1, 1, 3, 1, 2` px is high enough to
    /// believe it. The line then splits at a gap of one or two pixels and a
    /// word becomes its letters. That never bound while lines ran the width
    /// of the page, because any line long enough to hold several words holds
    /// their spaces too — it appears the moment a line is cut into segments
    /// that are each a single word, which is what
    /// [`crate::layout::lines::Params::column_gap_heights`] does on a form or
    /// an invoice.
    ///
    /// A valley that would call such a gap a space is not believed, and the
    /// line falls back to [`Params::no_valley_x_heights`] like any other line
    /// with nothing to say — which is the honest reading of a segment whose
    /// gaps contain no word space at all.
    ///
    /// # Its relationship to the fallback
    ///
    /// [`Params::no_valley_x_heights`] is the ceiling on this value, not a
    /// target for it. At equality the rule reads: a valley is believed only if
    /// it calls a space that the no-information rule would also call a space.
    /// Above equality it contradicts itself — it would reject a valley for
    /// calling a gap a space and then fall back to a rule that calls the same
    /// gap a space. Below equality it lets a valley believe narrower spaces
    /// than the default rule would, which is what tightly set text needs, and
    /// that is where the measurement put it.
    ///
    /// # What it resolves to
    ///
    /// The conversion truncates, so the value selects an integer pixel floor
    /// and is coarse at small x-heights: on a 9 px x-height every value from
    /// 0.23 to 0.33 is a 2 px floor and every value from 0.34 to 0.44 is a
    /// 3 px floor. A sweep of this parameter is really a sweep of that
    /// staircase.
    pub min_valley_x_heights: f32,
    /// Fewest cells a fragment must have before the fixed-pitch test runs at
    /// all -- cells after merging overlapping members together (see
    /// [`Cell`]), not raw member boxes, so a colon's two dots or a glyph the
    /// binarizer split in two count once, not twice. `0` disables the test
    /// outright — not "satisfied trivially by having at least zero cells" —
    /// so that a caller measuring whether this feature changed anything gets
    /// the byte-for-byte prior behaviour, not a test that fires on every
    /// two-cell fragment.
    ///
    /// `ARCHITECTURE.md` section 11, "fixed-pitch detection is next": a
    /// guess, and on chunk 8's tuning list, per `model/params.tsv`.
    pub pitch_min_glyphs: usize,
    /// The fraction of a fragment's centre-to-centre distances that must sit
    /// on an integer multiple of the median before the fragment is called
    /// fixed-pitch. See [`Params::pitch_min_glyphs`] for provenance.
    pub pitch_agreement: f64,
    /// How far a centre-to-centre distance may sit from an integer multiple
    /// of the median pitch and still count as agreeing, as a fraction of
    /// that median. See [`Params::pitch_min_glyphs`] for provenance.
    pub pitch_tolerance: f32,
    /// Diagnostic-only toggle for the amendment ablation
    /// (`docs/measurements/2026-09-22_fixed_pitch_spaces.txt`, "Amendment
    /// ablation"): `false` skips the cell-merge step (the amendment's item
    /// 1) and tests raw member boxes as pitch positions, exactly the
    /// pre-amendment behaviour. Default `true`; both toggles `true` must
    /// stay byte-identical to the amended build.
    pub pitch_cell_merge: bool,
    /// Diagnostic-only toggle, same provenance as
    /// [`Params::pitch_cell_merge`]: `false` skips the fitted-grid residual
    /// test (`ARCHITECTURE.md` section 11's third amendment, "the grid vote
    /// failed too", items 1-4) and falls back to the pre-amendment
    /// median-agreement test and `d >= 1.5p` split -- the second amendment's
    /// pairwise grid vote is gone entirely, not merely disabled, since it was
    /// found to fail its own gate (monospace recall 66.6%) and is not worth
    /// reproducing. `true` fits a straight line through the fragment's cell
    /// positions instead of voting on pairwise distances -- see
    /// [`fitted_grid`]. Default `true`, unless the fitted grid fails its own
    /// gate, in which case `model/params.tsv` records the bounded fallback
    /// and this default is `false`; either way both toggles' behaviour is
    /// fixed here, not by which one ships.
    pub pitch_grid_check: bool,
}

/// The shipped values, taken from the parameter block rather than restated.
///
/// The block in [`crate::params`] is the single definition of every knob, and
/// it is what `model/params.tsv` is asserted against at build time. A second
/// copy here would be a second definition that nothing compares.
impl Default for Params {
    fn default() -> Self {
        crate::params::Params::DEFAULT.words()
    }
}

/// One word: the line members it spans, and its box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordSpan {
    /// Component indices, left to right — the same indices [`TextLine`]
    /// carries, so they index the components slice, not the line.
    pub members: Vec<usize>,
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl WordSpan {
    pub fn width(&self) -> u32 {
        self.x1 - self.x0
    }
    pub fn height(&self) -> u32 {
        self.y1 - self.y0
    }
}

/// Where a line's space threshold came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThresholdSource {
    /// The valley between two populations the line's own gaps showed.
    Valley,
    /// Too few gaps, or too little separation, to find a valley; the
    /// x-height fallback decided instead.
    Fallback,
    /// A valley was found, but above the x-height ceiling, so the ceiling
    /// decided. Reported separately because a capped line is the one whose
    /// gap distribution had a third population in it.
    Capped,
    /// Every gap on the line is the same width, so the ceiling decided.
    ///
    /// Identical gaps are not an absence of evidence to be fallen back from
    /// — they are positive evidence of one population, which is what a
    /// monospaced run of one word looks like. Splitting such a line at the
    /// fallback would invent spaces inside exactly the identifier-shaped
    /// text `CLAUDE.md` rule 6 exists to protect.
    ///
    /// Untested against real input: no line in `bench/pages-cov` or
    /// `bench/holdout-cov` has perfectly uniform gaps, so this branch is
    /// measurably inert on both — a guard against a case the corpora do not
    /// contain, not one with a demonstrated catch.
    Uniform,
    /// The fragment's own centre-to-centre distances passed the fixed-pitch
    /// test ([`Params::pitch_min_glyphs`]), so it split by cell position — a
    /// space is an empty cell, `d_i >= 1.5` times the median pitch — instead
    /// of by the gap-valley rule, pooled or not.
    ///
    /// `ARCHITECTURE.md` section 11, "fixed-pitch detection is next": the
    /// case this exists for is a monospace numeric run like `112.50`, where a
    /// narrow glyph's wide side bearings make an intra-word gap read as a
    /// space under any gap threshold, but never move that glyph off its cell.
    FixedPitch,
}

/// The rule a line splits by: gaps strictly greater than `threshold` are
/// spaces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpaceRule {
    pub threshold: u32,
    pub source: ThresholdSource,
    /// Otsu's separability at the chosen threshold, `0.0` when no
    /// distribution was analysed. Reported so a caller — the decoder's
    /// segmentation prior, or a diagnostic — can tell a confident split from
    /// a forced one.
    pub separability: f64,
}

/// Horizontal gaps between a line's members, one per adjacent pair, in the
/// line's left-to-right order.
///
/// Measured from a running right edge rather than from the previous
/// component's own right edge, so a tall component that another one nests
/// beside — an `i` and its dot, a kerned pair whose boxes overlap — does not
/// produce a spurious gap. Overlap counts as zero, never as negative.
pub fn gaps(line: &TextLine, components: &[Component]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut cursor: Option<u32> = None;
    for &i in &line.members {
        let c = &components[i];
        if let Some(right) = cursor {
            out.push(c.x0.saturating_sub(right));
        }
        cursor = Some(cursor.map_or(c.x1, |r| r.max(c.x1)));
    }
    out
}

/// The space threshold for a line, with the default parameters.
pub fn space_rule(line: &TextLine, components: &[Component]) -> SpaceRule {
    space_rule_with(&gaps(line, components), line.x_height, &Params::default())
}

/// The space threshold for a set of gaps.
pub fn space_rule_with(gaps: &[u32], x_height: f32, p: &Params) -> SpaceRule {
    let fallback = SpaceRule {
        threshold: threshold_at(x_height, p.no_valley_x_heights),
        source: ThresholdSource::Fallback,
        separability: 0.0,
    };
    let ceiling = threshold_at(x_height, p.lone_gap_x_heights);
    if gaps.iter().min() == gaps.iter().max() {
        return SpaceRule {
            threshold: ceiling,
            source: ThresholdSource::Uniform,
            separability: 0.0,
        };
    }
    if gaps.len() < p.min_gaps {
        return fallback;
    }
    let Some((t, eta)) = valley(gaps) else { return fallback };
    if eta < p.min_separability {
        return fallback;
    }
    // The floor applies to the narrowest gap the valley would call a word
    // space, not to the threshold: the threshold is the widest *intra*-word
    // gap, which is small on any well-set line.
    if p.min_valley_x_heights > 0.0 {
        let floor = threshold_at(x_height, p.min_valley_x_heights);
        let narrowest_space = gaps.iter().copied().filter(|&g| g > t).min();
        if narrowest_space.is_none_or(|g| g <= floor) {
            return fallback;
        }
    }
    if t >= ceiling {
        return SpaceRule {
            threshold: ceiling,
            source: ThresholdSource::Capped,
            separability: eta,
        };
    }
    SpaceRule { threshold: t, source: ThresholdSource::Valley, separability: eta }
}

/// A threshold expressed as a fraction of the line's x-height. Saturating at
/// zero would split at every gap, so the floor is one pixel.
fn threshold_at(x_height: f32, x_heights: f32) -> u32 {
    let t = f64::from(x_height) * f64::from(x_heights);
    if t < 1.0 {
        1
    } else {
        t as u32
    }
}

/// Splits a line into words with the default parameters.
pub fn split(line: &TextLine, components: &[Component]) -> Vec<WordSpan> {
    split_with(line, components, &Params::default())
}

/// Splits a line into words.
///
/// A line with no members returns no words; every returned word has at least
/// one member. Tests the line for fixed pitch first -- "a whole line when
/// uncut" is a fragment too, per `ARCHITECTURE.md` section 11 -- and only
/// falls through to the gap-valley rule when it is not fixed-pitch.
pub fn split_with(line: &TextLine, components: &[Component], p: &Params) -> Vec<WordSpan> {
    if line.members.is_empty() {
        return Vec::new();
    }
    if let Some(pitch) = pitch_estimate(line, components, p) {
        return split_by_pitch(line, components, pitch, p);
    }
    let g = gaps(line, components);
    let rule = space_rule_with(&g, line.x_height, p);
    split_by_rule(line, components, &g, &rule)
}

/// The space threshold for every fragment of one band, pooling gaps across
/// fragments when the band was cut.
///
/// `ARCHITECTURE.md` section 11, "The column cut's precision collapse is
/// spurious spaces inside short fragments, not a mis-measured x-height": a
/// one-word fragment has one gap population, not two, so its own gaps show
/// Otsu a valley among *intra*-word gaps and the floor
/// ([`Params::min_valley_x_heights`]) does not always catch it, because the
/// floor is a per-x-height staircase and a fragment's narrowest called space
/// can land just above a step. The fix estimates the fragment's threshold
/// from the band it was cut from instead of from itself alone.
///
/// `fragments.len() <= 1` is a band the cut left whole (or the cut is off),
/// and gets [`space_rule_with`] on its own gaps, unchanged from before this
/// existed. Otherwise every fragment's own gaps are pooled, each expressed
/// as a multiple of *that fragment's own x-height* so mixed type sizes on
/// one band stay commensurable, and the valley search runs once on the pool.
/// The gap the cut fired on is never in the pool: it sits between two
/// fragments, not inside one, so it was never in either fragment's own gap
/// list to begin with -- nothing has to exclude it by name.
///
/// The result is applied to each fragment as that same multiple of *its own*
/// x-height, the way [`Params::lone_gap_x_heights`] and the other threshold
/// parameters already work: the pool decides a ratio, not a pixel count, and
/// only the last step converts a ratio to pixels, once per fragment.
///
/// A fragment that tests fixed-pitch ([`Params::pitch_min_glyphs`]) is
/// reported with [`ThresholdSource::FixedPitch`] and takes no part in the
/// pool: `ARCHITECTURE.md` section 11 says the gap-valley rule, "pooled or
/// not, is not consulted" for such a fragment, and its gaps are exactly the
/// tight intra-number population the same section's prior entry found
/// outvoting a band's real word spaces -- excluding them is what a fixed-
/// pitch fragment sitting beside a genuine text fragment on one band needs.
pub fn band_space_rules(
    fragments: &[TextLine],
    components: &[Component],
    p: &Params,
) -> Vec<SpaceRule> {
    band_fragment_rules(fragments, components, p).iter().map(FragmentRule::reported).collect()
}

/// Splits every fragment of one band into words, using [`band_space_rules`].
///
/// Returns one `Vec<WordSpan>` per fragment, in the same order as
/// `fragments`.
pub fn split_band_with(
    fragments: &[TextLine],
    components: &[Component],
    p: &Params,
) -> Vec<Vec<WordSpan>> {
    let rules = band_fragment_rules(fragments, components, p);
    fragments
        .iter()
        .zip(rules.iter())
        .map(|(line, rule)| {
            if line.members.is_empty() {
                return Vec::new();
            }
            match rule {
                FragmentRule::Pitch(pitch, _) => split_by_pitch(line, components, *pitch, p),
                FragmentRule::Gap(r) => {
                    let g = gaps(line, components);
                    split_by_rule(line, components, &g, r)
                }
            }
        })
        .collect()
}

/// One fragment's decided rule: either the gap-valley rule
/// ([`band_space_rules`]'s pooled or lone-fragment case), or the exact
/// `f64` pitch a fixed-pitch fragment tested on -- kept alongside a
/// [`SpaceRule`] for reporting, since [`SpaceRule::threshold`] is a rounded
/// `u32` and the split itself must use the un-rounded pitch.
enum FragmentRule {
    Gap(SpaceRule),
    Pitch(f64, SpaceRule),
}

impl FragmentRule {
    fn reported(&self) -> SpaceRule {
        match self {
            FragmentRule::Gap(r) => *r,
            FragmentRule::Pitch(_, r) => *r,
        }
    }
}

/// Shared by [`band_space_rules`] (which reports) and [`split_band_with`]
/// (which also needs the un-rounded pitch to split by), so the fixed-pitch
/// test and the pooling it excludes fragments from are computed exactly
/// once.
fn band_fragment_rules(fragments: &[TextLine], components: &[Component], p: &Params) -> Vec<FragmentRule> {
    let pitches: Vec<Option<f64>> =
        fragments.iter().map(|line| pitch_estimate(line, components, p)).collect();

    if fragments.len() <= 1 {
        return fragments
            .iter()
            .zip(&pitches)
            .map(|(line, pitch)| match pitch {
                Some(pitch) => FragmentRule::Pitch(*pitch, pitch_rule(*pitch)),
                None => FragmentRule::Gap(space_rule_with(&gaps(line, components), line.x_height, p)),
            })
            .collect();
    }

    let mut pool: Vec<f64> = Vec::new();
    for (line, pitch) in fragments.iter().zip(&pitches) {
        if pitch.is_some() || line.x_height <= 0.0 {
            continue;
        }
        for g in gaps(line, components) {
            pool.push(f64::from(g) / f64::from(line.x_height));
        }
    }

    let (threshold, source, separability) = pooled_rule(&pool, p);
    fragments
        .iter()
        .zip(&pitches)
        .map(|(line, pitch)| match pitch {
            Some(pitch) => FragmentRule::Pitch(*pitch, pitch_rule(*pitch)),
            None => FragmentRule::Gap(SpaceRule {
                threshold: threshold_at(line.x_height, threshold as f32),
                source,
                separability,
            }),
        })
        .collect()
}

/// The band-pooled threshold, in x-height multiples, and where it came from.
///
/// Mirrors [`space_rule_with`] exactly, but in ratio space: every gap in
/// `pool` is already a multiple of its own fragment's x-height, so the
/// ceiling, floor and fallback parameters -- themselves stated as x-height
/// multiples -- apply to the pool with no further conversion. Only the
/// caller ([`band_space_rules`]) converts back to pixels, once per fragment
/// against that fragment's own x-height.
fn pooled_rule(pool: &[f64], p: &Params) -> (f64, ThresholdSource, f64) {
    let fallback = (f64::from(p.no_valley_x_heights), ThresholdSource::Fallback, 0.0);
    let ceiling = f64::from(p.lone_gap_x_heights);
    if pool.is_empty() {
        return fallback;
    }
    let lo = pool.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = pool.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if lo == hi {
        return (ceiling, ThresholdSource::Uniform, 0.0);
    }
    if pool.len() < p.min_gaps {
        return fallback;
    }
    let Some((t, eta)) = valley_f64(pool) else { return fallback };
    if eta < p.min_separability {
        return fallback;
    }
    if p.min_valley_x_heights > 0.0 {
        let floor = f64::from(p.min_valley_x_heights);
        let narrowest_space =
            pool.iter().copied().filter(|&g| g > t).fold(f64::INFINITY, f64::min);
        if !(narrowest_space > floor) {
            return fallback;
        }
    }
    if t >= ceiling {
        return (ceiling, ThresholdSource::Capped, eta);
    }
    (t, ThresholdSource::Valley, eta)
}

/// One pitch position: a run of adjacent line members whose x-extents
/// overlap, merged into a single cell whose box is their union.
///
/// `ARCHITECTURE.md` section 11's amendment, item 1. Two components can
/// share a glyph cell without being one component -- a colon's two dots, an
/// `i`'s dot separated from its stem, a shape the binarizer split into
/// touching-but-separate pieces -- and counting each as its own pitch
/// position perturbs every neighbouring centre distance: `ARCHITECTURE.md`
/// section 11's defect 2. Overlap in x is the definition of sharing a cell,
/// so no new parameter is needed to decide it.
struct Cell {
    /// The position, one past this cell's last member, into `line.members`
    /// (not component indices) -- the only part of the cell's member range
    /// [`split_by_pitch`] needs, to find the member-level boundary a split
    /// between this cell and the next one falls on.
    end: usize,
    x0: u32,
    x1: u32,
}

impl Cell {
    fn centre(&self) -> f64 {
        (f64::from(self.x0) + f64::from(self.x1)) / 2.0
    }

    fn width(&self) -> u32 {
        self.x1 - self.x0
    }

    /// The centre of this cell's virtual sub-cell nearest its named edge,
    /// treating the cell as `round(width / pitch)` equal-width glyph
    /// positions rather than one.
    ///
    /// Only meaningful for a [`touching_runs`] cell. `ARCHITECTURE.md`
    /// section 11's second amendment, item 4: such a cell's own box centre
    /// sits at the run's *midpoint*, not at either fused glyph's true
    /// position, and using it to measure the distance to a real neighbouring
    /// cell is exactly the bias that manufactures a space beside a touching
    /// pair that was never there.
    fn edge_centre(&self, pitch: f64, near_left: bool) -> f64 {
        let width = f64::from(self.width());
        let n = (width / pitch).round().max(2.0);
        let slice = width / n;
        if near_left {
            f64::from(self.x0) + slice / 2.0
        } else {
            f64::from(self.x1) - slice / 2.0
        }
    }
}

/// Which of `cells` are *touching runs*: `ARCHITECTURE.md` section 11's
/// second amendment, "the grid check was all-or-nothing", item 2. A cell at
/// least `(2 - pitch_tolerance) * p` wide is treated as `round(width / p)`
/// fused glyph positions rather than one, because the amendment's ablation
/// (`docs/measurements/2026-09-22_fixed_pitch_spaces.txt`, "Amendment
/// ablation", task 2) traced most of the grid check's mono-recall cost to
/// exactly this shape recurring in real monospace rendering: two
/// neighbouring glyphs' anti-aliased edges already touching before `cells()`
/// ever ran, producing one component about `2p` wide whose union-box centre
/// sits roughly half a pitch off either neighbour's true grid position.
///
/// [`pitch_estimate`] excuses a wide distance flanking a touching run from
/// its vote; [`split_by_pitch`] measures such a distance from the run's
/// nearest sub-cell centre rather than its box centre (item 4). Uses only
/// the existing tolerance -- no new parameter.
fn touching_runs(cells: &[Cell], pitch: f64, tolerance: f32) -> Vec<bool> {
    let threshold = (2.0 - f64::from(tolerance)) * pitch;
    cells.iter().map(|c| f64::from(c.width()) >= threshold).collect()
}

/// Groups a line's members into [`Cell`]s, left to right.
///
/// A member starts a new cell unless its left edge is before the running
/// right edge of the cell so far -- the same overlap test [`gaps`] uses to
/// keep a nested box (an `i`'s dot tucked over the previous letter) from
/// reading as a negative gap, applied here to merge rather than to zero.
///
/// `merge` is the amendment ablation toggle
/// ([`Params::pitch_cell_merge`]): `false` turns every member into its own
/// cell, with no x-overlap fusion at all -- the pre-amendment behaviour,
/// kept here rather than in the caller so [`pitch_estimate`] and
/// [`split_by_pitch`] always agree on what a "cell" is.
fn cells(line: &TextLine, components: &[Component], merge: bool) -> Vec<Cell> {
    if !merge {
        return line
            .members
            .iter()
            .enumerate()
            .map(|(pos, &i)| {
                let c = &components[i];
                Cell { end: pos + 1, x0: c.x0, x1: c.x1 }
            })
            .collect();
    }
    let mut out = Vec::new();
    let (mut x0, mut x1) = (0u32, 0u32);
    for (pos, &i) in line.members.iter().enumerate() {
        let c = &components[i];
        if pos == 0 {
            x0 = c.x0;
            x1 = c.x1;
            continue;
        }
        if c.x0 < x1 {
            x0 = x0.min(c.x0);
            x1 = x1.max(c.x1);
        } else {
            out.push(Cell { end: pos, x0, x1 });
            x0 = c.x0;
            x1 = c.x1;
        }
    }
    out.push(Cell { end: line.members.len(), x0, x1 });
    out
}

/// Centre-to-centre horizontal distances between adjacent cells, aligned
/// with `cells`: entry `n` is the distance between `cells[n]` and
/// `cells[n + 1]`.
///
/// Centre position, not edge, because it is invariant to glyph width, which a
/// gap is not -- exactly the property `ARCHITECTURE.md` section 11's
/// fixed-pitch test needs: a narrow glyph's wide side bearings move a gap
/// without moving its cell.
fn cell_distances(cells: &[Cell]) -> Vec<f64> {
    cells.windows(2).map(|w| w[1].centre() - w[0].centre()).collect()
}

/// The median of a value set, `None` when empty. An even count averages the
/// two middle values; the pitch estimate this feeds is a magnitude, not a
/// position a tie needs broken towards one side of.
fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("centre distances are finite"));
    let n = sorted.len();
    Some(if n % 2 == 1 { sorted[n / 2] } else { (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0 })
}

/// One fragment fitted to a straight-line grid: `ARCHITECTURE.md` section
/// 11's third amendment, "the grid vote failed too", items 1-2.
///
/// A centre-to-centre *distance* carries the placement error of both its
/// ends, so a vote over distances lets one off-centre glyph or one touching
/// run spoil two votes at once, and a short line has only a handful of wide
/// distances to vote with -- the mechanism the second amendment's vote
/// (removed; see [`Params::pitch_grid_check`]) could not escape. Fitting a
/// line through *positions* instead uses every position once: a monospace
/// line's defining property is that every cell centre sits near `x0 + n*p`,
/// and one bad position costs the fit one residual among many, not two
/// votes among few.
struct FittedGrid {
    /// `bounds[k]` is the index into `index`/the position list where
    /// `cells[k]`'s own positions begin; `bounds[cells.len()]` is the total
    /// position count. A [`Cell`] is one physical region, so a split may
    /// only ever fall at a `bounds` entry, never inside the range it opens --
    /// this is what lets [`split_by_pitch_grid`] map an index jump back to a
    /// boundary between original components.
    bounds: Vec<usize>,
    /// Sequential index `n_i` (item 2), one per position, aligned with the
    /// flattened position list `bounds` indexes into.
    index: Vec<i64>,
    /// The least-squares fit's slope: the fitted pitch `p`, distinct from
    /// the median `p0` fed in, which item 2 uses only to assign `index`.
    pitch: f64,
    /// How many positions lie within `pitch_tolerance * pitch` of their
    /// fitted `x0 + n_i * pitch` -- item 3's numerator.
    agree: usize,
    /// The total position count -- item 3's denominator, which is `cells.len()`
    /// plus one extra per [`touching_runs`] cell's virtual subdivisions
    /// beyond its first.
    len: usize,
}

/// Fits `cells` to a straight-line grid, `ARCHITECTURE.md` section 11's
/// third amendment, items 1-2: `p0` is the median centre-to-centre distance
/// ([`pitch_estimate`]'s existing estimate, computed once and passed in
/// rather than recomputed, so both callers -- [`pitch_estimate`] and
/// [`split_by_pitch_grid`] -- agree on it exactly).
///
/// 1. A [`touching_runs`] cell -- one raw component already fused before
///    `cells()` ran, `round(width / p0)` glyph positions wide -- contributes
///    that many virtual centres, at its box's equal subdivisions, rather
///    than the one centre its own box gives. An ordinary cell contributes
///    its own centre only.
/// 2. Positions are indexed sequentially left to right, `n_0 = 0`,
///    `n_i = n_{i-1} + max(1, round(d_i / p0))`, `d_i` the distance between
///    consecutive positions (including the equal spacing within one
///    touching run's own subdivisions); then `x = x0 + n*p` is fit to every
///    `(n_i, x_i)` pair by ordinary least squares in `f64`.
///
/// `None` only when the fit is degenerate -- every position sharing one
/// index, so no slope exists, or the fitted slope is non-positive -- which
/// [`pitch_estimate`]'s [`Params::pitch_min_glyphs`] floor above it is not
/// provably sufficient to rule out for every pathological fragment, so this
/// stays a guard rather than an `expect`.
fn fitted_grid(cells: &[Cell], p0: f64, tolerance: f32) -> Option<FittedGrid> {
    let touching = touching_runs(cells, p0, tolerance);
    let mut positions: Vec<f64> = Vec::with_capacity(cells.len());
    let mut bounds: Vec<usize> = Vec::with_capacity(cells.len() + 1);
    for (cell, &is_touch) in cells.iter().zip(&touching) {
        bounds.push(positions.len());
        if is_touch {
            let width = f64::from(cell.width());
            let n = (width / p0).round().max(2.0) as usize;
            let slice = width / n as f64;
            for k in 0..n {
                positions.push(f64::from(cell.x0) + slice * (k as f64 + 0.5));
            }
        } else {
            positions.push(cell.centre());
        }
    }
    bounds.push(positions.len());

    let mut index: Vec<i64> = Vec::with_capacity(positions.len());
    index.push(0);
    for w in positions.windows(2) {
        let step = (((w[1] - w[0]) / p0).round() as i64).max(1);
        index.push(index.last().expect("just pushed n_0") + step);
    }

    let n = positions.len() as f64;
    let mean_n = index.iter().map(|&i| i as f64).sum::<f64>() / n;
    let mean_x = positions.iter().sum::<f64>() / n;
    let mut sxx = 0.0f64;
    let mut sxy = 0.0f64;
    for (&idx, &x) in index.iter().zip(&positions) {
        let dn = idx as f64 - mean_n;
        sxx += dn * dn;
        sxy += dn * (x - mean_x);
    }
    if sxx <= 0.0 {
        return None;
    }
    let pitch = sxy / sxx;
    if pitch <= 0.0 {
        return None;
    }
    let x0 = mean_x - pitch * mean_n;
    let tol = f64::from(tolerance) * pitch;
    let agree = index
        .iter()
        .zip(&positions)
        .filter(|&(&idx, &x)| (x - (x0 + pitch * idx as f64)).abs() <= tol)
        .count();

    Some(FittedGrid { bounds, index, pitch, agree, len: positions.len() })
}

/// Whether a fragment is fixed-pitch, and its pitch estimate if so.
///
/// `ARCHITECTURE.md` section 11, "fixed-pitch detection is next", amended
/// three times: cell merge, then the grid vote (removed; see
/// [`Params::pitch_grid_check`]), then the fitted grid below. A fragment is
/// fixed-pitch when, after merging overlapping members into [`Cell`]s --
///
/// 1. it has at least [`Params::pitch_min_glyphs`] cells; and
/// 2. either (`pitch_grid_check == true`, the third amendment): [`fitted_grid`]
///    fits at least [`Params::pitch_agreement`] of its positions within
///    [`Params::pitch_tolerance`] times the fitted pitch of their place on
///    the line -- the estimate returned is that fitted pitch; or
///    (`pitch_grid_check == false`, pre-amendment): at least
///    [`Params::pitch_agreement`] of the cells' centre-to-centre distances
///    lie within `pitch_tolerance` times the median distance `p0` of an
///    integer multiple `k * p0`, `k >= 1` -- the estimate returned is `p0`.
///
/// `pitch_min_glyphs == 0` disables the test outright rather than being
/// satisfied trivially by "at least zero cells" -- see its doc comment.
fn pitch_estimate(line: &TextLine, components: &[Component], p: &Params) -> Option<f64> {
    if p.pitch_min_glyphs == 0 {
        return None;
    }
    let cells = cells(line, components, p.pitch_cell_merge);
    if cells.len() < p.pitch_min_glyphs {
        return None;
    }
    let d = cell_distances(&cells);
    let p0 = median(&d)?;
    if p0 <= 0.0 {
        return None;
    }

    if p.pitch_grid_check {
        let grid = fitted_grid(&cells, p0, p.pitch_tolerance)?;
        if grid.len < p.pitch_min_glyphs {
            return None;
        }
        if (grid.agree as f64) < p.pitch_agreement * grid.len as f64 {
            return None;
        }
        return Some(grid.pitch);
    }

    let tolerance = f64::from(p.pitch_tolerance) * p0;
    let agree = d
        .iter()
        .filter(|&&di| {
            let k = (di / p0).round().max(1.0);
            (di - k * p0).abs() <= tolerance
        })
        .count();
    if (agree as f64) < p.pitch_agreement * d.len() as f64 {
        return None;
    }
    Some(p0)
}

/// The reportable [`SpaceRule`] for a fixed-pitch fragment: the threshold is
/// `1.5 * pitch` rounded for display, since [`SpaceRule::threshold`] is a
/// `u32`; the actual split ([`split_by_pitch`]) compares the un-rounded
/// pitch instead. `separability` is `0.0`, the same as [`ThresholdSource::Uniform`]
/// reports -- no distribution was analysed, a cell position was.
fn pitch_rule(pitch: f64) -> SpaceRule {
    let threshold = (1.5 * pitch).round().max(1.0) as u32;
    SpaceRule { threshold, source: ThresholdSource::FixedPitch, separability: 0.0 }
}

/// Splits `line` at every point `is_space` marks, walking members left to
/// right. `is_space(n)` asks about the gap or distance between `members[n]`
/// and `members[n + 1]`.
///
/// The one splitting loop both [`split_by_rule`] (gap-valley, `>` a pixel
/// threshold) and [`split_by_pitch`] (fixed-pitch, `>=` a distance threshold)
/// share, per `CLAUDE.md` rule 4 -- the two differ only in which quantity and
/// which comparison decide a space, not in how a decided space cuts the
/// member list.
fn split_by(line: &TextLine, components: &[Component], is_space: impl Fn(usize) -> bool) -> Vec<WordSpan> {
    let mut out: Vec<WordSpan> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    for (n, &i) in line.members.iter().enumerate() {
        if n > 0 && is_space(n - 1) && !current.is_empty() {
            out.push(box_of(&current, components));
            current = Vec::new();
        }
        current.push(i);
    }
    if !current.is_empty() {
        out.push(box_of(&current, components));
    }
    out
}

/// Splits `line` at every gap in `g` strictly wider than `rule.threshold`.
///
/// Shared by [`split_with`], which measures `rule` from the line's own gaps,
/// and [`split_band_with`], which measures it from the band pool instead --
/// the split itself does not care which.
fn split_by_rule(
    line: &TextLine,
    components: &[Component],
    g: &[u32],
    rule: &SpaceRule,
) -> Vec<WordSpan> {
    split_by(line, components, |n| g[n] > rule.threshold)
}

/// Splits a fixed-pitch fragment into words.
///
/// `pitch_grid_check == true` (the third amendment) delegates to
/// [`split_by_pitch_grid`): a space stands wherever [`fitted_grid`]'s
/// sequential index advances by two or more, item 4. `pitch_grid_check ==
/// false` keeps the pre-amendment rule below unchanged: a space at every
/// [`Cell`] boundary whose centre-to-centre distance is at least
/// `1.5 * pitch` -- an empty cell -- non-strict, unlike [`split_by_rule`]'s
/// gap threshold, since the rule is stated as `d_i >= 1.5p`, not `>`. A
/// distance that would be *wide* (`d >= (1 + pitch_tolerance) * p`) with a
/// [`touching_runs`] cell at either end is measured from that cell's nearest
/// sub-cell centre instead of its box centre -- the cell-merge amendment's
/// own correction, which stays live in both branches since it is not what
/// `pitch_grid_check` controls.
///
/// A split only ever falls between cells, never inside one, in either
/// branch: two members merged into one cell by x-overlap stay together
/// whatever the pitch rule says about that cell's neighbours, so a boundary
/// a caller sees split at is always a boundary between original component
/// indices, never through one.
fn split_by_pitch(
    line: &TextLine,
    components: &[Component],
    pitch: f64,
    p: &Params,
) -> Vec<WordSpan> {
    let cells = cells(line, components, p.pitch_cell_merge);

    if p.pitch_grid_check {
        return split_by_pitch_grid(line, components, &cells, p);
    }

    let raw = cell_distances(&cells);
    let touching = touching_runs(&cells, pitch, p.pitch_tolerance);
    let wide = (1.0 + f64::from(p.pitch_tolerance)) * pitch;
    let threshold = 1.5 * pitch;
    let mut member_is_space = vec![false; line.members.len().saturating_sub(1)];
    for (n, &di) in raw.iter().enumerate() {
        let excused = di >= wide && (touching[n] || touching[n + 1]);
        let measured = if excused {
            let left =
                if touching[n] { cells[n].edge_centre(pitch, false) } else { cells[n].centre() };
            let right = if touching[n + 1] {
                cells[n + 1].edge_centre(pitch, true)
            } else {
                cells[n + 1].centre()
            };
            right - left
        } else {
            di
        };
        if measured >= threshold {
            member_is_space[cells[n].end - 1] = true;
        }
    }
    split_by(line, components, |n| member_is_space[n])
}

/// Splits by `ARCHITECTURE.md` section 11's third amendment, item 4: a space
/// stands at every [`Cell`] boundary whose [`fitted_grid`] sequential index
/// advances by two or more, `n_{i} - n_{i-1} >= 2` -- an empty cell, which is
/// what `d >= 1.5p` approximated.
///
/// Recomputes `p0` and refits the grid from `cells` rather than threading a
/// [`FittedGrid`] through from [`pitch_estimate`]: both run the identical
/// deterministic computation over the identical `cells` and `p`, so they
/// agree exactly, the same way the pre-amendment [`split_by_pitch`] already
/// recomputed [`cell_distances`] and [`touching_runs`] independently of the
/// estimate that decided to call it.
///
/// Degrades to "no splits" if the refit is somehow degenerate -- provably
/// unreachable once [`pitch_estimate`] has already fit the same inputs
/// successfully, kept as a guard rather than a panic for the reason
/// [`fitted_grid`] states, and inert on the corpus in exactly the sense
/// [`ThresholdSource::Uniform`] already is.
fn split_by_pitch_grid(
    line: &TextLine,
    components: &[Component],
    cells: &[Cell],
    p: &Params,
) -> Vec<WordSpan> {
    let mut member_is_space = vec![false; line.members.len().saturating_sub(1)];
    if let Some(p0) = median(&cell_distances(cells)) {
        if let Some(grid) = fitted_grid(cells, p0, p.pitch_tolerance) {
            for k in 0..cells.len().saturating_sub(1) {
                let last_of_k = grid.bounds[k + 1] - 1;
                let first_of_next = grid.bounds[k + 1];
                if grid.index[first_of_next] - grid.index[last_of_k] >= 2 {
                    member_is_space[cells[k].end - 1] = true;
                }
            }
        }
    }
    split_by(line, components, |n| member_is_space[n])
}

fn box_of(members: &[usize], components: &[Component]) -> WordSpan {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for &i in members {
        let c = &components[i];
        x0 = x0.min(c.x0);
        y0 = y0.min(c.y0);
        x1 = x1.max(c.x1);
        y1 = y1.max(c.y1);
    }
    WordSpan { members: members.to_vec(), x0, y0, x1, y1 }
}

/// Otsu's threshold over a gap multiset, and its separability.
///
/// Otsu because "the valley between two populations" is precisely the
/// threshold that maximises between-class variance, and because it takes no
/// parameter of its own — the threshold comes out of the line's gaps and
/// nothing else. Returns the last gap value belonging to the intra-word
/// class, so a gap *strictly greater* is a space.
///
/// `None` when every gap is identical, which has no valley. Ties take the
/// smaller threshold: splitting at more places is the reading that keeps
/// more information, since a word wrongly split is repairable by the decoder
/// and two words wrongly joined are not.
///
/// A thin wrapper over [`valley_f64`], which the band-pooled space rule of
/// `ARCHITECTURE.md` section 11 needs in ratio space rather than integer
/// pixels; kept as one implementation rather than two, per `CLAUDE.md` rule
/// 4. `t as u32` is exact: `valley_f64` always returns a value copied from
/// `gaps` itself, never an interpolated one, and every value here started as
/// a `u32` cast up to `f64`, which is lossless at these magnitudes.
fn valley(gaps: &[u32]) -> Option<(u32, f64)> {
    let values: Vec<f64> = gaps.iter().map(|&g| f64::from(g)).collect();
    let (t, eta) = valley_f64(&values)?;
    Some((t as u32, eta))
}

/// Otsu's threshold over a continuous multiset, and its separability.
///
/// Generalises the histogram [`valley`] uses to values that need not be
/// integers — the unit the band pool works in, since each fragment's gap is
/// expressed as a multiple of that fragment's own x-height before pooling.
///
/// Grouping by distinct value and only testing a threshold once every value
/// equal to it has joined the intra-word class reproduces exactly what
/// [`valley`]'s histogram computes: a candidate whose class gains no new
/// mass over its predecessor cannot win, because between-class variance is a
/// function of the cumulative mass and sum alone, so a zero-count histogram
/// bin between two present values can never be the winning threshold. Ties
/// take the smaller threshold, for the reason [`valley`] states.
fn valley_f64(values: &[f64]) -> Option<(f64, f64)> {
    if values.len() < 2 {
        return None;
    }
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("gap ratios are finite"));
    let lo = sorted[0];
    let hi = *sorted.last().expect("checked non-empty above");
    if hi <= lo {
        return None;
    }

    let total = sorted.len() as f64;
    let sum: f64 = sorted.iter().sum();
    let mean = sum / total;
    let variance: f64 =
        sorted.iter().map(|&v| (v - mean) * (v - mean)).sum::<f64>() / total;
    if variance <= 0.0 {
        return None;
    }

    let (mut w0, mut s0) = (0f64, 0f64);
    let mut best: Option<(f64, f64)> = None;
    let mut i = 0usize;
    while i < sorted.len() {
        let v = sorted[i];
        let mut j = i;
        while j < sorted.len() && sorted[j] == v {
            w0 += 1.0;
            s0 += v;
            j += 1;
        }
        // `v` has now fully joined class 0. Only a candidate if class 1 is
        // still non-empty -- the same `w1 == 0` guard `valley` states, just
        // never reached instead of skipped, since there is nothing past the
        // last distinct value to skip to.
        if j < sorted.len() {
            let w1 = total - w0;
            let m0 = s0 / w0;
            let m1 = (sum - s0) / w1;
            let d = m0 - m1;
            let between = (w0 / total) * (w1 / total) * d * d;
            if best.is_none_or(|(_, b)| between > b) {
                best = Some((v, between));
            }
        }
        i = j;
    }
    best.map(|(t, between)| (t, between / variance))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::lines::{self, XHeightSource};

    fn c(label: u32, x0: u32, y0: u32, w: u32, h: u32) -> Component {
        Component { label, x0, y0, x1: x0 + w, y1: y0 + h, area: w * h, border_coverage: [1.0; 4] }
    }

    /// Builds a line of glyph boxes at the given left edges, all 8 wide and
    /// 10 tall, and groups it so the metrics are real rather than asserted.
    fn line_of(lefts: &[u32]) -> (Vec<Component>, lines::TextLine) {
        let comps: Vec<Component> =
            lefts.iter().enumerate().map(|(k, &x)| c(k as u32 + 1, x, 20, 8, 10)).collect();
        let mut ls = lines::group(&comps, 600, 100);
        assert_eq!(ls.len(), 1);
        (comps, ls.remove(0))
    }

    /// The case the whole stage exists for: two gap populations, tight
    /// inside words and wide between them, with no fixed multiple involved.
    #[test]
    fn a_line_splits_at_the_valley_of_its_own_gaps() {
        // "abc def gh": letters 2 apart, words 12 apart.
        let (comps, line) = line_of(&[0, 10, 20, 42, 52, 62, 84, 94]);
        let rule = space_rule(&line, &comps);
        assert_eq!(rule.source, ThresholdSource::Valley);
        let words = split(&line, &comps);
        assert_eq!(words.len(), 3);
        assert_eq!(words[0].members.len(), 3);
        assert_eq!(words[1].members.len(), 3);
        assert_eq!(words[2].members.len(), 2);
        assert_eq!((words[0].x0, words[0].x1), (0, 28));
    }

    /// A monospaced run has one gap population, not two. Splitting it
    /// anywhere would be inventing a space, so identical gaps take the
    /// ceiling and the line stays whole — the fallback would split this run
    /// into ten single letters, which is the identifier case rule 6 exists
    /// to protect.
    #[test]
    fn an_evenly_spaced_run_is_one_word() {
        let lefts: Vec<u32> = (0..10).map(|k| k * 12).collect();
        let (comps, line) = line_of(&lefts);
        let rule = space_rule(&line, &comps);
        assert_eq!(rule.source, ThresholdSource::Uniform);
        assert_eq!(split(&line, &comps).len(), 1);
    }

    /// Uniform is not "never split": a run spaced wider than the ceiling is
    /// still spaced, however evenly it is done.
    #[test]
    fn an_evenly_spaced_run_wider_than_the_ceiling_is_all_spaces() {
        let lefts: Vec<u32> = (0..5).map(|k| k * 20).collect();
        let (comps, line) = line_of(&lefts);
        let rule = space_rule(&line, &comps);
        assert_eq!(rule.source, ThresholdSource::Uniform);
        assert_eq!(split(&line, &comps).len(), 5);
    }

    /// Two words and one gap: no distribution to analyse, but a gap wider
    /// than the x-height is still a space.
    #[test]
    fn a_single_wide_gap_still_separates_two_words() {
        let (comps, line) = line_of(&[0, 10, 34]);
        assert_eq!(line.x_height_source, XHeightSource::FromCapHeight);
        let rule = space_rule(&line, &comps);
        assert_eq!(rule.source, ThresholdSource::Fallback);
        let words = split(&line, &comps);
        assert_eq!(words.len(), 2);
        assert_eq!(words[1].members.len(), 1);
    }

    /// Kerned and nested boxes must not read as gaps. An `i` with its dot
    /// tucked over the previous letter would otherwise split a word.
    #[test]
    fn overlapping_boxes_produce_a_zero_gap_not_a_negative_one() {
        let comps = vec![c(1, 0, 20, 12, 10), c(2, 8, 20, 8, 10), c(3, 18, 20, 8, 10)];
        let line = lines::group(&comps, 200, 100).remove(0);
        assert_eq!(gaps(&line, &comps), vec![0, 2]);
    }

    #[test]
    fn a_line_with_one_member_is_one_word() {
        let (comps, line) = line_of(&[5]);
        let words = split(&line, &comps);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].members, vec![0]);
    }

    /// The threshold is the last intra-word gap, so a gap equal to it stays
    /// inside the word and only a strictly wider one splits.
    #[test]
    fn the_threshold_is_inclusive_below_and_exclusive_above() {
        let rule = space_rule_with(&[1, 1, 1, 9, 9], 10.0, &Params::default());
        assert_eq!(rule.source, ThresholdSource::Valley);
        assert_eq!(rule.threshold, 1);
        assert!(rule.separability > 0.9);
    }

    /// The gaps of a single word contain no word space, so the valley among
    /// them must not be believed however cleanly it separates. `1, 1, 3, 1, 2`
    /// on a 9 px x-height is `112.50` read as one segment of an invoice's
    /// amount column.
    #[test]
    fn a_valley_among_intra_word_gaps_alone_is_not_a_word_space() {
        let p = Params::default();
        let rule = space_rule_with(&[1, 1, 3, 1, 2], 9.0, &p);
        assert_eq!(rule.source, ThresholdSource::Fallback);
        // And the fallback does not split them either, which is the point:
        // rejecting the valley has to leave the word whole.
        assert!([1, 1, 3, 1, 2].iter().all(|&g| g <= rule.threshold), "{rule:?}");
        // Removing the floor is what the old behaviour was, and it splits.
        let off = Params { min_valley_x_heights: 0.0, ..p };
        assert_eq!(space_rule_with(&[1, 1, 3, 1, 2], 9.0, &off).source, ThresholdSource::Valley);
    }

    /// The floor is a staircase, not a curve: the conversion truncates, so the
    /// shipped 0.3 is a 2 px floor on a 9 px x-height and a segment whose only
    /// called space is 3 px wide is still believed. `3.75` in an invoice's
    /// amount column is that segment. The floor is an aggregate improvement,
    /// not a guarantee about any one line, and this test is here so that stays
    /// visible rather than being discovered again.
    #[test]
    fn the_floor_is_a_staircase_and_does_not_catch_every_single_word_segment() {
        let p = Params::default();
        assert_eq!(space_rule_with(&[1, 3, 1], 9.0, &p).source, ThresholdSource::Valley);
        // One step up it is caught, which is what the sweep traded away.
        let higher = Params { min_valley_x_heights: 0.4, ..p };
        assert_eq!(space_rule_with(&[1, 3, 1], 9.0, &higher).source, ThresholdSource::Fallback);
    }

    #[test]
    fn identical_gaps_have_no_valley() {
        assert_eq!(valley(&[4, 4, 4, 4]), None);
    }

    /// The case `band_space_rules` exists for. Fragment 1 is the staircase
    /// case above -- gaps `[1, 3, 1]`, which alone on the default parameters
    /// still read as a believed valley and split "112.50"-style content into
    /// pieces. Fragment 2 is a genuine two-word fragment (gaps of 4 inside
    /// each word, 20 between them) on the same band. Pooled, fragment 1's
    /// gaps stay below the pooled threshold and it stays one word; fragment
    /// 2 still splits at its real space.
    ///
    /// Both fragments share an x-height of 8 (a power of two) so every
    /// gap-to-x-height ratio here is exact in binary floating point --
    /// the assertions exercise the pooling, not which way the last bit of
    /// an arbitrary x-height happens to round on this platform.
    #[test]
    fn a_band_pooled_threshold_keeps_a_monospace_fragment_whole() {
        let lefts: [u32; 10] = [
            0, 9, 20, 29, // fragment 1, "112.50"-style: gaps [1, 3, 1]
            77, 89, 101, // fragment 2, word A: gaps [4, 4]
            129, 141, 153, // fragment 2, word B: gaps [4, 4]
        ];
        let comps: Vec<Component> =
            lefts.iter().enumerate().map(|(k, &x)| c(k as u32 + 1, x, 20, 8, 10)).collect();

        let frag = |members: Vec<usize>, x0: u32, x1: u32| lines::TextLine {
            members,
            x0,
            y0: 20,
            x1,
            y1: 30,
            baseline: 30.0,
            x_height: 8.0,
            cap_height: 10.0,
            x_height_source: XHeightSource::FromCapHeight,
            median_height: 10,
        };
        let group = [frag(vec![0, 1, 2, 3], 0, 37), frag(vec![4, 5, 6, 7, 8, 9], 77, 161)];

        // Alone, fragment 1 reproduces the staircase bug from the test above:
        // its own three gaps still read as a believed valley.
        let p = Params::default();
        let alone = space_rule_with(&gaps(&group[0], &comps), group[0].x_height, &p);
        assert_eq!(alone.source, ThresholdSource::Valley, "{alone:?}");

        // Pooled with fragment 2, it does not, and fragment 2 still splits
        // at its real space.
        let spans = split_band_with(&group, &comps, &p);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].len(), 1, "fragment 1 must stay one word: {spans:?}");
        assert_eq!(spans[1].len(), 2, "fragment 2 must still split at its real space: {spans:?}");
    }

    /// Builds a monospaced glyph run: cells `width` px wide starting at `x0`,
    /// each glyph horizontally *centred* in its cell rather than filling it --
    /// which is what a real face's side bearings do. Centring keeps every
    /// centre position on the cell pitch regardless of glyph width, so a
    /// narrow glyph like `1` or `.` does not move off its cell even though
    /// the *gap* around it grows, which is the property
    /// `ARCHITECTURE.md` section 11's fixed-pitch test exists to use instead
    /// of the gap.
    fn monospace_cell(label: u32, cell: u32, width: u32, x0: u32, glyph_width: u32) -> Component {
        let offset = (width - glyph_width) / 2;
        let gx0 = x0 + cell * width + offset;
        c(label, gx0, 20, glyph_width, 10)
    }

    /// The case `ARCHITECTURE.md` section 11's fixed-pitch rule exists for:
    /// `112.50` in a monospace face, where `1` and `.` are narrow glyphs with
    /// wide side bearings. Every glyph is centred in a 12 px cell, so every
    /// centre-to-centre distance is exactly 12 px even though the *gaps*
    /// vary with each glyph's width -- the staircase bug
    /// `a_valley_among_intra_word_gaps_alone_is_not_a_word_space` and
    /// `a_band_pooled_threshold_keeps_a_monospace_fragment_whole` exist to
    /// catch by other means. Six glyphs meets the default
    /// `pitch_min_glyphs`, so this exercises the shipped parameters, not a
    /// relaxed test-only setting.
    #[test]
    fn a_monospace_fragment_with_wide_bearing_glyphs_stays_one_word() {
        let widths = [4u32, 4, 8, 2, 8, 8]; // '1' '1' '2' '.' '5' '0'
        let comps: Vec<Component> = widths
            .iter()
            .enumerate()
            .map(|(k, &w)| monospace_cell(k as u32 + 1, k as u32, 12, 0, w))
            .collect();
        let line = ls_line(&comps);

        // Both `pitch_grid_check` arms -- the shipped fallback and the
        // fitted grid -- must keep this one word: neither mechanism should
        // ever be stricter than plain agreement on a fragment with no gap
        // wide enough to be a word boundary at all.
        for grid in [false, true] {
            let p = Params { pitch_grid_check: grid, ..Params::default() };
            let rules = band_space_rules(std::slice::from_ref(&line), &comps, &p);
            assert_eq!(rules[0].source, ThresholdSource::FixedPitch, "grid={grid}: {rules:?}");
            let words = split_with(&line, &comps, &p);
            assert_eq!(words.len(), 1, "grid={grid}: 112.50 must stay one word: {words:?}");
        }

        // Control: pitch_min_glyphs = 0 disables the test outright and must
        // reproduce the old gap-valley behaviour -- here, splitting on the
        // spurious gaps around the narrow glyphs.
        let off = Params { pitch_min_glyphs: 0, ..Params::default() };
        let words_off = split_with(&line, &comps, &off);
        assert!(words_off.len() > 1, "the control must exercise the bug the feature fixes: {words_off:?}");
    }

    /// Regroups `comps` into the one line it forms, the way [`line_of`] does
    /// but without rebuilding boxes of a fixed width -- this module's
    /// fixed-pitch tests need the real per-glyph widths preserved.
    fn ls_line(comps: &[Component]) -> lines::TextLine {
        let mut ls = lines::group(comps, 200, 100);
        assert_eq!(ls.len(), 1, "fixture must be one line");
        ls.remove(0)
    }

    /// `QTY 123`-shaped: three letter cells, one empty cell standing for the
    /// space, then three digit cells, all on a 12 px pitch. The empty cell is
    /// exactly `k = 2` pitches, inside `pitch_tolerance` of it, and `1.5 *
    /// pitch` classes it a space -- the "chop by pitch position" rule
    /// `ARCHITECTURE.md` section 11 specifies, applied to the case Tesseract's
    /// survey names it for. Six glyphs meets the default `pitch_min_glyphs`.
    #[test]
    fn a_monospace_fragment_splits_at_its_empty_cell() {
        let letters = [0u32, 1, 2]; // Q T Y at cells 0, 1, 2
        let digits = [4u32, 5, 6]; // 1 2 3 at cells 4, 5, 6 -- cell 3 is empty
        let comps: Vec<Component> = letters
            .iter()
            .chain(digits.iter())
            .enumerate()
            .map(|(k, &cell)| monospace_cell(k as u32 + 1, cell, 12, 0, 8))
            .collect();
        let line = ls_line(&comps);

        for grid in [false, true] {
            let p = Params { pitch_grid_check: grid, ..Params::default() };
            let rules = band_space_rules(std::slice::from_ref(&line), &comps, &p);
            assert_eq!(rules[0].source, ThresholdSource::FixedPitch, "grid={grid}: {rules:?}");
            let words = split_with(&line, &comps, &p);
            assert_eq!(words.len(), 2, "grid={grid}: must split at the empty cell: {words:?}");
            assert_eq!(words[0].members, vec![0, 1, 2]);
            assert_eq!(words[1].members, vec![3, 4, 5]);
        }
    }

    /// The gate on the rule firing where it should not: a line whose
    /// centre-to-centre distances do not cluster on one pitch -- ordinary
    /// proportional-face kerning -- must not be classed fixed-pitch, and
    /// falls through to the existing gap-valley rule unchanged.
    #[test]
    fn a_proportional_line_is_not_classed_fixed_pitch() {
        // Centre-to-centre distances 8, 13, 8, 21, 8: three near one pitch,
        // two nowhere near an integer multiple of it, so agreement (3 of 5)
        // stays below the default 0.8.
        let lefts: [u32; 6] = [0, 8, 21, 29, 50, 58];
        let (comps, line) = line_of(&lefts);
        let p = Params::default();
        let rules = band_space_rules(std::slice::from_ref(&line), &comps, &p);
        assert_ne!(rules[0].source, ThresholdSource::FixedPitch, "{rules:?}");
    }

    /// `ACCOUNT STATEMENT`: an all-caps run in a proportional face with
    /// near-uniform advances -- two four-letter words at centre-to-centre
    /// distances of 10 within each and 13 between them, the shape
    /// `docs/measurements/2026-09-22_fixed_pitch_spaces.txt` diagnosed at
    /// 10 px. Two amendments in `ARCHITECTURE.md` section 11 tried to reject
    /// this shape from the fixed-pitch classifier -- an all-or-nothing grid
    /// consistency check, then a pairwise grid vote -- and both were
    /// measured to fail their own gates and were removed rather than kept as
    /// toggled dead code. What ships (`pitch_grid_check = false`, the
    /// section's bounded fallback) is the pre-amendment agreement test
    /// alone, and agreement alone cannot reject this shape: 6 of its 7
    /// distances land within tolerance of one cell, comfortably above the
    /// default 0.8 floor, so the fragment classifies fixed-pitch and its one
    /// real gap (13, short of the 1.5-cell floor at 15) never separates the
    /// two words. This is the "statement title regression... accepted and
    /// recorded as a known loss" the section's own bounded-fallback text
    /// names -- this test pins that acceptance down as a fixture so a
    /// future change that silently starts rejecting the shape again (or
    /// silently degrades further) is visible in a diff, not just a doc.
    #[test]
    fn an_account_statement_title_is_a_known_accepted_fixed_pitch_regression() {
        let lefts: [u32; 8] = [0, 10, 20, 30, 43, 53, 63, 73];
        let (comps, line) = line_of(&lefts);
        let p = Params::default();
        assert!(!p.pitch_grid_check, "the shipped default is the bounded fallback");

        let rules = band_space_rules(std::slice::from_ref(&line), &comps, &p);
        assert_eq!(rules[0].source, ThresholdSource::FixedPitch, "{rules:?}");
        let words = split(&line, &comps);
        assert_eq!(words.len(), 1, "known loss: the two words fuse under the fallback: {words:?}");
    }

    /// The fitted-grid mechanism (`ARCHITECTURE.md` section 11's third
    /// amendment, `pitch_grid_check = true`) does not rescue the case above
    /// either -- it is why that amendment failed its own gate rather than
    /// shipping. A single 0.3-cell step (the 13 gap against a 10 cell) is a
    /// small residual against a straight-line fit through 17 positions
    /// (`docs/measurements/2026-09-22_fixed_pitch_spaces.txt`, "Fitted
    /// grid"): least squares spreads that error across every position
    /// rather than displacing the ones after the gap, so agreement with the
    /// fitted line stays far above the 0.8 floor and the two words fuse
    /// exactly as under the fallback. Two four-letter words (distances 10)
    /// plus one wider gap (13) repeated to 17 glyphs, per the required test
    /// shape.
    #[test]
    fn a_wide_account_statement_title_is_not_rejected_by_the_fitted_grid_either() {
        let mut lefts = vec![0u32];
        for k in 1..17u32 {
            let step = if k == 4 || k == 9 || k == 13 { 13 } else { 10 };
            lefts.push(lefts[k as usize - 1] + step);
        }
        let (comps, line) = line_of(&lefts);
        let mut p = Params::default();
        p.pitch_grid_check = true;

        let rules = band_space_rules(std::slice::from_ref(&line), &comps, &p);
        assert_eq!(
            rules[0].source,
            ThresholdSource::FixedPitch,
            "the fitted grid also fails to reject this shape: {rules:?}"
        );
    }

    /// `ARCHITECTURE.md` section 11's amendment, item 1 (cell merge), for the
    /// defect it was written to fix: a component is not always a character
    /// cell. Reruns `a_monospace_fragment_splits_at_its_empty_cell`'s `QTY
    /// 123` fixture with `Y` rendered as two vertically stacked, fully
    /// x-overlapping components -- a colon's two dots or a glyph the
    /// binarizer split in two produce exactly this: one visual cell, two
    /// connected components, centre-to-centre distance `0.00` between them.
    ///
    /// Unmerged, that pair would still be six members feeding
    /// `pitch_min_glyphs`, so the fragment tests fixed-pitch regardless of
    /// this fix; what the fix guards is that the pitch estimate, agreement
    /// and split point are unperturbed by treating `Y`'s two halves as one
    /// position rather than two -- the fragment still lands on cell count
    /// exactly 6 (`Params::pitch_min_glyphs`'s default floor) after merging,
    /// and the empty cell between `Y` and `1` is still where it splits.
    #[test]
    fn a_monospace_fragment_with_a_stacked_component_stays_correctly_segmented() {
        let q = monospace_cell(1, 0, 12, 0, 8);
        let t = monospace_cell(2, 1, 12, 0, 8);
        let y_top = Component { y1: 25, ..monospace_cell(3, 2, 12, 0, 8) };
        let y_bottom = Component { y0: 25, label: 4, ..monospace_cell(4, 2, 12, 0, 8) };
        // `y_top`/`y_bottom` share `Y`'s cell exactly (same x0/x1), so their
        // centre-to-centre distance is 0.00 -- the pair the diagnosis found
        // inside a real monospace run.
        assert_eq!(y_top.x0, y_bottom.x0);
        assert_eq!(y_top.x1, y_bottom.x1);
        let d1 = monospace_cell(5, 4, 12, 0, 8); // cell 3 (between Y and 1) is empty
        let d2 = monospace_cell(6, 5, 12, 0, 8);
        let d3 = monospace_cell(7, 6, 12, 0, 8);
        let comps = vec![q, t, y_top, y_bottom, d1, d2, d3];
        let line = ls_line(&comps);
        assert_eq!(line.members.len(), 7);

        let p = Params::default();
        assert_eq!(
            cells(&line, &comps, true).len(),
            6,
            "Y's two halves must merge into one cell"
        );
        let rules = band_space_rules(std::slice::from_ref(&line), &comps, &p);
        assert_eq!(rules[0].source, ThresholdSource::FixedPitch, "{rules:?}");
        let words = split(&line, &comps);
        assert_eq!(words.len(), 2, "must still split at the empty cell: {words:?}");
        assert_eq!(words[0].members, vec![0, 1, 2, 3]);
        assert_eq!(words[1].members, vec![4, 5, 6]);
    }

    /// `ARCHITECTURE.md` section 11's second amendment, "the grid check was
    /// all-or-nothing", items 2 and 4: a *touching run*, one raw component
    /// about `2p` wide -- two neighbouring monospace glyphs' anti-aliased
    /// edges already touching before `cells()` ever ran, the H2 mechanism
    /// `docs/measurements/2026-09-22_fixed_pitch_spaces.txt`'s "Amendment
    /// ablation" task 2 found causing 8 of 10 rejected fragments. Six normal
    /// cells flank one such run (positions 3 and 4 fused into one 24 px
    /// component on a 12 px pitch); the run's own box centre sits exactly
    /// half a pitch off either neighbour's true position, so the *raw*
    /// box-centre distance either side of it is `18.00` -- exactly the
    /// `1.5 * pitch` empty-cell threshold, which would manufacture a
    /// spurious space on *both* sides of a pair that is not a space at all.
    #[test]
    fn a_monospace_fragment_with_one_touching_pair_stays_fixed_pitch_with_no_spurious_space() {
        let before: Vec<Component> =
            (0u32..6).map(|k| monospace_cell(k + 1, k, 12, 0, 8)).collect();
        let touch = Component {
            label: 7,
            x0: 72,
            y0: 20,
            x1: 96,
            y1: 30,
            area: 24 * 10,
            border_coverage: [1.0; 4],
        };
        let after: Vec<Component> =
            (8u32..14).map(|k| monospace_cell(k, k, 12, 0, 8)).collect();

        // The fixture's own precondition: the raw box-centre distance either
        // side of the touching run sits exactly at the empty-cell threshold,
        // so this test exercises the item-4 correction and not some other
        // reason the pair happens not to split.
        let left_of = f64::from(before.last().unwrap().x1 + before.last().unwrap().x0) / 2.0;
        let touch_centre = f64::from(touch.x0 + touch.x1) / 2.0;
        let right_of = f64::from(after[0].x0 + after[0].x1) / 2.0;
        assert_eq!(touch_centre - left_of, 18.0);
        assert_eq!(right_of - touch_centre, 18.0);

        let mut comps = before;
        comps.push(touch);
        comps.extend(after);
        let line = ls_line(&comps);
        assert_eq!(line.members.len(), 13);

        for grid in [false, true] {
            let p = Params { pitch_grid_check: grid, ..Params::default() };
            let rules = band_space_rules(std::slice::from_ref(&line), &comps, &p);
            assert_eq!(rules[0].source, ThresholdSource::FixedPitch, "grid={grid}: {rules:?}");
            let words = split_with(&line, &comps, &p);
            assert_eq!(
                words.len(),
                1,
                "grid={grid}: a touching pair beside on-grid glyphs must not manufacture a space: {words:?}"
            );
        }

        // Control: without the item-4 correction (raw box-centre distances
        // throughout), the same fragment's `18.00` either side of the run
        // clears the `>= 1.5 * pitch` threshold and splits into three.
        let cells = cells(&line, &comps, true);
        let raw = cell_distances(&cells);
        assert!(raw.iter().any(|&d| d >= 18.0), "the control must exercise the bug the fix guards against: {raw:?}");
    }

    /// `ARCHITECTURE.md` section 11's second amendment, item 3 (the grid
    /// vote), for the case the all-or-nothing test cost a third of monospace
    /// recall over: one off-centre glyph -- here, the third word's second
    /// letter translated 4 px off the pitch grid, the H1 mechanism the same
    /// ablation traced -- among several genuine, on-grid word gaps. The one
    /// bad distance it produces is not next to a touching run, so it is not
    /// excused; the vote must still pass it by majority, the way the
    /// pre-amendment per-distance agreement check already tolerates a single
    /// exception, rather than reject the whole line as the unconditional
    /// test did.
    #[test]
    fn a_mono_fragment_with_one_off_centre_glyph_among_several_word_gaps_stays_fixed_pitch() {
        // Six three-letter words on a 12 px pitch, cell slots skipping one
        // per word boundary for the real (two-pitch) word gap.
        let slots: [u32; 18] = [0, 1, 2, 4, 5, 6, 8, 9, 10, 12, 13, 14, 16, 17, 18, 20, 21, 22];
        let mut comps: Vec<Component> =
            slots.iter().enumerate().map(|(k, &slot)| monospace_cell(k as u32 + 1, slot, 12, 0, 8)).collect();
        // The third word's second letter (label 8, slot 9): translated 4 px
        // right of where `monospace_cell` would centre it, same width --
        // one glyph whose ink sits off the pitch grid, not a touching run.
        let off = comps.iter().position(|c| c.label == 8).expect("the fixture's ninth letter");
        comps[off].x0 += 4;
        comps[off].x1 += 4;
        let line = ls_line(&comps);

        for grid in [false, true] {
            let p = Params { pitch_grid_check: grid, ..Params::default() };
            let rules = band_space_rules(std::slice::from_ref(&line), &comps, &p);
            assert_eq!(rules[0].source, ThresholdSource::FixedPitch, "grid={grid}: {rules:?}");

            let words = split_with(&line, &comps, &p);
            assert_eq!(words.len(), 6, "grid={grid}: must still split at all five real word gaps: {words:?}");
        }
    }
}

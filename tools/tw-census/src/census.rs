//! # `census` — the classification logic, isolated from the I/O
//!
//! Everything in this module is a **pure function of an already-extracted
//! [`PageText`]**. Nothing here reads a file, spawns a thread, or prints.
//! That separation is the whole reason the module exists: the number this
//! tool produces gates a product decision (decision 019 §3.3), so the rule
//! that decides "does `Tw` reach this text?" has to be readable and
//! unit-testable on its own, without a 4,000-file corpus in the loop.
//!
//! ## The question being answered
//!
//! ISO 32000-1 §9.3.3 (`PDF_Spec` `iso32000__s__9.3.md`):
//!
//! > Word spacing shall be applied to every occurrence of the single-byte
//! > character code 32 in a string when using a simple font or a composite
//! > font that defines code 32 as a single-byte code. It shall not apply to
//! > occurrences of the byte value 32 in multiple-byte codes.
//!
//! Two things follow, and both are load-bearing for how this module counts:
//!
//! 1. **The predicate is about CODE WIDTH, not about font type.** A Type 0
//!    composite font whose CMap defines one-byte codespace ranges *is*
//!    reachable by `Tw`. pdfcer's published
//!    [`GlyphProvenance::composite`](pdfcer_core::text_extract::GlyphProvenance::composite)
//!    flag is derived from `ExtractFont::is_simple()`, which is exactly a
//!    code-width test (`CodeWidth::One` vs `CodeWidth::Two`) rather than a
//!    `/Subtype` test. So `!composite` is precisely the §9.3.3 predicate,
//!    not an approximation of it. This is why the census can be taken from
//!    the published flag at all.
//! 2. **A simple-font run with no code 32 in it is a run `Tw` cannot affect
//!    either.** Word spacing is applied *per occurrence of code 32*; a run
//!    with none has nothing for the operator's dial to move. Counting such
//!    a run as "the control works here" would inflate the headline. So this
//!    module reports the simple-font share and the *spaced* simple-font
//!    share as two separate numbers and never conflates them.
//!
//! ## The unit of measurement: one show operator
//!
//! Decision 019 §3.3 defines reachability (a) as "of all show operators in
//! the corpus, the fraction whose font is simple". So the run unit here is
//! the **show operator** (`Tj` / `'` / `"` / `TJ`), identified by
//! [`RunKey`] = (which decoded content buffer, byte span of the operator
//! within it). Every glyph a `TJ` array produces shares one span (§9.4.3),
//! which is what makes this identity work.
//!
//! Two consequences worth stating because they change what the number
//! means:
//!
//! - A `TextRun` in pdfcer's extraction output is *not* the same unit. Runs
//!   are split on marked-content and geometry boundaries, so one show
//!   operator can span several runs and one run can span several operators.
//!   Grouping by [`RunKey`] undoes both.
//! - Keys are pooled **per page**, not per document. A form XObject invoked
//!   twice on one page yields the same key twice and is counted once (its
//!   glyphs accumulate); the same form invoked on two pages is counted
//!   twice. This is the "as executed, per page" reading — the alternative
//!   ("as authored, once per file") would under-weight a form that carries
//!   a document's entire body text. Stated rather than silently chosen.
//!
//! ## Determinism
//!
//! [`PageCensus`] pools runs in a `HashMap`, whose iteration order is
//! non-deterministic. That is safe **only** because every value derived
//! from the pool is an exact integer sum, and integer addition is
//! commutative and associative — the tool never takes a first element,
//! samples, or picks a representative. (A prior harness in this repo was
//! bitten by exactly the opposite mistake, sampling from
//! `Document::objects()`'s `HashMap` order.) Within a single key, the
//! composite flag is taken from the first glyph *in run-and-glyph order*,
//! which is `Vec` order and therefore deterministic.
//!
//! ## What is deliberately NOT counted
//!
//! - **Derived whitespace.** pdfcer's extraction inserts synthetic spaces
//!   between glyphs that are far apart (`ExtractOptions::word_gap_ratio`).
//!   Those live in runs whose [`TextOrigin`] is not `Glyphs` and therefore
//!   carry no [`ExtractedGlyph`]s at all, so they cannot reach this
//!   module's counters. Good: a derived space is pdfcer's inference, not a
//!   code 32 in the file, and `Tw` would not touch it.
//! - **`/ActualText` runs.** Same mechanism, same reason (§9.10.3).
//! - **`TJ` numeric adjustments.** They are not glyphs and are not shown
//!   characters; they are the *other* inter-word mechanism, the one
//!   decision 015 §3.1 already chose over `Tw`.

use std::collections::HashMap;
use std::sync::Arc;

use pdfcer_core::span::ByteSpan;
use pdfcer_core::text_extract::{ContentStreamRef, PageText};
use pdfcer_core::text_state::{AmbientOrigin, AmbientTextState};

/// The single-byte character code word spacing applies to (§9.3.3).
const SPACE_CODE: u32 = 32;

/// Identity of one show operator within one page's extraction.
///
/// The pair is unique per operator because a span is a byte range inside a
/// named decoded buffer: two distinct operators in the same buffer occupy
/// disjoint ranges, and identical ranges in *different* buffers are
/// separated by the [`ContentStreamRef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RunKey {
    /// Which decoded content buffer the span indexes — the page's own
    /// concatenated `/Contents`, or a named form XObject's stream (§8.10.1).
    pub stream: ContentStreamRef,
    /// Byte span of the show operator within that buffer.
    pub span: ByteSpan,
}

/// What one show operator contributes to the census.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunRecord {
    /// `true` when the governing font segments show strings into
    /// multi-byte codes (§9.7.6.2) — i.e. `Tw` is structurally void here.
    pub composite: bool,
    /// Shown character codes produced by this operator.
    pub glyphs: u64,
    /// Of those, how many were shown in text rendering mode 3 or 7 and
    /// therefore paint nothing (the OCR "sandwich" convention).
    pub invisible: u64,
    /// Occurrences of character code 32 among this operator's codes.
    ///
    /// For a simple-font run this is literally the count of `Tw`-affected
    /// positions. For a composite run it is the count of the *two-byte*
    /// code `0x0020`, which §9.3.3 explicitly exempts — recorded only so
    /// the exemption can be shown to be non-trivial rather than assumed.
    pub space_codes: u64,
    /// Set when glyphs sharing this key disagreed about the composite
    /// flag. Should be impossible (one `Tf` governs one show operator), so
    /// a non-zero total is a signal that either the corpus or pdfcer's
    /// provenance is stranger than assumed, and the report says so.
    pub font_conflict: bool,
}

impl RunRecord {
    /// How `Tw` relates to this run. See [`RunClass`].
    #[must_use]
    pub const fn class(&self) -> RunClass {
        if self.composite {
            RunClass::Composite
        } else if self.space_codes > 0 {
            RunClass::SimpleSpaced
        } else {
            RunClass::SimpleUnspaced
        }
    }
}

/// The three ways a show operator can relate to word spacing.
///
/// The ordering of the three is the ordering of the argument: `Tw` is
/// *structurally* impossible on [`Composite`](RunClass::Composite),
/// *possible but inert* on [`SimpleUnspaced`](RunClass::SimpleUnspaced),
/// and *actually effective* only on
/// [`SimpleSpaced`](RunClass::SimpleSpaced).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunClass {
    /// Simple (one-byte-code) font, and at least one code 32 present — the
    /// only class a `Tw` control would visibly change.
    SimpleSpaced,
    /// Simple font, but no code 32 in the string. `Tw` would be legal and
    /// would do nothing.
    SimpleUnspaced,
    /// Multi-byte codes. §9.3.3 makes `Tw` void; pdfcer would refuse to
    /// emit it (R91).
    Composite,
}

/// Whether a document's text is uniformly one font model or a mixture.
///
/// Called out separately because it is a *product* signal rather than a
/// share: an operator editing a `Mixed` document finds the control present
/// on some selections and absent on others, which decision 019 §3.3
/// identifies as worse than a clean yes or no.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontMix {
    /// No show operators at all — a scan, a blank page set, or a file whose
    /// text pdfcer could not reach.
    NoText,
    /// Every show operator is simple.
    AllSimple,
    /// Every show operator is composite.
    AllComposite,
    /// Both kinds present.
    Mixed,
}

impl FontMix {
    /// The TSV / summary spelling.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NoText => "no-text",
            Self::AllSimple => "all-simple",
            Self::AllComposite => "all-composite",
            Self::Mixed => "mixed",
        }
    }
}

/// Whether one text-state parameter was ever touched, and whether it was
/// ever seen holding a non-default value.
///
/// This is the second number decision 019 §3.3 asked for — "(b)
/// prevalence" — and it sizes the *preservation* risk independently of the
/// authoring question. It is worth having whatever (a) says: pdfcer must
/// restore an ambient `Tw`/`Ts`/`Tc`/`Tz` correctly around any edit
/// regardless of whether it ever lets an operator author one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParamFlags {
    /// An operator in the stream set this parameter (its
    /// [`AmbientOrigin`] is anything but `Initial`), even if it set it to
    /// the default value.
    pub observed: bool,
    /// The parameter was in force at some shown glyph holding a value
    /// other than its Table 105 initial value.
    pub nondefault: bool,
}

// NOTE: there is deliberately no `merge` for `ParamFlags`/`Prevalence`.
// Prevalence is a **per-document** predicate ("did THIS document ever set
// `Tw`?"), and the corpus-level number decision 019 §3.3 asked for is the
// *count of documents* for which it holds — not a corpus-wide OR, which
// would saturate to `true` after the first file and mean nothing. The
// document→corpus fold therefore happens in the aggregator as a counter
// increment, and offering a merge here would only invite that mistake.

/// Per-parameter prevalence flags for one document.
///
/// Measured **at shown glyphs only**, which is a real limitation and is
/// stated in the report: a `Tw` set by a content stream and then never
/// used before the page ends is invisible here. That is the right bias for
/// this question — an unused setting affects no rendering and no edit — but
/// it means these numbers are a floor, not a total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Prevalence {
    /// `Tc`, character spacing (§9.3.2). Table 105 default `0`.
    pub char_spacing: ParamFlags,
    /// `Tw`, word spacing (§9.3.3). Table 105 default `0`.
    pub word_spacing: ParamFlags,
    /// `Tz`, horizontal scaling as the percentage operand (§9.3.4).
    /// Table 105 default `100`.
    pub h_scale: ParamFlags,
    /// `TL`, leading (§9.3.5). Table 105 default `0`.
    pub leading: ParamFlags,
    /// `Ts`, text rise (§9.3.7). Table 105 default `0`.
    pub rise: ParamFlags,
    /// `Tr`, text rendering mode (§9.3.6). Table 105 default `0`.
    pub render_mode: ParamFlags,
}

/// Tolerance for "is this operand at its Table 105 default?".
///
/// Producers write `0.00000` and `100.000` routinely, and a bit-exact
/// comparison would score those as non-default and inflate prevalence. The
/// window is far tighter than any spacing a human could see at any
/// plausible font size, so it cannot hide a real setting.
const DEFAULT_EPSILON: f64 = 1e-9;

impl Prevalence {
    /// Fold one observed ambient state into the flags.
    pub fn observe(&mut self, st: &AmbientTextState) {
        let pairs: [(&mut ParamFlags, f64, f64); 6] = [
            (&mut self.char_spacing, st.char_spacing.value, 0.0),
            (&mut self.word_spacing, st.word_spacing.value, 0.0),
            (&mut self.h_scale, st.h_scale.value, 100.0),
            (&mut self.leading, st.leading.value, 0.0),
            (&mut self.rise, st.rise.value, 0.0),
            (&mut self.render_mode, st.render_mode.value, 0.0),
        ];
        let origins = [
            &st.char_spacing.origin,
            &st.word_spacing.origin,
            &st.h_scale.origin,
            &st.leading.origin,
            &st.rise.origin,
            &st.render_mode.origin,
        ];
        for ((flags, value, default), origin) in pairs.into_iter().zip(origins) {
            if !matches!(origin, AmbientOrigin::Initial) {
                flags.observed = true;
            }
            if (value - default).abs() > DEFAULT_EPSILON {
                flags.nondefault = true;
            }
        }
    }
}

/// Additive counters over show operators and shown character codes.
///
/// Every field is a plain sum so that document totals, sub-corpus totals
/// and the grand total are the same type combined the same way — the
/// aggregate can never disagree with the per-file TSV because it is
/// literally the sum of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    /// Show operators seen.
    pub runs: u64,
    /// Of those, governed by a simple (one-byte-code) font.
    pub runs_simple: u64,
    /// Of those simple ones, containing at least one code 32.
    pub runs_simple_spaced: u64,
    /// Shown character codes seen.
    pub glyphs: u64,
    /// Of those, in a simple-font run.
    pub glyphs_simple: u64,
    /// Of those, in a simple-font run that contains at least one code 32.
    pub glyphs_simple_spaced: u64,
    /// Of all glyphs, how many were invisible (render mode 3 or 7).
    pub glyphs_invisible: u64,
    /// Occurrences of code 32 inside simple-font runs — the count of
    /// positions a `Tw` operand would actually move.
    pub space_codes_simple: u64,
    /// Occurrences of the two-byte code `0x0020` inside composite runs,
    /// which §9.3.3 exempts.
    pub space_codes_composite: u64,
    /// Show operators whose glyphs disagreed about the composite flag.
    pub font_conflicts: u64,
}

impl Totals {
    /// Fold one show operator's record in.
    pub fn add_run(&mut self, r: &RunRecord) {
        self.runs += 1;
        self.glyphs += r.glyphs;
        self.glyphs_invisible += r.invisible;
        if r.font_conflict {
            self.font_conflicts += 1;
        }
        if r.composite {
            self.space_codes_composite += r.space_codes;
        } else {
            self.runs_simple += 1;
            self.glyphs_simple += r.glyphs;
            self.space_codes_simple += r.space_codes;
            if r.space_codes > 0 {
                self.runs_simple_spaced += 1;
                self.glyphs_simple_spaced += r.glyphs;
            }
        }
    }

    /// Sum two totals. Used to roll documents into sub-corpora and
    /// sub-corpora into the grand total.
    pub fn merge(&mut self, other: Self) {
        self.runs += other.runs;
        self.runs_simple += other.runs_simple;
        self.runs_simple_spaced += other.runs_simple_spaced;
        self.glyphs += other.glyphs;
        self.glyphs_simple += other.glyphs_simple;
        self.glyphs_simple_spaced += other.glyphs_simple_spaced;
        self.glyphs_invisible += other.glyphs_invisible;
        self.space_codes_simple += other.space_codes_simple;
        self.space_codes_composite += other.space_codes_composite;
        self.font_conflicts += other.font_conflicts;
    }

    /// The document-level font mix these totals imply.
    #[must_use]
    pub const fn mix(&self) -> FontMix {
        if self.runs == 0 {
            FontMix::NoText
        } else if self.runs_simple == self.runs {
            FontMix::AllSimple
        } else if self.runs_simple == 0 {
            FontMix::AllComposite
        } else {
            FontMix::Mixed
        }
    }
}

/// One page's run pool, before it is folded into a document total.
///
/// Exists as its own type so the per-page pooling rule (see the module
/// docs) is a visible, testable step rather than an implementation detail
/// buried in a loop.
#[derive(Debug, Default)]
pub struct PageCensus {
    runs: HashMap<RunKey, RunRecord>,
    prevalence: Prevalence,
    /// Glyphs seen with no provenance attached. Should be zero when
    /// `capture_provenance` is on; a non-zero total means some text is
    /// invisible to this census and the report must say how much.
    unprovenanced_glyphs: u64,
}

impl PageCensus {
    /// Pool every provenanced glyph of one extracted page by its show
    /// operator, and fold the ambient text state into the prevalence flags.
    ///
    /// Ambient states are deduplicated by `Arc` pointer: pdfcer publishes
    /// one `Arc<AmbientTextState>` per run and clones the handle onto every
    /// glyph, so a pointer comparison skips the redundant work for all but
    /// the first glyph of each run without changing the result.
    pub fn add_page(&mut self, page: &PageText) {
        let mut last_state: Option<*const AmbientTextState> = None;
        for run in &page.runs {
            for glyph in &run.glyphs {
                let Some(prov) = glyph.provenance.as_ref() else {
                    self.unprovenanced_glyphs += 1;
                    continue;
                };
                let key = RunKey {
                    stream: prov.content_stream,
                    span: prov.operator_span,
                };
                let entry = self.runs.entry(key).or_insert(RunRecord {
                    composite: prov.composite,
                    ..RunRecord::default()
                });
                if entry.glyphs > 0 && entry.composite != prov.composite {
                    entry.font_conflict = true;
                }
                entry.glyphs += 1;
                if glyph.invisible {
                    entry.invisible += 1;
                }
                if glyph.code == SPACE_CODE {
                    entry.space_codes += 1;
                }

                let ptr = Arc::as_ptr(&prov.text_state);
                if last_state != Some(ptr) {
                    self.prevalence.observe(&prov.text_state);
                    last_state = Some(ptr);
                }
            }
        }
    }

    /// Collapse the pooled pages into additive counters plus prevalence.
    #[must_use]
    pub fn finish(&self) -> (Totals, Prevalence, u64) {
        let mut totals = Totals::default();
        for record in self.runs.values() {
            totals.add_run(record);
        }
        (totals, self.prevalence, self.unprovenanced_glyphs)
    }
}

/// Everything the census knows about one document.
#[derive(Debug, Clone, PartialEq)]
pub struct DocCensus {
    /// Show-operator and glyph counters.
    pub totals: Totals,
    /// Which text-state parameters were set / held non-default values.
    pub prevalence: Prevalence,
    /// Pages walked successfully.
    pub pages: u64,
    /// Pages whose extraction returned an error and were skipped. Counted
    /// separately because a partially-extracted document's shares are
    /// computed over what was reachable, and the reader deserves to know
    /// that.
    pub page_errors: u64,
    /// Glyphs that arrived without provenance (see
    /// [`PageCensus::unprovenanced_glyphs`]).
    pub unprovenanced_glyphs: u64,
}

/// Per-document outcome. Load failures, text-free documents and measured
/// documents are three different things and are never merged: decision
/// 019's bands are about *text*, and silently scoring a scan or an
/// unopenable file as "no simple text" would understate `Tw` badly.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The file could not be read from disk.
    ReadFailed(String),
    /// `Document::from_bytes` refused it. This is not evidence about `Tw`.
    LoadFailed(String),
    /// The page tree could not be walked, so no page was reachable.
    PageTreeFailed(String),
    /// pdfcer-core panicked on this file.
    Panicked(String),
    /// The per-file wall-clock budget was exceeded.
    TimedOut,
    /// The document loaded and was walked. `totals.runs == 0` means it is
    /// text-free (a scan, or a file whose text pdfcer could not reach) — a
    /// third category that belongs in neither numerator nor denominator of
    /// the reachability share.
    Measured(DocCensus),
}

impl Outcome {
    /// The TSV / summary spelling of the outcome's category.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ReadFailed(_) => "read-failed",
            Self::LoadFailed(_) => "load-failed",
            Self::PageTreeFailed(_) => "pagetree-failed",
            Self::Panicked(_) => "panic",
            Self::TimedOut => "timeout",
            Self::Measured(c) if c.totals.runs == 0 => "no-text",
            Self::Measured(_) => "measured",
        }
    }

    /// The free-text detail column, empty for a clean measurement.
    #[must_use]
    pub fn detail(&self) -> &str {
        match self {
            Self::ReadFailed(m)
            | Self::LoadFailed(m)
            | Self::PageTreeFailed(m)
            | Self::Panicked(m) => m,
            Self::TimedOut | Self::Measured(_) => "",
        }
    }
}

/// A percentage, or `None` when the denominator is zero.
///
/// Returned rather than a `0.0` sentinel because "0% of nothing" and "0%
/// of a million" are different claims and the summary must not print them
/// the same way.
#[must_use]
pub fn share(numerator: u64, denominator: u64) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        // f64 has 53 bits of mantissa; corpus counts here are far below
        // 2^53, so the conversion is exact.
        Some(numerator as f64 * 100.0 / denominator as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(composite: bool, glyphs: u64, spaces: u64) -> RunRecord {
        RunRecord {
            composite,
            glyphs,
            invisible: 0,
            space_codes: spaces,
            font_conflict: false,
        }
    }

    #[test]
    fn composite_run_is_never_tw_reachable_even_with_code_32() {
        // §9.3.3: the byte value 32 inside a MULTI-byte code is exempt.
        // The census must classify it as Composite regardless of how many
        // 0x0020 codes the run contains, or the headline is a lie.
        assert_eq!(rec(true, 10, 4).class(), RunClass::Composite);
        assert_eq!(rec(true, 10, 0).class(), RunClass::Composite);
    }

    #[test]
    fn simple_run_without_code_32_is_reachable_but_inert() {
        // A control that is legal but changes nothing must not be counted
        // as working: the two states are distinct classes on purpose.
        assert_eq!(rec(false, 10, 0).class(), RunClass::SimpleUnspaced);
        assert_eq!(rec(false, 10, 1).class(), RunClass::SimpleSpaced);
    }

    #[test]
    fn totals_split_simple_from_simple_spaced() {
        let mut t = Totals::default();
        t.add_run(&rec(false, 10, 2)); // simple, spaced
        t.add_run(&rec(false, 5, 0)); // simple, unspaced
        t.add_run(&rec(true, 20, 3)); // composite
        assert_eq!(t.runs, 3);
        assert_eq!(t.runs_simple, 2);
        assert_eq!(t.runs_simple_spaced, 1);
        assert_eq!(t.glyphs, 35);
        assert_eq!(t.glyphs_simple, 15);
        // Only the SPACED simple run's glyphs count toward the strict
        // measure — the 5 unspaced ones are simple but unreachable.
        assert_eq!(t.glyphs_simple_spaced, 10);
        assert_eq!(t.space_codes_simple, 2);
        assert_eq!(t.space_codes_composite, 3);
    }

    #[test]
    fn mix_distinguishes_the_four_document_shapes() {
        assert_eq!(Totals::default().mix(), FontMix::NoText);

        let mut simple = Totals::default();
        simple.add_run(&rec(false, 1, 1));
        assert_eq!(simple.mix(), FontMix::AllSimple);

        let mut composite = Totals::default();
        composite.add_run(&rec(true, 1, 0));
        assert_eq!(composite.mix(), FontMix::AllComposite);

        let mut mixed = Totals::default();
        mixed.add_run(&rec(false, 1, 1));
        mixed.add_run(&rec(true, 1, 0));
        assert_eq!(mixed.mix(), FontMix::Mixed);
    }

    #[test]
    fn merge_is_the_sum_of_the_parts() {
        // The aggregate must be re-derivable from the per-file TSV; that
        // is only true if merging is plain addition of every field.
        let mut a = Totals::default();
        a.add_run(&rec(false, 3, 1));
        let mut b = Totals::default();
        b.add_run(&rec(true, 7, 0));
        let mut both = Totals::default();
        both.add_run(&rec(false, 3, 1));
        both.add_run(&rec(true, 7, 0));

        a.merge(b);
        assert_eq!(a, both);
    }

    #[test]
    fn share_reports_an_empty_denominator_rather_than_zero_percent() {
        assert_eq!(share(0, 0), None);
        assert_eq!(share(0, 10), Some(0.0));
        assert_eq!(share(1, 4), Some(25.0));
    }

    #[test]
    fn prevalence_treats_tz_100_as_the_default() {
        // Table 105's Tz default is the operand 100, not 0. Getting this
        // backwards would report every text-bearing document as carrying a
        // non-default horizontal scale.
        let mut p = Prevalence::default();
        p.observe(&AmbientTextState::initial());
        assert!(!p.h_scale.nondefault);
        assert!(!p.h_scale.observed);

        let mut st = AmbientTextState::initial();
        st.set(
            pdfcer_core::text_state::TextStateParam::HorizScale,
            90.0,
            b"90 Tz",
        );
        p.observe(&st);
        assert!(p.h_scale.observed);
        assert!(p.h_scale.nondefault);
    }

    #[test]
    fn an_operator_that_sets_the_default_counts_as_observed_not_nondefault() {
        // `0 Tw` is a real operator that a restore must reproduce, but it
        // is not evidence anyone wanted word spacing. The two flags exist
        // to keep those apart.
        let mut st = AmbientTextState::initial();
        st.set(
            pdfcer_core::text_state::TextStateParam::WordSpacing,
            0.0,
            b"0 Tw",
        );
        let mut p = Prevalence::default();
        p.observe(&st);
        assert!(p.word_spacing.observed);
        assert!(!p.word_spacing.nondefault);
    }

    #[test]
    fn outcome_labels_separate_no_text_from_load_failure() {
        // The whole point of the outcome taxonomy: a scan is not a failure
        // and a failure is not evidence about Tw.
        let empty = Outcome::Measured(DocCensus {
            totals: Totals::default(),
            prevalence: Prevalence::default(),
            pages: 3,
            page_errors: 0,
            unprovenanced_glyphs: 0,
        });
        assert_eq!(empty.label(), "no-text");
        assert_eq!(
            Outcome::LoadFailed("bad xref".into()).label(),
            "load-failed"
        );
        assert_eq!(Outcome::TimedOut.label(), "timeout");
    }
}

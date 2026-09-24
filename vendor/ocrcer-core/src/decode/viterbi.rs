//! Beam-Viterbi decode over the segmentation lattice.
//!
//! # What this stage decides
//!
//! The matcher answers "what does this image look like?" for one candidate
//! cut. This stage answers the two questions the matcher structurally cannot:
//! *where the cuts go*, and *which of several look-alike classes the
//! neighbours argue for*. Both are answered by scoring whole paths through the
//! lattice rather than each edge alone.
//!
//! # The score
//!
//! A path's score is the sum over its edges of
//!
//! ```text
//!   w_match     * (bonus - distance)  (how well the image matched)
//! + w_bigram    * bigram_logp      (log2 P(class | previous class))
//! + w_confusion * confusion_adjust (context priors for look-alike pairs)
//! + w_seg       * segmentation_prior
//! - case_shape_penalty             (once per anomalous case transition)
//! ```
//!
//! where `bonus` is `char_bonus_slanted` for a word the layout stage's slant
//! estimator measured as slanted, and `char_bonus` otherwise (`decode_word`'s
//! `slanted` argument, `ARCHITECTURE.md` section 11, 2026-09-24: "char_bonus
//! re-sweep ... next a slanted-word bonus"). Setting the two equal makes a
//! build behave identically to one with no such parameter at all, which is
//! how it shipped until measured; `docs/measurements/2026-09-24_char_bonus_slanted.txt`
//! now sets `char_bonus_slanted` to a different, measured value.
//!
//! plus, once at the end of the word, `w_lex * lex_bonus[tier]` when the whole
//! path spells a lexicon word. **The lexicon term is added and never
//! subtracted** (`CLAUDE.md` rule 6): a word the graph does not hold simply
//! collects nothing, which is why `M8x1.25` survives this stage intact. Inside
//! an identifier-shaped word the term is suppressed entirely, so the graph
//! cannot even offer a bonus for a coincidental match.
//!
//! The case-shape term is the one piece of orthography a character bigram
//! cannot express, because it is a statement about a whole run of letters
//! rather than about a pair. Printed alphabetic words take three shapes —
//! `lower`, `Title`, `UPPER` — and a reading like `payMents`, `AMOunt` or
//! `TOtal` takes none of them. Those readings are what a case pair costs
//! when the matcher cannot tell `o` from `O`: on a normalised grid the two
//! are the same shape, so only four of the 107 dimensions can separate them,
//! and the decoder is where the rest of the evidence lives. See
//! [`CaseShape`] for the state machine and what it deliberately does not
//! penalise.
//!
//! `char_bonus` is what makes paths of different lengths comparable. Every
//! other term is negative or zero, so without a per-character credit the
//! shortest path through the lattice starts ahead of every longer one and the
//! decoder merges narrow letters into wide ones to collect the saving. The
//! credit is the distance a correct character is measured to cost, so an
//! average character is score-neutral rather than a cost to be avoided.
//!
//! All four weights, the tier bonuses and the segmentation penalties are in
//! `model/params.tsv`, where each carries its provenance. Every one of them
//! except `w_match` is currently labelled a guess.
//!
//! # Determinism
//!
//! Accumulation is `f64`. No logarithm, exponential or power is taken here —
//! the bigram table stores log2 values and the confusion table stores log2
//! adjustments, so this stage only adds and multiplies, which is what makes a
//! golden fixture carrying a decoder score valid on both x86 and wasm32.
//!
//! Hypotheses are ordered by [`Hypothesis::better_than`], which breaks an
//! exact tie by lowest class index and then by earliest segmentation cut, per
//! `ARCHITECTURE.md` section 8.2. That rule makes the beam's contents — not
//! merely its winner — reproducible, which matters because the beam is
//! truncated and a different order would drop a different hypothesis.
//!
//! # Contract
//!
//! [`decode_word`] is total: it returns a reading for any lattice that has at
//! least one path from start to end, and `None` only when the lattice offers
//! no complete path at all. Missing tables are not an error — a decode with no
//! bigrams, no lexicon and no confusions is the pure matcher reading, which is
//! the behaviour a model built before those tables existed should have.

use crate::decode::bigram::Bigrams;
use crate::decode::confusion::{self, Confusions};
use crate::decode::lexicon::Lexicon;
use crate::layout::segment::EdgeKind;
use crate::params::Decode;

/// One class the matcher offered for one lattice edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cand {
    pub class: u16,
    /// Distance to the nearest prototype of this class. Lower is better.
    pub distance: f32,
    /// `d1 / d2` from the match: how much better the winner was than the best
    /// rival of a different class. Carried through so the caller can turn the
    /// chosen reading into a confidence without matching again.
    pub ratio: f32,
}

/// One lattice edge with the matcher's answers for it.
#[derive(Debug, Clone)]
pub struct Hyp {
    pub from: usize,
    pub to: usize,
    /// Page x of the left cut, inclusive.
    pub x0: u32,
    /// Page x of the right cut, exclusive.
    pub x1: u32,
    pub kind: EdgeKind,
    /// Ink width over ink height for the cropped image. Used only by the
    /// segmentation prior.
    pub aspect: f32,
    /// Candidates, best first. An edge with none is unreadable and is skipped.
    pub cands: Vec<Cand>,
}

/// The lattice for one word, with its matches.
#[derive(Debug, Clone)]
pub struct WordLattice {
    /// Number of lattice nodes; node `0` is the start and `nodes - 1` the end.
    pub nodes: usize,
    pub edges: Vec<Hyp>,
}

/// What a class is, for the context tests. One byte per class, built once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClassInfo {
    pub digit: bool,
    pub letter: bool,
    pub upper: bool,
}

impl ClassInfo {
    /// Derives the flags from a codepoint.
    pub fn of(c: char) -> ClassInfo {
        ClassInfo { digit: c.is_ascii_digit(), letter: c.is_alphabetic(), upper: c.is_uppercase() }
    }
}

/// The case shape of the letter run a path is currently inside.
///
/// Printed alphabetic words come in three shapes and no others worth
/// modelling here: all lowercase (`amount`), title case (`Amount`), and all
/// uppercase (`AMOUNT`). A transition that leaves all three is charged
/// `case_shape_penalty` once, and the state moves to whichever shape the new
/// character is consistent with, so one bad character costs one penalty
/// rather than poisoning the rest of the word.
///
/// # What this deliberately does not penalise
///
/// The run **resets at every non-letter**, so `E&OE`, `PART NO.` and
/// `M8x1.25` are scored as separate runs and cost nothing. The term is also
/// suppressed outright inside an identifier-shaped word, the same
/// suppression `CLAUDE.md` rule 6 applies to the lexicon — a part number is
/// exactly where an orthographic prior is most likely to be confidently
/// wrong.
///
/// Intercaps that a reader would accept — `McDonald`, `iPhone`, `PhD` — are
/// charged. That is the admitted cost: they are rare in printed documents
/// and CAD drawing text (`CLAUDE.md` rule 7), and one penalty against a
/// correct match that is otherwise winning does not overturn it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseShape {
    /// No letters yet in the current run: the word start, or just after a
    /// digit, space or punctuation mark.
    Start,
    /// One or more lowercase letters and nothing else: `amount`.
    Lower,
    /// Exactly one letter so far, uppercase. Still could become either
    /// `Title` or `Upper`, so nothing is charged yet.
    OneUpper,
    /// An initial capital then lowercase: `Amount`.
    Title,
    /// Two or more uppercase letters: `AMOUNT`.
    Upper,
}

impl CaseShape {
    /// The state after reading a character, and whether the step was
    /// anomalous.
    ///
    /// The one place the three admitted shapes are written down. A reader
    /// checking this against the doc above should find the table exhaustive:
    /// five states times three character kinds.
    pub fn step(self, info: ClassInfo) -> (CaseShape, bool) {
        if !info.letter {
            return (CaseShape::Start, false);
        }
        match (self, info.upper) {
            (CaseShape::Start, true) => (CaseShape::OneUpper, false),
            (CaseShape::Start, false) => (CaseShape::Lower, false),
            (CaseShape::Lower, false) => (CaseShape::Lower, false),
            // `payMents`: an uppercase letter after lowercase.
            (CaseShape::Lower, true) => (CaseShape::OneUpper, true),
            (CaseShape::OneUpper, false) => (CaseShape::Title, false),
            (CaseShape::OneUpper, true) => (CaseShape::Upper, false),
            (CaseShape::Title, false) => (CaseShape::Title, false),
            // `AmOunt`: a capital resuming inside a title-cased word.
            (CaseShape::Title, true) => (CaseShape::OneUpper, true),
            (CaseShape::Upper, true) => (CaseShape::Upper, false),
            // `AMOunt`, `TOtal`, `DOWel`: lowercase after a capital run.
            (CaseShape::Upper, false) => (CaseShape::Title, true),
        }
    }
}

/// The authored tables the decoder consults, all optional.
#[derive(Debug, Clone, Copy, Default)]
pub struct Tables<'a> {
    pub bigrams: Option<&'a Bigrams>,
    pub lexicon: Option<&'a Lexicon>,
    pub confusions: Option<&'a Confusions>,
}

/// One decoded character.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Char {
    pub class: u16,
    pub x0: u32,
    pub x1: u32,
    /// The match ratio for the candidate that won. Feed to
    /// `confidence::character`.
    pub ratio: f32,
}

/// One decoded word.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub chars: Vec<Char>,
    /// The winning path's total score, in the units above. Comparable between
    /// readings of the *same* word and meaningless between different ones.
    pub score: f64,
    /// The lexicon tier the whole word matched, when it matched one.
    pub tier: Option<u8>,
    /// Whether the word was judged identifier-shaped, which suppressed the
    /// lexicon term.
    pub identifier: bool,
}

/// A partial path through the lattice.
#[derive(Debug, Clone, Copy)]
struct Hypothesis {
    score: f64,
    /// Lexicon node, or `None` once the path left the graph.
    lex: Option<u32>,
    /// Class of the last character, or `None` at the word start.
    prev: Option<u16>,
    /// Case shape of the letter run this path is inside. See [`CaseShape`].
    case: CaseShape,
    /// Index into the trace arena of the last step, or `usize::MAX` at start.
    trace: usize,
    /// The lattice node this path has reached. Used only for tie-breaking.
    from: usize,
}

impl Hypothesis {
    /// A total order over hypotheses at one lattice node.
    ///
    /// Score decides; an exact tie goes to the lower class index, then to the
    /// earlier segmentation cut, then to the older trace entry. The last term
    /// is what makes the order total rather than merely deterministic-looking:
    /// without it two paths identical in the first three could still swap.
    fn better_than(&self, other: &Hypothesis) -> bool {
        if self.score != other.score {
            return self.score > other.score;
        }
        let a = self.prev.unwrap_or(u16::MAX);
        let b = other.prev.unwrap_or(u16::MAX);
        if a != b {
            return a < b;
        }
        if self.from != other.from {
            return self.from < other.from;
        }
        self.trace < other.trace
    }
}

/// One step of a path, kept so the winner can be walked back to a string.
#[derive(Debug, Clone, Copy)]
struct Step {
    parent: usize,
    class: u16,
    x0: u32,
    x1: u32,
    ratio: f32,
}

/// Decodes one word's lattice.
///
/// `class_info` is indexed by class and must cover every class the matcher can
/// return; a class beyond it is treated as neither letter nor digit rather
/// than panicking, because a model and a charset that disagree is a load-time
/// problem and not something to take a page down for.
///
/// `slanted` is the layout stage's own verdict for this word
/// (`crate::layout::slant::Slant::slanted`), carried in rather than
/// recomputed here: this stage scores a lattice, it does not measure pixels.
/// When `true`, every edge's match term is credited `p.char_bonus_slanted`
/// instead of `p.char_bonus`; a caller that always passes `false` sees no
/// behaviour change from before this parameter existed, regardless of what
/// the two params carry.
///
/// Returns `None` when no path reaches the end node.
pub fn decode_word(
    lat: &WordLattice,
    class_info: &[ClassInfo],
    t: &Tables<'_>,
    p: &Decode,
    slanted: bool,
) -> Option<Word> {
    if lat.nodes == 0 || lat.edges.is_empty() {
        return None;
    }
    let end = lat.nodes - 1;
    let bonus = if slanted { p.char_bonus_slanted } else { p.char_bonus };

    // Edges leaving each node, in lattice order. Built once so the beam does
    // not rescan the edge list per node, and so the visit order is fixed.
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); lat.nodes];
    for (i, e) in lat.edges.iter().enumerate() {
        if e.from < lat.nodes && e.to <= end && e.to > e.from && !e.cands.is_empty() {
            out[e.from].push(i);
        }
    }

    let probe = probe_reading(lat, &out);
    let identifier = is_identifier(&probe, class_info, p);
    let info = |c: u16| class_info.get(c as usize).copied().unwrap_or_default();

    let beam = p.beam_width.max(1) as usize;
    let mut arena: Vec<Step> = Vec::new();
    let mut beams: Vec<Vec<Hypothesis>> = vec![Vec::new(); lat.nodes];
    beams[0].push(Hypothesis {
        score: 0.0,
        lex: t.lexicon.map(|l| l.root_node()).unwrap_or(None),
        prev: None,
        case: CaseShape::Start,
        trace: usize::MAX,
        from: 0,
    });

    for node in 0..end {
        // Take the node's beam so the loop below can push into later nodes.
        let here = core::mem::take(&mut beams[node]);
        if here.is_empty() {
            continue;
        }
        for &ei in &out[node] {
            let e = &lat.edges[ei];
            let seg = f64::from(p.w_seg) * segmentation_prior(e, p);
            // Contexts that do not depend on the class being chosen.
            let mut fixed = 0u8;
            if e.from == 0 {
                fixed |= confusion::CTX_WORD_START;
            }
            if e.to == end {
                fixed |= confusion::CTX_WORD_END;
            }
            if identifier {
                fixed |= confusion::CTX_IDENTIFIER;
            }
            // Neighbours are read from the probe, because "the adjacent
            // character reads as a digit" is a statement about the raw atom
            // reading and not about a decision this path has not made yet.
            if let Some(c) = probe_at(&probe, e.from, false).map(&info) {
                if c.digit {
                    fixed |= confusion::CTX_DIGIT_NEIGHBOUR;
                }
                if c.letter {
                    fixed |= confusion::CTX_LETTER_NEIGHBOUR;
                }
            }
            if let Some(c) = probe_at(&probe, e.to, true).map(&info) {
                if c.digit {
                    fixed |= confusion::CTX_DIGIT_NEIGHBOUR;
                }
                if c.letter {
                    fixed |= confusion::CTX_LETTER_NEIGHBOUR;
                }
            }

            for h in &here {
                let mut ctx = fixed;
                if h.prev.map(&info).is_some_and(|c| c.upper) {
                    ctx |= confusion::CTX_UPPER_RUN;
                }
                for c in &e.cands {
                    let lex = t.lexicon.map(|l| l.step_node(h.lex, c.class)).unwrap_or(None);
                    let mut ctx = ctx;
                    if lex.is_some() {
                        ctx |= confusion::CTX_LEXICON_WORD;
                    }
                    let mut s = h.score;
                    s += f64::from(p.w_match) * (f64::from(bonus) - f64::from(c.distance));
                    s += seg;
                    if let Some(bg) = t.bigrams {
                        let prev = h.prev.unwrap_or_else(|| bg.boundary());
                        s += f64::from(p.w_bigram) * f64::from(bg.logp(prev, c.class));
                    }
                    if let Some(cf) = t.confusions {
                        s += f64::from(p.w_confusion) * f64::from(cf.adjust(c.class, ctx));
                    }
                    // Orthography, charged as the path goes rather than at the
                    // word end: a word-final term cannot steer a beam that has
                    // already dropped the right reading.
                    let (case, anomalous) = h.case.step(info(c.class));
                    if anomalous && !identifier {
                        s -= f64::from(p.case_shape_penalty);
                    }
                    if e.to == end {
                        if let Some(bg) = t.bigrams {
                            s += f64::from(p.w_bigram) * f64::from(bg.logp(c.class, bg.boundary()));
                        }
                        if !identifier {
                            if let (Some(l), Some(_)) = (t.lexicon, lex) {
                                if let Some(tier) = l.tier_at(lex) {
                                    let i = (tier.clamp(1, 5) - 1) as usize;
                                    s += f64::from(p.w_lex) * f64::from(p.lex_bonus[i]);
                                }
                            }
                        }
                    }
                    if !s.is_finite() {
                        continue;
                    }
                    arena.push(Step {
                        parent: h.trace,
                        class: c.class,
                        x0: e.x0,
                        x1: e.x1,
                        ratio: c.ratio,
                    });
                    let next = Hypothesis {
                        score: s,
                        lex,
                        prev: Some(c.class),
                        case,
                        trace: arena.len() - 1,
                        from: e.from,
                    };
                    insert(&mut beams[e.to], next, beam);
                }
            }
        }
    }

    let best = *beams[end].first()?;
    let mut chars = Vec::new();
    let mut i = best.trace;
    while i != usize::MAX {
        let s = arena[i];
        chars.push(Char { class: s.class, x0: s.x0, x1: s.x1, ratio: s.ratio });
        i = s.parent;
    }
    chars.reverse();
    let tier = if identifier { None } else { t.lexicon.and_then(|l| l.tier_at(best.lex)) };
    Some(Word { chars, score: best.score, tier, identifier })
}

/// Keeps a node's beam sorted best-first and capped.
///
/// A linear insert rather than a sort-at-the-end, because the cap is small and
/// the order has to be stable against the tie-break rule at every moment, not
/// only once the node is finished.
fn insert(beam: &mut Vec<Hypothesis>, h: Hypothesis, cap: usize) {
    let at = beam.iter().position(|x| h.better_than(x)).unwrap_or(beam.len());
    if at >= cap {
        return;
    }
    beam.insert(at, h);
    beam.truncate(cap);
}

/// The atom reading: the top-1 class of each `Single` edge, indexed by the
/// node the edge leaves.
///
/// This is a *probe*, not a decision. It exists because two of the context
/// tests and the identifier test are statements about the neighbours, and a
/// decoder cannot consult a decision it has not made. Taking the raw
/// component reading is both cheap and the thing those tests actually mean:
/// what the glyph beside this one looks like before anything argued about it.
fn probe_reading(lat: &WordLattice, out: &[Vec<usize>]) -> Vec<Option<(usize, u16)>> {
    let mut probe: Vec<Option<(usize, u16)>> = vec![None; lat.nodes];
    for (node, edges) in out.iter().enumerate() {
        let pick = edges
            .iter()
            .filter(|&&i| lat.edges[i].kind == EdgeKind::Single)
            .min_by_key(|&&i| lat.edges[i].to)
            .or_else(|| edges.iter().min_by_key(|&&i| lat.edges[i].to));
        if let Some(&i) = pick {
            if let Some(c) = lat.edges[i].cands.first() {
                probe[node] = Some((lat.edges[i].to, c.class));
            }
        }
    }
    probe
}

/// The probe's class leaving `node` (`forward`) or arriving at it.
fn probe_at(probe: &[Option<(usize, u16)>], node: usize, forward: bool) -> Option<u16> {
    if forward {
        return probe.get(node).copied().flatten().map(|(_, c)| c);
    }
    probe.iter().flatten().find(|(to, _)| *to == node).map(|(_, c)| *c)
}

/// Whether the probe reading is identifier-shaped, which suppresses the
/// lexicon term for the whole word.
///
/// The test is character count, not a pattern: enough of the characters read
/// as digits and the word is long enough to be a code at all. A pattern would
/// have to enumerate the shapes a part number takes, and the shapes are the
/// thing this project does not get to assume.
fn is_identifier(probe: &[Option<(usize, u16)>], info: &[ClassInfo], p: &Decode) -> bool {
    let classes: Vec<u16> = probe.iter().flatten().map(|(_, c)| *c).collect();
    let digits = classes
        .iter()
        .filter(|c| info.get(**c as usize).copied().unwrap_or_default().digit)
        .count();
    let letters = classes
        .iter()
        .filter(|c| info.get(**c as usize).copied().unwrap_or_default().letter)
        .count();
    crate::params::identifier_shape(classes.len(), digits, letters, p)
}

/// The segmentation prior for one edge, in log2 units and never positive.
///
/// Two claims, both authored and both on chunk 8's tuning list:
///
/// * A character whose ink is far from the typical width-over-height band is
///   less likely to be one character. The penalty is measured in *tolerance
///   widths* past the band rather than in raw aspect, so it is dimensionless
///   and does not need a scale parameter of its own.
/// * Taking several atoms as one character, or cutting inside one, is a
///   hypothesis about the image being broken or touching. Taking an atom whole
///   is the ordinary case and costs nothing.
fn segmentation_prior(e: &Hyp, p: &Decode) -> f64 {
    let mut s = match e.kind {
        EdgeKind::Single => 0.0,
        EdgeKind::Merge => -f64::from(p.seg_merge_penalty),
        EdgeKind::Split => -f64::from(p.seg_split_penalty),
    };
    let tol = f64::from(p.seg_aspect_tolerance);
    if tol > 0.0 && e.aspect.is_finite() {
        let excess = (f64::from(e.aspect) - f64::from(p.seg_ideal_aspect)).abs() - tol;
        if excess > 0.0 {
            s -= excess / tol;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(class: u16, distance: f32) -> Cand {
        Cand { class, distance, ratio: 0.5 }
    }

    fn edge(from: usize, to: usize, cands: Vec<Cand>) -> Hyp {
        Hyp {
            from,
            to,
            x0: from as u32 * 10,
            x1: to as u32 * 10,
            kind: EdgeKind::Single,
            aspect: 0.6,
            cands,
        }
    }

    /// Classes 0..=9 are digits, 10..=35 are uppercase letters, 36..=61 are
    /// lowercase. Enough shape for the context tests without a charset.
    fn info() -> Vec<ClassInfo> {
        (0..62u16)
            .map(|i| match i {
                0..=9 => ClassInfo { digit: true, letter: false, upper: false },
                10..=35 => ClassInfo { digit: false, letter: true, upper: true },
                _ => ClassInfo { digit: false, letter: true, upper: false },
            })
            .collect()
    }

    fn classes(w: &Word) -> Vec<u16> {
        w.chars.iter().map(|c| c.class).collect()
    }

    #[test]
    fn with_no_tables_the_decoder_returns_the_matcher_reading() {
        let lat = WordLattice {
            nodes: 3,
            edges: vec![
                edge(0, 1, vec![cand(36, 0.1), cand(37, 0.9)]),
                edge(1, 2, vec![cand(38, 0.2), cand(39, 0.8)]),
            ],
        };
        let w = decode_word(&lat, &info(), &Tables::default(), &crate::params::Params::DEFAULT.decode, false)
            .expect("a path exists");
        assert_eq!(classes(&w), vec![36, 38]);
        assert!(w.tier.is_none() && !w.identifier);
    }

    #[test]
    fn a_lattice_with_no_complete_path_decodes_to_nothing() {
        let lat = WordLattice { nodes: 3, edges: vec![edge(0, 1, vec![cand(36, 0.1)])] };
        assert!(
            decode_word(&lat, &info(), &Tables::default(), &crate::params::Params::DEFAULT.decode, false)
                .is_none()
        );
    }

    /// Two segmentations of the same ink, decided by what the pieces match.
    ///
    /// Distances are read against `char_bonus`, which is the distance a
    /// correct character is measured to cost: 2.0 is a good match and 6.0 a
    /// poor one, so this is one clean merged glyph against two pieces that
    /// barely matched anything.
    #[test]
    fn the_better_scoring_segmentation_wins() {
        let mut merged = edge(0, 2, vec![cand(40, 2.0)]);
        merged.kind = EdgeKind::Merge;
        let lat = WordLattice {
            nodes: 3,
            edges: vec![
                edge(0, 1, vec![cand(36, 6.0)]),
                edge(1, 2, vec![cand(37, 6.0)]),
                merged,
            ],
        };
        let w = decode_word(&lat, &info(), &Tables::default(), &crate::params::Params::DEFAULT.decode, false)
            .expect("a path exists");
        assert_eq!(classes(&w), vec![40], "the single good match should beat two poor ones");
    }

    /// The bias `char_bonus` exists to remove: two ordinary characters must
    /// beat one merged glyph that matched no better than ordinary.
    ///
    /// Without the per-character credit every term in the score is negative,
    /// so the merged edge wins by emitting one fewer character regardless of
    /// what the ink says — which is what "Invoice" read as "Invææ" was.
    #[test]
    fn two_ordinary_characters_beat_one_merged_glyph_of_the_same_quality() {
        let p = crate::params::Params::DEFAULT.decode;
        let mut merged = edge(0, 2, vec![cand(40, p.char_bonus)]);
        merged.kind = EdgeKind::Merge;
        let lat = WordLattice {
            nodes: 3,
            edges: vec![
                edge(0, 1, vec![cand(36, p.char_bonus)]),
                edge(1, 2, vec![cand(37, p.char_bonus)]),
                merged,
            ],
        };
        let w = decode_word(&lat, &info(), &Tables::default(), &p, false).expect("a path exists");
        assert_eq!(classes(&w), vec![36, 37]);
    }

    /// `char_bonus_slanted` only reaches the score when `decode_word` is told
    /// the word is slanted. A non-slanted word must decode identically —
    /// same reading and the same `f64` score — no matter what value
    /// `char_bonus_slanted` carries, because `slanted = false` should route
    /// every edge's match term through `char_bonus` alone.
    #[test]
    fn a_non_slanted_word_ignores_char_bonus_slanted() {
        let lat = WordLattice {
            nodes: 3,
            edges: vec![
                edge(0, 1, vec![cand(36, 1.2), cand(37, 3.0)]),
                edge(1, 2, vec![cand(38, 2.5), cand(39, 5.5)]),
            ],
        };
        let mut p = crate::params::Params::DEFAULT.decode;
        let base = decode_word(&lat, &info(), &Tables::default(), &p, false).expect("decodes");

        // A value that would obviously move the score if it were consulted.
        p.char_bonus_slanted = p.char_bonus + 100.0;
        let moved = decode_word(&lat, &info(), &Tables::default(), &p, false).expect("decodes");
        assert_eq!(base, moved, "char_bonus_slanted must not affect a slanted=false decode");

        // Setting char_bonus_slanted equal to char_bonus is behaviour-neutral
        // even when the word IS slanted -- this is what made shipping the new
        // parameter safe before it was measured, and is checked here so it
        // stays true regardless of what the shipped default becomes.
        p.char_bonus_slanted = p.char_bonus;
        let slanted_default = decode_word(&lat, &info(), &Tables::default(), &p, true).expect("decodes");
        assert_eq!(base, slanted_default);
    }

    /// An edge far outside the aspect band is charged for it, and the charge
    /// is what decides between two otherwise identical readings.
    #[test]
    fn a_wildly_wrong_aspect_costs_the_edge() {
        let p = crate::params::Params::DEFAULT.decode;
        let ordinary = edge(0, 1, vec![cand(36, 0.5)]);
        let mut wide = ordinary.clone();
        wide.aspect = 6.0;
        assert_eq!(segmentation_prior(&ordinary, &p), 0.0);
        assert!(segmentation_prior(&wide, &p) < -1.0);
    }

    #[test]
    fn a_merge_costs_more_than_a_single_and_a_split_more_than_a_merge() {
        let p = crate::params::Params::DEFAULT.decode;
        let single = edge(0, 1, vec![cand(36, 0.5)]);
        let mut merge = single.clone();
        merge.kind = EdgeKind::Merge;
        let mut split = single.clone();
        split.kind = EdgeKind::Split;
        assert!(segmentation_prior(&single, &p) > segmentation_prior(&merge, &p));
        assert!(segmentation_prior(&merge, &p) > segmentation_prior(&split, &p));
    }

    /// Identifier detection is on the probe reading, so it does not depend on
    /// a decision the decoder has not made.
    #[test]
    fn a_digit_heavy_word_with_letters_in_it_is_identifier_shaped() {
        let p = crate::params::Params::DEFAULT.decode;
        // "M8" -> uppercase then digit.
        let probe = vec![Some((1usize, 22u16)), Some((2usize, 8u16)), None];
        assert!(is_identifier(&probe, &info(), &p));
        // A pure number is not: there is nothing for the lexicon to offer.
        let probe = vec![Some((1usize, 1u16)), Some((2usize, 2u16)), None];
        assert!(!is_identifier(&probe, &info(), &p));
        // An ordinary word is not.
        let probe = vec![Some((1usize, 40u16)), Some((2usize, 41u16)), None];
        assert!(!is_identifier(&probe, &info(), &p));
    }

    /// The same lattice decoded twice gives the same answer, including when
    /// every candidate scores identically and only the tie-break separates
    /// them.
    /// Walks a word through the shape machine and returns the positions that
    /// were charged, so a test reads as the word it is about.
    fn case_penalties(word: &str) -> Vec<usize> {
        let mut st = CaseShape::Start;
        let mut hits = Vec::new();
        for (i, ch) in word.chars().enumerate() {
            let (next, bad) = st.step(ClassInfo::of(ch));
            if bad {
                hits.push(i);
            }
            st = next;
        }
        hits
    }

    #[test]
    fn the_three_shapes_printed_words_take_are_free() {
        for w in ["amount", "Amount", "AMOUNT", "a", "A", "I"] {
            assert_eq!(case_penalties(w), Vec::<usize>::new(), "{w} should be free");
        }
    }

    #[test]
    fn the_case_readings_the_matcher_actually_produces_are_charged_once_each() {
        // Every one of these is a real end-to-end misreading from
        // `bench/pages`, where the matcher could not separate a case pair.
        assert_eq!(case_penalties("payMents"), vec![3]);
        assert_eq!(case_penalties("AMOunt"), vec![3]);
        assert_eq!(case_penalties("TOtal"), vec![2]);
        assert_eq!(case_penalties("DOWel"), vec![3]);
        assert_eq!(case_penalties("NOrthgate"), vec![2]);
    }

    /// The run resets at a non-letter, so a mixed token is scored as the
    /// separate words a reader sees rather than as one malformed one.
    #[test]
    fn a_non_letter_resets_the_run() {
        assert_eq!(case_penalties("E&OE"), Vec::<usize>::new());
        assert_eq!(case_penalties("M8x1.25"), Vec::<usize>::new());
        assert_eq!(case_penalties("4X"), Vec::<usize>::new());
        assert_eq!(case_penalties("PART NO."), Vec::<usize>::new());
    }

    /// One bad character costs one penalty, not the rest of the word.
    #[test]
    fn a_single_anomaly_does_not_poison_the_remainder() {
        assert_eq!(case_penalties("AMOuntsxx"), vec![3]);
        assert_eq!(case_penalties("payMentsxx"), vec![3]);
    }

    #[test]
    fn an_exact_tie_breaks_the_same_way_every_time() {
        let lat = WordLattice {
            nodes: 3,
            edges: vec![
                edge(0, 1, vec![cand(40, 0.5), cand(36, 0.5), cand(38, 0.5)]),
                edge(1, 2, vec![cand(41, 0.5), cand(37, 0.5)]),
            ],
        };
        let p = crate::params::Params::DEFAULT.decode;
        let first = decode_word(&lat, &info(), &Tables::default(), &p, false).expect("decodes");
        for _ in 0..8 {
            let again = decode_word(&lat, &info(), &Tables::default(), &p, false).expect("decodes");
            assert_eq!(first, again);
        }
        // And the rule is the documented one: lowest class index wins a tie.
        assert_eq!(classes(&first), vec![36, 37]);
    }

    /// The beam is capped, and capping it does not change the winner on a
    /// lattice small enough to search exhaustively.
    #[test]
    fn narrowing_the_beam_does_not_change_the_winner_here() {
        let lat = WordLattice {
            nodes: 4,
            edges: vec![
                edge(0, 1, vec![cand(36, 0.1), cand(37, 0.4), cand(38, 0.7)]),
                edge(1, 2, vec![cand(39, 0.2), cand(40, 0.5)]),
                edge(2, 3, vec![cand(41, 0.3), cand(42, 0.6)]),
            ],
        };
        let mut p = crate::params::Params::DEFAULT.decode;
        let wide = decode_word(&lat, &info(), &Tables::default(), &p, false).expect("decodes");
        p.beam_width = 1;
        let narrow = decode_word(&lat, &info(), &Tables::default(), &p, false).expect("decodes");
        assert_eq!(classes(&wide), classes(&narrow));
    }
}

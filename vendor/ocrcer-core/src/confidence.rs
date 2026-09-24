//! Confidence: the match margin pushed through an authored calibration
//! curve, then aggregated by geometric mean.
//!
//! # Contract
//!
//! [`character`] maps a matcher's `d1/d2` ratio to a confidence in
//! `0.0..=1.0`. [`word`] is the geometric mean over its characters and
//! [`line`] the geometric mean over its words weighted by character count
//! (`ARCHITECTURE.md` section 4.2).
//!
//! **Margin, not absolute distance.** Two classes that match a glyph equally
//! well must report low confidence even when both matched well in absolute
//! terms, because that ambiguity is precisely what a reviewer needs told and
//! precisely what a raw distance hides (`CLAUDE.md` rule 5).
//!
//! **Geometric, not arithmetic.** The quantity is a product of independent
//! probabilities, and an arithmetic mean lets one confident character mask a
//! hopeless one — which is the case that has to be surfaced, not smoothed.
//!
//! # Determinism
//!
//! No `ln`, no `exp`, no `powf`. IEEE 754 does not require any of them to be
//! correctly rounded, so a geometric mean taken through logarithms could
//! differ between x86 and wasm32 and every golden fixture carrying a
//! confidence would become platform-dependent. The n-th root here is
//! bisection over multiplication only, at a fixed iteration count, so it
//! gives the same bits everywhere.

/// Smallest confidence carried into an aggregate.
///
/// A character below this is indistinguishable from hopeless for any purpose
/// a caller has, and the floor is what keeps a long line's product inside
/// `f64` range rather than flushing it to zero — at which point the whole
/// line would report zero regardless of the rest of it.
pub const MIN_CONFIDENCE: f64 = 1.0e-4;

/// Bisection steps in the n-th root. Sixty-four halvings of `[0, 1]` land
/// below an `f64`'s last bit everywhere in the interval, and a *fixed* count
/// is what makes the answer identical on every target.
const ROOT_STEPS: u32 = 64;

/// The authored map from match ratio to confidence.
///
/// `knots` is `(ratio, confidence)`, ascending in ratio and non-increasing in
/// confidence, interpolated linearly between and clamped outside.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Calibration {
    pub knots: [(f32, f32); 6],
    /// How far the decoder's language model may pull a character's
    /// confidence down when it disagrees, as a floor on the multiplier.
    pub lm_floor: f32,
}

/// The curve shipped until the benchmark chunk measures one.
///
/// **Every number in it is a guess, and the whole curve is on chunk 8's
/// tuning list.** What is authored is the *shape*, and that part is
/// defensible: confidence must fall monotonically as the winner's margin
/// over its nearest rival of a different class narrows, it must be near one
/// when the rival is far away, and it must be small — not zero — when the two
/// are level, because a coin flip between two classes is still a reading.
///
/// What is not authored is where the knots sit. The honest replacement is a
/// measurement, not a better guess: bucket a corpus's characters by ratio,
/// count the fraction correct in each bucket, and put the knots on those
/// fractions. That is what makes the number a calibration rather than a
/// decoration, and it is what `reports_confidence()` returning true promises.
pub const AUTHORED: Calibration = Calibration {
    knots: [
        (0.00, 1.00),
        (0.50, 0.95),
        (0.70, 0.80),
        (0.85, 0.50),
        (0.95, 0.20),
        (1.00, 0.05),
    ],
    lm_floor: 0.80,
};

/// A character's confidence from its match ratio.
///
/// `ratio` is `d1 / d2` from [`crate::r#match::Match::ratio`]: zero when the
/// winner had no rival of another class, one when a rival matched exactly as
/// well. A non-finite ratio reads as fully ambiguous rather than as an error,
/// because a confidence is a report and not a gate.
pub fn character(cal: &Calibration, ratio: f32) -> f32 {
    if !ratio.is_finite() {
        return cal.knots[cal.knots.len() - 1].1;
    }
    let r = ratio.clamp(0.0, 1.0);
    let k = &cal.knots;
    if r <= k[0].0 {
        return k[0].1;
    }
    for i in 1..k.len() {
        if r <= k[i].0 {
            let (x0, y0) = k[i - 1];
            let (x1, y1) = k[i];
            let span = x1 - x0;
            if span <= 0.0 {
                return y1;
            }
            let t = (r - x0) / span;
            return y0 + (y1 - y0) * t;
        }
    }
    k[k.len() - 1].1
}

/// A character's confidence after the decoder's language model has had its
/// say.
///
/// `agreement` is `0.0` when the language model would have chosen otherwise
/// and `1.0` when it chose the same character. The floor is what keeps the
/// lexicon a bonus rather than a constraint (`CLAUDE.md` rule 6): a model
/// that disagrees may lower a confidence, but it may not drive one to zero,
/// because the string it disagrees with is often a part number it has never
/// seen and has no business doubting.
pub fn adjust(cal: &Calibration, base: f32, agreement: f32) -> f32 {
    let a = if agreement.is_finite() { agreement.clamp(0.0, 1.0) } else { 1.0 };
    let f = cal.lm_floor.clamp(0.0, 1.0);
    (base * (f + (1.0 - f) * a)).clamp(0.0, 1.0)
}

/// A word's confidence: the geometric mean over its characters.
///
/// An empty word reports `0.0`. Nothing was read, so nothing was read
/// confidently — reporting `1.0` for the absence of evidence is the error
/// this avoids.
pub fn word(chars: &[f32]) -> f32 {
    let items: Vec<(f64, u32)> = chars.iter().map(|&c| (f64::from(c), 1u32)).collect();
    weighted_geometric_mean(&items) as f32
}

/// A line's confidence: the geometric mean over its words, weighted by
/// character count.
///
/// Weighted because a one-character word and a twelve-character word are not
/// equal evidence about the line.
pub fn line(words: &[(f32, u32)]) -> f32 {
    let items: Vec<(f64, u32)> =
        words.iter().map(|&(c, n)| (f64::from(c), n)).filter(|&(_, n)| n > 0).collect();
    weighted_geometric_mean(&items) as f32
}

/// Weighted geometric mean of `(value, weight)` pairs, without logarithms.
///
/// Values are clamped into `[MIN_CONFIDENCE, 1.0]` first, so a caller that
/// hands over a zero gets a very small line rather than a zero line — one
/// unreadable character is not evidence that the rest of the page was
/// unreadable.
///
/// **Why the root comes first.** The obvious order — multiply everything,
/// then take the `W`-th root — underflows on a long line: a hundred
/// characters at `0.3` is already `1e-53`, and a page-length product is
/// zero. Rooting each value first and multiplying the roots gives the same
/// number by the same identity, and every factor is then at most one and at
/// least the final answer, so the running product falls monotonically from
/// `1.0` to the mean and can never leave `f64` range at all.
fn weighted_geometric_mean(items: &[(f64, u32)]) -> f64 {
    let mut total: u64 = 0;
    for &(_, w) in items {
        total += u64::from(w);
    }
    if total == 0 {
        return 0.0;
    }

    let mut product = 1.0f64;
    for &(v, w) in items {
        if w == 0 {
            continue;
        }
        let root = nth_root(v.clamp(MIN_CONFIDENCE, 1.0), total);
        product *= powi(root, u64::from(w));
    }
    product.clamp(0.0, 1.0)
}

/// `x^n` by squaring. Multiplication only, so it gives the same bits on every
/// target — which `powf` does not promise.
fn powi(x: f64, n: u64) -> f64 {
    let mut result = 1.0f64;
    let mut base = x;
    let mut e = n;
    while e > 0 {
        if e & 1 == 1 {
            result *= base;
        }
        e >>= 1;
        if e > 0 {
            base *= base;
        }
    }
    result
}

/// The `n`-th root of `p` for `p` in `(0, 1]`, by bisection.
///
/// Bisection rather than `powf` for the reason in the module doc: it uses
/// only comparison and multiplication, both exactly specified by IEEE 754, at
/// a fixed step count.
fn nth_root(p: f64, n: u64) -> f64 {
    if n == 0 {
        return 0.0;
    }
    if n == 1 || p <= 0.0 {
        return p.clamp(0.0, 1.0);
    }
    if p >= 1.0 {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..ROOT_STEPS {
        let mid = (lo + hi) * 0.5;
        if powi(mid, n) < p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_falls_as_the_margin_narrows() {
        let mut last = f32::INFINITY;
        for i in 0..=20 {
            let c = character(&AUTHORED, i as f32 / 20.0);
            assert!(c <= last + 1e-6, "not monotone at ratio {}", i as f32 / 20.0);
            assert!((0.0..=1.0).contains(&c));
            last = c;
        }
    }

    /// The case `CLAUDE.md` rule 5 names: two classes matching equally well
    /// must report low confidence even though both matched well.
    #[test]
    fn two_equally_good_classes_report_low_confidence() {
        assert!(character(&AUTHORED, 1.0) < 0.1);
        assert!(character(&AUTHORED, 0.0) > 0.99);
    }

    #[test]
    fn the_knots_are_hit_exactly() {
        for &(r, c) in &AUTHORED.knots {
            assert!((character(&AUTHORED, r) - c).abs() < 1e-6, "knot at {r}");
        }
    }

    /// An arithmetic mean would report 0.55 here and hide the hopeless
    /// character. The geometric mean is what surfaces it.
    #[test]
    fn one_hopeless_character_drags_the_word_down() {
        let w = word(&[1.0, 1.0, 1.0, 0.1]);
        assert!(w < 0.6, "word confidence was {w}");
        assert!(w > 0.0);
    }

    #[test]
    fn a_uniform_word_reports_its_own_value() {
        let w = word(&[0.8, 0.8, 0.8, 0.8, 0.8]);
        assert!((w - 0.8).abs() < 1e-5, "got {w}");
    }

    /// The property that makes the root worth writing by hand: a long line
    /// must not flush to zero just because the product underflowed.
    #[test]
    fn a_very_long_line_does_not_underflow_to_zero() {
        let chars = vec![0.35f32; 4000];
        let w = word(&chars);
        assert!((w - 0.35).abs() < 1e-3, "got {w}");
    }

    #[test]
    fn a_line_weights_its_words_by_character_count() {
        // One confident twelve-character word beside one hopeless
        // one-character word: the line should lean toward the long one.
        let l = line(&[(0.9, 12), (0.2, 1)]);
        assert!(l > 0.75 && l < 0.9, "got {l}");
        // And reversing the weights reverses the lean.
        let l2 = line(&[(0.9, 1), (0.2, 12)]);
        assert!(l2 < 0.3, "got {l2}");
    }

    #[test]
    fn nothing_read_is_not_read_confidently() {
        assert_eq!(word(&[]), 0.0);
        assert_eq!(line(&[]), 0.0);
        assert_eq!(line(&[(0.9, 0)]), 0.0);
    }

    /// The language model may lower a confidence but never zero it: a part
    /// number it has never seen is not evidence against the reading.
    #[test]
    fn the_language_model_can_lower_a_confidence_but_not_erase_it() {
        let base = 0.9;
        let agree = adjust(&AUTHORED, base, 1.0);
        let disagree = adjust(&AUTHORED, base, 0.0);
        assert!((agree - base).abs() < 1e-6);
        assert!(disagree < base);
        assert!(disagree >= base * AUTHORED.lm_floor - 1e-6);
    }

    #[test]
    fn the_root_inverts_the_power() {
        for n in [1u64, 2, 3, 7, 32] {
            for p in [0.999f64, 0.5, 0.1, 1e-3] {
                let r = nth_root(p, n);
                assert!((powi(r, n) - p).abs() < 1e-12, "n={n} p={p} r={r}");
            }
        }
    }

    /// Nothing here may depend on the order values arrive in, or a fixture
    /// would depend on the lattice's visit order rather than on its answer.
    #[test]
    fn aggregation_is_order_independent() {
        let a = word(&[0.9, 0.4, 0.7, 0.99, 0.2]);
        let b = word(&[0.2, 0.99, 0.7, 0.4, 0.9]);
        assert!((a - b).abs() < 1e-6, "{a} vs {b}");
    }
}

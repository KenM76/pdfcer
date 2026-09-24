//! Per-word slant estimation: shear and score, integer angles only.
//!
//! # Contract
//!
//! `ARCHITECTURE.md` section 11 (2026-09-24 decision, "Slant estimator, per
//! word, before segmentation") gates italic prototypes on measured slant
//! rather than adding them to the shared pool unconditionally. [`estimate`]
//! is the measurement: shear the word's own ink by each candidate integer
//! angle 0..=20 degrees and score the sheared vertical projection by the sum
//! of squared column-ink counts (research addendum, 2026-09-22 document,
//! "2026-09-24" section) — the angle that collapses the ink into the fewest,
//! tallest columns wins.
//!
//! # Shear direction
//!
//! In this project's y-down pixel coordinates, ordinary (right-leaning)
//! italic has its ink shifted right of upright the further *above* the
//! baseline a row sits, i.e. `italic_x = upright_x - (y - baseline)*tan(θ)`
//! for `y < baseline`. Undoing that is `deslant_x = x + (y - baseline)*tan(θ)`
//! — addition, not subtraction. This was verified empirically (2026-09-24,
//! `--layout` on `filing__r000583`, a real italic-serif page): the
//! subtracting form scored strictly *lower* at every angle above 0 on real
//! italic ink, the signature of shearing the wrong way, before the sign here
//! was corrected. [`angle_scores`] exposes the full per-angle curve for
//! exactly this kind of check.
//!
//! Deterministic and free of any runtime trig call, on purpose: `sin`/`cos`/
//! `tan` are not guaranteed bit-identical between x86 and wasm32, and this
//! project's fixtures must be (`ARCHITECTURE.md` section 8.2). [`TAN_Q12`] is
//! a compile-time table of `tan(degrees)` in Q12 fixed point, and every shear
//! is integer arithmetic against it — `round_div` is the only division, and
//! it rounds to nearest with ties away from zero, the same on every target.
//!
//! Ties go to the smaller angle (`> ` not `>=` in the angle search), which is
//! what keeps a single already-vertical mark — a table-cell border, a ruled
//! line — reading as upright without a special case: shearing a one-column
//! run only translates it, translation does not change a sum of squares, so
//! every angle scores it identically and the first (smallest) wins.

/// Detection thresholds, read from [`crate::params::Params::slant`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// The smallest best-angle, in degrees, that counts as slanted at all.
    pub slant_min_deg: f32,
    /// How much the best angle's score must beat the upright (0°) score by,
    /// as a ratio, before the word is called slanted.
    pub slant_margin: f32,
}

/// What the estimator found for one word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slant {
    /// The best-scoring integer angle, 0..=20, ties broken to the smaller.
    pub angle_deg: u32,
    /// Whether the word passes both of [`Params`]'s gates.
    pub slanted: bool,
}

/// The largest candidate angle searched, in degrees.
const MAX_DEG: u32 = 20;

/// `round(4096 * tan(degrees))` for `degrees` in `0..=20`, so every shear is
/// an integer multiply-and-shift against a compile-time constant rather than
/// a runtime trig call.
const TAN_Q12: [i64; (MAX_DEG + 1) as usize] = [
    0, 71, 143, 215, 286, 358, 431, 503, 576, 649, 722, 796, 871, 946, 1021, 1098, 1175, 1252,
    1331, 1410, 1491,
];

/// Divides `n` by `d` and rounds to the nearest integer, ties away from zero.
/// The only division this module performs, so every target rounds shears the
/// same way.
fn round_div(n: i64, d: i64) -> i64 {
    if d == 0 {
        return 0;
    }
    let neg = (n < 0) != (d < 0);
    let n = n.unsigned_abs() as i64;
    let d = d.unsigned_abs() as i64;
    let q = (n + d / 2) / d;
    if neg {
        -q
    } else {
        q
    }
}

/// The sum-of-squared-column-ink score at every candidate angle, `0..=20`
/// degrees, index by degree. Split out from [`estimate`] so a diagnostic can
/// print the whole curve rather than just the winner -- the shape of the
/// curve, not only its peak, is what tells a reader whether the estimator
/// found a real maximum or is reporting a near-flat tie-break default.
///
/// Returns all zeros on a degenerate box, matching [`estimate`]'s guard.
pub fn angle_scores(
    page_labels: &[u32],
    page_width: u32,
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
    member_labels: &[u32],
    baseline: f32,
) -> [f64; (MAX_DEG + 1) as usize] {
    let mut scores = [0f64; (MAX_DEG + 1) as usize];
    if x1 <= x0 || y1 <= y0 || page_width == 0 || page_labels.is_empty() {
        return scores;
    }
    let baseline_round = baseline.round() as i64;
    let width = i64::from(x1 - x0);
    let dy0 = (i64::from(y0) - baseline_round).unsigned_abs() as i64;
    let dy1 = (i64::from(y1) - baseline_round).unsigned_abs() as i64;
    let max_dy = dy0.max(dy1);
    // The widest a row's shift can be at the steepest candidate angle, plus
    // one for rounding: the column array is padded by this much on each side
    // so a shifted column is never dropped for landing outside it.
    let buffer = round_div(max_dy * TAN_Q12[MAX_DEG as usize], 4096).unsigned_abs() as i64 + 1;
    let col_width = usize::try_from(width + 2 * buffer).unwrap_or(1).max(1);

    for angle in 0..=MAX_DEG {
        let tan_q12 = TAN_Q12[angle as usize];
        let mut cols = vec![0u32; col_width];
        for y in y0..y1 {
            let dy = i64::from(y) - baseline_round;
            let shift = round_div(dy * tan_q12, 4096);
            let row_start = y as usize * page_width as usize;
            for x in x0..x1 {
                let idx = row_start + x as usize;
                let Some(&lab) = page_labels.get(idx) else { continue };
                if lab == 0 || member_labels.binary_search(&lab).is_err() {
                    continue;
                }
                let col = (i64::from(x) - i64::from(x0)) + shift + buffer;
                if col >= 0 {
                    if let Some(c) = cols.get_mut(col as usize) {
                        *c += 1;
                    }
                }
            }
        }
        scores[angle as usize] = cols.iter().map(|&c| f64::from(c) * f64::from(c)).sum();
    }
    scores
}

/// Estimates one word's slant from its own ink.
///
/// `page_labels` is the page's component-label raster (`page_width` wide,
/// row-major, `0` for background); `member_labels` is the sorted, deduplicated
/// set of labels that belong to this word, so a pixel from a neighbouring
/// word or rule inside the same bounding box is not counted. `(x0, y0)..(x1,
/// y1)` is the word's own bounding box and `baseline` the line's baseline in
/// the same page coordinates.
///
/// Returns `angle_deg: 0, slanted: false` on a degenerate box (`x1 <= x0` or
/// `y1 <= y0`) rather than searching nothing.
pub fn estimate(
    page_labels: &[u32],
    page_width: u32,
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
    member_labels: &[u32],
    baseline: f32,
    p: &Params,
) -> Slant {
    let scores = angle_scores(page_labels, page_width, x0, x1, y0, y1, member_labels, baseline);
    let score0 = scores[0];
    let mut best_angle = 0u32;
    let mut best_score = 0f64;
    for (angle, &score) in scores.iter().enumerate() {
        // Strict `>` only: a tie keeps the smaller angle already found.
        if score > best_score {
            best_score = score;
            best_angle = angle as u32;
        }
    }

    // A winner at the search boundary is not a confirmed peak: the curve may
    // still have been rising when the search stopped, and this method has no
    // way to tell a real ~20 degree slant from a steeper diagonal mark (a
    // dimension leader, an arrowhead, cross-hatching) that a word's bounding
    // box happened to sweep in as a member component. Every genuine italic
    // peak measured against real italic ink (2026-09-24, `filing__r000583`)
    // landed at 18 degrees or below with a clear decline past it, so this
    // costs no real detections; it was added after bench/pages-cov's drawing
    // pages showed boundary-angle false positives that an interior peak
    // never produced (`docs/measurements/2026-09-24_italic_gating.txt`).
    let confirmed_peak = best_angle < MAX_DEG;
    let slanted = score0 > 0.0
        && confirmed_peak
        && best_angle as f32 >= p.slant_min_deg
        && best_score >= f64::from(p.slant_margin) * score0;
    Slant { angle_deg: best_angle, slanted }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: Params = Params { slant_min_deg: 6.0, slant_margin: 1.15 };

    /// Paints a filled rectangle of one label into a page-labels raster.
    fn rect(labels: &mut [u32], width: u32, x0: u32, y0: u32, x1: u32, y1: u32, lab: u32) {
        for y in y0..y1 {
            for x in x0..x1 {
                labels[(y * width + x) as usize] = lab;
            }
        }
    }

    /// An upright "H": two vertical strokes joined by a horizontal bar, none
    /// of it slanted. Shearing at any nonzero angle only fragments the two
    /// strokes into more, thinner columns per row-band, so 0 degrees is the
    /// actual maximum, not merely the tie-break default.
    #[test]
    fn an_upright_word_reads_zero_degrees() {
        let (w, h) = (30u32, 20u32);
        let mut labels = vec![0u32; (w * h) as usize];
        rect(&mut labels, w, 5, 2, 8, 18, 1);
        rect(&mut labels, w, 18, 2, 21, 18, 1);
        rect(&mut labels, w, 5, 9, 21, 12, 1);
        let r = estimate(&labels, w, 5, 21, 2, 18, &[1], 18.0, &P);
        assert_eq!(r.angle_deg, 0);
        assert!(!r.slanted);
    }

    /// A word sheared by exactly 12 degrees the way ordinary (right-leaning)
    /// italic actually leans -- ink above the baseline shifted right, using
    /// this module's own Q12 table so the test is self-consistent about what
    /// "12 degrees" means in fixed point -- is detected within +/-2 degrees
    /// and called slanted.
    #[test]
    fn a_twelve_degree_word_is_detected_within_two_degrees() {
        let (w, h) = (60u32, 30u32);
        let mut labels = vec![0u32; (w * h) as usize];
        let baseline = 25i64;
        let tan12 = TAN_Q12[12];
        // A single diagonal stroke: at true 12 degrees this is one ink column
        // per row after shearing back, so its score there strictly dominates
        // every other candidate angle. `x = 30 - shift` leans right going up
        // (`shift` is negative above the baseline), the same direction real
        // italic does; [`estimate`]'s `+ shift` undoes exactly this.
        for y in 0..h {
            let dy = i64::from(y) - baseline;
            let shift = round_div(dy * tan12, 4096);
            let x = 30i64 - shift;
            if (0..w as i64).contains(&x) {
                labels[(y as i64 * w as i64 + x) as usize] = 1;
            }
        }
        let r = estimate(&labels, w, 0, w, 0, h, &[1], baseline as f32, &P);
        assert!((r.angle_deg as i32 - 12).abs() <= 2, "got {}", r.angle_deg);
        assert!(r.slanted);
    }

    /// A vertical rule (a table-cell border, a ruled line) must never be
    /// flagged: every angle translates a single-column mark without changing
    /// its score, so the smaller-angle tie-break lands on 0 with no special
    /// case needed.
    #[test]
    fn a_vertical_rule_is_not_flagged() {
        let (w, h) = (20u32, 40u32);
        let mut labels = vec![0u32; (w * h) as usize];
        rect(&mut labels, w, 9, 0, 11, 40, 1);
        let r = estimate(&labels, w, 9, 11, 0, 40, &[1], 20.0, &P);
        assert_eq!(r.angle_deg, 0);
        assert!(!r.slanted);
    }

    /// A degenerate box is read as upright rather than searched.
    #[test]
    fn a_degenerate_box_is_upright() {
        let labels = vec![0u32; 100];
        let r = estimate(&labels, 10, 5, 5, 0, 10, &[1], 5.0, &P);
        assert_eq!(r, Slant { angle_deg: 0, slanted: false });
    }
}

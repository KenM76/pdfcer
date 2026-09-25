//! Matching a glyph's feature vector against the prototype bank.
//!
//! # Contract
//!
//! [`nearest`] takes a raw feature vector and returns the best class per
//! class, ascending by distance, truncated to `k`, together with the `d1`
//! and `d2` that [`crate::confidence`] turns into a margin
//! (`ARCHITECTURE.md` section 4.1 and 4.2).
//!
//! Coarse to fine, as section 4.1 sets out: prune by hole count — an exact
//! integer match, measured to cost 0.02 accuracy points — then weighted L2
//! against whatever survives. The aspect-band and baseline-class prune of
//! step 2 is **not enabled**; both forms were measured and both lost
//! accuracy.
//!
//! Deterministic: `f64` accumulation, prototypes visited in file order,
//! ties broken to the lower class index.

use crate::feature::{holes_of, FEATURE_DIMS};
use crate::ocrw::Model;

/// One class's best showing against a query.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    pub class: u16,
    /// Row index into [`Model::prototypes`] of the prototype that produced
    /// `distance`, so a caller can ask which face and size won.
    pub prototype: usize,
    /// Weighted Euclidean distance in standardised feature space.
    pub distance: f32,
}

/// What the matcher says about one glyph hypothesis.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    /// Best candidate per class, ascending by distance, at most `k` long.
    /// No class appears twice.
    pub best: Vec<Candidate>,
    /// Distance to the best-matching prototype.
    pub d1: f32,
    /// Distance to the best prototype of a *different* class, or infinity
    /// when the gate left only one class standing. Infinity is the honest
    /// answer there: nothing competed, so the margin is unbounded.
    pub d2: f32,
    /// Whether the hole-count gate was applied. False when no class in the
    /// bank was ever measured with the query's hole count, in which case the
    /// gate was dropped rather than allowed to return nothing.
    pub gated: bool,
}

impl Match {
    pub fn top(&self) -> Option<Candidate> {
        self.best.first().copied()
    }
    /// `d1 / d2`, the ratio section 4.2 calibrates. Zero when `d2` is
    /// infinite, which reads as a perfect margin.
    pub fn ratio(&self) -> f32 {
        if self.d2.is_infinite() {
            0.0
        } else if self.d2 > 0.0 {
            self.d1 / self.d2
        } else {
            // Two different classes at distance zero: the query is exactly on
            // top of both, so the margin is nil, not perfect.
            1.0
        }
    }
}

/// Matches a raw feature vector against the bank.
///
/// `raw` is the extractor's output, unstandardised: standardising here rather
/// than at the call site is what guarantees the query is measured with the
/// file's own constants and not with a compiled-in copy of them.
///
/// `italic_ok` is the caller's word-level slant verdict — pass `true` for a
/// word measured as slanted, or when there is no word context at all (a
/// diagnostic scanning isolated glyphs, say). It only has an effect when the
/// model's `layout.italic_gating` is on (`ARCHITECTURE.md` section 11,
/// 2026-09-24 decision): gating off means every prototype is always
/// eligible, exactly as before this parameter existed, so an upright page
/// gated off pays nothing for it beyond the one branch below.
///
/// `None` when `k == 0`, when the bank is empty, or when the query is not
/// finite — a non-finite query would poison every comparison and is a bug
/// upstream, not a glyph.
pub fn nearest(model: &Model, raw: &[f32; FEATURE_DIMS], k: usize, italic_ok: bool) -> Option<Match> {
    if k == 0 || model.n_prototypes() == 0 || model.classes.is_empty() {
        return None;
    }
    if raw.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let q = model.standardise(raw);
    if q.iter().any(|v| !v.is_finite()) {
        return None;
    }

    // Step 1: the hole-count gate. A class passes when the bank ever saw a
    // prototype of it with this many holes.
    let holes = holes_of(raw);
    let bit = 1u8 << holes;
    let mut allowed: Vec<bool> =
        model.class_holes.iter().map(|m| m & bit != 0).collect();
    allowed.resize(model.classes.len(), false);
    let mut gated = allowed.iter().any(|&a| a);
    if !gated {
        // The gate is an optimisation; it must never be the reason a glyph
        // goes unread. A hole count no class in the bank ever showed means
        // the bank cannot speak to this query, so ask all of it.
        allowed.iter_mut().for_each(|a| *a = true);
        gated = false;
    }

    let w: Vec<f64> = model.weights.iter().map(|&v| f64::from(v)).collect();
    let qd: Vec<f64> = q.iter().map(|&v| f64::from(v)).collect();

    let mut best_d = vec![f64::INFINITY; model.classes.len()];
    let mut best_p = vec![usize::MAX; model.classes.len()];

    // Skipping happens before any distance is computed, so an upright page
    // gets back the control wall time exactly (`ARCHITECTURE.md` section 11).
    let skip_italic = model.params.layout.italic_gating != 0 && !italic_ok;

    // A second, cross-class ceiling alongside the per-class one below. The
    // per-class ceiling only tightens within one class's own prototypes; with
    // a bank of hundreds of classes and `k` far smaller, most classes never
    // enter the reported top-k, and starting their ceiling at infinity gives
    // them no early pressure at all. `m` is the number of finite per-class
    // bests the answer needs to be exact about: `k` for `best`, at least 2 so
    // `d2` (the best *other* class) is always exact too.
    //
    // `global_ceiling` is the current m-th smallest finite value in `best_d`.
    // `best_d` entries only ever fall, so this is non-increasing over the
    // scan and is always >= its own final value. A prototype's partial sum
    // can never exceed its true (fully summed) distance, since every
    // per-dimension term is non-negative; so a prototype whose true distance
    // is <= the *final* global ceiling can never be wrongly cut by a
    // snapshot of it taken earlier, when the snapshot can only be looser.
    // That is what makes pruning against it exact for every class that ends
    // up in the top-m, which is the only thing the caller ever reads
    // (`docs/measurements/2026-09-24_dense_page_speed.md` has the full
    // argument). The per-class acceptance test below is untouched by this —
    // it still records a class's true best whenever one is found, top-m or
    // not, so this is pruning only, never a change to what gets recorded.
    let m = k.max(2);
    let mut global_ceiling = f64::INFINITY;
    let mut finite_count = 0usize;
    let mut scratch: Vec<f64> = Vec::with_capacity(model.classes.len());

    // Profiling only: `prof_on` is read once per call, not once per
    // prototype, so the hot loop below pays one bool check either way.
    let prof_on = crate::prof::enabled();

    for p in 0..model.n_prototypes() {
        let class = model.prototype_class[p] as usize;
        if !allowed[class] {
            continue;
        }
        if skip_italic && model.prototype_italic.get(p).copied().unwrap_or(false) {
            continue;
        }
        if prof_on {
            crate::prof::add(&crate::prof::COUNTERS.prototypes_visited, 1);
        }
        let ceiling = best_d[class];
        let checkpoint_ceiling = ceiling.min(global_ceiling);
        let row = &model.prototypes[p * FEATURE_DIMS..(p + 1) * FEATURE_DIMS];
        // Early abandon against *this class's* current best, or the current
        // cross-class one, whichever is tighter. The final acceptance test
        // below stays against `ceiling` alone (see the comment above): this
        // only decides how early a losing prototype can stop being summed.
        // Checked every 16 dimensions so the branch costs little.
        let mut acc = 0.0f64;
        let mut abandoned = false;
        for (i, &r) in row.iter().enumerate() {
            let d = qd[i] - f64::from(r);
            acc += w[i] * d * d;
            if i % 16 == 15 && acc >= checkpoint_ceiling {
                abandoned = true;
                break;
            }
        }
        if abandoned && prof_on {
            crate::prof::add(&crate::prof::COUNTERS.prototypes_abandoned, 1);
        }
        if !abandoned && acc < ceiling {
            let was_finite = best_d[class].is_finite();
            best_d[class] = acc;
            best_p[class] = p;
            if !was_finite {
                finite_count += 1;
            }
            if finite_count >= m {
                scratch.clear();
                scratch.extend(best_d.iter().copied().filter(|d| d.is_finite()));
                let idx = m - 1;
                if scratch.len() > idx {
                    scratch.select_nth_unstable_by(idx, |a, b| a.partial_cmp(b).unwrap());
                    global_ceiling = scratch[idx];
                }
            }
        }
    }

    let mut cands: Vec<Candidate> = best_d
        .iter()
        .enumerate()
        .filter(|(_, d)| d.is_finite())
        .map(|(c, &d)| Candidate {
            class: c as u16,
            prototype: best_p[c],
            distance: d.sqrt() as f32,
        })
        .collect();
    if cands.is_empty() {
        return None;
    }
    // Ascending by distance; a tie goes to the lower class index, which is
    // the rule `ARCHITECTURE.md` section 8.2 states so that a fixture passes
    // identically on x86 and wasm32.
    cands.sort_by(|a, b| {
        a.distance.partial_cmp(&b.distance).expect("distances are finite").then(a.class.cmp(&b.class))
    });

    let d1 = cands[0].distance;
    let d2 = cands.get(1).map_or(f32::INFINITY, |c| c.distance);
    cands.truncate(k);
    Some(Match { best: cands, d1, d2, gated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::{holes_of, HOLE_COUNT};
    use crate::ocrw::{Class, Model};

    /// A bank whose prototypes are axis-aligned points, so every distance is
    /// arithmetic a reader can check by hand.
    fn bank(rows: &[(u16, f32, u8)]) -> Model {
        let n_classes = rows.iter().map(|r| r.0).max().unwrap() as usize + 1;
        let mut prototypes = vec![0.0f32; rows.len() * FEATURE_DIMS];
        let mut prototype_class = Vec::new();
        let mut class_holes = vec![0u8; n_classes];
        for (p, &(class, v, holes)) in rows.iter().enumerate() {
            prototypes[p * FEATURE_DIMS] = v;
            prototypes[p * FEATURE_DIMS + HOLE_COUNT] = f32::from(holes);
            prototype_class.push(class);
            class_holes[class as usize] |= 1 << holes;
        }
        Model {
            build_id: "test".into(),
            feature_version: crate::feature::FEATURE_VERSION,
            classes: (0..n_classes)
                .map(|i| Class {
                    index: i as u16,
                    codepoint: char::from_u32('a' as u32 + i as u32).unwrap(),
                    category: "letter".into(),
                    case_twin: None,
                })
                .collect(),
            faces: Vec::new(),
            sizes: Vec::new(),
            prototypes,
            prototype_class,
            prototype_italic: Vec::new(),
            mean: [0.0; FEATURE_DIMS],
            sd: [1.0; FEATURE_DIMS],
            class_holes,
            weights: [1.0; FEATURE_DIMS],
            class_info: Vec::new(),
            params: crate::params::Params::DEFAULT,
            lexicon: None,
            bigrams: None,
            confusions: None,
        }
    }

    fn query(v: f32, holes: u8) -> [f32; FEATURE_DIMS] {
        let mut q = [0.0f32; FEATURE_DIMS];
        q[0] = v;
        q[HOLE_COUNT] = f32::from(holes);
        q
    }

    #[test]
    fn the_nearest_prototype_wins_and_its_class_is_reported_once() {
        let m = bank(&[(0, 0.0, 0), (0, 0.4, 0), (1, 1.0, 0), (2, 5.0, 0)]);
        let r = nearest(&m, &query(0.3, 0), 3, true).unwrap();
        assert_eq!(r.best.len(), 3);
        assert_eq!(r.top().unwrap().class, 0);
        assert_eq!(r.top().unwrap().prototype, 1, "the closer of class 0's two");
        // Classes appear once each, in distance order.
        assert_eq!(r.best.iter().map(|c| c.class).collect::<Vec<_>>(), vec![0, 1, 2]);
    }

    /// `d2` is the best of a *different* class, which is the whole point:
    /// two prototypes of the same class agreeing says nothing about margin.
    #[test]
    fn d2_skips_the_winner_s_own_class() {
        let m = bank(&[(0, 0.0, 0), (0, 0.01, 0), (1, 2.0, 0)]);
        let r = nearest(&m, &query(0.0, 0), 3, true).unwrap();
        assert!((r.d1 - 0.0).abs() < 1e-6);
        assert!((r.d2 - 2.0).abs() < 1e-5, "d2 was {}", r.d2);
    }

    /// The gate is an exact integer match on hole count.
    #[test]
    fn the_hole_gate_removes_classes_with_the_wrong_count() {
        let m = bank(&[(0, 0.0, 1), (1, 0.1, 0), (2, 9.0, 1)]);
        let r = nearest(&m, &query(0.0, 1), 5, true).unwrap();
        assert!(r.gated);
        assert_eq!(r.best.iter().map(|c| c.class).collect::<Vec<_>>(), vec![0, 2]);
    }

    /// A hole count nothing in the bank ever showed must not leave the glyph
    /// unread: the gate drops rather than returning nothing.
    #[test]
    fn an_unseen_hole_count_drops_the_gate_rather_than_matching_nothing() {
        let m = bank(&[(0, 0.0, 0), (1, 1.0, 0)]);
        let r = nearest(&m, &query(0.0, 2), 5, true).unwrap();
        assert!(!r.gated);
        assert_eq!(r.best.len(), 2);
    }

    /// Early abandonment is an optimisation and must not change the answer.
    #[test]
    fn early_abandonment_gives_the_same_answer_as_a_full_scan() {
        let rows: Vec<(u16, f32, u8)> =
            (0..40u16).map(|i| (i % 7, f32::from(i) * 0.37, 0)).collect();
        let m = bank(&rows);
        for step in 0..20 {
            let q = query(step as f32 * 0.5, 0);
            let r = nearest(&m, &q, 7, true).unwrap();
            // Recompute the winner the slow, obvious way.
            let mut brute: Vec<(f64, u16)> = Vec::new();
            for (p, &(class, v, _)) in rows.iter().enumerate() {
                let _ = p;
                let d = f64::from(q[0] - v) * f64::from(q[0] - v);
                brute.push((d, class));
            }
            brute.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
            assert_eq!(r.top().unwrap().class, brute[0].1, "step {step}");
        }
    }

    /// The cross-class ceiling only activates once `m = max(k, 2)` classes
    /// have a finite best, which needs more classes than `k` to exercise —
    /// unlike the test above, this bank has far more classes than `top_k`,
    /// so most classes never enter `best` and are exactly where the new
    /// ceiling is supposed to prune harder than the per-class one alone.
    #[test]
    fn the_cross_class_ceiling_gives_the_same_answer_as_a_full_scan() {
        let rows: Vec<(u16, f32, u8)> =
            (0..400u16).map(|i| (i, f32::from(i % 97) * 0.13, 0)).collect();
        let m = bank(&rows);
        for step in 0..25 {
            let q = query(step as f32 * 0.7, 0);
            let r = nearest(&m, &q, 3, true).unwrap();
            let mut brute: Vec<(f64, u16)> = rows
                .iter()
                .map(|&(class, v, _)| (f64::from(q[0] - v) * f64::from(q[0] - v), class))
                .collect();
            brute.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
            // One entry per class, ascending, matching the brute force's
            // per-class best in the same order `nearest` would report it.
            let mut brute_best: Vec<(u16, f64)> = Vec::new();
            for &(d, class) in &brute {
                if !brute_best.iter().any(|&(c, _)| c == class) {
                    brute_best.push((class, d));
                }
            }
            brute_best.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap().then(a.0.cmp(&b.0)));

            assert!(
                (f64::from(r.d1) - brute_best[0].1.sqrt()).abs() < 1e-9,
                "step {step} d1: got {} want {}",
                r.d1,
                brute_best[0].1.sqrt()
            );
            assert!(
                (f64::from(r.d2) - brute_best[1].1.sqrt()).abs() < 1e-9,
                "step {step} d2: got {} want {}",
                r.d2,
                brute_best[1].1.sqrt()
            );
            let got: Vec<u16> = r.best.iter().map(|c| c.class).collect();
            let want: Vec<u16> = brute_best.iter().take(3).map(|&(c, _)| c).collect();
            assert_eq!(got, want, "step {step}");
        }
    }

    /// Per-dimension weights scale the squared term, so a weight of zero
    /// blinds the matcher to that dimension entirely.
    #[test]
    fn weights_scale_the_dimensions_they_name() {
        let mut m = bank(&[(0, 0.0, 0), (1, 1.0, 0)]);
        let plain = nearest(&m, &query(0.9, 0), 2, true).unwrap();
        assert_eq!(plain.top().unwrap().class, 1);
        m.weights[0] = 0.0;
        let blind = nearest(&m, &query(0.9, 0), 2, true).unwrap();
        assert_eq!(blind.top().unwrap().class, 0, "with dimension 0 ignored, the tie goes low");
        assert_eq!(blind.d1, 0.0);
    }

    #[test]
    fn a_non_finite_query_is_refused_rather_than_matched() {
        let m = bank(&[(0, 0.0, 0)]);
        let mut q = query(0.0, 0);
        q[3] = f32::NAN;
        assert!(nearest(&m, &q, 1, true).is_none());
    }

    #[test]
    fn the_ratio_is_zero_when_nothing_competed() {
        let m = bank(&[(0, 0.0, 0)]);
        let r = nearest(&m, &query(0.0, 0), 1, true).unwrap();
        assert!(r.d2.is_infinite());
        assert_eq!(r.ratio(), 0.0);
    }

    #[test]
    fn holes_of_reads_the_dimension_the_extractor_writes() {
        assert_eq!(holes_of(&query(0.0, 2)), 2);
        let mut q = query(0.0, 0);
        q[HOLE_COUNT] = 7.0;
        assert_eq!(holes_of(&q), 2, "the count clamps where the extractor clamps");
    }
}

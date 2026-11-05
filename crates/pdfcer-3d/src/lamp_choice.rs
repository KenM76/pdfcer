//! Which of a frame's lamps cast a shadow: the [`MAX_LAMP_SHADOWS`] adding
//! most at the camera target, chosen the same way on both paths, with a
//! margin so a lamp already casting keeps its slot while it stays close to
//! the top.

use crate::MAX_LAMP_SHADOWS;
use crate::model_shade::Local;
use crate::vec3::{length, sub};

/// How far past [`MAX_LAMP_SHADOWS`] a lamp already casting may fall in the
/// ranking and keep casting, so two lamps of nearly equal score do not swap
/// shadows (and the host redraw cube faces) on every small camera move.
pub(crate) const MARGIN: usize = 4;

/// Each shadow slot's lamp, by its index among the model's point and spot
/// lights.
pub(crate) type Slots = [Option<usize>; MAX_LAMP_SHADOWS];

/// No lamp casting: the frame before a fresh choice.
pub(crate) const NO_SLOTS: Slots = [None; MAX_LAMP_SHADOWS];

/// Lamp `l`'s claim to a shadow seen towards `target` from `eye`: its
/// luminance (Rec. 709 weights over colour × intensity × gain) over its
/// squared distance to the target, that distance held to at least 1 % of
/// the eye's, so a lamp at the target does not score infinity. 0 for NaN.
pub(crate) fn score(l: &Local, target: [f64; 3], eye: [f64; 3]) -> f64 {
    let [r, g, b] = l.colour;
    let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let floor = 0.01 * length(sub(target, eye));
    let d = length(sub(l.position, target)).max(floor);
    let s = luminance / (d * d).max(f64::MIN_POSITIVE);
    if s.is_nan() { 0.0 } else { s }
}

/// The slots for `candidates` (model index, [`score`]), given the frame
/// before's `previous`. A lamp in `previous` keeps its slot while it ranks
/// within the top [`MAX_LAMP_SHADOWS`] + [`MARGIN`]; the free slots then go,
/// lowest first, to the best-ranked candidates not yet casting. Ranking is
/// by score, ties to the lower model index. With an empty `previous` this
/// is the top [`MAX_LAMP_SHADOWS`] in rank order.
pub(crate) fn choose(candidates: &[(usize, f64)], previous: &Slots) -> Slots {
    let mut ranked = candidates.to_vec();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let near_top: Vec<usize> = ranked
        .iter()
        .take(MAX_LAMP_SHADOWS + MARGIN)
        .map(|c| c.0)
        .collect();
    let mut slots: Slots = previous.map(|m| m.filter(|m| near_top.contains(m)));
    let free: Vec<usize> = (slots.iter().enumerate())
        .filter(|(_, m)| m.is_none())
        .map(|(s, _)| s)
        .collect();
    let mut free = free.into_iter();
    for &(m, _) in &ranked {
        if slots.contains(&Some(m)) {
            continue;
        }
        let Some(s) = free.next() else { break };
        if let Some(slot) = slots.get_mut(s) {
            *slot = Some(m);
        }
    }
    slots
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // Tests fail loudly by design.
mod tests {
    use super::*;

    /// Candidates 0..n scoring n, n - 1, …: model index = rank.
    fn ranked(n: usize) -> Vec<(usize, f64)> {
        (0..n).map(|m| (m, (n - m) as f64)).collect()
    }

    #[test]
    fn a_fresh_choice_is_the_top_sixteen_in_rank_order() {
        let got = choose(&ranked(20), &[None; MAX_LAMP_SHADOWS]);
        let want: Vec<Option<usize>> = (0..MAX_LAMP_SHADOWS).map(Some).collect();
        assert_eq!(got.to_vec(), want);
        // Fewer candidates than slots: all of them, the rest empty.
        let few = choose(&[(9, 1.0), (4, 3.0)], &[None; MAX_LAMP_SHADOWS]);
        assert_eq!(few[..3], [Some(4), Some(9), None]);
    }

    #[test]
    fn ties_go_to_model_order() {
        let tied: Vec<(usize, f64)> = (0..20).rev().map(|m| (m, 1.0)).collect();
        let got = choose(&tied, &[None; MAX_LAMP_SHADOWS]);
        let want: Vec<Option<usize>> = (0..MAX_LAMP_SHADOWS).map(Some).collect();
        assert_eq!(got.to_vec(), want);
    }

    #[test]
    fn a_casting_lamp_keeps_its_slot_within_the_margin() {
        // Lamp 18 (19th) cast in slot 5 last frame: it keeps slot 5, and
        // the 16th-ranked lamp (15) goes without.
        let mut previous = [None; MAX_LAMP_SHADOWS];
        previous[5] = Some(18);
        let got = choose(&ranked(30), &previous);
        assert_eq!(got[5], Some(18));
        assert!(!got.contains(&Some(15)) && got.contains(&Some(14)));
        assert_eq!(got.iter().flatten().count(), MAX_LAMP_SHADOWS);
        // Lamp 20 (21st) is past the margin: it loses its slot to lamp 0.
        previous[5] = Some(MAX_LAMP_SHADOWS + MARGIN);
        let got = choose(&ranked(30), &previous);
        assert_eq!(got[5], Some(5));
        assert!(!got.contains(&Some(MAX_LAMP_SHADOWS + MARGIN)));
    }

    #[test]
    fn kept_lamps_stay_put_and_joiners_take_the_lowest_free_slots() {
        let mut previous = [None; MAX_LAMP_SHADOWS];
        previous[0] = Some(7);
        previous[3] = Some(2);
        previous[9] = Some(99); // Gone from the frame: its slot frees.
        let got = choose(&ranked(4), &previous);
        // Lamp 2 keeps slot 3; lamps 7 and 99 are gone, so 0, 1 and 3
        // take slots 0, 1 and 2.
        assert_eq!(got[..5], [Some(0), Some(1), Some(3), Some(2), None]);
    }

    #[test]
    fn the_score_is_luminance_over_distance_squared_with_a_floor() {
        let lamp = |position, colour| Local {
            position,
            colour,
            attenuation: [1.0, 0.0, 0.0],
            cone: None,
            reach: f64::INFINITY,
            model: 0,
            slot: None,
        };
        let eye = [0.0, 0.0, 100.0];
        let white = score(&lamp([0.0, 0.0, 2.0], [1.0; 3]), [0.0; 3], eye);
        assert!((white - 0.25).abs() < 1e-12);
        let green = score(&lamp([0.0, 0.0, 2.0], [0.0, 1.0, 0.0]), [0.0; 3], eye);
        assert!((green - 0.7152 / 4.0).abs() < 1e-12);
        // At the target the distance is held to 1 % of the eye's, 1.
        let at = score(&lamp([0.0; 3], [1.0; 3]), [0.0; 3], eye);
        assert!((at - 1.0).abs() < 1e-12);
        assert_eq!(score(&lamp([0.0; 3], [f64::NAN; 3]), [0.0; 3], eye), 0.0);
    }
}

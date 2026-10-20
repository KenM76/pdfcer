//! The automatic word-gap threshold ([`ExtractOptions::word_gap_auto`]).
//!
//! Letter gaps and word gaps form two clusters on almost every page. The
//! threshold is the midpoint of the widest empty interval between
//! consecutive measured gap ratios whose midpoint lies in
//! [`SEARCH`]; a page without such a valley keeps the fixed ratio. No spec
//! basis (S3/S4), like the fixed ratio it replaces.

use super::ExtractOptions;
use super::page::Item;

/// Where a word-gap threshold may fall, in ems. Below it a kerning pair
/// would split words; above it no real word space reaches.
const SEARCH: (f32, f32) = (0.06, 0.60);
/// Gap ratios outside this band are line changes or overlaps, not
/// letter or word gaps.
const BAND: (f32, f32) = (-0.10, 1.50);
/// The narrowest empty interval that counts as a valley, in ems.
const MIN_VALLEY: f32 = 0.05;
/// Fewest measured pairs, and fewest pairs on each side of the valley,
/// before the page is trusted to have two clusters.
const MIN_PAIRS: usize = 12;
const MIN_SIDE: usize = 3;

/// The options `assemble` should use, and whether the word ratio in them
/// was measured from this page (`false`: the fixed ratio, either because
/// auto is off or because the page had no valley).
pub(super) fn resolve(items: &[Item], options: &ExtractOptions) -> (ExtractOptions, bool) {
    let mut used = options.clone();
    if !options.word_gap_auto {
        return (used, false);
    }
    match measure(&gap_ratios(items, options)) {
        Some(r) => {
            used.word_gap_ratio = r;
            (used, true)
        }
        None => (used, false),
    }
}

/// Every same-line, same-direction gap between consecutive glyphs, as a
/// fraction of the larger effective size.
fn gap_ratios(items: &[Item], options: &ExtractOptions) -> Vec<f32> {
    let mut out = Vec::new();
    let mut prev: Option<(f32, f32, f32, (f32, f32))> = None;
    for item in items {
        let Item::Glyph(g) = item else {
            prev = None;
            continue;
        };
        let dir = g.direction;
        if let Some((ex, ey, size, pdir)) = prev {
            let size = size.max(g.size);
            let same_dir = pdir.0 * dir.0 + pdir.1 * dir.1 >= options.same_direction_cos;
            let (dx, dy) = (g.x - ex, g.y - ey);
            let perp = dx * pdir.1 - dy * pdir.0;
            let r = (dx * pdir.0 + dy * pdir.1) / size;
            if same_dir
                && size.is_finite()
                && size > 1e-6
                && perp.abs() <= options.line_gap_ratio * size
                && r > BAND.0
                && r < BAND.1
            {
                out.push(r);
            }
        }
        prev = Some((
            g.x + g.advance * dir.0,
            g.y + g.advance * dir.1,
            g.size,
            dir,
        ));
    }
    out
}

/// The valley midpoint, or `None` for a page without two clusters.
fn measure(ratios: &[f32]) -> Option<f32> {
    if ratios.len() < MIN_PAIRS {
        return None;
    }
    let mut r: Vec<f32> = ratios.iter().copied().filter(|v| v.is_finite()).collect();
    r.sort_by(f32::total_cmp);
    let mut best: Option<(f32, f32)> = None;
    for (i, (&lo, &hi)) in r.iter().zip(r.iter().skip(1)).enumerate() {
        let mid = (lo + hi) / 2.0;
        let width = hi - lo;
        let sides = (i + 1, r.len() - i - 1);
        if width >= MIN_VALLEY
            && (SEARCH.0..=SEARCH.1).contains(&mid)
            && sides.0 >= MIN_SIDE
            && sides.1 >= MIN_SIDE
            && best.is_none_or(|(bw, _)| width > bw)
        {
            best = Some((width, mid));
        }
    }
    best.map(|(_, mid)| mid)
}

#[cfg(test)]
#[allow(clippy::expect_used)] // a test panics on failure by design
mod tests {
    use super::measure;

    #[test]
    fn two_clusters_put_the_threshold_in_the_valley() {
        let mut r = vec![0.0, 0.01, -0.02, 0.03, 0.02, 0.0, 0.01, 0.04, 0.0, 0.02];
        r.extend([0.30, 0.28, 0.33, 0.31]);
        let t = measure(&r).expect("two clusters");
        assert!(t > 0.04 && t < 0.28, "{t}");
    }

    #[test]
    fn one_cluster_or_too_few_pairs_measures_nothing() {
        assert_eq!(measure(&[0.0; 40]), None);
        assert_eq!(measure(&[0.0, 0.3, 0.0, 0.3]), None);
    }
}

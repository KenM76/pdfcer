use super::*;
use crate::color::cmyk_to_srgb;

fn counts(v: [f32; 3]) -> [i32; 3] {
    v.map(|x| (x * 255.0).round() as i32)
}

fn close(a: [i32; 3], b: [i32; 3], within: i32) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= within)
}

#[test]
fn white_is_no_ink() {
    assert_eq!(
        srgb_to_cmyk_fast(CmykIntent::Calibrated, 1.0, 1.0, 1.0),
        [0.0; 4]
    );
}

#[test]
fn in_gamut_colours_round_trip_within_three_counts() {
    let mut worst = 0;
    for r in [0.25_f32, 0.4, 0.55, 0.7, 0.85] {
        for g in [0.3_f32, 0.45, 0.6, 0.75] {
            for b in [0.2_f32, 0.35, 0.5, 0.65, 0.8] {
                let fit =
                    srgb_to_cmyk_with(CmykIntent::Calibrated, BlackGeneration::Medium, r, g, b);
                if !fit.in_gamut {
                    continue;
                }
                let [c, m, y, k] = srgb_to_cmyk_fast(CmykIntent::Calibrated, r, g, b);
                let back = counts(cmyk_to_srgb(c, m, y, k));
                let want = counts([r, g, b]);
                for (x, w) in back.iter().zip(want.iter()) {
                    worst = worst.max((x - w).abs());
                }
            }
        }
    }
    assert!(worst <= 3, "worst round-trip error {worst} counts");
}

#[test]
fn mid_grey_stays_neutral_and_carries_cmy() {
    let [c, m, y, k] = srgb_to_cmyk_fast(CmykIntent::Calibrated, 0.5, 0.5, 0.5);
    assert!(
        c > 0.3 && m > 0.3 && y > 0.3 && k < 0.2,
        "{:?}",
        [c, m, y, k]
    );
    let back = counts(cmyk_to_srgb(c, m, y, k));
    assert!(close(back, [128, 128, 128], 2), "{back:?}");
}

/// Request X005's rows: a `/Difference` with pure K complements C, M and Y
/// and keeps K. Acrobat's renderings are the reference; the second row is
/// outside the gamut and clipped differently, so only its R and B are held.
#[test]
fn difference_with_black_lands_near_the_reference() {
    let rows = [
        ([0.8_f32, 0.4, 0.2], [7, 139, 193], [true, true, true]),
        ([0.2, 0.6, 0.9], [186, 105, 49], [true, false, true]),
        ([0.5, 0.5, 0.5], [137, 112, 106], [true, true, true]),
    ];
    for (rgb, want, held) in rows {
        let [c, m, y, k] = srgb_to_cmyk_fast(CmykIntent::Calibrated, rgb[0], rgb[1], rgb[2]);
        let got = counts(cmyk_to_srgb(1.0 - c, 1.0 - m, 1.0 - y, k));
        for ((g, w), h) in got.iter().zip(want.iter()).zip(held) {
            assert!(!h || (g - w).abs() <= 8, "{rgb:?}: {got:?} vs {want:?}");
        }
    }
}

/// The baked grids are the solver's output: a stale grid fails here.
#[test]
fn baked_nodes_match_the_solver() {
    let at = |i: usize| i as f32 / (INVERSE_GRID_NODES - 1) as f32;
    for intent in [CmykIntent::Calibrated, CmykIntent::NeutralBlack] {
        for (r, g, b) in [
            (0, 0, 0),
            (16, 16, 16),
            (8, 8, 8),
            (13, 6, 3),
            (3, 10, 14),
            (16, 0, 9),
        ] {
            let want = srgb_to_cmyk_with(intent, BlackGeneration::Medium, at(r), at(g), at(b)).cmyk;
            let got = srgb_to_cmyk_fast(intent, at(r), at(g), at(b));
            for (x, w) in got.iter().zip(want.iter()) {
                assert!(
                    (x - w).abs() < 1e-4,
                    "{intent:?} node {:?}: {got:?} vs {want:?}",
                    (r, g, b)
                );
            }
        }
    }
}

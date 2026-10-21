use super::*;
use crate::color::cmyk_to_srgb;

fn round_trips(fit: &CmykFit, rgb: [f32; 3]) -> bool {
    let [c, m, y, k] = fit.cmyk;
    let back = cmyk_to_srgb(c, m, y, k);
    back.iter()
        .zip(rgb.iter())
        .all(|(a, b)| (a * 255.0).round() == (b * 255.0).round())
}

/// Every colour the table can draw comes back to the same bytes, under both
/// black choices: sampled by drawing CMYK on a grid, so each target is
/// known to be in gamut. The targets are not rounded to bytes first: an
/// 8-bit colour at a gamut corner can sit outside the gamut, and is then
/// rightly reported out of it.
#[test]
fn every_drawable_colour_round_trips_to_the_byte() {
    let steps = [0.0, 0.15, 0.4, 0.7, 1.0];
    let mut checked = 0;
    for &c in &steps {
        for &m in &steps {
            for &y in &steps {
                for &k in &[0.0, 0.3, 0.6] {
                    let rgb = cmyk_to_srgb(c, m, y, k);
                    for black in [BlackGeneration::Most, BlackGeneration::Least] {
                        let fit = srgb_to_cmyk_with(
                            CmykIntent::Calibrated,
                            black,
                            rgb[0],
                            rgb[1],
                            rgb[2],
                        );
                        assert!(fit.in_gamut, "{rgb:?} {black:?}: {fit:?}");
                        assert!(
                            fit.residual <= CMYK_FIT_TOLERANCE / 10.0,
                            "{rgb:?} {black:?}: {fit:?}"
                        );
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 750);
}

#[test]
fn most_black_uses_more_k_than_least() {
    let rgb = cmyk_to_srgb(0.3, 0.3, 0.3, 0.3);
    let most = srgb_to_cmyk_with(
        CmykIntent::Calibrated,
        BlackGeneration::Most,
        rgb[0],
        rgb[1],
        rgb[2],
    );
    let least = srgb_to_cmyk_with(
        CmykIntent::Calibrated,
        BlackGeneration::Least,
        rgb[0],
        rgb[1],
        rgb[2],
    );
    assert!(most.cmyk[3] > least.cmyk[3] + 0.1, "{most:?} {least:?}");
}

/// An 8-bit colour inside the gamut comes back as the same bytes: each
/// drawn from an interior CMYK, so its bytes are reachable.
#[test]
fn an_8_bit_colour_comes_back_as_its_bytes() {
    let inks = [
        [0.3, 0.5, 0.2, 0.1],
        [0.6, 0.1, 0.4, 0.2],
        [0.1, 0.7, 0.6, 0.0],
        [0.2, 0.2, 0.2, 0.5],
    ];
    for [c, m, y, k] in inks {
        let rgb = cmyk_to_srgb(c, m, y, k).map(|v| (v * 255.0).round() / 255.0);
        let fit = srgb_to_cmyk(rgb[0], rgb[1], rgb[2]);
        assert!(fit.in_gamut && round_trips(&fit, rgb), "{rgb:?}: {fit:?}");
    }
}

/// The GUI's report: RGB 0,160,255 through the §10.4.2 formula re-reads as
/// 0,129,200. The fit is out of gamut, says so, and lands nearer.
#[test]
fn an_unreachable_blue_is_flagged_and_closer_than_the_naive_formula() {
    let rgb = [0.0, 160.0 / 255.0, 1.0];
    let fit = srgb_to_cmyk(rgb[0], rgb[1], rgb[2]);
    let [c, m, y, k] = fit.cmyk;
    let back = cmyk_to_srgb(c, m, y, k);
    let naive = cmyk_to_srgb(1.0, 1.0 - 160.0 / 255.0, 0.0, 0.0);
    let dist = |a: [f32; 3]| sq_error(a, rgb);
    assert!(dist(back) < dist(naive), "{back:?} vs naive {naive:?}");
    assert_eq!(fit.in_gamut, fit.residual <= CMYK_FIT_TOLERANCE);
}

#[test]
fn white_is_no_ink_and_nan_reads_as_zero() {
    let fit = srgb_to_cmyk(1.0, 1.0, 1.0);
    assert!(fit.in_gamut);
    assert!(fit.cmyk.iter().all(|&v| v < 0.01), "{fit:?}");
    let nan = srgb_to_cmyk(f32::NAN, f32::NAN, f32::NAN);
    assert_eq!(nan.cmyk, srgb_to_cmyk(0.0, 0.0, 0.0).cmyk);
}

//! Blend functions `B(cb, cs)`: ISO 32000-2 §11.3.5 Tables 136 and 137, and
//! §11.3.4's complement for subtractive blending spaces.

use crate::{Blend, Colour, Space};

/// `B(cb, cs)` for one component of a separable mode (Table 136), in
/// additive terms.
fn separable(mode: Blend, cb: f32, cs: f32) -> f32 {
    match mode {
        Blend::Multiply => cb * cs,
        Blend::Screen => cb + cs - cb * cs,
        Blend::Overlay => separable(Blend::HardLight, cs, cb),
        Blend::Darken => cb.min(cs),
        Blend::Lighten => cb.max(cs),
        Blend::ColorDodge if cs < 1.0 => (cb / (1.0 - cs)).min(1.0),
        Blend::ColorDodge => 1.0,
        Blend::ColorBurn if cs > 0.0 => 1.0 - ((1.0 - cb) / cs).min(1.0),
        Blend::ColorBurn => 0.0,
        Blend::HardLight if cs <= 0.5 => cb * 2.0 * cs,
        Blend::HardLight => separable(Blend::Screen, cb, 2.0 * cs - 1.0),
        Blend::SoftLight if cs <= 0.5 => cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb),
        Blend::SoftLight => {
            let d = if cb <= 0.25 {
                ((16.0 * cb - 12.0) * cb + 4.0) * cb
            } else {
                cb.sqrt()
            };
            cb + (2.0 * cs - 1.0) * (d - cb)
        }
        Blend::Difference => (cb - cs).abs(),
        Blend::Exclusion => cb + cs - 2.0 * cb * cs,
        Blend::Normal | Blend::Hue | Blend::Saturation | Blend::Color | Blend::Luminosity => cs,
    }
}

type Rgb = [f32; 3];

/// Table 137's `Lum`, with the standard's own coefficients.
fn lum(c: Rgb) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// `ClipColor`: two sequential rescales toward the neutral axis.
fn clip_colour(mut c: Rgb) -> Rgb {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    if n < 0.0 && l - n > 0.0 {
        for v in &mut c {
            *v = l + (*v - l) * l / (l - n);
        }
    }
    if x > 1.0 && x - l > 0.0 {
        for v in &mut c {
            *v = l + (*v - l) * (1.0 - l) / (x - l);
        }
    }
    c
}

fn set_lum(c: Rgb, l: f32) -> Rgb {
    let d = l - lum(c);
    clip_colour([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: Rgb) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// `SetSat`: the largest component becomes `s`, the smallest 0, the middle
/// one scaled between them; all 0 when the components are equal. Tied
/// components take the same value, as the spec's positional form gives.
fn set_sat(c: Rgb, s: f32) -> Rgb {
    let cmax = c[0].max(c[1]).max(c[2]);
    let cmin = c[0].min(c[1]).min(c[2]);
    if cmax <= cmin {
        return [0.0; 3];
    }
    c.map(|v| {
        if v >= cmax {
            s
        } else if v <= cmin {
            0.0
        } else {
            (v - cmin) * s / (cmax - cmin)
        }
    })
}

/// A non-separable mode over additive RGB.
fn non_separable(mode: Blend, cb: Rgb, cs: Rgb) -> Rgb {
    match mode {
        Blend::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        Blend::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        Blend::Color => set_lum(cs, lum(cb)),
        Blend::Luminosity => set_lum(cb, lum(cs)),
        _ => cs,
    }
}

/// `B(cb, cs)` in `space`. Components past the space's own are zero.
///
/// A subtractive space blends the complements of its components and
/// complements the result (§11.3.4), `K` included for separable modes. For
/// a non-separable mode in DeviceCMYK, `C`, `M` and `Y` are complemented to
/// RGB, blended, and complemented back; `K` is the backdrop's for `Hue`,
/// `Saturation` and `Color` and the source's for `Luminosity` (§11.3.5.3).
pub(crate) fn blend(mode: Blend, space: Space, cb: Colour, cs: Colour) -> Colour {
    if mode == Blend::Normal {
        return cs;
    }
    let n = space.components();
    let mut out = [0.0; 4];
    if mode.is_separable() {
        for ((o, &b), &s) in out.iter_mut().zip(&cb).zip(&cs).take(n) {
            *o = if space.is_subtractive() {
                1.0 - separable(mode, 1.0 - b, 1.0 - s)
            } else {
                separable(mode, b, s)
            };
        }
        return out;
    }
    match space {
        Space::Rgb => {
            let r = non_separable(mode, [cb[0], cb[1], cb[2]], [cs[0], cs[1], cs[2]]);
            out[..3].copy_from_slice(&r);
        }
        Space::Cmyk => {
            let rb = [1.0 - cb[0], 1.0 - cb[1], 1.0 - cb[2]];
            let rs = [1.0 - cs[0], 1.0 - cs[1], 1.0 - cs[2]];
            let r = non_separable(mode, rb, rs);
            out = [
                1.0 - r[0],
                1.0 - r[1],
                1.0 - r[2],
                if mode == Blend::Luminosity {
                    cs[3]
                } else {
                    cb[3]
                },
            ];
        }
    }
    out
}

/// `B(cb, cs)` on one spot plane. Only a separable, white-preserving mode
/// reaches a spot colorant (ISO 32000-1 §11.7.4.2; ISO 32000-2 notes
/// `Difference` and `Exclusion` as the separable ones that are not), and it
/// blends the complements like any subtractive component. Any other mode is
/// `Normal` there.
pub(crate) fn spot(mode: Blend, cb: f32, cs: f32) -> f32 {
    if !mode.is_separable() || matches!(mode, Blend::Difference | Blend::Exclusion) {
        return cs;
    }
    1.0 - separable(mode, 1.0 - cb, 1.0 - cs)
}

#[cfg(test)]
mod tests {
    // Tests fail by panicking.
    #![allow(clippy::indexing_slicing)]
    use super::*;

    fn close(a: Colour, b: Colour) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    #[test]
    fn table_136_spot_checks() {
        let g = |m| blend(m, Space::Rgb, [0.25, 0.0, 0.0, 0.0], [0.75, 0.0, 0.0, 0.0])[0];
        assert!((g(Blend::Multiply) - 0.1875).abs() < 1e-6);
        assert!((g(Blend::Screen) - 0.8125).abs() < 1e-6);
        assert!((g(Blend::Difference) - 0.5).abs() < 1e-6);
        assert!((g(Blend::Exclusion) - 0.625).abs() < 1e-6);
        assert!((g(Blend::Darken) - 0.25).abs() < 1e-6);
        // HardLight branches on the source, Overlay on the backdrop.
        assert!((g(Blend::HardLight) - 0.625).abs() < 1e-6);
        assert!((g(Blend::Overlay) - 0.375).abs() < 1e-6);
        assert!((g(Blend::ColorDodge) - 1.0).abs() < 1e-6);
        assert!((g(Blend::ColorBurn) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn a_subtractive_multiply_darkens_like_an_additive_one() {
        // Multiply in CMYK is Screen of the inks: more ink, darker.
        let out = blend(
            Blend::Multiply,
            Space::Cmyk,
            [0.5, 0.0, 0.0, 0.0],
            [0.5, 0.0, 0.0, 0.0],
        );
        assert!(close(out, [0.75, 0.0, 0.0, 0.0]));
    }

    #[test]
    fn luminosity_takes_the_source_k_and_hue_the_backdrop_k() {
        let cb = [0.2, 0.4, 0.6, 0.1];
        let cs = [0.7, 0.1, 0.3, 0.5];
        assert!((blend(Blend::Luminosity, Space::Cmyk, cb, cs)[3] - 0.5).abs() < 1e-6);
        assert!((blend(Blend::Hue, Space::Cmyk, cb, cs)[3] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn colour_keeps_the_backdrop_luminosity() {
        let cb = [0.2, 0.4, 0.6, 0.0];
        let out = blend(Blend::Color, Space::Rgb, cb, [1.0, 0.0, 0.0, 0.0]);
        let l = |c: Colour| lum([c[0], c[1], c[2]]);
        assert!((l(out) - l(cb)).abs() < 1e-5);
        assert!(out[0] > out[1] && out[0] > out[2]);
    }

    #[test]
    fn set_sat_zeroes_the_minimum_even_for_a_neutral() {
        assert_eq!(set_sat([0.5, 0.5, 0.5], 0.3), [0.0, 0.0, 0.0]);
    }
}

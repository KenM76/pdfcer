//! The four non-separable blend modes, ISO 32000-1 §11.3.5.3 Table 137,
//! computed with the clause's own pseudocode and `Lum` coefficients
//! (0.30, 0.59, 0.11).

/// One of Table 137's four modes.
///
/// Separate from `tiny_skia::BlendMode` on purpose: these four are the ones
/// pdfcer computes, and keeping them in their own type makes it impossible to
/// route one to the rasteriser by accident — which is the exact mistake
/// decision 066 was minted about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NonSeparableBlend {
    /// `SetLum(SetSat(Cs, Sat(Cb)), Lum(Cb))` — source hue, backdrop
    /// saturation and luminosity.
    Hue,
    /// `SetLum(SetSat(Cb, Sat(Cs)), Lum(Cb))` — backdrop hue and luminosity,
    /// source saturation.
    Saturation,
    /// `SetLum(Cs, Lum(Cb))` — source hue and saturation, backdrop
    /// luminosity.
    Color,
    /// `SetLum(Cb, Lum(Cs))` — backdrop hue and saturation, source
    /// luminosity.
    Luminosity,
}

impl NonSeparableBlend {
    /// The Table 136/137 `/BM` name, for diagnostics.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Hue => "Hue",
            Self::Saturation => "Saturation",
            Self::Color => "Color",
            Self::Luminosity => "Luminosity",
        }
    }

    /// Resolve a `/BM` name to one of the four, or `None`.
    ///
    /// Kept beside the implementation rather than in `gstate`'s
    /// `blend_mode_from_name` because that function returns a
    /// `tiny_skia::BlendMode`, and these four must never be expressible as
    /// one.
    #[must_use]
    pub fn from_name(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"Hue" => Self::Hue,
            b"Saturation" => Self::Saturation,
            b"Color" => Self::Color,
            b"Luminosity" => Self::Luminosity,
            _ => return None,
        })
    }
}

/// An RGB colour in [0, 1], the space Table 137's pseudocode operates in.
type Rgb = [f32; 3];

/// `Lum(C) = 0.3 × C_red + 0.59 × C_green + 0.11 × C_blue` (§11.3.5.3).
///
/// See the module docs: these constants are the clause's own and are not to
/// be replaced with a broadcast luma standard.
#[must_use]
fn lum(c: Rgb) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// `Sat(C) = max(C) − min(C)` (§11.3.5.3).
#[must_use]
fn sat(c: Rgb) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// `ClipColor(C)` — pull an out-of-gamut colour back **at constant
/// luminosity** (§11.3.5.3).
///
/// Trap 3 in the module docs: this is not a clamp. A clamp moves the
/// luminosity; this rescales the colour toward the neutral axis and leaves
/// `Lum(C)` where it was, which is the whole reason the function exists.
#[must_use]
fn clip_color(mut c: Rgb) -> Rgb {
    // Captured BEFORE either block, per trap 2 — the rescales must not see
    // each other's `l`, `n` or `x`.
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);

    // STRICT `<`, per trap 7. And the divisor `l - n` is guarded: it is zero
    // exactly when the colour is a neutral at or below 0, which is reachable
    // (trap 8). §11.3.2 forbids malfunctioning; the colour is already
    // neutral there, so leaving it is the answer the rescale would converge
    // to anyway.
    if n < 0.0 && (l - n).abs() > f32::EPSILON {
        for ch in &mut c {
            *ch = l + (((*ch - l) * l) / (l - n));
        }
    }
    // SEQUENTIAL, not `else if`, per trap 1: this runs on the output of the
    // block above when a colour is out of gamut at both ends.
    if x > 1.0 && (x - l).abs() > f32::EPSILON {
        for ch in &mut c {
            *ch = l + (((*ch - l) * (1.0 - l)) / (x - l));
        }
    }
    c
}

/// `SetLum(C, l)` — give `C` the luminosity `l` (§11.3.5.3).
///
/// Always returns through [`clip_color`]; callers must not call it again.
#[must_use]
fn set_lum(c: Rgb, l: f32) -> Rgb {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

/// `SetSat(C, s)` — give `C` the saturation `s` (§11.3.5.3).
///
/// Traps 4, 5 and 6 all live in this function, which is why it is written
/// out by index rather than with a sort: **min/mid/max are positional and
/// resolved on entry**, `C_mid` is computed before `C_max` because its
/// formula reads `C_max`, and `C_min = 0.0` is unconditional.
#[must_use]
fn set_sat(c: Rgb, s: f32) -> Rgb {
    // `imin`/`imid`/`imax` are a permutation of 0..3, so every `at`/`put`
    // below is in range; the fallbacks exist only to keep this panic-free.
    let at = |i: usize| c.get(i).copied().unwrap_or(0.0);
    // Resolve the three positions ONCE, on entry (trap 5). `imin`/`imid`/
    // `imax` are indices into the original colour and stay fixed while the
    // values beneath them change.
    let (mut imin, mut imid, mut imax) = (0_usize, 1_usize, 2_usize);
    if at(imin) > at(imid) {
        std::mem::swap(&mut imin, &mut imid);
    }
    if at(imid) > at(imax) {
        std::mem::swap(&mut imid, &mut imax);
    }
    if at(imin) > at(imid) {
        std::mem::swap(&mut imin, &mut imid);
    }

    let mut out = c;
    let mut put = |i: usize, v: f32| {
        if let Some(slot) = out.get_mut(i) {
            *slot = v;
        }
    };
    // STRICT `>` (trap 7): a perfectly neutral colour takes the `else` and
    // comes out black, which `SetLum` then lifts to the requested luminosity.
    if at(imax) > at(imin) {
        // `C_mid` FIRST — its formula reads the ENTRY values of `C_min` and
        // `C_max`, so assigning `C_max = s` before it corrupts the result
        // (trap 6).
        put(imid, ((at(imid) - at(imin)) * s) / (at(imax) - at(imin)));
        put(imax, s);
    } else {
        put(imid, 0.0);
        put(imax, 0.0);
    }
    // UNCONDITIONAL, outside the `if`/`else` (trap 4).
    put(imin, 0.0);
    out
}

/// Table 137's `B(Cb, Cs)` for one of the four modes.
///
/// `cb` is the backdrop, `cs` the source, both RGB in [0, 1]. The result is
/// the **blended colour**, before any alpha compositing — §11.3.6's
/// `Union`/`Composite` arithmetic is the caller's, exactly as it is for the
/// eleven separable modes the rasteriser handles.
#[must_use]
pub fn blend(mode: NonSeparableBlend, cb: Rgb, cs: Rgb) -> Rgb {
    match mode {
        // Note the asymmetry the clause flags: Hue and Saturation BOTH end
        // with `Lum(cb)` (trap 9). They differ only in which colour is
        // re-saturated and which supplies the saturation.
        NonSeparableBlend::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        NonSeparableBlend::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        NonSeparableBlend::Color => set_lum(cs, lum(cb)),
        NonSeparableBlend::Luminosity => set_lum(cb, lum(cs)),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;

    fn close(a: Rgb, b: Rgb) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4)
    }

    /// THE CANONICAL CASE, and the reason this module exists.
    ///
    /// `Luminosity` of a BLACK source over a pure BLUE backdrop must be
    /// black: the result takes the backdrop's hue and saturation but the
    /// source's luminosity, and the source's luminosity is zero.
    ///
    /// `tiny_skia` 0.11.4 returns `(0, 0, 227)` here — the failure that
    /// produced decision 066. If this test ever passes while the render
    /// output is still blue, the wiring is bypassing this module.
    #[test]
    fn luminosity_of_black_over_blue_is_black() {
        let got = blend(
            NonSeparableBlend::Luminosity,
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
        );
        assert!(
            close(got, [0.0, 0.0, 0.0]),
            "Luminosity(black over blue) must be black, got {got:?} — tiny_skia \
             0.11.4 returns (0, 0, 0.89) here, which is the defect decision 066 \
             was minted about"
        );
    }

    /// `ClipColor` preserves luminosity — the property that makes it not a
    /// clamp (trap 3).
    ///
    /// Checked on colours that are out of gamut low, high, and BOTH, because
    /// the both case is what proves the two `if`s are sequential (trap 1).
    #[test]
    fn clip_color_preserves_luminosity_and_is_not_a_clamp() {
        for c in [
            [-0.4, 0.5, 0.9_f32],
            [0.2, 1.6, 0.4],
            [-0.3, 1.5, 0.5], // out at BOTH ends: exercises trap 1
        ] {
            let before = lum(c);
            let after = clip_color(c);
            assert!(
                (lum(after) - before).abs() < 1e-3,
                "ClipColor must hold luminosity: {c:?} -> {after:?}, \
                 lum {before} -> {}",
                lum(after)
            );
            // And it must differ from the naive clamp, or the whole
            // distinction is untested.
            let clamped = [
                c[0].clamp(0.0, 1.0),
                c[1].clamp(0.0, 1.0),
                c[2].clamp(0.0, 1.0),
            ];
            assert!(
                !close(after, clamped),
                "ClipColor({c:?}) came out equal to a per-channel clamp — that \
                 is the tiny_skia defect, reproduced here"
            );
        }
    }

    /// An in-gamut colour passes through `ClipColor` untouched, and the
    /// comparisons are strict (trap 7): exactly 0.0 and exactly 1.0 do not
    /// trigger a rescale.
    #[test]
    fn clip_color_leaves_in_gamut_colours_alone_including_the_endpoints() {
        for c in [
            [0.0, 0.5, 1.0_f32],
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.3, 0.4, 0.5],
        ] {
            assert!(
                close(clip_color(c), c),
                "{c:?} is in gamut and must not move"
            );
        }
    }

    /// `SetSat` zeroes the minimum channel **unconditionally** (trap 4) and
    /// produces the requested saturation.
    #[test]
    fn set_sat_zeroes_the_minimum_and_hits_the_target() {
        let out = set_sat([0.2, 0.6, 0.4], 0.5);
        assert!(
            (sat(out) - 0.5).abs() < EPS,
            "saturation must be 0.5, got {out:?}"
        );
        assert!(
            (out[0] - 0.0).abs() < EPS,
            "the MINIMUM channel must be zeroed unconditionally; got {out:?}"
        );
        // The mid channel keeps its relative position: (0.6-0.2)/(0.6-0.2)
        // is max, (0.4-0.2)/(0.6-0.2) = 0.5 of the range.
        assert!(
            (out[2] - 0.25).abs() < EPS,
            "mid channel misplaced: {out:?}"
        );
    }

    /// A neutral colour takes `SetSat`'s `else` branch and comes out black —
    /// including its minimum channel, which the unconditional assignment
    /// covers (traps 4 and 7).
    #[test]
    fn set_sat_of_a_neutral_is_black() {
        let out = set_sat([0.5, 0.5, 0.5], 0.8);
        assert!(
            close(out, [0.0, 0.0, 0.0]),
            "a neutral must come out black, got {out:?}"
        );
    }

    /// `SetLum` moves luminosity to the target and leaves it there.
    #[test]
    fn set_lum_reaches_its_target() {
        for target in [0.0_f32, 0.25, 0.5, 1.0] {
            let out = set_lum([0.2, 0.7, 0.4], target);
            assert!(
                (lum(out) - target).abs() < 1e-3,
                "SetLum to {target} gave {out:?} with lum {}",
                lum(out)
            );
        }
    }

    /// Hue and Saturation both take luminosity from the BACKDROP (trap 9).
    ///
    /// The easy error is giving `Saturation` the source's luminosity, which
    /// looks plausible and is wrong. This pins both.
    #[test]
    fn hue_and_saturation_both_take_backdrop_luminosity() {
        let cb = [0.2, 0.6, 0.9_f32];
        let cs = [0.8, 0.1, 0.3];
        for mode in [NonSeparableBlend::Hue, NonSeparableBlend::Saturation] {
            let out = blend(mode, cb, cs);
            assert!(
                (lum(out) - lum(cb)).abs() < 1e-3,
                "{} must take Lum from the BACKDROP ({}), got {} from {out:?}",
                mode.name(),
                lum(cb),
                lum(out)
            );
        }
    }

    /// `Color` takes the backdrop's luminosity; `Luminosity` takes the
    /// source's. They are each other's mirror and swapping them is the other
    /// easy error.
    #[test]
    fn color_and_luminosity_are_mirrors() {
        let cb = [0.2, 0.6, 0.9_f32];
        let cs = [0.8, 0.1, 0.3];
        let colour = blend(NonSeparableBlend::Color, cb, cs);
        let luminos = blend(NonSeparableBlend::Luminosity, cb, cs);
        assert!(
            (lum(colour) - lum(cb)).abs() < 1e-3,
            "Color takes backdrop lum"
        );
        assert!(
            (lum(luminos) - lum(cs)).abs() < 1e-3,
            "Luminosity takes source lum"
        );
        // `Color(cb, cs)` and `Luminosity(cs, cb)` are the same operation
        // with the arguments swapped -- a structural identity from Table 137
        // itself, and a check that neither is silently ignoring an argument.
        let swapped = blend(NonSeparableBlend::Luminosity, cs, cb);
        assert!(
            close(colour, swapped),
            "Color(cb,cs)={colour:?} must equal Luminosity(cs,cb)={swapped:?}"
        );
    }

    /// Every mode, over every combination of the eight RGB corners plus a
    /// few midtones, must produce a finite in-gamut colour.
    ///
    /// This is the "shall not malfunction" check (§11.3.2, trap 8): the
    /// division guards are reachable from real inputs, and NaN reaching a
    /// pixmap is a crash or a black hole in the page.
    #[test]
    fn no_input_produces_nan_or_an_out_of_gamut_result() {
        let corners: Vec<Rgb> = {
            let v = [0.0_f32, 1.0];
            let mut out = Vec::new();
            for r in v {
                for g in v {
                    for b in v {
                        out.push([r, g, b]);
                    }
                }
            }
            out.push([0.5, 0.5, 0.5]);
            out.push([0.25, 0.6, 0.9]);
            out
        };
        for mode in [
            NonSeparableBlend::Hue,
            NonSeparableBlend::Saturation,
            NonSeparableBlend::Color,
            NonSeparableBlend::Luminosity,
        ] {
            for &cb in &corners {
                for &cs in &corners {
                    let out = blend(mode, cb, cs);
                    for (i, ch) in out.iter().enumerate() {
                        assert!(
                            ch.is_finite(),
                            "{} ({cb:?}, {cs:?}) channel {i} is {ch}",
                            mode.name()
                        );
                        assert!(
                            *ch >= -1e-3 && *ch <= 1.0 + 1e-3,
                            "{} ({cb:?}, {cs:?}) channel {i} out of gamut: {ch}",
                            mode.name()
                        );
                    }
                }
            }
        }
    }

    /// The `/BM` names resolve, and nothing else does.
    #[test]
    fn only_the_four_names_resolve() {
        assert_eq!(
            NonSeparableBlend::from_name(b"Hue"),
            Some(NonSeparableBlend::Hue)
        );
        assert_eq!(
            NonSeparableBlend::from_name(b"Luminosity"),
            Some(NonSeparableBlend::Luminosity)
        );
        for other in [&b"Multiply"[..], b"Normal", b"Darken", b"hue", b""] {
            assert_eq!(
                NonSeparableBlend::from_name(other),
                None,
                "{other:?} must not resolve to a non-separable mode"
            );
        }
    }
}

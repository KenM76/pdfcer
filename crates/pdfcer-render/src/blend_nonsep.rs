//! The four **non-separable** blend modes — ISO 32000-1 §11.3.5.3, Table 137.
//!
//! `Hue`, `Saturation`, `Color` and `Luminosity`. They are called
//! non-separable because, unlike Table 136's eleven, they **cannot be
//! computed one channel at a time** — each output channel depends on all
//! three input channels of both colours.
//!
//! # Why pdfcer computes these itself instead of calling the rasteriser
//!
//! `tiny_skia` 0.11.4 has `BlendMode::Hue`/`Saturation`/`Color`/`Luminosity`,
//! named after exactly these operations, and routing to them was one line
//! away from shipping in `Pass 90.1`. **They are measurably wrong** — up to
//! **107/255** error on 9.4–15.5 % of random colour pairs, over 60,000
//! measured pixels.
//!
//! Root cause, reproduced rather than inferred: the crate's `clip_color`
//! gates its low-gamut rescale on `mx >= 0` where the standard (and upstream
//! Skia) gate on `mn < 0`, so the branch is dead and negative channels
//! produced by `SetLum` get hard-clamped instead of rescaled at constant
//! luminosity. **Canonical demonstration, and the first test below:**
//! `Luminosity` of a **black** source over a pure **blue** backdrop must be
//! black; the crate returns `(0, 0, 227)`.
//!
//! That measurement produced `ARCHITECTURE.md` §12 **decision 066** — pdfcer
//! does not route a spec-governed computation to a dependency whose output it
//! has not verified against the standard. **Decision 066 refused TRUSTING
//! tiny-skia's implementation; it did not refuse the feature.** This module
//! is the other half of that refusal finally arriving: the operation, done
//! here, verified against the clause.
//!
//! # There is no HSL conversion here, and that is deliberate
//!
//! The spec's own words (`iso32000__s__11.3.5.md` §4.1): *"There is no HSL
//! conversion to implement — the pseudocode IS the specification. Any
//! implementation that round-trips through an actual HSL/HSV space is not
//! conformant."* Every function below is a transcription of the printed
//! pseudocode, not a re-derivation of it.
//!
//! # The traps, every one of which the clause names explicitly
//!
//! These are transcribed from `iso32000__s__11.3.5.md`'s reading notes, and
//! each one has a test at the bottom of this file:
//!
//! 1. **[`clip_color`]'s two `if`s are SEQUENTIAL, not `else if`.** A colour
//!    can be both below 0 and above 1 after [`set_lum`], and the second
//!    rescale runs on the output of the first.
//! 2. **`l`, `n` and `x` are captured BEFORE either block.** Recomputing
//!    them between the blocks changes the result.
//! 3. **A per-channel `clamp(0, 1)` is NOT `ClipColor`.** It is precisely the
//!    defect that makes the dependency wrong.
//! 4. **[`set_sat`]'s `C_min = 0.0` is UNCONDITIONAL** — outside the
//!    `if`/`else`. Putting it inside the `else` leaves `C_min` at its input
//!    value on the common branch, wrong for every non-neutral colour.
//! 5. **min/mid/max are POSITIONAL, resolved once on entry**, and the
//!    identities may not hold afterwards. Do not re-sort.
//! 6. **`C_mid` is assigned before `C_max`, and its formula reads `C_max`.**
//!    Assigning `C_max = s` first corrupts `C_mid`. Order is load-bearing.
//! 7. **Comparisons are STRICT** — `n < 0.0`, `x > 1.0`, `C_max > C_min`.
//!    Exactly 0.0 or 1.0 does not trigger a rescale.
//! 8. **Division by zero is reachable**: `l − n` is 0 when the colour is a
//!    neutral at or below 0, `x − l` when it is a neutral at or above 1.
//!    §11.3.2's "shall not malfunction" applies; both are guarded.
//! 9. **`Hue` and `Saturation` BOTH finish with `Lum(Cb)`** — the backdrop
//!    supplies luminosity in both. They differ only in which colour is
//!    passed to `SetSat` and which supplies `Sat`.
//!
//! # The luminosity coefficients are 0.30 / 0.59 / 0.11 and must not be
//! "improved"
//!
//! Not Rec.601's `0.299/0.587/0.114`, not Rec.709's `0.2126/0.7152/0.0722`.
//! The clause prints two-decimal constants that sum to exactly 1.0.
//! Substituting Rec.601 changes `Color` and `Luminosity` on every pixel by a
//! small uniform amount — invisible on screen, **wrong in separations**,
//! which is the output this feature exists for.

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
    // Resolve the three positions ONCE, on entry (trap 5). `imin`/`imid`/
    // `imax` are indices into the original colour and stay fixed while the
    // values beneath them change.
    let (mut imin, mut imid, mut imax) = (0_usize, 1_usize, 2_usize);
    if c[imin] > c[imid] {
        std::mem::swap(&mut imin, &mut imid);
    }
    if c[imid] > c[imax] {
        std::mem::swap(&mut imid, &mut imax);
    }
    if c[imin] > c[imid] {
        std::mem::swap(&mut imin, &mut imid);
    }

    let mut out = c;
    // STRICT `>` (trap 7): a perfectly neutral colour takes the `else` and
    // comes out black, which `SetLum` then lifts to the requested luminosity.
    if c[imax] > c[imin] {
        // `C_mid` FIRST — its formula reads the ENTRY values of `C_min` and
        // `C_max`, so assigning `C_max = s` before it corrupts the result
        // (trap 6).
        out[imid] = ((c[imid] - c[imin]) * s) / (c[imax] - c[imin]);
        out[imax] = s;
    } else {
        out[imid] = 0.0;
        out[imax] = 0.0;
    }
    // UNCONDITIONAL, outside the `if`/`else` (trap 4).
    out[imin] = 0.0;
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

/// Composite a solid paint onto `pixmap` through a Table 137 blend mode.
///
/// # Why this exists rather than a `tiny_skia::BlendMode`
///
/// Because the rasteriser's four non-separable modes are measurably wrong
/// (module docs; `ARCHITECTURE.md` §12 decision 066), so pdfcer rasterises the
/// paint to a **coverage mask with the same rasteriser a normal paint uses**
/// and does the blend itself, per pixel.
///
/// That shape is not invented here — it is exactly what
/// [`crate::overprint::composite`] does for §11.7.4.3, and using the same
/// rasteriser for the coverage is what keeps an edge the same SHAPE as a
/// normally-painted one. Only the per-pixel rule differs.
///
/// # The compositing arithmetic, and what is assumed
///
/// `t = coverage × alpha` is how much of the blended result replaces what is
/// there. §11.3.6's general formula reduces to a lerp between the backdrop
/// and `B(Cb, Cs)` when the backdrop is opaque, which it is here: pdfcer's
/// page group is flattened over white at the end (§11.4.7), and a fully
/// transparent destination pixel has no meaningful colour, so it is treated
/// as white paper — an unpainted sheet.
///
/// **This mirrors `overprint::composite`'s treatment deliberately**, so the
/// two per-pixel paths cannot come to disagree about what an untouched pixel
/// is.
///
/// Returns the number of pixels changed, for the diagnostics counter.
pub(crate) fn composite(
    pixmap: &mut tiny_skia::Pixmap,
    coverage: &tiny_skia::Mask,
    mode: NonSeparableBlend,
    source: [f32; 3],
    alpha: f32,
    region: (u32, u32, u32, u32),
) -> u32 {
    let width = pixmap.width();
    let (x0, y0, x1, y1) = region;
    let cov = coverage.data();
    let mut changed = 0_u32;

    for y in y0..y1 {
        for x in x0..x1 {
            let idx = (y * width + x) as usize;
            let Some(&cbyte) = cov.get(idx) else {
                continue;
            };
            let c = f32::from(cbyte) / 255.0;
            if c <= 0.0 {
                continue;
            }
            let t = c * alpha;

            let Some(px) = pixmap.pixels().get(idx).copied() else {
                continue;
            };
            // ONE formula, in `crate::compositor` — §11.4.4's element
            // composite, with the backdrop's own alpha carried explicitly.
            //
            // ★ This used to substitute a WHITE backdrop wherever the
            // destination was transparent, and lerp from white by `t`. That
            // is the specialisation of §11.4.4 to `α_b = 1`, and it is
            // wrong in exactly the place the page group makes common:
            // §11.4.7 starts the page buffer TRANSPARENT and composites the
            // white medium in once at the end, so "transparent" means "no
            // backdrop", not "paper". `Sat(white) = 0` and `Lum(white) = 1`
            // make `Hue`/`Saturation`/`Color` of anything over white
            // *white*, which is what suite `PCS1_162` rendered.
            let out = crate::compositor::composite_element(
                crate::compositor::Pixel::from_premultiplied(px),
                crate::compositor::Pixel { c: source, a: t },
                crate::compositor::Blend::NonSeparable(mode),
            );
            if let Some(newpx) = out.to_premultiplied() {
                if newpx != px {
                    changed += 1;
                }
                pixmap.pixels_mut()[idx] = newpx;
            }
        }
    }
    changed
}

/// Composite a transparency group's **result** onto its backdrop through a
/// Table 137 blend mode — §11.4.5.
///
/// # Why this is separate from [`composite`]
///
/// [`composite`] blends one **solid** colour through a coverage mask; this
/// blends a whole **buffer**, where every pixel carries its own colour *and*
/// its own alpha. The group's alpha is what says which pixels the group
/// actually marked, so it replaces the coverage mask — and multiplying the
/// two concepts together into one function would mean a mask that is
/// sometimes coverage and sometimes group alpha, which is how the two get
/// confused.
///
/// # The arithmetic
///
/// For each pixel: `t = group_alpha × opacity` is how much of the blended
/// result replaces the backdrop, exactly as coverage does in [`composite`].
/// A fully transparent group pixel contributes nothing and is skipped, which
/// is also what makes this cheap on a page-sized buffer holding a small
/// group.
///
/// The backdrop is treated as white paper where it is fully transparent, the
/// same convention [`composite`] and `overprint::composite` use — stated
/// again rather than cross-referenced because three functions agreeing about
/// what an untouched pixel is only matters if each one says so.
pub(crate) fn composite_layer(
    dest: &mut tiny_skia::Pixmap,
    group: &tiny_skia::Pixmap,
    mode: NonSeparableBlend,
    opacity: f32,
) {
    let n = dest.pixels().len().min(group.pixels().len());
    for idx in 0..n {
        let g = crate::compositor::Pixel::from_premultiplied(group.pixels()[idx]);
        if g.a <= 0.0 {
            continue;
        }
        // §11.4.5: the constant alpha in force at the `Do` multiplies the
        // GROUP'S alpha, giving the source alpha of the group-as-element.
        let source = crate::compositor::Pixel {
            c: g.c,
            a: g.a * opacity.clamp(0.0, 1.0),
        };
        let out = crate::compositor::composite_element(
            crate::compositor::Pixel::from_premultiplied(dest.pixels()[idx]),
            source,
            crate::compositor::Blend::NonSeparable(mode),
        );
        if let Some(px) = out.to_premultiplied() {
            dest.pixels_mut()[idx] = px;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;

    fn close(a: Rgb, b: Rgb) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4)
    }

    /// ★ THE CANONICAL CASE, and the reason this module exists.
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

    /// ★ Hue and Saturation both take luminosity from the BACKDROP (trap 9).
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

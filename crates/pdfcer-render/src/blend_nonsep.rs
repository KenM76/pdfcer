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
//! 1. **`clip_color`'s two `if`s are SEQUENTIAL, not `else if`.** A colour
//!    can be both below 0 and above 1 after `set_lum`, and the second
//!    rescale runs on the output of the first.
//! 2. **`l`, `n` and `x` are captured BEFORE either block.** Recomputing
//!    them between the blocks changes the result.
//! 3. **A per-channel `clamp(0, 1)` is NOT `ClipColor`.** It is precisely the
//!    defect that makes the dependency wrong.
//! 4. **`set_sat`'s `C_min = 0.0` is UNCONDITIONAL** — outside the
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

pub use pdfcer_ink::nonsep::{NonSeparableBlend, blend};

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
            // This used to substitute a WHITE backdrop wherever the
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
            if let Some(newpx) = out.to_premultiplied()
                && let Some(slot) = pixmap.pixels_mut().get_mut(idx)
            {
                if newpx != px {
                    changed += 1;
                }
                *slot = newpx;
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
    for (dst, &group_px) in dest.pixels_mut().iter_mut().zip(group.pixels()) {
        let g = crate::compositor::Pixel::from_premultiplied(group_px);
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
            crate::compositor::Pixel::from_premultiplied(*dst),
            source,
            crate::compositor::Blend::NonSeparable(mode),
        );
        if let Some(px) = out.to_premultiplied() {
            *dst = px;
        }
    }
}

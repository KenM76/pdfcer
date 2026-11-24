//! Blend modes over both of ISO 32000-1 §11.3.5's tables, and §11.3.7.3's
//! `Union`. Components are in `[0, 1]`; [`Blend::apply_subtractive`]
//! applies §11.3.4's complement for a subtractive space.

use crate::nonsep::NonSeparableBlend;

/// §11.3.7.3's `Union` — the "either" of two shape or alpha values.
///
/// ```text
/// Union(b, s) = 1 − [(1 − b) × (1 − s)] = b + s − (b × s)
/// ```
///
/// Named with a trailing underscore because `union` is a reserved word in
/// Rust 2024 only as a contextual keyword for `union` types; the underscore
/// removes any question at the call site.
///
/// # Why `max` is not a substitute
///
/// `max(0.5, 0.5) = 0.5`; `Union(0.5, 0.5) = 0.75`. Two half-covering
/// elements leave a quarter of the pixel uncovered, not a half. Using
/// `max` under-reports accumulated alpha everywhere two partially
/// transparent things overlap, which is every anti-aliased edge in a
/// transparency group.
#[must_use]
#[inline]
pub fn union_(b: f32, s: f32) -> f32 {
    b.mul_add(-s, b + s)
}

/// A blend mode, over **both** of clause 11.3.5's tables.
///
/// # Why one enum rather than `tiny_skia::BlendMode`
///
/// Because this module must be able to name a blend that `tiny_skia`
/// cannot compute correctly. `pdfcer-render`'s `blend_mode_from_name` returns
/// a `tiny_skia::BlendMode` and deliberately refuses the four
/// non-separable modes (decision 066: the dependency's `clip_color` gates
/// its low-gamut rescale on the wrong comparison and is measurably wrong by
/// up to 107/255). A compositor that owns the arithmetic needs a type that
/// can carry all seventeen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    /// Table 136's `Normal` — and its PDF 1.3 alias `Compatible`, which
    /// §11.3.5 defines as exactly source-over.
    Normal,
    /// `B(cb, cs) = cb × cs`.
    Multiply,
    /// `B(cb, cs) = cb + cs − (cb × cs)`.
    Screen,
    /// `HardLight(cs, cb)` — the arguments are **swapped**, §11.3.5 §3.1.
    Overlay,
    /// `min(cb, cs)`.
    Darken,
    /// `max(cb, cs)`.
    Lighten,
    /// `min(1, cb / (1 − cs))` if `cs < 1`, else `1`.
    ColorDodge,
    /// `1 − min(1, (1 − cb) / cs)` if `cs > 0`, else `0`.
    ColorBurn,
    /// `Multiply(cb, 2cs)` if `cs ≤ 0.5`, else `Screen(cb, 2cs − 1)`.
    HardLight,
    /// The two-branch soft light with `D(x)`.
    SoftLight,
    /// `|cb − cs|` — note the **absolute value**; ISO 32000-1's printed
    /// table drops the bars (corpus erratum `GD-4`).
    Difference,
    /// `cb + cs − 2 × cb × cs`.
    Exclusion,
    /// One of Table 137's four, computed by [`crate::nonsep`].
    NonSeparable(NonSeparableBlend),
}

impl Blend {
    /// `true` when `B(cb, cs) = cs` for every `cb` — i.e. the mode is
    /// indistinguishable from `Normal` and the whole blend can be skipped.
    #[must_use]
    pub fn is_normal(self) -> bool {
        self == Self::Normal
    }

    /// Evaluate `B(C_b, C_s)` for this mode.
    ///
    /// Both arguments are **un-premultiplied, additive** components in
    /// `[0, 1]` — §11.3.4's requirement. A subtractive (CMYK/Separation/
    /// DeviceN) space must complement its components before calling this
    /// and complement the result back; that is `Pass 97.1`'s work and is
    /// deliberately **not** done here, because doing it in an sRGB buffer
    /// would be a second wrong answer rather than a first right one.
    #[must_use]
    pub fn apply(self, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
        match self {
            Self::Normal => cs,
            Self::NonSeparable(m) => crate::nonsep::blend(m, cb, cs),
            _ => [
                blend_separable(self, cb[0], cs[0]),
                blend_separable(self, cb[1], cs[1]),
                blend_separable(self, cb[2], cs[2]),
            ],
        }
    }

    /// Evaluate `B(C_b, C_s)` on **four subtractive** components —
    /// §11.3.4's complement rule, and §11.3.5.3's CMYK detour for the four
    /// non-separable modes.
    ///
    /// # The two rules, and why they are not the same rule
    ///
    /// For a **separable** mode §11.3.4 is the whole story: complement each
    /// component, apply the scalar function, complement the result back.
    ///
    /// For a **non-separable** mode §11.3.5.3 is more specific, and it is
    /// quoted here because the difference is easy to lose:
    ///
    /// > "The formulas in this sub-clause apply to **RGB** spaces. Blending
    /// > in **CMYK** spaces … **shall** be handled in the following way:
    /// > the C, M and Y components **shall** be converted to their
    /// > complementary R, G and B components in the usual way; the
    /// > preceding formulas **shall** be applied to the RGB colour values;
    /// > the results **shall** be converted back to C, M and Y. For the
    /// > **K** component, the result **shall** be the K component of **Cb**
    /// > for the `Hue`, `Saturation` and `Color` blend modes; it **shall**
    /// > be the K component of **Cs** for the `Luminosity` blend mode."
    ///
    /// ⇒ **K is not blended, it is SELECTED**, and which operand it comes
    /// from depends on the mode. Running all four channels through the
    /// non-separable formulas produces entirely plausible output and is
    /// non-conforming — the failure mode this project exists to catch.
    ///
    /// The CMY half is not a second rule: complementing C, M and Y **is**
    /// §11.3.4's complement for those three. §11.3.5.3 is the
    /// specialisation, not a competitor.
    #[must_use]
    pub fn apply_subtractive(self, cb: [f32; 4], cs: [f32; 4]) -> [f32; 4] {
        match self {
            Self::Normal => cs,
            Self::NonSeparable(m) => {
                let rb = [1.0 - cb[0], 1.0 - cb[1], 1.0 - cb[2]];
                let rs = [1.0 - cs[0], 1.0 - cs[1], 1.0 - cs[2]];
                let out = crate::nonsep::blend(m, rb, rs);
                // §11.3.5.3's K selection, by mode. `Luminosity` takes the
                // SOURCE's black; the other three take the BACKDROP's.
                use crate::nonsep::NonSeparableBlend as N;
                let k = if matches!(m, N::Luminosity) {
                    cs[3]
                } else {
                    cb[3]
                };
                [1.0 - out[0], 1.0 - out[1], 1.0 - out[2], k]
            }
            _ => {
                let mut out = [0.0_f32; 4];
                for ((o, &b), &s) in out.iter_mut().zip(&cb).zip(&cs) {
                    *o = 1.0 - blend_separable(self, 1.0 - b, 1.0 - s);
                }
                out
            }
        }
    }
}

/// §11.3.5.2 Table 136 — one component of one separable blend mode.
///
/// Transcribed from `iso32000__s__11.3.5.md` §3, **including its four
/// recorded printing errata** (`GD-1`…`GD-4`), each of which is
/// independently corroborated there by ISO 32000-2:2020 and by W3C
/// Compositing Level 1:
///
/// * `GD-1` — `ColorDodge`'s second branch is `1` **if `cs = 1`**.
/// * `GD-2` — `ColorBurn`'s second branch is `0` **if `cs = 0`**.
/// * `GD-3` — `SoftLight`'s `D(x)` is `((16x − 12)x + 4)x` for `x ≤ 0.25`.
/// * `GD-4` — `Difference` is `|cb − cs|`. ISO 32000-1's printed table
///   shows `cb – cs`; the bars are path-drawn and lost by every text
///   extractor. NOTE 14's own words ("subtracts the darker … from the
///   lighter") require the absolute value.
///
/// # Panics
///
/// Never. [`Blend::Normal`] and [`Blend::NonSeparable`] are handled by
/// [`Blend::apply`] before this is reached; they fall through to `cs`
/// here rather than panicking, because a panic in a per-pixel loop is a
/// worse failure than a `Normal` blend.
#[must_use]
pub fn blend_separable(mode: Blend, cb: f32, cs: f32) -> f32 {
    match mode {
        Blend::Multiply => cb * cs,
        Blend::Screen => cb.mul_add(-cs, cb + cs),
        // §11.3.5 §3.1: Overlay(cb, cs) = HardLight(cs, cb). The swap is
        // the whole definition — writing the HardLight branches out again
        // with the arguments in place is how the swap gets lost.
        Blend::Overlay => blend_separable(Blend::HardLight, cs, cb),
        Blend::Darken => cb.min(cs),
        Blend::Lighten => cb.max(cs),
        Blend::ColorDodge => {
            if cs < 1.0 {
                (cb / (1.0 - cs)).min(1.0)
            } else {
                1.0
            }
        }
        Blend::ColorBurn => {
            if cs > 0.0 {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            } else {
                0.0
            }
        }
        Blend::HardLight => {
            if cs <= 0.5 {
                cb * (2.0 * cs)
            } else {
                let s = cs.mul_add(2.0, -1.0);
                cb.mul_add(-s, cb + s)
            }
        }
        Blend::SoftLight => {
            if cs <= 0.5 {
                // cb − (1 − 2cs) × cb × (1 − cb)
                cs.mul_add(2.0, -1.0).mul_add(cb * (1.0 - cb), cb)
            } else {
                let d = if cb <= 0.25 {
                    // ((16x − 12)x + 4)x
                    (cb.mul_add(16.0, -12.0).mul_add(cb, 4.0)) * cb
                } else {
                    cb.sqrt()
                };
                cs.mul_add(2.0, -1.0).mul_add(d - cb, cb)
            }
        }
        Blend::Difference => (cb - cs).abs(),
        Blend::Exclusion => (cb * cs).mul_add(-2.0, cb + cs),
        // Unreachable through `Blend::apply`, and also through
        // `blend_spots`, which screens `NonSeparable` out before this is
        // called (§11.7.4.2). See the Panics note.
        //
        // Answering `cs` rather than panicking makes this arm a SECOND,
        // accidental enforcement of §11.7.4.2 -- `cs` is exactly what
        // `Normal` would give. Established by sabotage, not by reading:
        // removing `blend_spots`'s guard leaves behaviour unchanged
        // because of this line. It is not the guarantee, it merely
        // coincides with it, and the two are documented separately so a
        // change to either does not silently take the other with it.
        Blend::Normal | Blend::NonSeparable(_) => cs,
    }
}

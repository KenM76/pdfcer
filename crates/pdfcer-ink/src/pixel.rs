//! A subtractive pixel (four process tints, [`MAX_SPOTS`] spot tints and an
//! alpha) and §11.4.4/§11.4.8's element compositing over it.

use crate::blend::{Blend, blend_separable, union_};

/// A colour and an alpha in a **subtractive** space — four ink tints,
/// `0.0` meaning *no ink*.
///
/// # Why this is a separate type and not an additive pixel with a flag
///
/// Because every formula that touches it needs to know, and a flag is
/// something a call site can forget to read. `1 − c` is either the
/// complement §11.3.4 requires or a bug, and the type is what decides
/// which — the same argument [`Blend::NonSeparable`] makes for keeping the
/// four Table 137 modes out of `tiny_skia::BlendMode`.
///
/// Four components rather than N: `Pass 97.1`'s first deliverable is the
/// **blending colour space**, which for every file in the suite corpus is
/// `DeviceCMYK`. Spot planes are the *second* deliverable and want a
/// runtime-N plane-major buffer (measured 3–5× faster than interleaved,
/// `docs/compositor-plan.md` §3.2). Building N now would fuse two
/// questions that fail independently.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelCmyk {
    /// Subtractive tints `[C, M, Y, K]`, `0.0..=1.0`.
    pub c: [f32; 4],
    /// Tints of the page's SPOT colorants, indexed by the page's spot roster
    /// (`Pass 217.0`). Unused entries are `0.0` — "no ink", which is the
    /// correct value for a colorant this page never names.
    ///
    /// A fixed array HERE and a `Vec` in the buffer, because they answer
    /// different questions. `PixelCmyk` is a TRANSIENT value — one pixel in
    /// flight — so a fixed array keeps it `Copy` and costs nothing that
    /// survives the call. The buffer is per-page STORAGE where an unused plane
    /// is real memory, so it allocates only what the page's roster needs.
    pub s: [f32; MAX_SPOTS],
    /// Alpha, `[0, 1]`.
    pub a: f32,
}

/// The most spot colorants one page can composite natively.
///
/// # Why there is a ceiling, and why it is small
///
/// A census of 4,023 real PDFs found **98.6% name no spot colorant at all**,
/// 99.85% name three or fewer, and the maximum seen anywhere was nine. A small
/// ceiling therefore covers essentially the whole corpus, and the pages it does
/// not cover fall back to the flattening that predates this — disclosed, not
/// silent.
///
/// MEMORY is what sets the number, not the census. Each plane costs 4 bytes
/// per pixel on top of the existing 20, and a 300 DPI US-Letter page with four
/// spot planes needs **289 MiB** against the 256 MiB default buffer ceiling —
/// so it would be REFUSED outright. That measurement is why planes are
/// allocated from the page's actual roster rather than provisioned to this
/// maximum, and why this maximum is 4 rather than 9.
pub const MAX_SPOTS: usize = 4;

impl PixelCmyk {
    /// A fully transparent pixel.
    ///
    /// **Its colour is `[0,0,0,0]` — NO INK — and that is deliberately
    /// NOT what a zeroed `DeviceCMYK` buffer should be initialised to.**
    /// §8.6.4.4 gives `DeviceCMYK` an initial colour of `[0 0 0 1]`, so a
    /// `memset(0)` over a CMYK buffer yields **white**, not black, and
    /// inverts every luminosity soft mask built over it. That trap is
    /// correct in sRGB and wrong in CMYK, which is exactly why it belongs
    /// at the Stage A → Stage B boundary and is named here.
    ///
    /// This constant is for a **transparent** pixel, whose colour §11.3.2
    /// declares undefined and which every formula multiplies by its own
    /// zero alpha. It is not an initialiser for an opaque one.
    pub const TRANSPARENT: Self = Self {
        c: [0.0; 4],
        s: [0.0; MAX_SPOTS],
        a: 0.0,
    };
}

/// **§11.4.4's element-compositing formula, in a subtractive space.**
///
/// Identical in structure to `pdfcer-render`'s additive `composite_element` — the compositing
/// arithmetic itself is space-agnostic, which is §11.3.6's own point: the
/// weighted average is affine and its weights sum to exactly 1, so
/// complementation commutes with it. **Only `B()` needs the round trip**,
/// and that is where [`Blend::apply_subtractive`] does it.
///
/// Deliberately a second function rather than a branch inside the first:
/// the two operate on different-width arrays, and unifying them behind a
/// const generic would put a type parameter on the one function every
/// per-pixel loop in this crate calls.
#[must_use]
pub fn composite_element_cmyk(backdrop: PixelCmyk, source: PixelCmyk, blend: Blend) -> PixelCmyk {
    let ab = backdrop.a.clamp(0.0, 1.0);
    let a_s = source.a.clamp(0.0, 1.0);
    let ai = union_(ab, a_s);
    if ai <= 0.0 {
        return PixelCmyk::TRANSPARENT;
    }
    let w = a_s / ai;
    let blended = if blend.is_normal() || ab <= 0.0 {
        source.c
    } else {
        let b = blend.apply_subtractive(backdrop.c, source.c);
        let mut m = [0.0_f32; 4];
        for ((m, &b), &s) in m.iter_mut().zip(&b).zip(&source.c) {
            *m = ab.mul_add(b - s, s);
        }
        m
    };
    let blended_s = blend_spots(blend, backdrop, source, ab);
    let mut c = [0.0_f32; 4];
    for ((c, &x), &b) in c.iter_mut().zip(&blended).zip(&backdrop.c) {
        *c = w.mul_add(x - b, b);
    }
    let mut sp = [0.0_f32; MAX_SPOTS];
    for ((sp, &x), &b) in sp.iter_mut().zip(&blended_s).zip(&backdrop.s) {
        *sp = w.mul_add(x - b, b);
    }
    PixelCmyk { c, s: sp, a: ai }
}

/// §11.3.6's blend-and-mix step applied to the SPOT colorant planes.
///
/// Structurally identical to what [`composite_element_cmyk`] does for the
/// four process channels — `C_s' = (1 - α_b) × C_s + α_b × B(C_b, C_s)`,
/// per §11.3.6 — with one substitution that is a `shall`, not a choice.
///
/// # §11.7.4.2: a spot colorant takes SEPARABLE blend modes only
///
/// The clause is normative and unambiguous: only **separable,
/// white-preserving** blend modes may be applied to a spot colour. A
/// non-separable mode (`Hue`, `Saturation`, `Color`, `Luminosity`), and
/// `Difference` and `Exclusion`, which ISO 32000-2 notes are not
/// white-preserving,
/// are therefore **degraded to `Normal`** on every spot plane, while the
/// four process channels still get them.
///
/// That asymmetry is deliberate and is not an approximation. It is what the
/// standard requires, and the implementation reason reinforces it:
/// [`Blend::apply_subtractive`]'s non-separable arm complements exactly
/// three channels, treats the fourth as black, and hands the triple to a
/// **CIE-derived** hue/saturation/luminosity computation. That computation
/// is defined on a colour, not on an arbitrary ink coverage. There is no
/// meaning to the "hue" of a `PANTONE 265 C` plane, so the function cannot
/// be extended over spot planes even if the clause permitted it — the arm
/// is structurally CMYK-only.
///
/// ## This guard is the SECOND enforcement, not the only one — measured
///
/// Deleting the `NonSeparable` arm from the test below changes **nothing
/// observable**, and that was established by sabotage rather than assumed:
/// the test for it still passed with the guard removed.
///
/// The reason is that [`blend_separable`]'s own final arm already answers
/// `cs` for `Blend::NonSeparable(_)`, so a non-separable mode reaching the
/// per-channel loop would come back as the source tint anyway — which is
/// `Normal`, which is the required behaviour. The two mechanisms agree.
///
/// It is kept regardless, for two reasons that survive the redundancy:
///
/// 1. It states the clause **at the branch the clause governs**, where the
///    next reader of this function will look for it. `blend_separable`'s
///    fallback is documented as a panic-avoidance measure, not as a
///    conformance rule, and relying on it would make §11.7.4.2's
///    enforcement an accident of an unrelated function's error handling.
/// 2. It keeps `blend_separable`'s own *"unreachable through
///    `Blend::apply`"* note true of this path too. Without the guard, the
///    spot loop would be a live caller of an arm labelled unreachable.
///
/// ⇒ **The test below asserts the observable contract, which is right, and
/// cannot distinguish which mechanism delivered it. Do not read a green run
/// as proof that this guard is load-bearing.**
///
/// # Why `ab <= 0.0` short-circuits to the source
///
/// Same reason as the process path: §11.3.6's mix weight is the backdrop
/// alpha, so a fully transparent backdrop contributes nothing and `B` need
/// not be evaluated at all. Skipping it is not merely faster — several
/// separable functions are undefined-ish at the extremes and this keeps
/// them off a path whose result is known.
#[must_use]
fn blend_spots(blend: Blend, backdrop: PixelCmyk, source: PixelCmyk, ab: f32) -> [f32; MAX_SPOTS] {
    // `NonSeparable` joins `Normal` here, per §11.7.4.2 above. Matched
    // explicitly rather than folded into the `is_normal()` test so that the
    // clause is visible at the branch it governs; `Difference` and
    // `Exclusion` by ISO 32000-2's note on each.
    if blend.is_normal()
        || matches!(
            blend,
            Blend::NonSeparable(_) | Blend::Difference | Blend::Exclusion
        )
        || ab <= 0.0
    {
        return source.s;
    }
    let mut out = [0.0_f32; MAX_SPOTS];
    for ((slot, &cb), &cs) in out.iter_mut().zip(backdrop.s.iter()).zip(source.s.iter()) {
        // The `1.0 - x` pairs convert ink coverage to the additive
        // colour value Table 136's functions are written in, exactly as
        // `apply_subtractive` does for the process channels. A spot plane
        // is subtractive in the same sense: 0.0 is no ink.
        let b = 1.0 - blend_separable(blend, 1.0 - cb, 1.0 - cs);
        *slot = ab.mul_add(b - cs, cs);
    }
    out
}

/// **§11.4.8's knockout formula, in a subtractive space.**
///
/// The subtractive twin of `pdfcer-render`'s additive `composite_element_knockout`, standing in the
/// same relationship to it that [`composite_element_cmyk`] stands to
/// `pdfcer-render`'s additive `composite_element`: identical structure, four components instead of
/// three, and `B()` reached through [`Blend::apply_subtractive`] so that
/// §11.3.4's complement is applied around the blend function and
/// §11.3.5.3's **K selection** replaces the formula entirely for the four
/// non-separable modes.
///
/// # Why this is a second function and not a const generic
///
/// The same reason [`composite_element_cmyk`] is: unifying them would put
/// a type parameter on the function every per-pixel loop in this crate
/// calls, to save about twenty lines that the compiler would then have to
/// monomorphise back into two anyway.
///
/// # Everything load-bearing about the additive version is load-bearing here
///
/// The blend function reads the **initial** backdrop, never the
/// accumulated one — using `accum` turns a knockout group back into a
/// normal one while still looking entirely plausible on opaque artwork.
/// Shape and alpha stay separate because `f_s` appears **alone** in
/// `(1 − f_si)` where the ordinary formula has `(1 − α_si)`, and the two
/// coincide exactly when opacity is 1, which is why a fixture built from
/// opaque fills cannot tell a correct implementation from a collapsed one.
///
/// # Returns
///
/// `(⟨C_i, α_i⟩, α_gi)` — the accumulated pixel and the group's own alpha
/// excluding the backdrop, which the caller must carry forward because it
/// cannot be recovered from `α_i` alone.
#[must_use]
pub fn composite_element_knockout_cmyk(
    initial: PixelCmyk,
    accum: PixelCmyk,
    source: PixelCmyk,
    shape: f32,
    accum_group_alpha: f32,
    blend: Blend,
) -> (PixelCmyk, f32) {
    let a0 = initial.a.clamp(0.0, 1.0);
    let a_s = source.a.clamp(0.0, 1.0);
    let f_s = shape.clamp(a_s, 1.0);
    let ag_prev = accum_group_alpha.clamp(0.0, 1.0);

    // α_gi = (1 − f_si)·α_g(i−1) + α_si   (the α_gb term is identically 0)
    let ag = (1.0 - f_s).mul_add(ag_prev, a_s).clamp(0.0, 1.0);
    let ai = union_(a0, ag);
    if ai <= 0.0 {
        return (PixelCmyk::TRANSPARENT, 0.0);
    }

    let blended = if a0 <= 0.0 || blend.is_normal() {
        source.c
    } else {
        let b = blend.apply_subtractive(initial.c, source.c);
        let mut m = [0.0_f32; 4];
        for ((m, &b), &s) in m.iter_mut().zip(&b).zip(&source.c) {
            *m = a0.mul_add(b - s, s);
        }
        m
    };
    let k = (f_s - a_s) * a0;
    let mut c = [0.0_f32; 4];
    for (((c, &x), &i0), &n) in c.iter_mut().zip(&blended).zip(&initial.c).zip(&accum.c) {
        let ct = a_s.mul_add(x, k * i0);
        *c = (1.0 - f_s).mul_add(accum.a * n, ct) / ai;
    }
    // The spot planes, by the same recurrence (`Pass 239.0`). This wrote
    // `[0.0; MAX_SPOTS]` until then -- the "knockout groups drop spot ink"
    // approximation, silent because the initial backdrop carried no spot
    // planes to notice the loss against. `blend_spots` already carries
    // §11.7.4.2's rule that only separable, white-preserving modes reach a
    // spot plane; here it is applied against the group's INITIAL backdrop
    // exactly as the process channels are.
    let blended_s = blend_spots(blend, initial, source, a0);
    let mut sp = [0.0_f32; MAX_SPOTS];
    for (((sp, &x), &i0), &n) in sp.iter_mut().zip(&blended_s).zip(&initial.s).zip(&accum.s) {
        let st = a_s.mul_add(x, k * i0);
        *sp = (1.0 - f_s).mul_add(accum.a * n, st) / ai;
    }
    (PixelCmyk { c, s: sp, a: ai }, ag)
}

/// **§11.4.4's backdrop removal, in a subtractive space.**
///
/// The subtractive twin of `pdfcer-render`'s additive `remove_backdrop`. Note it needs **no**
/// complement: §11.4.4's correction is a linear extrapolation, and
/// complementing both operands of `x + k·(x − y)` and complementing the
/// result again is the identity. Only `B()` sits on the wrong side of the
/// complement, which is §11.3.6's own observation and is why
/// [`Blend::apply_subtractive`] is the only place the round trip happens.
#[must_use]
pub fn remove_backdrop_cmyk(group: PixelCmyk, initial: PixelCmyk, group_alpha: f32) -> [f32; 4] {
    let a0 = initial.a.clamp(0.0, 1.0);
    let agn = group_alpha.clamp(0.0, 1.0);
    if a0 <= 0.0 || agn <= 0.0 {
        return group.c;
    }
    let k = a0.mul_add(-1.0, a0 / agn);
    let mut out = [0.0_f32; 4];
    for ((o, &g), &b) in out.iter_mut().zip(&group.c).zip(&initial.c) {
        *o = k.mul_add(g - b, g);
    }
    out
}

/// [`remove_backdrop_cmyk`] on the spot planes: §11.4.4's
/// `C_n + (C_n − C_0)·(α_0/α_gn − α_0)` applied to each plane, a spot
/// colorant being one more subtractive component (§11.7.3).
#[must_use]
pub fn remove_backdrop_spots(
    group: PixelCmyk,
    initial: PixelCmyk,
    group_alpha: f32,
) -> [f32; MAX_SPOTS] {
    let a0 = initial.a.clamp(0.0, 1.0);
    let agn = group_alpha.clamp(0.0, 1.0);
    let k = if a0 <= 0.0 || agn <= 0.0 {
        0.0
    } else {
        a0.mul_add(-1.0, a0 / agn)
    };
    let mut out = [0.0_f32; MAX_SPOTS];
    for ((o, &n), &i0) in out.iter_mut().zip(&group.s).zip(&initial.s) {
        *o = k.mul_add(n - i0, n);
    }
    out
}

impl PixelCmyk {
    /// The ink this pixel puts on the sheet: each tint times the alpha,
    /// which is the page group composited over a medium of no ink
    /// (ISO 32000-2 §11.4.7). Returns `(process, spots)`.
    #[must_use]
    pub fn landed(self) -> ([f32; 4], [f32; MAX_SPOTS]) {
        let a = self.a.clamp(0.0, 1.0);
        (self.c.map(|t| t * a), self.s.map(|t| t * a))
    }
}

/// One overprinting paint at one pixel (§11.7.4.3): each process component
/// takes `rules[i].apply(backdrop, source)`, each spot plane with
/// `Some(tint)` competes with that tint and a `None` plane keeps the
/// backdrop, and the result is mixed into the backdrop by `t` (coverage
/// times constant alpha). Alpha accumulates as `Union(α_b, t)`.
#[must_use]
pub fn overprint_pixel(
    before: PixelCmyk,
    rules: [crate::overprint::ComponentRule; 4],
    source: [f32; 4],
    spots: [Option<f32>; MAX_SPOTS],
    t: f32,
) -> PixelCmyk {
    let mut out = [0.0_f32; 4];
    for (((o, rule), &b), &s) in out.iter_mut().zip(&rules).zip(&before.c).zip(&source) {
        *o = rule.apply(b, s).clamp(0.0, 1.0);
    }
    let mut mixed = [0.0_f32; 4];
    for ((m, &o), &b) in mixed.iter_mut().zip(&out).zip(&before.c) {
        *m = t.mul_add(o - b, b);
    }
    let mut spot_out = before.s;
    for (slot, stated) in spot_out.iter_mut().zip(spots.iter()) {
        if let Some(tint) = *stated {
            *slot = t.mul_add(tint - *slot, *slot);
        }
    }
    PixelCmyk {
        c: mixed,
        s: spot_out,
        a: t.mul_add(1.0 - before.a, before.a),
    }
}

//! # Device colour-space conversion (ISO 32000-1 §8.6.4)
//!
//! The single place in pdfcer where a `DeviceCMYK` colour becomes an sRGB
//! colour. Everything that paints, inspects, or records a CMYK colour — the
//! `k`/`K` operators, `DeviceCMYK` image samples, `ICCBased /N 4` fallbacks,
//! and `pdfcer-core`'s decomposed-object colour record — routes through
//! [`cmyk_to_srgb`]. That is a deliberate structural constraint, not tidiness:
//! two conversions that disagree show up as a filled rectangle and an image of
//! the "same" CMYK rendering as visibly different colours in one document.
//!
//! `DeviceGray` and `DeviceRGB` are here too ([`gray_to_srgb`],
//! [`rgb_to_srgb`]) so the three device spaces read as one family, but they
//! are trivial: §8.6.4.2 makes gray 0 = black / 1 = white, and §8.6.4.3 makes
//! RGB components direct intensities.
//!
//! ---
//!
//! ## 1. What the spec mandates: nothing
//!
//! §8.6.4.4 defines `DeviceCMYK` as four components, "each in the range 0.0
//! (zero concentration) to 1.0 (maximum concentration)", and describes the
//! space as **subtractive**. That is the entire normative content. The
//! standard specifies **no** conversion from CMYK to a display's RGB, and no
//! colorimetry for the inks: §8.6.4's whole point is that device colour spaces
//! are *device-dependent* — the same operands mean whatever the output device
//! makes of them.
//!
//! This is the finding that shapes the rest of this module. **There is no
//! "correct" answer to be spec-compliant about.** A conversion here cannot be
//! validated against the standard because the standard declines to have an
//! opinion. It can only be compared with what other implementations chose.
//!
//! ## 2. So what does everyone else do? They choose.
//!
//! - **Acrobat** resolves untagged `DeviceCMYK` through a *user-configurable
//!   working-space ICC profile*, defaulting out of the box to **U.S. Web
//!   Coated (SWOP) v2** (sourced in `Acrobat_Features/
//!   prepress__colour_management_and_icc.md`, attributed to Dov Isaacs,
//!   formerly of Adobe Acrobat Engineering). A house default, changeable by
//!   any user, with the single documented exception that a PDF/X file's own
//!   `/OutputIntents` profile is substituted automatically.
//! - **pdfium** ships a fixed calibrated lookup table (`AdobeCMYK_to_sRGB1`)
//!   in the same SWOP-derived family, with no configuration at all.
//! - **pdfcer, before this module**, used the naive additive
//!   `1 − min(1, x + k)`, which is not a colour model of anything — it is what
//!   you write when you need *a* formula and have no data.
//!
//! **pdfcer is therefore choosing, not matching.** This module makes a house
//! choice of the same kind Acrobat makes, and it should never be described as
//! "colorimetrically correct" — that claim has no referent for an untagged
//! device colour. What it *is*: a SWOP-family rendering, which is what both
//! Acrobat's default and pdfium's fixed table are, reached by converging on
//! pdfium because pdfium is the reference this project already measures
//! against (`tools/render-parity`, decision 010).
//!
//! ## 3. The model: quadrilinear interpolation over a fitted node grid
//!
//! A uniform **6 × 6 × 6 × 6** grid of sRGB nodes spans the CMYK unit
//! hypercube (`cmyk_table::NODES`, 1,296 entries). A conversion locates the
//! cell containing (c, m, y, k) and blends that cell's 16 corner nodes by the
//! product of the four per-axis fractions — quadrilinear interpolation, the
//! four-dimensional generalisation of the bilinear filter.
//!
//! Why interpolate a grid rather than evaluate a formula? Because the shape
//! being modelled is not formula-shaped. Real ink behaviour is dominated by
//! *where the ink combinations land* — solid cyan is `(0, 174, 239)`, not the
//! `(0, 255, 255)` an additive formula assumes; solid black ink alone is
//! `(35, 31, 32)`, not `(0, 0, 0)`; C+M+Y solid is `(54, 53, 57)`, a warm
//! near-black rather than black. Those are measured facts about a printing
//! condition, not consequences of an algebraic rule, and no closed form
//! recovers them without carrying the same facts as coefficients. See §6 for
//! the two data-free closed forms that were tried and how far short they fall.
//!
//! ### Cost — the property that made this affordable
//!
//! **Per-pixel work is independent of the table size.** A quadrilinear lookup
//! touches exactly 16 nodes whatever `L` is, so a bigger, more accurate table
//! costs memory and nothing else. The implementation collapses one axis at a
//! time — 8 lerps along k, then 4, then 2, then 1 — which is 15 lerps (45
//! multiply-adds) rather than the 16 four-factor weight products plus 48
//! multiply-accumulates the equivalent weighted sum would cost, and it
//! addresses the corners by constant stride off one base index instead of
//! recomputing a mixed-radix index per corner. The table is 15.5 KB, read-only
//! and shared. There is no allocation, no branch on colour value, and no
//! per-document setup.
//!
//! **Measured**, end to end on a deliberately incoherent 4-megapixel full-page
//! `DeviceCMYK` image (worst case for cache locality), whole-page render,
//! median of five: naive **290 ms** → 16-corner weighted sum ~610 ms →
//! **this, 495 ms**. That is ≈ +51 ns per converted sample, and +71 % on a page
//! that is nothing but a full-bleed CMYK raster — a whole-page figure that also
//! carries 16 MB of Flate inflation and a 4-megapixel PNG encode, so the
//! conversion's share of a realistic page is smaller still. For **vector**
//! content — CAD exports, the
//! motivating case — the conversion runs once per `k`/`K` operator rather than
//! per pixel and the cost is unmeasurable. Full numbers and the remaining
//! headroom (a one-entry memo, which would collapse the cost on flat art and
//! do nothing for photographs) in `tools/cmyk-calibration/README.md` §6.
//!
//! ### Structural exactness at the corners that can be named
//!
//! All 16 hypercube corners — the solid inks and their overprints — are the
//! *measured* values rather than the least-squares estimate, because they are
//! what someone checks when they check whether the conversion is right, and
//! the unsnapped fit misses solid cyan by ~3/255 while trying to also fit the
//! cell interior around it. `cmyk(0,0,0,0)` is additionally forced to exactly
//! white and `cmyk(1,1,1,1)` to exactly black: paper white is how producers
//! paint an opaque background and an off-by-one there is a visible pale
//! rectangle on a white page, and the darkest expressible value must not float
//! above zero. Snapping costs nothing measurable — see
//! `tools/cmyk-calibration/README.md` §3.2.
//!
//! ## 4. Where the numbers came from
//!
//! `tools/cmyk-calibration/cmyk_probe.py` emits a synthetic PDF holding one
//! `k`-filled rectangle per point of a 9-level (6,561-point) CMYK lattice,
//! renders it with pdfium at one device pixel per PDF unit, and reads back the
//! centre pixel of every patch. `fit.py` then solves for the node colours by
//! linear least squares — the interpolation weights are the design matrix, so
//! the optimum is closed-form: no iterations, no seed, no tuning knob (which
//! is what keeps this clear of the project's W14 rule against tuning a
//! threshold until a number turns green).
//!
//! **Provenance matters here and is deliberately narrow.** The data is
//! *measured render output*, not an extract from an ICC profile. pdfcer reads,
//! ships, and redistributes no profile — which is exactly what keeps this free
//! of the redistribution terms that attach to most real-world CMYK profiles,
//! including the SWOP v2 whose curve data is proprietary and unpublished
//! (rule 13, `docs/LEGAL.md` §6).
//!
//! ## 5. Measured accuracy
//!
//! Against 4,000 uniformly-random CMYK points rendered by pdfium — a set the
//! fit never saw, chosen random rather than a lattice because a lattice
//! validation set silently coincides with the model's own grid nodes whenever
//! the resolutions share a divisor:
//!
//! | conversion | mean Δ | p95 Δ | max Δ | pixels >8/255 |
//! |---|---|---|---|---|
//! | naive additive (before) | 32.5 | 77 | 100 | **97.0 %** |
//! | multiplicative `(1−x)(1−k)` | 14.9 | 50 | 100 | 90.9 % |
//! | **this module** | **1.16** | **5.8** | **17** | **2.6 %** |
//!
//! Δ is per-channel absolute difference in 0–255; "pixels >8/255" is decision
//! 006 §3.7's headline metric, the fraction of samples where some channel
//! differs by more than 8.
//!
//! ## 6. What was rejected, and why it is recorded here
//!
//! - **Naive additive `1 − min(1, x + k)`** — the previous behaviour. Not a
//!   model; 97 % of samples off by more than 8/255. It survived as a
//!   selectable `CmykIntent` variant until 2026-08-28 so an operator could
//!   reproduce a pre-calibration pdfcer export, and was deleted by operator
//!   ruling once that population had aged out (`Pass 153.0`). It stays in
//!   this rejected list, which is what this section is for.
//! - **Multiplicative `(1 − x)(1 − k)`** — the other data-free closed form,
//!   and a genuine improvement (mean 32.5 → 14.9) for zero bytes. Still 91 %
//!   of samples beyond 8/255, because it inherits the additive form's
//!   assumption that the inks are ideal sRGB primaries. Kept in `fit.py` as a
//!   scored baseline so the "how far can arithmetic alone get?" question stays
//!   answered rather than re-litigated.
//! - **Extracting a table from a real CMYK ICC profile** — the accuracy
//!   ceiling for a fixed choice, and the licensing trap. Most redistributable
//!   CMYK profiles carry terms of their own (the ECI profiles permit
//!   redistribution only with their licence attached and no fee); SWOP v2's
//!   data is proprietary and not published as an algorithm at all. Not
//!   pdfcer's call to make alone.
//! - **Parsing embedded `ICCBased` profiles with a real CMM** — the only path
//!   that eventually matches Acrobat rather than approximating a default. It
//!   solves a *different* problem: an arbitrary embedded profile, not the
//!   untagged `DeviceCMYK` this module covers, which by definition has no
//!   profile to parse. Needs a CMM dependency and its own decision.
//!
//! ## 7. Re-targeting
//!
//! The grid is data, and the tool that produced it is committed. Pointing
//! pdfcer at a different printing condition means re-running the probe against
//! a different reference and re-emitting the table — no code change. If pdfcer
//! ever gains a user-selectable working CMYK space (Acrobat's model), this
//! table becomes its default entry rather than being replaced.

mod cmyk_table;
mod intent;

pub use intent::{RenderingIntent, image_intent};

use cmyk_table::{GRID_L, NODES};

use crate::settings::CmykIntent;

/// Convert `DeviceGray` (§8.6.4.2) to sRGB: 0.0 = black, 1.0 = white.
///
/// Note the polarity trap this exists to keep visible — `DeviceGray` 0.0 is
/// **black**, the same direction as RGB, whereas `DeviceCMYK` 0.0 is *no ink*,
/// i.e. white. The two device spaces run opposite ways.
///
/// # Examples
///
/// ```
/// assert_eq!(pdfcer_core::color::gray_to_srgb(1.0), [1.0, 1.0, 1.0]);
/// assert_eq!(pdfcer_core::color::gray_to_srgb(0.0), [0.0, 0.0, 0.0]);
/// ```
#[must_use]
pub fn gray_to_srgb(v: f32) -> [f32; 3] {
    let v = v.clamp(0.0, 1.0);
    [v, v, v]
}

/// Convert `DeviceRGB` (§8.6.4.3) to sRGB — component-wise identity, clamped.
///
/// pdfcer treats the `DeviceRGB` components as already being sRGB. That is the
/// same class of house choice §2 of the module docs describes (the spec calls
/// `DeviceRGB` device-dependent too), but an uncontroversial one: every
/// mainstream viewer displays untagged `DeviceRGB` verbatim.
///
/// # Examples
///
/// ```
/// assert_eq!(pdfcer_core::color::rgb_to_srgb(0.25, 0.5, 2.0), [0.25, 0.5, 1.0]);
/// ```
#[must_use]
pub fn rgb_to_srgb(r: f32, g: f32, b: f32) -> [f32; 3] {
    [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)]
}

/// Convert `DeviceCMYK` (§8.6.4.4) to sRGB through the calibrated node grid.
///
/// Components are ink concentrations in 0.0–1.0 and are clamped, so an
/// out-of-range operand from a malformed content stream cannot index outside
/// the table. The result is always in 0.0–1.0 per channel.
///
/// This is a **house choice**, not a spec-mandated or colorimetrically
/// "correct" conversion — see the module documentation §1–§2 for why no such
/// thing exists for an untagged device colour, and §5 for the measured
/// agreement with pdfium.
///
/// # Algorithm
///
/// Locate the grid cell containing the point, then blend its 16 corner nodes
/// by the product of the per-axis interpolation fractions. `t = x * (L − 1)`
/// gives a position in node units; the base index is `floor(t)` clamped to
/// `L − 2` so that `x == 1.0` lands on the last *cell* with fraction 1.0
/// rather than running off the end of the grid.
///
/// # Examples
///
/// ```
/// use pdfcer_core::color::cmyk_to_srgb;
///
/// // No ink at all is exactly paper white, and four-colour solid is exactly
/// // black — both pinned, because both are directly observable on a page.
/// assert_eq!(cmyk_to_srgb(0.0, 0.0, 0.0, 0.0), [1.0, 1.0, 1.0]);
/// assert_eq!(cmyk_to_srgb(1.0, 1.0, 1.0, 1.0), [0.0, 0.0, 0.0]);
///
/// // Solid black INK alone is a warm near-black, not #000000 — the single
/// // most visible consequence of calibrating this conversion.
/// let k100 = cmyk_to_srgb(0.0, 0.0, 0.0, 1.0);
/// assert!(k100[0] > 0.10 && k100[0] < 0.18, "{k100:?}");
/// ```
#[must_use]
pub fn cmyk_to_srgb(c: f32, m: f32, y: f32, k: f32) -> [f32; 3] {
    let (bc, fc) = cell_position(c);
    let (bm, fm) = cell_position(m);
    let (by, fy) = cell_position(y);
    let (bk, fk) = cell_position(k);

    // Address the cell's 16 corners by STRIDE rather than by recomputing a
    // mixed-radix index per corner. `NODES` is row-major in (c, m, y, k), so
    // stepping one node along k is +1, along y is +L, along m is +L², along c
    // is +L³. `origin` is the cell's near corner; every other corner is
    // `origin` plus a subset of the four strides, which the compiler turns
    // into constant offsets off one base pointer.
    const SK: usize = 1;
    const SY: usize = GRID_L;
    const SM: usize = GRID_L * GRID_L;
    const SC: usize = GRID_L * GRID_L * GRID_L;
    let origin = bc * SC + bm * SM + by * SY + bk;

    // Stage 1 — collapse the k axis, leaving the 8 corners of a (c, m, y)
    // cube. `edge(dc, dm, dy)` is the k-interpolated colour at the cell corner
    // offset by those three 0/1 steps.
    let edge = |dc: usize, dm: usize, dy: usize| {
        let at = origin + dc * SC + dm * SM + dy * SY;
        lerp(node(at), node(at + SK), fk)
    };

    // Stage 2 — collapse y, then m, then c. Fifteen lerps in total (45
    // multiply-adds), against the 16 four-factor weight products plus 48
    // multiply-accumulates an equivalent weighted sum would cost, with every
    // intermediate staying in registers. It is also the form that makes the
    // structure legible: an interpolation over four independent axes, one at a
    // time.
    let y00 = lerp(edge(0, 0, 0), edge(0, 0, 1), fy);
    let y01 = lerp(edge(0, 1, 0), edge(0, 1, 1), fy);
    let y10 = lerp(edge(1, 0, 0), edge(1, 0, 1), fy);
    let y11 = lerp(edge(1, 1, 0), edge(1, 1, 1), fy);
    let m0 = lerp(y00, y01, fm);
    let m1 = lerp(y10, y11, fm);
    let [r, g, b] = lerp(m0, m1, fc);

    // Every node is in [0, 1] and a lerp of two such values stays in [0, 1],
    // so this is already in range up to float rounding; the clamp is
    // belt-and-braces for the last ulp.
    [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)]
}

/// `DeviceCMYK` → sRGB under an explicit operator intent
/// (ISO 32000-1 §8.6.4.4).
///
/// [`cmyk_to_srgb`] is this function at [`CmykIntent::Calibrated`], and
/// stays the entry point for callers that have no intent to pass.
///
/// # Why an intent exists at all
///
/// §8.6.4.4 specifies **no** conversion — `DeviceCMYK` is device-dependent
/// by definition, and the standard is silent by design rather than by
/// omission. Acrobat's own answer is a user-configurable working-space
/// profile. There is therefore no correct conversion to implement, only a
/// choice to make, and under the operator directive of 2026-08-08 a choice
/// the standard leaves open becomes a setting rather than a hard-coded
/// answer — defaulted to what is usually followed, which here means
/// agreement with the dominant reader.
///
/// # The three answers
///
/// - [`CmykIntent::Calibrated`] — the measured table. Solid black ink is a
///   warm near-black.
/// - [`CmykIntent::NeutralBlack`] — identical **except** where
///   `C = M = Y = 0`, which becomes a neutral `1 − K`. This is a
///   deliberate departure from the reference, for line art that strokes in
///   pure K and is expected to be truly black. It changes *only* the
///   pure-K axis; every mixed colour is untouched, so a drawing's black
///   lines go true black while any photographic content on the same page
///   keeps its calibrated rendering.
///
/// # Examples
///
/// ```
/// use pdfcer_core::color::cmyk_to_srgb_with;
/// use pdfcer_core::settings::CmykIntent;
///
/// // The whole point of the alternative: pure K becomes true black.
/// assert_eq!(
///     cmyk_to_srgb_with(CmykIntent::NeutralBlack, 0.0, 0.0, 0.0, 1.0),
///     [0.0, 0.0, 0.0]
/// );
///
/// // And a colour that is NOT on the pure-K axis is left calibrated, so
/// // the setting cannot quietly change a photograph.
/// let mixed = (0.5, 0.2, 0.1, 0.3);
/// assert_eq!(
///     cmyk_to_srgb_with(CmykIntent::NeutralBlack, mixed.0, mixed.1, mixed.2, mixed.3),
///     cmyk_to_srgb_with(CmykIntent::Calibrated, mixed.0, mixed.1, mixed.2, mixed.3)
/// );
/// ```
#[must_use]
pub fn cmyk_to_srgb_with(intent: CmykIntent, c: f32, m: f32, y: f32, k: f32) -> [f32; 3] {
    match intent {
        CmykIntent::Calibrated => cmyk_to_srgb(c, m, y, k),
        CmykIntent::NeutralBlack => {
            // "Pure K" is tested on the three chromatic channels only, and
            // with `<= 0.0` rather than an epsilon: a content stream that
            // says `0 0 0 1 k` writes exact zeros, and a stream that says
            // `0.001 0 0 1 k` is asking for a tinted black and should get
            // one. A tolerance here would silently capture near-neutrals
            // that the author distinguished on purpose.
            let chromatic = |v: f32| !(v.is_nan() || v <= 0.0);
            if chromatic(c) || chromatic(m) || chromatic(y) {
                cmyk_to_srgb(c, m, y, k)
            } else {
                let k = if k.is_nan() { 0.0 } else { k.clamp(0.0, 1.0) };
                let v = 1.0 - k;
                [v, v, v]
            }
        }
    }
}

/// One axis of [`cmyk_to_srgb`]: the base node index of the enclosing cell and
/// the fraction into it.
///
/// `min(L - 2)` is what makes the closed interval work — without it an exact
/// 1.0 would produce base `L - 1`, and the far corner would index `L`, off the
/// end of the axis. With it, 1.0 lands on the last *cell* at fraction 1.0.
#[inline]
fn cell_position(v: f32) -> (usize, f32) {
    // NaN is handled BEFORE the clamp and not by it: `f32::clamp` returns NaN
    // for a NaN input (both of its comparisons are false), which would then
    // poison every interpolation weight and hand the caller a NaN colour that
    // surfaces as an invisible or garbage pixel far from here. A non-numeric
    // operand means "no ink".
    let v = if v.is_nan() { 0.0 } else { v };
    let t = v.clamp(0.0, 1.0) * (GRID_L - 1) as f32;
    // `t` is finite and in [0, L-1] after the clamp, so the cast is exact and
    // cannot saturate.
    let i0 = (t as usize).min(GRID_L - 2);
    (i0, t - i0 as f32)
}

/// Fetch a node without a panicking index.
///
/// Every call site computes `i` from clamped axis positions and 0/1 strides,
/// so it is provably in range — but a renderer must not be one arithmetic
/// mistake away from a panic on a page it was handed, and the project's
/// `indexing_slicing` lint says so structurally. The unreachable arm returns
/// black rather than an arbitrary value: a wrong-but-dark pixel is the least
/// misleading failure a colour lookup can have.
#[inline]
fn node(i: usize) -> [f32; 3] {
    match NODES.get(i) {
        Some(v) => *v,
        None => [0.0, 0.0, 0.0],
    }
}

/// Component-wise linear interpolation between two colours.
#[inline]
fn lerp([ar, ag, ab]: [f32; 3], [br, bg, bb]: [f32; 3], t: f32) -> [f32; 3] {
    [ar + (br - ar) * t, ag + (bg - ag) * t, ab + (bb - ab) * t]
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

    /// 8-bit rounding of a converted colour, so expectations read in the same
    /// units the probe TSVs and the parity harness use.
    fn u8s(rgb: [f32; 3]) -> [u8; 3] {
        [
            (rgb[0] * 255.0).round() as u8,
            (rgb[1] * 255.0).round() as u8,
            (rgb[2] * 255.0).round() as u8,
        ]
    }

    /// The two corners callers can SEE being wrong (module docs §3).
    #[test]
    fn paper_white_and_four_colour_solid_are_exact() {
        assert_eq!(cmyk_to_srgb(0.0, 0.0, 0.0, 0.0), [1.0, 1.0, 1.0]);
        assert_eq!(cmyk_to_srgb(1.0, 1.0, 1.0, 1.0), [0.0, 0.0, 0.0]);
    }

    /// Spot-check the primaries against the MEASURED pdfium values recorded in
    /// `tools/cmyk-calibration/out/fit-pdfium.tsv`. These are the landmarks a
    /// bad refit, a transposed index order, or a broken interpolation would
    /// move first — solid cyan reading `(0,255,255)` again means the naive
    /// formula has crept back; reading solid magenta's value means the index
    /// order got shuffled.
    ///
    /// Tolerance is 6/255, comfortably above the model's own p95 of 5.8 at
    /// arbitrary points but far below the 60–90 the naive formula misses these
    /// by, so the test discriminates the thing it is for.
    #[test]
    fn primaries_match_measured_reference_within_tolerance() {
        let cases: &[([f32; 4], [u8; 3], &str)] = &[
            ([1.0, 0.0, 0.0, 0.0], [0, 174, 239], "solid cyan"),
            ([0.0, 1.0, 0.0, 0.0], [237, 2, 140], "solid magenta"),
            ([0.0, 0.0, 1.0, 0.0], [255, 241, 1], "solid yellow"),
            ([0.0, 0.0, 0.0, 1.0], [35, 31, 32], "solid black ink"),
            ([1.0, 1.0, 0.0, 0.0], [46, 48, 146], "cyan+magenta (blue)"),
            ([1.0, 0.0, 1.0, 0.0], [0, 165, 79], "cyan+yellow (green)"),
            ([0.0, 1.0, 1.0, 0.0], [238, 29, 35], "magenta+yellow (red)"),
            ([1.0, 1.0, 1.0, 0.0], [54, 53, 57], "three-ink solid"),
            ([0.0, 0.0, 0.0, 0.5], [147, 149, 152], "50% black"),
        ];
        for &([c, m, y, k], want, label) in cases {
            let got = u8s(cmyk_to_srgb(c, m, y, k));
            for ch in 0..3 {
                let d = (i32::from(got[ch]) - i32::from(want[ch])).abs();
                assert!(
                    d <= 6,
                    "{label}: got {got:?} want {want:?} (Δ{d} on ch{ch})"
                );
            }
        }
    }

    /// Adding black ink never meaningfully lightens a colour, on any of the
    /// 1,296 grid-node ink combinations. A monotonicity break would mean the
    /// node order is scrambled even if the corner spot-checks happen to pass.
    ///
    /// The 5/255 slack is not padding to make the test pass — it is a property
    /// of the reference. Channels sitting on their floor wobble below 8-bit
    /// quantisation in the measured data (solid yellow's blue channel is
    /// `1/255`, and yellow + 20 % K measures `2/255`), so demanding strict
    /// monotonicity would assert something the ground truth itself does not
    /// satisfy. The tolerance is an order of magnitude below any divergence
    /// that would indicate a scrambled table.
    #[test]
    fn increasing_black_never_lightens() {
        for ci in 0..6 {
            for mi in 0..6 {
                for yi in 0..6 {
                    let mut prev = [1.0f32; 3];
                    for ki in 0..6 {
                        let got = cmyk_to_srgb(
                            ci as f32 / 5.0,
                            mi as f32 / 5.0,
                            yi as f32 / 5.0,
                            ki as f32 / 5.0,
                        );
                        for ch in 0..3 {
                            assert!(
                                got[ch] <= prev[ch] + 5.0 / 255.0,
                                "c{ci} m{mi} y{yi} k{ki} ch{ch}: {} > {}",
                                got[ch],
                                prev[ch]
                            );
                        }
                        prev = got;
                    }
                }
            }
        }
    }

    /// Out-of-range and non-finite operands clamp instead of indexing out of
    /// the table. A content stream is untrusted input; `-1e30 0 0 0 k` is a
    /// legal thing to write and must not panic.
    #[test]
    fn out_of_range_operands_clamp() {
        assert_eq!(cmyk_to_srgb(-5.0, -5.0, -5.0, -5.0), [1.0, 1.0, 1.0]);
        assert_eq!(cmyk_to_srgb(9.0, 9.0, 9.0, 9.0), [0.0, 0.0, 0.0]);
        assert_eq!(cmyk_to_srgb(f32::NAN, 0.0, 0.0, 0.0), [1.0, 1.0, 1.0]);
        let inf = cmyk_to_srgb(f32::INFINITY, 0.0, 0.0, f32::NEG_INFINITY);
        assert!(inf.iter().all(|v| (0.0..=1.0).contains(v)), "{inf:?}");
    }

    /// Every output stays in gamut for a dense sweep — the invariant the
    /// interpolation's "weights sum to 1" argument rests on.
    #[test]
    fn output_is_always_in_gamut() {
        for i in 0..=10 {
            for j in 0..=10 {
                for l in 0..=10 {
                    for n in 0..=10 {
                        let rgb = cmyk_to_srgb(
                            i as f32 / 10.0,
                            j as f32 / 10.0,
                            l as f32 / 10.0,
                            n as f32 / 10.0,
                        );
                        assert!(rgb.iter().all(|v| (0.0..=1.0).contains(v)), "{rgb:?}");
                    }
                }
            }
        }
    }

    /// The gray ramp is the axis a CAD export and a scanned page spend most of
    /// their ink on, and the one whose behaviour changed most visibly: pdfcer
    /// used to map `0 0 0 k` to a pure neutral `1 − k`. It is now a slightly
    /// warm, slightly lifted ramp, matching the reference.
    #[test]
    fn black_only_ramp_is_the_reference_ramp_not_a_neutral_one() {
        // 25 % K: reference (199, 200, 202) vs the old naive (191, 191, 191).
        let q = u8s(cmyk_to_srgb(0.0, 0.0, 0.0, 0.25));
        assert!((i32::from(q[0]) - 199).abs() <= 6, "{q:?}");
        assert!(q[2] >= q[0], "the ramp is cool, not neutral: {q:?}");
    }
}

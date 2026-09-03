//! # Shadings — gradient fills (ISO 32000-1 §8.7.4)
//!
//! The model behind the `sh` operator (§8.7.4.2, Table 77) and behind a
//! `PatternType 2` shading pattern (§8.7.4.1, Table 76). Spec sources, all
//! in the PDF-spec RAG at `D:\Dev\Rag-Specialized\PDF_Spec\`:
//!
//! | Clause | RAG file | What it governs here |
//! |---|---|---|
//! | §8.7.2, §8.7.4.1–.2, Table 76/77/78 | `iso32000__s__8.7.md` | pattern space, the `sh` operator, the entries common to every shading dictionary |
//! | §7.10 | `iso32000__s__7.10.md` | the `/Function` evaluator |
//! | §8.6 | `iso32000__s__8.6.md` | `/ColorSpace` |
//!
//! ## What this slice does, and what it deliberately does not
//!
//! **It paints the analytic types.** This module builds the *model* --
//! it resolves a shading dictionary, classifies it, loads its colour space
//! and its function, pre-samples the colour ramp, and **reports precisely
//! what it found** -- and then rasterises types 1, 2 and 3.
//!
//! This paragraph opened "**It does not paint.** ... Painting lands in the
//! next slice" for as long as it took that slice to land and longer. The
//! sentence was true when written and became the most misleading line in
//! the file, because it is the first thing a reader of this module sees
//! and it contradicts `ShadingDiagnostics::painted` forty lines of struct
//! away. Kept visible rather than quietly swapped: a module header is a
//! claim about the module, and claims that outlive their subject are what
//! standing rule `R212` is about.
//!
//! That paragraph has now been overtaken twice. The **mesh** types (4-7)
//! were "modelled and refused, never painted" until `Pass 125.0`; they are
//! now decoded and rasterised by [`crate::mesh`], which this module owns
//! the dictionary half of and delegates the stream half to.
//!
//! The geometry both halves need - 8.7.4.5, ISO 32000-1 Tables 79-86 - is
//! in the spec corpus in two files: `iso32000__s__8.7.4.5__analytic.md`
//! for types 1/2/3 (labels SH1-SH45) and
//! `iso32000__s__8.7.4.5__mesh.md` for types 4/5/6/7 (labels MSH1-MSH36).
//! The corpus index's "do not answer from recall" marker on the mesh
//! family was retired by the second of those. Project rule 1:
//! spec-governed geometry is not written from training-data recall, and
//! neither half of this feature was.
//!
//! ### Three things the corpus corrected that this module had wrong
//!
//! Recorded rather than silently fixed, because each is a mistake a
//! reasonable reader would make again.
//!
//! 1. **The table numbers are off by one from the obvious guess.** See
//!    [`Geometry`].
//! 2. **There is no quadratic in the radial clause.** A whole-document
//!    search for "quadratic" and "discriminant" returns **zero hits in
//!    both editions**. §8.7.4.5.4 specifies a *painting process* — paint
//!    every blend circle in order of increasing parameter, opaquely, so a
//!    point covered by several takes the colour of the last one painted,
//!    "corresponding to the greatest value of s". The familiar quadratic
//!    is an implementer's inversion of that process and **must not be
//!    written with a §8.7.4.5.4 citation beside it**: the clause supports
//!    the greatest-parameter rule, not the algebra.
//! 3. **The axial parameter is `x′`, not `s`.** `s` is the *radial*
//!    clause's variable. There is no `y′` at all, because an axial shading
//!    is constant perpendicular to its axis.
//!
//! ### Two ambiguities the painting slice inherits
//!
//! - **A zero-length axial axis.** ISO 32000-1 is **silent** (the
//!   projection's denominator is zero); ISO 32000-2's Table 79 **adds
//!   "nothing shall be painted."** pdfcer paints nothing at every version —
//!   a later edition resolving an earlier silence is a forced default, not
//!   a free choice, so this is not a settings-register candidate.
//! - **Radial extension has no stopping rule of its own.** Extension at
//!   the larger end is bounded by `/BBox`, and `/BBox` is **optional**.
//!   That makes an unbounded extended radial a **hang risk**, not merely a
//!   wrong render, and the painting slice must bound it by the clip extent
//!   rather than trust the dictionary.
//!
//! ### Why a non-painting slice is worth shipping on its own
//!
//! Before it, a page full of gradients reported this and nothing else:
//!
//! ```text
//! deferred=52 … first distinct names: BDC, sh, BMC
//! ```
//!
//! An anonymous count. It cannot answer "how many gradients?", "which
//! types?", "would the next slice fix this page?" — and it cannot
//! distinguish a shading pdfcer will soon paint from a type 7 tensor-patch
//! mesh that is a much larger piece of work. After it, the same page
//! reports the shadings by type, by colour space, and by whether the model
//! loaded at all. That is the inventory the next slice is scoped from, and
//! it is the disclosure project rule 4 asks for in the meantime: *nothing
//! was painted here, and here is exactly what it was*.
//!
//! ## The two paint routes anchor differently — the thing most easily got wrong
//!
//! The same shading dictionary reaches the page two ways, and they use
//! **opposite** coordinate spaces. From `iso32000__s__8.7.md`:
//!
//! - **PM7** (§8.7.4.2 + Table 77): `sh` "applies the corresponding
//!   gradient fill directly to current user space", and "All coordinates in
//!   the shading dictionary are interpreted relative to the **current user
//!   space**." So `sh` is **CTM-relative**: a `cm` before it moves the
//!   gradient.
//! - **PM2/PM3** (§8.7.2 + NOTE 1): a pattern's `/Matrix` maps pattern
//!   space to "the **default (initial)** coordinate space of the page", and
//!   "Changes to the page's transformation matrix that occur within the
//!   page's content stream, such as rotation and scaling, **have no effect
//!   on the pattern**." So a `PatternType 2` fill is **base-CTM-relative**:
//!   a `cm` before it does *not* move the gradient.
//!
//! The standard states the contrast itself, in one parenthesis in Table 77.
//! Two more behavioural differences follow the same split and are recorded
//! on [`Shading::background`] and in [`PaintRoute`].
//!
//! ## Why pdfcer will evaluate shadings itself rather than use `tiny_skia`'s gradients
//!
//! Recorded here because it shapes the next slice and is not obvious.
//!
//! `tiny_skia::LinearGradient` is a faithful two-point model and would map
//! cleanly onto an axial shading. **`tiny_skia::RadialGradient` cannot
//! express a PDF radial shading in the general case**: its constructor
//! takes *one* radius, and its source says in terms *"Unlike Skia, we have
//! only the Focal radial gradient type"* — i.e. the start radius is fixed at
//! **0**. §8.7.4.5.4's `/Coords` is `[x0 y0 r0 x1 y1 r1]` with **both radii
//! free**, and the two configurations the clause describes routinely have
//! `r0 > 0`.
//!
//! The failure that would cause is silent and plausible: passing `r1` and
//! dropping `r0` renders *a* gradient, with the ramp starting at the centre
//! instead of at the inner circle. No error, no `None`.
//!
//! Neither does any `SpreadMode` express `/Extend [false false]`, which
//! means **nothing is painted** beyond the ends — `Pad` replicates the edge
//! colour instead.
//!
//! Using the crate for the cases it covers and hand-rolling the rest would
//! leave **two code paths for one feature**, exercised on different files,
//! and the one that runs less often is the one that rots. So: one path,
//! pdfcer's own, uniform across types 1–3 and extensible to 4–7. The cost is
//! a per-pixel evaluation, and [`ColorRamp`] is what makes that cheap —
//! see its docs. Full write-up in
//! `D:\dev\rag\rust\tiny_skia_radial_gradient_has_no_start_radius.md`.

use std::sync::Arc;

use pdfcer_core::function::PdfFunction;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::settings::CmykIntent;
use pdfcer_core::view::DocumentView;

use crate::color::{ColorDiagnostics, ColorSpace};
use crate::gstate::Rgb;

/// Entries in the pre-sampled colour ramp ([`ColorRamp`]).
///
/// # Why 256, and why a constant rather than a knob
///
/// §10.6.3 gives a renderer explicit latitude over subdivision granularity
/// (the `/SM` smoothness tolerance bounds the *error*, not the method), so
/// sampling the `/Function` into a table and interpolating between samples
/// is a sanctioned implementation, not a corner cut.
///
/// 256 is chosen because it is the point past which a further doubling
/// cannot change an 8-bit-per-channel output for a monotone ramp: the
/// destination is a `tiny_skia::Pixmap` with 8 bits per channel, so ramp
/// steps finer than 1/256 of the domain cannot produce a distinguishable
/// pixel. A knob would therefore expose a setting whose upper half has no
/// observable effect, which `docs/ARCHITECTURE.md`'s settings discipline
/// treats as worse than no knob.
///
/// It is **not** a fidelity ceiling for non-monotone functions — a type 4
/// PostScript calculator function can oscillate faster than the sample
/// rate, and such a ramp is undersampled here. That is disclosed
/// ([`ShadingDiagnostics::ramps_sampled`] counts every ramp built) rather
/// than silently assumed away, and the honest fix if it ever matters is
/// adaptive sampling, not a bigger constant.
pub const RAMP_SAMPLES: usize = 256;

/// Guard on `/Function` outputs, to bound a malformed file's cost.
///
/// A shading's function feeds a colour space, and no colour space pdfcer
/// resolves has more than a handful of components (`DeviceN` is the widest
/// realistic case). A file declaring thousands would otherwise make ramp
/// construction allocate proportionally.
const MAX_FUNCTION_OUTPUTS: usize = 32;

/// Which of the two paint routes a shading arrived by (§8.7.4.2 Table 77
/// versus §8.7.4.1 Table 76).
///
/// This is not cosmetic bookkeeping: the route decides the **coordinate
/// space** the shading's own geometry is expressed in, and it changes two
/// further behaviours. Carrying it as a type rather than a `bool` is
/// deliberate — a `bool` at a call site reads as "is_pattern", and the
/// three consequences below are not derivable from that name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaintRoute {
    /// The `sh` operator (§8.7.4.2, Table 77).
    ///
    /// - Coordinates are **current user space** — the CTM in effect when
    ///   `sh` executes. PM7.
    /// - `/Background` **is ignored**: "The `Background` entry, if present,
    ///   is ignored." (Table 77.)
    /// - The paint area is the **current clip region**, not a path: `sh`
    ///   "works without reference to the current colour in the graphics
    ///   state" and takes no path. §8.7.4.2 adds a `should` that it "be
    ///   applied only to bounded or geometrically defined shadings" —
    ///   on an unbounded one it fills the whole clip.
    ShOperator,
    /// A `PatternType 2` shading pattern named by `scn`/`SCN`
    /// (§8.7.4.1, Table 76).
    ///
    /// - Coordinates are **pattern space**: the pattern dictionary's own
    ///   `/Matrix` maps them to the *default* coordinate space of the
    ///   parent content stream, **not** the CTM at paint time. PM2/PM3.
    /// - `/Background` **applies** (Table 78: "applied only when the
    ///   shading is used as part of a shading pattern").
    /// - The paint area is the **path being filled or stroked**.
    /// - It does **not tile** (§8.7.4.2 NOTE) — a gradient inside a tiling
    ///   pattern is a `sh` invoked from a `PatternType 1` content stream,
    ///   which is a different structure entirely.
    ShadingPattern,
}

impl PaintRoute {
    /// Whether `/Background` (Table 78) is honoured on this route.
    ///
    /// Exists so the difference is asserted in one place rather than
    /// re-derived at each use, and so the Table 77 / Table 78 pair that
    /// establishes it is cited once.
    #[must_use]
    pub const fn honours_background(self) -> bool {
        matches!(self, Self::ShadingPattern)
    }
}

/// A shading's geometry — the `ShadingType`-specific half of its
/// dictionary (§8.7.4.5).
///
/// # Why the variants carry no evaluator yet
///
/// # ★ The table numbers, because they are off-by-one from the obvious guess
///
/// ISO 32000-1 numbers this family **78 common / 79 type 1 / 80 type 2 /
/// 81 type 3**, with the meshes at 82–84. The intuitive reading — "axial
/// is Table 79 because axial is the first interesting one" — is wrong by
/// one all the way down, and every citation in this module carried that
/// error until the spec corpus was consulted. **ISO 32000-2 renumbers the
/// whole family again** (77/78/79/80, with `sh` at Table 76); citations
/// here are to ISO 32000-1, and `iso32000__s__8.7.4.5__analytic.md` opens
/// with the edition-mapping table.
///
/// The mesh stream encodings (Tables 82–84) are still not in the corpus;
/// the analytic types now are. The raw dictionary values are
/// captured here because *parsing* them is governed by Table 78 and the
/// per-type tables' entry lists, which are settled — it is only their
/// **semantics** that this slice declines to guess at.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// `ShadingType 1` — function-based (§8.7.4.5.2, **Table 79**).
    FunctionBased {
        /// `/Domain`, default `[0 1 0 1]`.
        domain: [f32; 4],
        /// `/Matrix`, default identity — the domain-to-target mapping.
        /// Stored as the raw six numbers rather than a `tiny_skia::
        /// Transform` because it composes with the route's own matrix and
        /// converting twice invites a transposition bug.
        matrix: [f32; 6],
    },
    /// `ShadingType 2` — axial (§8.7.4.5.3, **Table 80**).
    Axial {
        /// `/Coords` — `[x0 y0 x1 y1]`, the axis endpoints. Required.
        coords: [f32; 4],
        /// `/Domain`, default `[0 1]`.
        domain: [f32; 2],
        /// `/Extend`, default `[false false]`.
        extend: [bool; 2],
    },
    /// `ShadingType 3` — radial (§8.7.4.5.4, **Table 81**).
    Radial {
        /// `/Coords` — `[x0 y0 r0 x1 y1 r1]`, two circles. Required.
        ///
        /// **Both radii are free.** See the module docs for why this rules
        /// out `tiny_skia::RadialGradient`.
        coords: [f32; 6],
        /// `/Domain`, default `[0 1]`.
        domain: [f32; 2],
        /// `/Extend`, default `[false false]`.
        extend: [bool; 2],
    },
    /// `ShadingType` 4, 5, 6 or 7 — a mesh shading whose geometry lives in
    /// a **stream**, not in the dictionary (§8.7.4.5.5–.8).
    ///
    /// Recognised and named, never approximated. These are a materially
    /// larger piece of work than types 1–3 — a bit-packed vertex stream
    /// with `/BitsPerCoordinate`, `/BitsPerComponent`, `/BitsPerFlag` and
    /// a `/Decode` array, plus Coons and tensor patch surfaces — and
    /// keeping them a distinct variant is what lets the inventory say how
    /// much of a corpus actually needs them before that work is scoped.
    Mesh {
        /// 4, 5, 6 or 7.
        shading_type: u8,
    },
}

impl Geometry {
    /// The `ShadingType` number this geometry came from (Table 78).
    #[must_use]
    pub const fn shading_type(&self) -> u8 {
        match self {
            Self::FunctionBased { .. } => 1,
            Self::Axial { .. } => 2,
            Self::Radial { .. } => 3,
            Self::Mesh { shading_type } => *shading_type,
        }
    }

    /// Whether pdfcer's *painting* slice is expected to handle this type.
    ///
    /// Separate from "did the model load", which is [`Shading`]'s business,
    /// and separate again from "is it paintable" - since `Pass 125.0` a
    /// mesh IS painted, by [`crate::mesh`]'s forward rasteriser rather than
    /// by this module's inverse-mapped pixel loop. This predicate answers
    /// only "does a closed-form parametric coordinate exist at every
    /// point?", which is what [`Geometry::param_at`] needs and what a mesh
    /// does not have.
    #[must_use]
    pub const fn is_analytic(&self) -> bool {
        matches!(
            self,
            Self::FunctionBased { .. } | Self::Axial { .. } | Self::Radial { .. }
        )
    }
}

/// A shading's `/Function` entry, in either of the two shapes §8.7.4.4
/// permits.
///
/// > The `/Function` entry accepts either a single function with *n*
/// > outputs, or an array of *n* functions each with one output, and the
/// > two forms are equivalent.
///
/// Modelled as one type with two variants rather than normalised to one
/// shape at parse time, because the arity mismatch a malformed file
/// produces is different in each case and pdfcer reports which.
#[derive(Debug, Clone)]
pub enum ShadingFunction {
    /// One function, *n* outputs.
    Single(Arc<PdfFunction>),
    /// *n* functions, one output each, in colour-component order.
    PerComponent(Vec<Arc<PdfFunction>>),
}

impl ShadingFunction {
    /// How many colour components this function set produces.
    #[must_use]
    pub fn outputs(&self) -> usize {
        match self {
            Self::Single(f) => f.outputs(),
            Self::PerComponent(fs) => fs.len(),
        }
    }

    /// Evaluate at parametric coordinate(s), appending components to `out`.
    ///
    /// Returns `false` if any constituent function failed, leaving `out` in
    /// an unspecified but safe state — the caller treats a failure as "this
    /// ramp entry has no colour" rather than substituting one.
    fn eval(&self, inputs: &[f64], out: &mut Vec<f64>) -> bool {
        out.clear();
        match self {
            Self::Single(f) => f.eval_into(inputs, out).is_ok(),
            Self::PerComponent(fs) => {
                let mut scratch = Vec::new();
                for f in fs {
                    if f.eval_into(inputs, &mut scratch).is_err() {
                        return false;
                    }
                    // Each is declared 1-out; take the first and tolerate a
                    // wider one rather than refusing, since a producer
                    // emitting a 3-out function in an n-function array is
                    // malformed in a way that is still unambiguous.
                    match scratch.first() {
                        Some(v) => out.push(*v),
                        None => return false,
                    }
                }
                true
            }
        }
    }
}

/// A colour ramp pre-sampled from a shading's `/Function` over its
/// parametric domain.
///
/// # Why pre-sample rather than evaluate per pixel
///
/// A `/Function` evaluation is not cheap — a type 0 sampled function
/// interpolates a stream, and a type 4 runs a PostScript calculator. A
/// full-page gradient at 150 DPI is millions of pixels, and evaluating per
/// pixel would run the calculator millions of times to produce at most 256
/// distinguishable 8-bit values.
///
/// Sampling once into [`RAMP_SAMPLES`] entries turns the inner loop into
/// arithmetic plus an index. §10.6.3's smoothness tolerance is what makes
/// this a sanctioned implementation rather than an approximation pdfcer
/// invented — see [`RAMP_SAMPLES`].
///
/// **The colour-space conversion is baked in too.** Each entry is already
/// sRGB, so the per-pixel path never touches [`ColorSpace::to_rgb`] — which
/// matters most for the spaces that are expensive per call: a `Separation`
/// or `DeviceN` ramp runs its `/tintTransform` 256 times instead of once
/// per pixel.
#[derive(Debug, Clone)]
pub struct ColorRamp {
    /// [`RAMP_SAMPLES`] colours, evenly spaced across the parametric
    /// domain. An entry is `None` where the function or the colour-space
    /// conversion failed at that sample — recorded rather than filled in,
    /// so a partially-broken function does not silently gain invented
    /// colours at the broken end.
    samples: Vec<Option<Rgb>>,
    /// The **authored colorants** for the same samples, when the shading's
    /// colour space has any — see [`crate::color::ColorSpace::to_cmyk`].
    ///
    /// # Why the ramp carries two answers instead of one
    ///
    /// [`Self::samples`] is what the shading looks like; this is what it
    /// asked for. The first is enough to paint with and useless for
    /// overprint, because §11.7.4.3 is defined over *"colour components
    /// specified in the current colour space"* and an sRGB triple has
    /// specified all three of its own regardless of what the file said.
    ///
    /// **Empty when the space has no colorants to preserve** — a
    /// `DeviceRGB` or `CalGray` shading, or a `Separation` whose alternate
    /// is not `DeviceCmyk`. Empty is a real answer, not a missing one, and
    /// callers fall back to the sRGB route rather than inventing inks.
    cmyk: Vec<Option<[f32; 4]>>,
    /// The **authored process tints** for the same samples — Table 149's
    /// question, *"which process tints did the file state?"* — one per
    /// sample, `[0; 4]` where the space names no process colorant
    /// (`Pass 239.0`).
    ///
    /// Distinct from [`Self::cmyk`] for the reason `image::OverprintSource`
    /// gives at length: `cmyk` is the tint transform's OUTPUT, which already
    /// contains every spot's ink as process colour; this is only what the
    /// operands named. When the spots go to planes of their own, THIS is the
    /// process half the paint must use, or the spot lands twice.
    ///
    /// Empty when the space is not a `Separation`/`DeviceN` naming a spot.
    process: Vec<[f32; 4]>,
    /// The **authored spot tints** per sample, one column per entry of
    /// [`Self::spot_colorants`]. Same length discipline as `process`.
    spots: Vec<Vec<f32>>,
    /// The spot colorants this shading's space names — name and tint curve
    /// — in declaration order. Empty for every other space.
    spot_colorants: Vec<crate::overprint::SpotColorant>,
    /// The domain the samples span, `[t0, t1]`, as taken from the
    /// shading's own `/Domain`.
    domain: [f32; 2],
}

impl ColorRamp {
    /// Build a ramp by sampling `function` across `domain` and converting
    /// each result through `space`.
    ///
    /// The parametric input is mapped `t = t0 + (t1 - t0) * i/(N-1)`, so
    /// both endpoints are sampled exactly — which is what makes a
    /// two-stop gradient reproduce its declared end colours rather than
    /// values half a step inside them.
    #[must_use]
    pub fn build(
        function: &ShadingFunction,
        domain: [f32; 2],
        space: &ColorSpace,
        bridges: &crate::icc::ColorBridges,
        intent: CmykIntent,
        diag: &mut ColorDiagnostics,
    ) -> Self {
        let mut samples = Vec::with_capacity(RAMP_SAMPLES);
        // ★ Built in the SAME loop as `samples`, from the SAME `comps`, so
        // the two answers cannot describe different points of the ramp. A
        // second pass would be a second evaluation of a `/tintTransform`
        // that is allowed to be arbitrary PostScript, and nothing would
        // force the two passes to agree.
        let mut cmyk = Vec::with_capacity(RAMP_SAMPLES);
        // The spot half (`Pass 239.0`): which components are spots, and
        // their curves, resolved ONCE for the ramp. `classify` is asked with
        // `in_image_sample = false` because a shading is not a sampled
        // image (Table 149 row 1's qualifier), and with the narrowest scope
        // because the scope only decides whether a process source is
        // upgraded to `DeviceCmykDirect` — irrelevant to whether a space
        // NAMES a spot.
        let kind = crate::overprint::classify(
            space,
            false,
            pdfcer_core::settings::OverprintZeroTintScope::DeviceCmykOnly,
        );
        let arity = space.components();
        let spot_slots: Vec<(usize, std::sync::Arc<[u8]>)> = match &kind {
            Some(k) if crate::overprint::names_a_spot_colorant(k) => {
                crate::overprint::authored_spots(k, &vec![0.0_f32; arity])
                    .into_iter()
                    .map(|(component, name, _)| (component, std::sync::Arc::from(name)))
                    .collect()
            }
            _ => Vec::new(),
        };
        let spot_colorants: Vec<crate::overprint::SpotColorant> = spot_slots
            .iter()
            .map(|(component, name)| {
                (
                    std::sync::Arc::clone(name),
                    std::sync::Arc::new(crate::overprint::spot_lut(
                        space, *component, arity, intent,
                    )),
                )
            })
            .collect();
        let mut process: Vec<[f32; 4]> = Vec::with_capacity(if spot_slots.is_empty() {
            0
        } else {
            RAMP_SAMPLES
        });
        let mut spots: Vec<Vec<f32>> = Vec::with_capacity(process.capacity());
        let mut raw = Vec::new();
        let mut comps: Vec<f32> = Vec::new();
        let span = f64::from(domain[1] - domain[0]);
        for i in 0..RAMP_SAMPLES {
            #[allow(clippy::cast_precision_loss)]
            let frac = i as f64 / (RAMP_SAMPLES - 1) as f64;
            let t = f64::from(domain[0]) + span * frac;
            if !function.eval(&[t], &mut raw) {
                samples.push(None);
                cmyk.push(None);
                if !spot_slots.is_empty() {
                    process.push([0.0; 4]);
                    spots.push(vec![0.0; spot_slots.len()]);
                }
                continue;
            }
            comps.clear();
            #[allow(clippy::cast_possible_truncation)]
            comps.extend(raw.iter().map(|v| *v as f32));
            // Through the page's bridges, not the bare space (`Pass 243.0`):
            // an `ICCBased` ramp goes through its profile and a `Lab` ramp
            // through the output intent, exactly as a fill in the same space
            // does. `ColorBridges::none()` is the identity.
            samples.push(bridges.to_rgb(space, &comps, intent, diag));
            cmyk.push(bridges.to_cmyk(space, &comps, diag));
            // Authored tints from the SAME `comps`, in the SAME loop, for the
            // reason the two lines above share it.
            if !spot_slots.is_empty()
                && let Some(k) = &kind
            {
                process.push(crate::overprint::authored_tints(k, &comps).unwrap_or([0.0; 4]));
                spots.push(
                    spot_slots
                        .iter()
                        .map(|(component, _)| comps.get(*component).copied().unwrap_or(0.0))
                        .collect(),
                );
            }
        }
        // All-or-nothing: a ramp whose space yields colorants at some
        // samples and not others would let a shading overprint across part
        // of its span and not the rest, which is a seam no file asked for.
        // Either the space has colorants or it does not.
        if cmyk.iter().any(Option::is_none) {
            cmyk.clear();
        }
        Self {
            samples,
            cmyk,
            process,
            spots,
            spot_colorants,
            domain,
        }
    }

    /// The spot colorants this ramp's space names, with their curves —
    /// what a caller hands to `overprint::resolve_spot_planes` before
    /// painting in ink (`Pass 239.0`). Empty for every space that names no
    /// spot, which is the answer that keeps the plane apparatus off the
    /// common path.
    #[must_use]
    pub(crate) fn spot_colorants(&self) -> &[crate::overprint::SpotColorant] {
        &self.spot_colorants
    }

    /// The **authored** colour at `t` — process tints as the file stated
    /// them, plus one tint per entry of [`Self::spot_colorants`] — for a
    /// paint that deposits the spots into their own planes. `None` where
    /// the ramp carries no spot half or the function did not evaluate.
    ///
    /// The counterpart of [`Self::at_cmyk`], which is the FLATTENED answer;
    /// a caller uses exactly one of the two per paint, never both.
    #[must_use]
    pub(crate) fn at_authored(&self, t: f32) -> Option<([f32; 4], &[f32])> {
        if self.spot_colorants.is_empty() {
            return None;
        }
        let i = self.index_of(t);
        // The function failed at this sample: the flattened twin is `None`
        // there too, and a paint must not invent ink.
        self.cmyk.get(i).copied().flatten()?;
        Some((*self.process.get(i)?, self.spots.get(i)?.as_slice()))
    }

    /// The colour at parametric coordinate `t`, or `None` where the
    /// function did not evaluate there.
    ///
    /// `t` outside `domain` is **clamped to the nearest end**. That is the
    /// ramp's own contract and is not a statement about `/Extend`:
    /// whether anything is painted beyond a shading's ends is a geometry
    /// question decided before this is called. Clamping here only ensures
    /// that when the geometry *does* ask for an out-of-domain colour, it
    /// gets the end colour rather than an index panic.
    /// The **authored colorants** at `t`, or `None` where this ramp has
    /// none — the colorant twin of [`Self::at`], clamped identically.
    ///
    /// A caller that gets `None` must paint through [`Self::at`] instead.
    /// It must not substitute a converted value: the whole reason this
    /// exists is that a converted value cannot answer the overprint
    /// question.
    #[must_use]
    pub fn at_cmyk(&self, t: f32) -> Option<[f32; 4]> {
        if self.cmyk.is_empty() {
            return None;
        }
        *self.cmyk.get(self.index_of(t))?
    }

    /// Whether this ramp can be painted in ink at all.
    #[must_use]
    pub fn has_colorants(&self) -> bool {
        !self.cmyk.is_empty()
    }

    /// Which of the four process channels this ramp EVER writes
    /// (`Pass 201.0`).
    ///
    /// # Why a colorant NAME is not enough to answer this
    ///
    /// Table 149's rules are selected on the colorant names a source space
    /// declares. For a **spot** colorant that is not the whole story: the
    /// spot's ink lands in whichever process channels its tint transform
    /// writes, and its NAME maps to none of them. `Pass 195.0` fixed the
    /// resulting loss by widening a mixed source to `[Source; 4]` -- which
    /// writes the source's value into channels the source never claimed.
    ///
    /// ★★ THAT TRADE WAS DOCUMENTED AS SAFE AND WAS NOT. `Pass 195.0`'s own
    /// comment reads *"it writes the source's M and K, which are 0 for this
    /// shading, so it knocks out backdrop magenta and black that the spot
    /// never claimed. No patch in the conformance corpus detects that"*. One
    /// does, on **sixteen** marks: a `1 0 1 .5 k` check mark under an
    /// overprinting `/DeviceN [<spot>, /Cyan]` shading lost its `K = 0.5` to
    /// the shading's `K = 0`, and vanished.
    ///
    /// # Why this is answerable HERE and was not answerable there
    ///
    /// `Pass 195.0` could not narrow per channel because
    /// `cmyk_group_rules` is called once per graphics state with a
    /// PLACEHOLDER colour (`[0, 0, 0, 0]`) -- the real colour only exists
    /// per sample. A ramp is different: it is the whole set of colours this
    /// shading can produce, already built, so its reach is a property of the
    /// SHADING rather than of a sample, and computing it costs one pass over
    /// samples that already exist.
    ///
    /// A channel counts as written if any sample is non-zero. Zero ink in
    /// every sample means the shading never touches that plane, and Table 149
    /// says a backdrop component a source does not claim is kept.
    #[must_use]
    pub fn ink_reach(&self) -> [bool; 4] {
        let mut reach = [false; 4];
        for sample in self.cmyk.iter().flatten() {
            for (i, r) in reach.iter_mut().enumerate() {
                if sample.get(i).is_some_and(|v| *v > 0.0) {
                    *r = true;
                }
            }
        }
        reach
    }

    #[must_use]
    pub fn at(&self, t: f32) -> Option<Rgb> {
        self.samples.get(self.index_of(t)).copied().flatten()
    }

    /// Map a parametric `t` onto a sample index, clamping out-of-domain to
    /// the nearest end.
    ///
    /// ★ Shared by [`Self::at`] and [`Self::at_cmyk`] rather than written
    /// twice, and that is a correctness property rather than tidiness: the
    /// two lookups describe the SAME point of the same ramp, and two copies
    /// of a rounding expression are two things that can drift. This is
    /// decision 084's shared-predicate rule at the smallest possible scale.
    ///
    /// The clamp is the ramp's own contract and says nothing about
    /// `/Extend`: whether anything is painted beyond a shading's ends is a
    /// geometry question decided before this is called.
    fn index_of(&self, t: f32) -> usize {
        let [t0, t1] = self.domain;
        let span = t1 - t0;
        let frac = if span.abs() < f32::EPSILON {
            0.0
        } else {
            ((t - t0) / span).clamp(0.0, 1.0)
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            (frac * (RAMP_SAMPLES - 1) as f32).round() as usize
        }
    }

    /// Whether every sample produced a colour.
    ///
    /// A ramp with holes still paints — the holes simply paint nothing —
    /// but it is a shortfall an operator should be told about, so the
    /// inventory reports it.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.samples.iter().all(Option::is_some)
    }
}

/// A resolved shading dictionary (§8.7.4.3, Table 78) plus its geometry.
#[derive(Debug, Clone)]
pub struct Shading {
    /// The `ShadingType`-specific half.
    pub geometry: Geometry,
    /// `/ColorSpace` — required by Table 78.
    ///
    /// §8.7.4.4 forbids this being a `Pattern` space. A file that does it
    /// anyway is refused rather than painted, because a pattern-within-a-
    /// shading has no defined colour at a parametric coordinate.
    pub color_space: Arc<ColorSpace>,
    /// `/Function` — optional in Table 78 (types 4–7 may carry colour per
    /// vertex instead), required in practice for types 1–3.
    pub function: Option<ShadingFunction>,
    /// The pre-sampled ramp, when there is a function and a domain to
    /// sample it over. `None` for a mesh shading with per-vertex colour.
    pub ramp: Option<ColorRamp>,
    /// `/Background`, in `color_space`'s components.
    ///
    /// **Honoured only on the [`PaintRoute::ShadingPattern`] route** —
    /// Table 77 says `sh` ignores it. Stored regardless of route so the
    /// model is a faithful reading of the dictionary and the route
    /// decision stays at the paint site.
    pub background: Option<Vec<f32>>,
    /// `/BBox`, in the shading's own target coordinate space.
    pub bbox: Option<[f32; 4]>,
    /// `/AntiAlias`, default `false`.
    pub anti_alias: bool,
    /// The decoded stream half of a type 4-7 shading (`MSH1`: those four
    /// "shall be represented as streams").
    ///
    /// `None` for the analytic types, and also for a mesh whose stream
    /// could not be turned into geometry - in which case the refusal has
    /// already been counted in [`ShadingDiagnostics::mesh_unusable`] and
    /// named in the notes, so a `None` here is never silent.
    ///
    /// Behind an [`Arc`] because a mesh is the one part of a shading that
    /// can be megabytes, and a [`Shading`] is cloned per paint on some
    /// routes.
    pub mesh: Option<Arc<crate::mesh::Mesh>>,
}

impl Shading {
    /// Resolve a shading dictionary from an arbitrary object.
    ///
    /// Accepts a dictionary or a stream (types 4–7 are streams, and Table
    /// 78's entries live in the stream dictionary), which is why the
    /// parameter is an [`Object`] rather than a [`Dict`].
    ///
    /// Returns `None` only when the dictionary is unusable — absent, not a
    /// dictionary, missing `ShadingType`, or carrying a colour space that
    /// would not resolve. Every such refusal is counted and named in
    /// `diag`, never silent.
    #[must_use]
    pub fn load(
        doc: &DocumentView<'_>,
        obj: &Object,
        resources: &Dict,
        policy: crate::font::RenderPolicy<'_>,
        icc: crate::image::IccContext<'_>,
        color_diag: &mut ColorDiagnostics,
        diag: &mut ShadingDiagnostics,
    ) -> Option<Self> {
        // The two policy axes a shading reads, taken from the one struct the
        // interpreter already carries rather than as two parameters -- the
        // parameter list crossed clippy's ceiling when the bridge context
        // joined it (`Pass 243.0`), and the lint was right: `cmyk_intent`
        // and `mesh_patch_padding` are two fields of one operator setting.
        let intent = policy.cmyk_intent;
        let patch_padding = policy.mesh_patch_padding;
        let resolved = doc.resolve(obj);
        // Table 78's entries live in the dictionary either way; a mesh
        // shading is a stream whose *dictionary* carries them.
        // The STREAM itself, when there is one. A mesh's geometry is in
        // the payload rather than in the dictionary (`MSH1`), so keeping
        // only `&s.dict` -- which is all the analytic types ever needed --
        // would throw away the half that matters for types 4-7.
        let stream = match resolved {
            Object::Stream(s) => Some(s),
            _ => None,
        };
        let dict = match resolved {
            Object::Dict(d) => d,
            Object::Stream(s) => &s.dict,
            _ => {
                diag.refused += 1;
                diag.note("shading object is neither a dictionary nor a stream");
                return None;
            }
        };

        let Some(shading_type) = dict.get(b"ShadingType").and_then(|o| num(doc, o)) else {
            diag.refused += 1;
            diag.note("shading dictionary has no /ShadingType (Table 78 requires it)");
            return None;
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let shading_type = shading_type.round() as i64;

        let geometry = match shading_type {
            1 => Geometry::FunctionBased {
                domain: array_n(doc, dict, b"Domain").unwrap_or([0.0, 1.0, 0.0, 1.0]),
                matrix: array_n(doc, dict, b"Matrix").unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            },
            2 | 3 => {
                let domain = array_n(doc, dict, b"Domain").unwrap_or([0.0, 1.0]);
                let extend = extend_pair(doc, dict);
                if shading_type == 2 {
                    let Some(coords) = array_n::<4>(doc, dict, b"Coords") else {
                        diag.refused += 1;
                        diag.note(
                            "axial shading has no usable /Coords (Table 80 requires 4 numbers)",
                        );
                        return None;
                    };
                    Geometry::Axial {
                        coords,
                        domain,
                        extend,
                    }
                } else {
                    let Some(coords) = array_n::<6>(doc, dict, b"Coords") else {
                        diag.refused += 1;
                        diag.note(
                            "radial shading has no usable /Coords (Table 81 requires 6 numbers)",
                        );
                        return None;
                    };
                    Geometry::Radial {
                        coords,
                        domain,
                        extend,
                    }
                }
            }
            4..=7 => Geometry::Mesh {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                shading_type: shading_type as u8,
            },
            other => {
                diag.refused += 1;
                diag.note(&format!(
                    "/ShadingType {other} is not one of the seven §8.7.4.5 types"
                ));
                return None;
            }
        };

        let Some(cs_obj) = dict.get(b"ColorSpace") else {
            diag.refused += 1;
            diag.note("shading dictionary has no /ColorSpace (Table 78 requires it)");
            return None;
        };
        let Some(color_space) =
            crate::color::resolve_object(doc, doc.resolve(cs_obj), resources, 0, color_diag)
        else {
            diag.refused += 1;
            diag.note("shading /ColorSpace did not resolve");
            return None;
        };
        // §8.7.4.4: a shading's colour space "shall not be a Pattern space".
        // Refused rather than painted: a pattern has no colour at a
        // parametric coordinate, so there is nothing to put in the ramp.
        if matches!(*color_space, ColorSpace::Pattern { .. }) {
            diag.refused += 1;
            diag.note("shading /ColorSpace is a Pattern space, which §8.7.4.4 forbids");
            return None;
        }

        // ★ The page's colour bridges for this space, resolved ONCE per
        // shading (`Pass 243.0`). Until this Pass a shading's colour was the
        // one route left that never saw the bridge cache: an `ICCBased` RGB
        // gradient was Table 66's reinterpretation beside a fill that went
        // through its profile, and a `Lab` gradient bridged through
        // `rgb_to_cmyk` beside a fill that separated through the output
        // intent. Same page, same profile, two colours.
        let bridges = icc.bridges_for(&color_space);
        if bridges.is_managed() {
            diag.ramps_managed += 1;
        }

        let function = load_function(doc, dict, diag);
        if let Some(f) = &function {
            let want = color_space.components();
            if f.outputs() != want {
                diag.function_arity_mismatch += 1;
                diag.note(&format!(
                    "shading /Function produces {} component(s) but its colour space takes {want}",
                    f.outputs()
                ));
            }
        }

        // `/Decode` is read once here rather than inside the mesh parser,
        // because a PARAMETRIC mesh's ramp domain is its `c` pair -- MSH14
        // clause 2 shrinks the array to `[xmin xmax ymin ymax t_min
        // t_max]` -- and the ramp is built before the parse.
        let mesh_decode = components(doc, dict, b"Decode");

        // The analytic types have a 1-D parametric domain to sample, and so
        // does a mesh WITH a `/Function`: MSH14's `t`. A mesh without one
        // carries colour per vertex and has no ramp at all, which is a real
        // answer rather than a missing one.
        let ramp = match (&geometry, &function) {
            (Geometry::Mesh { .. }, Some(f)) => {
                // The `t` range is the `/Decode` array's third pair. If it
                // is absent the mesh will refuse below for the same reason,
                // so [0, 1] here only keeps the ramp from being a second
                // place that decides the file is malformed.
                let domain = mesh_decode
                    .as_ref()
                    .filter(|d| d.len() >= 6)
                    .map_or([0.0, 1.0], |d| [d[4], d[5]]);
                diag.ramps_sampled += 1;
                let r = ColorRamp::build(f, domain, &color_space, &bridges, intent, color_diag);
                if !r.is_complete() {
                    diag.ramps_incomplete += 1;
                    diag.note(
                        "mesh shading /Function failed at one or more ramp samples; those patches paint nothing",
                    );
                }
                Some(r)
            }
            (Geometry::Axial { domain, .. } | Geometry::Radial { domain, .. }, Some(f)) => {
                diag.ramps_sampled += 1;
                let r = ColorRamp::build(f, *domain, &color_space, &bridges, intent, color_diag);
                if !r.is_complete() {
                    diag.ramps_incomplete += 1;
                    diag.note(
                        "shading /Function failed at one or more ramp samples; those bands paint nothing",
                    );
                }
                Some(r)
            }
            _ => None,
        };

        if geometry.is_analytic() && function.is_none() {
            diag.missing_function += 1;
            diag.note(&format!(
                "/ShadingType {} has no usable /Function, so it has no colour at any coordinate",
                geometry.shading_type()
            ));
        }

        // ---------------------------------------------------------------
        // The stream half (types 4-7). Everything above this point is
        // Table 78, which governs all seven types identically.
        // ---------------------------------------------------------------
        let mesh = if let Geometry::Mesh { shading_type } = geometry {
            let decoded = stream.and_then(|st| {
                let raw = doc.slice(st.data_span)?;
                pdfcer_core::filters::decode_stream(&st.dict, raw).ok()
            });
            let outcome = match (stream, decoded.as_deref()) {
                (None, _) => Err(crate::mesh::MeshRefusal::NotAStream),
                (Some(_), None) => Err(crate::mesh::MeshRefusal::Undecodable),
                (Some(_), Some(data)) => {
                    let input = crate::mesh::ParseInput {
                        shading_type,
                        data,
                        decode: mesh_decode.as_deref(),
                        bits_per_coordinate: uint(doc, dict, b"BitsPerCoordinate"),
                        bits_per_component: uint(doc, dict, b"BitsPerComponent"),
                        bits_per_flag: uint(doc, dict, b"BitsPerFlag"),
                        vertices_per_row: uint(doc, dict, b"VerticesPerRow"),
                        space: &color_space,
                        bridges: &bridges,
                        parametric: function.is_some(),
                        patch_padding,
                        intent,
                    };
                    crate::mesh::parse(&input, color_diag)
                }
            };
            match outcome {
                Ok(m) => {
                    diag.mesh_records += m.records;
                    if m.truncated {
                        diag.mesh_truncated += 1;
                        diag.note(
                            "mesh shading stream ended part-way through a record; the complete records were painted and the remainder discarded",
                        );
                    }
                    if let Some(rows) = m.rows_inferred {
                        diag.note(&format!(
                            "type 5 mesh row count is not in the dictionary and was inferred from the stream length: {rows} row(s)"
                        ));
                    }
                    Some(Arc::new(m))
                }
                Err(reason) => {
                    diag.mesh_unusable += 1;
                    diag.note(reason.reason());
                    None
                }
            }
        } else {
            None
        };

        diag.count(&geometry);

        Some(Self {
            geometry,
            color_space,
            function,
            ramp,
            background: components(doc, dict, b"Background"),
            bbox: array_n(doc, dict, b"BBox"),
            anti_alias: boolean(doc, dict, b"AntiAlias"),
            mesh,
        })
    }

    /// Whether pdfcer can paint this shading.
    ///
    /// The name said "the NEXT slice will be able to" for as long as there
    /// was a next slice, and outlived two of them. Read against
    /// [`ShadingDiagnostics::painted`]: the gap between the two is shadings
    /// pdfcer understood and still did not put on the page, which is the
    /// difference between an operator whose file is malformed and one
    /// waiting on a capability.
    ///
    /// True for an analytic type with a ramp, or for a mesh whose stream
    /// decoded. False is always accompanied by a named reason in
    /// [`ShadingDiagnostics::notes`] - a shading pdfcer declines and cannot
    /// explain would violate project rule 4.
    #[must_use]
    pub fn is_paintable(&self) -> bool {
        if self.mesh.is_some() {
            // A mesh needs no ramp unless it is the parametric form, and
            // the parametric form's ramp is built by `load` exactly when
            // `/Function` is present. So "the stream decoded" is the whole
            // condition.
            return true;
        }
        self.geometry.is_analytic() && self.ramp.is_some()
    }
}

/// Load `/Function` in either §8.7.4.4 shape.
fn load_function(
    doc: &DocumentView<'_>,
    dict: &Dict,
    diag: &mut ShadingDiagnostics,
) -> Option<ShadingFunction> {
    let entry = dict.get(b"Function")?;
    let resolved = doc.resolve(entry);
    // The array form is *n* one-output functions. It is checked first
    // because a function may itself be an array-valued object in no other
    // sense, so there is no ambiguity to resolve.
    if let Some(items) = resolved.as_array() {
        let mut fs = Vec::with_capacity(items.len());
        for item in items {
            match PdfFunction::load(doc, doc.resolve(item)) {
                Ok(f) => fs.push(Arc::new(f)),
                Err(_) => {
                    diag.function_unloadable += 1;
                    diag.note("a shading /Function array member did not load");
                    return None;
                }
            }
        }
        if fs.is_empty() || fs.len() > MAX_FUNCTION_OUTPUTS {
            diag.function_unloadable += 1;
            diag.note("shading /Function array is empty or implausibly wide");
            return None;
        }
        return Some(ShadingFunction::PerComponent(fs));
    }
    match PdfFunction::load(doc, resolved) {
        Ok(f) => {
            if f.outputs() == 0 || f.outputs() > MAX_FUNCTION_OUTPUTS {
                diag.function_unloadable += 1;
                diag.note("shading /Function declares an implausible output count");
                return None;
            }
            Some(ShadingFunction::Single(Arc::new(f)))
        }
        Err(_) => {
            diag.function_unloadable += 1;
            diag.note("shading /Function did not load");
            None
        }
    }
}

/// A numeric object as `f64`, resolving a reference first.
fn num(doc: &DocumentView<'_>, obj: &Object) -> Option<f64> {
    doc.resolve(obj).as_number()
}

/// A fixed-length numeric array entry, or `None` if absent, not an array,
/// the wrong length, or carrying a non-numeric element.
///
/// Strict about length on purpose: `/Coords` with three numbers is not an
/// axial shading missing one value, it is a file whose geometry pdfcer
/// cannot know, and guessing the fourth would paint a plausible wrong
/// gradient.
fn array_n<const N: usize>(doc: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Option<[f32; N]> {
    let items = doc.resolve(dict.get(key)?).as_array()?;
    if items.len() != N {
        return None;
    }
    let mut out = [0.0f32; N];
    for (slot, item) in out.iter_mut().zip(items) {
        #[allow(clippy::cast_possible_truncation)]
        {
            *slot = num(doc, item)? as f32;
        }
    }
    Some(out)
}

/// A variable-length numeric array entry (`/Background`, whose length is
/// the colour space's component count rather than a fixed number).
fn components(doc: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Option<Vec<f32>> {
    let items = doc.resolve(dict.get(key)?).as_array()?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        #[allow(clippy::cast_possible_truncation)]
        out.push(num(doc, item)? as f32);
    }
    Some(out)
}

/// A boolean dictionary entry, defaulting to `false` when absent or of the
/// wrong type.
///
/// `Object` has no `as_bool` accessor — booleans are read by matching the
/// variant, which is the idiom the rest of this crate uses.
/// Read a non-negative integer entry, or `None` if it is absent, not a
/// number, or negative.
///
/// Separate from [`num`] because every consumer of one of these
/// (`BitsPerCoordinate`, `BitsPerComponent`, `BitsPerFlag`,
/// `VerticesPerRow`) validates the value against a closed set immediately
/// afterwards, and a negative or fractional value must fail that check
/// rather than wrap into a plausible one.
fn uint(doc: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Option<u32> {
    let v = num(doc, dict.get(key)?)?;
    if !v.is_finite() || v < 0.0 || v > f64::from(u32::MAX) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(v.round() as u32)
}

fn boolean(doc: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> bool {
    matches!(
        dict.get(key).map(|o| doc.resolve(o)),
        Some(Object::Boolean(true))
    )
}

/// `/Extend`, default `[false false]` (Tables 80 and 81).
///
/// **Axial and radial `/Extend` do NOT mean the same thing**, which is why
/// this returns the raw flags and decides nothing. Axial extension
/// continues a flat half-plane of the boundary colour indefinitely; radial
/// extension continues the *linear interpolation of centre and radius*, so
/// the circles keep moving and growing. Implementing radial extend as
/// "clamp the parameter to [0,1]" gives the right colour and the wrong
/// shape (`iso32000__s__8.7.4.5__analytic.md`, SH37).
fn extend_pair(doc: &DocumentView<'_>, dict: &Dict) -> [bool; 2] {
    let Some(items) = dict
        .get(b"Extend")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_array)
    else {
        return [false, false];
    };
    let get = |i: usize| {
        matches!(
            items.get(i).map(|o| doc.resolve(o)),
            Some(Object::Boolean(true))
        )
    };
    [get(0), get(1)]
}

/// Counted disclosures from shading handling.
///
/// Structured to answer the questions an operator actually asks about a
/// page with a blank gradient on it, in order: *did pdfcer see a shading at
/// all?* (`encountered`), *what kind?* (`by_type`), *will an update fix
/// it?* (`paintable` vs `mesh`), *or is my file broken?* (`refused` and the
/// named reasons).
///
/// This block said **"every counter here is currently a 'found, not
/// painted' census"** through two slices that painted, which is the failure
/// `R212` is about: a claim ABOUT a module, sitting where it is read first,
/// with nothing under test to contradict it.
///
/// What survives from the original reasoning, because it was right and is
/// why `painted` reads correctly today: a counter that appears at the same
/// moment its feature does cannot be used to show the feature arriving. Both
/// [`Self::painted`] and [`Self::paintable`] predate the code that moves
/// them, so a pre-Pass and post-Pass binary can be compared on the same
/// file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ShadingDiagnostics {
    /// Shading dictionaries encountered, by either route.
    pub encountered: usize,
    /// Of those, how many arrived via the `sh` operator rather than a
    /// `PatternType 2` pattern. The two anchor differently, so a page that
    /// renders wrong in only one of them is a different bug.
    pub via_sh: usize,
    /// Per-`ShadingType` census, indexed 1..=7 at positions 0..=6.
    pub by_type: [usize; 7],
    /// Shadings whose model loaded completely enough to paint.
    ///
    /// This said "once the geometry slice lands" until 2026-08-22. That
    /// slice landed; the sentence did not notice. Read against
    /// [`Self::painted`] -- the gap between the two is shadings pdfcer
    /// understood and still did not put on the page.
    pub paintable: usize,
    /// Shadings actually painted.
    ///
    /// This said "**Zero in this slice, by construction**" long after the
    /// analytic painting slice made it non-zero, which is worse than a
    /// stale comment: a reader who trusted it would have taken a correct
    /// non-zero count for a bug. Meaningless alone -- it is half of a pair
    /// with [`Self::paintable`].
    pub painted: usize,
    /// Shading dictionaries refused outright, with a named reason.
    pub refused: usize,
    /// Analytic shadings carrying no usable `/Function`.
    pub missing_function: usize,
    /// `/Function` entries that would not load.
    pub function_unloadable: usize,
    /// `/Function` output count disagreeing with the colour space's
    /// component count (§8.7.4.4).
    pub function_arity_mismatch: usize,
    /// Colour ramps built.
    pub ramps_sampled: usize,
    /// Ramps with at least one sample the function failed to produce.
    pub ramps_incomplete: usize,
    /// Shadings whose colour space had a bridge on this page — an
    /// `ICCBased` space with a usable profile, or a CIE space on a document
    /// with an output intent — so their colour went through it rather than
    /// through Table 66's reinterpretation or `rgb_to_cmyk` (`Pass 243.0`).
    /// Counted per shading loaded, meshes included. The disclosure half of
    /// rule 4 for a conversion that leaves nothing on screen to point at.
    pub ramps_managed: usize,
    /// Geometry decoded from mesh streams - triangles for types 4/5,
    /// patches for types 6/7.
    ///
    /// Zero beside a non-zero [`Self::mesh_unusable`] says the streams were
    /// there and pdfcer could not read them; zero beside a non-zero
    /// [`Self::by_type`] entry for 4-7 with no refusal would be a bug.
    pub mesh_records: usize,
    /// Mesh streams that ended part-way through a record, or (type 5) did
    /// not hold a whole number of rows.
    ///
    /// pdfcer paints the complete records and discards the remainder.
    /// ISO 32000-1 states an error condition for exactly one of the four
    /// types (`MSH20`, type 4) and is silent for the other three, so this
    /// is a product decision disclosed rather than a conformance verdict.
    pub mesh_truncated: usize,
    /// Mesh shadings whose stream could not be turned into geometry at all,
    /// each with a named reason in [`Self::notes`].
    ///
    /// Distinct from [`Self::refused`], which counts dictionaries rejected
    /// before the stream was ever reached.
    pub mesh_unusable: usize,
    /// First few distinct human-readable reasons.
    pub notes: Vec<String>,
}

/// Cap on [`ShadingDiagnostics::notes`], matching the other diagnostic
/// note lists in this crate.
const MAX_NOTES: usize = 12;

impl ShadingDiagnostics {
    /// Record a distinct reason.
    fn note(&mut self, reason: &str) {
        if self.notes.len() < MAX_NOTES && !self.notes.iter().any(|s| s == reason) {
            self.notes.push(reason.to_owned());
        }
    }

    /// Record a successfully classified geometry in the per-type census.
    fn count(&mut self, geometry: &Geometry) {
        let t = geometry.shading_type() as usize;
        if (1..=7).contains(&t) {
            self.by_type[t - 1] += 1;
        }
    }

    /// Note that a shading was reached, and by which route.
    ///
    /// Separate from [`Self::count`] because a shading that is *refused*
    /// still counts as encountered — otherwise a page whose every shading
    /// is malformed reports zero shadings, which is the same number a page
    /// with no gradients reports.
    pub fn reached(&mut self, route: PaintRoute) {
        self.encountered += 1;
        if route == PaintRoute::ShOperator {
            self.via_sh += 1;
        }
    }

    /// Mesh shadings (types 4–7) seen — the share of a corpus that needs
    /// the stream-decoding work rather than the parametric work.
    #[must_use]
    pub fn mesh(&self) -> usize {
        self.by_type[3..7].iter().sum()
    }

    /// Fold a nested form XObject's shading diagnostics into this one.
    pub fn merge(&mut self, other: Self) {
        self.encountered += other.encountered;
        self.via_sh += other.via_sh;
        for (slot, add) in self.by_type.iter_mut().zip(other.by_type) {
            *slot += add;
        }
        self.paintable += other.paintable;
        self.painted += other.painted;
        self.refused += other.refused;
        self.missing_function += other.missing_function;
        self.function_unloadable += other.function_unloadable;
        self.function_arity_mismatch += other.function_arity_mismatch;
        self.ramps_sampled += other.ramps_sampled;
        self.ramps_incomplete += other.ramps_incomplete;
        self.ramps_managed += other.ramps_managed;
        self.mesh_records += other.mesh_records;
        self.mesh_truncated += other.mesh_truncated;
        self.mesh_unusable += other.mesh_unusable;
        for note in other.notes {
            self.note(&note);
        }
    }
}

// ===========================================================================
// PAINTING — §8.7.4.5, the analytic types
// ===========================================================================
//
// Everything below is sourced label-by-label from
// `iso32000__s__8.7.4.5__analytic.md` in the PDF-spec RAG. Each function
// names the labels it implements, so a future reader can check the code
// against the clause without re-deriving anything, and so a claim that is
// pdfcer's own rather than the standard's is visibly marked as such.

/// The parametric coordinate a device point maps to, or the reason it maps
/// to nothing.
///
/// # Why "unpainted" is a value rather than an `Option<f32>` alias
///
/// The standard distinguishes two situations that an `Option` would blur,
/// and they are visibly different on the page:
///
/// - **Outside the shading** — SH26's *"t is undefined and the point shall
///   be left unpainted"*. The backdrop shows through.
/// - **Inside the shading but the colour did not resolve** — a ramp hole
///   (the `/Function` failed at that sample). Also unpainted, but it is a
///   *defect*, and it is counted separately so a gradient with a hole in it
///   does not report as a gradient that was correctly clipped.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Param {
    /// Paint this point with the ramp colour at parametric `t`.
    At(f32),
    /// Leave this point alone — the standard says so for this geometry.
    Unpainted,
}

impl Geometry {
    /// Map a point in the shading's **target coordinate space** to a
    /// parametric `t`, or to "leave unpainted".
    ///
    /// The target space is current user space under `sh` and pattern space
    /// under a shading pattern (SH1, SH6) — the caller has already applied
    /// whichever transform that is, so this function is route-agnostic.
    ///
    /// Returns `None` for a geometry this slice does not paint (type 1,
    /// and the meshes), which the caller distinguishes from
    /// [`Param::Unpainted`].
    fn param_at(&self, x: f32, y: f32) -> Option<Param> {
        match self {
            // Type 1 is MODELLED but not painted in this slice. It needs a
            // 2-in function evaluated over an inverse `/Matrix` and has no
            // `/Extend` at all — outside the transformed domain rectangle
            // it is `/Background` or nothing (SH21). Deliberately deferred
            // rather than rushed: the suite measurement found **zero**
            // type-1 shadings against 5 axial and 9 radial, so it is the
            // cheapest of the three to leave and the least missed.
            Self::FunctionBased { .. } | Self::Mesh { .. } => None,
            Self::Axial {
                coords,
                domain,
                extend,
            } => Some(axial_param(*coords, *domain, *extend, x, y)),
            Self::Radial {
                coords,
                domain,
                extend,
            } => Some(radial_param(*coords, *domain, *extend, x, y)),
        }
    }
}

/// §8.7.4.5.3 axial: project a point onto the axis and map to `t`.
///
/// # The projection (SH25)
///
/// The spec's variable is **`x′`, not `s`** — `s` belongs to the radial
/// clause, and carrying it here would read as a cross-clause error. There
/// is no `y′`: *"all points along a line in domain space perpendicular to
/// the line from (0, 0) to (1, 0) have the same colour, only the new value
/// of x needs to be computed"*.
///
/// ```text
///         (x1 − x0) × (x − x0)  +  (y1 − y0) × (y − y0)
/// x′  =  ───────────────────────────────────────────────
///               (x1 − x0)²  +  (y1 − y0)²
/// ```
///
/// Normalised by **‖axis‖², not ‖axis‖** — `x′` is a fraction of the axis,
/// not an arc length. It is 0 at `(x0,y0)` and 1 at `(x1,y1)`.
///
/// # The `t` mapping and `/Extend` (SH26, SH27)
///
/// | `x′` | `t` | painted? |
/// |---|---|---|
/// | `0 ≤ x′ ≤ 1` | `t0 + (t1−t0)·x′` | yes |
/// | `x′ < 0`, `Extend[0]` | `t0` | yes, flat, indefinitely |
/// | `x′ < 0`, else | undefined | **no** |
/// | `x′ > 1`, `Extend[1]` | `t1` | yes, flat, indefinitely |
/// | `x′ > 1`, else | undefined | **no** |
///
/// The bounds are **inclusive at both ends**, so `x′` exactly 0 or 1 takes
/// the interpolation branch rather than an extend branch — no gap and no
/// double-cover at the joins.
///
/// # The degenerate axis (SH29 / AMB-1)
///
/// When the endpoints coincide the denominator is 0/0. **ISO 32000-1 is
/// silent**; **ISO 32000-2 Table 79 adds "If the starting and ending
/// coordinates are coincident … nothing shall be painted."** pdfcer applies
/// the 2.0 rule at every version: a later edition resolving an earlier
/// silence is a *forced* default, not a choice between two readings, so
/// this is not a settings-register candidate. It also happens to be the
/// only answer that cannot produce NaN pixels.
fn axial_param(coords: [f32; 4], domain: [f32; 2], extend: [bool; 2], x: f32, y: f32) -> Param {
    let [x0, y0, x1, y1] = coords;
    let (dx, dy) = (x1 - x0, y1 - y0);
    let denom = dx.mul_add(dx, dy * dy);
    if denom <= f32::EPSILON {
        return Param::Unpainted;
    }
    let x_prime = dx.mul_add(x - x0, dy * (y - y0)) / denom;
    let [t0, t1] = domain;
    if x_prime < 0.0 {
        if extend[0] {
            Param::At(t0)
        } else {
            Param::Unpainted
        }
    } else if x_prime > 1.0 {
        if extend[1] {
            Param::At(t1)
        } else {
            Param::Unpainted
        }
    } else {
        Param::At((t1 - t0).mul_add(x_prime, t0))
    }
}

/// §8.7.4.5.4 radial: find the blend circle covering a point and map to `t`.
///
/// # The model is a PAINTING ORDER, not a root selection (SH39)
///
/// > "Conceptually, all of the blend circles shall be painted in order of
/// > increasing values of *s* … The painting is opaque, with the colour of
/// > each circle completely overlaying those preceding it. Therefore, if a
/// > point lies within more than one blend circle, its final colour shall
/// > be that of the last of the enclosing circles to be painted,
/// > corresponding to the **greatest value of s**."
///
/// Four `shall`s. The operative consequence (SH40): **the colour is the one
/// at the greatest admissible `s` whose blend circle encloses the point.**
///
/// The blend circles themselves are plain linear interpolations (SH33) —
/// the radius interpolates **linearly**, not by area:
///
/// ```text
/// xc(s) = x0 + s × (x1 − x0)
/// yc(s) = y0 + s × (y1 − y0)
///  r(s) = r0 + s × (r1 − r0)
/// ```
///
/// # ★ The quadratic below is pdfcer's derivation, NOT the standard's (AMB-2)
///
/// A whole-document search for "quadratic" and "discriminant" returns
/// **zero hits in both ISO 32000-1 and ISO 32000-2.** The standard never
/// inverts its own painting model. What follows is the inversion, and it is
/// written here as pdfcer's own algebra so that nobody later attaches a
/// §8.7.4.5.4 citation to it:
///
/// Substituting the SH33 equations into `|P − c(s)| = r(s)` and writing
/// `px = Px − x0`, `py = Py − y0`, `dr = r1 − r0`:
///
/// ```text
/// a = dx² + dy² − dr²
/// b = −2 × (px·dx + py·dy + r0·dr)
/// c = px² + py² − r0²
/// ```
///
/// **The obligation this carries is observational equivalence**: the roots
/// are only correct insofar as picking the greater admissible one
/// reproduces the opaque-increasing-`s` painting order. That is the
/// contract to test against, not the algebra.
///
/// `a` is zero exactly when the cone's half-angle is 45° — the two circles'
/// radii change as fast as their centres separate — and then the equation
/// is linear, not degenerate. Handled explicitly rather than by an epsilon
/// nudge, because that case is a *shape*, not a numerical accident.
///
/// # The permitted range of `s` (SH40), assembled from four places
///
/// ```text
/// s ∈ [0, 1]                       always
/// s < 0   admitted iff Extend[0]
/// s > 1   admitted iff Extend[1]
/// r(s) ≥ 0 for any admitted s      (SH38 — INFORMATIVE only, AMB-4)
/// ```
///
/// The `r(s) ≥ 0` exclusion appears **only in NOTE 1**, i.e. informative in
/// both editions. pdfcer applies it anyway: a negative radius has no
/// geometric meaning, and the alternative is painting a circle of imaginary
/// size. Recorded as a choice, not as compliance.
///
/// # Both radii zero (SH34)
///
/// > "The radii r0 and r1 shall both be greater than or equal to 0. If one
/// > radius is 0, the corresponding circle shall be a point… If both are 0,
/// > nothing shall be painted."
fn radial_param(coords: [f32; 6], domain: [f32; 2], extend: [bool; 2], x: f32, y: f32) -> Param {
    let [x0, y0, r0, x1, y1, r1] = coords;
    // SH34: both radii zero paints nothing at all.
    if r0 <= 0.0 && r1 <= 0.0 {
        return Param::Unpainted;
    }
    let (dx, dy, dr) = (x1 - x0, y1 - y0, r1 - r0);
    let (px, py) = (x - x0, y - y0);

    let a = dx.mul_add(dx, dy * dy) - dr * dr;
    let b = -2.0 * (px.mul_add(dx, py * dy) + r0 * dr);
    let c = px.mul_add(px, py * py) - r0 * r0;

    // Admissibility, per SH40 plus the informative r(s) >= 0 rule.
    let admissible = |s: f32| {
        if s < 0.0 && !extend[0] {
            return false;
        }
        if s > 1.0 && !extend[1] {
            return false;
        }
        dr.mul_add(s, r0) >= 0.0
    };

    // The greatest admissible root wins (SH39/SH40). Both roots are tried
    // largest-first, so the first admissible one IS the greatest.
    let chosen = if a.abs() <= f32::EPSILON {
        // Linear: b·s + c = 0. Not a degenerate case to be nudged past —
        // it is the exact-45-degree cone, a real shape a real file can
        // contain.
        if b.abs() <= f32::EPSILON {
            None
        } else {
            let s = -c / b;
            admissible(s).then_some(s)
        }
    } else {
        let disc = b.mul_add(b, -4.0 * a * c);
        if disc < 0.0 {
            None
        } else {
            let root = disc.sqrt();
            let s1 = (-b + root) / (2.0 * a);
            let s2 = (-b - root) / (2.0 * a);
            let (hi, lo) = if s1 >= s2 { (s1, s2) } else { (s2, s1) };
            if admissible(hi) {
                Some(hi)
            } else if admissible(lo) {
                Some(lo)
            } else {
                None
            }
        }
    };

    let Some(s) = chosen else {
        return Param::Unpainted;
    };
    // SH39: beyond either end the colour is FLAT at that end's t, even
    // though the geometry keeps moving (SH37 — this is the half that
    // differs from axial, and clamping `s` here rather than earlier is
    // what keeps the shape right while the colour flattens).
    let [t0, t1] = domain;
    let s_clamped = s.clamp(0.0, 1.0);
    Param::At((t1 - t0).mul_add(s_clamped, t0))
}

/// Paint a shading over a device-space region.
///
/// # Why per-pixel, and why that is affordable
///
/// See the module docs for why `tiny_skia`'s gradients cannot express the
/// general radial case. The cost that buys is one inverse transform and one
/// [`ColorRamp::at`] lookup per pixel — arithmetic and an array index. The
/// `/Function` itself was evaluated [`RAMP_SAMPLES`] times when the model
/// was built, not once per pixel, which is the difference between running a
/// PostScript calculator 256 times and running it eight million times.
///
/// # The paint area, and the two clips
///
/// `region` is the device-space rectangle to consider — for `sh` that is
/// the current clip's bounds (Table 77: `sh` fills the clip, it takes no
/// path). Two further restrictions apply inside it:
///
/// - `clip`, the current clip mask's per-pixel coverage.
/// - `/BBox`, which **clips** rather than merely bounds (SH7: *"temporary
///   clipping boundary … in addition to the current clipping path"*), and
///   is expressed in the shading's **target** space (SH6), so it is tested
///   after the inverse transform rather than before it.
///
/// # What is deliberately not done here
///
/// **Anti-aliasing.** `/AntiAlias` has no algorithm in either edition, 1.7
/// explicitly permits ignoring it, and 2.0 removes that permission without
/// adding a duty (AMB-8). Each pixel is tested at its centre and painted
/// fully or not at all.
///
/// **`/Background`.** It applies only on the pattern route (SH3/SH4) and
/// this slice paints only `sh`, where Table 77 says it is ignored. The
/// parameter is absent rather than passed-and-unused so that adding the
/// pattern route is a visible change here.
/// The per-pixel geometry decision, shared by **both** paint routes.
///
/// Returns `Some((t, coverage))` when this device pixel is inside the
/// shading's `/BBox`, has a parametric coordinate, and is not fully clipped
/// away; `None` when it should be skipped.
///
/// # ★ Why this is a function rather than two copies of six lines
///
/// Because `paint_region` (sRGB) and `paint_region_cmyk` (ink) must agree
/// about **which pixels a shading covers**, exactly and always. Two copies of
/// a pixel-centre offset, a `/BBox` comparison and a clip lookup are two
/// things that can drift, and the drift would show as a shading whose
/// coverage changed when a page happened to composite in ink — a difference
/// no file asked for and nothing would flag. This is decision 084's
/// shared-predicate rule: two paths that must agree share the predicate that
/// decides, rather than each computing it.
///
/// The pixel CENTRE offset in particular is load-bearing and is stated once
/// here: a gradient sampled at pixel corners is a half-pixel shifted against
/// every other paint in this renderer, which shows up as a seam where a
/// shading abuts a path filled with the same colour.
fn sample_at(
    shading: &Shading,
    to_target: tiny_skia::Transform,
    px: i32,
    py: i32,
    width: i32,
    clip: Option<&tiny_skia::Mask>,
) -> Option<(f32, f32)> {
    let mut pt = tiny_skia::Point::from_xy(px as f32 + 0.5, py as f32 + 0.5);
    to_target.map_point(&mut pt);

    // /BBox clips, in TARGET space (SH6, SH7).
    if let Some([bx0, by0, bx1, by1]) = shading.bbox
        && (pt.x < bx0.min(bx1)
            || pt.x > bx0.max(bx1)
            || pt.y < by0.min(by1)
            || pt.y > by0.max(by1))
    {
        return None;
    }

    let Some(Param::At(t)) = shading.geometry.param_at(pt.x, pt.y) else {
        return None;
    };

    let idx = (py as usize) * (width as usize) + (px as usize);
    let coverage = match clip {
        // The clip mask is one byte of coverage per device pixel, laid out
        // in the same row-major order as the pixmap.
        Some(mask) => f32::from(mask.data()[idx]) / 255.0,
        None => 1.0,
    };
    Some((t, coverage))
}

fn paint_region(
    shading: &Shading,
    ramp: &ColorRamp,
    to_target: tiny_skia::Transform,
    region: (i32, i32, i32, i32),
    clip: Option<&tiny_skia::Mask>,
    alpha: f32,
    pixmap: &mut tiny_skia::Pixmap,
) -> usize {
    let (x_lo, y_lo, x_hi, y_hi) = region;
    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let alpha = alpha.clamp(0.0, 1.0);
    let mut painted = 0usize;

    for py in y_lo.max(0)..y_hi.min(height) {
        for px in x_lo.max(0)..x_hi.min(width) {
            let Some((t, coverage)) = sample_at(shading, to_target, px, py, width, clip) else {
                continue;
            };
            let Some(rgb) = ramp.at(t) else {
                continue;
            };
            let idx = (py as usize) * (width as usize) + (px as usize);
            let a = alpha * coverage;
            if a <= 0.0 {
                continue;
            }

            // Source-over in PREMULTIPLIED space, which is what the pixmap
            // stores and therefore the form with no conversion round-trip:
            //
            //     out = src·a + dst·(1 − a)
            //
            // Done by hand because tiny_skia's blitters take a `Paint`
            // carrying ONE colour, and the entire point of a shading is
            // that every pixel has a different one.
            //
            // Straight (non-premultiplied) blending was written first and
            // rejected: it needs a divide by the output alpha, which is a
            // second place for a 0/0 to appear on exactly the pixels where
            // nothing should be drawn anyway.
            let dst = pixmap.pixels()[idx];
            let inv = 1.0 - a;
            let out_a = a
                .mul_add(255.0, f32::from(dst.alpha()) * inv)
                .round()
                .clamp(0.0, 255.0) as u8;
            // Each channel is CLAMPED TO out_a, not merely to 255.
            //
            // Algebraically a premultiplied channel can never exceed its
            // own alpha here — `src·a·255 + dst.c·inv ≤ a·255 +
            // dst.alpha·inv` because a straight colour is ≤ 1 and `dst.c ≤
            // dst.alpha` by the premultiplied invariant. But the channels
            // and the alpha are rounded INDEPENDENTLY, and rounding can
            // put a channel one unit above alpha. `from_rgba` rejects that
            // (it validates the invariant), so without this clamp a
            // scattering of pixels along a gradient would silently fail to
            // paint — a speckled hole through the gradient, which reads as
            // a rasteriser bug rather than as a rounding artefact.
            let mix = |src: f32, d: u8| -> u8 {
                let v = (src * a)
                    .mul_add(255.0, f32::from(d) * inv)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                v.min(out_a)
            };
            if let Some(c) = tiny_skia::PremultipliedColorU8::from_rgba(
                mix(rgb.r, dst.red()),
                mix(rgb.g, dst.green()),
                mix(rgb.b, dst.blue()),
                out_a,
            ) {
                pixmap.pixels_mut()[idx] = c;
                painted += 1;
            }
        }
    }
    painted
}

impl Shading {
    /// Paint this shading over a device-space area.
    ///
    /// # The two arguments that carry all the anchoring
    ///
    /// `to_target` maps **device space to the shading's target coordinate
    /// space** — the inverse of whatever transform the paint route
    /// establishes. That single parameter is where the §8.7.2-versus-Table
    /// 77 distinction lives, and keeping it a parameter rather than
    /// deriving it here is deliberate: the route knows which transform is
    /// correct, and this function does not need to.
    ///
    /// - `sh` passes the inverse of the **current CTM** (SH1, PM7).
    /// - A shading pattern passes the inverse of `base_ctm × /Matrix`
    ///   (PM2/PM3) — the pattern's own matrix concatenated with the
    ///   *initial* transform of the parent content stream, not the CTM at
    ///   paint time.
    ///
    /// `region` is the device-space `(left, top, right, bottom)` to
    /// consider. For `sh` that is the current clip's bounding box, because
    /// Table 77 gives the operator no path: it fills the clip region.
    ///
    /// # Returns
    ///
    /// The number of device pixels actually written. Zero is a legitimate
    /// answer — a fully-clipped shading, or one whose `/Extend` is false at
    /// both ends and whose geometry misses the region entirely — and the
    /// caller reports it rather than treating it as failure.
    ///
    /// `None` means this shading is of a type this build does not paint
    /// (type 1, and the meshes), which is a different fact from "painted
    /// zero pixels" and must not collapse into it.
    /// Paint this shading **natively in ink**, honouring overprint.
    ///
    /// Returns `None` for the same reasons [`Shading::paint`] does, and
    /// additionally when this shading's ramp carries no authored colorants —
    /// in which case the caller must fall back to the sRGB route rather than
    /// converting, because a converted colour cannot answer §11.7.4.3's
    /// "which components did the source specify?".
    ///
    /// Whether [`Self::paint_cmyk`] has any authored ink to paint with.
    ///
    /// # ★ Why callers must ask THIS and not `self.ramp.has_colorants()`
    ///
    /// Because a **mesh keeps its ink somewhere else**. A non-parametric
    /// mesh has no `ColorRamp` at all — its colour is per-vertex, decided
    /// when the stream was decoded — so a gate written as
    /// `ramp.is_some_and(has_colorants)` reads `false` for a fully
    /// ink-bearing `DeviceCMYK` mesh and silently sends it to the bridge.
    ///
    /// That is not hypothetical: it is precisely the shape of the two type
    /// 7 patches that survived `Pass 137.0`. The widened analytic route was
    /// correct and could not reach them, because the *test in front of it*
    /// asked about the wrong carrier. A predicate that lives beside the
    /// paint method it predicts cannot drift the way a hand-written gate at
    /// a call site can.
    ///
    /// `true` here does not promise a paint will write pixels — the
    /// geometry may miss the region or be fully clipped — only that ink
    /// exists to write.
    #[must_use]
    pub(crate) fn has_colorants(&self) -> bool {
        if let Some(mesh) = self.mesh.as_ref() {
            return match mesh.colorants {
                crate::mesh::MeshColorants::None => false,
                crate::mesh::MeshColorants::Vertex => true,
                crate::mesh::MeshColorants::Parametric => {
                    self.ramp.as_ref().is_some_and(ColorRamp::has_colorants)
                }
            };
        }
        self.ramp.as_ref().is_some_and(ColorRamp::has_colorants)
    }

    /// The geometry decision is [`sample_at`], the *same* function
    /// [`Shading::paint`] uses, so the two routes cover exactly the same
    /// pixels.
    ///
    /// # ★ A mesh takes the first branch and shares nothing below it
    ///
    /// `Pass 137.1` added the mesh route, and it dispatches on `self.mesh`
    /// **before** the analytic checks, exactly as [`Shading::paint`] does.
    /// The two algorithms are opposite rather than variations — a mesh is
    /// forward-rasterised from triangles, an analytic shading is
    /// inverse-mapped per pixel — so there is no shared ramp lookup, no
    /// `Param`, and no shared inverse map except for `/BBox`.
    ///
    /// **This method returning `None` for a mesh no longer means "meshes
    /// cannot paint in ink".** It now means this *particular* mesh has no
    /// authored ink to paint with: an additive colour space, or a
    /// parametric mesh whose ramp has no colorants. The caller's fallback
    /// is unchanged either way, but a reader diagnosing a bridged mesh
    /// needs to know which of the two questions the `None` answered.
    #[must_use]
    ///
    /// # The spot planes (`Pass 239.0`)
    ///
    /// `spot_planes` are the plane indices for [`ColorRamp::spot_colorants`],
    /// in order, from `overprint::resolve_spot_planes` — or empty, in which
    /// case the ramp's FLATTENED ink paints exactly as before. With planes,
    /// each pixel's source is the ramp's authored process tints plus its
    /// spot tints, and every plane the ramp does not name keeps the
    /// backdrop. A mesh ignores them (it has no ramp-shaped spot half yet)
    /// and paints its flattened ink; that is disclosed by the caller.
    ///
    /// Eight parameters, allowed: the paint's geometry, its two rule sets
    /// and its destination are one call's worth of state, and a struct for
    /// them would have exactly this one constructor site.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_cmyk(
        &self,
        to_target: tiny_skia::Transform,
        region: (i32, i32, i32, i32),
        clip: Option<&tiny_skia::Mask>,
        alpha: f32,
        rules: [crate::overprint::ComponentRule; 4],
        spot_planes: &[usize],
        buf: &mut crate::cmyk_buffer::CmykBuffer,
    ) -> Option<usize> {
        if let Some(mesh) = self.mesh.as_ref() {
            return crate::mesh::paint_cmyk(
                mesh,
                self.ramp.as_ref(),
                to_target,
                self.bbox,
                region,
                clip,
                alpha,
                rules,
                buf,
            );
        }
        if !self.geometry.is_analytic() || matches!(self.geometry, Geometry::FunctionBased { .. }) {
            return None;
        }
        let ramp = self.ramp.as_ref()?;
        if !ramp.has_colorants() {
            return None;
        }
        let (x_lo, y_lo, x_hi, y_hi) = region;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let clamped = (
            x_lo.max(0) as u32,
            y_lo.max(0) as u32,
            x_hi.max(0) as u32,
            y_hi.max(0) as u32,
        );
        let width = buf.width() as i32;
        let deposit = !spot_planes.is_empty() && spot_planes.len() == ramp.spot_colorants().len();
        #[allow(clippy::cast_possible_wrap)]
        let changed = if deposit {
            buf.composite_overprint_varying_spots(clamped, rules, alpha, |x, y| {
                let (t, coverage) = sample_at(self, to_target, x as i32, y as i32, width, clip)?;
                let (process, tints) = ramp.at_authored(t)?;
                let mut s: [Option<f32>; crate::compositor::MAX_SPOTS] =
                    [None; crate::compositor::MAX_SPOTS];
                for (plane, tint) in spot_planes.iter().zip(tints.iter()) {
                    if let Some(slot) = s.get_mut(*plane) {
                        *slot = Some(*tint);
                    }
                }
                Some((process, s, coverage))
            })
        } else {
            buf.composite_overprint_varying(clamped, rules, alpha, |x, y| {
                let (t, coverage) = sample_at(self, to_target, x as i32, y as i32, width, clip)?;
                let c = ramp.at_cmyk(t)?;
                Some((c, coverage))
            })
        };
        Some(changed as usize)
    }

    #[must_use]
    pub fn paint(
        &self,
        to_target: tiny_skia::Transform,
        region: (i32, i32, i32, i32),
        clip: Option<&tiny_skia::Mask>,
        alpha: f32,
        pixmap: &mut tiny_skia::Pixmap,
    ) -> Option<usize> {
        // A mesh is FORWARD-rasterised and shares nothing below this line
        // but the arguments: no `Param`, no ramp lookup per pixel, no
        // inverse map except for `/BBox`. See `crate::mesh`'s module docs
        // for why the two algorithms are opposite rather than variations.
        if let Some(mesh) = self.mesh.as_ref() {
            return crate::mesh::paint(
                mesh,
                self.ramp.as_ref(),
                to_target,
                self.bbox,
                region,
                clip,
                alpha,
                pixmap,
            );
        }
        if !self.geometry.is_analytic() {
            return None;
        }
        // Type 1 is modelled but not painted in this build — see
        // `Geometry::param_at`. Asked here rather than inside the pixel
        // loop so a whole-region no-op costs one branch, not W×H of them.
        if matches!(self.geometry, Geometry::FunctionBased { .. }) {
            return None;
        }
        let ramp = self.ramp.as_ref()?;
        Some(paint_region(
            self, ramp, to_target, region, clip, alpha, pixmap,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // §8.7.4.5.3 — axial (SH25, SH26, SH27, SH29)
    // -----------------------------------------------------------------

    /// The horizontal reference case: axis from x=50 to x=150 at y=0.
    const AX: [f32; 4] = [50.0, 0.0, 150.0, 0.0];
    const D01: [f32; 2] = [0.0, 1.0];

    fn t_of(p: Param) -> Option<f32> {
        match p {
            Param::At(t) => Some(t),
            Param::Unpainted => None,
        }
    }

    #[test]
    fn axial_endpoints_are_exactly_zero_and_one() {
        // SH25's own anchoring sentence: "(0, 0) and (1, 0) in the domain
        // correspond respectively to (x0, y0) and (x1, y1) on the axis."
        assert_eq!(
            t_of(axial_param(AX, D01, [false, false], 50.0, 0.0)),
            Some(0.0)
        );
        assert_eq!(
            t_of(axial_param(AX, D01, [false, false], 150.0, 0.0)),
            Some(1.0)
        );
        assert_eq!(
            t_of(axial_param(AX, D01, [false, false], 100.0, 0.0)),
            Some(0.5)
        );
    }

    #[test]
    fn axial_bounds_are_inclusive_so_the_ends_are_not_a_gap() {
        // SH26 writes the interpolation branch as `0 <= x' <= 1`. Exactly 0
        // and exactly 1 therefore take the INTERPOLATION branch, not an
        // extend branch. With `/Extend [false false]` a strict `<`/`>` here
        // would leave a one-parameter-wide hole at each end that no test
        // sampling the interior would ever notice.
        assert!(t_of(axial_param(AX, D01, [false, false], 50.0, 0.0)).is_some());
        assert!(t_of(axial_param(AX, D01, [false, false], 150.0, 0.0)).is_some());
    }

    #[test]
    fn axial_ignores_the_perpendicular_coordinate() {
        // SH25: "all points along a line in domain space perpendicular to
        // the line from (0, 0) to (1, 0) have the same colour, only the new
        // value of x needs to be computed". There is no y'.
        let near = t_of(axial_param(AX, D01, [false, false], 100.0, 0.0));
        let far = t_of(axial_param(AX, D01, [false, false], 100.0, 9_999.0));
        assert_eq!(near, far);
    }

    #[test]
    fn axial_projection_normalises_by_the_squared_axis_length() {
        // The discriminating case, and the reason a diagonal axis is in the
        // fixture set. On a DIAGONAL axis, dividing by ‖axis‖ instead of
        // ‖axis‖² is off by a factor of ‖axis‖ — here 141.42 — which a
        // horizontal-axis test cannot see, because a horizontal unit axis
        // makes the two divisors differ by a factor the endpoints hide.
        //
        // Axis (0,0)->(100,100). The point (100, 0) projects to the axis
        // midpoint: ((100-0)*(100-0) + (100-0)*(0-0)) / (100² + 100²)
        // = 10000 / 20000 = 0.5. Dividing by ‖axis‖ would give 70.7.
        let diag = [0.0, 0.0, 100.0, 100.0];
        assert_eq!(
            t_of(axial_param(diag, D01, [true, true], 100.0, 0.0)),
            Some(0.5)
        );
        assert_eq!(
            t_of(axial_param(diag, D01, [true, true], 0.0, 100.0)),
            Some(0.5)
        );
    }

    #[test]
    fn axial_extend_false_leaves_the_point_unpainted() {
        // SH26, verbatim: "otherwise, t is undefined and the point shall be
        // left unpainted." NOT clamped, and NOT painted with the boundary
        // colour — those are what `/Extend true` does.
        assert_eq!(
            axial_param(AX, D01, [false, false], 40.0, 0.0),
            Param::Unpainted
        );
        assert_eq!(
            axial_param(AX, D01, [false, false], 160.0, 0.0),
            Param::Unpainted
        );
    }

    #[test]
    fn axial_extend_true_continues_the_boundary_colour_indefinitely() {
        // SH27: flat t0 / t1, "indefinitely" — so a point far outside gets
        // the same answer as one just outside.
        assert_eq!(
            t_of(axial_param(AX, D01, [true, true], 40.0, 0.0)),
            Some(0.0)
        );
        assert_eq!(
            t_of(axial_param(AX, D01, [true, true], -1e6, 0.0)),
            Some(0.0)
        );
        assert_eq!(
            t_of(axial_param(AX, D01, [true, true], 160.0, 0.0)),
            Some(1.0)
        );
        assert_eq!(
            t_of(axial_param(AX, D01, [true, true], 1e6, 0.0)),
            Some(1.0)
        );
        // Each end is independent.
        assert_eq!(
            axial_param(AX, D01, [true, false], 160.0, 0.0),
            Param::Unpainted
        );
        assert_eq!(
            axial_param(AX, D01, [false, true], 40.0, 0.0),
            Param::Unpainted
        );
    }

    #[test]
    fn axial_honours_a_non_unit_domain() {
        // SH26: t = t0 + (t1 - t0) * x'. `/Domain` is NOT required to be
        // [0 1], and a shading whose function is defined over [2 5] must be
        // fed values in that range or the ramp is indexed off its own end.
        let dom = [2.0, 5.0];
        assert_eq!(
            t_of(axial_param(AX, dom, [true, true], 50.0, 0.0)),
            Some(2.0)
        );
        assert_eq!(
            t_of(axial_param(AX, dom, [true, true], 150.0, 0.0)),
            Some(5.0)
        );
        assert_eq!(
            t_of(axial_param(AX, dom, [true, true], 100.0, 0.0)),
            Some(3.5)
        );
    }

    #[test]
    fn a_zero_length_axial_axis_paints_nothing() {
        // AMB-1 / SH29. ISO 32000-1 is SILENT here and the projection's
        // denominator is 0/0; ISO 32000-2 Table 79 adds "If the starting and
        // ending coordinates are coincident (x0=x1 and y0=y1) nothing shall
        // be painted."
        //
        // pdfcer applies the 2.0 rule at every version. A later edition
        // resolving an earlier silence is a forced default rather than a
        // choice between two readings — and it is also the only answer that
        // cannot emit NaN pixels, which is what an unguarded 0/0 would do.
        let degenerate = [50.0, 50.0, 50.0, 50.0];
        assert_eq!(
            axial_param(degenerate, D01, [true, true], 50.0, 50.0),
            Param::Unpainted
        );
        assert_eq!(
            axial_param(degenerate, D01, [true, true], 0.0, 0.0),
            Param::Unpainted
        );
    }

    // -----------------------------------------------------------------
    // §8.7.4.5.4 — radial (SH33, SH34, SH36, SH39, SH40, AMB-3)
    // -----------------------------------------------------------------

    /// Concentric, r0 = 0 -> r1 = 80, centred at (100,100). The ordinary
    /// "radial gradient" shape.
    const CONC: [f32; 6] = [100.0, 100.0, 0.0, 100.0, 100.0, 80.0];

    #[test]
    fn radial_uses_the_circumference_model_so_the_centre_is_t0() {
        // ★ The test that decides AMB-3, and the one most worth reading.
        //
        // ISO 32000-1 says a point lying "WITHIN more than one blend circle"
        // takes the colour of the greatest enclosing s; ISO 32000-2 changes
        // "within" to "ON". Those are discs versus circumferences, and they
        // are not a shade of meaning — they give opposite answers here.
        //
        // Under the DISC reading, the centre of this shading lies inside
        // every blend circle from s=0 to s=1, so the greatest s is 1 and the
        // centre paints t1. Worse, so does every other point inside the
        // outer circle, and the entire shading collapses to a flat t1 disc.
        // That the disc reading destroys the gradient it is describing is
        // the argument that it is a wording artefact, not the model.
        //
        // Under the CIRCUMFERENCE reading — solve |P - c(s)| = r(s) — only
        // the s=0 circle (a point, since r0=0) passes through the centre, so
        // the centre paints t0. That is 2.0's "on", it is what pdfcer
        // implements, and it is what pdfium produces (measured 2026-08-17).
        assert_eq!(
            t_of(radial_param(CONC, D01, [false, false], 100.0, 100.0)),
            Some(0.0)
        );
    }

    #[test]
    fn radial_interpolates_linearly_in_radius_not_in_area() {
        // SH33: r(s) = r0 + s*(r1 - r0) — a plain linear interpolation. A
        // by-area interpolation (which "looks more correct" for a glow and
        // is a real implementer's temptation) would put the midpoint colour
        // at radius 80/sqrt(2) = 56.6, not at 40.
        assert_eq!(
            t_of(radial_param(CONC, D01, [false, false], 140.0, 100.0)),
            Some(0.5)
        );
        assert_eq!(
            t_of(radial_param(CONC, D01, [false, false], 180.0, 100.0)),
            Some(1.0)
        );
        // Direction is irrelevant — the circles are circles.
        assert_eq!(
            t_of(radial_param(CONC, D01, [false, false], 100.0, 140.0)),
            Some(0.5)
        );
        assert_eq!(
            t_of(radial_param(CONC, D01, [false, false], 60.0, 100.0)),
            Some(0.5)
        );
    }

    #[test]
    fn radial_extend_false_leaves_the_outside_unpainted() {
        // Distance 100 needs s = 1.25, which is outside [0,1] and therefore
        // admissible only under `/Extend[1]`.
        assert_eq!(
            radial_param(CONC, D01, [false, false], 200.0, 100.0),
            Param::Unpainted
        );
    }

    #[test]
    fn radial_extend_true_admits_s_past_one_but_flattens_the_colour() {
        // SH39: "Blend circles extending beyond the ending circle shall be
        // painted in the colour defined for the ending circle (t = t1)."
        //
        // So the GEOMETRY continues (s > 1 is admitted, the circles keep
        // growing) while the COLOUR is flat. SH37 is explicit that this is
        // where radial `/Extend` differs from axial: implementing it as
        // "clamp s to [0,1]" before the geometry gives the right colour and
        // the wrong shape. Clamping AFTER admission, as here, gives both.
        assert_eq!(
            t_of(radial_param(CONC, D01, [false, true], 200.0, 100.0)),
            Some(1.0)
        );
        assert_eq!(
            t_of(radial_param(CONC, D01, [false, true], 1e5, 100.0)),
            Some(1.0)
        );
    }

    #[test]
    fn radial_with_both_radii_zero_paints_nothing() {
        // SH34, verbatim: "If both are 0, nothing shall be painted."
        let nothing = [100.0, 100.0, 0.0, 140.0, 100.0, 0.0];
        assert_eq!(
            radial_param(nothing, D01, [true, true], 100.0, 100.0),
            Param::Unpainted
        );
        assert_eq!(
            radial_param(nothing, D01, [true, true], 120.0, 100.0),
            Param::Unpainted
        );
    }

    #[test]
    fn radial_picks_the_greatest_s_when_two_blend_circles_both_pass_through_the_point() {
        // ★★ THE SELECTION-RULE TEST, and it exists because the first
        // version of this suite COULD NOT SEE the rule inverted.
        //
        // Sabotage check, 2026-08-17: swapping the greatest-root branch for
        // the smallest left all 17 tests green. SH39/SH40 is the sentence
        // the spec corpus singles out as "the single most-misimplemented in
        // the clause", and the suite was blind to exactly it. The reason was
        // that every earlier fixture had only ONE admissible root — on a
        // concentric shading the second root is negative, so hi-vs-lo never
        // arises and both orderings agree.
        //
        // This geometry is chosen so both roots are admissible AND both lie
        // inside [0,1], so no clamp can mask the difference either:
        //
        //   start circle  centre (60,100)  r0 = 10
        //   end circle    centre (140,100) r1 = 50
        //   probe point   (70,100)
        //
        // The s=0 circle passes through the probe (distance from (60,100) is
        // 10 = r0). So does the s=0.5 circle: centre (100,100), radius 30,
        // and the probe is 30 away. Two blend circles, both through the same
        // point, both admissible.
        //
        // SH39: the circles are painted opaquely in increasing s, so "its
        // final colour shall be that of the LAST of the enclosing circles to
        // be painted, corresponding to the GREATEST value of s". The answer
        // is 0.5, not 0.0.
        let two_roots = [60.0, 100.0, 10.0, 140.0, 100.0, 50.0];
        assert_eq!(
            t_of(radial_param(two_roots, D01, [false, false], 70.0, 100.0)),
            Some(0.5),
            "the GREATEST admissible s wins (SH39/SH40); 0.0 means the smallest root was taken and the painting order is inverted"
        );
    }

    #[test]
    fn radial_greatest_s_still_wins_when_the_larger_root_needs_extend() {
        // The same rule one step out: here the larger root is s = 1.0833,
        // admissible ONLY because `/Extend[1]` is true. With extend false
        // the answer legitimately falls back to the smaller root, so this
        // pair also proves the admissibility filter runs BEFORE the
        // greatest-wins choice rather than after it.
        let cone = [60.0, 100.0, 25.0, 140.0, 100.0, 45.0];
        // Extended: the larger root wins, and its colour flattens to t1.
        assert_eq!(
            t_of(radial_param(cone, D01, [true, true], 100.0, 100.0)),
            Some(1.0)
        );
        // Not extended: the larger root is inadmissible, so the smaller one
        // is the greatest ADMISSIBLE s. Not the same number, and not a
        // contradiction.
        let unextended = t_of(radial_param(cone, D01, [false, false], 100.0, 100.0));
        assert!(
            unextended.is_some_and(|t| (t - 0.15).abs() < 1e-4),
            "expected the smaller root 0.15 when extension is off, got {unextended:?}"
        );
    }

    #[test]
    fn radial_t_does_not_run_backwards_along_a_cone() {
        // SH39/SH40, the selection rule. On a cone — neither circle inside
        // the other — a point can sit on TWO blend circles, and the spec's
        // opaque increasing-s painting order means the larger s wins.
        //
        // Both radii non-zero and centres apart: this is precisely the shape
        // `tiny_skia::RadialGradient` cannot represent, so it is also the
        // case that would silently degrade if pdfcer ever delegated.
        let cone = [60.0, 100.0, 25.0, 140.0, 100.0, 45.0];
        // A point on the axis between the two centres is enclosed by a range
        // of blend circles; the answer must be the largest admissible one,
        // so it must exceed what the smallest enclosing circle would give.
        let mid = t_of(radial_param(cone, D01, [false, false], 100.0, 100.0));
        assert!(mid.is_some(), "a point inside the cone must be painted");
        let mid = mid.unwrap();
        assert!(
            (0.0..=1.0).contains(&mid),
            "an unextended cone yields s in [0,1], got {mid}"
        );
        // Sanity on ordering: moving toward the ending circle's centre must
        // not DECREASE t on this configuration.
        let nearer_end = t_of(radial_param(cone, D01, [false, false], 130.0, 100.0)).unwrap();
        assert!(
            nearer_end >= mid,
            "t must not run backwards along the cone: {mid} -> {nearer_end}"
        );
    }

    #[test]
    fn radial_honours_a_non_unit_domain() {
        let dom = [10.0, 20.0];
        assert_eq!(
            t_of(radial_param(CONC, dom, [false, false], 100.0, 100.0)),
            Some(10.0)
        );
        assert_eq!(
            t_of(radial_param(CONC, dom, [false, false], 140.0, 100.0)),
            Some(15.0)
        );
        assert_eq!(
            t_of(radial_param(CONC, dom, [false, false], 180.0, 100.0)),
            Some(20.0)
        );
    }

    // -----------------------------------------------------------------
    // The ramp
    // -----------------------------------------------------------------

    #[test]
    fn a_ramp_samples_both_endpoints_exactly() {
        // The mapping is t = t0 + (t1-t0)*i/(N-1), so index 0 is exactly t0
        // and index N-1 is exactly t1. Sampling at bin CENTRES instead would
        // put the end colours half a step inside the domain, and a two-stop
        // gradient would never reach either of its declared colours — a
        // small error that is invisible on a subtle ramp and obvious on a
        // black-to-white one.
        let ramp = ColorRamp {
            // Empty: these tests exercise the sRGB lookup, and an empty
            // colorant vector is the honest state for a ramp built from a
            // space that has none.
            cmyk: Vec::new(),
            process: Vec::new(),
            spots: Vec::new(),
            spot_colorants: Vec::new(),
            samples: (0..RAMP_SAMPLES)
                .map(|i| {
                    #[allow(clippy::cast_precision_loss)]
                    let v = i as f32 / (RAMP_SAMPLES - 1) as f32;
                    Some(Rgb { r: v, g: v, b: v })
                })
                .collect(),
            domain: [0.0, 1.0],
        };
        assert_eq!(ramp.at(0.0).unwrap().r, 0.0);
        assert_eq!(ramp.at(1.0).unwrap().r, 1.0);
        // Out-of-domain clamps to the nearest end rather than panicking.
        // This is the RAMP's contract and says nothing about `/Extend`:
        // whether a point outside is painted at all was decided by the
        // geometry before this is ever called.
        assert_eq!(ramp.at(-5.0).unwrap().r, 0.0);
        assert_eq!(ramp.at(5.0).unwrap().r, 1.0);
    }

    #[test]
    fn a_degenerate_ramp_domain_does_not_divide_by_zero() {
        let ramp = ColorRamp {
            // Empty: these tests exercise the sRGB lookup, and an empty
            // colorant vector is the honest state for a ramp built from a
            // space that has none.
            cmyk: Vec::new(),
            process: Vec::new(),
            spots: Vec::new(),
            spot_colorants: Vec::new(),
            samples: vec![Some(Rgb::BLACK); RAMP_SAMPLES],
            domain: [3.0, 3.0],
        };
        assert!(ramp.at(3.0).is_some());
        assert!(ramp.at(0.0).is_some());
    }
}

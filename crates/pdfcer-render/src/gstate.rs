//! # Graphics state (ISO 32000-1 §8.4)
//!
//! The device-independent graphics-state parameters Pass 1's
//! interpreter consults, plus the `q`/`Q` stack. Spec sources:
//! `iso32000__s__8.4.md` (Table 52 initial values, Table 57 operators),
//! `iso32000__s__8.4.3.md` (caps/joins/miter/dash),
//! `iso32000__s__8.6.md` (device colour spaces + per-space initial
//! colours), `iso32000__s__8.3.md` (CTM) in the PDF-spec RAG.
//!
//! ## What is and isn't in the state
//!
//! Per §8.5.2.1 (a deliberate PDF-vs-PostScript divergence): **the
//! current path is NOT part of the graphics state** — `q`/`Q` never
//! save or restore it. The **clipping path IS** (Table 52). The Pass 1
//! subset here: CTM, stroke/fill colour (device spaces only), line
//! width/cap/join/miter/dash, the clip, and the **text state**.
//! ExtGState (`gs`) keys are honored for the subset that maps to these
//! fields (Table 58 triage per the RAG: LW/LC/LJ/ML/D honored; the
//! rest recognize-and-defer).
//!
//! ## Why the text state is in here
//!
//! §9.3's first sentence: "the text state comprises those **graphics
//! state** parameters that only affect text." All nine of Table 104's
//! parameters — including the selected font and size — are therefore
//! saved by `q` and restored by `Q`, exactly like the line width, and
//! §9.3's scope rule adds that they "may appear outside text objects",
//! persist across text objects in a content stream, and are reset only
//! at the start of each page.
//!
//! The text **matrices** are the opposite case and are deliberately NOT
//! here: §9.4.1 confines `Tm`/`Tlm`/`Trm` to a single `BT`…`ET` block,
//! so they live in [`crate::text::TextObject`], owned by the
//! interpreter. A `q`/`Q` pair inside a text object must not move the
//! pen.
//!
//! ## Colour model (Pass 1)
//!
//! Device colour spaces only (§8.6.4): DeviceGray, DeviceRGB,
//! DeviceCMYK, set by `g`/`G`, `rg`/`RG`, `k`/`K` (Table 74). Initial
//! colour is black in every device space (§8.6.4: gray 0 / RGB 0,0,0 /
//! CMYK 0,0,0,1). Colours are stored converted to RGB at set time.
//!
//! **The conversion itself is not here.** All three device spaces
//! delegate to [`pdfcer_core::color`], which is the single conversion
//! site in the project — the `k`/`K` operators, `DeviceCMYK` image
//! samples, and `pdfcer-core`'s decomposed-object colour record all pass
//! through the same function. Two CMYK conversions that disagree would
//! paint a filled rectangle and an image of the "same" CMYK in visibly
//! different colours within one document, which is precisely the class
//! of divergence this crate exists to avoid.
//!
//! Note the consequence for `DeviceCMYK`'s initial colour: CMYK
//! `(0,0,0,1)` is solid **black ink**, which the calibrated conversion
//! renders as a warm near-black rather than `#000000`. That is the
//! reference behaviour, not a defect — see `pdfcer_core::color`'s module
//! docs §1–§2 for why an untagged device colour has no "correct" RGB
//! and pdfcer is therefore choosing rather than matching.

use pdfcer_core::settings::CmykIntent;
use tiny_skia::Transform;

/// A 2-D affine transform in `f64`, kept alongside the `f32`
/// [`GraphicsState::ctm`] that `tiny_skia` actually consumes.
///
/// # Why this exists, and what goes wrong without it
///
/// `tiny_skia::Transform` is `f32`. Composing a content-stream `cm` that
/// carries a **page coordinate** with a deep-zoom base CTM produces a
/// translation that is the difference of two large, nearly equal numbers:
///
/// ```text
///   base:  sx = 8_100_000            tx = -4_374_000_000
///   cm:    sx = 2.8e-9               tx = 540
///   =>     tx' = 540 * 8_100_000 + (-4_374_000_000) = a few hundred
/// ```
///
/// The **answer** is small; both **operands** are ~4.4e9, where `f32`'s
/// spacing is **512**. So `tx'` came out quantised in 512-pixel steps —
/// and because a translation error moves every point of the form
/// equally, the symptom was not distortion but a whole drawing sitting
/// hundreds of pixels from where it belonged, or off the canvas
/// entirely. Measured on `tools/gen-scale-demo`: of eleven Form XObjects
/// framed on one molecule, **11 rendered at scale 2e6, 7 at 1.25e7, 3 at
/// 2.5e7, and 1 at 5e7**.
///
/// Computing the composition in `f64` and narrowing **the small result**
/// removes it entirely: `f64`'s spacing at 4.4e9 is 1e-6, so the
/// cancellation is exact to far below a pixel, and the `f32` value handed
/// to `tiny_skia` is then a few hundred — a magnitude `f32` represents to
/// seven digits.
///
/// This is the same trick `Pass 74.2` used for the base CTM
/// (`crate::region_base_geometry_of` — "the subtraction happens HERE, in
/// `f64`, and only its small result is narrowed"), pushed one level down
/// to content-stream composition, which is where it was still missing.
///
/// # What this does NOT fix, and what does
///
/// Path points that are themselves large page coordinates. A point near
/// `x = 540` has an `f32` spacing of `6.1e-5 pt` — **21.5 µm** — so any
/// feature smaller than that, written as an absolute page coordinate, is
/// quantised before any matrix touches it. No amount of precision in the
/// matrix recovers it.
///
/// That is the SECOND of `Pass 74.7`'s two algorithms, gated on
/// [`Self::needs_precise_paths`] because it costs per point while this
/// costs per `cm`: when the gate fires, the interpreter builds the path
/// **relative to its own first point**, differencing in `f64` so what
/// reaches `tiny_skia` is a set of small offsets instead of a set of
/// nearly-equal large numbers. The CTM handed alongside keeps its linear
/// part and carries the origin's own mapped position.
///
/// ★ The first attempt built the path in DEVICE space instead, with an
/// identity transform. It was correct, and it was **three times slower**
/// at extreme zoom on a stroke-heavy CAD sheet (93 s against 31 s),
/// because `tiny_skia` flattens curves to a tolerance measured in the
/// path's own units — and a path whose coordinates are millions gets
/// subdivided accordingly. It also forced a similarity restriction, since
/// an identity transform cannot scale a pen, and a helper existed here to
/// test for one.
///
/// Differencing instead of transforming removed all three problems at
/// once: coordinates stay at user-space magnitude so flattening is
/// unchanged, the linear part still reaches `tiny_skia` so strokes and
/// dashes need no adjustment, and any affine CTM is admissible. The same
/// CAD region went from 31 s to **1.3 s** — the deep-zoom cost was never
/// really about precision, it was the same large magnitudes hurting the
/// rasteriser in a different way.
///
/// # Convention
///
/// PDF's row-vector convention, matching `tiny_skia`: a point is a row
/// `[x y 1]` multiplied on the LEFT of the matrix, and
/// [`Self::post_concat`] means *apply `self` first, then `other`* — the
/// same sense as `Transform::post_concat`, verified against it by
/// `mat64_post_concat_matches_tiny_skia`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat64 {
    /// Row 1 column 1 — x scale.
    pub sx: f64,
    /// Row 1 column 2 — y shear.
    pub ky: f64,
    /// Row 2 column 1 — x shear.
    pub kx: f64,
    /// Row 2 column 2 — y scale.
    pub sy: f64,
    /// Row 3 column 1 — x translation.
    pub tx: f64,
    /// Row 3 column 2 — y translation.
    pub ty: f64,
}

impl Mat64 {
    /// The identity.
    pub const IDENTITY: Self = Self {
        sx: 1.0,
        ky: 0.0,
        kx: 0.0,
        sy: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    /// From the six numbers of a PDF matrix `[a b c d e f]`, in that
    /// order — which is `from_row(sx, ky, kx, sy, tx, ty)`, the same
    /// argument order `tiny_skia::Transform::from_row` uses.
    #[must_use]
    pub const fn from_row(sx: f64, ky: f64, kx: f64, sy: f64, tx: f64, ty: f64) -> Self {
        Self {
            sx,
            ky,
            kx,
            sy,
            tx,
            ty,
        }
    }

    /// Widen an `f32` transform. Lossless — every `f32` is an `f64`.
    #[must_use]
    pub fn from_f32(t: Transform) -> Self {
        Self {
            sx: f64::from(t.sx),
            ky: f64::from(t.ky),
            kx: f64::from(t.kx),
            sy: f64::from(t.sy),
            tx: f64::from(t.tx),
            ty: f64::from(t.ty),
        }
    }

    /// Narrow to the `f32` transform `tiny_skia` consumes.
    ///
    /// The precision that matters is spent BEFORE this call, not saved by
    /// avoiding it: the composition happened in `f64`, so what is narrowed
    /// here is the small result rather than the large operands.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn to_f32(self) -> Transform {
        Transform::from_row(
            self.sx as f32,
            self.ky as f32,
            self.kx as f32,
            self.sy as f32,
            self.tx as f32,
            self.ty as f32,
        )
    }

    /// Apply `self`, then `other` — PDF's `CTM' = M × CTM` for `cm`
    /// (§8.3.4), and the same sense as `Transform::post_concat`.
    #[must_use]
    pub fn post_concat(self, other: Self) -> Self {
        Self {
            sx: self.sx * other.sx + self.ky * other.kx,
            ky: self.sx * other.ky + self.ky * other.sy,
            kx: self.kx * other.sx + self.sy * other.kx,
            sy: self.kx * other.ky + self.sy * other.sy,
            tx: self.tx * other.sx + self.ty * other.kx + other.tx,
            ty: self.tx * other.ky + self.ty * other.sy + other.ty,
        }
    }

    /// Map a point.
    #[must_use]
    pub fn map(self, x: f64, y: f64) -> (f64, f64) {
        (
            x * self.sx + y * self.kx + self.tx,
            x * self.ky + y * self.sy + self.ty,
        )
    }

    /// Does painting through this transform need `f64` PATH COORDINATES,
    /// not merely an `f64` composition?
    ///
    /// # The predicate, and why one number covers both mechanisms
    ///
    /// A device coordinate is `point * scale + translation`. When the
    /// result is small — which it is, or it would not be on the canvas —
    /// the two terms are nearly equal and opposite, so **both** are about
    /// `|translation|`. That makes `|translation|` the magnitude at which
    /// the cancellation happens, whichever operand carries it:
    ///
    /// - a large `tx` composed into the matrix (fixed by `Mat64` itself), and
    /// - a large path coordinate scaled up (fixed only by building the
    ///   path in device space),
    ///
    /// both lose about `|translation| * 2^-23` device pixels. One
    /// comparison therefore answers for both.
    ///
    /// The threshold is **1/20 of a device pixel**, giving
    /// `|t| > 0.05 * 2^23 ≈ 419_000`. Worked through:
    ///
    /// | render | `max(|tx|,|ty|)` | error | precise? |
    /// |---|---|---|---|---|
    /// | whole page at 1.6x | ~1 300 | 0.0002 px | no |
    /// | the two cells, ~62 000 % | ~334 000 | 0.04 px | no |
    /// | one mitochondrion, ~16 M % | ~8.5e7 | 10 px | **yes** |
    /// | the molecule box, ~1.5 G % | ~4.4e9 | 524 px | **yes** |
    ///
    /// So ordinary rendering never pays for it, which is the requirement
    /// this method exists to satisfy: *fix the precision without
    /// affecting speed where the precision is not needed*.
    #[must_use]
    pub fn needs_precise_paths(self) -> bool {
        // 2^-23 is the relative spacing of `f32`'s 24-bit significand.
        const F32_REL: f64 = 1.0 / 8_388_608.0;
        const MAX_DEVICE_ERROR_PX: f64 = 0.05;
        self.tx.abs().max(self.ty.abs()) * F32_REL > MAX_DEVICE_ERROR_PX
    }
}

/// Line cap style (Table 54: 0 butt, 1 round, 2 projecting square).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap {
    /// 0 — butt: squared off at the endpoint.
    Butt,
    /// 1 — round: semicircle over the endpoint.
    Round,
    /// 2 — projecting square: extends half a line width beyond.
    Square,
}

/// Line join style (Table 55: 0 miter, 1 round, 2 bevel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoin {
    /// 0 — mitered corner (subject to the miter limit).
    Miter,
    /// 1 — rounded corner.
    Round,
    /// 2 — beveled (truncated) corner.
    Bevel,
}

/// An RGB colour in [0, 1] components — the Pass 1 working colour
/// (module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    /// Red component, 0.0–1.0.
    pub r: f32,
    /// Green component, 0.0–1.0.
    pub g: f32,
    /// Blue component, 0.0–1.0.
    pub b: f32,
}

impl Rgb {
    /// Black — the initial colour in every device space (§8.6.4).
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
    };

    /// Build from a `[r, g, b]` triple as [`pdfcer_core::color`] returns one.
    const fn from_triple([r, g, b]: [f32; 3]) -> Self {
        Self { r, g, b }
    }

    /// From a DeviceGray value (`g`/`G`): gray 0 = black, 1 = white
    /// (§8.6.4.2).
    #[must_use]
    pub fn from_gray(v: f32) -> Self {
        Self::from_triple(pdfcer_core::color::gray_to_srgb(v))
    }

    /// From DeviceRGB components (`rg`/`RG`) — §8.6.4.3.
    #[must_use]
    pub fn from_rgb(r: f32, g: f32, b: f32) -> Self {
        Self::from_triple(pdfcer_core::color::rgb_to_srgb(r, g, b))
    }

    /// From DeviceCMYK components (`k`/`K`) — §8.6.4.4, via the operator's
    /// chosen conversion in [`pdfcer_core::color::cmyk_to_srgb_with`].
    ///
    /// The intent is a **parameter rather than a global** because §8.6.4.4
    /// mandates no conversion at all: the answer is the operator's, so it
    /// has to travel with the render that used it. A process-wide setting
    /// would be one line shorter and would make two renders of the same
    /// page differ for a reason not visible at either call site — which is
    /// exactly the property `tools/render-parity` depends on not having.
    #[must_use]
    pub fn from_cmyk(intent: CmykIntent, c: f32, m: f32, y: f32, k: f32) -> Self {
        Self::from_triple(pdfcer_core::color::cmyk_to_srgb_with(intent, c, m, y, k))
    }
}

/// The Pass 1 graphics-state subset (module docs), with Table 52 /
/// §8.4.3.6 / §8.6.4 initial values in `default_with_ctm`.
#[derive(Debug, Clone)]
pub struct GraphicsState {
    /// Current transformation matrix: user space → device space
    /// (§8.3.4; initial value is device-dependent, supplied by the
    /// caller from page geometry + zoom).
    ///
    /// **Derived from [`Self::ctm64`], never composed directly.** Every
    /// write is `ctm64.to_f32()`; composing here instead would reinstate
    /// the cancellation [`Mat64`] exists to prevent. Read freely — this
    /// is the value `tiny_skia` consumes.
    pub ctm: Transform,
    /// The same transform in `f64`, and the one composition happens in.
    ///
    /// Kept as a second field rather than replacing [`Self::ctm`] because
    /// every painting call takes a `tiny_skia::Transform` and would
    /// otherwise narrow at each of two dozen call sites. One narrowing,
    /// at the point of composition, is both cheaper and easier to reason
    /// about: there is exactly one place where precision is lost, and it
    /// is after the arithmetic that needed it.
    pub ctm64: Mat64,
    /// Stroking colour (used by `S`/`s` and the stroke half of `B`…).
    pub stroke_color: Rgb,
    /// Non-stroking colour (fills, and the fill half of `B`…).
    pub fill_color: Rgb,
    /// Line width in user-space units (Table 52 initial: 1.0).
    pub line_width: f32,
    /// Line cap (Table 52 initial: 0 = butt).
    pub line_cap: LineCap,
    /// Line join (Table 52 initial: 0 = miter).
    pub line_join: LineJoin,
    /// Miter limit (Table 52 initial: 10.0).
    pub miter_limit: f32,
    /// Dash pattern `(array, phase)` in user-space units (§8.4.3.6
    /// initial: solid — empty array, phase 0).
    pub dash: (Vec<f32>, f32),
    /// Constant alpha for **non-stroking** painting — `/ca` (§11.6.4.4,
    /// Table 58). Initial value 1.0.
    ///
    /// "Constant" distinguishes it from the alpha a `/SMask` would
    /// supply per pixel: this is one number applied to everything the
    /// operation paints. That is what makes it cheap to honour and what
    /// makes ignoring it so visible — a 0.5 fill rendered opaque is not
    /// subtly off, it is the wrong colour everywhere it covers.
    ///
    /// Lives on the graphics state rather than beside the paint so that
    /// `q`/`Q` save and restore it for free (§8.4.2), which is the whole
    /// reason Table 58's entries belong here.
    pub fill_alpha: f32,
    /// Constant alpha for **stroking** painting — `/CA` (§11.6.4.4,
    /// Table 58). Initial value 1.0.
    ///
    /// Separate from [`Self::fill_alpha`] because the standard makes
    /// them separate: a single `gs` may set either, both, or neither,
    /// and a stroke and a fill in the same operation can legitimately
    /// differ in opacity.
    pub stroke_alpha: f32,
    /// The current **blend mode** — `/BM` (§11.3.5, Table 58). Initial
    /// value `Normal`.
    ///
    /// Lives here for the same reason the two alphas do: `q`/`Q` must save
    /// and restore it (§8.4.2), and Table 58's entries are graphics-state
    /// parameters, not paint parameters.
    ///
    /// # Why this is a `tiny_skia::BlendMode` and not a pdfcer enum
    ///
    /// A pdfcer enum would have to be mapped to a `tiny_skia::BlendMode` at
    /// every paint site — four of them — and a mapping performed four
    /// times is a mapping that can disagree with itself three ways.
    /// Resolving ONCE, at the `gs` operator, means every paint site is
    /// handed a value that is already correct or already refused.
    ///
    /// The mapping itself is [`blend_mode_from_name`], and it is the only
    /// place in the crate that knows a PDF blend-mode name. Its
    /// correctness against ISO 32000-1 Tables 136 and 137 is asserted
    /// numerically in `crates/pdfcer-render/tests/blend_modes.rs` rather
    /// than assumed from the two specifications' shared ancestry.
    pub blend_mode: tiny_skia::BlendMode,
    /// Overprint for **stroking** operations — `/OP` (§8.6.7, Table 58).
    /// Initial value `false`.
    pub overprint_stroke: bool,
    /// Overprint for **non-stroking** operations — `/op` (Table 58).
    /// Initial value `false`.
    ///
    /// Table 58 makes `/OP` set BOTH parameters unless `/op` appears in the
    /// SAME ExtGState dictionary, which is why these are set together at
    /// the operator rather than independently here.
    pub overprint_fill: bool,
    /// Overprint mode — `/OPM` (§8.6.7). `0` or `1`; initial value `0`.
    ///
    /// Mode 1 is the "nonzero overprint mode" in which a zero DeviceCMYK
    /// component leaves the backdrop alone.
    ///
    /// **APPLIED since `bf75351`.** This doc used to end "pdfcer renders to
    /// an additive RGB display, so on the shipped path this is tracked and
    /// reported rather than applied", which stopped being true when
    /// `overprint::composite` shipped and was not revised then. Third of
    /// four copies of the same stale narrative found on 2026-08-18.
    ///
    /// The spec nuance the old wording was reaching for is real and worth
    /// keeping: §8.6.7 says mode 1 "shall not apply if the device's native
    /// colour space is not `DeviceCMYK`", and pdfcer's raster target is
    /// additive RGB. pdfcer applies it anyway because it **simulates** a
    /// DeviceCMYK device per pixel — reconstructing CMYK, applying Table
    /// 149, converting back. That simulation is Overprint Preview, which
    /// ISO 32000-1 never describes for a non-separating device, so it is a
    /// product decision rather than a conformance obligation. Acrobat makes
    /// the same one automatically for PDF/X.
    ///
    /// Stored as the `i64` the file carried rather than a `bool`: `OP-N2`
    /// records that values other than 0 and 1 have no specified behaviour,
    /// and pdfcer keeps what it read instead of normalising it away.
    pub overprint_mode: i64,
    /// The current **rendering intent** — `/RI` and the `ri` operator
    /// (§8.6.5.8, ISO 32000-1 Table 70) (`Pass 199.0`).
    ///
    /// Initial value `RelativeColorimetric` (ISO 32000-1 Table 52's *Initial
    /// value*, made binding by §8.4.1, `shall`).
    ///
    /// ★ **`gs` does not reset it.** An `/ExtGState` with no `/RI` leaves this
    /// alone: §8.4.5 makes `gs` cumulative, and ISO 32000-2's uniquely-printed
    /// "The default value is: Default" for that entry was DELETED by
    /// ISO-approved erratum `pdf-issues` #360 for exactly that reason. It was
    /// re-raised in 2026 and closed as a duplicate, so it is a live trap.
    ///
    /// ★★ **This governs PAINTING, not the page group's conversion to the
    /// device.** §11.7.5.3 (`shall`) ties a painting operation to the intent in
    /// force at that moment; the page-group-to-device hop is a separate step
    /// with its own answer (`RelativeColorimetric` per ISO 32000-2 §11.4.7).
    /// Reusing this field for that conversion would apply a source-side choice
    /// to a destination-side one.
    pub rendering_intent: pdfcer_core::color::RenderingIntent,
    /// Current clipping path as a device-space mask, `None` = the
    /// initial clip = the entire page (§8.5.4). Stored rasterized
    /// (tiny-skia `Mask`) because PDF only ever intersects clips —
    /// never enlarges them (§8.5.4 NOTE 2) — so a mask composes by
    /// per-pixel multiplication without needing path booleans.
    ///
    /// # Why `Arc`, and why sharing is sound
    ///
    /// A `Mask` is page-sized (one byte per pixel: ~1 MB at 1191×842),
    /// and `q` pushes a **clone** of the whole graphics state. On a CAD
    /// sheet measured 2026-08-07 that is 129,951 `q` operations against
    /// a live clip — **6.8 seconds of pure memcpy**, the single largest
    /// cost in a 17.5 s render, larger than rasterizing every clip path.
    ///
    /// Sharing is sound because **a clip is never mutated in place**.
    /// `intersect_clip` builds a *fresh* mask and assigns it; the old
    /// one is only ever read. So `q` needs a new *reference*, not a new
    /// buffer, and `Q` drops one. No copy-on-write is required — there
    /// is no write.
    ///
    /// This is why the type is `Arc<Mask>` and not `Rc<Mask>`: nothing
    /// here is threaded today, but `pdfcer-render` is a library whose
    /// callers may render pages in parallel, and `Rc` would make
    /// `GraphicsState` non-`Send` for a saving of one non-atomic
    /// increment per `q`.
    pub clip: Option<std::sync::Arc<tiny_skia::Mask>>,
    /// The clip as it stood BEFORE a soft mask was folded into it, or
    /// [`None`] when no soft mask is in force.
    ///
    /// §11.6.5's soft mask multiplies every paint's coverage, which is
    /// exactly what [`Self::clip`] already does at every paint site in the
    /// renderer — so a soft mask is applied by multiplying it into the clip
    /// rather than by threading a second mask through ten call sites.
    ///
    /// That leaves one thing to undo: `gs` with `/SMask /None` **resets**
    /// the soft mask (Table 58), and a mask already multiplied into the
    /// clip cannot be divided back out. This holds the pre-multiplication
    /// clip so the reset can restore it exactly.
    ///
    /// Saved and restored by `q`/`Q` along with the rest of the state, so
    /// the common `q … gs … Q` shape needs no help from it at all.
    ///
    /// KNOWN LIMIT, disclosed rather than hidden: a `W n` clip established
    /// **while** a soft mask is in force updates [`Self::clip`] but not
    /// this snapshot, so a subsequent `/SMask /None` in the same `q` level
    /// would restore a clip that predates that `W n`. The renderer counts
    /// that case (`soft_masks_reset_stale`) instead of silently producing
    /// the wrong clip.
    pub clip_before_smask: Option<Option<std::sync::Arc<tiny_skia::Mask>>>,
    /// The soft mask **as built**, before it was multiplied into
    /// [`Self::clip`] — §11.6.5.
    ///
    /// # Why the same mask is kept twice
    ///
    /// Because it is two different things depending on what it is applied
    /// to, and until `Pass 97.0` pdfcer only had the first:
    ///
    /// * For an **elementary object** — a fill, a stroke, a glyph, an
    ///   image — §11.6.4.1 makes the mask value the object's `q_m`, which
    ///   multiplies coverage exactly as a clip does. Folding it into
    ///   [`Self::clip`] is therefore the operation, not an approximation
    ///   of it.
    /// * For a **transparency group** — §11.4.5 — the mask applies to the
    ///   group's **RESULT**, once, after its contents have been
    ///   composited together. Folding it into the clip applies it to each
    ///   object *inside* instead, which multiplies twice wherever two of
    ///   them overlap and reaches the wrong answer wherever they blend.
    ///
    /// So the folded copy stays (every paint site already honours it) and
    /// this one exists so a group `Do` can take the mask **out** of its
    /// contents' clip and hand it to the composite.
    ///
    /// §11.6.6 also requires the mask to be **reset to `None` inside** the
    /// group, "to ensure that they are not applied twice" — which is the
    /// same statement from the other side, and is why a group's inner
    /// state clears this field.
    pub soft_mask: Option<std::sync::Arc<tiny_skia::Mask>>,
    /// Number of clips established since the soft mask was set, used only
    /// to detect the stale-restore case described on
    /// [`Self::clip_before_smask`].
    pub clips_since_smask: u32,
    /// Device-space bounding box of [`Self::clip`]'s non-zero region, as
    /// `(left, top, right, bottom)`. `None` exactly when `clip` is.
    ///
    /// It lives HERE, in the graphics state, rather than in a side table,
    /// and that placement is the whole correctness argument: a clip bbox
    /// must be saved by `q` and restored by `Q` exactly as the mask is,
    /// because `Q` reinstates a LARGER clip. Tracked outside the state it
    /// shrinks monotonically and never widens, which on the reference CAD
    /// sheet made a 1.34% bounding-box cull rate measure as 73.71%.
    ///
    /// Maintained today only to feed [`crate::profile`]; it is a `Copy`
    /// 16-byte field, so `q` pays nothing meaningful for it.
    pub clip_bbox: Option<(f32, f32, f32, f32)>,
    /// The same clip as [`Self::clip`], expressed as an index into a
    /// display list's clip table instead of as a built mask — `None` while
    /// rendering, `Some` while recording (`crate::display_list`).
    ///
    /// # Why this lives HERE, beside the mask, and not in the recorder
    ///
    /// For exactly the reason [`Self::clip_bbox`] does: **`Q` reinstates a
    /// LARGER clip.** A recorder tracking the current clip on its own stack
    /// would have to mirror `q`/`Q`, and a second stack that must stay in
    /// step with the first is a second stack that will not. Kept in the
    /// graphics state, it is saved and restored by the same `q`/`Q` that
    /// saves and restores everything else, for free and by construction.
    ///
    /// The two are written **as a pair** ([`Self::clip_ref`] reads them as
    /// one), and the failure mode of letting them diverge is a recorded
    /// paint that ignores its clip — visible only on replay.
    pub clip_id: Option<crate::display_list::ClipId>,
    /// The §11.3.5.3 **non-separable** blend mode in force, if any.
    ///
    /// Carried BESIDE [`Self::blend_mode`] rather than inside it, because the
    /// two are computed by different machinery and must not be
    /// interchangeable: [`Self::blend_mode`] is handed to the rasteriser, and
    /// these four are exactly the ones the rasteriser gets wrong
    /// (`ARCHITECTURE.md` §12 decision 066). One field would make routing a
    /// non-separable mode to `tiny_skia` a type-correct mistake.
    ///
    /// When this is `Some`, [`Self::blend_mode`] stays `SourceOver` and the
    /// paint path composites per pixel through [`crate::blend_nonsep`]. That
    /// pairing is deliberate: a paint site that has not been taught about
    /// this field composites NORMALLY rather than with a wrong rule, which
    /// is the same fail-safe direction the old refusal had.
    pub nonseparable: Option<crate::blend_nonsep::NonSeparableBlend>,
    /// The nine §9.3 text-state parameters (module docs: they ARE
    /// graphics-state parameters, so `q`/`Q` save and restore them).
    pub text: crate::text::TextState,
}

impl GraphicsState {
    /// Set the CTM from its `f64` form, keeping the `f32` copy in step.
    ///
    /// The ONLY sanctioned way to change the CTM. `ctm` and `ctm64` are a
    /// pair whose whole value is that they agree; assigning `ctm`
    /// directly would let them diverge silently, and the divergence would
    /// show up as content drawn in the wrong place at high zoom — a
    /// symptom nobody would trace back to a field assignment.
    pub fn set_ctm64(&mut self, m: Mat64) {
        self.ctm64 = m;
        self.ctm = m.to_f32();
    }

    /// The §8.4/§8.6 initial state over a caller-supplied device CTM.
    #[must_use]
    pub fn default_with_ctm(ctm: Transform) -> Self {
        Self::default_with_ctm64(Mat64::from_f32(ctm))
    }

    /// The same, seeded from an `f64` base transform.
    ///
    /// Preferred at every entry point that HAS the `f64` coefficients —
    /// a region render computes them that way (`Pass 74.2`) and then
    /// narrows, so going through [`Self::default_with_ctm`] there throws
    /// away precision one line before the code that needs it.
    #[must_use]
    pub fn default_with_ctm64(ctm64: Mat64) -> Self {
        let ctm = ctm64.to_f32();
        Self {
            ctm,
            ctm64,
            stroke_color: Rgb::BLACK,
            fill_color: Rgb::BLACK,
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            dash: (Vec::new(), 0.0),
            fill_alpha: 1.0,
            stroke_alpha: 1.0,
            // Table 58's initial value for /BM is `Normal`, which is
            // Porter-Duff source-over — tiny_skia's own default.
            blend_mode: tiny_skia::BlendMode::SourceOver,
            // Table 58 initial values: overprint off, overprint mode 0.
            overprint_stroke: false,
            overprint_fill: false,
            overprint_mode: 0,
            // Table 52's *Initial value* for the rendering intent.
            rendering_intent: pdfcer_core::color::RenderingIntent::RelativeColorimetric,
            clip: None,
            clip_before_smask: None,
            soft_mask: None,
            clips_since_smask: 0,
            clip_bbox: None,
            clip_id: None,
            nonseparable: None,
            text: crate::text::TextState::default(),
        }
    }

    /// The clip in force, in whichever representation the caller's drawing
    /// target needs.
    ///
    /// The single reader of both fields, so a paint site cannot pass the
    /// mask and forget the id — see [`Self::clip_id`].
    pub(crate) fn clip_ref(&self) -> crate::canvas::ClipRef<'_> {
        crate::canvas::ClipRef {
            mask: self.clip.as_deref(),
            id: self.clip_id,
        }
    }
}

/// The `q`/`Q` stack (Table 57). Depth-guarded: Annex C gives 28 as
/// the architectural q/Q nesting limit; pdfcer accepts more on read
/// (readers should exceed writer guidance) but bounds it as an
/// ARCHITECTURE.md §10 guard.
#[derive(Debug)]
pub struct GStateStack {
    stack: Vec<GraphicsState>,
    /// The live state.
    pub current: GraphicsState,
}

/// Maximum `q` nesting accepted (pdfcer policy; Annex C's writer
/// guidance is 28 — this is ~9× headroom before a hostile stream is
/// refused further nesting).
pub const MAX_Q_DEPTH: usize = 256;

impl GStateStack {
    /// Fresh stack over the initial state.
    #[must_use]
    pub fn new(initial: GraphicsState) -> Self {
        Self {
            stack: Vec::new(),
            current: initial,
        }
    }

    /// `q` — push a copy. Returns false (and does nothing) past
    /// [`MAX_Q_DEPTH`]; the interpreter surfaces that as a diagnostic.
    pub fn push(&mut self) -> bool {
        if self.stack.len() >= MAX_Q_DEPTH {
            return false;
        }
        self.stack.push(self.current.clone());
        true
    }

    /// `Q` — restore. An unbalanced `Q` (empty stack) is a no-op
    /// returning false — the RAG's real-world-tolerance note (spec
    /// says balanced; producers disagree); surfaced as a diagnostic.
    pub fn pop(&mut self) -> bool {
        match self.stack.pop() {
            Some(prev) => {
                self.current = prev;
                true
            }
            None => false,
        }
    }
}

/// Map a PDF `/BM` blend-mode name (§11.3.5, Tables 136 and 137) to the
/// rasterizer's equivalent, or `None` if pdfcer does not implement it.
///
/// # Why a table and not a guess
///
/// `tiny_skia` implements the W3C *Compositing and Blending Level 1*
/// model. That model and ISO 32000-1's clause 11 share an ancestry — the
/// W3C spec's blend functions were taken from PDF's — but "shared
/// ancestry" is not "identical", and a blend function that is subtly wrong
/// is invisible on screen and wrong in print. So this mapping is
/// **verified numerically**, mode by mode, against the standard's own
/// `B(cb, cs)` formulas in `tests/blend_modes.rs`; it is not asserted from
/// the names matching.
///
/// # The two that are deliberately absent
///
/// `Compatible` is Table 136's alias for `Normal` (it exists for PDF 1.3
/// compatibility and means exactly source-over), so it maps to the same
/// value rather than being refused.
///
/// Anything else — an unknown name, or a name from a later extension —
/// returns `None`, and the caller counts it and paints `Normal`. Refusing
/// to paint at all would be worse: the marks belong on the page, and only
/// the compositing rule is in doubt.
#[must_use]
pub fn blend_mode_from_name(name: &[u8]) -> Option<tiny_skia::BlendMode> {
    use tiny_skia::BlendMode as B;
    Some(match name {
        b"Normal" | b"Compatible" => B::SourceOver,
        b"Multiply" => B::Multiply,
        b"Screen" => B::Screen,
        b"Overlay" => B::Overlay,
        b"Darken" => B::Darken,
        b"Lighten" => B::Lighten,
        b"ColorDodge" => B::ColorDodge,
        b"ColorBurn" => B::ColorBurn,
        b"HardLight" => B::HardLight,
        b"SoftLight" => B::SoftLight,
        b"Difference" => B::Difference,
        b"Exclusion" => B::Exclusion,
        // ★ THE FOUR NON-SEPARABLE MODES (Table 137) ARE ABSENT FROM THIS
        // FUNCTION, and they are absent because they are IMPLEMENTED
        // ELSEWHERE — not because they are refused.
        //
        // `Hue`, `Saturation`, `Color` and `Luminosity` are computed by
        // pdfcer in `crate::blend_nonsep`, resolved by
        // `NonSeparableBlend::from_name`, and carried on
        // `GraphicsState::nonseparable` rather than here. This function
        // returns a `tiny_skia::BlendMode`, and the whole point is that
        // these four must never be expressible as one:
        //
        // `tiny_skia` 0.11.4 HAS `BlendMode::Hue`/`Saturation`/`Color`/
        // `Luminosity`, routing to them is a one-line move, and they are
        // MEASURABLY WRONG against both ISO 32000-1 and W3C Compositing-1 —
        // up to 107/255 error on 9.4–15.5% of random colour pairs, over
        // 60,000 measured pixels. Root cause, reproduced rather than
        // inferred: the crate's `clip_color` gates its low-gamut rescale on
        // `mx >= 0` where the standard and upstream Skia use `mn < 0`, so
        // the branch is dead and negative channels are hard-clamped instead
        // of rescaled at constant luminosity. Keeping them out of this
        // function's return type makes that mistake unrepresentable.
        //
        // ★★ CORRECTED 2026-08-19, and the previous wording is the reason
        // `R199` exists. It read:
        //
        //   "Returning `None` costs a correct rendering of four modes …
        //    implementing them properly means compositing Table 137 by hand
        //    against §11.3.6 NOTE 2, WHICH IS A PASS OF ITS OWN."
        //
        // That Pass is `85.4b`, and it took an afternoon — because
        // `Pass 85.5` had already built the per-paint destination read the
        // work needed. **A recorded blocker is a dated reading, not a
        // standing fact** (`R199`): a stale figure dies when someone
        // re-measures it and a stale contract dies when a caller hits it,
        // but a stale blocker dies never, because its function is to stop
        // the person who would have checked.
        _ => return None,
    })
}

#[cfg(test)]
mod mat64_tests {
    use super::Mat64;
    use tiny_skia::Transform;

    /// `Mat64::post_concat` must mean exactly what
    /// `Transform::post_concat` means.
    ///
    /// Asserted rather than assumed, because the convention is the one
    /// thing here that cannot be checked by reading: PDF's matrix is a
    /// row-vector form, `tiny_skia`'s argument order interleaves the
    /// shears (`from_row(sx, ky, kx, sy, tx, ty)`), and getting either
    /// backwards produces a transform that is still plausible — shears
    /// transposed, or the two operands applied in the wrong order — and
    /// only shows up as content in the wrong place on documents that use
    /// a non-axis-aligned `cm`.
    #[test]
    fn mat64_post_concat_matches_tiny_skia() {
        // Deliberately asymmetric: a matrix with equal shears, or a pure
        // scale, cannot distinguish a transpose from the truth.
        let cases = [
            (
                [1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0],
                [2.0f32, 0.0, 0.0, 3.0, 5.0, 7.0],
            ),
            (
                [2.0, 0.5, -0.25, 3.0, 11.0, -4.0],
                [0.5, -1.5, 0.75, 0.25, -3.0, 8.0],
            ),
            (
                [0.0, 1.0, -1.0, 0.0, 100.0, 0.0],
                [1.0, 0.0, 0.0, -1.0, 0.0, 792.0],
            ),
            // The shape this type exists for: a tiny scale with a large
            // translation, composed with a large scale.
            (
                [2.834_645_7e-9, 0.0, 0.0, 2.834_645_7e-9, 540.0, 558.85],
                [8.1e6, 0.0, 0.0, -8.1e6, -4.374e9, 4.527e9],
            ),
        ];
        for (m, n) in cases {
            let tm = Transform::from_row(m[0], m[1], m[2], m[3], m[4], m[5]);
            let tn = Transform::from_row(n[0], n[1], n[2], n[3], n[4], n[5]);
            let want = tm.post_concat(tn);
            let got = Mat64::from_f32(tm)
                .post_concat(Mat64::from_f32(tn))
                .to_f32();
            // The LINEAR part must agree to `f32` rounding: no
            // cancellation happens there, so any difference is a
            // convention error, not a precision one.
            for (a, b, what) in [
                (want.sx, got.sx, "sx"),
                (want.ky, got.ky, "ky"),
                (want.kx, got.kx, "kx"),
                (want.sy, got.sy, "sy"),
            ] {
                assert!(
                    (a - b).abs() <= a.abs().max(1.0) * 1e-5,
                    "{what}: tiny_skia {a} vs Mat64 {b} for {m:?} x {n:?}"
                );
            }
        }
    }

    /// And the translation must be BETTER, not merely equal — which is
    /// the entire point, and is why the test above checks only the linear
    /// part for agreement.
    ///
    /// ★ The first version of this test used round numbers — a form at
    /// `x = 540` and a device origin of exactly `540 * scale` — and the
    /// `f32` route produced **exactly 0**, i.e. the right answer. Two
    /// large numbers that are equal cancel perfectly in any precision;
    /// the failure needs them to be large and *nearly* equal. The
    /// self-check at the bottom is what caught it, and is kept for the
    /// next person who adjusts these constants.
    #[test]
    fn mat64_translation_survives_a_cancellation_f32_cannot() {
        // The real numbers from `tools/gen-scale-demo`'s molecule box.
        let scale = 8_104_752.0_f64;
        let form_x = 540.0_f64; // where the `cm` puts the form
        let region_llx = 539.999_891_9_f64; // where the viewport starts
        let origin = (region_llx * scale).floor(); // the base CTM's -tx

        let cm = Mat64::from_row(2.834_645_7e-9, 0.0, 0.0, 2.834_645_7e-9, form_x, 0.0);
        let base = Mat64::from_row(scale, 0.0, 0.0, scale, -origin, 0.0);
        let exact = cm.post_concat(base).tx;

        // Truth, computed independently of the type under test.
        let truth = form_x * scale - origin;
        assert!(
            (exact - truth).abs() < 0.01,
            "f64 composition should be exact to well under a pixel: got {exact}, truth {truth}"
        );

        // The same composition entirely in `f32`, which is what the
        // renderer did before this type existed.
        #[allow(clippy::cast_possible_truncation)]
        let f32_way = Transform::from_row(2.834_645_7e-9, 0.0, 0.0, 2.834_645_7e-9, 540.0, 0.0)
            .post_concat(Transform::from_row(
                scale as f32,
                0.0,
                0.0,
                scale as f32,
                -origin as f32,
                0.0,
            ))
            .tx;
        let f32_error = (f64::from(f32_way) - truth).abs();
        assert!(
            f32_error > 50.0,
            "this test is only meaningful if the f32 route is visibly wrong; its error was {f32_error} px, so either the fixture stopped exercising the cancellation or f32 got better. Round numbers cancel EXACTLY -- the operands have to be large and merely NEARLY equal."
        );
    }

    /// The gate that keeps ordinary rendering on the fast path.
    #[test]
    fn needs_precise_paths_only_fires_at_deep_zoom() {
        let at = |tx: f64| Mat64::from_row(1.0, 0.0, 0.0, 1.0, tx, 0.0).needs_precise_paths();
        assert!(!at(0.0), "whole page");
        assert!(!at(1_300.0), "a page-fit render's translation");
        assert!(
            !at(334_000.0),
            "the two cells at ~62 000 %: 0.04 px of error"
        );
        assert!(at(8.5e7), "one mitochondrion at ~16 M %: 10 px of error");
        assert!(at(4.4e9), "the molecule box at ~1.5 G %: 524 px of error");
        // The y component counts too — a page rotated 90 degrees puts the
        // magnitude there instead, and a predicate that only looked at x
        // would silently stop working on landscape scans.
        assert!(Mat64::from_row(1.0, 0.0, 0.0, 1.0, 0.0, 8.5e7).needs_precise_paths());
    }
}

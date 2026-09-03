//! The reusable parsed handle — `Pass 75.0`.
//!
//! A [`DisplayList`] is one page's drawing, *already interpreted*, in a form
//! a shell can hold across frames and replay against any viewport it likes.
//! Its whole reason to exist is one measurement.
//!
//! # 1. The measurement that determines everything below
//!
//! On the reference A3 CAD sheet (`ncored-benchmark-cad-drawing.pdf`,
//! 148,517 paints · 24,128 clip ops — `docs/render-region-measurements.md`
//! §4a):
//!
//! | fact | number |
//! |---|---:|
//! | a **1 × 1 point** region — 2 pixels | ~667 ms |
//! | the whole page at scale 1 — 1,002,822 pixels | ~941 ms |
//! | the same run with every `fill_path`/`stroke_path` ablated | ~591 ms |
//! | ⇒ painting is | **~11 %** of the floor |
//! | ⇒ everything outside `Interpreter::paint` | **~83 %** |
//!
//! A two-pixel render costing 667 ms is not a rasteriser problem. Rendering
//! a **region** does not make a page cheaper, because a region still walks
//! the entire content stream; a shell that pans by re-rendering a viewport
//! therefore pays ~0.7 s **per frame** on this document. That is the
//! regression this module prevents.
//!
//! ## 1.1 Why the list stores FINISHED paths
//!
//! Because ~83 % of the cost is *outside* the paint call — tokenising,
//! operator dispatch, graphics state, and `PathBuilder` pushes. A path is
//! built incrementally by `m`/`l`/`c`/`re`, so its construction is spread
//! across thousands of operators and is **not** inside `paint()`; only
//! `builder.finish()` is.
//!
//! So the recording point is *where a finished path first exists as a
//! value*. A cache keyed any earlier would have to cache the operator
//! stream and rebuild the path — which is most of the cost it exists to
//! avoid.
//!
//! # 2. What is recorded, and in whose coordinate space
//!
//! Three op kinds ([`Op`]) plus a nesting one, and a separate table of clip
//! **definitions** ([`ClipDef`]).
//!
//! Everything is recorded in the **device space of the whole page at the
//! list's own scale** — `page_device_geometry(page, scale)`. Replaying a
//! region is then a `post_translate` by the region's device origin, which is
//! exactly the relationship `render_page_region` already has to
//! `render_page` (§4).
//!
//! ## 2.1 ★ The list is keyed on SCALE as well as `(page, epoch)`, and that
//! is a deliberate narrowing of what the consumer asked for
//!
//! The requesting shell asked for `(page, epoch)`. This implementation adds
//! **scale**, and refuses by name when it does not match
//! ([`RenderError::DisplayListStale`]). The reason is not conservatism:
//!
//! - Half the interpreter's decisions are **device-dependent**, made from
//!   the CTM at the moment the operator runs: whether a stroke is a
//!   hairline, whether an image is minified and therefore which filter it
//!   gets, whether an image edge is anti-aliased, and the size of every
//!   clip mask. Recording at one scale and replaying at another would have
//!   to *re-derive* all of those, and each re-derivation is a second
//!   implementation of a rule that already exists — the trap this project
//!   has now written down three times.
//! - Even setting those aside, replaying at a different scale composes the
//!   transform in a different **order** (`M ∘ (B ∘ S)` versus
//!   `(M ∘ B) ∘ S`), and floating-point composition is not associative. The
//!   difference is sub-ULP in the coefficients and invisible in isolation,
//!   but the acceptance criterion for this Pass is **byte-identity**, not
//!   near-identity, and a criterion that is "usually" met is not met.
//!
//! What the narrowing costs and what it does not: **panning at a fixed
//! zoom — the per-frame case, and the one the consumer described — is
//! fully served.** A zoom *step* costs one rebuild. A shell wanting
//! continuous zoom scales its existing texture during the gesture and
//! rebuilds when it settles, which is what every viewer does and is the
//! shell's call, not this crate's.
//!
//! ## 2.2 Clips are recorded as PATHS, never as masks
//!
//! `GraphicsState::clip` is an `Option<Arc<tiny_skia::Mask>>`, and a `Mask`
//! is **device-sized**. A recorded mask would be valid only for the pixmap
//! geometry that built it, so panning would invalidate it — the precise
//! thing a display list exists to survive.
//!
//! So [`ClipDef`] stores the path, rule, CTM and parent, and masks are
//! rebuilt at replay against the region's own geometry. That is affordable
//! because the clip cache already serves **99.83 %** of applications on the
//! reference sheet: a replay pays ~41 mask builds, not 24,128.
//!
//! Recording therefore does **not** build masks at all, which is why a
//! recording pass is *cheaper* than a render rather than an extra cost on
//! top of one.
//!
//! # 3. ★ Where the recorder REFUSES, and why refusing is the feature
//!
//! Some operators cannot be recorded faithfully, and every one of them has
//! the same shape: **it reads the destination back**.
//!
//! | operator | why it cannot be recorded |
//! |---|---|
//! | `sh` and shading patterns | evaluated per destination pixel |
//! | overprint composites (§11.7.4.3) | composites against the destination's own colorants |
//! | soft masks (`/SMask`) | built from a rendered buffer |
//!
//! A recording that hit one of these is **poisoned by name**
//! ([`PoisonReason`]) and [`record_page`] returns
//! [`RenderError::PageNotRecordable`], so the caller falls back to
//! [`crate::render_page_region`]. It does **not** return a list that
//! renders the page *nearly* right.
//!
//! That is the whole judgement, and it is worth stating plainly: **a
//! display list that is subtly wrong is strictly worse than no display
//! list**, because the wrongness is invisible at the call site and shows up
//! as a document that looks different when you pan. Refusing is loud,
//! cheap, and correct.
//!
//! The reference CAD sheet needs none of them — measured on the benchmark
//! page: `images=0 shadings=0 patterns_unpainted=0 blend_modes_applied=0
//! soft_masks_applied=0 groups_composited=0`. It is pure paths and text.
//!
//! # 4. What a replay reproduces, exactly
//!
//! [`DisplayList::replay_region`] performs, in order, precisely what
//! `render_impl` performs for a region:
//!
//! 1. the same region → device-rect arithmetic (shared code, not a copy —
//!    [`crate::region_device_geometry`]);
//! 2. a **transparent** pixmap (§11.4.7's isolated page group);
//! 3. the recorded ops, each culled against the region's device rect;
//! 4. §11.4.7's composite of the page group over nominally-white paper.
//!
//! Annotations are recorded into the list along with page content, in their
//! natural z-order, so a replay needs no document, no page and no options —
//! which is what makes a stale replay *unrepresentable* rather than merely
//! detectable. There is nothing left for a caller to pass that could
//! disagree with what was recorded.

use std::collections::HashMap;
use std::sync::Arc;

use pdfcer_core::object::ObjId;
use pdfcer_core::page_tree::{Page, Rect as PageRect};
use pdfcer_core::view::DocumentView;
use tiny_skia::{FillRule, Mask, Path, Pixmap, Stroke, Transform};

use crate::canvas::{BrushSpec, Canvas, LayerPaint};
use crate::font::RenderOptions;
use crate::interpret::Diagnostics;
use crate::{RenderError, RenderedPage};

/// The most memory one [`DisplayList`] may occupy before recording is
/// refused — `ARCHITECTURE.md` §10's output-size ceiling, applied to the one
/// output in this crate a document can make arbitrarily large.
///
/// # Why a display list needs a ceiling when a render does not
///
/// Because a render is **transient**. The interpreter builds a path, paints
/// it and drops it, so a page with ten million tiny fills costs time and a
/// fixed-size pixmap. A recorder *retains* every one of them, which turns the
/// same 100 KB file into an unbounded allocation — the shape §10 exists to
/// refuse, and one that arrives as a hang rather than as an error.
///
/// # Where the number comes from, and a correction to how it was first
/// justified
///
/// The ceiling is **256 MiB**.
///
/// It was originally justified as *"~8.5× the reference A3 CAD sheet's
/// ~30 MiB"*, and **that ratio was wrong — not arithmetically, but because
/// the CAD sheet is not the largest thing measured.** Running the guard
/// across 3,245 files (`examples/guard_probe.rs`; the whole veraPDF corpus,
/// every synthetic fixture, and this project's working documents) found a
/// larger one:
///
/// | | ops | held |
/// |---|---:|---:|
/// | reference A3 CAD sheet | 127,267 | 29.5 MiB |
/// | ★ `veraPDF … 6.1.12 … t03-fail-c.pdf` | — | **41.9 MiB** |
///
/// So the real headroom against the largest **observed** input is **6×**,
/// not 8.5×. The ceiling did not move; the claim about it did.
///
/// Worth noting *which* file it was, because it is not an accident: a
/// §6.1.12 *implementation-limits* conformance file is built to stress
/// exactly this kind of ceiling. The suite whose job is to find a
/// resource guard found the biggest one, which is the standing rule's whole
/// argument for running new guards against it.
///
/// It is deliberately generous rather than tight, and the asymmetry is the
/// argument: a false refusal costs a fallback to
/// [`crate::render_page_region`], which is correct and merely slower, while a
/// ceiling set too high costs the process. Cheap failure on one side, fatal
/// on the other.
///
/// A caller that hits it gets [`RenderError::PageNotRecordable`] with
/// [`PoisonReason::TooLarge`], and should render that page directly.
///
/// # Two-sided discharge, and the half that could not be demonstrated
///
/// `ROADMAP.md`'s standing rule requires a new resource guard to be shown
/// both to **fire** and to stay **silent**.
///
/// - **Silent: discharged, and well past what the rule asks.** 3,245 files
///   walked, **0** refused as `TooLarge`. Beside it, a figure a shell wants:
///   **77 pages (2.4 %) were refused for a *capability* reason** — a
///   shading, an overprint composite, a soft mask — so roughly one page in
///   forty falls back to [`crate::render_page_region`], and the other
///   thirty-nine get a display list.
/// - **Firing: demonstrated only synthetically.** The unit tests in this
///   module drive the ceiling directly with a small limit. **No real file
///   was found that reaches 256 MiB**, and the reason is the table above:
///   the largest observed list is 6× under it. Recorded plainly rather than
///   dressed up — a clean sweep means the ceiling does not trip on
///   legitimate documents, and says nothing about whether a hostile one
///   could be constructed to.
pub const MAX_DISPLAY_LIST_BYTES: usize = 256 * 1024 * 1024;

/// Index of a [`ClipDef`] in a [`DisplayList`]'s clip table.
///
/// A newtype rather than a bare `u32` so a clip index cannot be confused
/// with an op index or a page number, and `Copy` because it is threaded
/// through the graphics state, which `q`/`Q` copy wholesale.
///
/// **Invariant, relied on by the replay builder:** a definition's parent
/// always has a strictly smaller index, because a clip can only be created
/// by intersecting the one already in force, which was pushed earlier. That
/// makes the table a topologically sorted DAG and makes cycle-checking
/// unnecessary rather than merely omitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClipId(u32);

impl ClipId {
    /// This id as a table index.
    fn index(self) -> usize {
        self.0 as usize
    }
}

/// One clipping path, as the definition that *produces* a mask rather than
/// as the mask itself (module docs §2.2).
#[derive(Debug, Clone)]
pub(crate) struct ClipDef {
    /// The clipping path, in the space `ctm` maps to page-device space.
    ///
    /// **`None` is §8.5.4's degenerate case** — a pending clip over an
    /// *empty path*, which admits nothing. Modelled as the absence of a
    /// path rather than as a flag beside an ignored one, because the two
    /// behave differently at replay (an empty clip is an all-zero mask and
    /// does **not** multiply by its parent), and a flag that says "ignore
    /// the field next to me" is a representable invalid state waiting for
    /// someone to read the field anyway.
    pub path: Option<Arc<Path>>,
    /// `W` (nonzero) or `W*` (even-odd).
    pub rule: FillRule,
    /// Path space → page-device space at the list's scale.
    pub ctm: Transform,
    /// The clip this one was intersected with, if any.
    pub parent: Option<ClipId>,
}

/// One recorded drawing operation.
///
/// Deliberately **not** an operator: there is no `Op::SetColour`, no
/// `Op::Concat`, no `Op::Save`. The graphics state was already resolved
/// during interpretation, and re-recording it would mean re-implementing
/// the state machine at replay — a second interpreter, which is exactly
/// what this design refuses (module docs §3).
#[derive(Debug, Clone)]
pub(crate) enum Op {
    /// Fill a finished path.
    Fill {
        /// The path, in the space `ctm` maps to page-device space.
        path: Arc<Path>,
        /// Colour or image, already decomposed into owned parts.
        brush: BrushSpec,
        /// Nonzero or even-odd.
        rule: FillRule,
        /// Path space → page-device space at the list's scale.
        ctm: Transform,
        /// The clip in force, if any.
        clip: Option<ClipId>,
        /// Device-space bounds at the list's scale, for the replay cull.
        /// `None` means "bounds not computable" — a degenerate or
        /// non-finite transform — and such an op is **never culled**.
        bounds: Option<DeviceBounds>,
    },
    /// Stroke a finished path.
    Stroke {
        /// The path, in the space `ctm` maps to page-device space.
        path: Arc<Path>,
        /// Colour, already decomposed into owned parts.
        brush: BrushSpec,
        /// Width, cap, join, miter limit and dash — owned, because a
        /// `Stroke`'s dash array is a `Vec`.
        ///
        /// `Arc` because consecutive strokes overwhelmingly share one: a
        /// CAD sheet sets a line width once and strokes ten thousand
        /// segments with it, and cloning the dash `Vec` per op would cost
        /// more than the path.
        stroke: Arc<Stroke>,
        /// Path space → page-device space at the list's scale.
        ctm: Transform,
        /// The clip in force, if any.
        clip: Option<ClipId>,
        /// Device-space bounds at the list's scale, **already widened for
        /// the stroke's own extent**. See [`stroke_bounds`].
        bounds: Option<DeviceBounds>,
    },
    /// A sub-drawing composited as **one object** — §11.4.5's transparency
    /// group, and an annotation's `/CA` constant alpha, which are the same
    /// operation.
    Layer {
        /// How the finished sub-drawing composites into its parent.
        paint: LayerPaint,
        /// The sub-drawing.
        ops: Vec<Op>,
    },
}

/// An axis-aligned device-space rectangle, as four `f32`s.
///
/// `tiny_skia::Rect` would do, but it refuses to construct an empty or
/// non-finite rectangle, and the cull wants to *store* whatever bounds it
/// got and decide later. A plain quadruple has no opinions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DeviceBounds {
    /// Left edge, device pixels.
    pub left: f32,
    /// Top edge, device pixels.
    pub top: f32,
    /// Right edge, device pixels.
    pub right: f32,
    /// Bottom edge, device pixels.
    pub bottom: f32,
}

impl DeviceBounds {
    /// Whether these bounds can possibly mark anything inside `region`.
    ///
    /// Conservative in exactly one direction: it must never answer `false`
    /// for something that would have painted. A one-pixel slack is added
    /// on every side because an anti-aliased edge writes into the pixel
    /// *outside* the geometric bound, and because a hairline stroke is
    /// widened by the rasteriser rather than by the path.
    fn intersects(self, region: Self) -> bool {
        self.left - 1.0 < region.right
            && self.right + 1.0 > region.left
            && self.top - 1.0 < region.bottom
            && self.bottom + 1.0 > region.top
    }
}

/// Why a page could not be recorded (module docs §3).
///
/// Each variant names an operator class that **reads the destination back**
/// and therefore has no recordable formulation. The reason travels with the
/// error so a caller can log *what* it fell back for, rather than only that
/// it fell back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PoisonReason {
    /// A `sh` operator, or a shading used as a fill pattern (§8.7.4).
    /// Evaluated per destination pixel.
    Shading,
    /// An overprint composite (§11.7.4.3), which reads the destination's
    /// own colorants.
    Overprint,
    /// A soft mask (`/SMask` in an `/ExtGState`, §11.6.5), whose coverage
    /// comes from a rendered buffer.
    SoftMask,
    /// A tiling pattern (§8.7.3).
    TilingPattern,
    /// One of §11.3.5.3's four non-separable blend modes, which pdfcer
    /// composites per pixel against the destination (`crate::blend_nonsep`).
    NonSeparableBlend,
    /// A **non-isolated transparency group whose contents blend against
    /// their backdrop** (§11.4.4 NOTE 2). Rendering it correctly means
    /// running the group's content stream a second time over a copy of the
    /// backdrop and then applying §11.4.4's backdrop removal — both of
    /// which read the destination, so a recording cannot reproduce it.
    ///
    /// A recorded page containing one replays as the *isolated*
    /// approximation, which is why the recording is refused by name rather
    /// than quietly kept: the difference is a plausible-looking picture,
    /// not a visible failure.
    NonIsolatedGroup,
    /// The recording **scale** puts the page's own transform past what
    /// `f32` can hold, so a recorded list could not agree with a direct
    /// render.
    ///
    /// # Why this exists, and why the threshold is not a new one
    ///
    /// A display list stores each op's CTM as a `tiny_skia::Transform`,
    /// which is `f32`, and `replay_region` shifts it by the region's
    /// device origin. Both are fine while the numbers are small. At a
    /// recording scale of 5 000 a letter page's transform already carries
    /// a translation of ~4e6, where `f32`'s spacing costs half a device
    /// pixel; at 500 000 it costs 47.
    ///
    /// `Pass 74.7` fixed exactly that arithmetic **on the direct path**,
    /// which carries the CTM in `f64`
    /// ([`crate::gstate::Mat64`]). It did not fix it here, and a
    /// recording that is half a pixel out from the render it is supposed
    /// to substitute for is the module's own stated nightmare: *"a display
    /// list that is subtly wrong is strictly worse than no display list"*.
    ///
    /// ★ **The threshold is [`Mat64::needs_precise_paths`] — the SAME one
    /// the direct path uses to decide whether it needs its precise
    /// route.** That is what makes this a boundary rather than a
    /// compromise: below it both paths do identical `f32` arithmetic and
    /// agree exactly; above it the direct path switches to `f64` and this
    /// one refuses. There is no scale at which the two disagree, which is
    /// the property `R211` asks of any second rendering path.
    ///
    /// For a US-Letter page this fires above a recording scale of roughly
    /// **530** — far beyond any zoom a list is worth caching for, and the
    /// fallback is a direct render that is correct at any scale.
    ScaleBeyondF32,
    /// The recording exceeded [`MAX_DISPLAY_LIST_BYTES`].
    ///
    /// Unlike the others this is not a *capability* limit — the page is
    /// perfectly recordable in principle, it is simply too big to be worth
    /// holding. The remedy is the same either way: render it directly.
    TooLarge,
    /// The page's **blending colour space is subtractive** (§11.4.7,
    /// §11.7.2), so a direct render composites it in a colorant buffer and
    /// a replay cannot.
    ///
    /// # Why this is a refusal rather than a difference to live with
    ///
    /// A display list replays into a `tiny_skia::Pixmap`. That is the whole
    /// point of the type — a recorded page is re-rasterised at a viewport
    /// chosen later — and a `Pixmap` cannot hold ink. So from `Pass 97.1e`
    /// onward a subtractive page rendered directly and the same page
    /// replayed from a recording produce **different pixels**, and the
    /// replayed one is the pre-Pass approximation.
    ///
    /// That is precisely the failure this module refuses everywhere else:
    /// not a visible breakage but *a plausible wrong picture*. The comment
    /// at the recording site has said so since `Pass 97.1d`, when the space
    /// was first threaded through; it became actionable the moment a paint
    /// render started doing something different with it.
    ColorantBuffer,
}

impl PoisonReason {
    /// A short, stable name for logs and error messages.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ColorantBuffer => "subtractive blending colour space",
            Self::Shading => "shading",
            Self::Overprint => "overprint composite",
            Self::SoftMask => "soft mask",
            Self::TilingPattern => "tiling pattern",
            Self::NonSeparableBlend => "non-separable blend mode",
            Self::NonIsolatedGroup => "non-isolated transparency group",
            Self::TooLarge => "recording exceeded MAX_DISPLAY_LIST_BYTES",
            Self::ScaleBeyondF32 => "recording scale past f32 transform precision",
        }
    }
}

/// What a [`DisplayList`] was recorded for.
///
/// # Why the caller must restate this to replay
///
/// Because `pdfcer-render` cannot tell whether a document has been edited —
/// `epoch` is the **shell's** counter, and this crate has no way to observe
/// it changing. So the only way a stale handle can be caught is for the
/// caller to say what it believes it is holding, and for the list to
/// disagree out loud ([`RenderError::DisplayListStale`]).
///
/// That is the whole of criterion 2: *"a stale handle is impossible to use
/// silently"*. A display list that renders a document's previous state
/// while reporting success is strictly worse than no cache at all.
///
/// # Why `Eq` and `Hash` are hand-written
///
/// `scale` is an `f32`, so the derived `PartialEq` would make a key
/// containing NaN unequal to itself — a cache entry that could never be
/// found again. Comparing bit patterns instead gives a total, reflexive
/// equality, which is what a *key* needs, and is why the type also carries
/// `Hash` (so a shell can use it in a `HashMap` directly).
#[derive(Debug, Clone, Copy)]
pub struct DisplayListKey {
    /// The page object this list was recorded from.
    pub page: ObjId,
    /// The caller's document-edit counter at the moment of recording.
    ///
    /// `pdfcer-render` never interprets this — it only compares it. A shell
    /// that bumps a counter on every mutation gets correct invalidation;
    /// one that does not, does not, and this crate cannot tell the
    /// difference. Stated rather than assumed.
    pub epoch: u64,
    /// Device pixels per PDF user-space unit (module docs §2.1 — this is
    /// part of the key, and deliberately so).
    pub scale: f32,
}

impl PartialEq for DisplayListKey {
    fn eq(&self, other: &Self) -> bool {
        self.page == other.page
            && self.epoch == other.epoch
            && self.scale.to_bits() == other.scale.to_bits()
    }
}

impl Eq for DisplayListKey {}

impl std::hash::Hash for DisplayListKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.page.hash(state);
        self.epoch.hash(state);
        self.scale.to_bits().hash(state);
    }
}

/// One page's drawing, interpreted once and replayable against any region
/// at the recorded scale.
///
/// Build with [`record_page`]; use with [`DisplayList::replay_region`].
/// Hold one per `(page, epoch, scale)` the shell is showing — and read
/// [`DisplayList::memory_bytes`] before deciding how many that is.
#[derive(Debug)]
pub struct DisplayList {
    key: DisplayListKey,
    /// Page-device geometry at the list's scale: the base CTM every
    /// recorded op's own CTM already includes.
    ///
    /// Kept for its documentary value and for `page_ctm()`'s consumers even
    /// though `replay_region` no longer derives a region's geometry from it
    /// — see [`Self::page_box`] for why that derivation moved.
    #[allow(dead_code)]
    page_ctm: Transform,
    /// Full-page device size at the list's scale. Not a raster size — no
    /// pixmap of this size is ever allocated — but the size every recorded
    /// clip bbox was clamped against.
    page_size: (u32, u32),
    /// The page's `CropBox` and `/Rotate`, kept so a replay can recompute
    /// a region's device geometry from the SAME `f64` arithmetic a fresh
    /// render uses.
    ///
    /// ★ Not redundant with [`Self::page_ctm`], and the difference is the
    /// point: that transform is `f32`, so recovering the page box from it
    /// at a scale of two million recovers it to the nearest ~128 device
    /// pixels. A recorded list outlives its `Page`, so the box has to be
    /// carried rather than re-read.
    page_box: pdfcer_core::page_tree::Rect,
    /// See [`Self::page_box`].
    page_rotate: u16,
    ops: Vec<Op>,
    clips: Vec<ClipDef>,
    /// The recorder's running total — see `RecorderState::approx_bytes`.
    bytes: usize,
    diagnostics: Diagnostics,
}

impl DisplayList {
    /// What this list was recorded for.
    #[must_use]
    pub const fn key(&self) -> DisplayListKey {
        self.key
    }

    /// The full-page device size at the list's scale, in pixels.
    ///
    /// **Not** a raster size — recording allocates no pixmap, so this may
    /// legitimately exceed [`crate::MAX_PIXMAP_EDGE`]; a deep zoom is
    /// exactly the case where a whole-page raster is impossible and a
    /// region is not. It is what every recorded clip bounding box was
    /// clamped against, and what a shell needs to know which regions are
    /// on the page at all.
    #[must_use]
    pub const fn page_device_size(&self) -> (u32, u32) {
        self.page_size
    }

    /// The interpretation diagnostics from the recording pass — identical
    /// to what a direct render of the same page reports, because it *is*
    /// the same interpreter run.
    #[must_use]
    pub const fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// How many drawing operations the list holds, layers counted as one
    /// plus their contents.
    #[must_use]
    pub fn op_count(&self) -> usize {
        fn walk(ops: &[Op]) -> usize {
            ops.iter()
                .map(|op| match op {
                    Op::Fill { .. } | Op::Stroke { .. } => 1,
                    Op::Layer { ops, .. } => 1 + walk(ops),
                })
                .sum()
        }
        walk(&self.ops)
    }

    /// How many DISTINCT clipping paths the list holds.
    ///
    /// Worth exposing beside [`Self::op_count`] because the two behave
    /// completely differently at replay: ops are culled per region, clip
    /// masks are **built** per region, and a mask is a region-sized buffer.
    /// A list whose clip count is close to its op count is one whose replay
    /// will be slow for a reason no amount of culling fixes — see
    /// `RecorderState::push_clip`, where that exact failure was measured.
    #[must_use]
    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }

    /// Approximate heap footprint in bytes.
    ///
    /// # Why this is `pub` rather than a debug aid
    ///
    /// Because acceptance criterion 4 is *"a held display list for a
    /// 148,517-paint sheet has a size; it is measured and documented, not
    /// assumed small"* — and a shell holding one per open page needs that
    /// number at runtime, not from a document.
    ///
    /// # What it counts, and what it deliberately does not
    ///
    /// Counted: the op vectors' own storage, every path's points and
    /// verbs, and the clip table's paths. On a page of paths and text —
    /// which is what the expensive documents are — that is essentially all
    /// of it.
    ///
    /// **Not** counted, and each for the same reason: image texels
    /// (`Arc<Pixmap>`) and stroke parameter blocks (`Arc<Stroke>`) are
    /// **shared**, both within a list and potentially with whatever else
    /// holds them. Attributing a shared buffer wholly to this list would
    /// overstate the cost of holding a second list for the same page, which
    /// is precisely the decision this number exists to inform. A caller
    /// needing the true resident set should measure the process; this is
    /// the list's own contribution.
    #[must_use]
    pub const fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.bytes
    }

    /// Rasterise `region` (in PDF user space) from this list.
    ///
    /// `expected` is the key the caller believes it is holding; a mismatch
    /// is refused rather than served — see [`DisplayListKey`].
    ///
    /// # Errors
    ///
    /// - [`RenderError::DisplayListStale`] if `expected` is not this list's
    ///   key.
    /// - [`RenderError::BadRasterSize`] if the region is empty or its
    ///   raster exceeds [`crate::MAX_PIXMAP_EDGE`] — the same guard, with
    ///   the same numbers, that [`crate::render_page_region`] applies.
    pub fn replay_region(
        &self,
        expected: DisplayListKey,
        region: PageRect,
    ) -> Result<RenderedPage, RenderError> {
        if expected != self.key {
            return Err(RenderError::DisplayListStale {
                expected_epoch: expected.epoch,
                recorded_epoch: self.key.epoch,
                expected_scale: expected.scale,
                recorded_scale: self.key.scale,
            });
        }
        // The SAME arithmetic a fresh region render uses, called rather
        // than copied: a second implementation of the corner mapping is a
        // second place for the `/Rotate` axis swap to be got wrong, and
        // "byte-identical to a fresh region render" would then be a claim
        // about two different rectangles.
        // ★ `region_base_geometry_of`, in `f64`, which is what a fresh
        // region render now calls. The comment above is a claim about the
        // two paths agreeing, and it stopped being true the moment the
        // direct path moved to `f64` -- so this call is what keeps it a
        // fact rather than a wish. See that function for the measured
        // table: at a scale of 2.15 M a requested 800x600 viewport came
        // back as 800x512 through the `f32` route.
        let g =
            crate::region_base_geometry_of(self.page_box, self.page_rotate, self.key.scale, region)
                .ok_or(RenderError::BadRasterSize {
                    width: 0,
                    height: 0,
                })?;
        let (width, height, x0, y0) = (g.width, g.height, g.x0, g.y0);
        if width == 0
            || height == 0
            || width > crate::MAX_PIXMAP_EDGE
            || height > crate::MAX_PIXMAP_EDGE
        {
            return Err(RenderError::BadRasterSize { width, height });
        }
        let Some(mut pixmap) = Pixmap::new(width, height) else {
            return Err(RenderError::BadRasterSize { width, height });
        };

        // The region's own device rect, in the list's page-device space —
        // which is the space every recorded `bounds` is already in, so the
        // cull is four comparisons and no arithmetic.
        #[allow(clippy::cast_precision_loss)]
        let region_bounds = DeviceBounds {
            left: x0,
            top: y0,
            right: x0 + width as f32,
            bottom: y0 + height as f32,
        };

        let mut masks = MaskBuilder::new(&self.clips, width, height, x0, y0);
        replay_ops(&self.ops, &mut pixmap, &mut masks, region_bounds);

        // §11.4.7's second formula, exactly as `render_impl` performs it.
        crate::flatten_page_group_over_white(&mut pixmap);

        Ok(RenderedPage {
            pixmap,
            diagnostics: self.diagnostics.clone(),
        })
    }
}

/// Interpret one page **once** and keep the result (module docs).
///
/// The page is walked exactly as [`crate::render_page_region`] walks it —
/// same interpreter, same annotation survey, same order — but drawing into
/// a recorder instead of a pixmap. Nothing is rasterised, so this costs
/// *less* than a render rather than a render plus a recording: no clip
/// masks are built (module docs §2.2) and no spans are filled.
///
/// `epoch` is the caller's document-edit counter; see [`DisplayListKey`].
///
/// # Errors
///
/// - [`RenderError::PageNotRecordable`] if the page uses an operator with
///   no recordable formulation (module docs §3). **Fall back to
///   [`crate::render_page_region`]** — the page renders correctly, it just
///   cannot be cached.
/// - [`RenderError::Cancelled`], [`RenderError::Content`] and
///   [`RenderError::BadRasterSize`] as for a direct render.
pub fn record_page(
    doc: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    epoch: u64,
    options: &RenderOptions,
) -> Result<DisplayList, RenderError> {
    let (page_w, page_h, page_ctm) = crate::page_device_geometry(page, scale);
    // A recording allocates no raster, so the MAX_PIXMAP_EDGE ceiling does
    // NOT apply here — that is the point of recording at a deep zoom. Zero
    // still has to be refused: a page with no extent has no geometry to
    // record clip bounds against.
    if page_w == 0 || page_h == 0 {
        return Err(RenderError::BadRasterSize {
            width: page_w,
            height: page_h,
        });
    }
    // ★ AND THE PRECISION CEILING, which is the other half of "recording at
    // a deep zoom" and was missing until `Pass 74.7` gave the direct path a
    // precision this one does not have.
    //
    // Everything in a recorded list is `f32`: each op's CTM, and the
    // `post_translate` `replay_region` applies to move it into the region's
    // space. Past the point where that arithmetic costs a twentieth of a
    // pixel, a replay and a direct render of the same page stop agreeing —
    // and this module's whole posture is that a list which is subtly wrong
    // is worse than no list.
    //
    // The test is `Mat64::needs_precise_paths`, the SAME predicate the
    // interpreter uses to decide whether it needs its `f64` path. Below it,
    // both paths do identical `f32` arithmetic and agree exactly; above it,
    // one switches to `f64` and the other refuses. No scale exists at which
    // they quietly disagree, which is what `R211` asks of a second
    // rendering path.
    if crate::gstate::Mat64::from_f32(page_ctm).needs_precise_paths() {
        return Err(RenderError::PageNotRecordable {
            reason: PoisonReason::ScaleBeyondF32,
        });
    }

    let mut recorder = Recorder::new(page_w, page_h);
    let scope = options.effective_annotation_scope();
    // Resolved ONCE, and used for two things that must not disagree: what
    // the interpreter is told the blending space is, and whether this page
    // is recordable at all.
    // ★ THE SAME PREDICATE THE DIRECT RENDER USES, including the same
    // `page_blend_space_source` setting -- decision 084's rule that two
    // paths which must agree share the predicate deciding when they can,
    // rather than each computing its own. A recording that judged the
    // blending space differently from the render would refuse the wrong
    // pages, and both directions are bad: refusing too few hands back a
    // replay that composites in the wrong space.
    let page_space = if scope.paints_page_content() {
        crate::interpret::page_blend_space(
            doc,
            page.id,
            &page.resources,
            &mut Default::default(),
            options.policy().page_blend_space_source,
        )
        .0
    } else {
        crate::compositor::BlendSpace::Additive
    };
    let diagnostics = {
        let mut canvas = Canvas::record(&mut recorder);
        // ★ REFUSED BEFORE A SINGLE OPERATOR IS WALKED. A direct render of
        // a subtractive page composites it in a colorant buffer; a replay
        // cannot, because a replay's destination is a `Pixmap`. Recording
        // it anyway would hand the caller a cache entry that renders a
        // DIFFERENT and worse picture than the uncached path -- which is
        // the exact failure mode every other `PoisonReason` in this module
        // exists to prevent, and the one hardest to notice, because the
        // replayed page looks entirely reasonable.
        if page_space.is_subtractive() {
            canvas.refuse(PoisonReason::ColorantBuffer);
        }
        let mut diagnostics = if scope.paints_page_content() {
            let content = crate::ContentStream::from_page(doc, page)?;
            let initial = crate::gstate::GraphicsState::default_with_ctm(page_ctm);
            let mut diagnostics = crate::interpret::run_on(
                doc,
                &content,
                &page.resources,
                &options.fonts,
                initial,
                &mut canvas,
                options.cancel.as_ref(),
                options.policy(),
                // §11.4.7's page group carries the blending colour
                // space, and a RECORDING must resolve it exactly as a
                // paint does — a display list that recorded the wrong
                // space would replay a plausible wrong picture, which
                // is the one outcome this module refuses everywhere
                // else by poisoning.
                page_space,
            );
            diagnostics.contents_streams_unresolved = page.contents_unresolved;
            diagnostics
        } else {
            Diagnostics {
                page_content_suppressed: true,
                ..Diagnostics::default()
            }
        };
        crate::annot::survey_page_annotations(
            doc,
            page,
            page_ctm,
            &options.fonts,
            scope,
            &mut diagnostics,
            &mut canvas,
            options.cancel.as_ref(),
            options.policy(),
        );
        diagnostics
    };

    if options
        .cancel
        .as_ref()
        .is_some_and(crate::cancel::RenderCancel::is_cancelled)
    {
        return Err(RenderError::Cancelled);
    }
    // Checked AFTER the walk, deliberately: the walk is what discovers the
    // refusal, and a page whose LAST operator is a shading is exactly as
    // unrecordable as one whose first is.
    if let Some(reason) = recorder.poison_reason() {
        return Err(RenderError::PageNotRecordable { reason });
    }

    let bytes = recorder.approx_bytes;
    let (ops, clips) = recorder.finish();
    Ok(DisplayList {
        // Carried, not derived: see the field docs. A recorded list
        // outlives the `Page` it came from, and recovering the box from
        // the `f32` `page_ctm` at deep zoom recovers it to the nearest
        // ~128 device pixels.
        page_box: page.crop_box,
        page_rotate: page.rotate,
        key: DisplayListKey {
            page: page.id,
            epoch,
            scale,
        },
        page_ctm,
        page_size: (page_w, page_h),
        ops,
        clips,
        bytes,
        diagnostics,
    })
}

/// Draw `ops` into `pixmap`, culling anything that cannot mark `region`.
fn replay_ops(ops: &[Op], pixmap: &mut Pixmap, masks: &mut MaskBuilder<'_>, region: DeviceBounds) {
    for op in ops {
        match op {
            Op::Fill {
                path,
                brush,
                rule,
                ctm,
                clip,
                bounds,
            } => {
                if bounds.is_some_and(|b| !b.intersects(region)) {
                    continue;
                }
                let mask = masks.mask_for(*clip);
                pixmap.fill_path(
                    path,
                    &brush.to_paint(),
                    *rule,
                    masks.to_region(*ctm),
                    mask.as_deref(),
                );
            }
            Op::Stroke {
                path,
                brush,
                stroke,
                ctm,
                clip,
                bounds,
            } => {
                if bounds.is_some_and(|b| !b.intersects(region)) {
                    continue;
                }
                let mask = masks.mask_for(*clip);
                pixmap.stroke_path(
                    path,
                    &brush.to_paint(),
                    stroke,
                    masks.to_region(*ctm),
                    mask.as_deref(),
                );
            }
            Op::Layer { paint, ops } => {
                // Same size as the destination and TRANSPARENT to start —
                // §11.4.7's isolated backdrop, and the same buffer
                // `Canvas::layer` allocates in paint mode.
                let Some(mut buf) = Pixmap::new(pixmap.width(), pixmap.height()) else {
                    continue;
                };
                replay_ops(ops, &mut buf, masks, region);
                pixmap.draw_pixmap(
                    0,
                    0,
                    buf.as_ref(),
                    &tiny_skia::PixmapPaint {
                        opacity: paint.opacity.clamp(0.0, 1.0),
                        blend_mode: paint.blend,
                        quality: tiny_skia::FilterQuality::Nearest,
                    },
                    Transform::identity(),
                    // No mask: the contents were already clipped while
                    // being drawn.
                    None,
                );
            }
        }
    }
}

/// Rebuilds recorded [`ClipDef`]s into region-sized masks, once each.
///
/// # Why this is memoised and why that is the affordable half of the design
///
/// A page applies far more clips than it defines — 24,128 applications over
/// ~40 distinct masks on the reference sheet, one path alone accounting for
/// 97.3 %. Rebuilding per application would make replay slower than the
/// interpretation it replaces; rebuilding per *definition* costs ~41 masks.
///
/// The memo is keyed by [`ClipId`], and because a definition's parent always
/// has a smaller index (see [`ClipId`]), recursion terminates without a
/// cycle check.
struct MaskBuilder<'a> {
    defs: &'a [ClipDef],
    built: Vec<Option<Option<Arc<Mask>>>>,
    width: u32,
    height: u32,
    x0: f32,
    y0: f32,
}

impl<'a> MaskBuilder<'a> {
    fn new(defs: &'a [ClipDef], width: u32, height: u32, x0: f32, y0: f32) -> Self {
        Self {
            defs,
            built: vec![None; defs.len()],
            width,
            height,
            x0,
            y0,
        }
    }

    /// A recorded page-device CTM, moved into this region's device space.
    ///
    /// One `post_translate` by the region's device origin — which is
    /// precisely the difference between the base CTM `render_page` uses and
    /// the one `render_page_region` uses, so this is the region relationship
    /// the crate already has, applied at a different point in the chain.
    fn to_region(&self, ctm: Transform) -> Transform {
        ctm.post_translate(-self.x0, -self.y0)
    }

    fn mask_for(&mut self, id: Option<ClipId>) -> Option<Arc<Mask>> {
        let id = id?;
        if let Some(cached) = &self.built[id.index()] {
            return cached.clone();
        }
        let def = self.defs[id.index()].clone();
        // Parent first — except for an empty clip, which deliberately does
        // NOT recurse: it admits nothing regardless of what it was
        // intersected with, and the painting path models it as a bare
        // all-zero mask rather than as a product.
        let parent = if def.path.is_none() {
            None
        } else {
            self.mask_for(def.parent)
        };
        let built = self.build(&def, parent.as_deref());
        self.built[id.index()] = Some(built.clone());
        built
    }

    fn build(&self, def: &ClipDef, parent: Option<&Mask>) -> Option<Arc<Mask>> {
        let mut mask = Mask::new(self.width, self.height)?;
        let Some(path) = def.path.as_deref() else {
            // `Mask::new` zero-fills, so an all-zero mask is already what
            // an empty clip is. Stated rather than left implicit, because
            // "the constructor zeroes" is exactly the assumption a future
            // allocator change would break silently.
            return Some(Arc::new(mask));
        };
        mask.fill_path(path, def.rule, true, self.to_region(def.ctm));
        if let Some(old) = parent {
            // The painting path restricts this multiply to the new path's
            // device bounds and documents that the restriction is an
            // IDENTITY (outside them the new mask is zero, and 0 × old is
            // 0). Doing it unrestricted here therefore produces the same
            // bytes, and is one fewer place for the bounds arithmetic to
            // be wrong.
            let old_data = old.data();
            for (n, o) in mask.data_mut().iter_mut().zip(old_data.iter()) {
                *n = u8::try_from((u16::from(*n) * u16::from(*o)) / 255).unwrap_or(255);
            }
        }
        Some(Arc::new(mask))
    }
}

/// A path's approximate heap footprint.
///
/// The single definition, used by the recorder's running total and therefore
/// by [`DisplayList::memory_bytes`] — see `RecorderState::approx_bytes` for
/// why those must not be two calculations.
fn path_bytes(p: &Path) -> usize {
    std::mem::size_of::<Path>() + std::mem::size_of_val(p.points()) + p.verbs().len()
}

/// One op's approximate heap footprint, its nested layer contents included.
fn op_bytes(op: &Op) -> usize {
    std::mem::size_of::<Op>()
        + match op {
            Op::Fill { path, .. } | Op::Stroke { path, .. } => path_bytes(path),
            Op::Layer { ops, .. } => ops.iter().map(op_bytes).sum(),
        }
}

/// Device-space bounds of a filled path under `ctm`.
pub(crate) fn fill_bounds(path: &Path, ctm: Transform) -> Option<DeviceBounds> {
    let b = path.bounds().transform(ctm)?;
    Some(DeviceBounds {
        left: b.left(),
        top: b.top(),
        right: b.right(),
        bottom: b.bottom(),
    })
}

/// Device-space bounds of a **stroked** path under `ctm`.
///
/// # Why this is deliberately generous
///
/// A stroke marks outside its path's own bounds — by half the line width
/// everywhere, by more at a miter join, and by the cap at an open end. The
/// exact answer is `path.stroke(...)`'s outline, which costs about as much
/// as painting the stroke and would defeat the cull it feeds.
///
/// So the path bounds are outset in **path space** by `width/2 × miter
/// limit` (the worst case a miter can reach) before transforming, and the
/// caller's [`DeviceBounds::intersects`] adds a further device pixel.
///
/// The asymmetry is the point: over-estimating costs a paint that would
/// have been culled; under-estimating **drops a visible mark**, and the
/// acceptance criterion is byte-identity. A zero width is treated as one
/// device pixel, because that is what the rasteriser draws for a hairline.
pub(crate) fn stroke_bounds(path: &Path, stroke: &Stroke, ctm: Transform) -> Option<DeviceBounds> {
    let b = path.bounds();
    let pad = (stroke.width / 2.0) * stroke.miter_limit.max(1.0);
    let pad = if pad.is_finite() { pad.max(0.0) } else { 0.0 };
    let outset = tiny_skia::Rect::from_ltrb(
        b.left() - pad,
        b.top() - pad,
        b.right() + pad,
        b.bottom() + pad,
    )?;
    let d = outset.transform(ctm)?;
    // A hairline (width 0, or a width that scales below a pixel) is drawn
    // one device pixel wide whatever the geometry says.
    Some(DeviceBounds {
        left: d.left() - 1.0,
        top: d.top() - 1.0,
        right: d.right() + 1.0,
        bottom: d.bottom() + 1.0,
    })
}

/// The clip table and op stack a recording canvas writes into.
///
/// Lives here rather than in `canvas` because everything it stores is a
/// display-list concept; `canvas` knows only that it has somewhere to put
/// what it is handed.
pub(crate) struct RecorderState {
    /// Full-page device size at the recording scale — what
    /// `Canvas::width`/`height` report, and what clip bboxes clamp to.
    pub width: u32,
    pub height: u32,
    /// The op-vector stack. `frames[0]` is the page; a `Layer` pushes.
    pub frames: Vec<Vec<Op>>,
    /// Clip definitions, parents always at smaller indices.
    pub clips: Vec<ClipDef>,
    /// Definitions already recorded, keyed by `(build hash, parent)` — see
    /// [`RecorderState::push_clip`] for why this table is load-bearing
    /// rather than an optimisation.
    seen_clips: HashMap<(u64, Option<ClipId>), ClipId>,
    /// The first refusal, if any. First rather than last because it is the
    /// one whose context a reader can still reconstruct.
    pub poison: Option<PoisonReason>,
    /// Running heap estimate, in the same units [`DisplayList::memory_bytes`]
    /// reports.
    ///
    /// Accumulated rather than computed at the end **on purpose**: the number
    /// that guards [`MAX_DISPLAY_LIST_BYTES`] and the number reported to the
    /// caller must be the *same* number, or the guard enforces something the
    /// caller cannot see. Doing it incrementally also means a runaway page is
    /// refused while it runs rather than after it has already allocated.
    pub approx_bytes: usize,
    /// The ceiling `approx_bytes` is charged against.
    ///
    /// A field rather than a direct read of [`MAX_DISPLAY_LIST_BYTES`] for
    /// exactly one reason: **so the refusal can be tested.** Proving the
    /// guard fires by recording 256 MiB of real ops would be a minute-long
    /// test that allocates a quarter of a gigabyte to assert one boolean;
    /// proving it with a small limit and four ops is the same assertion in
    /// milliseconds. A guard whose only evidence is that it compiles is a
    /// guard nobody has seen work.
    pub max_bytes: usize,
}

impl RecorderState {
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            frames: vec![Vec::new()],
            clips: Vec::new(),
            seen_clips: HashMap::new(),
            poison: None,
            approx_bytes: 0,
            max_bytes: MAX_DISPLAY_LIST_BYTES,
        }
    }

    /// Append an op to the innermost open frame, charging its size against
    /// [`MAX_DISPLAY_LIST_BYTES`].
    ///
    /// Once the ceiling is crossed the recording is poisoned and further ops
    /// are **dropped** rather than stored: there is nothing to gain by
    /// growing a list that has already been refused. The walk itself
    /// continues rather than unwinding, so the diagnostics still describe
    /// what the page actually contains — the recording is what failed, not
    /// the interpretation.
    pub(crate) fn push(&mut self, op: Op) {
        if self.poison == Some(PoisonReason::TooLarge) {
            return;
        }
        self.approx_bytes += op_bytes(&op);
        if self.approx_bytes > self.max_bytes {
            self.poison(PoisonReason::TooLarge);
            return;
        }
        if let Some(frame) = self.frames.last_mut() {
            frame.push(op);
        }
    }

    /// Record a clipping path and return its id, **reusing an existing
    /// definition** when this one is identical.
    ///
    /// # ★ Why deduplication here is correctness-shaped, not tidiness
    ///
    /// Because the replay builds one mask per *distinct id*, and a page
    /// applies vastly more clips than it defines. On the reference CAD sheet
    /// that ratio is **24,128 applications over ~40 distinct masks** — one
    /// path alone accounts for 97.3 % — and the painting path already
    /// exploits it via [`crate::clip_cache::ClipCache`], which is where the
    /// 99.83 % hit rate comes from.
    ///
    /// A recorder that pushed a fresh definition per `W n` would hand the
    /// replay 24,128 masks to build instead of 40, and each one is a
    /// region-sized buffer plus a `fill_path`. **Measured, before this table
    /// existed: 1.79 s per replayed frame at scale 1 against 0.71 s for a
    /// direct region render — the cache was 2.5x SLOWER than no cache.** It
    /// looked fine at scale 8, where the viewport is small enough that
    /// almost every op culls before its clip is ever requested, which is
    /// exactly the shape of measurement that ships a regression.
    ///
    /// # The key, and the risk it inherits deliberately
    ///
    /// `ClipCache::build_key` — the *same* hash the painting path
    /// deduplicates on, so the two agree by construction about what "the
    /// same clip" means — paired with the parent id, because two identical
    /// paths intersected with different incoming clips are different masks.
    ///
    /// It is a hash comparison with no confirming equality check, so a
    /// collision would alias two distinct clips. That is the risk the live
    /// clip cache already carries for the same reason (and documents at
    /// `ClipCache::build_key`); matching it is deliberate, because a
    /// recorder that deduplicated differently from the painter would make
    /// the two disagree about a mask, which is a worse failure than the one
    /// being avoided.
    pub(crate) fn push_clip(&mut self, def: ClipDef) -> ClipId {
        let key = def.path.as_deref().map(|path| {
            crate::clip_cache::ClipCache::build_key(
                path,
                def.rule,
                def.ctm,
                self.width,
                self.height,
            )
        });
        // An EMPTY clip is never deduplicated: it has no path to hash, and
        // there is nothing to gain — a page emits at most a handful.
        if let Some(hash) = key
            && let Some(existing) = self.seen_clips.get(&(hash, def.parent))
        {
            return *existing;
        }
        #[allow(clippy::cast_possible_truncation)]
        let id = ClipId(self.clips.len() as u32);
        self.approx_bytes +=
            std::mem::size_of::<ClipDef>() + def.path.as_deref().map_or(0, path_bytes);
        if let Some(hash) = key {
            self.seen_clips.insert((hash, def.parent), id);
        }
        self.clips.push(def);
        id
    }

    /// Mark the recording unusable, keeping the FIRST reason.
    pub(crate) fn poison(&mut self, reason: PoisonReason) {
        if self.poison.is_none() {
            self.poison = Some(reason);
        }
    }
}

/// A display list under construction.
///
/// A thin wrapper so `canvas` can hold `&mut Recorder` without importing
/// the op vocabulary, and so `record_page` can take the finished halves
/// apart without exposing the stack.
pub(crate) type Recorder = RecorderState;

impl Recorder {
    /// The refusal, if the walk hit one.
    pub(crate) const fn poison_reason(&self) -> Option<PoisonReason> {
        self.poison
    }

    /// Consume the recorder, yielding the root op list and the clip table.
    ///
    /// Any frames still open are a recorder bug rather than a document
    /// problem — `Canvas::layer` pops what it pushes — so they are
    /// flattened into the root rather than dropped, which fails visibly
    /// (wrong compositing) instead of invisibly (missing content).
    pub(crate) fn finish(mut self) -> (Vec<Op>, Vec<ClipDef>) {
        while self.frames.len() > 1 {
            let inner = self.frames.pop().unwrap_or_default();
            if let Some(outer) = self.frames.last_mut() {
                outer.extend(inner);
            }
        }
        (self.frames.pop().unwrap_or_default(), self.clips)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::{BlendMode, PathBuilder, Rect as SkRect};

    use crate::canvas::{Brush, BrushSpec};

    fn unit_fill() -> Op {
        let rect = SkRect::from_ltrb(0.0, 0.0, 1.0, 1.0).expect("unit rect");
        Op::Fill {
            path: Arc::new(PathBuilder::from_rect(rect)),
            brush: BrushSpec {
                brush: Brush::Solid {
                    rgba: [0, 0, 0, 255],
                },
                blend: BlendMode::SourceOver,
                anti_alias: true,
                cmyk: None,
                spots: Vec::new(),
                process_tints: None,
            },
            rule: FillRule::Winding,
            ctm: Transform::identity(),
            clip: None,
            bounds: None,
        }
    }

    /// ★ The §10 ceiling actually fires, and does so **by name**.
    ///
    /// Driven against a deliberately tiny limit — see `RecorderState::max_bytes`
    /// for why the limit is a field.
    #[test]
    fn the_size_ceiling_refuses_rather_than_growing() {
        let mut rec = Recorder::new(100, 100);
        rec.max_bytes = std::mem::size_of::<Op>() * 2;

        for _ in 0..64 {
            rec.push(unit_fill());
        }

        assert_eq!(
            rec.poison_reason(),
            Some(PoisonReason::TooLarge),
            "crossing the ceiling must poison the recording by name"
        );
        let (ops, _) = rec.finish();
        assert!(
            ops.len() < 64,
            "ops recorded after the refusal are dropped, not stored; got {}",
            ops.len()
        );
    }

    /// The control: the same 64 ops under the real ceiling are all kept, so
    /// the test above is measuring the ceiling and not some other refusal.
    #[test]
    fn the_size_ceiling_does_not_fire_on_an_ordinary_page() {
        let mut rec = Recorder::new(100, 100);
        for _ in 0..64 {
            rec.push(unit_fill());
        }
        assert_eq!(rec.poison_reason(), None);
        let (ops, _) = rec.finish();
        assert_eq!(ops.len(), 64);
    }

    /// A layer's contents are charged to the ceiling too.
    ///
    /// Worth its own case because `Op::Layer`'s own `size_of` says nothing
    /// about the vector hanging off it — the natural way to write the
    /// accounting misses exactly this, and the failure mode is a page of
    /// deeply nested groups slipping past the guard.
    #[test]
    fn a_layers_contents_count_towards_the_ceiling() {
        let bare = op_bytes(&Op::Layer {
            paint: LayerPaint {
                opacity: 1.0,
                blend: BlendMode::SourceOver,
                nonseparable: None,
            },
            ops: Vec::new(),
        });
        let loaded = op_bytes(&Op::Layer {
            paint: LayerPaint {
                opacity: 1.0,
                blend: BlendMode::SourceOver,
                nonseparable: None,
            },
            ops: vec![unit_fill(), unit_fill()],
        });
        assert!(
            loaded > bare,
            "a layer holding two fills must cost more than an empty one \
             ({loaded} vs {bare})"
        );
    }

    /// Clip definitions are deduplicated, which is what keeps a replay's
    /// mask count near 40 instead of near 24,000 on the reference sheet.
    #[test]
    fn an_identical_clip_is_recorded_once() {
        let rect = SkRect::from_ltrb(0.0, 0.0, 10.0, 10.0).expect("rect");
        let def = || ClipDef {
            path: Some(Arc::new(PathBuilder::from_rect(rect))),
            rule: FillRule::Winding,
            ctm: Transform::identity(),
            parent: None,
        };
        let mut rec = Recorder::new(100, 100);
        let a = rec.push_clip(def());
        let b = rec.push_clip(def());
        assert_eq!(
            a, b,
            "the same clip under the same parent is one definition"
        );

        // ...but not across DIFFERENT parents, because the intersected mask
        // differs even when the path does not.
        let nested = ClipDef {
            parent: Some(a),
            ..def()
        };
        assert_ne!(rec.push_clip(nested), a);
        assert_eq!(rec.clips.len(), 2);
    }
}

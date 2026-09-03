//! `Canvas` — the one thing the content-stream interpreter draws onto.
//!
//! # Why this module exists
//!
//! Until `Pass 75.0` the interpreter threaded `&mut tiny_skia::Pixmap`
//! through sixteen signatures and painted straight into it. That is
//! perfectly correct and perfectly un-reusable: **every render re-walks the
//! whole content stream**, because the only artefact a walk produces is
//! pixels, and pixels cannot be replayed at a different viewport.
//!
//! Measured on the reference A3 CAD sheet
//! (`ncored-benchmark-cad-drawing.pdf`, 148,517 paints · 24,128 clip ops —
//! `docs/render-region-measurements.md` §4a):
//!
//! | fact | number |
//! |---|---:|
//! | a **1 × 1 point** region — 2 pixels | ~667 ms |
//! | the whole page, scale 1 — 1,002,822 pixels | ~941 ms |
//! | the same run with every `fill_path`/`stroke_path` ablated away | ~591 ms |
//! | ⇒ time actually spent **painting** | **~11 %** |
//! | ⇒ time spent **outside** `Interpreter::paint` | **~83 %** |
//!
//! A two-pixel render costing 667 ms is not a rasteriser problem. It is
//! the cost of *interpretation* — tokenising, operator dispatch, graphics
//! state, and `PathBuilder` pushes — paid in full for a viewport the size
//! of a full stop. A shell that pans by re-rendering a region therefore
//! pays ~0.7 s per frame on this document, which is the regression
//! `Pass 75.0` exists to prevent.
//!
//! `Canvas` is the seam that makes the walk's output *substitutable*. The
//! interpreter no longer knows whether the thing it draws onto is a
//! pixmap or a tape recorder; it hands over finished paths, decomposed
//! brushes and clip references, and something downstream decides whether
//! those become pixels now or a [`crate::display_list::DisplayList`] to
//! replay later.
//!
//! # The contract this type must honour, and why it is stated so bluntly
//!
//! **In paint mode, `Canvas` must be byte-for-byte indistinguishable from
//! painting into the `Pixmap` directly.** Not "visually identical", not
//! "within a rounding step" — identical, because this crate's whole test
//! suite asserts exact pixels and the pdfium parity harness compares
//! whole-page rasters. Every method here is therefore a *forward*, and the
//! brush decomposition in [`BrushSpec::to_paint`] rebuilds exactly the
//! `tiny_skia::Paint` the call site used to build inline.
//!
//! That is also why the indirection landed as its own commit, with the
//! full suite and the parity harness green, **before** any recording code
//! existed: a green run at that point proves the plumbing is transparent,
//! so every later failure is in the recorder rather than in the seam.
//!
//! # What lives here and what deliberately does not
//!
//! Here: the target abstraction ([`Canvas`]), the owned description of a
//! paint ([`Brush`], [`BrushSpec`]), and the layer primitive
//! ([`Canvas::layer`]) that models "composite this sub-drawing as one
//! object" — which is both §11.4.5's transparency-group composite and an
//! annotation's `/CA` constant-alpha composite, because those two are the
//! same operation and always were.
//!
//! Not here: anything that knows what a PDF is. `Canvas` has no opinion
//! about operators, resources or the standard; it is a drawing target.

use std::sync::Arc;

use tiny_skia::{
    BlendMode, FillRule, FilterQuality, Mask, Paint, Path, Pattern, Pixmap, PixmapPaint,
    SpreadMode, Stroke, Transform,
};

use crate::display_list::{
    ClipDef, ClipId, DeviceBounds, Op, PoisonReason, Recorder, fill_bounds, stroke_bounds,
};

use crate::cmyk_paint::{device_region, paint_solid_into_cmyk};

/// What a paint is made of, in **owned** terms.
///
/// # Why this exists at all — `tiny_skia::Paint` cannot be stored
///
/// `Paint<'a>` holds `Shader<'a>`, and the image shader
/// ([`Pattern`]) **borrows the texel pixmap it samples**. A recorder that
/// tried to keep a `Paint` around would be keeping a borrow of a buffer
/// the interpreter is about to drop. So a paint is decomposed into owned
/// parts on the way in and rebuilt on the way out — which is also why
/// [`BrushSpec::to_paint`] is the single place the rebuild happens, and
/// why it must reproduce the old inline construction exactly.
#[derive(Debug, Clone)]
pub(crate) enum Brush {
    /// A solid colour, stored as the **8-bit RGBA quadruple** rather than
    /// a `tiny_skia::Color`.
    ///
    /// Deliberate: the interpreter's `solid()` has always built its paint
    /// with `Paint::set_color_rgba8`, so storing floats and converting
    /// back would re-run a lossy quantisation and could land a step away
    /// from the byte the old code produced. Storing what was actually
    /// handed to `tiny_skia` removes the question.
    Solid {
        /// `[r, g, b, a]`, already quantised exactly as the call site did.
        rgba: [u8; 4],
    },
    /// An image, sampled through [`Pattern`] over §8.9.4's unit square.
    ///
    /// Constructed **only** by the recording branch of
    /// [`Canvas::fill_image`]: paint mode builds its shader straight off
    /// the interpreter's borrow, so the texel copy this variant implies is
    /// paid exactly where something will read it back.
    /// A native gradient (`Pass 248.3`) — recorded by the EXPORT recorder
    /// only, so an SVG can write `<linearGradient>`/`<radialGradient>`
    /// instead of a raster. Replay paints it through tiny-skia's own
    /// gradient shaders, `Pad` spread; the `Extend` = false ends that
    /// SVG expresses as a clip are approximated by `Pad` on replay, which
    /// no cache ever sees (cache mode still refuses shadings).
    Gradient(Arc<crate::shading::GradientSpec>),
    Image {
        /// The decoded texels, owned because the interpreter's own copy
        /// goes out of scope while a display list outlives the walk.
        texels: Arc<Pixmap>,
        /// Nearest or bilinear, as chosen by `/Interpolate` and the
        /// operator's minification setting (`IM-A1`).
        quality: FilterQuality,
        /// Image space to user space (the unit-square flip).
        transform: Transform,
    },
}

/// A [`Brush`] plus the two paint-level flags that are not part of the
/// brush itself: §11.3.5's blend mode and the anti-alias switch.
///
/// Kept separate from [`Brush`] because both flags are properties of *this
/// paint*, not of the colour or image being painted with — an image's
/// anti-alias flag, in particular, is a function of the CTM
/// (`image_edge_needs_antialiasing`), not of the image.
#[derive(Debug, Clone)]
pub(crate) struct BrushSpec {
    /// What to paint with.
    pub brush: Brush,
    /// §11.3.5 `/BM`, carried on the paint so path fill, path stroke,
    /// glyph fill and glyph stroke cannot come to disagree about it.
    pub blend: BlendMode,
    /// Whether tiny_skia anti-aliases this paint's edges.
    pub anti_alias: bool,
    /// The paint's SPOT colorants and their tints, when the file stated
    /// any — the half [`Self::cmyk`] structurally cannot carry.
    ///
    /// Empty for every process colour space, which is 98.6 % of a
    /// 4,023-file corpus, so the common paint allocates nothing.
    ///
    /// # Why the LUT rides along, and why it is an `Arc`
    ///
    /// A spot colorant's appearance comes from its **tint transform**
    /// (§8.6.6.4), which is a property of the colour space and is
    /// therefore knowable only in the interpreter. The colorant buffer,
    /// which is where the tint has to arrive, sees only a `BrushSpec`. So
    /// the curve has to travel with the paint.
    ///
    /// It is sampled **once per colorant per document**, cached in the
    /// interpreter, and shared by `Arc` — so cloning a `BrushSpec`
    /// (which happens per paint, and again inside every knockout split)
    /// is a refcount bump rather than 256 samples of a PostScript
    /// calculator function. Putting a `SpotLut` here by value would
    /// reintroduce exactly the per-paint cost the type was created to
    /// avoid.
    pub spots: Vec<SpotInk>,
    /// The paint's colour as **process tints only** — the four process
    /// channels this source actually named, with everything else zero —
    /// for use when [`Self::spots`] are deposited into their own planes.
    ///
    /// # ★★ Why this is a THIRD colour field and not a refinement of `cmyk`
    ///
    /// [`Self::cmyk`] is the colour **flattened**: for a `/Separation` over
    /// a `DeviceCMYK` alternate it is the tint transform's own output,
    /// which is what `Pass 140.1` established as the right paint colour and
    /// what makes a spot FILL and a spot IMAGE of the same tint agree.
    ///
    /// That is correct exactly while the spot has nowhere else to go. The
    /// moment its tint is also deposited into a plane, the flattened value
    /// lays the SAME INK DOWN A SECOND TIME — the collapse multiplies the
    /// plane's contribution into a process colour that already contains it.
    ///
    /// Not theoretical: the first cut of the deposit did exactly this, and
    /// `devicen_image_ink`'s agreement tests caught it as `(97, 169, 135)`
    /// against an expected `(158, 208, 186)`. The arithmetic is decisive —
    /// `158² / 255 = 97.9` — which is what a value multiplied by itself
    /// looks like.
    ///
    /// So the two answer different questions and both are needed:
    /// *"what does this colour flatten to"* (`cmyk`, used when no plane is
    /// granted) and *"which process channels did this source state"*
    /// (`process_tints`, used when every spot got one). A spot-only source
    /// answers `[0, 0, 0, 0]` here, which is the truth: it states no
    /// process ink at all.
    pub process_tints: Option<[f32; 4]>,
    /// The paint's colour as **authored subtractive tints**, when the
    /// canvas it lands on composites in a subtractive space.
    ///
    /// # Why this is a second field rather than a replacement for the
    /// quantised RGBA
    ///
    /// Because the sRGB path must not move. [`Brush::Solid`]'s `[u8; 4]`
    /// is byte-locked by a test to what the interpreter's inline `solid()`
    /// produced before `Pass 75.0`, and a "correctness improvement"
    /// smuggled in under a colorant field is exactly the change nobody
    /// would think to look for when the parity harness moved. So the two
    /// coexist: `rgba` is what `tiny_skia` paints, `cmyk` is what the
    /// colorant buffer composites, and neither derives from the other.
    ///
    /// # Why it is `Option` rather than always present
    ///
    /// `None` means "the interpreter did not resolve colorants for this
    /// paint" -- a pattern fill, a recorded image brush replayed later, or
    /// a paint built by a test. The colorant buffer then falls back to
    /// [`crate::overprint::rgb_to_cmyk`] on the quantised RGB, which is
    /// §11.6.6's required conversion performed with the only transform
    /// this crate has, and is measurably worse than the authored values
    /// (`DeviceCMYK 0 1 0 0` recovers as `0, 0.995, 0.409, 0.071`). The
    /// distinction is worth a branch precisely because it is not free.
    pub cmyk: Option<[f32; 4]>,
}

/// One spot colorant a paint states: its identity, its tint, and how it
/// looks.
///
/// # The name is the identity, as BYTES
///
/// §8.6.6.4's device test consults only the colorant name, and §7.3.5
/// NOTE 4 makes byte-differing names distinct **even if they render
/// identically**. This is the key `CmykBuffer::spot_index` matches on, so
/// a lossy decode here would let two different inks share one plane and
/// composite as one colour.
#[derive(Debug, Clone)]
pub(crate) struct SpotInk {
    /// The colorant name, `#xx`-decoded — the comparison form §7.3.5
    /// specifies.
    pub colorant: std::sync::Arc<[u8]>,
    /// The tint the file stated, `0.0..=1.0`.
    pub tint: f32,
    /// This colorant alone on white paper, sampled across the tint range.
    ///
    /// Shared rather than owned — see [`BrushSpec::spots`]. Built by the
    /// interpreter, which is the only place the tint transform is
    /// reachable.
    pub lut: std::sync::Arc<crate::cmyk_buffer::SpotLut>,
}

impl BrushSpec {
    /// A solid colour at `alpha`, quantised exactly as the interpreter has
    /// always quantised it.
    ///
    /// The `as u8` truncations and the `round()` on alpha are **copied
    /// deliberately** from the previous inline `solid()` — changing either
    /// would shift colours by a level on some documents, and a
    /// "correctness improvement" smuggled in under a refactor is exactly
    /// the change nobody would think to look for when the parity harness
    /// moved.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn solid(c: crate::gstate::Rgb, alpha: f32, blend: BlendMode) -> Self {
        Self {
            brush: Brush::Solid {
                rgba: [
                    (c.r * 255.0) as u8,
                    (c.g * 255.0) as u8,
                    (c.b * 255.0) as u8,
                    (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
                ],
            },
            blend,
            // Every solid paint in this renderer has always been
            // anti-aliased; the flag is spelled out rather than defaulted
            // so the image case below reads as the exception it is.
            anti_alias: true,
            // Deliberately NOT derived from `c` here. The interpreter knows
            // the authored colour space and this function does not; a
            // reconstruction made at this level would be indistinguishable
            // from an authored value at the point of use, which is the one
            // property the colorant buffer must be able to tell apart.
            cmyk: None,
            // Same argument as `cmyk` above: only the interpreter knows
            // whether a colour space named a spot colorant, and an empty
            // vector here is the honest "this level was not told".
            spots: Vec::new(),
            process_tints: None,
        }
    }

    /// A device-space raster as a brush (`Pass 248.1`): the export
    /// recorder's harvested scratch, placed by `transform` (a pure
    /// translation to the harvested box's origin). `Nearest`, not
    /// anti-aliased, and no colorant data -- it is already pixels on
    /// the page grid, and resampling it would blur what was exact.
    pub(crate) fn raster(texels: Arc<Pixmap>, transform: Transform) -> Self {
        Self {
            brush: Brush::Image {
                texels,
                quality: FilterQuality::Nearest,
                transform,
            },
            blend: BlendMode::SourceOver,
            anti_alias: false,
            cmyk: None,
            spots: Vec::new(),
            process_tints: None,
        }
    }

    /// The same paint, carrying the spot colorants the file stated.
    ///
    /// Separate from [`Self::with_cmyk`] rather than folded into it
    /// because the two halves come from different readers
    /// (`overprint::authored_tints` and `overprint::authored_spots`) and a
    /// source can legitimately have one and not the other: a
    /// `/Separation` states a spot and no process tint, and a
    /// `DeviceCMYK` states process tints and no spot.
    #[must_use]
    pub(crate) fn with_spots(mut self, spots: Vec<SpotInk>, process: Option<[f32; 4]>) -> Self {
        self.spots = spots;
        self.process_tints = process;
        self
    }

    /// The same paint, carrying its authored subtractive tints.
    ///
    /// Called by the interpreter, which is the only place that can answer
    /// "what colorants did the file actually state?" -- see
    /// `Interpreter::authored_cmyk`.
    #[must_use]
    pub(crate) fn with_cmyk(mut self, cmyk: [f32; 4]) -> Self {
        self.cmyk = Some(cmyk);
        self
    }

    /// Rebuild the `tiny_skia::Paint` this spec describes.
    ///
    /// The returned paint borrows `self` (for the image case's texels),
    /// which is why this returns a value with a lifetime rather than a
    /// `'static` paint.
    pub(crate) fn to_paint(&self) -> Paint<'_> {
        match &self.brush {
            Brush::Solid { rgba } => {
                let mut paint = Paint::default();
                paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
                paint.anti_alias = self.anti_alias;
                paint.blend_mode = self.blend;
                paint
            }
            Brush::Gradient(g) => {
                use crate::shading::GradientKind;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let a = (g.alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
                let stops: Vec<tiny_skia::GradientStop> = g
                    .stops
                    .iter()
                    .map(|(o, c)| {
                        tiny_skia::GradientStop::new(
                            *o,
                            tiny_skia::Color::from_rgba8(c[0], c[1], c[2], a),
                        )
                    })
                    .collect();
                let shader = match g.kind {
                    GradientKind::Linear { x0, y0, x1, y1 } => tiny_skia::LinearGradient::new(
                        tiny_skia::Point::from_xy(x0, y0),
                        tiny_skia::Point::from_xy(x1, y1),
                        stops,
                        SpreadMode::Pad,
                        g.transform,
                    ),
                    GradientKind::Radial { cx, cy, r, fx, fy } => tiny_skia::RadialGradient::new(
                        tiny_skia::Point::from_xy(fx, fy),
                        tiny_skia::Point::from_xy(cx, cy),
                        r,
                        stops,
                        SpreadMode::Pad,
                        g.transform,
                    ),
                };
                Paint {
                    // A degenerate gradient (zero-length axis, zero radius)
                    // paints nothing rather than a guessed solid.
                    shader: shader
                        .unwrap_or(tiny_skia::Shader::SolidColor(tiny_skia::Color::TRANSPARENT)),
                    blend_mode: self.blend,
                    anti_alias: self.anti_alias,
                    force_hq_pipeline: false,
                }
            }
            Brush::Image {
                texels,
                quality,
                transform,
            } => Paint {
                shader: Pattern::new(
                    texels.as_ref().as_ref(),
                    SpreadMode::Pad,
                    *quality,
                    1.0,
                    *transform,
                ),
                blend_mode: self.blend,
                anti_alias: self.anti_alias,
                force_hq_pipeline: false,
            },
        }
    }

    /// Split this paint into **shape** and **opacity** — the two things
    /// §11.4.6 refuses to let a knockout group collapse.
    ///
    /// Returns a copy of the spec painting at **full opacity**, plus the
    /// `q_s` that was taken out of it.
    ///
    /// # Why a knockout element cannot just be painted normally
    ///
    /// §11.4.8 scales the destination by `(1 − f_si)` where the
    /// non-knockout formula has `(1 − α_si)`. Since `α_s = f_s × q_s`, the
    /// two coincide **exactly** when `q_s = 1` and diverge otherwise — a
    /// knockout element erases more of what is under it than an ordinary
    /// one does. So the compositor needs `f_s` on its own, and the only
    /// way to obtain it from the rasteriser is to ask for coverage
    /// **without** the constant alpha folded in: paint at `q_s = 1` and
    /// read the resulting alpha channel, which is then pure coverage.
    ///
    /// Image brushes carry no constant alpha of their own — the shader is
    /// built at opacity 1 and any `/ca` reaches them through a different
    /// route — so they report `q_s = 1.0` and are unchanged.
    pub(crate) fn split_shape_and_opacity(&self) -> (Self, f32) {
        match &self.brush {
            Brush::Solid { rgba } => (
                Self {
                    brush: Brush::Solid {
                        rgba: [rgba[0], rgba[1], rgba[2], 255],
                    },
                    blend: self.blend,
                    anti_alias: self.anti_alias,
                    // The colorants are a property of the COLOUR. Splitting
                    // shape from opacity changes the alpha byte and nothing
                    // else, so dropping them here would silently downgrade
                    // an authored paint to a reconstructed one inside every
                    // knockout group.
                    cmyk: self.cmyk,
                    // Carried for the identical reason, and cheaply: the
                    // clone is a refcount bump per colorant. Dropping them
                    // here would make a spot fill inside a knockout group
                    // silently lose its plane and fall back to flattening.
                    spots: self.spots.clone(),
                    process_tints: self.process_tints,
                },
                f32::from(rgba[3]) / 255.0,
            ),
            Brush::Image { .. } | Brush::Gradient(_) => (self.clone(), 1.0),
        }
    }

    /// The 8-bit quadruple this spec paints with, when it is a solid.
    ///
    /// Exists for the round-trip assertions below and for nothing else — a
    /// paint's colour never affects *where* it lands, so this is
    /// diagnostic rather than geometric.
    #[cfg(test)]
    pub(crate) const fn solid_rgba(&self) -> Option<[u8; 4]> {
        match &self.brush {
            Brush::Solid { rgba } => Some(*rgba),
            Brush::Image { .. } | Brush::Gradient(_) => None,
        }
    }
}

/// How a sub-drawing is composited back into its parent — the
/// `draw_pixmap` half of a transparency group or a `/CA` annotation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LayerPaint {
    /// Constant alpha applied to the layer **as a whole**, which is the
    /// entire reason a layer exists: applying it per-operator instead
    /// darkens every place the drawing overlaps itself.
    pub opacity: f32,
    /// The blend mode in force at the composite (§11.4.5: the outer
    /// state applies to the group's *result*, not to its contents).
    pub blend: BlendMode,
    /// A §11.3.5.3 **non-separable** outer mode, if the state carries one.
    ///
    /// Separate from [`Self::blend`] for the same reason
    /// [`crate::gstate::GraphicsState::nonseparable`] is separate from
    /// `blend_mode`: these four cannot be expressed as a
    /// `tiny_skia::BlendMode` without being computed wrongly. When this is
    /// `Some`, [`Self::blend`] is `SourceOver` and the composite goes per
    /// pixel through [`crate::blend_nonsep`].
    pub nonseparable: Option<crate::blend_nonsep::NonSeparableBlend>,
}

/// The clip in force at a paint, in **both** the representations the two
/// canvas modes need.
///
/// # Why one type rather than two arguments
///
/// Because the two are the same fact and must not be able to disagree.
/// Painting needs a device-sized coverage `Mask`; recording needs an index
/// into a clip table, because a mask is valid only for the pixmap geometry
/// that built it (`crate::display_list` module docs §2.2). Threading them
/// separately would let a call site pass one and forget the other, and the
/// failure mode of forgetting the id is a recorded paint that **ignores its
/// clip** — content spilling outside a clipped region, on replay only, on
/// documents nobody thought to check.
///
/// Both are read out of the graphics state together
/// ([`crate::gstate::GraphicsState::clip_ref`]), so `q`/`Q` carry them as
/// the pair they are.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ClipRef<'a> {
    /// The built coverage mask — always `None` while recording, because a
    /// recording canvas never builds one.
    pub mask: Option<&'a Mask>,
    /// The recorded clip definition — always `None` while painting.
    pub id: Option<ClipId>,
}

/// The interpreter's drawing target.
///
/// The interpreter cannot tell these apart, and that is the whole design:
/// one content-stream walk serves both rasterising now and recording for
/// later, so there is no second interpreter to drift.
pub(crate) enum Canvas<'a> {
    /// Draw straight into a pixmap — the original behaviour, byte for byte.
    Paint(&'a mut Pixmap),
    /// Draw nowhere; record what *would* have been drawn, for replay
    /// against a viewport chosen later (`crate::display_list`).
    Record(&'a mut Recorder),
    /// Draw into a **knockout group** (§11.4.6): every element composites
    /// against the group's *initial* backdrop rather than against the
    /// elements beneath it, so each one has to be rasterised on its own
    /// before it can be accumulated. See [`KnockoutTarget`].
    Knockout(&'a mut KnockoutTarget),
    /// Draw into a **subtractive colorant buffer** (§11.7.2, §11.6.6):
    /// the page's blending colour space is `DeviceCMYK`, so every blend
    /// and every composite on this page is required to happen in ink
    /// rather than on screen. See [`crate::cmyk_buffer::CmykBuffer`].
    ///
    /// Structurally this is the same move [`Self::Knockout`] makes and for
    /// the same reason: `tiny_skia` can rasterise a shape but cannot
    /// composite it under a model it does not implement, so the paint is
    /// rasterised to a coverage mask and composited here.
    Cmyk(&'a mut crate::cmyk_buffer::CmykBuffer),
}

impl<'a> Canvas<'a> {
    /// Wrap a pixmap as a paint-mode canvas.
    pub(crate) fn paint(pixmap: &'a mut Pixmap) -> Self {
        Self::Paint(pixmap)
    }

    /// Wrap a recorder as a recording canvas.
    pub(crate) fn record(recorder: &'a mut Recorder) -> Self {
        Self::Record(recorder)
    }

    /// Wrap a subtractive colorant buffer as a paint-mode canvas.
    ///
    /// Engaged by [`crate::render_page`] only when the page group declares
    /// a subtractive blending colour space; every other page keeps the
    /// sRGB path byte for byte.
    pub(crate) fn cmyk(buffer: &'a mut crate::cmyk_buffer::CmykBuffer) -> Self {
        Self::Cmyk(buffer)
    }

    /// The colorant buffer behind this canvas, when there is one.
    ///
    /// The counterpart to [`Canvas::pixmap_mut`] for the four operators
    /// that read their destination back — a shading, an overprint
    /// composite, a per-paint non-separable blend, a shading pattern. Each
    /// asks this first and takes its native subtractive path when the
    /// answer is `Some`; `pixmap_mut` remains the sRGB answer.
    pub(crate) fn cmyk_mut(&mut self) -> Option<&mut crate::cmyk_buffer::CmykBuffer> {
        match self {
            Self::Cmyk(b) => Some(b),
            _ => None,
        }
    }

    /// How many spot colorant planes this canvas's buffer holds, or `0` for
    /// a canvas that has no colorant buffer at all.
    ///
    /// A **read-only** peek, deliberately: its one caller
    /// (`Interpreter::overprint_would_change`) is a predicate taking
    /// `&self`, and giving it a `&mut` handle to satisfy a question about
    /// a count would let a predicate paint.
    pub(crate) fn spot_plane_count(&self) -> usize {
        match self {
            Self::Cmyk(b) => b.spot_plane_count(),
            _ => 0,
        }
    }

    /// Refuse the recording, by name, keeping the first reason.
    ///
    /// A no-op in paint mode: a painter has nothing to refuse. Callers can
    /// therefore call it **unconditionally** at a site that cannot be
    /// recorded, and that is what keeps the refusal impossible to forget —
    /// the alternative shape, `if recording { poison }`, is a branch
    /// somebody eventually writes without the poison in it.
    pub(crate) fn refuse(&mut self, reason: PoisonReason) {
        if let Self::Record(r) = self {
            r.poison(reason);
        }
    }

    /// Whether this canvas is the EXPORT recorder (`Pass 248.3`) — the one
    /// destination that can take a native gradient instead of pixels.
    pub(crate) fn exporting(&self) -> bool {
        matches!(self, Self::Record(r) if r.export.is_some())
    }

    /// Record a shading as a native gradient fill (`Pass 248.3`).
    ///
    /// Export recorder only: returns `false` on every other canvas, and
    /// the site then takes its ordinary route. `path` is in the space
    /// `ctm` maps to device; `spec.transform` maps gradient space to that
    /// same path space.
    pub(crate) fn record_gradient(
        &mut self,
        path: &Path,
        spec: crate::shading::GradientSpec,
        rule: FillRule,
        ctm: Transform,
        clip: ClipRef<'_>,
    ) -> bool {
        let Self::Record(r) = self else {
            return false;
        };
        let Some(export) = r.export.as_mut() else {
            return false;
        };
        export.tally.shadings_as_gradients += 1;
        let brush = BrushSpec {
            brush: Brush::Gradient(Arc::new(spec)),
            blend: BlendMode::SourceOver,
            anti_alias: true,
            cmyk: None,
            spots: Vec::new(),
            process_tints: None,
        };
        r.push_masked(
            Op::Fill {
                bounds: fill_bounds(path, ctm),
                path: Arc::new(path.clone()),
                brush,
                rule,
                ctm,
                clip: clip.id,
            },
            clip.mask,
        );
        true
    }

    /// The EXPORT recorder's scratch, for a site that has just refused
    /// (`Pass 248.1`): paint the operator into it exactly as into a page,
    /// and the recorder harvests the result as an image fill under
    /// `clip`. `None` on every other canvas, including a cache-mode
    /// recorder, so a site can call it unconditionally right after
    /// `refuse` and fall through to its old behaviour on `None`.
    pub(crate) fn export_scratch(&mut self, clip: Option<ClipId>) -> Option<&mut Pixmap> {
        match self {
            Self::Record(r) => r.export_scratch(clip),
            _ => None,
        }
    }

    /// Record a clipping path, when recording.
    ///
    /// Returns the new clip id, or `None` in paint mode — where the caller
    /// builds a real mask instead.
    pub(crate) fn record_clip(&mut self, def: ClipDef) -> Option<ClipId> {
        match self {
            Self::Paint(_) | Self::Knockout(_) | Self::Cmyk(_) => None,
            Self::Record(r) => Some(r.push_clip(def)),
        }
    }

    /// Device width in pixels.
    ///
    /// Load-bearing beyond the obvious: clip masks, soft masks and
    /// overprint coverage buffers are all allocated at exactly this size,
    /// so a canvas that lied about it would produce masks that do not
    /// align with the paints they gate.
    pub(crate) fn width(&self) -> u32 {
        match self {
            Self::Paint(p) => p.width(),
            Self::Record(r) => r.width,
            Self::Knockout(k) => k.accum.width(),
            Self::Cmyk(b) => b.width(),
        }
    }

    /// Device height in pixels. See [`Canvas::width`].
    pub(crate) fn height(&self) -> u32 {
        match self {
            Self::Paint(p) => p.height(),
            Self::Record(r) => r.height,
            Self::Knockout(k) => k.accum.height(),
            Self::Cmyk(b) => b.height(),
        }
    }

    /// Fill `path` (given in the space `ctm` maps to device space).
    pub(crate) fn fill(
        &mut self,
        path: &Path,
        brush: &BrushSpec,
        rule: FillRule,
        ctm: Transform,
        clip: ClipRef<'_>,
    ) {
        match self {
            Self::Paint(p) => p.fill_path(path, &brush.to_paint(), rule, ctm, clip.mask),
            Self::Knockout(k) => {
                let bounds = fill_bounds(path, ctm);
                let (opaque, q_s) = brush.split_shape_and_opacity();
                k.element(bounds, q_s, brush.blend, |scratch| {
                    scratch.fill_path(path, &opaque.to_paint(), rule, ctm, clip.mask);
                });
            }
            Self::Cmyk(b) => {
                paint_solid_into_cmyk(b, path, brush, Some(rule), None, ctm, clip);
            }
            Self::Record(r) => r.push_masked(
                Op::Fill {
                    bounds: fill_bounds(path, ctm),
                    path: Arc::new(path.clone()),
                    brush: brush.clone(),
                    rule,
                    ctm,
                    clip: clip.id,
                },
                clip.mask,
            ),
        }
    }

    /// Stroke `path` with `stroke`, in the space `ctm` maps to device
    /// space.
    pub(crate) fn stroke(
        &mut self,
        path: &Path,
        brush: &BrushSpec,
        stroke: &Stroke,
        ctm: Transform,
        clip: ClipRef<'_>,
    ) {
        match self {
            Self::Paint(p) => p.stroke_path(path, &brush.to_paint(), stroke, ctm, clip.mask),
            Self::Knockout(k) => {
                let bounds = stroke_bounds(path, stroke, ctm);
                let (opaque, q_s) = brush.split_shape_and_opacity();
                k.element(bounds, q_s, brush.blend, |scratch| {
                    scratch.stroke_path(path, &opaque.to_paint(), stroke, ctm, clip.mask);
                });
            }
            Self::Cmyk(b) => {
                paint_solid_into_cmyk(b, path, brush, None, Some(stroke), ctm, clip);
            }
            Self::Record(r) => r.push_masked(
                Op::Stroke {
                    bounds: stroke_bounds(path, stroke, ctm),
                    path: Arc::new(path.clone()),
                    brush: brush.clone(),
                    // One `Arc<Stroke>` per op rather than an interned table.
                    // A CAD sheet sets a line width once and strokes ten
                    // thousand segments with it, so interning is the obvious
                    // win — and is deliberately NOT taken here, because a key
                    // over a float-bearing struct with a dash `Vec` is a
                    // correctness question, and this Pass's budget is spent on
                    // byte-identity. Named as a follow-on rather than left as a
                    // silent inefficiency.
                    stroke: Arc::new(stroke.clone()),
                    ctm,
                    clip: clip.id,
                },
                clip.mask,
            ),
        }
    }

    /// Fill `path` with an **image**, sampled through `tiny_skia`'s
    /// pattern shader over §8.9.4's unit square.
    ///
    /// # Why images do not go through [`Canvas::fill`]
    ///
    /// Because [`Brush::Image`] owns its texels (`Arc<Pixmap>`) and the
    /// interpreter does not — it holds a freshly decoded `Pixmap` by
    /// reference. Building a `BrushSpec` before the call would therefore
    /// copy the whole decoded raster **on every image paint, in paint
    /// mode, where nothing ever reads the copy**.
    ///
    /// Taking the borrow here moves that copy inside the recording branch,
    /// which is the only branch that needs an owned image. Paint mode
    /// builds the shader straight off the borrow, exactly as the inline
    /// code it replaced did.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fill_image(
        &mut self,
        path: &Path,
        texels: &Pixmap,
        ink: Option<&crate::image::CmykTexels>,
        // The image's colour AS AUTHORED — process tints plus one plane per
        // spot colorant — when the interpreter wants the spots deposited
        // (`Pass 238.0`). `None` under the composite device model, and for
        // every image whose space names no spot. Taken in preference to
        // `ink` when EVERY spot gets a plane; see the Cmyk arm.
        authored: Option<&crate::image::OverprintSource>,
        // What to do to the spot planes this image does NOT name. Only a
        // colorant buffer reads it; every other destination has no planes.
        spot_source: crate::cmyk_buffer::SpotSource,
        quality: FilterQuality,
        image_to_user: Transform,
        blend: BlendMode,
        anti_alias: bool,
        ctm: Transform,
        clip: ClipRef<'_>,
    ) {
        match self {
            Self::Record(r) => {
                r.push_masked(
                    Op::Fill {
                        bounds: fill_bounds(path, ctm),
                        path: Arc::new(path.clone()),
                        brush: BrushSpec {
                            // HERE is the copy this method's borrow exists to
                            // defer — paid once, in the only branch that will
                            // ever read it.
                            brush: Brush::Image {
                                texels: Arc::new(texels.clone()),
                                quality,
                                transform: image_to_user,
                            },
                            blend,
                            anti_alias,
                            // A recorded image brush is replayed into a
                            // `Pixmap`, never into a colorant buffer -- a
                            // display list is refused outright on a subtractive
                            // page. See `PoisonReason::ColorantBuffer`.
                            cmyk: None,
                            // Same reason, and doubly so: an image brush states
                            // no colorants at all, spot or process.
                            spots: Vec::new(),
                            process_tints: None,
                        },
                        rule: FillRule::Winding,
                        ctm,
                        clip: clip.id,
                    },
                    clip.mask,
                );
            }
            Self::Paint(p) => {
                let paint = Paint {
                    shader: Pattern::new(
                        texels.as_ref(),
                        SpreadMode::Pad,
                        quality,
                        1.0,
                        image_to_user,
                    ),
                    blend_mode: blend,
                    anti_alias,
                    force_hq_pipeline: false,
                };
                p.fill_path(path, &paint, FillRule::Winding, ctm, clip.mask);
            }
            Self::Cmyk(b) => {
                // ★★ THIS USED TO BE "THE ONE PAINT KIND THAT CANNOT GO
                // NATIVE", and the comment explaining why is worth keeping
                // because it was exactly right about the cause:
                //
                //   "the reason is upstream of this method: `DecodedImage`
                //    holds a `Pixmap`, so a `DeviceCMYK` image's samples were
                //    already flattened to sRGB inside the decode loop, one
                //    call before this one. Bridging here is therefore not a
                //    shortcut taken at the canvas -- it is the only
                //    information that reaches the canvas."
                //
                // The fix was therefore upstream too: `DecodedImage::ink`
                // now carries the authored colorants for a `DeviceCMYK`
                // image, so the information DOES reach the canvas and the
                // round trip is unnecessary. See `CmykBuffer::
                // composite_cmyk_image` for why no better inverse would have
                // served -- `CMYK -> sRGB` is many-to-one.
                //
                // The bridge below still runs for every other image, where
                // sRGB genuinely is all there is.
                //
                // ★★ THE SPOT ROUTE (`Pass 238.0`), taken before the ink
                // route because it is the same information one level less
                // flattened. `authored.tints` are the process tints the
                // FILE stated — all zero for a pure spot — and each
                // `authored.spots` plane is one colorant's tint. If every
                // colorant gets a plane, those are rasterised through the
                // identical shader and composited together; if any is
                // refused (roster cap, byte ceiling), the whole image falls
                // through to `ink`, which is the tint transform's output
                // and already contains every spot as process ink. Never
                // both: that lays the ink down twice, the defect the fill
                // path's agreement tests caught on day one.
                if let Some(src) = authored
                    && !src.spots.is_empty()
                {
                    let mut planes: Vec<usize> = Vec::with_capacity(src.spots.len());
                    for spot in &src.spots {
                        match b.spot_index(&spot.colorant, || (*spot.lut).clone()) {
                            Some(plane) => planes.push(plane),
                            None => break,
                        }
                    }
                    if planes.len() == src.spots.len()
                        && let (Some(mut cmy), Some(mut k)) = (
                            Pixmap::new(b.width(), b.height()),
                            Pixmap::new(b.width(), b.height()),
                        )
                    {
                        let draw = |src_px: &Pixmap, dst: &mut Pixmap| {
                            let paint = Paint {
                                shader: Pattern::new(
                                    src_px.as_ref(),
                                    SpreadMode::Pad,
                                    quality,
                                    1.0,
                                    image_to_user,
                                ),
                                blend_mode: BlendMode::SourceOver,
                                anti_alias,
                                force_hq_pipeline: false,
                            };
                            dst.fill_path(path, &paint, FillRule::Winding, ctm, clip.mask);
                        };
                        draw(&src.tints.cmy, &mut cmy);
                        draw(&src.tints.k, &mut k);
                        let mut spot_pix: Vec<Pixmap> = Vec::with_capacity(src.spots.len());
                        let mut all = true;
                        for spot in &src.spots {
                            match Pixmap::new(b.width(), b.height()) {
                                Some(mut px) => {
                                    draw(&spot.tint, &mut px);
                                    spot_pix.push(px);
                                }
                                None => {
                                    all = false;
                                    break;
                                }
                            }
                        }
                        if all {
                            let pairs: Vec<(usize, &Pixmap)> =
                                planes.iter().copied().zip(spot_pix.iter()).collect();
                            if let Some(region) =
                                device_region(fill_bounds(path, ctm), 1.0, b.width(), b.height())
                            {
                                b.composite_cmyk_image(
                                    &cmy,
                                    &k,
                                    &pairs,
                                    region,
                                    1.0,
                                    crate::compositor::Blend::from_tiny_skia(blend)
                                        .unwrap_or(crate::compositor::Blend::Normal),
                                    spot_source,
                                );
                            }
                            return;
                        }
                    }
                }
                if let Some(ink) = ink
                    && let (Some(mut cmy), Some(mut k)) = (
                        Pixmap::new(b.width(), b.height()),
                        Pixmap::new(b.width(), b.height()),
                    )
                {
                    // Rasterised TWICE through the identical shader, transform
                    // and path, so the ink lands on exactly the pixels the
                    // sRGB path would have covered. Any difference in
                    // interpolation or edge coverage between the two would
                    // show up as a fringe of the wrong colour.
                    let draw = |src: &Pixmap, dst: &mut Pixmap| {
                        let paint = Paint {
                            shader: Pattern::new(
                                src.as_ref(),
                                SpreadMode::Pad,
                                quality,
                                1.0,
                                image_to_user,
                            ),
                            blend_mode: BlendMode::SourceOver,
                            anti_alias,
                            force_hq_pipeline: false,
                        };
                        dst.fill_path(path, &paint, FillRule::Winding, ctm, clip.mask);
                    };
                    draw(&ink.cmy, &mut cmy);
                    draw(&ink.k, &mut k);
                    if let Some(region) =
                        device_region(fill_bounds(path, ctm), 1.0, b.width(), b.height())
                    {
                        b.composite_cmyk_image(
                            &cmy,
                            &k,
                            &[],
                            region,
                            1.0,
                            crate::compositor::Blend::from_tiny_skia(blend)
                                .unwrap_or(crate::compositor::Blend::Normal),
                            spot_source,
                        );
                    }
                    return;
                }
                // Every other image: sRGB is the only information there is.
                //
                // The scratch is rasterised with `tiny_skia` exactly as the
                // `Paint` arm above does, so an image edge has identical
                // geometry on both paths; only the composite differs.
                // `SourceOver` into a transparent scratch, then the real
                // blend on the way into the buffer -- letting `tiny_skia`
                // apply the blend would blend against the scratch, which is
                // empty, and silently reduce every mode to `Normal`.
                if let Some(mut scratch) = Pixmap::new(b.width(), b.height()) {
                    let paint = Paint {
                        shader: Pattern::new(
                            texels.as_ref(),
                            SpreadMode::Pad,
                            quality,
                            1.0,
                            image_to_user,
                        ),
                        blend_mode: BlendMode::SourceOver,
                        anti_alias,
                        force_hq_pipeline: false,
                    };
                    scratch.fill_path(path, &paint, FillRule::Winding, ctm, clip.mask);
                    let region = device_region(fill_bounds(path, ctm), 1.0, b.width(), b.height());
                    if let Some(region) = region {
                        b.composite_srgb_with(
                            &scratch,
                            region,
                            1.0,
                            crate::compositor::Blend::from_tiny_skia(blend)
                                .unwrap_or(crate::compositor::Blend::Normal),
                            spot_source,
                        );
                    }
                }
            }
            Self::Knockout(k) => {
                // An image carries its own per-sample alpha, and that alpha
                // is SHAPE here, not opacity: §11.6.4.2 makes an image's
                // `/SMask` an object-shape input unless `/AIS` says
                // otherwise. So `q_s = 1` and the scratch's alpha channel
                // is `f_s` directly.
                let bounds = fill_bounds(path, ctm);
                k.element(bounds, 1.0, blend, |scratch| {
                    let paint = Paint {
                        shader: Pattern::new(
                            texels.as_ref(),
                            SpreadMode::Pad,
                            quality,
                            1.0,
                            image_to_user,
                        ),
                        blend_mode: blend,
                        anti_alias,
                        force_hq_pipeline: false,
                    };
                    scratch.fill_path(path, &paint, FillRule::Winding, ctm, clip.mask);
                });
            }
        }
    }

    /// Paint an image through **§11.7.4.3's `CompatibleOverprint`** —
    /// Table 149 applied per colour component, with the source tints
    /// arriving per *sample* rather than per paint.
    ///
    /// # Why an image needs its own entry point at all
    ///
    /// [`Interpreter::paint_overprint`](crate::interpret) covers paths and
    /// glyphs, and it composites **one** source colour through a coverage
    /// mask. An image has a different colour at every texel, so the source
    /// cannot be a `[f32; 4]`. What it *can* be — and this is the whole
    /// reason the operation is affordable — is a second and third
    /// rasterisation of the **same** shape through the **same** shader, one
    /// carrying `C, M, Y` and one carrying `K`, exactly as
    /// [`Canvas::fill_image`]'s ink path already does for a `DeviceCMYK`
    /// image. Identical transform, identical filter quality, identical edge
    /// coverage; only the values sampled differ. Reconstructing the mapping
    /// by inverting the CTM and sampling per device pixel would be a second
    /// implementation of the resampling and would disagree with the first at
    /// every edge.
    ///
    /// `rules` are computed **once by the caller**, from the image's colour
    /// space. That is Table 149's own shape, not a shortcut: its
    /// `Separation`/`DeviceN` row selects on **which colorants the space
    /// names**, never on their tints, and one image has one colour space.
    ///
    /// # Returns
    ///
    /// `Some(changed_pixels)` when the composite ran; **`None` when this
    /// canvas cannot read its destination back**, which a recording canvas
    /// never can. The caller's documented response to `None` is to paint the
    /// image normally **and disclose the shortfall** — never to paint
    /// nothing, and never to paint normally in silence (rule 4).
    ///
    /// A `Some(0)` is a success, not a fallback: it means the composite ran
    /// and turned out to change nothing, which is a different fact from
    /// "overprint was not applied" and is counted as such.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fill_image_overprint(
        &mut self,
        path: &Path,
        tints: &crate::image::CmykTexels,
        // The image's spot planes (`Pass 238.0`), each already resolved to
        // a plane index by the interpreter — empty when the image names no
        // spot, or when any of them was refused a plane. Under overprint a
        // plane the source names takes the source's tint and every other
        // plane keeps the backdrop, which is Table 149's `Separation` /
        // `DeviceN` row applied to the planes it was written for.
        spots: &[(usize, &crate::image::SpotTexel)],
        rules: [crate::overprint::ComponentRule; 4],
        quality: FilterQuality,
        image_to_user: Transform,
        anti_alias: bool,
        ctm: Transform,
        clip: ClipRef<'_>,
        alpha: f32,
    ) -> Option<u32> {
        // A recording canvas has no destination to read; poison it by name
        // rather than dropping the effect silently. Checked FIRST so the two
        // scratch allocations below are not paid for a canvas that cannot
        // use them.
        if matches!(self, Self::Record(_)) {
            self.refuse(PoisonReason::Overprint);
            return None;
        }
        let (w, h) = (self.width(), self.height());
        let (Some(mut cmy), Some(mut k)) = (Pixmap::new(w, h), Pixmap::new(w, h)) else {
            return None;
        };
        let mut spot_pix: Vec<(usize, Pixmap)> = Vec::with_capacity(spots.len());
        for (plane, _) in spots {
            spot_pix.push((*plane, Pixmap::new(w, h)?));
        }
        // The identical shader/transform/quality/clip pair `fill_image`'s ink
        // branch uses. `SourceOver` into a transparent scratch: the blend
        // mode belongs to the composite below, and letting `tiny_skia` apply
        // it here would blend against an empty scratch and silently reduce
        // every mode to Normal.
        {
            let draw = |src: &Pixmap, dst: &mut Pixmap| {
                let paint = Paint {
                    shader: Pattern::new(
                        src.as_ref(),
                        SpreadMode::Pad,
                        quality,
                        1.0,
                        image_to_user,
                    ),
                    blend_mode: BlendMode::SourceOver,
                    anti_alias,
                    force_hq_pipeline: false,
                };
                dst.fill_path(path, &paint, FillRule::Winding, ctm, clip.mask);
            };
            draw(&tints.cmy, &mut cmy);
            draw(&tints.k, &mut k);
            for ((_, spot), (_, dst)) in spots.iter().zip(spot_pix.iter_mut()) {
                draw(&spot.tint, dst);
            }
        }
        let region = device_region(fill_bounds(path, ctm), 1.0, w, h)?;
        // Un-premultiply by the alpha both planes were multiplied by — the
        // same dance `composite_cmyk_image` performs, and the reason
        // `write_ink` premultiplies in the first place. The alpha IS the
        // sample's coverage: §11.6.4.2 makes an image's own alpha *shape*,
        // and the constant `ca` is opacity, so the two multiply rather than
        // one standing in for the other.
        let source_at = |x: u32, y: u32| {
            let idx = (y * w + x) as usize;
            let (cm, kk) = (cmy.pixels().get(idx)?, k.pixels().get(idx)?);
            let a = f32::from(cm.alpha()) / 255.0;
            if a <= 0.0 {
                return None;
            }
            let un = |v: u8| f32::from(v) / 255.0 / a;
            Some((
                [un(cm.red()), un(cm.green()), un(cm.blue()), un(kk.red())],
                a,
            ))
        };
        if let Some(b) = self.cmyk_mut() {
            if spot_pix.is_empty() {
                return Some(b.composite_overprint_varying(region, rules, alpha, source_at));
            }
            let spot_at = |x: u32, y: u32| {
                let idx = (y * w + x) as usize;
                let mut out: [Option<f32>; crate::compositor::MAX_SPOTS] =
                    [None; crate::compositor::MAX_SPOTS];
                for (plane, px) in &spot_pix {
                    if let (Some(slot), Some(p)) = (out.get_mut(*plane), px.pixels().get(idx)) {
                        let a = f32::from(p.alpha()) / 255.0;
                        *slot = (a > 0.0).then(|| f32::from(p.red()) / 255.0 / a);
                    }
                }
                out
            };
            return Some(
                b.composite_overprint_varying_spots(region, rules, alpha, |x, y| {
                    source_at(x, y).map(|(c, a)| (c, spot_at(x, y), a))
                }),
            );
        }
        // Every other destination reads back as sRGB. `pixmap_mut` handles
        // the knockout accumulator's disclosure itself and answers `None`
        // for anything that cannot read back at all.
        let dest = self.pixmap_mut()?;
        Some(crate::overprint::composite_varying(
            dest, rules, alpha, region, source_at,
        ))
    }

    /// The raw destination buffer, for the operations that **read pixels
    /// back** and therefore cannot be expressed as a recorded draw.
    ///
    /// Exactly two callers: `paint_overprint` (§11.7.4.3 composites
    /// against the destination's own colorants) and the soft-mask path.
    /// Both are destination-dependent by definition — there is no "record
    /// this and replay it later" formulation of *"read what is already
    /// there"*, because what is already there depends on the viewport.
    ///
    /// A recording canvas returns `None` here, and the caller's documented
    /// job is then to **poison the recording by name** rather than to
    /// quietly skip the effect. Returning `None` and letting a caller
    /// treat it as "nothing to do" would be precisely the silent
    /// wrongness rule 4 forbids.
    pub(crate) fn pixmap_mut(&mut self) -> Option<&mut Pixmap> {
        match self {
            Self::Paint(p) => Some(p),
            Self::Record(_) => None,
            // ★ A DISCLOSED SHORTFALL, not an oversight. The three callers
            // here read the destination back — a shading, an overprint
            // composite, a non-separable per-paint blend — and there is no
            // formulation of "read what is already there" that also yields
            // the element's own shape in isolation. Handing over the
            // accumulator lets those operators run and paint the right
            // marks; what they lose is the knockout semantics for that one
            // element, which then layers instead of knocking out.
            //
            // Counted by the caller as `knockout_approximated`, and wrong
            // in a bounded, nameable way: it is the answer a NON-knockout
            // group would have given, which is also the answer every
            // element gives when its opacity is 1 (§11.4.6 — the two
            // recurrences coincide at `q_s = 1`).
            Self::Knockout(k) => {
                k.approximated += 1;
                Some(&mut k.accum)
            }
            // A colorant buffer has no sRGB pixmap to hand out, and
            // fabricating one would defeat the entire point of the buffer.
            // Every caller of this method also calls
            // [`Canvas::cmyk_mut`] first and takes a native subtractive
            // path, so reaching here with a `Cmyk` canvas means a NEW
            // destination-reading operator was added without one -- which
            // the caller reports as a refusal rather than painting wrongly.
            Self::Cmyk(_) => None,
        }
    }

    /// Draw a sub-drawing into its own buffer and composite the result as
    /// **one object**.
    ///
    /// `f` is handed a nested canvas of the same device size. The nested
    /// drawing starts fully **transparent** — which is §11.4.7's isolated
    /// backdrop, and is also what an annotation's `/CA` composite needs
    /// (a white scratch would composite an opaque rectangle over the
    /// page).
    ///
    /// # Returns
    ///
    /// `Some(f(..))` when the layer ran. `None` when the layer **could not
    /// be started at all**, in which case `f` was *never called* and the
    /// caller must decide what to do instead — `do_form` falls back to
    /// painting inline and counts the group as flattened; an annotation's
    /// `/CA` path reports a degenerate placement and paints nothing. Those
    /// two fallbacks differ, which is exactly why this returns "did not
    /// start" rather than performing a fallback of its own choosing.
    pub(crate) fn layer<R>(
        &mut self,
        paint: LayerPaint,
        f: impl FnOnce(&mut Canvas<'_>) -> R,
    ) -> Option<R> {
        match self {
            Self::Knockout(k) => {
                // A child of a knockout group is ONE element, and its own
                // buffer is where the recursion bottoms out. Rendering it
                // into a transparent buffer is exact whenever its interior
                // composites `Normal` (§11.4.4 NOTE 5) — which is what an
                // annotation's `/CA` layer always is, and what a group
                // element usually is — and its result is then accumulated
                // by §11.4.8's own formula rather than painted over.
                let mut buf = Pixmap::new(k.accum.width(), k.accum.height())?;
                let result = {
                    let mut sub = Canvas::Paint(&mut buf);
                    f(&mut sub)
                };
                let blend = layer_blend(paint);
                k.element_from_pixmap(&buf, paint.opacity.clamp(0.0, 1.0), blend);
                Some(result)
            }
            Self::Cmyk(b) => {
                // A layer on a subtractive page gets a subtractive layer.
                // No conversion happens at this boundary in either
                // direction, which is the point: content inside a layer
                // and identical content outside it must reach the page as
                // the same ink.
                let mut child = b.take_child()?;
                let result = {
                    let mut sub = Canvas::Cmyk(&mut child);
                    f(&mut sub)
                };
                let blend = layer_blend(paint);
                b.composite_buffer(&child, paint.opacity.clamp(0.0, 1.0), blend);
                b.give_back_child(child);
                Some(result)
            }
            Self::Paint(p) => {
                // Same size as the parent, deliberately, and not the
                // sub-drawing's bounding box: the contents are drawn under
                // the SAME CTM as the parent, so a smaller buffer would
                // need a translation threaded through every paint site and
                // every clip mask. Page-sized costs ~4 bytes per pixel per
                // nesting level and needs no coordinate change at all.
                let mut buf = Pixmap::new(p.width(), p.height())?;
                let result = {
                    let mut sub = Canvas::Paint(&mut buf);
                    f(&mut sub)
                };
                if let Some(mode) = paint.nonseparable {
                    // ★ §11.4.5's composite, through Table 137, per pixel.
                    //
                    // `draw_pixmap` cannot carry these four — that is the
                    // whole of decision 066 — so the group's RESULT is
                    // blended here instead. This is the case suite's
                    // `Transp_Basic_BM` patches are made of: every one of
                    // their non-separable modes sits at a group `Do`, not at
                    // a paint, so without this the feature reaches no suite
                    // pixel at all.
                    crate::blend_nonsep::composite_layer(
                        p,
                        &buf,
                        mode,
                        paint.opacity.clamp(0.0, 1.0),
                    );
                } else {
                    p.draw_pixmap(
                        0,
                        0,
                        buf.as_ref(),
                        &PixmapPaint {
                            opacity: paint.opacity.clamp(0.0, 1.0),
                            blend_mode: paint.blend,
                            quality: FilterQuality::Nearest,
                        },
                        Transform::identity(),
                        // No mask: the contents were already clipped while
                        // being drawn, so re-applying the clip here would
                        // double-multiply its anti-aliased edge and darken
                        // every clipped boundary by one pass.
                        None,
                    );
                }
                Some(result)
            }
            Self::Record(r) => {
                // A recorded layer is a frame on the op stack: everything
                // `f` draws lands in it, and popping turns it into one
                // `Op::Layer` in the parent. No buffer is allocated,
                // because no pixels exist yet — which is also why this
                // branch cannot fail the way the paint branch can.
                // Export mode: anything painted into the scratch before
                // this layer belongs to the PARENT frame, and anything
                // painted inside it belongs to the layer -- so the
                // scratch is harvested at both boundaries.
                r.harvest();
                r.frames.push(Vec::new());
                let result = {
                    let mut sub = Canvas::Record(r);
                    f(&mut sub)
                };
                r.harvest();
                let ops = r.frames.pop().unwrap_or_default();
                r.push(Op::Layer {
                    paint,
                    ops,
                    mask: None,
                });
                Some(result)
            }
        }
    }

    /// Draw a **transparency group** and composite its result per
    /// ISO 32000-1 §11.4.4 — including the initial backdrop a
    /// **non-isolated** group's elements are entitled to see.
    ///
    /// # Why this is not [`Canvas::layer`]
    ///
    /// [`Canvas::layer`] models *"composite this sub-drawing as one
    /// object"*, which is exactly right for an annotation's `/CA` and for
    /// an **isolated** group — both start from a fully transparent buffer,
    /// because that is what §11.4.5 says an isolated group's initial
    /// backdrop *is*.
    ///
    /// A **non-isolated** group is a different computation, and §11.4.4
    /// NOTE 2 gives the reason in one sentence: *"The elements of a group
    /// are composited onto a backdrop that includes the group's initial
    /// backdrop. This is done to achieve the correct effects of the blend
    /// modes, most of which are dependent on both the backdrop and source
    /// colours being blended."* Painting such a group into a transparent
    /// buffer hands every interior blend a backdrop of **nothing**, and
    /// `B(nothing, C_s)` degenerates to `C_s` — which is why suite's
    /// `PCS3_161` renders as a grid of saturated primaries.
    ///
    /// # The two runs, and why the second is conditional
    ///
    /// The standard's model needs two per-pixel quantities the group's own
    /// buffer cannot both hold (`iso32000__s__11.4.md` §8): `C_n`, the
    /// colour accumulated **over** the backdrop, and `α_gn`, the group's
    /// own alpha **excluding** it. A `tiny_skia::Pixmap` has one alpha
    /// channel.
    ///
    /// So the contents are run twice:
    ///
    /// | run | initial buffer | what it yields |
    /// |---|---|---|
    /// | 1 | transparent | `α_gn` — and `C` itself, when it is exact |
    /// | 2 | a copy of the backdrop | `C_n` |
    ///
    /// **Run 2 is skipped whenever run 1's answer is already exact**, and
    /// that is not a heuristic — it is §11.4.4 NOTE 5's own condition. With
    /// every interior element compositing `Normal`, the backdrop's
    /// contribution to `C_n` is precisely what backdrop removal takes back
    /// out, so `C = C_iso` identically. The closure therefore reports
    /// whether its contents blended against anything, and only a group that
    /// did pays for a second walk. On ordinary documents — where a `/BM`
    /// other than `/Normal` is rare — this costs nothing at all.
    ///
    /// It is also skipped when the backdrop is **empty**: `α_0 = 0`
    /// everywhere makes the group isolated by §11.4.5's own substitution,
    /// with no branch needed.
    ///
    /// # Arguments
    ///
    /// * `paint` — the outer state at the `Do`: constant alpha, blend mode,
    ///   and a non-separable mode if the state carries one. §11.4.5: these
    ///   apply to the group's **result**, never to its contents.
    /// * `isolated` — the group's `/I` flag (Table 147). Expressed
    ///   downstream entirely as `α_0 = 0`, which is the whole of the
    ///   normative change §11.4.5 makes.
    /// * `f` — runs the group's content stream into the canvas it is
    ///   handed, and returns `(result, backdrop_dependent)`. It may be
    ///   called **twice**; the second call's `result` is discarded, so a
    ///   caller accumulating diagnostics must merge only the returned one.
    ///
    /// # Returns
    ///
    /// `None` when the buffer could not be allocated at all, with the same
    /// contract [`Canvas::layer`] has: `f` was never called and the caller
    /// decides what to do instead.
    pub(crate) fn group<R>(
        &mut self,
        paint: LayerPaint,
        isolated: bool,
        knockout: bool,
        mask: Option<&Mask>,
        mut f: impl FnMut(&mut Canvas<'_>) -> (R, bool),
    ) -> Option<GroupOutcome<R>> {
        // §11.4.6 is orthogonal to §11.4.5 — *"isolated and knockout are
        // independent attributes"* — so knockout is dispatched first and
        // `isolated` is carried into it as the initial backdrop's identity
        // rather than as a second branch.
        // ★ A SUBTRACTIVE CANVAS TAKES ITS OWN KNOCKOUT PATH, and the
        // history of this two-line dispatch is worth keeping.
        //
        // `Pass 97.1e` first sent `Cmyk` to the ordinary bridged arm below,
        // which cost `PCS1_161` -- the suite's KNOCKOUT patch -- eleven of
        // the thirteen traps `Pass 97.0c` had just removed, by silently
        // substituting non-knockout semantics on exactly the pages that
        // test them. Sending it to `KnockoutTarget` instead recovered most
        // of that but not all: the accumulation ran in sRGB, so the group's
        // interior blended additively inside a page that did not, and the
        // patch sat at 4 traps against a pre-Pass baseline of 2.
        //
        // `Pass 97.1f` gives it a native subtractive accumulator, so
        // §11.4.6's semantics and §11.3.4's space hold at the same time and
        // neither is traded for the other.
        if knockout && matches!(self, Self::Cmyk(_)) {
            return self.knockout_group_cmyk(paint, isolated, mask, f);
        }
        if knockout && !matches!(self, Self::Record(_)) {
            return self.knockout_group(paint, isolated, mask, f);
        }
        match self {
            Self::Cmyk(b) => {
                // ★★ THE SECOND CONTENT WALK, `Pass 97.1g`.
                //
                // This arm used to treat EVERY group here as isolated and
                // say so: *"there is no way to hand one to the other
                // without the very round trip the colorant buffer exists
                // to delete."* That sentence was reasoning about the
                // SRGB-interior era, when a group's child was a `Pixmap`
                // and the parent was ink, so handing the backdrop down
                // meant converting it. `Pass 97.1f` gave the child a
                // native colorant buffer, and from that commit onwards the
                // parent and the child hold the same four planes in the
                // same space -- so the backdrop can simply be COPIED, and
                // the obstacle the comment described had already stopped
                // existing. It went on being quoted for three Passes.
                //
                // Worth keeping as a shape rather than an anecdote: a
                // comment justifying an approximation is not re-read when
                // the thing it blames is removed, because nothing links
                // them. The approximation outlived its own reason.
                //
                // Structure below is a deliberate PORT of the `Paint` arm,
                // line for line, so the two paths stay legibly the same
                // computation in two spaces (§11.4.4 is one clause, not two).
                let blend = layer_blend(paint);
                let opacity = paint.opacity.clamp(0.0, 1.0);

                // Run 1: transparent start. Its alpha is `α_gn`.
                let mut iso = b.take_child()?;
                let (result, backdrop_dependent) = {
                    let mut sub = Canvas::Cmyk(&mut iso);
                    f(&mut sub)
                };
                // The group's own bridge tallies ride along: a child
                // buffer's bridged pixels and bridged sub-groups are the
                // parent page's too, and dropping them here would make a
                // page composited entirely out of bridged images report
                // zero bridging.
                b.absorb_counters(&iso);

                // §11.4.5's substitution, applied as a test rather than a
                // branch -- the same three-way test the `Paint` arm uses,
                // and for the same reasons. A backdrop that is transparent
                // everywhere IS an isolated group's backdrop, so there is
                // nothing to run twice and nothing to remove.
                let backdrop_present = b.backdrop_present();
                if isolated || !backdrop_dependent || !backdrop_present {
                    if let Some(m) = mask {
                        iso.apply_mask(m);
                    }
                    b.composite_buffer(&iso, opacity, blend);
                    b.give_back_child(iso);
                    return Some(GroupOutcome {
                        result,
                        backdrop_rerun: false,
                        // A knockout group reaching here has lost its
                        // knockout semantics along with its blending
                        // space; reported through the channel that already
                        // means exactly that.
                        knockout_approximated: usize::from(knockout),
                    });
                }

                // Run 2: the same content stream over the group's own
                // initial backdrop. `b` is untouched until the merge
                // below, so it still holds the frozen backdrop both the
                // copy and the removal need.
                //
                // ★ ALLOCATION FAILURE FALLS BACK, IT DOES NOT DROP THE
                // GROUP. `child_from_backdrop` returns `None` on the same
                // condition `take_child` does, and a page that cannot
                // afford one more buffer should still get the isolated
                // approximation it used to get -- counted, so the
                // disclosure stays honest about which one it got.
                let Some(mut nis) = b.child_from_backdrop() else {
                    b.note_group_approximated();
                    if let Some(m) = mask {
                        iso.apply_mask(m);
                    }
                    b.composite_buffer(&iso, opacity, blend);
                    b.give_back_child(iso);
                    return Some(GroupOutcome {
                        result,
                        backdrop_rerun: false,
                        knockout_approximated: usize::from(knockout),
                    });
                };
                {
                    let mut sub = Canvas::Cmyk(&mut nis);
                    let _ = f(&mut sub);
                }
                // Run 2's counters are DELIBERATELY NOT absorbed. They are
                // the same content walked a second time, and adding them
                // would double every bridged-pixel and sub-group tally on
                // exactly the pages this Pass improves -- a disclosure
                // number that gets worse because the renderer got better
                // is worse than no number.
                //
                // The mask is passed INTO the merge rather than applied to
                // a buffer first: §11.4.4's removal divides by the UNMASKED
                // `α_gn`. See `composite_non_isolated`.
                b.composite_non_isolated(&iso, &nis, opacity, blend, mask.map(Mask::data));
                // ★ ONE buffer is handed back, not two, and it is the
                // cheaper one to clear. `give_back_child` keeps a SINGLE
                // spare, so returning both would clear `nis` -- whose dirty
                // rectangle spans the whole backdrop it was seeded from --
                // and then immediately drop it to make room for `iso`,
                // whose rectangle is only the group's own marks. Paying the
                // larger clear for a buffer about to be freed is pure loss.
                drop(nis);
                b.give_back_child(iso);
                Some(GroupOutcome {
                    result,
                    backdrop_rerun: true,
                    knockout_approximated: usize::from(knockout),
                })
            }
            Self::Paint(p) => {
                let mut iso = Pixmap::new(p.width(), p.height())?;
                let (result, backdrop_dependent) = {
                    let mut sub = Canvas::Paint(&mut iso);
                    f(&mut sub)
                };
                // §11.4.5's substitution, applied as a test rather than a
                // branch: a backdrop that is transparent everywhere IS an
                // isolated group's backdrop, so there is nothing to run
                // twice and nothing to remove.
                let backdrop_present = p.pixels().iter().any(|px| px.alpha() > 0);
                if isolated || !backdrop_dependent || !backdrop_present {
                    composite_group_result(p, &iso, paint, mask);
                    return Some(GroupOutcome {
                        result,
                        backdrop_rerun: false,
                        knockout_approximated: 0,
                    });
                }
                // Run 2: the same content stream, over the group's own
                // initial backdrop. `p` is untouched until the composite
                // below, so it is the frozen backdrop the removal needs.
                let mut nis = (*p).clone();
                {
                    let mut sub = Canvas::Paint(&mut nis);
                    let _ = f(&mut sub);
                }
                composite_non_isolated_group(p, &iso, &nis, paint, mask);
                Some(GroupOutcome {
                    result,
                    backdrop_rerun: true,
                    knockout_approximated: 0,
                })
            }
            Self::Knockout(k) => {
                // ★ §11.4.6 NOTE 6 / §11.6.6 — THE NESTING TRAP, and it is
                // the one an implementation reaches for the wrong buffer
                // on: *"When a non-isolated group is nested within a
                // knockout group, the initial backdrop of the inner group
                // is the same as that of the outer group; it is not the
                // immediate backdrop of the inner group."*
                //
                // So the child is handed `initial`, NOT `accum`. Handing
                // it the accumulator is what a "just pass the current
                // buffer down" implementation does, and it is wrong in a
                // way that only shows where a knockout group has more than
                // one overlapping child — i.e. exactly where knockout is
                // the feature under test.
                let mut iso = Pixmap::new(k.accum.width(), k.accum.height())?;
                let (result, backdrop_dependent) = {
                    let mut sub = Canvas::Paint(&mut iso);
                    f(&mut sub)
                };
                if let Some(m) = mask {
                    apply_mask(&mut iso, m);
                }
                let blend = layer_blend(paint);
                let opacity = paint.opacity.clamp(0.0, 1.0);
                let initial_present = k.initial.pixels().iter().any(|px| px.alpha() > 0);
                if isolated || !backdrop_dependent || !initial_present {
                    k.element_from_pixmap(&iso, opacity, blend);
                    return Some(GroupOutcome {
                        result,
                        backdrop_rerun: false,
                        knockout_approximated: 0,
                    });
                }
                let mut nis = k.initial.clone();
                {
                    let mut sub = Canvas::Paint(&mut nis);
                    let _ = f(&mut sub);
                }
                k.element_from_non_isolated(&iso, &nis, opacity, blend);
                Some(GroupOutcome {
                    result,
                    backdrop_rerun: true,
                    knockout_approximated: 0,
                })
            }
            Self::Record(r) => {
                r.harvest();
                r.frames.push(Vec::new());
                let (result, backdrop_dependent) = {
                    let mut sub = Canvas::Record(r);
                    f(&mut sub)
                };
                r.harvest();
                let ops = r.frames.pop().unwrap_or_default();
                // §11.4.5's mask is a device-sized buffer built for
                // THIS viewport. In CACHE mode a replay at another scale
                // would apply it at the wrong resolution, so the page is
                // refused by name rather than replayed as a plausible
                // wrong picture. In EXPORT mode (`Pass 248.1`) the
                // recording is consumed at exactly this scale, so the
                // mask is valid and travels WITH the layer; `poison`
                // there only counts it (`ExportTally::soft_masks_kept`).
                let kept_mask = match mask {
                    Some(m) if r.export.is_some() => Some(Arc::new(m.clone())),
                    _ => None,
                };
                r.push(Op::Layer {
                    paint,
                    ops,
                    mask: kept_mask,
                });
                // Cache mode only: export mode counted the mask at the `gs`
                // site (`ExportTally::soft_masks_kept`), where every soft
                // mask is seen once whether a group or an object wears it.
                if mask.is_some() && r.export.is_none() {
                    r.poison(PoisonReason::SoftMask);
                }
                if !isolated && backdrop_dependent {
                    // Replay would give the isolated approximation. Refuse
                    // the recording by name rather than keep a plausible
                    // wrong one — the same contract every other
                    // destination-reading operator here has.
                    r.poison(PoisonReason::NonIsolatedGroup);
                }
                Some(GroupOutcome {
                    result,
                    backdrop_rerun: false,
                    knockout_approximated: 0,
                })
            }
        }
    }
}

impl Canvas<'_> {
    /// Render a **knockout group** (§11.4.6) and composite its result.
    ///
    /// # Why this is a separate entry point rather than a flag inside
    /// [`Canvas::group`]
    ///
    /// Because the two differ in *where the elements are composited*, not
    /// in how the result is composited afterwards. An ordinary group's
    /// elements go into one buffer and the group's own code never sees
    /// them individually; a knockout group's elements each need their own
    /// rasterisation, their own shape, and a read of a backdrop the buffer
    /// no longer holds. That is a different target type
    /// ([`KnockoutTarget`]), and swapping the target is the whole change.
    ///
    /// # The initial backdrop
    ///
    /// * **isolated** — fully transparent, per §11.4.5.
    /// * **non-isolated** — a frozen copy of the parent at group entry.
    ///   A copy, not a view: §11.4.6's `b = 0` means every element reads
    ///   the *same* backdrop, and the parent is about to be written to.
    ///
    /// Inside another knockout group the copy is taken from **that**
    /// group's initial backdrop, not from its accumulator — §11.4.6
    /// NOTE 6's nesting rule, handled where the copy is made so a caller
    /// cannot get it wrong.
    ///
    /// # Why the content stream is walked ONCE here
    ///
    /// [`Canvas::group`] runs a non-isolated group twice to recover
    /// `α_gn`. A knockout target does not need that: it accumulates
    /// `α_gi` as its own plane while the elements arrive, because it has
    /// to see them individually anyway. The second walk was always a
    /// consequence of *not* seeing them.
    fn knockout_group<R>(
        &mut self,
        paint: LayerPaint,
        isolated: bool,
        mask: Option<&Mask>,
        mut f: impl FnMut(&mut Canvas<'_>) -> (R, bool),
    ) -> Option<GroupOutcome<R>> {
        let (w, h) = (self.width(), self.height());
        let initial = if isolated {
            Pixmap::new(w, h)?
        } else {
            match self {
                Self::Paint(p) => (*p).clone(),
                // §11.4.6 NOTE 6: the inner group inherits the OUTER
                // group's initial backdrop, not its accumulated result.
                Self::Knockout(k) => k.initial.clone(),
                // ★ A NON-ISOLATED knockout group on a subtractive page
                // gets the ISOLATED backdrop, and that is a named
                // approximation rather than an oversight: the backdrop is
                // ink and this buffer is screen colour, so there is
                // nothing to copy that would not go through the very round
                // trip the colorant buffer exists to delete. §11.4.8's
                // alpha recurrence is identical either way (`α_gb` is
                // always zero in a knockout group), so what is lost is
                // confined to `C_b` and `α_b`.
                Self::Record(_) => Pixmap::new(w, h)?,
                // ★ THE ROUND TRIP THAT IS WORSE THAN NOT DOING ONE AND
                // BETTER THAN HAVING NO BACKDROP AT ALL, and the numbers
                // are in `snapshot_srgb_backdrop`'s own docs: handing a
                // subtractive page's knockout groups a TRANSPARENT initial
                // backdrop took suite `PCS1_161` from 2 traps to 15. A
                // knockout group's entire definition is "composite against
                // the group's initial backdrop"; give it nothing and it
                // knocks out against nothing.
                Self::Cmyk(b) => b.snapshot_srgb_backdrop()?,
            }
        };
        let mut target = KnockoutTarget::new(initial)?;
        let (result, _dependent) = {
            let mut sub = Canvas::Knockout(&mut target);
            f(&mut sub)
        };
        let approximated = target.approximated;
        match self {
            Self::Paint(p) => target.finish(p, paint, mask),
            Self::Knockout(k) => {
                let mut r = target.result_pixmap();
                if let Some(m) = mask {
                    apply_mask(&mut r, m);
                }
                k.element_from_pixmap(&r, paint.opacity.clamp(0.0, 1.0), layer_blend(paint));
            }
            // Unreachable -- see the note on the `initial` match above.
            // A real knockout result would still be bridgeable, so the arm
            // does the bridgeable thing rather than dropping the group.
            Self::Cmyk(b) => {
                let r = target.result_pixmap();
                bridge_layer_into_cmyk(b, &r, paint, mask);
            }
            Self::Record(_) => {}
        }
        Some(GroupOutcome {
            result,
            backdrop_rerun: false,
            knockout_approximated: approximated,
        })
    }
}

impl Canvas<'_> {
    /// §11.4.6 / §11.4.8 on a **subtractive** canvas.
    ///
    /// # Why this is a separate method from [`Canvas::knockout_group`] and
    /// not a branch inside it
    ///
    /// Because it needs no [`KnockoutTarget`] at all, and the reason is
    /// structural rather than incidental. `KnockoutTarget` exists to
    /// recover an element's **shape** in isolation, which it can only do by
    /// rasterising each element into a spare pixmap first — `tiny_skia`
    /// rasterises and composites in one call and offers no way to see the
    /// coverage it used. A subtractive paint already arrives as a coverage
    /// mask plus a colour, so `f_s` is the coverage byte and `α_s` is that
    /// times the constant alpha, both available per pixel with nothing to
    /// reconstruct. The whole scratch-buffer-and-accumulate dance
    /// disappears.
    ///
    /// The knockout state therefore lives on the buffer itself
    /// (`CmykBuffer::into_knockout`) and every composite dispatches on it,
    /// which also means a knockout group nested inside another one needs no
    /// special handling here: the inner group's own buffer carries its own
    /// state.
    ///
    /// # Isolation
    ///
    /// Expressed entirely as the identity of the initial backdrop, exactly
    /// as it is in the additive path: transparent for an isolated group, a
    /// copy of the parent's current content for a non-isolated one.
    /// §11.4.8's alpha recurrence is the same either way — `α_gb` is
    /// identically zero in a knockout group — so there is no second branch.
    ///
    /// # Returns
    ///
    /// `None` if a buffer could not be allocated, with the same contract as
    /// [`Canvas::group`]: `f` was never called.
    fn knockout_group_cmyk<R>(
        &mut self,
        paint: LayerPaint,
        isolated: bool,
        mask: Option<&Mask>,
        mut f: impl FnMut(&mut Canvas<'_>) -> (R, bool),
    ) -> Option<GroupOutcome<R>> {
        let Self::Cmyk(parent) = self else {
            return None;
        };
        let (w, h, intent) = (parent.width(), parent.height(), parent.intent());
        // The parent's own ceiling, so a group cannot decline to composite
        // on a page whose buffer was already paid for. See
        // `CmykBuffer::max_bytes`.
        let max_bytes = Some(parent.max_bytes());
        let initial = if isolated {
            crate::cmyk_buffer::CmykBuffer::new(w, h, intent, max_bytes)?
        } else {
            // A full copy of the parent, which is what §11.4.8's `C_b` is.
            // Expensive and unavoidable: the backdrop has to survive every
            // element of the group while the accumulator is overwritten by
            // each one.
            parent.clone()
        };
        let mut child = crate::cmyk_buffer::CmykBuffer::new(w, h, intent, max_bytes)?
            .into_knockout(&initial)?;
        let (result, _dependent) = {
            let mut sub = Canvas::Cmyk(&mut child);
            f(&mut sub)
        };
        let mut done = child.finish_knockout();
        if let Some(m) = mask {
            done.apply_mask(m);
        }
        parent.absorb_counters(&done);
        let blend = layer_blend(paint);
        parent.composite_buffer(&done, paint.opacity.clamp(0.0, 1.0), blend);
        Some(GroupOutcome {
            result,
            backdrop_rerun: false,
            // ZERO, and that is the claim this method makes: nothing was
            // approximated. Every element of this group composited through
            // §11.4.8's own formula, in the space the page declared.
            knockout_approximated: 0,
        })
    }
}

/// A **knockout group**'s accumulation state — ISO 32000-1 §11.4.6 /
/// §11.4.8.
///
/// # Why a knockout group needs a type and an isolated one does not
///
/// In an ordinary group every element composites onto the result of the
/// elements beneath it, which is exactly what painting into one buffer
/// *is*. In a knockout group every element composites onto the group's
/// **initial** backdrop instead, and the accumulated result is a weighted
/// average taken with the element's own **shape** as the weight. Neither
/// quantity survives being painted into a shared buffer: the initial
/// backdrop is overwritten by the first element, and the shape is fused
/// with the opacity the moment the two are multiplied into one alpha.
///
/// So this type holds four planes where a `Pixmap` holds one:
///
/// | field | standard's name | why it cannot be folded into `accum` |
/// |---|---|---|
/// | [`Self::initial`] | `⟨C_0, α_0⟩` | read by **every** element; `accum` no longer holds it after the first |
/// | [`Self::accum`] | `⟨C_i, α_i⟩` | the running result, including the backdrop |
/// | [`Self::group_alpha`] | `α_gi` | excludes the backdrop; `α_i` includes it, and `α_i > α_gi` whenever `α_0 > 0` |
/// | [`Self::group_shape`] | `f_gi` | `f ≠ α` for any element with `q < 1`, and §11.4.6 makes computing it a `shall` for a group used inside another knockout group |
///
/// # ★ Why the fixtures for this must set `/ca < 1`
///
/// Knockout and non-knockout are **identical** when every element is
/// opaque: `q_s = 1` gives `α_s = f_s`, and the two recurrences coincide
/// term for term (§11.4.6). A test built from opaque fills therefore
/// passes under both the correct implementation and the collapsed one,
/// which is why every knockout test in this crate sets a fractional alpha.
pub(crate) struct KnockoutTarget {
    /// `⟨C_0, α_0⟩` — the group's initial backdrop, frozen at group entry.
    /// Fully transparent for an isolated knockout group, which is the
    /// whole of what `/I` changes here.
    initial: Pixmap,
    /// `⟨C_i, α_i⟩` — the accumulated result, **including** the initial
    /// backdrop.
    accum: Pixmap,
    /// `α_gi` — the group's own accumulated alpha, **excluding** the
    /// initial backdrop. This is what the group returns as its `α`.
    group_alpha: Vec<f32>,
    /// `f_gi` — the group's own accumulated shape. Returned as the group's
    /// `f`, which matters when this group is itself an element of another
    /// knockout group (§11.4.6's `shall`).
    group_shape: Vec<f32>,
    /// Reused per element, so a group with a thousand elements allocates
    /// one page-sized buffer rather than a thousand. Cleared over each
    /// element's own device bounds only.
    scratch: Pixmap,
    /// Elements that could not be given knockout semantics because they
    /// read the destination back (`Canvas::pixmap_mut`). Surfaced by the
    /// caller; never silently zero.
    approximated: usize,
}

impl KnockoutTarget {
    /// Build the state for a knockout group whose initial backdrop is
    /// `initial`.
    ///
    /// Returns `None` if either page-sized buffer cannot be allocated, with
    /// the same contract [`Canvas::group`] has: the caller falls back and
    /// discloses rather than dropping content.
    pub(crate) fn new(initial: Pixmap) -> Option<Self> {
        let scratch = Pixmap::new(initial.width(), initial.height())?;
        let n = (initial.width() as usize) * (initial.height() as usize);
        Some(Self {
            accum: initial.clone(),
            group_alpha: vec![0.0; n],
            group_shape: vec![0.0; n],
            scratch,
            initial,
            approximated: 0,
        })
    }

    /// Device-pixel bounds to touch, from a paint's `f32` bounds.
    ///
    /// One pixel of padding on every side, because anti-aliased coverage
    /// reaches a fraction outside the geometric bound and a knockout
    /// element that stops one pixel short leaves a seam of the element
    /// beneath it showing — the exact artefact knockout exists to prevent.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn region(&self, bounds: Option<DeviceBounds>) -> (u32, u32, u32, u32) {
        let (w, h) = (self.accum.width(), self.accum.height());
        bounds.map_or((0, 0, w, h), |b| {
            (
                (b.left - 1.0).floor().max(0.0) as u32,
                (b.top - 1.0).floor().max(0.0) as u32,
                (((b.right + 1.0).ceil().max(0.0)) as u32).min(w),
                (((b.bottom + 1.0).ceil().max(0.0)) as u32).min(h),
            )
        })
    }

    /// Rasterise one element into the scratch buffer and accumulate it by
    /// §11.4.8.
    ///
    /// `paint_into` must draw at **full opacity**, so the scratch's alpha
    /// channel comes back as pure coverage — `f_s`. `q_s` is the constant
    /// opacity that was taken out of the paint to make that true.
    fn element(
        &mut self,
        bounds: Option<DeviceBounds>,
        q_s: f32,
        blend: BlendMode,
        paint_into: impl FnOnce(&mut Pixmap),
    ) {
        let (x0, y0, x1, y1) = self.region(bounds);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        // Clear only what will be read. A page-sized clear per element is
        // the difference between a knockout group costing what its marks
        // cost and costing what the page costs, times the element count.
        clear_region(&mut self.scratch, (x0, y0, x1, y1));
        paint_into(&mut self.scratch);
        let blend = crate::compositor::Blend::from_tiny_skia(blend)
            .unwrap_or(crate::compositor::Blend::Normal);
        self.accumulate((x0, y0, x1, y1), q_s, blend, |k, idx| {
            let px = crate::compositor::Pixel::from_premultiplied(k.scratch.pixels()[idx]);
            (px.c, px.a)
        });
    }

    /// Accumulate an already-rendered buffer as one element — the child
    /// group / annotation-layer case.
    ///
    /// `f_s` comes from the buffer's alpha, which for a group is `α_gn`;
    /// using it as the shape too is the `f_g ≈ α_g` approximation §11.4.6
    /// permits exactly when every element inside that child had `q = 1`.
    /// pdfcer takes it because the alternative is a fifth plane threaded
    /// through every nested group for a difference that only appears when a
    /// translucent element sits inside a translucent group inside a
    /// knockout group.
    fn element_from_pixmap(&mut self, buf: &Pixmap, q_s: f32, blend: crate::compositor::Blend) {
        let region = (0, 0, self.accum.width(), self.accum.height());
        self.accumulate_from(region, q_s, blend, buf, None);
    }

    /// The child-group case where the child is **non-isolated** and its
    /// interior blended against something: §11.4.4's backdrop removal is
    /// applied against **this group's** initial backdrop before the result
    /// is accumulated.
    fn element_from_non_isolated(
        &mut self,
        iso: &Pixmap,
        nis: &Pixmap,
        q_s: f32,
        blend: crate::compositor::Blend,
    ) {
        let region = (0, 0, self.accum.width(), self.accum.height());
        self.accumulate_from(region, q_s, blend, iso, Some(nis));
    }

    /// Shared body of the two buffer-sourced element paths.
    fn accumulate_from(
        &mut self,
        region: (u32, u32, u32, u32),
        q_s: f32,
        blend: crate::compositor::Blend,
        shape_source: &Pixmap,
        colour_over_backdrop: Option<&Pixmap>,
    ) {
        use crate::compositor::{Pixel, remove_backdrop};
        let width = self.accum.width();
        let (x0, y0, x1, y1) = region;
        for y in y0..y1 {
            for x in x0..x1 {
                let idx = (y * width + x) as usize;
                let s = Pixel::from_premultiplied(shape_source.pixels()[idx]);
                if s.a <= 0.0 {
                    continue;
                }
                let c = match colour_over_backdrop {
                    None => s.c,
                    Some(nis) => {
                        let initial = Pixel::from_premultiplied(self.initial.pixels()[idx]);
                        let over = Pixel::from_premultiplied(nis.pixels()[idx]);
                        remove_backdrop(over, initial, s.a)
                    }
                };
                self.accumulate_one(idx, c, s.a, q_s, blend);
            }
        }
    }

    /// Per-pixel accumulation shared by every element kind.
    fn accumulate(
        &mut self,
        region: (u32, u32, u32, u32),
        q_s: f32,
        blend: crate::compositor::Blend,
        read: impl Fn(&Self, usize) -> ([f32; 3], f32),
    ) {
        let width = self.accum.width();
        let (x0, y0, x1, y1) = region;
        for y in y0..y1 {
            for x in x0..x1 {
                let idx = (y * width + x) as usize;
                let (c, f_s) = read(self, idx);
                if f_s <= 0.0 {
                    continue;
                }
                self.accumulate_one(idx, c, f_s, q_s, blend);
            }
        }
    }

    /// §11.4.8, one pixel.
    fn accumulate_one(
        &mut self,
        idx: usize,
        c: [f32; 3],
        f_s: f32,
        q_s: f32,
        blend: crate::compositor::Blend,
    ) {
        use crate::compositor::{Pixel, composite_element_knockout, union_};
        let source = Pixel { c, a: f_s * q_s };
        let (out, ag) = composite_element_knockout(
            Pixel::from_premultiplied(self.initial.pixels()[idx]),
            Pixel::from_premultiplied(self.accum.pixels()[idx]),
            source,
            f_s,
            self.group_alpha[idx],
            blend,
        );
        if let Some(px) = out.to_premultiplied() {
            self.accum.pixels_mut()[idx] = px;
        }
        self.group_alpha[idx] = ag;
        self.group_shape[idx] = union_(self.group_shape[idx], f_s);
    }

    /// The group's **result** as a plain pixmap: colour after §11.4.4's
    /// backdrop removal, alpha `α_gn`.
    ///
    /// Backdrop removal runs against [`Self::initial`] with `α_gn` — which
    /// is why `group_alpha` had to be a plane and not the accumulator's
    /// alpha channel. For an isolated knockout group `α_0 = 0` makes the
    /// removal the identity, exactly as §11.4.5 NOTE 2 says, with no
    /// branch.
    ///
    /// Returns an all-transparent pixmap if allocation fails, which is the
    /// same visible outcome as a group that marked nothing — the honest
    /// degradation, since the alternative is dropping the caller's whole
    /// page.
    fn result_pixmap(&self) -> Pixmap {
        use crate::compositor::{Pixel, remove_backdrop};
        let Some(mut out) = Pixmap::new(self.accum.width(), self.accum.height()) else {
            return self.accum.clone();
        };
        for idx in 0..self.group_alpha.len().min(self.accum.pixels().len()) {
            let agn = self.group_alpha[idx];
            if agn <= 0.0 {
                continue;
            }
            let initial = Pixel::from_premultiplied(self.initial.pixels()[idx]);
            let over = Pixel::from_premultiplied(self.accum.pixels()[idx]);
            let c = remove_backdrop(over, initial, agn);
            if let Some(px) = (Pixel { c, a: agn }).to_premultiplied() {
                out.pixels_mut()[idx] = px;
            }
        }
        out
    }

    /// Composite this group's result onto its parent, by §11.4.4's element
    /// formula.
    fn finish(&self, dest: &mut Pixmap, paint: LayerPaint, mask: Option<&Mask>) {
        use crate::compositor::{Pixel, composite_element};
        let blend = layer_blend(paint);
        let opacity = paint.opacity.clamp(0.0, 1.0);
        let mut result = self.result_pixmap();
        if let Some(m) = mask {
            apply_mask(&mut result, m);
        }
        let n = dest.pixels().len().min(result.pixels().len());
        for idx in 0..n {
            let g = Pixel::from_premultiplied(result.pixels()[idx]);
            if g.a <= 0.0 {
                continue;
            }
            let backdrop = Pixel::from_premultiplied(dest.pixels()[idx]);
            let source = Pixel {
                c: g.c,
                a: g.a * opacity,
            };
            if let Some(px) = composite_element(backdrop, source, blend).to_premultiplied() {
                dest.pixels_mut()[idx] = px;
            }
        }
    }
}

/// Multiply a rendered group's alpha by a soft mask — §11.4.5.
///
/// # Why this multiplies ALPHA and not shape
///
/// §11.6.4.1 splits a soft mask into `f_m` (mask shape) and `q_m` (mask
/// opacity) according to `/AIS`. Under the **default** `/AIS false` the
/// mask value is the *opacity*: `f_m = 1`, `q_m = M`. So it scales `α_s`
/// and leaves `f_s` alone.
///
/// That distinction is invisible outside a knockout group — where only
/// `α_s` is read — and load-bearing inside one, because §11.4.8's
/// destination scale is `(1 − f_si)`. A renderer that routes the mask
/// through a coverage channel is silently implementing `/AIS true` for
/// every group. pdfcer's approximation here is the reverse and the safer
/// one: the mask is applied to alpha only, so a group inside a knockout
/// group knocks out by its **unmasked** shape. Named rather than hidden;
/// `/AIS true` is not yet distinguished.
///
/// The buffer is premultiplied, so both the colour and the alpha scale by
/// the same factor and the un-premultiplied colour is unchanged — which is
/// what "the mask changes how much of the group you see, not what colour
/// it is" means arithmetically.
pub(crate) fn apply_mask(buf: &mut Pixmap, mask: &Mask) {
    let data = mask.data();
    for (px, &m8) in buf.pixels_mut().iter_mut().zip(data.iter()) {
        // A fully open mask and an untouched pixel are both no-ops, and
        // together they are most of a page: a soft mask is usually a small
        // gradient on a large sheet.
        if m8 == u8::MAX || px.alpha() == 0 {
            continue;
        }
        let m = u32::from(m8);
        let scale = |v: u8| u8::try_from((u32::from(v) * m) / 255).unwrap_or(u8::MAX);
        if let Some(q) = tiny_skia::PremultipliedColorU8::from_rgba(
            scale(px.red()),
            scale(px.green()),
            scale(px.blue()),
            scale(px.alpha()),
        ) {
            *px = q;
        }
    }
}

/// Composite a rendered sRGB layer or group **result** into a colorant
/// buffer — the bridge every nested drawing on a subtractive page crosses.
///
/// # What this is, honestly
///
/// The group's **interior** was composited in sRGB, by an ordinary
/// [`Canvas::Paint`] child, because a colorant buffer cannot be handed to
/// `tiny_skia` and a group's contents are rasterised the same way any other
/// content is. Its **result** then crosses into ink here, and composites
/// under §11.4.4's formula in the subtractive space, through
/// [`crate::cmyk_buffer::CmykBuffer::composite_srgb`].
///
/// So a page with groups on it gets:
///
/// | | blends in |
/// |---|---|
/// | elements painted directly on the page | **ink** — correct |
/// | a group's result against the page | **ink** — correct |
/// | elements *inside* a group, against each other | screen — **wrong** |
///
/// The third row is `Pass 97.1f`'s work and is counted here
/// ([`crate::cmyk_buffer::CmykBuffer::groups_bridged`]) rather than
/// described in a comment, because a shortfall nobody counts is a shortfall
/// nobody notices has grown.
///
/// # Why this is nonetheless an improvement and not a wash
///
/// Before this Pass **every** row above blended in screen colour. §11.4.5
/// makes the outer state — the group's `/ca` and its `/BM` — apply to the
/// group's *result*, which is precisely the composite this function
/// performs, so the row that moves is the one the standard singles out.
///
/// # The soft mask
///
/// Applied to the sRGB result before the bridge, by the same
/// [`apply_mask`] the additive path uses. §11.4.5's mask multiplies the
/// group's **alpha**, and alpha is space-independent — so performing it on
/// either side of the colour conversion gives the same answer, and doing it
/// on the sRGB side reuses code that is already tested.
fn bridge_layer_into_cmyk(
    buf: &mut crate::cmyk_buffer::CmykBuffer,
    group: &Pixmap,
    paint: LayerPaint,
    mask: Option<&Mask>,
) {
    buf.note_group_approximated();
    let region = (0, 0, buf.width(), buf.height());
    let blend = layer_blend(paint);
    let opacity = paint.opacity.clamp(0.0, 1.0);
    if let Some(m) = mask {
        let mut masked = group.clone();
        apply_mask(&mut masked, m);
        buf.composite_srgb(&masked, region, opacity, blend);
    } else {
        buf.composite_srgb(group, region, opacity, blend);
    }
}

/// Zero one rectangle of a pixmap.
///
/// `Pixmap::fill` clears the whole buffer, which is the wrong cost for a
/// per-element scratch, and `Pixmap` exposes no sub-rectangle clear — so
/// the rows are zeroed directly.
fn clear_region(p: &mut Pixmap, region: (u32, u32, u32, u32)) {
    let width = p.width();
    let (x0, y0, x1, y1) = region;
    let blank = tiny_skia::PremultipliedColorU8::TRANSPARENT;
    let px = p.pixels_mut();
    for y in y0..y1 {
        let row = (y * width) as usize;
        for x in x0..x1 {
            px[row + x as usize] = blank;
        }
    }
}

/// What [`Canvas::group`] did, alongside whatever the content run returned.
#[derive(Debug)]
pub(crate) struct GroupOutcome<R> {
    /// The value the **first** content run returned.
    pub result: R,
    /// The group's contents were re-run over a copy of their own backdrop
    /// (§11.4.4). Counted by the caller so the cost is visible rather than
    /// inferred from a stopwatch.
    pub backdrop_rerun: bool,
    /// Elements of a **knockout** group that had to be composited with
    /// non-knockout semantics because they read the destination back — a
    /// shading, an overprint composite, a per-paint non-separable blend.
    /// Zero for every non-knockout group.
    pub knockout_approximated: usize,
}

/// Resolve a [`LayerPaint`] to the compositor's own blend type.
///
/// The two fields cannot both be set — `nonseparable` is `Some` exactly
/// when `blend` was parked at `SourceOver` — so this is a resolution, not a
/// merge. An unrecognised `tiny_skia::BlendMode` (one no PDF `/BM` name
/// produces) falls to `Normal`, which is what an unknown mode composites as
/// everywhere else in this crate.
fn layer_blend(paint: LayerPaint) -> crate::compositor::Blend {
    paint.nonseparable.map_or_else(
        || {
            crate::compositor::Blend::from_tiny_skia(paint.blend)
                .unwrap_or(crate::compositor::Blend::Normal)
        },
        crate::compositor::Blend::NonSeparable,
    )
}

/// Composite an **isolated** group's result — the path this crate has
/// always taken, kept byte-for-byte.
///
/// # Why this is not routed through `crate::compositor` too
///
/// Because it does not need to be, and routing it would move every
/// anti-aliased edge in the corpus by a quantisation step. `tiny_skia`'s
/// `draw_pixmap` already computes §11.4.4's formula for the isolated case
/// in 8-bit premultiplied arithmetic; pdfcer's `f32` version is the same
/// function with different rounding. The new arithmetic is therefore
/// confined to the case that is currently **wrong**, which keeps the
/// pdfium parity gate a signal about correctness rather than about
/// rounding.
fn composite_group_result(
    dest: &mut Pixmap,
    group: &Pixmap,
    paint: LayerPaint,
    mask: Option<&Mask>,
) {
    // §11.4.5 — the mask applies to the group's RESULT. Applied to a copy
    // rather than in place because `group` is also run 1's `α_gn` source
    // for the non-isolated path, and mutating it there would silently
    // change what backdrop removal divides by.
    let masked;
    let group = if let Some(m) = mask {
        let mut g = group.clone();
        apply_mask(&mut g, m);
        masked = g;
        &masked
    } else {
        group
    };
    if let Some(mode) = paint.nonseparable {
        crate::blend_nonsep::composite_layer(dest, group, mode, paint.opacity.clamp(0.0, 1.0));
    } else {
        dest.draw_pixmap(
            0,
            0,
            group.as_ref(),
            &PixmapPaint {
                opacity: paint.opacity.clamp(0.0, 1.0),
                blend_mode: paint.blend,
                quality: FilterQuality::Nearest,
            },
            Transform::identity(),
            // No mask: the contents were already clipped while being
            // drawn, so re-applying the clip here would double-multiply
            // its anti-aliased edge.
            None,
        );
    }
}

/// Composite a **non-isolated** group's result: §11.4.4's backdrop removal,
/// then §11.4.4's element formula, both in `f32`.
///
/// * `dest` — on entry the frozen initial backdrop; on exit the composited
///   result.
/// * `iso` — run 1's buffer. Only its **alpha** is read, and it is `α_gn`:
///   the group's own accumulated alpha, excluding the backdrop.
/// * `nis` — run 2's buffer: the group's colour accumulated **over** that
///   backdrop.
///
/// # Why `f32` here is not fastidiousness
///
/// Backdrop removal contains a single `1/α_gn`, which amplifies whatever
/// error its input carries by that factor. At `α_gn = 0.02` a half-level
/// 8-bit error becomes 25 levels — visible, and exactly the magnitude the
/// suite transparency panels trap on. The *inputs* are still 8-bit (the
/// elements were rasterised by `tiny_skia` into a `Pixmap`), and that
/// remaining quantisation is a documented shortfall of this stage rather
/// than a solved problem: `Pass 97.1`'s colorant buffer is where the
/// accumulation itself becomes `f32`.
fn composite_non_isolated_group(
    dest: &mut Pixmap,
    iso: &Pixmap,
    nis: &Pixmap,
    paint: LayerPaint,
    mask: Option<&Mask>,
) {
    use crate::compositor::{Pixel, composite_element, remove_backdrop};

    let blend = layer_blend(paint);
    let opacity = paint.opacity.clamp(0.0, 1.0);
    let mask = mask.map(Mask::data);
    let n = dest
        .pixels()
        .len()
        .min(iso.pixels().len())
        .min(nis.pixels().len());
    for idx in 0..n {
        // The group's OWN alpha. Zero means the group marked nothing here,
        // and §11.4.4's result is then unreachable whatever its colour:
        // leave the backdrop alone.
        let agn = f32::from(iso.pixels()[idx].alpha()) / 255.0;
        if agn <= 0.0 {
            continue;
        }
        let backdrop = Pixel::from_premultiplied(dest.pixels()[idx]);
        let over = Pixel::from_premultiplied(nis.pixels()[idx]);
        // ★ The removal divides by the UNMASKED `α_gn`. The mask is not
        // part of the group's own accumulation — §11.4.5 applies it to the
        // finished result — so masking before the removal would divide by
        // the wrong number and shift the colour, not just the alpha.
        let c = remove_backdrop(over, backdrop, agn);
        // §11.4.5: the constant alpha at the `Do`, and the soft mask,
        // multiply the group's own alpha to give the source alpha of the
        // group-as-element. §11.6.4.1: under the default `/AIS false` the
        // mask value is an OPACITY (`q_m`), which is why it multiplies
        // alpha here rather than shape.
        let m = mask.map_or(1.0, |d| d.get(idx).map_or(1.0, |v| f32::from(*v) / 255.0));
        let source = Pixel {
            c,
            a: agn * opacity * m,
        };
        if let Some(px) = composite_element(backdrop, source, blend).to_premultiplied() {
            dest.pixels_mut()[idx] = px;
        }
    }
}

/// Shader-shaped assertions that the decomposition round-trips.
///
/// These exist because the whole safety argument for `Pass 75.0`'s
/// plumbing commit is *"`to_paint` rebuilds what the call site used to
/// build inline"*, and an argument that is only made in a comment is an
/// argument nobody can re-run.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_brush_quantises_exactly_as_the_old_inline_paint_did() {
        let c = crate::gstate::Rgb {
            r: 0.5,
            g: 0.25,
            b: 1.0,
        };
        let spec = BrushSpec::solid(c, 0.5, BlendMode::Multiply);

        // The old inline construction, reproduced here verbatim.
        let mut expected = Paint::default();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        expected.set_color_rgba8(
            (c.r * 255.0) as u8,
            (c.g * 255.0) as u8,
            (c.b * 255.0) as u8,
            (0.5_f32.clamp(0.0, 1.0) * 255.0).round() as u8,
        );
        expected.anti_alias = true;
        expected.blend_mode = BlendMode::Multiply;

        let got = spec.to_paint();
        assert_eq!(got.blend_mode, expected.blend_mode);
        assert_eq!(got.anti_alias, expected.anti_alias);
        match (got.shader, expected.shader) {
            (tiny_skia::Shader::SolidColor(a), tiny_skia::Shader::SolidColor(b)) => {
                assert_eq!(a, b)
            }
            _ => panic!("a solid brush must produce a SolidColor shader"),
        }
    }

    #[test]
    fn alpha_rounds_rather_than_truncates() {
        // 0.5 × 255 = 127.5. Truncation gives 127, rounding gives 128, and
        // the interpreter has always rounded. A regression here would be
        // one level of alpha on every semi-transparent object in every
        // document — visible in aggregate, invisible in review.
        let c = crate::gstate::Rgb {
            r: 0.0,
            g: 0.0,
            b: 0.0,
        };
        let spec = BrushSpec::solid(c, 0.5, BlendMode::SourceOver);
        assert_eq!(spec.solid_rgba().map(|q| q[3]), Some(128));
    }
}

//! # pdfcer-render — headless rasterizer (draw-ops → pixels)
//!
//! Turns a loaded document's pages into pixmaps: content-stream
//! interpretation ([`interpret`]) over a CPU rasterizer (`tiny-skia`,
//! the `PRIOR_ART.md`/decision-001 selection — pure Rust, no GPU, no
//! windowing, WASM-clean). "Headless" is the whole point:
//! `render page 3 to PNG` must work on a server with no display, which
//! is also what makes the future web fork and `pdfcer render-page`
//! possible from one code path. Spec basis for the page-level geometry
//! here: `iso32000__s__8.3.md` (device space, y-axis, CTM),
//! `iso32000__s__7.7.3.md` (CropBox clipping, clockwise `/Rotate`).
//!
//! ## Load-bearing invariant
//!
//! Zero GUI/windowing dependencies (docs/ARCHITECTURE.md §3), verified
//! by `cargo tree` in CI (a CPU rasterizer is explicitly fine; a
//! windowing toolkit is not). `pdfce-gui` displays these pixmaps;
//! `pdfcer render-page` writes them to PNG; the WASM fork reuses
//! both paths unchanged.
//!
//! ## Page → device geometry (this module's own job)
//!
//! PDF user space is y-UP with its origin at the page's lower-left;
//! raster device space is y-DOWN with the origin at the top-left. The
//! base device CTM therefore composes, in order (§8.3.4 row-vector
//! convention):
//! 1. translate the resolved **CropBox** (the visible region — Table
//!    30: content "shall be clipped to this rectangle") to the origin;
//! 2. flip y and scale by the zoom factor;
//! 3. apply the page's **`/Rotate`** (CLOCKWISE quarter-turns at
//!    display time, Table 30 — note the opposite sense from §8.3.3's
//!    counter-clockwise rotation matrices), swapping the pixmap's
//!    width/height for 90°/270°.
//!
//! ## Honesty contract
//!
//! Rendering is best-effort by design at this Pass (images and shadings
//! are recognized-but-deferred; text renders, but a font with no
//! embedded program is drawn with a bundled substitute face), and every
//! shortfall is COUNTED in [`interpret::Diagnostics`] and returned with
//! the pixels — the caller can always tell a faithful raster from a
//! partial one ("fuzzy, never sneaky").
//!
//! For text specifically, three counters answer three different
//! questions an operator actually asks (decision 004 §6.4, rule R20):
//! `glyphs_substituted` + `substituted_fonts` ("are these the
//! document's own letterforms?"), `glyphs_notdef` ("is anything
//! missing?"), and `fonts_unsupported` ("was any text skipped
//! outright?"). A shell that renders pages is expected to surface
//! these; they are not decoration.

#![forbid(unsafe_code)]

pub mod annot;
/// The four NON-SEPARABLE blend modes (ISO 32000-1 §11.3.5.3, Table 137),
/// computed by pdfcer because the rasteriser's are measurably wrong —
/// see the module docs and `ARCHITECTURE.md` §12 decision 066.
pub mod blend_nonsep;
pub mod cancel;
pub(crate) mod canvas;
/// Reuse of identical clip masks within one render — see the module's
/// own docs for the census that justified it.
pub(crate) mod clip_cache;
pub(crate) mod cmyk_buffer;
pub(crate) mod cmyk_paint;
pub mod color;
pub mod compositor;
pub mod display_list;
pub mod emf;
pub mod export;
pub mod font;
pub mod gstate;
pub mod icc;
pub mod image;
pub mod interpret;
pub mod layer_state;
pub mod mask;
pub mod mesh;
pub mod overprint;
pub mod profile;
pub mod shading;
pub mod svg;
pub mod text;
pub mod type3;

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree::Page;
use tiny_skia::{Pixmap, Transform};

// `DocumentView` is a parameter type of this crate's public entry points
// ([`render_page_view`], [`render_page_with_view`]), so it is re-exported
// rather than merely imported — the Rust API Guidelines' rule against
// naming a dependency's type in a public signature without giving
// consumers a way to name it through this crate (same reason `tiny_skia`
// is re-exported above).
#[doc(inline)]
pub use pdfcer_core::view::DocumentView;

// The annotation scope and its Table 169 classification are named in
// `RenderOptions`' public surface, so they are re-exported at the crate
// root alongside it — a shell should not have to know they live in
// `annot` to write `pdfcer_render::AnnotationScope::Document`.
pub use annot::{AnnotationClass, AnnotationScope};
// The §8.6 colour-space model and its disclosures. `ColorDiagnostics` is a
// field of `Diagnostics`, which is already re-exported here, so a shell
// reading `diagnostics.color.tint_transform_not_applied` must be able to
// name its type without knowing which module it lives in.
pub use color::{ColorDiagnostics, ColorSpace, ColorState, Colorant, DeviceSpace};
pub use display_list::{
    ClipId, DisplayList, DisplayListKey, ExportTally, MAX_DISPLAY_LIST_BYTES, PoisonReason,
    record_page,
};
pub use font::{
    FallbackKey, FontData, FontEnvironment, GlyphSource, InkProbe, InkProbeSource, PageBackdrop,
    RenderOptions, RenderPolicy,
};
pub use interpret::Diagnostics;
pub use layer_state::LayerVisibility;
pub use shading::{ColorRamp, Geometry, PaintRoute, Shading, ShadingDiagnostics, ShadingFunction};
// `RenderedPage::pixmap` is a public field of a `tiny_skia` type, so this
// crate must re-export `tiny_skia` or every consumer has to add its own
// dependency on it and guess a compatible version — the Rust API
// Guidelines' rule against exposing a dependency's types without
// re-exporting the dependency. `pdfce-gui` reads pixels through this
// path; `pdfcer` reaches `Pixmap::encode_png` through it.
pub use tiny_skia;
// Re-export the core types the rendering surface is built around, so a
// consumer can name them through this crate alone (kept from Pass 0's
// surface for continuity). `DocumentView` joins them at Pass 17.0 for
// exactly the same reason: it is now a parameter type of this crate's
// public entry points, so a consumer must be able to name it here.
#[doc(inline)]
pub use pdfcer_core::{PdfError, PdfVersion};

/// Maximum pixmap edge, in pixels (pdfcer policy, `ARCHITECTURE.md`
/// §10.1): bounds the raster allocation. 16,384 px of RGBA is exactly
/// **1.00 GiB**, which is where the number comes from — memory grows as the
/// **square** of the edge, so this is not a round number that could simply be
/// raised.
///
/// # ★ Its original justification was wrong, and how it was wrong is useful
///
/// This doc comment used to end: *"16,384 px covers a 14,400-unit (200-inch,
/// Annex C max) page edge at 80+ DPI — **beyond any plausible viewing zoom at
/// Pass 1**."*
///
/// The arithmetic was right and the conclusion was not, because it reasoned
/// about **pages** when the operator zooms into **regions**. Bounding
/// `page_edge × scale` makes this constant a *zoom ceiling*, and one that gets
/// **tighter the larger the sheet**:
///
/// | sheet | longest edge | max zoom @ 2× DPR |
/// |---|---:|---:|
/// | A4 portrait | 842 pt | 9.7× |
/// | A3 | 1191 pt | 6.9× |
/// | **A1 landscape** | 2384 pt | **3.4×** |
/// | A0 | 3370 pt | 2.4× |
///
/// That is backwards for drafting review: the big sheets carry the detail
/// worth magnifying. It was found by the `pdfcer-gui` session building a viewer
/// against this crate, against the operator's requirement *"I want to be able
/// to zoom in as much as feasibly possible."*
///
/// **The fix was not a bigger number** — reaching 6.9× on A1 would cost 4 GiB
/// per open page — **but [`render_page_region`]**, which makes this constant
/// bound the *returned pixmap* instead, at which point 16,384 is generous and
/// memory becomes a function of viewport area rather than of zoom.
///
/// The lesson worth carrying past this constant: a guard justified by *"beyond
/// any plausible X"* is a **prediction about use**, not a fact about the
/// format, and it should be re-read whenever the use changes.
pub const MAX_PIXMAP_EDGE: u32 = 16 * 1024;

/// Bytes of storage each pixel of the **subtractive compositing buffer**
/// costs: four colorant planes plus alpha.
///
/// Taken from the buffer's own element type rather than restated, so a
/// change to that type cannot desynchronise this constant from the
/// arithmetic it describes. **20 bytes** today (`f32` × 5).
///
/// Published because a caller sizing a raster needs the cost per pixel to
/// predict the ceiling — but prefer [`will_composite_in_cmyk`], which asks
/// the actual question and does the arithmetic here, where it cannot rot.
pub const CMYK_BYTES_PER_PIXEL: usize = cmyk_buffer::BYTES_PER_PIXEL;

/// The default ceiling on a single subtractive compositing buffer, in bytes.
///
/// At [`CMYK_BYTES_PER_PIXEL`] this permits **13,421,772 pixels** — a
/// US-Letter page to roughly 379 DPI, or A4 to about 518 % zoom. Past it the
/// page composites in sRGB instead and **says so** (`cmyk_buffer_refused`),
/// which is a disclosed approximation rather than a failed render.
///
/// # This is a default, not a limit
///
/// [`RenderOptions::with_max_cmyk_buffer_bytes`] overrides it, and
/// `pdfcer_core::settings::Settings::max_cmyk_buffer_bytes` is where the
/// operator's persisted choice arrives from. The ceiling exists because
/// `ARCHITECTURE.md` §10 forbids an **untrusted-input-sized** allocation
/// without one — page dimensions come from the file — and an operator who
/// names a number is not untrusted input. So there is deliberately no cap on
/// the override, exactly as `max_zoom_percent` has none.
///
/// Raising it costs memory **and time**: compositing in ink measured roughly
/// 50 % slower than compositing on screen at the same pixel count. Whole-page
/// A4 at the end of the [`MAX_PIXMAP_EDGE`] tier (1946 %) would want ~3.8 GB
/// of buffer; a square 16,384² raster, ~5.4 GB.
///
/// **And this bounds ONE buffer, not the render.** A transparency group gets
/// a page-sized child, a sibling group reuses a retained spare, and a
/// knockout group also holds a full copy of its initial backdrop — so peak
/// resident memory is up to roughly **4×** this on a page with nested
/// transparency. That is the number a shell should put beside the setting.
pub const DEFAULT_MAX_CMYK_BUFFER_BYTES: usize = cmyk_buffer::DEFAULT_MAX_CMYK_BUFFER_BYTES;

/// How many pixels a subtractive compositing buffer may cover under
/// `max_bytes`, where `None` means [`DEFAULT_MAX_CMYK_BUFFER_BYTES`].
///
/// The argument is `Option<usize>` so that it takes
/// `pdfcer_core::settings::Settings::max_cmyk_buffer_bytes` verbatim: a
/// caller never has to decide what "unset" means, because the answer lives
/// here.
///
/// ```
/// # use pdfcer_render::{max_cmyk_composite_pixels, DEFAULT_MAX_CMYK_BUFFER_BYTES};
/// assert_eq!(max_cmyk_composite_pixels(None), 13_421_772);
/// assert_eq!(
///     max_cmyk_composite_pixels(Some(DEFAULT_MAX_CMYK_BUFFER_BYTES)),
///     max_cmyk_composite_pixels(None)
/// );
/// // Doubling the ceiling doubles the pixels — the relation is linear.
/// assert_eq!(max_cmyk_composite_pixels(Some(40)), 2);
/// ```
#[must_use]
pub const fn max_cmyk_composite_pixels(max_bytes: Option<usize>) -> u64 {
    let bytes = match max_bytes {
        Some(b) => b,
        None => DEFAULT_MAX_CMYK_BUFFER_BYTES,
    };
    (bytes / CMYK_BYTES_PER_PIXEL) as u64
}

/// Would a raster of `width_px × height_px` composite in ink, or fall back
/// to sRGB?
///
/// **This is the question a caller actually has**, which is why it is a
/// function rather than a constant to divide by: a shell choosing between a
/// whole-page raster and a region raster needs to know whether the choice
/// changes the colours, and hiding the arithmetic here means a change to
/// [`CMYK_BYTES_PER_PIXEL`] cannot leave a second copy of it behind.
///
/// `max_bytes` is `None` for the built-in default — see
/// [`max_cmyk_composite_pixels`].
///
/// # What a `true` does and does not promise
///
/// It promises the **ceiling** does not refuse this size. It is not an
/// allocation: a machine that cannot find the memory still refuses, and
/// still discloses it the same way. And it says nothing about whether the
/// page *wants* ink — a page whose group declares no subtractive blending
/// space composites on screen at every size, correctly (§8.6.6.4).
///
/// ```
/// # use pdfcer_render::will_composite_in_cmyk;
/// // A4 (595 x 842 pt) at 5.17x — 13.39 M px, inside the default ceiling.
/// assert!(will_composite_in_cmyk(3076, 4353, None));
/// // A4 at 5.18x — 13.44 M px, past it.
/// assert!(!will_composite_in_cmyk(3082, 4362, None));
/// // ...and inside it again once the operator pays for a bigger buffer.
/// assert!(will_composite_in_cmyk(3082, 4362, Some(512 * 1024 * 1024)));
/// ```
#[must_use]
pub const fn will_composite_in_cmyk(
    width_px: u32,
    height_px: u32,
    max_bytes: Option<usize>,
) -> bool {
    if width_px == 0 || height_px == 0 {
        return false;
    }
    (width_px as u64) * (height_px as u64) <= max_cmyk_composite_pixels(max_bytes)
}

/// Rasterization errors.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RenderError {
    /// The page's content streams failed to decode or tokenize.
    #[error(transparent)]
    Content(#[from] pdfcer_core::content::ContentError),
    /// The caller asked for this render to stop
    /// ([`cancel::RenderCancel`]).
    ///
    /// **Not a failure.** It means the answer stopped being wanted —
    /// the operator zoomed again, edited, or closed the document — and
    /// a caller should discard it silently rather than surfacing it. It
    /// is an error variant only because a cancelled render has no
    /// pixmap to return, and inventing a half-painted one would be
    /// worse than saying so.
    #[error("render cancelled by the caller")]
    Cancelled,
    /// `CropBox × scale` exceeds [`MAX_PIXMAP_EDGE`] or is empty.
    #[error("requested raster size {width}x{height} is empty or exceeds MAX_PIXMAP_EDGE")]
    BadRasterSize {
        /// Requested width in pixels.
        width: u32,
        /// Requested height in pixels.
        height: u32,
    },
    /// The page uses an operator that has no recordable formulation, so
    /// [`display_list::record_page`] refused rather than returning a list
    /// that would render the page **nearly** right.
    ///
    /// **This is not a rendering failure.** The page renders correctly
    /// through [`render_page_region`]; it simply cannot be cached. A caller
    /// that sees this should fall back, once, and remember not to retry the
    /// recording for that `(page, epoch)`.
    #[error("page cannot be recorded as a display list: {reason}", reason = reason.as_str())]
    PageNotRecordable {
        /// Which operator class refused — see [`PoisonReason`].
        reason: display_list::PoisonReason,
    },
    /// A [`DisplayList`] was replayed with a key it was not recorded for.
    ///
    /// # Why this exists rather than a silent re-render
    ///
    /// Because `pdfcer-render` cannot observe a shell's edit epoch, and a
    /// display list that renders a document's **previous state** while
    /// reporting success is strictly worse than no cache at all. The
    /// mismatch is therefore refused by name, with both keys in the
    /// message, so a log line says which half drifted.
    // ONE LINE, no backslash continuation. The continuation form was used
    // here and shipped ten literal spaces into the message (6af5655):
    // `rustfmt` and patch tooling both eat the backslash, and nothing but
    // reading the RENDERED string catches it. `rustfmt` leaves a long
    // literal alone, so the long line IS the safe form.
    #[error(
        "display list is for epoch {recorded_epoch} at scale {recorded_scale}, but was replayed as epoch {expected_epoch} at scale {expected_scale}"
    )]
    DisplayListStale {
        /// The epoch the caller believed it held.
        expected_epoch: u64,
        /// The epoch the list was recorded at.
        recorded_epoch: u64,
        /// The scale the caller believed it held.
        expected_scale: f32,
        /// The scale the list was recorded at.
        recorded_scale: f32,
    },
}

/// A rendered page: pixels plus the honesty report.
#[derive(Debug)]
pub struct RenderedPage {
    /// The rasterized page, RGBA (premultiplied, as `tiny_skia` stores it).
    ///
    /// **White-backed by default** — every pixel opaque — and see-through
    /// where nothing was painted when the render asked for
    /// [`PageBackdrop::Transparent`] (`Pass 248.0`). Which one this is
    /// follows from the [`RenderOptions`] the caller passed; the struct
    /// does not repeat it, because a copy of an input is a second place
    /// for the fact to go stale.
    pub pixmap: Pixmap,
    /// What was NOT fully rendered (module docs).
    pub diagnostics: Diagnostics,
}

/// Rasterize one page of a **loaded file** at `scale` device pixels per
/// user-space unit (`scale = dpi / 72.0`; `1.0` ≈ 72 DPI).
///
/// A thin wrapper over [`render_page_view`] with `doc.view()` — see that
/// function for what the distinction buys and why this signature was kept.
///
/// # Errors
///
/// [`RenderError`] — content decode failures or a raster size outside
/// the guard. Deferred/unknown operators are NOT errors; they are
/// diagnostics (module docs).
pub fn render_page(doc: &Document, page: &Page, scale: f32) -> Result<RenderedPage, RenderError> {
    render_page_view(&doc.view(), page, scale)
}

/// Rasterize one page of a loaded file with caller-supplied options.
///
/// A thin wrapper over [`render_page_with_view`] with `doc.view()`.
///
/// # Errors
///
/// As [`render_page`].
pub fn render_page_with(
    doc: &Document,
    page: &Page,
    scale: f32,
    options: &RenderOptions,
) -> Result<RenderedPage, RenderError> {
    render_page_with_view(&doc.view(), page, scale, options)
}

/// Rasterize one page of **whatever document view the caller hands over**
/// — a loaded file, or an editing session with its unsaved edits applied.
///
/// # Why this exists (decision 018)
///
/// Until Pass 17.0 this crate only knew how to render a `&Document`, and
/// the GUI could only give it
/// [`EditSession::document()`](pdfcer_core::edit::EditSession::document) —
/// the BASE revision. Every editing feature from Pass 3.1 to Pass 16.2
/// therefore authored correctly and displayed not at all. Taking a
/// [`DocumentView`] is the fix: it carries both halves a renderer needs
/// (the object graph *and* the byte source its stream spans resolve
/// against), so a session's staged appearance and content streams resolve
/// out of the R45 staging buffer instead of reading off the end of the base
/// file.
///
/// The `&Document` entry points ([`render_page`], [`render_page_with`]) are
/// preserved verbatim as wrappers over these, which is what kept the
/// blast radius of that change near zero: `pdfcer`, `tools/roundtrip`,
/// `tools/font-parity`, `tools/render-parity` and every existing render
/// test are untouched, and "render the file on disk" remains a
/// one-argument idea.
///
/// # Errors
///
/// As [`render_page`].
pub fn render_page_view(
    view: &DocumentView<'_>,
    page: &Page,
    scale: f32,
) -> Result<RenderedPage, RenderError> {
    render_page_with_view(view, page, scale, &RenderOptions::default())
}

/// Rasterize one page of a document view with caller-supplied options —
/// currently the [`FontEnvironment`], i.e. the substitute faces available
/// to any font in the document that carries no embedded program (decision
/// 004 §6.3).
///
/// This is the seam through which a shell hands the renderer a system
/// face it discovered — a CJK font, or a user's explicit override.
/// `pdfcer-render` itself never enumerates, opens, or reads a font
/// (rule R19), which is what makes the same document render to the same
/// pixels on a CI runner, a developer laptop, and the WASM fork.
///
/// See [`render_page_view`] for the view-vs-document distinction.
///
/// # Errors
///
/// As [`render_page`].
pub fn render_page_with_view(
    doc: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    options: &RenderOptions,
) -> Result<RenderedPage, RenderError> {
    render_impl(doc, page, scale, None, options)
}

/// Rasterize a **sub-rectangle** of a page, so a viewer at high
/// magnification pays for the pixels it is showing rather than for the whole
/// sheet.
///
/// `region` is in **page space, pre-scale** — the same coordinate system as
/// [`Page::crop_box`], y-up. The returned pixmap covers exactly that region at
/// `scale`, and [`MAX_PIXMAP_EDGE`] then guards **the region** rather than the
/// page.
///
/// # Why this exists, and what it changes
///
/// Every other entry point rasterises the whole page, so the pixmap edge is
/// `page_edge × scale` and [`MAX_PIXMAP_EDGE`] silently becomes a **zoom
/// ceiling that gets tighter the larger the sheet** — backwards for drafting
/// review, where the big sheets carry the detail worth magnifying. On an A1
/// landscape sheet at 2× device pixel ratio that ceiling is **3.4×**; on the
/// A3 sheet the measurements were taken from, **6.9×**.
///
/// Raising the constant does not fix it: a whole-page RGBA pixmap grows as the
/// **square** of the edge, and 16,384 px is not arbitrary — it is exactly
/// 1.00 GiB. Reaching 6.9× on that sheet would cost 4 GiB per open page. With
/// a region, memory is a function of **viewport area** and stops scaling with
/// zoom altogether.
///
/// # ★ The cost model, measured — read this before building tiles on it
///
/// A region render still **interprets the whole content stream**. There is no
/// display-list cache and no parse-once/fill-many seam; this function shares
/// [`render_page_with_view`]'s implementation exactly, differing only in the
/// pixmap size and a translation on the base CTM.
///
/// What makes it worth having anyway is that `tiny_skia` culls geometry
/// outside the pixmap cheaply, so the expensive half — anti-aliased span
/// filling — is paid only for the region. On a dense A3 CAD drawing
/// (148,517 paints, 24,128 clip ops) the measured figures are in
/// `docs/render-region-measurements.md`.
///
/// **The consequence for a tiled strategy is the important part:** because
/// interpretation is re-paid per call, N tiles cost N × the interpretation.
/// A 3×3 ring is **not** free. Prefer one region covering the viewport over
/// nine tiles covering the same area, and treat tiling as a mechanism for
/// *bounding memory*, not for *saving time*.
///
/// # Errors
///
/// [`RenderError::BadRasterSize`] if the region is empty or its raster exceeds
/// [`MAX_PIXMAP_EDGE`]; otherwise as [`render_page`].
pub fn render_page_region(
    doc: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    region: pdfcer_core::page_tree::Rect,
    options: &RenderOptions,
) -> Result<RenderedPage, RenderError> {
    render_impl(doc, page, scale, Some(region), options)
}

/// The single rasterisation path, whole-page and region alike.
///
/// Shared deliberately rather than copied: a second implementation would be a
/// second place for the annotation z-order, the cancellation contract, the
/// `FormFieldsOnly` scope rule and the `contents_unresolved` carry-through to
/// drift. The ONLY difference a region makes is the pixmap size and a
/// translation on the base CTM — everything downstream is handed a CTM and a
/// pixmap and neither knows nor cares which it got.
fn render_impl(
    doc: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    region: Option<pdfcer_core::page_tree::Rect>,
    options: &RenderOptions,
) -> Result<RenderedPage, RenderError> {
    let (page_w, page_h, page_ctm) = page_device_geometry(page, scale);

    // `base_ctm64` is the one the interpreter composes `cm` against;
    // `base_ctm` is the narrowed copy every `tiny_skia` call takes. They
    // are produced together and must not be derived from each other in
    // the wrong direction -- widening the `f32` back to `f64` would carry
    // its rounding forward and is exactly the bug this pair removes.
    let (width, height, base_ctm, base_ctm64) = match region {
        None => (page_w, page_h, page_ctm, gstate::Mat64::from_f32(page_ctm)),
        Some(r) => {
            // ★ `region_base_geometry`, not `region_device_geometry` +
            // `post_translate`. The two agree exactly at ordinary
            // magnifications and diverge at deep zoom, where the old pair
            // did its arithmetic at the magnitude of the device coordinate
            // instead of the magnitude of the answer -- see that function
            // for the measured table (a requested 800x600 viewport came
            // back as 800x512 at a scale of 2.15 M).
            let Some(g) = region_base_geometry(page, scale, r) else {
                return Err(RenderError::BadRasterSize {
                    width: 0,
                    height: 0,
                });
            };
            (g.width, g.height, g.ctm, g.ctm64)
        }
    };

    if width == 0 || height == 0 || width > MAX_PIXMAP_EDGE || height > MAX_PIXMAP_EDGE {
        return Err(RenderError::BadRasterSize { width, height });
    }
    let Some(mut pixmap) = Pixmap::new(width, height) else {
        return Err(RenderError::BadRasterSize { width, height });
    };
    // §11.4.7 THE PAGE GROUP. The buffer starts fully TRANSPARENT, not
    // white, and the white paper is composited in ONCE at the end by
    // `flatten_page_group_over_white` below.
    //
    // This is not a refinement — it is the difference between right and
    // wrong for eleven of the fifteen blend modes, and pdfcer had it wrong
    // until 2026-08-17. §11.4.7:
    //
    //   "All of the elements painted directly onto a page … shall be
    //    treated as if they were contained in a transparency group P …
    //    called the page group." "Ordinarily … the page group shall be
    //    treated as an ISOLATED group, whose results shall then be
    //    composited with a backdrop colour appropriate for the medium.
    //    The backdrop is nominally white…"
    //
    //   ⟨Cg, fg, αg⟩ = Composite(U, 0, P)   then   C = (1 − αg)·W + αg·Cg
    //
    // An *isolated* group's initial backdrop is `U` — fully transparent —
    // and §11.4.5 adds that blend modes inside a group "shall not be
    // influenced by the group's backdrop". Compositing straight onto opaque
    // white therefore hands every blend function a backdrop of 1.0, which
    // is only harmless where `B(1.0, cs) = cs`: `Normal`, `Compatible`,
    // `Multiply`, `Darken`. For the other eleven the first object painted
    // at a given pixel comes out solid white or inverted — `Screen` of
    // anything over white is white, `Difference` inverts, and so on.
    //
    // The correction is these two steps and it is EXACT, not an
    // approximation: the pair of formulas above IS the standard's own
    // definition of what a page is. For `Normal` the two models coincide
    // algebraically, which is why almost nothing else in the test suite
    // moves.
    //
    // (`Pixmap::new` already zeroes, so there is no fill to perform here.
    // The line is kept as a comment rather than deleted because "the page
    // starts white" is the intuition every reader arrives with, and the
    // point is that the intuition is wrong.)

    // The one scope decision that is not about annotations at all:
    // `FormFieldsOnly` (Acrobat's "Form fields only") prints the widget
    // appearances and NOT the page beneath them, because the sourced
    // workflow is printing onto a pre-printed paper form where the page
    // background already physically exists. See `AnnotationScope`.
    let scope = options.effective_annotation_scope();

    // §11.4.7 / §11.7.2 — THE PAGE'S BLENDING COLOUR SPACE, read BEFORE the
    // buffer is chosen, because it is what chooses the buffer.
    //
    // ★ This read was already here one block below, feeding a counter. It
    // is hoisted rather than duplicated: two reads of the same catalog
    // entry could disagree after a future change to `page_blend_space`, and
    // the one thing that must not happen is a page counted as subtractive
    // and composited additively, or the reverse.
    let mut page_space_diag = color::ColorDiagnostics::default();
    // Destructured immediately: `page_space` drives behaviour and
    // `page_space_from` drives DISCLOSURE only. Keeping them as one tuple
    // past this point invites a caller to test the pair for equality and
    // accidentally make provenance behavioural.
    let (page_space, page_space_from) = if scope.paints_page_content() {
        interpret::page_blend_space(
            doc,
            page.id,
            &page.resources,
            &mut page_space_diag,
            options.policy().page_blend_space_source,
        )
    } else {
        // No page content means no page group to have a blending space.
        // Annotations composite in sRGB either way.
        (
            compositor::BlendSpace::Additive,
            interpret::BlendSpaceFrom::DeviceNative,
        )
    };

    // ★ THE SWITCH. A colorant buffer is engaged ONLY for a page whose
    // group declares a subtractive blending space -- 13 of 51 files in the
    // print-conformance suite, and 15 of 4,012 in `fixtures/external`, of
    // which every single one is a conformance fixture rather than an
    // organic document. Every other page keeps the sRGB path byte for byte,
    // which is not merely a safety measure: ISO 32000-1 §8.6.6.4 makes it
    // the SPECIFIED behaviour on an additive device, where a `Separation`
    // "never applies a process colorant directly; it always reverts to the
    // alternate colour space". Routing an ordinary page through ink would
    // be a deviation, not an improvement -- and Poppler #1565 is the
    // empirical version of the same warning, where enabling overprint
    // preview visibly shifted unrelated RGB raster content.
    let mut cmyk = if page_space.is_subtractive() {
        cmyk_buffer::CmykBuffer::new(
            width,
            height,
            options.policy().cmyk_intent,
            options.policy().max_cmyk_buffer_bytes,
        )
    } else {
        None
    };
    // A page that asked for ink and could not have it. Recorded here, where
    // both facts are in scope, and reported rather than failed.
    let cmyk_refused = usize::from(page_space.is_subtractive() && cmyk.is_none());
    // ONE canvas for the whole page — content first, then annotations over
    // it. Scoped in a block so the mutable borrow of `pixmap` ends before
    // the page-group flatten below, which needs the buffer back.
    //
    // Why a canvas rather than the pixmap itself: `Canvas` is the seam a
    // display list replaces the pixmap at (Pass 75.0, `crate::canvas`). In
    // paint mode it forwards everything, so this call chain is byte-for-byte
    // what it was.
    let diagnostics = {
        let mut canvas = match cmyk.as_mut() {
            Some(buffer) => canvas::Canvas::cmyk(buffer),
            None => canvas::Canvas::paint(&mut pixmap),
        };
        let mut diagnostics = if scope.paints_page_content() {
            let content = ContentStream::from_page(doc, page)?;
            let initial = gstate::GraphicsState::default_with_ctm64(base_ctm64);
            // §11.4.7 / §11.3.4 — THE PAGE'S BLENDING COLOUR SPACE, read
            // before anything is painted, because every element on the page
            // and every non-isolated group inside it composites in it.
            //
            // ★ This is the number the suite transparency panels turn on:
            // all of them declare `/Group /CS /DeviceCMYK` here, including
            // the one whose own objects are `ICCBased` RGB, so §11.3.4's
            // complement governs every blend on those pages and pdfcer
            // performs none of them that way yet. Counted rather than
            // silently ignored. `Pass 97.1e`'s colorant buffer IS that
            // fix and it shipped; a subtractive page composites in ink.
            let mut diagnostics = interpret::run_on(
                doc,
                &content,
                &page.resources,
                &options.fonts,
                initial,
                &mut canvas,
                options.cancel.as_ref(),
                options.policy(),
                page_space,
            );
            if page_space.is_subtractive() {
                diagnostics.blend_space_subtractive += 1;
            }
            // Set unconditionally, including for an ordinary additive page.
            // Reporting provenance only when it is interesting would make
            // its ABSENCE ambiguous between "not inferred" and "not
            // recorded", which is the shape that makes a disclosure
            // unreadable.
            diagnostics.blend_space_from = page_space_from.token();
            diagnostics.blend_space_from_output_intent =
                usize::from(page_space_from == interpret::BlendSpaceFrom::OutputIntent);
            // Carry the page-level omission into the render diagnostics. The
            // interpreter cannot observe it — the streams it never received
            // leave no trace in the operator stream — so the count is copied
            // from the page here, where the two facts meet. Without this the
            // raster of a page with a dangling `/Contents` would be silently
            // blank, which is exactly the "sneaky" outcome the project forbids.
            diagnostics.contents_streams_unresolved = page.contents_unresolved;
            diagnostics
        } else {
            // Content streams are not merely skipped at paint time — they are
            // never decoded. Two consequences worth stating rather than
            // discovering:
            //
            // - A page whose `/Contents` fails to decode still renders under
            //   this scope, because nothing asked it to. That is correct for
            //   the pre-printed-form workflow (the operator wants the field
            //   values, not the page) and it is the reason this branch cannot
            //   return `RenderError::Content`.
            // - `contents_streams_unresolved` stays 0, because pdfcer did not
            //   look. Reporting a page-level incompleteness it never measured
            //   would be an invented fact; the honest disclosure is the
            //   suppression flag itself.
            Diagnostics {
                page_content_suppressed: true,
                ..Diagnostics::default()
            }
        };

        // Pass 6.0: survey the page's annotations (ISO 32000-1 §12.5;
        // docs/decisions/008) and paint their appearances OVER the page content
        // (their natural z-order). The survey always COUNTS; painting is gated
        // by the effective scope, so `--no-annotations` still discloses how
        // many annotations exist while reproducing the pre-6.0 content-only
        // raster byte-for-byte — and a narrowed scope ("Document", "Document
        // and Stamps") discloses how many annotations it withheld.
        annot::survey_page_annotations(
            doc,
            page,
            base_ctm,
            &options.fonts,
            scope,
            &mut diagnostics,
            &mut canvas,
            options.cancel.as_ref(),
            options.policy(),
        );
        diagnostics
    };

    // THE ONE PLACE A CANCELLED RENDER BECOMES AN ERROR.
    //
    // The interpreter stops early and returns its diagnostics; it never
    // decides what that means. Here, at the only entry point that hands
    // out a `RenderedPage`, a set flag becomes `Cancelled` instead — so
    // no caller can be handed a half-painted pixmap and mistake it for
    // the page. Checked AFTER the work rather than before, because a
    // render cancelled on its last operator is still cancelled, and a
    // partial raster is exactly what must not escape.
    if options
        .cancel
        .as_ref()
        .is_some_and(cancel::RenderCancel::is_cancelled)
    {
        return Err(RenderError::Cancelled);
    }

    // §11.4.7's second formula: composite the isolated page group over the
    // medium's nominally-white backdrop.
    //
    // ★★ TWO IMPLEMENTATIONS OF ONE CLAUSE, AND THEY ARE NOT THE SAME
    // ORDER OF OPERATIONS.
    //
    // For an sRGB buffer the group's colour is ALREADY in the device space,
    // so §11.4.7's "convert the result to the device's native colour space
    // BEFORE compositing it with the context-dependent backdrop" is
    // vacuous -- there is nothing to convert -- and the media composite is
    // the only step.
    //
    // For a colorant buffer it is emphatically not vacuous. The conversion
    // comes FIRST and the white comes SECOND, because the conversion is
    // non-affine and the two orders give different pixels. Flattening onto
    // CMYK white (no ink) and converting afterwards is the intuitive order
    // and is wrong; see `CmykBuffer::to_srgb_over_white`, which carries the
    // worked number.
    let mut diagnostics = diagnostics;
    diagnostics.cmyk_buffer_refused += cmyk_refused;
    if let Some(buffer) = cmyk {
        diagnostics.cmyk_buffer_engaged = true;
        diagnostics.cmyk_bridged_pixels += buffer.bridged_pixels();
        diagnostics.cmyk_native_image_pixels += buffer.native_image_pixels();
        diagnostics.cmyk_groups_approximated += buffer.groups_approximated();
        diagnostics.cmyk_unbridged_images += buffer.unbridged_images();
        // The one failure mode here is an allocation the page has already
        // proven possible, so falling back to the transparent pixmap would
        // be a blank page. Keeping the (empty) pixmap and flattening it is
        // the same outcome the additive path would produce for a page that
        // painted nothing, which is at least a white sheet.
        // ★ SAMPLED BEFORE THE COLLAPSE, WHICH IS THE ENTIRE POINT.
        // `to_srgb_over_white` consumes the colorant state into sRGB and
        // the buffer is dropped immediately after; one line later there is
        // nothing left to ask. A probe taken after the collapse could only
        // report the same number a PNG already carries.
        let probe_cmyk = options
            .ink_probe
            .map(|(px, py)| probe_ink_from_buffer(&buffer, px, py, width, height));
        // §11.4.7's media composite, or — for an export that keeps the
        // page's transparency (`Pass 248.0`) — the same conversion with the
        // group's own alpha carried through instead of resolved against
        // paper. Two sibling collapses rather than one with a flag, because
        // the transparent one must never contain the `1 − a` term and a
        // shared body with a branch inside its pixel loop is where that
        // term would creep back in.
        let collapsed = match options.backdrop {
            PageBackdrop::White => buffer.to_srgb_over_white(),
            PageBackdrop::Transparent => buffer.to_srgb_transparent(),
        };
        if let Some(collapsed) = collapsed {
            pixmap = collapsed;
        } else if options.backdrop == PageBackdrop::White {
            flatten_page_group_over_white(&mut pixmap);
        }
        diagnostics.ink_probe = probe_cmyk;
    } else {
        // `Transparent` is the ONE-LINE case on the additive path: the
        // group already holds `(Cg·αg, αg)`, so keeping the page's
        // transparency is declining to add the paper. See
        // `RenderOptions::backdrop`.
        if options.backdrop == PageBackdrop::White {
            flatten_page_group_over_white(&mut pixmap);
        }
        diagnostics.ink_probe = options
            .ink_probe
            .map(|(px, py)| probe_ink_screen(px, py, width, height));
    }
    // The sRGB half is read from the finished raster in BOTH branches, so
    // one probe line always states both ends of the conversion under test.
    // Deliberately after the collapse: this is the number the operator's
    // PNG carries, and reading it from anywhere else would let the probe
    // and the file disagree.
    if let Some(probe) = diagnostics
        .ink_probe
        .as_mut()
        .filter(|p| p.source != InkProbeSource::OutOfRange)
    {
        let idx = (probe.y as usize) * (width as usize) + (probe.x as usize);
        if let Some(px) = pixmap.pixels().get(idx) {
            // `demultiply` because the page is opaque here (alpha 255
            // after the media composite), so the premultiplied bytes
            // already are the colour -- but going through the accessor
            // keeps this correct if that ever stops being true.
            let c = px.demultiply();
            probe.srgb = Some([c.red(), c.green(), c.blue()]);
        }
    }

    Ok(RenderedPage {
        pixmap,
        diagnostics,
    })
}

/// Composite an isolated page group over an opaque white backdrop —
/// §11.4.7's `C = (1 − αg) × W + αg × Cg`, with `W = 1` (white).
///
/// # Why this is a separate step rather than a white initial fill
///
/// Because §11.4.5 says blend modes inside a group "shall not be influenced
/// by the group's backdrop", and §11.4.7 makes the page group **isolated**,
/// i.e. its initial backdrop is fully transparent. A white initial fill
/// hands every blend function `cb = 1.0`, which is only harmless for the
/// four modes satisfying `B(1.0, cs) = cs`. Doing the white at the END is
/// what the standard actually specifies.
///
/// # The arithmetic, and why it is a plain lerp
///
/// The buffer is **premultiplied** (`Cg × αg` is what is stored), so the
/// formula's `αg × Cg` term is already the stored value and the whole
/// composite reduces to *"add the uncovered fraction of white"*:
///
/// ```text
///     stored  = αg × Cg                     (premultiplied)
///     result  = (1 − αg) × 255 + stored
/// ```
///
/// which is then opaque. No division, no unpremultiply round-trip, and no
/// precision loss on the covered pixels — a fully covered pixel
/// (`αg = 255`) is returned byte-identical, which is what keeps every
/// existing pixel assertion in the test suite stable.
/// Read one pixel's colorants out of the page's four-colorant buffer.
///
/// # Why the range check is here and not at the flag
///
/// Because the raster's dimensions are not known until the page's
/// geometry has been resolved — `--region`, `--scale` and the `/MediaBox`
/// between them decide it — so a coordinate the operator supplies cannot
/// be validated when it is parsed. Validating it here means the report
/// can say *"outside a 1224 × 1584 raster"* rather than *"invalid"*, which
/// is the difference between a usable answer and a rejection.
fn probe_ink_from_buffer(
    buffer: &cmyk_buffer::CmykBuffer,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> InkProbe {
    if x >= width || y >= height {
        return InkProbe {
            x,
            y,
            source: InkProbeSource::OutOfRange,
            cmyk: None,
            spots: Vec::new(),
            alpha: None,
            srgb: None,
        };
    }
    let idx = (y as usize) * (width as usize) + (x as usize);
    let px = buffer.pixel(idx);
    InkProbe {
        x,
        y,
        source: InkProbeSource::CmykBuffer,
        cmyk: Some(px.c),
        // The name comes from the buffer's roster and the tint from the
        // pixel, zipped in plane order -- `PixelCmyk::s` is padded to
        // `MAX_SPOTS`, so zipping against the roster drops the padding
        // rather than reporting phantom colorants.
        spots: buffer.spot_roster_at(idx),
        alpha: Some(px.a),
        // Filled by the caller from the finished raster; see there.
        srgb: None,
    }
}

/// The same question asked of a page that never held ink.
///
/// Returns the classification and nothing else. The temptation is to run
/// the sRGB result back through `rgb_to_cmyk` and report *that* — it would
/// fill four fields, print identically, and be a **different quantity**:
/// a max-GCR reconstruction of the output rather than a reading of a
/// composite that never happened. `PCS3_132`'s green is the standing
/// example of what that substitution costs (decision 098).
fn probe_ink_screen(x: u32, y: u32, width: u32, height: u32) -> InkProbe {
    InkProbe {
        x,
        y,
        source: if x >= width || y >= height {
            InkProbeSource::OutOfRange
        } else {
            InkProbeSource::ScreenSrgb
        },
        cmyk: None,
        // A page that never held ink has no colorant planes either, and
        // the same argument the doc above makes for `cmyk` applies: an
        // empty roster is the truth, not a placeholder.
        spots: Vec::new(),
        alpha: None,
        srgb: None,
    }
}

fn flatten_page_group_over_white(pixmap: &mut Pixmap) {
    for px in pixmap.pixels_mut() {
        let a = u32::from(px.alpha());
        if a == 255 {
            // Fully covered: white contributes nothing. Skipping is not
            // just an optimisation — it guarantees byte-identical output
            // for the overwhelmingly common opaque case.
            continue;
        }
        let add = 255 - a;
        // Saturating because the sum is bounded by 255 by construction
        // (`stored ≤ a` for a valid premultiplied pixel), but a malformed
        // buffer must not panic in a release render.
        let mix = |c: u8| u8::try_from(u32::from(c) + add).unwrap_or(255);
        *px = tiny_skia::PremultipliedColorU8::from_rgba(
            mix(px.red()),
            mix(px.green()),
            mix(px.blue()),
            255,
        )
        .unwrap_or(*px);
    }
}

/// Map a user-space region onto the page's device grid: the raster size it
/// needs, and the device-space origin its top-left corner sits at.
///
/// Returns `None` when the mapping is not finite — a degenerate or
/// non-invertible page CTM — which the caller reports as a bad raster size
/// rather than guessing at a rectangle.
///
/// # Why the four corners are mapped rather than two opposite points
///
/// Because `/Rotate` 90/270 swaps the axes. A two-point mapping is correct
/// for the unrotated case and silently **transposed** for the odd
/// quarter-turns — the kind of bug that only shows up on landscape scans.
///
/// # Why the origin floors and the extent ceils
///
/// So the requested region is fully **covered** rather than cropped by up
/// to a pixel on each edge. A tiled caller that lost a sub-pixel per tile
/// would show seams along every tile boundary.
///
/// # ★ Why this is a shared function and not two similar blocks
///
/// [`display_list::DisplayList::replay_region`] must land on **exactly**
/// the same rectangle a fresh region render lands on, or "byte-identical to
/// a fresh render" would be a claim about two different rasters. A second
/// implementation of this arithmetic is a second place for the `/Rotate`
/// axis swap or the floor/ceil convention to drift, and the drift would
/// surface as a one-pixel offset that looks like a rounding bug rather than
/// like a duplicated rule.
#[must_use]
pub fn region_device_geometry(
    page_ctm: Transform,
    region: pdfcer_core::page_tree::Rect,
) -> Option<(u32, u32, f32, f32)> {
    #[allow(clippy::cast_possible_truncation)]
    let corners = [
        (region.llx as f32, region.lly as f32),
        (region.urx as f32, region.lly as f32),
        (region.urx as f32, region.ury as f32),
        (region.llx as f32, region.ury as f32),
    ];
    let mapped: Vec<(f32, f32)> = corners
        .iter()
        .map(|&(x, y)| {
            let mut p = [tiny_skia::Point::from_xy(x, y)];
            page_ctm.map_points(&mut p);
            (p[0].x, p[0].y)
        })
        .collect();
    let min_x = mapped.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
    let max_x = mapped.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
    let min_y = mapped.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
    let max_y = mapped.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
    if !min_x.is_finite() || !min_y.is_finite() {
        return None;
    }
    let x0 = min_x.floor();
    let y0 = min_y.floor();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let w = (max_x.ceil() - x0).max(0.0) as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let h = (max_y.ceil() - y0).max(0.0) as u32;
    Some((w, h, x0, y0))
}

/// The device pixmap size and base CTM for a **region** of a page, with
/// every intermediate computed in `f64`.
///
/// # ★ Why this exists rather than composing two `Transform`s
///
/// [`region_device_geometry`] maps the region's corners through the page's
/// `f32` `Transform` and the caller then `post_translate`s by the result.
/// Both steps are exact at ordinary magnifications and **both fail in the
/// same way at deep zoom**, because they do their arithmetic at the
/// magnitude of the device coordinate rather than at the magnitude of the
/// answer.
///
/// `f32` carries 24 bits of significand, so above 16.7 M the gap between
/// representable values exceeds 1. At a scale of 2.15 M a point 700 units
/// up the page lands at `y = 1.5e9`, where that gap is **128 pixels** — so
/// a 600-pixel-tall viewport is measured as the difference of two numbers
/// quantised to multiples of 128 and comes out **512**. Measured, on a
/// requested 800×600 viewport:
///
/// | scale | raster returned |
/// |---:|---|
/// | 10 000 | 800×602 |
/// | 100 000 | 800×592 |
/// | 1 000 000 | 800×640 |
/// | 2 152 300 | **800×512** |
///
/// The width holds because that test point is near `x = 100`; the height
/// collapses because it is near `y = 700`. **Deep-zoom fidelity therefore
/// depends on where you are on the page**, which is not a property anyone
/// would guess and is the whole reason this is worth a separate function.
///
/// # What the `f64` buys, and where `f32` is still fine
///
/// The rasteriser is `f32` and stays `f32` — that is `tiny_skia`'s type and
/// changing it is not on the table. What matters is that the numbers handed
/// TO it are small: this function subtracts the region's device origin
/// **before** narrowing, so the translation `tiny_skia` receives is the
/// distance from the region's own corner (a few hundred) rather than from
/// the page's (a few billion). The large magnitudes exist only inside `f64`
/// arithmetic here and never reach the transform.
///
/// # Returns
///
/// `(width, height, base_ctm)`, or `None` if the mapping is not finite or
/// the region is empty in device space.
#[must_use]
pub fn region_base_geometry(
    page: &Page,
    scale: f32,
    region: pdfcer_core::page_tree::Rect,
) -> Option<RegionGeometry> {
    region_base_geometry_of(page.crop_box, page.rotate, scale, region)
}

/// Everything a caller needs to rasterise one region of a page.
///
/// A struct rather than a tuple because two of the five fields are only
/// meaningful together with a third: `x0`/`y0` are the region's origin in
/// **page-device** space, which is the space a recorded display list's op
/// bounds and clip masks live in, while `ctm` has already had that origin
/// subtracted out. Handing back five positional values invites a caller to
/// apply the offset twice, and that mistake renders a plausible picture of
/// the wrong part of the page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegionGeometry {
    /// Raster width in device pixels.
    pub width: u32,
    /// Raster height in device pixels.
    pub height: u32,
    /// User space → **region-local** device space.
    ///
    /// ★ This said "the translation is the distance from the region's own
    /// corner, so it stays small at any magnification". **It is not
    /// small.** For the usual case of a `CropBox` whose origin is `(0,0)`,
    /// the translation IS `-x0` — the region's device origin — which at a
    /// scale of 8 million is about `-4.4e9`. What stays small is the
    /// RESULT of applying it to a point inside the region, which is a
    /// different quantity and is the one the sentence was reaching for.
    ///
    /// The distinction is not pedantic: a translation of `-4.4e9` in `f32`
    /// is quantised in steps of **512 device pixels**, so a page seeded
    /// from this field alone lands hundreds of pixels from where it
    /// belongs. Use [`Self::ctm64`] for that; this field is for callers
    /// that hand a transform straight to `tiny_skia`.
    pub ctm: Transform,
    /// The same transform in `f64`, un-narrowed.
    ///
    /// The coefficients here are what [`region_base_geometry_of`] computes
    /// before it narrows — so seeding a graphics state from this rather
    /// than from [`Self::ctm`] is the difference between composing a
    /// deep-zoom page against an exact origin and composing it against one
    /// rounded to the nearest 512 pixels.
    pub ctm64: crate::gstate::Mat64,
    /// The region's left edge in page-device space.
    pub x0: f32,
    /// The region's top edge in page-device space.
    pub y0: f32,
}

/// [`region_base_geometry`] against a page box and rotation given
/// directly, for callers that hold those rather than a [`Page`].
///
/// # Why this split exists
///
/// A [`crate::display_list::DisplayList`] outlives the `Page` it was
/// recorded from, and its `replay_region` must compute **exactly** the
/// rectangle a fresh region render would — its own comment says so, and
/// says why: *"a second implementation of the corner mapping is a second
/// place for the `/Rotate` axis swap to be got wrong, and 'byte-identical
/// to a fresh region render' would then be a claim about two different
/// rectangles."*
///
/// That comment was written when both paths called
/// [`region_device_geometry`]. Moving the direct path to `f64` **broke the
/// invariant it asserts**, silently, in the direction the comment predicts
/// — so the fix is this shared entry point rather than a second `f64`
/// implementation on the replay side.
#[must_use]
pub fn region_base_geometry_of(
    crop: pdfcer_core::page_tree::Rect,
    rotate: u16,
    scale: f32,
    region: pdfcer_core::page_tree::Rect,
) -> Option<RegionGeometry> {
    // ★ NARROW TO `f32` FIRST, EXACTLY AS `page_device_geometry` DOES, then
    // widen. This looks like a pointless round trip and is the difference
    // between a fix and a regression.
    //
    // The whole-page path truncates the `CropBox` to `f32` before it
    // multiplies. Computing here from the `f64` box instead produces
    // slightly different corner positions, which floor/ceil to a different
    // pixel on some pages — and poster tiling asserts that tiles rendered
    // as regions REASSEMBLE into the whole-page crop. That test went red on
    // the first version of this function, which is exactly what it is for.
    //
    // So the coefficients here are bit-for-bit the page path's. The `f64`
    // is spent on the two places it actually buys something: mapping the
    // corners, and subtracting the region origin from the translation.
    let (llx, lly, urx, ury) = (
        f64::from(crop.llx as f32),
        f64::from(crop.lly as f32),
        f64::from(crop.urx as f32),
        f64::from(crop.ury as f32),
    );
    let s = f64::from(scale.max(0.0));

    // The same four derivations `page_device_geometry` encodes, in `f64`.
    // Kept as explicit coefficient tuples rather than reusing that
    // function's `Transform`, because narrowing to `f32` is exactly the
    // step this function exists to postpone.
    let (sx, ky, kx, sy, tx, ty) = match rotate {
        90 => (0.0, s, s, 0.0, -lly * s, -llx * s),
        180 => (-s, 0.0, 0.0, s, urx * s, -lly * s),
        270 => (0.0, -s, -s, 0.0, ury * s, urx * s),
        _ => (s, 0.0, 0.0, -s, -llx * s, ury * s),
    };
    // Plain multiply-add, NOT `mul_add`. The fused form rounds once where
    // this rounds twice, and `tiny_skia::Transform::map_points` — which the
    // previous implementation used and which poster tiling's whole-page
    // comparison is calibrated against — is unfused. A fused intermediate
    // differs in the last bit, which floors to a different pixel often
    // enough to shift a tile.
    let map = |x: f64, y: f64| (sx * x + kx * y + tx, ky * x + sy * y + ty);
    // ★ THE REGION'S CORNERS STAY `f64`, and this is the opposite of the
    // treatment the PAGE BOX gets six lines above. The asymmetry is the
    // measurement:
    //
    // A deep-zoom region is a TINY INTERVAL AROUND A LARGE COORDINATE. At
    // a scale of 4.3 M an 800-pixel viewport is 1.86e-4 pt wide, so its
    // corners are 100.0 ± 9.3e-5 — and `f32` near 100 has a spacing of
    // 7.6e-6, which resolves that half-width to about one part in twelve.
    // Narrowing here was tried and cost the viewport its shape again
    // (a requested 800x600 came back as 790x526 at 4.3 M).
    //
    // The page BOX is different in kind: its corners are the page's own
    // extent, which is what `page_device_geometry` narrows, and matching
    // that bit-for-bit is what keeps a region render byte-identical to a
    // crop of the whole page.
    //
    // ⇒ Narrow what the other path narrows; keep full precision on what
    // only this path sees.
    let corners = [
        map(region.llx, region.lly),
        map(region.urx, region.lly),
        map(region.urx, region.ury),
        map(region.llx, region.ury),
    ];
    let min_x = corners.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let max_x = corners
        .iter()
        .map(|p| p.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max_y = corners
        .iter()
        .map(|p| p.1)
        .fold(f64::NEG_INFINITY, f64::max);
    if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
        return None;
    }
    let (x0, y0) = (min_x.floor(), min_y.floor());
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let w = (max_x.ceil() - x0).max(0.0) as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let h = (max_y.ceil() - y0).max(0.0) as u32;
    if w == 0 || h == 0 {
        return None;
    }
    // ★ The subtraction happens HERE, in `f64`, and only its small result
    // is narrowed. Doing it the other way round -- narrowing `tx` and `x0`
    // and subtracting in `f32` -- is arithmetically identical and
    // numerically useless, because both operands are the large number.
    let ctm64 = crate::gstate::Mat64::from_row(sx, ky, kx, sy, tx - x0, ty - y0);
    // Narrowed here, and ALSO kept whole. The `f32` copy is what
    // `tiny_skia` consumes at the leaves; the `f64` copy is what a
    // content stream's own `cm` operators compose against, and composing
    // against the narrowed one throws away the precision this function
    // spent its whole body protecting, one call later.
    let ctm = ctm64.to_f32();
    #[allow(clippy::cast_possible_truncation)]
    Some(RegionGeometry {
        width: w,
        height: h,
        ctm,
        ctm64,
        x0: x0 as f32,
        y0: y0 as f32,
    })
}

/// Compute the device pixmap size and base CTM for a page at `scale`
/// (module docs: CropBox → origin, y-flip, clockwise `/Rotate` with
/// axis swap for the odd quarter-turns).
#[must_use]
pub fn page_device_geometry(page: &Page, scale: f32) -> (u32, u32, Transform) {
    let crop = page.crop_box;
    let (llx, lly, urx, ury) = (
        crop.llx as f32,
        crop.lly as f32,
        crop.urx as f32,
        crop.ury as f32,
    );
    let s = scale.max(0.0);
    let w = ((urx - llx) * s).ceil().max(0.0) as u32;
    let h = ((ury - lly) * s).ceil().max(0.0) as u32;

    // Derivations (user (x,y) → device (x', y'), y-down):
    //   unrotated: x' = (x−llx)·s          y' = (ury−y)·s
    //   90° CW:    x' = (y−lly)·s          y' = (x−llx)·s
    //   180°:      x' = (urx−x)·s          y' = (y−lly)·s
    //   270° CW:   x' = (ury−y)·s          y' = (urx−x)·s
    // Transform::from_row(sx, ky, kx, sy, tx, ty) encodes
    //   x' = sx·x + kx·y + tx ;  y' = ky·x + sy·y + ty
    // (the §8.3 RAG note's "verify tiny-skia's row order at
    // implementation time" is discharged by the pixel-level tests
    // below).
    match page.rotate {
        90 => (
            h,
            w,
            Transform::from_row(0.0, s, s, 0.0, -lly * s, -llx * s),
        ),
        180 => (
            w,
            h,
            Transform::from_row(-s, 0.0, 0.0, s, urx * s, -lly * s),
        ),
        270 => (
            h,
            w,
            Transform::from_row(0.0, -s, -s, 0.0, ury * s, urx * s),
        ),
        _ => (
            w,
            h,
            Transform::from_row(s, 0.0, 0.0, -s, -llx * s, ury * s),
        ),
    }
}

// ---------------------------------------------------------------------------
// Shared image-codec test fixtures — declared HERE, at file scope, and NOT
// inside `mod tests` (see the paragraph marked ★)
// ---------------------------------------------------------------------------
//
// These three files live in `pdfcer-core` and are `#[cfg(test)]`-only there,
// so they are not reachable as a normal cross-crate path. `#[path]` loads
// each as a real module file, which is what lets it keep the `//!` module
// docs and the GENERATED-FILE banner it opens with — `include!` cannot,
// because an inner attribute has to be lexically first in a block and a
// macro expansion never is. That part of the original reasoning was right.
//
// ★ WHAT WAS WRONG WAS THE PLACEMENT: these sat INSIDE the inline
// `mod tests` below, and that did not compile on Linux.
//
// `mod tests` is inline in `lib.rs`, so Rust resolves a `#[path]` written
// inside it relative to a phantom `src/tests/` directory that does not exist
// on disk. Windows collapses the `..` components lexically and never touches
// it; Linux resolves a path one component at a time and returns ENOENT on
// the missing directory. `src/tests/../../../pdfcer-core/...` is lexically
// correct and still fails to open.
//
// Measured on the public repository, 2026-08-09: SIX OF SIX CI RUNS RED,
// with `cargo test (windows-latest)` GREEN beside `cargo test
// (ubuntu-latest)`, `cargo clippy` and `cargo fmt --check` all failing on
// `couldn't read .../src/tests/../../../pdfcer-core/src/image_codec/fixtures.rs`.
// A green Windows job next to a red Linux one is this bug's signature, and
// that split is exactly why it survived every local run.
//
// At file scope the base directory is `src/`, which is real, so one `..`
// comes off each path and Linux can resolve them.
#[cfg(test)]
#[path = "../../pdfcer-core/src/image_codec/fixtures.rs"]
mod jpeg_fixtures;

#[cfg(test)]
#[path = "../../pdfcer-core/src/image_codec/fixtures_bilevel.rs"]
mod bilevel_fixtures;

#[cfg(test)]
#[path = "../../pdfcer-core/src/image_codec/fixtures_jpx.rs"]
mod jpx_fixtures;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use pdfcer_core::object::ObjId;

    /// Assemble a classic (§7.5.4 cross-reference *table*) PDF from
    /// numbered object bodies and return its first page.
    ///
    /// `objects` must be numbered 1..=n contiguously, with object 1 the
    /// catalog — the xref section is generated from that assumption.
    /// Bodies are raw bytes so that image streams (which are binary by
    /// nature) can be built by the same helper as dictionaries.
    fn build_pdf(objects: &[(u32, Vec<u8>)]) -> (Document, Page) {
        let mut buf = b"%PDF-1.4\n".to_vec();
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        for (num, body) in objects {
            offsets.push((*num, buf.len()));
            buf.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            buf.extend_from_slice(body);
            buf.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = buf.len();
        let size = objects.len() + 1;
        buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
        for num in 1..=objects.len() as u32 {
            let (_, off) = offsets.iter().find(|(n, _)| *n == num).unwrap();
            buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
        }
        buf.extend_from_slice(
            format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
                .as_bytes(),
        );
        let doc = Document::from_bytes(buf).unwrap();
        let page = pdfcer_core::page_tree::pages(&doc).unwrap().remove(0);
        (doc, page)
    }

    /// A stream object body: `<< dict_extra /Length N >> stream … endstream`.
    fn stream_object(dict_extra: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {dict_extra} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream");
        out
    }

    /// Build a one-page document whose content stream is `content`,
    /// with a 100×100 MediaBox (`page_extra` appends page attrs).
    fn doc_with_content(content: &str, page_extra: &str) -> (Document, Page) {
        doc_with_extra_objects(content, page_extra, &[])
    }

    /// As [`doc_with_content`], plus caller-supplied objects numbered
    /// from **5** upward (1–4 are catalog, page tree, page, contents).
    fn doc_with_extra_objects(
        content: &str,
        page_extra: &str,
        extra: &[(u32, Vec<u8>)],
    ) -> (Document, Page) {
        let mut objects: Vec<(u32, Vec<u8>)> = vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 \
                  /MediaBox [0 0 100 100] /Resources << >> >>"
                    .to_vec(),
            ),
            (
                3,
                format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R {page_extra} >>")
                    .into_bytes(),
            ),
            (4, stream_object("", content.as_bytes())),
        ];
        objects.extend_from_slice(extra);
        build_pdf(&objects)
    }

    /// A one-page document with a single XObject resource named `/X1`
    /// (object 5) built from `xobj_dict` + `xobj_data`.
    fn doc_with_xobject(content: &str, xobj_dict: &str, xobj_data: &[u8]) -> (Document, Page) {
        doc_with_extra_objects(
            content,
            "/Resources << /XObject << /X1 5 0 R >> >>",
            &[(5, stream_object(xobj_dict, xobj_data))],
        )
    }

    /// `cm` that maps the user-space unit square (§8.9.4) onto the whole
    /// 100×100 test page, so a 2×2 image's four samples land in the four
    /// 50×50 quadrants and can be asserted exactly.
    const FULL_PAGE_CM: &str = "100 0 0 100 0 0 cm";

    /// A one-page document with a `/Font` resource named `F1`, for the
    /// text tests. The page's OWN `/Resources` wins over the inherited
    /// empty one (§7.7.3.3), so this is a complete override.
    fn doc_with_font(content: &str, font_dict: &str) -> (Document, Page) {
        doc_with_content(
            content,
            &format!("/Resources << /Font << /F1 {font_dict} >> >>"),
        )
    }

    /// A non-embedded standard-14 Helvetica, the minimum legal font
    /// dictionary (§9.6.2.2: `/Widths` and `/FontDescriptor` are
    /// optional for the standard 14).
    const HELVETICA: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";

    /// The same, carrying an explicit width for `H` (code 72) so the
    /// advance tests do not depend on the AFM width tables, which live
    /// in `pdfcer-core` (decision 004 §7). `722` is Helvetica's real AFM
    /// advance for `H`.
    const HELVETICA_H_722: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /FirstChar 72 /LastChar 72 /Widths [722] >>";

    fn pixel(pm: &Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
        let p = pm.pixel(x, y).unwrap();
        (p.red(), p.green(), p.blue())
    }

    /// Bounding box `(min_x, min_y, max_x, max_y)` of every non-white
    /// pixel, or `None` if the page is blank.
    ///
    /// Glyph tests assert against ink EXTENTS rather than named pixels:
    /// a specific pixel's colour depends on the substitute face's exact
    /// outlines, but "the second glyph starts to the right of the
    /// first" and "doubling the matrix doubles the height" are
    /// properties of the text engine, not of the face.
    fn ink_bbox(pm: &Pixmap) -> Option<(u32, u32, u32, u32)> {
        let mut bbox: Option<(u32, u32, u32, u32)> = None;
        for y in 0..pm.height() {
            for x in 0..pm.width() {
                if pixel(pm, x, y) != (255, 255, 255) {
                    bbox = Some(match bbox {
                        None => (x, y, x, y),
                        Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                    });
                }
            }
        }
        bbox
    }

    #[test]
    fn filled_rectangle_lands_where_the_math_says() {
        // Fill the lower-left quadrant red. In device space (y-down)
        // that is the BOTTOM-left of the image.
        let (doc, page) = doc_with_content("1 0 0 rg 0 0 50 50 re f", "");
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.pixmap.width(), 100);
        // Inside the rect (device y 75 = user y 25):
        assert_eq!(pixel(&out.pixmap, 25, 75), (255, 0, 0));
        // Top-left quadrant is paper white:
        assert_eq!(pixel(&out.pixmap, 25, 25), (255, 255, 255));
        assert_eq!(out.diagnostics.unknown_ops, 0);
    }

    #[test]
    fn cm_premultiplies_translation_then_scale() {
        // `2 0 0 2 0 0 cm` then `1 0 0 1 10 0 cm`: the second cm's
        // translation happens in the ALREADY-SCALED space, so a rect
        // at x=0 lands at device x = 2·10 = 20, not 10 (§8.3.4 order).
        let (doc, page) =
            doc_with_content("2 0 0 2 0 0 cm 1 0 0 1 10 0 cm 0 0 0 rg 0 0 10 50 re f", "");
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(pixel(&out.pixmap, 25, 95), (0, 0, 0)); // 20..40 black
        assert_eq!(pixel(&out.pixmap, 15, 95), (255, 255, 255)); // 10..20 white
    }

    #[test]
    fn q_restores_state() {
        let (doc, page) = doc_with_content("q 1 0 0 rg 0 0 10 10 re f Q 0 60 10 10 re f", "");
        let out = render_page(&doc, &page, 1.0).unwrap();
        // First rect red; second painted AFTER Q restored black fill.
        assert_eq!(pixel(&out.pixmap, 5, 95), (255, 0, 0));
        assert_eq!(pixel(&out.pixmap, 5, 35), (0, 0, 0));
    }

    #[test]
    fn evenodd_vs_nonzero_ring() {
        // Two nested same-direction rects: nonzero fills both;
        // even-odd leaves the ring only (§8.5.3.3.2 / §8.5.3.3.3).
        let ring = "0 0 0 rg 10 10 80 80 re 30 30 40 40 re";
        let (doc, page) = doc_with_content(&format!("{ring} f*"), "");
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(pixel(&out.pixmap, 50, 50), (255, 255, 255)); // hole
        assert_eq!(pixel(&out.pixmap, 15, 50), (0, 0, 0)); // ring

        let (doc, page) = doc_with_content(&format!("{ring} f"), "");
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(pixel(&out.pixmap, 50, 50), (0, 0, 0)); // filled
    }

    #[test]
    fn deferred_clip_applies_after_paint() {
        // `re W n` sets the clip WITHOUT painting; the following
        // full-page fill is clipped to it (§8.5.4).
        let (doc, page) = doc_with_content("0 0 60 60 re W n 1 0 0 rg 0 0 100 100 re f", "");
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(pixel(&out.pixmap, 30, 70), (255, 0, 0)); // inside
        assert_eq!(pixel(&out.pixmap, 80, 20), (255, 255, 255)); // outside
    }

    #[test]
    fn page_rotate_90_swaps_dimensions_and_turns_content() {
        // A rect hugging the LEFT edge hugs the TOP edge after 90° CW
        // display rotation (Table 30).
        let (doc, page) = doc_with_content("0 0 0 rg 0 0 10 100 re f", "/Rotate 90");
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!((out.pixmap.width(), out.pixmap.height()), (100, 100));
        assert_eq!(pixel(&out.pixmap, 50, 5), (0, 0, 0)); // top band
        assert_eq!(pixel(&out.pixmap, 50, 50), (255, 255, 255));
    }

    /// ★ A region of a ROTATED page is still the crop of the rotated page.
    ///
    /// This test lives here, as a unit test over the in-memory
    /// [`doc_with_content`] helper, for a reason worth recording: **no fixture
    /// in `fixtures/synthetic/` carries a `/Rotate` key at all.** A companion
    /// integration test was written first, found nothing to load, and skipped
    /// — reporting success while testing nothing. The helper can set
    /// `/Rotate 90` directly, so the coverage is real here and unreachable
    /// there.
    ///
    /// What it pins: `render_page_region` maps all **four** corners of the
    /// requested region through the page CTM and takes their device-space
    /// bounding box. Mapping only two opposite corners is correct for an
    /// unrotated page and **silently transposed** at 90° and 270°, because
    /// those quarter-turns swap the axes. That defect would render the wrong
    /// part of a landscape scan while looking entirely plausible.
    #[test]
    fn a_region_of_a_rotated_page_is_the_crop_of_the_rotated_page() {
        // Same content and rotation as `page_rotate_90_swaps_dimensions...`:
        // a rect hugging the LEFT edge, which after 90° CW display rotation
        // hugs the TOP edge. The full page is 100x100 device px at scale 1.
        let (doc, page) = doc_with_content("0 0 0 rg 0 0 10 100 re f", "/Rotate 90");
        let full = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!((full.pixmap.width(), full.pixmap.height()), (100, 100));

        // The whole page as a region must reproduce the whole page exactly —
        // the strongest single statement available, since it exercises the
        // corner mapping under rotation and compares against a known-good
        // raster rather than against itself.
        let whole = pdfcer_core::page_tree::Rect::from_corners(0.0, 0.0, 100.0, 100.0);
        let got =
            render_page_region(&doc.view(), &page, 1.0, whole, &RenderOptions::default()).unwrap();
        assert_eq!(
            (got.pixmap.width(), got.pixmap.height()),
            (100, 100),
            "the region covering the whole rotated page must have the ROTATED \
             page's dimensions; a transposed mapping shows up here first"
        );
        assert_eq!(
            got.pixmap.data(),
            full.pixmap.data(),
            "and identical pixels — a two-corner mapping would transpose them"
        );

        // Now a genuine sub-region: the top-left quarter of the DEVICE image,
        // which under 90° CW comes from the page-space rect x∈[0,50], y∈[50,100].
        let quarter = pdfcer_core::page_tree::Rect::from_corners(0.0, 50.0, 50.0, 100.0);
        let tile = render_page_region(&doc.view(), &page, 1.0, quarter, &RenderOptions::default())
            .unwrap();
        assert_eq!((tile.pixmap.width(), tile.pixmap.height()), (50, 50));
        // The black band sits along the top of the rotated page, so this tile
        // is black at its top rows and white further down.
        assert_eq!(pixel(&tile.pixmap, 25, 5), (0, 0, 0), "top band present");
        assert_eq!(
            pixel(&tile.pixmap, 25, 45),
            (255, 255, 255),
            "and white below it"
        );
    }

    #[test]
    fn stroke_uses_stroke_color_and_width() {
        let (doc, page) = doc_with_content("0 0 1 RG 4 w 20 20 m 80 20 l S", "");
        let out = render_page(&doc, &page, 1.0).unwrap();
        // Horizontal line at user y=20 (device y=80), blue, ~4 wide.
        assert_eq!(pixel(&out.pixmap, 50, 80), (0, 0, 255));
        assert_eq!(pixel(&out.pixmap, 50, 70), (255, 255, 255));
    }

    // ---------------------------------------------------------------
    // Text rendering (§9.3, §9.4; decision 004 §4.3)
    // ---------------------------------------------------------------

    #[test]
    fn std14_text_paints_and_discloses_the_substitution() {
        // A non-embedded standard-14 font: real glyphs get painted, and
        // rule R20 requires the operator to be able to tell they came
        // from a bundled face rather than the document's own program.
        let (doc, page) = doc_with_font("BT /F1 48 Tf 10 30 Td (Hi) Tj ET", HELVETICA);
        let out = render_page(&doc, &page, 1.0).unwrap();

        let bbox = ink_bbox(&out.pixmap).expect("text painted no pixels");
        // Ink sits above the baseline at user y=30 (device y=70) and to
        // the right of the pen at user x=10.
        assert!(bbox.0 >= 9, "ink starts left of the pen: {bbox:?}");
        assert!(bbox.3 <= 72, "ink falls below the baseline: {bbox:?}");

        assert_eq!(out.diagnostics.unknown_ops, 0);
        assert!(out.diagnostics.glyphs_substituted >= 2, "R20 must count");
        assert_eq!(out.diagnostics.substituted_fonts, vec!["Helvetica"]);
        assert_eq!(out.diagnostics.fonts_unsupported, 0);
    }

    #[test]
    fn tj_advances_the_pen_by_the_glyph_width() {
        // §9.4.4: tx = ((w0 − Tj/1000)·Tfs + Tc + Tw)·Th. With w0 =
        // 0.722, Tfs = 20 and everything else default, the second `H`
        // must start 14.44 user units right of the first.
        let one = render_page(
            &doc_with_font("BT /F1 20 Tf 5 40 Td (H) Tj ET", HELVETICA_H_722).0,
            &doc_with_font("BT /F1 20 Tf 5 40 Td (H) Tj ET", HELVETICA_H_722).1,
            1.0,
        )
        .unwrap();
        let (doc, page) = doc_with_font("BT /F1 20 Tf 5 40 Td (H) Tj (H) Tj ET", HELVETICA_H_722);
        let two = render_page(&doc, &page, 1.0).unwrap();

        let b1 = ink_bbox(&one.pixmap).unwrap();
        let b2 = ink_bbox(&two.pixmap).unwrap();
        // Same left edge: the first glyph is unmoved.
        assert_eq!(b1.0, b2.0, "first glyph moved: {b1:?} vs {b2:?}");
        // The second glyph's right edge is ~14.44 further right.
        let delta = i64::from(b2.2) - i64::from(b1.2);
        assert!(
            (12..=17).contains(&delta),
            "advance was {delta} px, expected ~14 ({b1:?} vs {b2:?})"
        );
    }

    #[test]
    fn std14_without_widths_uses_the_afm_metrics() {
        // §9.6.2.2: the standard 14 may omit `/Widths` entirely, in
        // which case the advances come from the AFM tables in
        // `pdfcer_core::fontdata` — keyed by GLYPH NAME, so this
        // exercises the whole encoding → name → width chain, not just
        // an array index. Helvetica's `H` is 722 either way, so the two
        // font dictionaries must render identically.
        let render = |font: &str| {
            let (doc, page) = doc_with_font("BT /F1 20 Tf 5 40 Td (H) Tj (H) Tj ET", font);
            let out = render_page(&doc, &page, 1.0).unwrap();
            ink_bbox(&out.pixmap).unwrap()
        };
        assert_eq!(render(HELVETICA), render(HELVETICA_H_722));
    }

    #[test]
    fn tj_array_numbers_are_subtracted_in_thousandths() {
        // Table 109: "This amount shall be SUBTRACTED from the current
        // horizontal coordinate… a positive adjustment has the effect
        // of moving the next glyph painted to the LEFT."
        let render = |content: &str| {
            let (doc, page) = doc_with_font(content, HELVETICA_H_722);
            let out = render_page(&doc, &page, 1.0).unwrap();
            ink_bbox(&out.pixmap).unwrap()
        };
        let plain = render("BT /F1 20 Tf 20 40 Td [(H) 0 (H)] TJ ET");
        let left = render("BT /F1 20 Tf 20 40 Td [(H) 500 (H)] TJ ET");
        let right = render("BT /F1 20 Tf 20 40 Td [(H) -500 (H)] TJ ET");

        // 500/1000 × 20 = 10 user units, positive → left.
        assert!(
            left.2 < plain.2 && plain.2 < right.2,
            "TJ sign wrong: left={left:?} plain={plain:?} right={right:?}"
        );
        let shift = i64::from(right.2) - i64::from(plain.2);
        assert!((8..=12).contains(&shift), "shift was {shift} px, want ~10");
    }

    #[test]
    fn render_mode_3_is_invisible_but_still_valid() {
        // Table 106 mode 3 — how OCR text layers are made invisible.
        // The page must stay blank AND the stream must not be treated
        // as broken.
        let (doc, page) = doc_with_font("BT /F1 48 Tf 3 Tr 10 30 Td (Hi) Tj ET", HELVETICA);
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(ink_bbox(&out.pixmap), None, "mode 3 painted something");
        assert_eq!(out.diagnostics.unknown_ops, 0);
        assert_eq!(out.diagnostics.tolerated, 0);
        assert_eq!(out.diagnostics.glyphs_notdef, 0);
    }

    #[test]
    fn tm_vertical_scale_scales_the_glyph() {
        // §9.4.4: Trm = [Tfs·Th, 0, 0, Tfs, 0, Ts] × Tm × CTM, so a Tm
        // with d = 2 doubles the painted glyph height. (Tm REPLACES the
        // text matrix — Table 108 — so no `Td` is needed.)
        let render = |m: &str| {
            let (doc, page) =
                doc_with_font(&format!("BT /F1 20 Tf {m} Tm (H) Tj ET"), HELVETICA_H_722);
            let out = render_page(&doc, &page, 1.0).unwrap();
            let b = ink_bbox(&out.pixmap).unwrap();
            b.3 - b.1 + 1
        };
        let single = render("1 0 0 1 10 20");
        let double = render("1 0 0 2 10 20");
        assert!(
            double >= single * 2 - 2 && double <= single * 2 + 2,
            "height {single} → {double}, expected ~2×"
        );
    }

    #[test]
    fn q_restores_the_text_state() {
        // §9.3: "the text state comprises those GRAPHICS STATE
        // parameters that only affect text" — so `Tf` (font AND size)
        // is saved by `q` and restored by `Q`, exactly like line width.
        let render = |content: &str| {
            let (doc, page) = doc_with_font(content, HELVETICA_H_722);
            let out = render_page(&doc, &page, 1.0).unwrap();
            let b = ink_bbox(&out.pixmap).unwrap();
            b.3 - b.1 + 1
        };
        // Baseline: 20 pt throughout.
        let small = render("BT /F1 20 Tf 10 20 Td (H) Tj ET");
        // A 60 pt `Tf` inside q…Q must NOT survive the `Q`.
        let restored = render("BT /F1 20 Tf q /F1 60 Tf Q 10 20 Td (H) Tj ET");
        // Control: without the `Q`, the glyph really is 3× taller —
        // proving the test can tell the difference.
        let big = render("BT /F1 20 Tf q /F1 60 Tf 10 20 Td (H) Tj ET");

        assert_eq!(small, restored, "Q did not restore Tfs");
        assert!(big > restored * 2, "control failed: {big} vs {restored}");
    }

    #[test]
    fn text_rendering_is_deterministic() {
        // Rule R19: same input → same pixels. Nothing in the text path
        // may consult the filesystem, the environment, or a hash-order
        // -dependent iteration.
        let (doc, page) = doc_with_font(
            "BT /F1 24 Tf 5 60 Td (Hello) Tj 0 -30 Td (World) Tj ET",
            HELVETICA,
        );
        let a = render_page(&doc, &page, 1.5).unwrap();
        let b = render_page(&doc, &page, 1.5).unwrap();
        assert_eq!(a.pixmap.data(), b.pixmap.data());
        assert!(ink_bbox(&a.pixmap).is_some());
    }

    #[test]
    fn showing_text_without_tf_is_skipped_not_guessed() {
        // §9.3 Table 105: `Tf` has NO initial value. Substituting a
        // font here would be "sneaky"; skipping + diagnosing is right.
        let (doc, page) = doc_with_font("BT 10 10 Td (Hi) Tj ET", HELVETICA);
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(ink_bbox(&out.pixmap), None);
        assert!(out.diagnostics.tolerated >= 1);
        assert_eq!(out.diagnostics.unknown_ops, 0);
    }

    /// §9.6.6.3: a Type 3 font with an EMPTY encoding paints nothing —
    /// and is not thereby an unsupported font.
    ///
    /// ★ This test asserted `fonts_unsupported == 1` until `Pass 126.0`,
    /// citing decision 004 §4.3's deferred list. That was correct for as
    /// long as Type 3 was deferred and became a claim about the renderer
    /// that the renderer contradicted — `R212`. It is rewritten rather
    /// than deleted, because the FILE it describes is interesting for a
    /// new reason.
    ///
    /// §9.6.6.3 makes a Type 3 font's code-to-name mapping "entirely
    /// defined by its `Encoding` entry", with no built-in encoding to
    /// fall back on and no meaningful `StandardEncoding` base (the
    /// `CharProcs` keys are arbitrary names). So an empty `/Encoding` is
    /// a font whose every code resolves to nothing — blank page, by the
    /// standard, not by a shortfall.
    ///
    /// The distinction is the whole point: `fonts_unsupported` means
    /// "pdfcer cannot render this font", and reporting it here would send
    /// an operator looking for a missing feature instead of a missing
    /// `/Differences` array.
    #[test]
    fn a_type3_font_with_an_empty_encoding_paints_nothing_and_is_not_unsupported() {
        let (doc, page) = doc_with_font(
            "BT /F1 24 Tf 10 10 Td (Hi) Tj ET",
            "<< /Type /Font /Subtype /Type3 /FontMatrix [0.001 0 0 0.001 0 0] \
             /CharProcs << >> /Encoding << >> >>",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(ink_bbox(&out.pixmap), None, "no code has a glyph name");
        assert_eq!(
            out.diagnostics.fonts_unsupported, 0,
            "the font loaded; it simply has no glyphs to show"
        );
        assert_eq!(
            out.diagnostics.type3_glyphs_missing, 2,
            "both codes of \"Hi\" must be reported as glyphs that do not exist"
        );
        assert_eq!(out.diagnostics.type3_glyph_procs_run, 0);
    }

    /// A Type 3 font missing Table 112's IRREDUCIBLE entries is still
    /// refused, and this is the pair that keeps the test above honest.
    ///
    /// Without it, "not unsupported" could be read as "pdfcer never
    /// refuses a Type 3 font", which would be a different and much worse
    /// property: a font with no `/CharProcs` has no glyph descriptions
    /// anywhere, and a font with no `/FontMatrix` has no mapping from
    /// glyph space to text space — guessing the conventional
    /// `[0.001 …]` would render a nonstandard font at a thousand times
    /// the wrong size.
    #[test]
    fn a_type3_font_without_charprocs_is_still_refused_by_name() {
        let (doc, page) = doc_with_font(
            "BT /F1 24 Tf 10 10 Td (Hi) Tj ET",
            "<< /Type /Font /Subtype /Type3 /FontMatrix [0.001 0 0 0.001 0 0] \
             /Encoding << >> >>",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(ink_bbox(&out.pixmap), None);
        assert_eq!(out.diagnostics.fonts_unsupported, 1);
        assert_eq!(
            out.diagnostics
                .fonts_unsupported_by_reason
                .get("Type3")
                .copied(),
            Some(1),
            "the refusal must be NAMED, not merely counted"
        );
    }

    #[test]
    fn non_identity_cmap_is_counted_unsupported() {
        // §9.7.5's predefined CJK CMaps are deferred WITH the licensing
        // check on Adobe's CMap resources (decision 004 §4.3).
        let (doc, page) = doc_with_font(
            "BT /F1 24 Tf 10 10 Td <0041> Tj ET",
            "<< /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /90ms-RKSJ-H \
             /DescendantFonts [] >>",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(ink_bbox(&out.pixmap), None);
        assert_eq!(out.diagnostics.fonts_unsupported, 1);
    }

    // ---------------------------------------------------------------
    // Operator-supplied fonts (decision 012) — the three trust levels,
    // the FontEnvironment.named seam, subset-tag/style resolution, and
    // the positions-from-/Widths invariant.
    // ---------------------------------------------------------------

    /// A non-embedded simple TrueType referencing `Calibri` (a font
    /// pdfcer does not bundle), with explicit `/Widths` so the pen advance
    /// is face-INDEPENDENT (decision 004 §3.6) and codes 72/73 = `H`/`I`.
    /// `/Flags 32` = Nonsymbolic, so the StandardEncoding arm resolves
    /// the two letters in any Latin substitute face.
    const CALIBRI_NONEMBEDDED: &str = "<< /Type /Font /Subtype /TrueType /BaseFont /Calibri \
         /FirstChar 72 /LastChar 73 /Widths [600 600] \
         /FontDescriptor << /Type /FontDescriptor /FontName /Calibri /Flags 32 >> >>";

    /// Render `content`/`font` with a caller-built [`FontEnvironment`].
    fn render_with_fonts(
        content: &str,
        font: &str,
        env: crate::font::FontEnvironment,
    ) -> RenderedPage {
        let (doc, page) = doc_with_font(content, font);
        let opts = RenderOptions {
            fonts: env,
            ..RenderOptions::default()
        };
        render_page_with(&doc, &page, 1.0, &opts).unwrap()
    }

    #[test]
    fn nonembedded_font_without_font_dir_is_bundled() {
        // The default (no supplied faces): a non-embedded Calibri draws
        // from a BUNDLED Base-14 face and is disclosed as such — never
        // as supplied, never as embedded (R63).
        let out = render_with_fonts(
            "BT /F1 40 Tf 5 40 Td (HI) Tj ET",
            CALIBRI_NONEMBEDDED,
            crate::font::FontEnvironment::bundled(),
        );
        assert!(ink_bbox(&out.pixmap).is_some(), "text must paint");
        assert!(out.diagnostics.glyphs_substituted >= 2, "bundled counted");
        assert_eq!(out.diagnostics.substituted_fonts, vec!["Calibri"]);
        assert_eq!(out.diagnostics.glyphs_supplied, 0, "nothing supplied");
        assert!(out.diagnostics.supplied_fonts.is_empty());
    }

    #[test]
    fn supplied_face_is_disclosed_as_supplied_not_bundled() {
        // A face registered under the exact /BaseFont name draws from the
        // SUPPLIED level (R63): counted in glyphs_supplied/supplied_fonts,
        // and glyphs_substituted stays zero — the two trust levels never
        // conflate.
        let mut env = crate::font::FontEnvironment::bundled();
        let serif = env
            .fallback(crate::font::FallbackKey::Serif)
            .expect("bundled serif present")
            .clone();
        env.insert_named("Calibri", serif);
        let out = render_with_fonts("BT /F1 40 Tf 5 40 Td (HI) Tj ET", CALIBRI_NONEMBEDDED, env);
        assert!(ink_bbox(&out.pixmap).is_some(), "text must paint");
        assert!(out.diagnostics.glyphs_supplied >= 2, "supplied counted");
        assert_eq!(out.diagnostics.supplied_fonts, vec!["Calibri"]);
        assert_eq!(
            out.diagnostics.glyphs_substituted, 0,
            "a supplied glyph is NEVER counted as bundled"
        );
        assert!(out.diagnostics.substituted_fonts.is_empty());
    }

    #[test]
    fn supplied_face_equal_to_bundled_is_byte_identical_proving_positions() {
        // The decision-004 §3.6 invariant, made airtight: register the
        // SAME face bytes the bundled fallback would use. The two rasters
        // are then BYTE-IDENTICAL — positions AND shapes — so the only
        // thing "supplying" a font changed is the disclosure (bundled →
        // supplied). Because positions come from /Widths, this is the
        // strongest possible statement of "positions are identical across
        // the two runs."
        let bundled_out = render_with_fonts(
            "BT /F1 40 Tf 5 40 Td (HI) Tj ET",
            CALIBRI_NONEMBEDDED,
            crate::font::FontEnvironment::bundled(),
        );
        let mut env = crate::font::FontEnvironment::bundled();
        // What the bundled fallback for a nonsymbolic non-serif is: Sans.
        let same = env
            .fallback(crate::font::FallbackKey::Sans)
            .expect("bundled sans present")
            .clone();
        env.insert_named("Calibri", same);
        let supplied_out =
            render_with_fonts("BT /F1 40 Tf 5 40 Td (HI) Tj ET", CALIBRI_NONEMBEDDED, env);

        assert_eq!(
            bundled_out.pixmap.data(),
            supplied_out.pixmap.data(),
            "supplying the same face as the fallback must not move a single pixel"
        );
        // …but the disclosure flipped from bundled to supplied.
        assert!(bundled_out.diagnostics.glyphs_substituted >= 2);
        assert_eq!(bundled_out.diagnostics.glyphs_supplied, 0);
        assert!(supplied_out.diagnostics.glyphs_supplied >= 2);
        assert_eq!(supplied_out.diagnostics.glyphs_substituted, 0);
    }

    #[test]
    fn supplied_face_advance_matches_bundled_from_widths() {
        // Positions-from-/Widths across DIFFERENT faces: the pen advance
        // between two identical glyphs equals the /Widths value regardless
        // of which face draws. Measure the advance as (two-glyph right
        // edge − one-glyph right edge) within each face — LSB cancels
        // because it is the same letter in the same face — and assert the
        // bundled and supplied advances agree.
        let advance = |env_builder: &dyn Fn() -> crate::font::FontEnvironment| {
            let one = render_with_fonts(
                "BT /F1 40 Tf 5 40 Td (H) Tj ET",
                CALIBRI_NONEMBEDDED,
                env_builder(),
            );
            let two = render_with_fonts(
                "BT /F1 40 Tf 5 40 Td (H) Tj (H) Tj ET",
                CALIBRI_NONEMBEDDED,
                env_builder(),
            );
            let b1 = ink_bbox(&one.pixmap).unwrap();
            let b2 = ink_bbox(&two.pixmap).unwrap();
            i64::from(b2.2) - i64::from(b1.2)
        };
        let bundled = advance(&crate::font::FontEnvironment::bundled);
        let supplied = advance(&|| {
            let mut env = crate::font::FontEnvironment::bundled();
            let serif = env
                .fallback(crate::font::FallbackKey::Serif)
                .unwrap()
                .clone();
            env.insert_named("Calibri", serif);
            env
        });
        // /Widths[600] at 40pt = 24 user units ≈ 24 px. The advance must
        // be the SAME for both faces (positions from /Widths, not shapes).
        assert_eq!(
            bundled, supplied,
            "advance differed by face: bundled={bundled} supplied={supplied}"
        );
    }

    #[test]
    fn supplied_face_matches_through_subset_tag() {
        // decision 012's one behavioral subtlety: a supplied `Calibri`
        // must match a document's subset-tagged `ABCDEF+Calibri`, via the
        // strip_subset_tag retry in substitute_face.
        let tagged = "<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Calibri \
             /FirstChar 72 /LastChar 73 /Widths [600 600] \
             /FontDescriptor << /Type /FontDescriptor /FontName /ABCDEF+Calibri /Flags 32 >> >>";
        let mut env = crate::font::FontEnvironment::bundled();
        let serif = env
            .fallback(crate::font::FallbackKey::Serif)
            .unwrap()
            .clone();
        env.insert_named("Calibri", serif);
        let out = render_with_fonts("BT /F1 40 Tf 5 40 Td (HI) Tj ET", tagged, env);
        assert!(
            out.diagnostics.glyphs_supplied >= 2,
            "subset tag must match"
        );
        // The name disclosed is the verbatim /BaseFont (subset tag and
        // all) — that is what the operator sees in the document.
        assert_eq!(out.diagnostics.supplied_fonts, vec!["ABCDEF+Calibri"]);
        assert_eq!(out.diagnostics.glyphs_substituted, 0);
    }

    #[test]
    fn unmatched_variant_falls_to_bundled_and_is_disclosed() {
        // A supplied `Calibri` does NOT cover a document's `Consolas`:
        // the render falls to a bundled face and discloses BUNDLED, never
        // silently borrowing the wrong supplied face (fuzzy-never-sneaky).
        let consolas = "<< /Type /Font /Subtype /TrueType /BaseFont /Consolas \
             /FirstChar 72 /LastChar 73 /Widths [600 600] \
             /FontDescriptor << /Type /FontDescriptor /FontName /Consolas /Flags 32 >> >>";
        let mut env = crate::font::FontEnvironment::bundled();
        let serif = env
            .fallback(crate::font::FallbackKey::Serif)
            .unwrap()
            .clone();
        env.insert_named("Calibri", serif);
        let out = render_with_fonts("BT /F1 40 Tf 5 40 Td (HI) Tj ET", consolas, env);
        assert!(out.diagnostics.glyphs_substituted >= 2, "bundled fallback");
        assert_eq!(out.diagnostics.substituted_fonts, vec!["Consolas"]);
        assert_eq!(out.diagnostics.glyphs_supplied, 0);
    }

    #[test]
    fn composite_nonembedded_stays_a_hard_skip_even_with_font_dir() {
        // decision 012 §6 named non-goal (FF2/R65): supplied fonts must
        // NOT attempt composite substitution. Registering a face under
        // the composite font's name changes nothing — it still returns
        // CompositeNotEmbedded.
        let mut env = crate::font::FontEnvironment::bundled();
        let serif = env
            .fallback(crate::font::FallbackKey::Serif)
            .unwrap()
            .clone();
        env.insert_named("X", serif);
        // A Type0/Identity-H whose descendant CIDFontType2 has NO embedded
        // program — the CompositeNotEmbedded hard skip.
        let (doc, page) = doc_with_extra_objects(
            "BT /F1 24 Tf 10 10 Td <0041> Tj ET",
            "/Resources << /Font << /F1 6 0 R >> >>",
            &[
                (
                    6,
                    b"<< /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H \
                      /DescendantFonts [5 0 R] >>"
                        .to_vec(),
                ),
                (
                    5,
                    b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /X \
                      /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
                      /FontDescriptor << /Type /FontDescriptor /FontName /X /Flags 4 >> >>"
                        .to_vec(),
                ),
            ],
        );
        let opts = RenderOptions {
            fonts: env,
            ..RenderOptions::default()
        };
        let out = render_page_with(&doc, &page, 1.0, &opts).unwrap();
        assert_eq!(ink_bbox(&out.pixmap), None, "composite must not paint");
        assert_eq!(out.diagnostics.fonts_unsupported, 1);
        assert_eq!(
            out.diagnostics
                .fonts_unsupported_by_reason
                .get("CompositeNotEmbedded")
                .copied()
                .unwrap_or(0),
            1
        );
        assert_eq!(out.diagnostics.glyphs_supplied, 0);
    }

    #[test]
    fn render_is_font_dir_independent_for_unreferenced_supplied_faces() {
        // R64: the bundled determinism gate is not perturbed by supplied
        // faces the page does not reference. A page drawn with an embedded
        // font (or a bundled substitute) renders BYTE-IDENTICALLY whether
        // or not an unrelated supplied face is present in the environment
        // — so an ambient font-dir config can never move the R59 gate.
        let base = render_with_fonts(
            "BT /F1 40 Tf 5 40 Td (HI) Tj ET",
            HELVETICA,
            crate::font::FontEnvironment::bundled(),
        );
        let mut env = crate::font::FontEnvironment::bundled();
        let serif = env
            .fallback(crate::font::FallbackKey::Serif)
            .unwrap()
            .clone();
        // A supplied face the page never names.
        env.insert_named("Consolas", serif.clone());
        env.insert_named("Calibri", serif);
        let with_unrelated = render_with_fonts("BT /F1 40 Tf 5 40 Td (HI) Tj ET", HELVETICA, env);
        assert_eq!(
            base.pixmap.data(),
            with_unrelated.pixmap.data(),
            "an unreferenced supplied face changed the render — R64 broken"
        );
        assert_eq!(with_unrelated.diagnostics.glyphs_supplied, 0);
    }

    // ---------------------------------------------------------------
    // Form XObjects (§8.10, Table 95)
    // ---------------------------------------------------------------

    /// A form XObject dictionary with the given extra entries.
    fn form_dict(extra: &str) -> String {
        format!("/Type /XObject /Subtype /Form {extra}")
    }

    #[test]
    fn form_xobject_matrix_maps_form_space_into_user_space() {
        // §8.10: "the Matrix entry shall specify the mapping from form
        // space to the current user space." The form paints a 20×20
        // square at ITS origin; the matrix translates it to (50, 60).
        let (doc, page) = doc_with_xobject(
            "/X1 Do",
            &form_dict("/BBox [0 0 100 100] /Matrix [1 0 0 1 50 60] /Resources << >>"),
            b"0 0 0 rg 0 0 20 20 re f",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        // User (55, 65) → device (55, 35).
        assert_eq!(pixel(&out.pixmap, 55, 35), (0, 0, 0));
        // Where the square would be WITHOUT the matrix: paper.
        assert_eq!(pixel(&out.pixmap, 5, 95), (255, 255, 255));
        assert_eq!(out.diagnostics.forms_rendered, 1);
        assert_eq!(out.diagnostics.deferred_ops, 0, "Do is no longer deferred");
    }

    #[test]
    fn deep_zoom_places_content_from_a_large_page_coordinate() {
        // THE REGRESSION TEST FOR `Mat64`, and it is written as a
        // PLACEMENT assertion rather than a matrix one on purpose: the
        // defect it guards never produced a wrong-looking matrix, it
        // produced a correct-looking one whose translation was rounded to
        // the nearest 512 device pixels, and the only visible symptom was
        // content sitting somewhere else.
        //
        // The fixture is the shape that broke: a form placed by a `cm`
        // carrying a PAGE COORDINATE, holding geometry whose own
        // coordinates are small, viewed as a tiny region at a scale where
        // `f32` cannot hold the intermediate product. Before `Pass 74.7`
        // the black square landed off-canvas and the render came back
        // blank.
        let (doc, page) = doc_with_xobject(
            // 1 unit of form space = 2e-8 pt, placed at (90, 90) -- so the
            // form's 1000x1000 box is 2e-5 pt across, about 7 nanometres.
            "q 0.00000002 0 0 0.00000002 90 90 cm /X1 Do Q",
            &form_dict("/BBox [0 0 1000 1000] /Resources << >>"),
            b"0 0 0 rg 200 200 600 600 re f",
        );

        // A 2e-5 pt window on the form at 2e7 device pixels per point:
        // 400 x 400 px. The intermediate product is 90 * 2e7 = 1.8e9,
        // where `f32`'s spacing is **128 pixels** -- a third of the
        // viewport -- and the region origin is another number of the same
        // size, so the cancellation is the whole game.
        //
        // ★ THE SCALE IS 19 999 993, NOT 20 000 000, AND THAT IS NOT
        // FUSSINESS. The first draft used round numbers and the test
        // passed against a deliberately sabotaged build -- because
        // `90 * 2e7 = 1.8e9` is exactly representable in `f32` (it is
        // 14 062 500 x 2^7), so the two large operands cancelled
        // perfectly and `f32` got the right answer for the wrong reason.
        // A cancellation only loses precision when the operands are large
        // and merely NEARLY equal. Adjusting these constants without
        // re-running the sabotage check is how this test becomes
        // decorative.
        //
        // With the scale as written: `90 * 19 999 993 = 1 799 999 370`,
        // which is not a multiple of 128, so `f32` must round it -- and
        // 128 device pixels is a third of this 400 px viewport.
        let region = pdfcer_core::page_tree::Rect {
            llx: 90.0,
            lly: 90.0,
            urx: 90.000_02,
            ury: 90.000_02,
        };
        let out = render_page_region(
            &doc.view(),
            &page,
            19_999_993.0,
            region,
            &RenderOptions::default(),
        )
        .unwrap();

        assert_eq!(
            out.diagnostics.forms_rendered, 1,
            "the form must not be culled"
        );

        // ★ MEASURE THE SQUARE'S EDGES, do not sample a few points.
        //
        // The first version asserted "black at 45%, white at 5%" and
        // passed against a build with **half** the fix removed, because
        // that half leaves ~10 px of error and 10 px does not flip a
        // sample taken 160 px from the edge. A placement test whose
        // tolerance is a third of the picture is a test of almost
        // nothing.
        //
        // The square is form-space [200,800] of a [0,1000] box, and the
        // region is exactly that box -- so its edges must land at 20 % and
        // 80 % of the viewport, and the tolerance is THREE PIXELS.
        let (w, h) = (out.pixmap.width(), out.pixmap.height());
        assert!(
            w >= 200 && h >= 200,
            "expected a real viewport, got {w}x{h}"
        );

        let black_at = |x: u32, y: u32| pixel(&out.pixmap, x, y) == (0, 0, 0);
        let row = h / 2;
        let col = w / 2;
        let left = (0..w).find(|&x| black_at(x, row));
        let right = (0..w).rev().find(|&x| black_at(x, row));
        let top = (0..h).find(|&y| black_at(col, y));
        let bottom = (0..h).rev().find(|&y| black_at(col, y));

        #[allow(clippy::cast_precision_loss)]
        let (wf, hf) = (w as f32, h as f32);
        for (got, want, what) in [
            (left, 0.20 * wf, "left edge"),
            (right, 0.80 * wf, "right edge"),
            (top, 0.20 * hf, "top edge"),
            (bottom, 0.80 * hf, "bottom edge"),
        ] {
            let Some(got) = got else {
                panic!(
                    "{what}: no black pixel found at all -- the square is off canvas, which is what an f32 translation at this magnitude does"
                );
            };
            #[allow(clippy::cast_precision_loss)]
            let delta = (got as f32 - want).abs();
            assert!(
                delta <= 3.0,
                "{what}: found at {got}, expected {want} (off by {delta} px). The square is misplaced, which is the symptom of composing this CTM in f32."
            );
        }
    }

    #[test]
    fn deep_zoom_keeps_a_feature_smaller_than_an_f32_step_at_a_page_coordinate() {
        // `Pass 74.7`'s SECOND algorithm, and the case the first one
        // cannot reach. Here the damage is done before any matrix is
        // involved: an `f32` near `x = 90` has a spacing of 7.6e-6, so a
        // rectangle 8e-6 wide -- about one representable step -- is
        // already destroyed by the time the operands are narrowed. No
        // amount of precision in the CTM recovers it.
        //
        // The remedy is to build the path RELATIVE TO ITS OWN FIRST
        // POINT, in `f64`, so what reaches `tiny_skia` is a set of small
        // differences rather than a set of nearly-equal large numbers.
        //
        // Note what is NOT being asserted: that the rectangle is in the
        // right PLACE. That is the first algorithm's job and its own test
        // covers it. This one asserts the rectangle still has a SIZE.
        let (doc, page) =
            doc_with_content("0 0 0 rg 90.000002 90.000002 0.000008 0.000008 re f", "");

        let region = pdfcer_core::page_tree::Rect {
            llx: 90.0,
            lly: 90.0,
            urx: 90.000_02,
            ury: 90.000_02,
        };
        let out = render_page_region(
            &doc.view(),
            &page,
            20_000_000.0,
            region,
            &RenderOptions::default(),
        )
        .unwrap();

        // 8e-6 pt at 2e7 px/pt = 160 px.
        let (w, h) = (out.pixmap.width(), out.pixmap.height());
        // Scanned over the WHOLE raster rather than one row: the
        // rectangle sits in a corner of this viewport, and the first
        // version sampled the middle row, which lands exactly on its
        // edge. A test that depends on where you happened to look is a
        // test of where you happened to look.
        let black_at = |x: u32, y: u32| pixel(&out.pixmap, x, y) == (0, 0, 0);
        let xs: Vec<u32> = (0..w).filter(|&x| (0..h).any(|y| black_at(x, y))).collect();
        assert!(
            !xs.is_empty(),
            "the rectangle vanished entirely -- which is what happens when both of its x coordinates round to the same f32"
        );
        #[allow(clippy::cast_precision_loss)]
        let width = (xs[xs.len() - 1] - xs[0] + 1) as f32;
        assert!(
            (width - 160.0).abs() <= 3.0,
            "expected a 160 px rectangle, measured {width} px. Anything much narrower means the two page coordinates collapsed onto the same f32 step; anything much wider means they landed on different ones and the size is an artefact of the rounding rather than the document."
        );
    }

    #[test]
    fn form_whose_bbox_misses_the_canvas_is_culled_not_executed() {
        // The cull in `do_form` is an OPTIMISATION that must not be
        // observable in the raster, so this test asserts both halves:
        // the form is counted as culled rather than rendered, AND the
        // page is byte-identical to the same page with no `Do` at all.
        //
        // §8.10.1 makes `/BBox` a clip on the form's contents, so a form
        // painting far outside its own box paints nothing wherever the
        // box lands. Here the box is translated a page-width away, so it
        // cannot touch a pixel and skipping the whole stream is exact.
        let (doc, page) = doc_with_xobject(
            "/X1 Do",
            &form_dict("/BBox [0 0 10 10] /Matrix [1 0 0 1 900 900] /Resources << >>"),
            b"0 0 0 rg 0 0 100 100 re f",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.forms_culled, 1, "off-canvas form culled");
        assert_eq!(
            out.diagnostics.forms_rendered, 0,
            "a culled form is not a rendered form; conflating the two is \
             how a slow render reports as a fast one"
        );

        // The byte-identity half. Rendered against the same page with the
        // `Do` removed entirely — if the cull ever became approximate,
        // this is the assertion that would catch it.
        let (doc2, page2) = doc_with_xobject(
            " ",
            &form_dict("/BBox [0 0 10 10] /Resources << >>"),
            b"0 0 0 rg 0 0 100 100 re f",
        );
        let bare = render_page(&doc2, &page2, 1.0).unwrap();
        assert_eq!(
            out.pixmap.data(),
            bare.pixmap.data(),
            "culling must be invisible in the raster"
        );
    }

    #[test]
    fn hairline_mode_thins_strokes_but_leaves_fills_untouched() {
        // Pass 254.0 — the CAD "line weights off" display mode. A fat stroke
        // must collapse to ~1 device pixel; a FILL must be byte-for-byte
        // unchanged (only S/s/B/B* strokes change, never geometry).
        fn dark(pm: &tiny_skia::Pixmap) -> usize {
            pm.pixels()
                .iter()
                .filter(|p| p.red() < 128 && p.alpha() > 0)
                .count()
        }
        let hairline = RenderOptions {
            stroke_display: crate::font::StrokeDisplay::Hairline,
            ..Default::default()
        };

        // A 12-unit-wide horizontal stroke across the middle of a 100x100 page.
        let (doc, page) = doc_with_content("12 w 20 50 m 80 50 l S", "");
        let actual =
            render_page_with_view(&doc.view(), &page, 1.0, &RenderOptions::default()).unwrap();
        let hair = render_page_with_view(&doc.view(), &page, 1.0, &hairline).unwrap();
        let (a, h) = (dark(&actual.pixmap), dark(&hair.pixmap));
        assert!(a > 0, "the thick stroke paints in the default (a={a})");
        assert!(
            h * 3 < a,
            "hairline must collapse the 12 px stroke to ~1 px, a large drop (a={a} h={h})"
        );

        // A FILL alone renders IDENTICALLY in both modes — hairline never
        // touches fills, or a hatch built from thin fills would vanish.
        let (fdoc, fpage) = doc_with_content("0 0 0 rg 20 20 60 30 re f", "");
        let fa =
            render_page_with_view(&fdoc.view(), &fpage, 1.0, &RenderOptions::default()).unwrap();
        let fh = render_page_with_view(&fdoc.view(), &fpage, 1.0, &hairline).unwrap();
        assert_eq!(
            dark(&fa.pixmap),
            dark(&fh.pixmap),
            "hairline must not change a filled region"
        );
    }

    #[test]
    fn subpixel_culling_is_off_by_default_and_counted_when_on() {
        // The OPT-IN, lossy cull. Three things are asserted and the third
        // is the one that matters: that the default renders it, that the
        // flag drops it AND says so, and that the two rasters DIFFER.
        //
        // Without the last assertion this test would pass against an
        // implementation that counted the form and painted it anyway --
        // which is the failure mode a "did the counter go up?" test
        // cannot see, and the opposite of the one where a silent
        // optimisation drops content without counting it.
        let (doc, page) = doc_with_xobject(
            // 0.4 x 0.4 device px at scale 1.0: under the half-pixel bar
            // in both axes, and dark enough that its coverage shows.
            "q 0.4 0 0 0.4 20 20 cm /X1 Do Q",
            &form_dict("/BBox [0 0 1 1] /Resources << >>"),
            b"0 0 0 rg 0 0 1 1 re f",
        );

        let on = RenderOptions {
            subpixel_culling: true,
            ..Default::default()
        };

        let off =
            render_page_with_view(&doc.view(), &page, 1.0, &RenderOptions::default()).unwrap();
        let fast = render_page_with_view(&doc.view(), &page, 1.0, &on).unwrap();

        assert_eq!(
            off.diagnostics.subpixel_culled, 0,
            "the default must not drop anything -- decision 082 puts a lossy trade with the operator, so it cannot be the default"
        );
        assert_eq!(off.diagnostics.forms_rendered, 1);

        assert_eq!(
            fast.diagnostics.subpixel_culled, 1,
            "with the option on, the form is dropped AND counted"
        );
        assert_eq!(
            fast.diagnostics.forms_rendered, 0,
            "a dropped form is not a rendered one"
        );
        assert_eq!(
            fast.diagnostics.forms_culled, 0,
            "and it is NOT reported as the exact /BBox cull -- that one changes no pixel and this one does, so merging the counters would let a fidelity trade hide inside a correctness optimisation"
        );

        assert_ne!(
            off.pixmap.data(),
            fast.pixmap.data(),
            "the rasters must DIFFER. If they do not, either the form was painted anyway or the fixture is too small to contribute coverage -- and in both cases this test is asserting nothing about lossiness"
        );
    }

    #[test]
    fn form_partly_on_canvas_is_not_culled() {
        // The complement of the test above, and the one that would fail
        // if the cull were made greedier than §8.10.1 allows. The BBox
        // straddles the left edge, so part of it is on the canvas and the
        // form must run.
        let (doc, page) = doc_with_xobject(
            "/X1 Do",
            &form_dict("/BBox [0 0 40 40] /Matrix [1 0 0 1 -20 -20] /Resources << >>"),
            b"0 0 0 rg 0 0 100 100 re f",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.forms_culled, 0);
        assert_eq!(out.diagnostics.forms_rendered, 1);
        assert_eq!(
            pixel(&out.pixmap, 5, 95),
            (0, 0, 0),
            "the on-canvas part of the form still paints"
        );
    }

    #[test]
    fn form_bbox_clips_the_form_content() {
        // §8.10.1 step (c): "clip according to the form dictionary's
        // BBox entry." The form tries to fill the whole page.
        let (doc, page) = doc_with_xobject(
            "/X1 Do",
            &form_dict("/BBox [0 0 10 10] /Resources << >>"),
            b"0 0 0 rg 0 0 100 100 re f",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(pixel(&out.pixmap, 5, 95), (0, 0, 0), "inside the BBox");
        assert_eq!(
            pixel(&out.pixmap, 50, 50),
            (255, 255, 255),
            "outside the BBox must stay paper white"
        );
    }

    #[test]
    fn form_bbox_is_clipped_after_matrix_not_before() {
        // The ORDER of §8.10.1's steps (b) then (c) is load-bearing: a
        // 2× Matrix makes the same BBox cover four times the page area.
        // A reader that clipped first would give a 10×10 result.
        let (doc, page) = doc_with_xobject(
            "/X1 Do",
            &form_dict("/BBox [0 0 10 10] /Matrix [2 0 0 2 0 0] /Resources << >>"),
            b"0 0 0 rg 0 0 100 100 re f",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        // User (15, 15) is inside the 20×20 transformed BBox…
        assert_eq!(pixel(&out.pixmap, 15, 85), (0, 0, 0));
        // …user (25, 25) is outside it.
        assert_eq!(pixel(&out.pixmap, 25, 75), (255, 255, 255));
    }

    #[test]
    fn form_bbox_corners_may_be_given_in_either_order() {
        // §7.9.5: a rectangle's two corners may be written in either
        // order, so `[10 10 0 0]` is the same box as `[0 0 10 10]`.
        let (doc, page) = doc_with_xobject(
            "/X1 Do",
            &form_dict("/BBox [10 10 0 0] /Resources << >>"),
            b"0 0 0 rg 0 0 100 100 re f",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(pixel(&out.pixmap, 5, 95), (0, 0, 0));
        assert_eq!(pixel(&out.pixmap, 50, 50), (255, 255, 255));
    }

    #[test]
    fn a_zero_area_bbox_paints_nothing_and_is_not_an_error() {
        // §8.10 gotcha: "a BBox with zero width or height clips
        // everything away — legal, and means paint nothing." The
        // failure mode this guards against is treating the degenerate
        // rectangle as "no BBox" and painting the form UNCLIPPED, which
        // is the exact opposite of what the spec asks for.
        let (doc, page) = doc_with_xobject(
            "/X1 Do",
            &form_dict("/BBox [10 10 10 90] /Resources << >>"),
            b"0 0 0 rg 0 0 100 100 re f",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(ink_bbox(&out.pixmap), None, "a zero-width BBox painted");
        assert_eq!(out.diagnostics.forms_rendered, 1, "still a valid form");
    }

    #[test]
    fn form_graphics_state_is_inherited_but_not_leaked_back() {
        // §8.10.1: "the initial graphics state for the form shall be
        // inherited from the graphics state in effect at the time Do is
        // invoked", and step (e) restores afterwards — even though this
        // form leaves an UNBALANCED `q` behind (§8.4.2's balance rule is
        // per content stream; producers break it).
        let (doc, page) = doc_with_xobject(
            "1 0 0 rg /X1 Do 0 60 20 20 re f",
            &form_dict("/BBox [0 0 100 100] /Resources << >>"),
            b"0 0 20 20 re f q 0 0 1 rg",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        // The form inherited the caller's red fill (it never set one).
        assert_eq!(pixel(&out.pixmap, 10, 90), (255, 0, 0));
        // After the form, the caller's own red is intact — the form's
        // blue and its dangling `q` did not escape.
        assert_eq!(pixel(&out.pixmap, 10, 30), (255, 0, 0));
    }

    #[test]
    fn a_form_that_invokes_itself_is_stopped_by_the_cycle_guard() {
        // The primary unbounded-recursion risk in the renderer
        // (`iso32000__s__8.8.md`). The guard is keyed on the XObject's
        // OBJECT NUMBER, so it holds however the form reaches itself.
        let (doc, page) = doc_with_xobject(
            "/X1 Do",
            &form_dict("/BBox [0 0 100 100] /Resources << /XObject << /X1 5 0 R >> >>"),
            b"0 0 0 rg 0 0 10 10 re f /X1 Do",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(
            pixel(&out.pixmap, 5, 95),
            (0, 0, 0),
            "the first pass must still paint"
        );
        assert_eq!(out.diagnostics.xobject_depth_overflows, 1);
        assert_eq!(out.diagnostics.forms_rendered, 1);
    }

    /// Build a chain of `depth` DISTINCT form XObjects, each invoking
    /// the next under the name `/X1`, with only the innermost one
    /// painting. Distinct objects mean the cycle guard never fires, so
    /// the only thing that can stop the chain is the depth bound.
    fn nested_form_chain(depth: usize) -> (Document, Page) {
        let mut extra: Vec<(u32, Vec<u8>)> = Vec::new();
        for i in 0..depth {
            let num = 5 + i as u32;
            let last = i + 1 == depth;
            let dict = if last {
                form_dict("/BBox [0 0 100 100] /Resources << >>")
            } else {
                form_dict(&format!(
                    "/BBox [0 0 100 100] /Resources << /XObject << /X1 {} 0 R >> >>",
                    num + 1
                ))
            };
            let body: &[u8] = if last {
                b"0 0 0 rg 0 0 10 10 re f"
            } else {
                b"/X1 Do"
            };
            extra.push((num, stream_object(&dict, body)));
        }
        doc_with_extra_objects(
            "/X1 Do",
            "/Resources << /XObject << /X1 5 0 R >> >>",
            &extra,
        )
    }

    #[test]
    fn a_32_deep_form_chain_renders_because_the_corpus_contains_one() {
        // veraPDF's PDF/A-1b §6.1.12 implementation-limits suite ships
        // `6-1-12-t08-pass-*.pdf`: a CONFORMANT file with a deliberate
        // chain of 32 nested form XObjects. §6.1.12 requires a reader
        // not to impose Annex C's implementation limits — and Annex C
        // has no form-nesting limit to impose in the first place, so
        // pdfcer's guard is pure policy and has to clear the corpus.
        //
        // This is the same shape of bug the 8 KiB MAX_TOKEN_LEN guard
        // had against `6-1-12-t02-pass-k.pdf` in the previous Pass: a
        // guard chosen from intuition about "real" documents rather
        // than from measurement.
        let (doc, page) = nested_form_chain(32);
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(
            out.diagnostics.xobject_depth_overflows, 0,
            "a 32-deep chain is conformant and must render"
        );
        assert_eq!(out.diagnostics.forms_rendered, 32);
        assert!(
            ink_bbox(&out.pixmap).is_some(),
            "the innermost form must have painted"
        );
    }

    #[test]
    fn deep_form_nesting_is_stopped_by_the_depth_guard() {
        // Past the bound the chain is refused rather than followed —
        // bounded memory, bounded time, and a counted diagnostic. A
        // chain of DISTINCT forms, so only MAX_XOBJECT_DEPTH can stop
        // it (the cycle set never fires).
        let (doc, page) = nested_form_chain(interpret::MAX_XOBJECT_DEPTH + 3);
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.xobject_depth_overflows, 1);
        assert_eq!(
            out.diagnostics.forms_rendered,
            interpret::MAX_XOBJECT_DEPTH,
            "exactly MAX_XOBJECT_DEPTH forms should have run"
        );
        // The only form that paints sits past the guard, so the page is
        // blank — and, crucially, the test terminated.
        assert_eq!(ink_bbox(&out.pixmap), None);
    }

    #[test]
    fn form_without_resources_falls_back_to_the_callers_and_says_so() {
        // §7.8.3 case 3: obsolete (PDF ≤ 1.1) but not forbidden, so the
        // fallback happens — and, being non-conformant input, it is
        // disclosed rather than absorbed.
        let (doc, page) = doc_with_extra_objects(
            "/X1 Do",
            "/Resources << /XObject << /X1 5 0 R >> /Font << /F1 6 0 R >> >>",
            &[
                (
                    5,
                    stream_object(
                        &form_dict("/BBox [0 0 100 100]"),
                        b"BT /F1 40 Tf 5 40 Td (H) Tj ET",
                    ),
                ),
                (6, HELVETICA.as_bytes().to_vec()),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert!(
            ink_bbox(&out.pixmap).is_some(),
            "the caller's /F1 must have resolved"
        );
        assert!(
            out.diagnostics
                .sample_ops
                .iter()
                .any(|s| s.contains("without /Resources")),
            "the fallback must be disclosed: {:?}",
            out.diagnostics.sample_ops
        );
    }

    #[test]
    fn form_text_object_does_not_leak_across_the_boundary() {
        // §9.4.1 confines Tm/Tlm to one BT…ET. A form invoked INSIDE a
        // caller's text object (ill-formed per Figure 9, common in the
        // wild) must neither see nor move the caller's pen — but the
        // text STATE (font, size) is graphics state and IS inherited.
        let (doc, page) = doc_with_extra_objects(
            "BT /F1 30 Tf 5 60 Td /X1 Do (H) Tj ET",
            "/Resources << /XObject << /X1 5 0 R >> /Font << /F1 6 0 R >> >>",
            &[
                (
                    5,
                    stream_object(
                        &form_dict("/BBox [0 0 100 100] /Resources << >>"),
                        // A stray `Td` with no BT of its own: tolerated,
                        // and must not disturb the caller.
                        b"90 0 Td",
                    ),
                ),
                (6, HELVETICA.as_bytes().to_vec()),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        let bbox = ink_bbox(&out.pixmap).expect("the glyph must still paint");
        assert!(
            bbox.0 < 20,
            "the form's Td moved the caller's pen: {bbox:?}"
        );
    }

    // ---------------------------------------------------------------
    // Image XObjects (§8.9) and inline images (§8.9.7)
    // ---------------------------------------------------------------

    /// An image XObject dictionary with the given extra entries.
    fn image_dict(extra: &str) -> String {
        format!("/Type /XObject /Subtype /Image /Width 2 /Height 2 {extra}")
    }

    /// Render a 2×2 image XObject over the whole page and return the
    /// four quadrant colours in sample order: (0,0), (1,0), (0,1), (1,1).
    fn quadrants(dict: &str, data: &[u8], prelude: &str) -> (RenderedPage, [(u8, u8, u8); 4]) {
        let (doc, page) =
            doc_with_xobject(&format!("q {prelude} {FULL_PAGE_CM} /X1 Do Q"), dict, data);
        let out = render_page(&doc, &page, 1.0).unwrap();
        let q = [
            pixel(&out.pixmap, 25, 25),
            pixel(&out.pixmap, 75, 25),
            pixel(&out.pixmap, 25, 75),
            pixel(&out.pixmap, 75, 75),
        ];
        (out, q)
    }

    #[test]
    fn gray_image_samples_land_where_the_unit_square_mapping_says() {
        // §8.9.3 orders samples by row from the image's UPPER-left;
        // §8.9.4 maps image space onto the user-space unit square whose
        // origin is its LOWER-left, via `[1/w 0 0 −1/h 0 1]`. So sample
        // (0,0) belongs at the TOP-left of the painted square. Getting
        // the −1/h wrong flips every image in the document.
        let (out, q) = quadrants(
            &image_dict("/ColorSpace /DeviceGray /BitsPerComponent 8"),
            &[0x00, 0xFF, 0xFF, 0x40],
            "",
        );
        assert_eq!(q[0], (0, 0, 0));
        assert_eq!(q[1], (255, 255, 255));
        assert_eq!(q[2], (255, 255, 255));
        assert_eq!(q[3], (0x40, 0x40, 0x40));
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(out.diagnostics.images_unsupported, 0);
        assert_eq!(out.diagnostics.deferred_ops, 0);
    }

    #[test]
    fn rgb_image_reads_three_components_per_sample() {
        let data = [
            0xFF, 0x00, 0x00, // (0,0) red
            0x00, 0xFF, 0x00, // (1,0) green
            0x00, 0x00, 0xFF, // (0,1) blue
            0xFF, 0xFF, 0x00, // (1,1) yellow
        ];
        let (_, q) = quadrants(
            &image_dict("/ColorSpace /DeviceRGB /BitsPerComponent 8"),
            &data,
            "",
        );
        assert_eq!(q, [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0)]);
    }

    #[test]
    fn cmyk_image_uses_the_same_conversion_as_the_k_operator() {
        // THE point of this test is the word "same". `DeviceCMYK` image
        // samples and the `k` operator are two entirely different code paths
        // — one goes through `image::Space::to_rgb` per sample, the other
        // through `interpret`'s operator dispatch — and both must land on the
        // single conversion in `pdfcer_core::color`. Two conversions that
        // disagree would paint an image and a filled rectangle of the same
        // CMYK in different colours inside one document, which is the exact
        // failure this crate's single-conversion-site rule exists to prevent.
        //
        // So the expectation is not a hard-coded triple: it is the OTHER
        // path's output, rendered from a content stream in the same layout.
        // A future change to the conversion moves both sides together and
        // this test keeps passing; a change that forks them fails it, which
        // is the only thing it can usefully detect.
        let data = [
            0xFF, 0x00, 0x00, 0x00, // solid cyan
            0x00, 0xFF, 0x00, 0x00, // solid magenta
            0x00, 0x00, 0xFF, 0x00, // solid yellow
            0x00, 0x00, 0x00, 0xFF, // solid black ink
        ];
        let (_, from_image) = quadrants(
            &image_dict("/ColorSpace /DeviceCMYK /BitsPerComponent 8"),
            &data,
            "",
        );

        // The same four inks as `k`-operator fills, one per quadrant of the
        // same 100×100 page (`FULL_PAGE_CM` is a 100-unit square).
        let (doc, page) = doc_with_content(
            "1 0 0 0 k 0 50 50 50 re f \
             0 1 0 0 k 50 50 50 50 re f \
             0 0 1 0 k 0 0 50 50 re f \
             0 0 0 1 k 50 0 50 50 re f",
            "",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        let from_operator = [
            pixel(&out.pixmap, 25, 25),
            pixel(&out.pixmap, 75, 25),
            pixel(&out.pixmap, 25, 75),
            pixel(&out.pixmap, 75, 75),
        ];
        // Equal to within ±1 per channel, and the ±1 is NOT colour slack.
        // The two paths reach the pixmap by different routes: an image sample
        // is written as a `ColorU8` directly, whereas a fill goes through
        // tiny-skia's premultiplied `PremultipliedColorU8`, whose
        // multiply-then-divide-by-alpha round trip loses up to one 8-bit step.
        // Measured here as `(237,2,140)` from the image vs `(237,1,140)` from
        // the fill for solid magenta. That is a rasterizer quantisation
        // artefact that predates this test and is invisible on screen; a real
        // conversion fork would show up as tens of steps, not one. Asserting
        // exact equality would therefore be asserting something about
        // tiny-skia's premultiply rounding, not about colour agreement.
        for (i, (img, op)) in from_image.iter().zip(&from_operator).enumerate() {
            let d = [
                (i32::from(img.0) - i32::from(op.0)).abs(),
                (i32::from(img.1) - i32::from(op.1)).abs(),
                (i32::from(img.2) - i32::from(op.2)).abs(),
            ];
            assert!(
                d.iter().all(|&v| v <= 1),
                "quadrant {i}: image {img:?} vs k-operator {op:?} differ by {d:?}"
            );
        }

        // And pin ONE landmark absolutely, so a fork that broke both paths
        // identically still fails: solid cyan ink is the reference's
        // (0, 174, 239), not the naive additive formula's (0, 255, 255).
        assert_eq!(from_image[0], (0, 174, 239), "solid cyan");
    }

    #[test]
    fn indexed_image_looks_each_sample_up_in_the_palette() {
        // §8.6.6.3 with a byte-STRING lookup (the form the spec's own
        // example uses). hival 1 → a two-entry palette: 0 red, 1 green.
        // At 1 bpc each ROW is padded to a whole byte (§8.9.3), so the
        // data is two bytes: row 0 = samples 0,1; row 1 = samples 1,0.
        let (_, q) = quadrants(
            &image_dict("/ColorSpace [/Indexed /DeviceRGB 1 <FF000000FF00>] /BitsPerComponent 1"),
            &[0b0100_0000, 0b1000_0000],
            "",
        );
        assert_eq!(q, [(255, 0, 0), (0, 255, 0), (0, 255, 0), (255, 0, 0)]);
    }

    #[test]
    fn indexed_default_decode_passes_samples_through_unchanged() {
        // Table 90's `Indexed` default is `[0 2ⁿ−1]`, NOT `[0 1]` — the
        // most-often-got-wrong row in the table. With 8 bpc and a
        // 4-entry palette, a `[0 1]` default would collapse every index
        // to 0 and paint the image a solid colour.
        let (_, q) = quadrants(
            &image_dict(
                "/ColorSpace [/Indexed /DeviceRGB 3 <FF0000 00FF00 0000FF FFFF00>] \
                 /BitsPerComponent 8",
            ),
            &[0, 1, 2, 3],
            "",
        );
        assert_eq!(q, [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0)]);
    }

    #[test]
    fn decode_array_one_zero_inverts_intensities() {
        // §8.9.5.2 NOTE 3: "if the colour space is DeviceGray and the
        // Decode array is [1.0 0.0], an input value of 0 is mapped to
        // 1.0 (white)". `Dmin > Dmax` IS the inversion mechanism — a
        // reader that min/max-normalizes the pair (correct for a
        // rectangle, §7.9.5) destroys it.
        let plain = quadrants(
            &image_dict("/ColorSpace /DeviceGray /BitsPerComponent 8"),
            &[0x00, 0xFF, 0xFF, 0x00],
            "",
        )
        .1;
        let inverted = quadrants(
            &image_dict("/ColorSpace /DeviceGray /BitsPerComponent 8 /Decode [1 0]"),
            &[0x00, 0xFF, 0xFF, 0x00],
            "",
        )
        .1;
        assert_eq!(
            plain,
            [(0, 0, 0), (255, 255, 255), (255, 255, 255), (0, 0, 0)]
        );
        assert_eq!(
            inverted,
            [(255, 255, 255), (0, 0, 0), (0, 0, 0), (255, 255, 255)]
        );
    }

    #[test]
    fn image_mask_paints_the_current_colour_and_leaves_the_rest_alone() {
        // §8.9.6.2: a stencil mask "designates places where the current
        // colour shall be painted"; masked-out areas "retain their
        // former contents." The DEFAULT Decode `[0 1]` means
        // **0 = ink** — the opposite of the usual bitmap intuition.
        let (out, q) = quadrants(
            // No /ColorSpace, no /BitsPerComponent — both are the
            // conformant form for an image mask.
            &image_dict("/ImageMask true"),
            &[0b0100_0000, 0b1000_0000],
            "1 0 0 rg",
        );
        assert_eq!(q[0], (255, 0, 0), "sample 0 marks with the fill colour");
        assert_eq!(q[1], (255, 255, 255), "sample 1 leaves the page alone");
        assert_eq!(q[2], (255, 255, 255));
        assert_eq!(q[3], (255, 0, 0));
        assert_eq!(out.diagnostics.images_rendered, 1);
    }

    #[test]
    fn image_mask_decode_one_zero_reverses_the_polarity() {
        // §8.9.6.2: "if the Decode array is [1 0], these meanings shall
        // be reversed" — the standard idiom, and by far the commonest
        // way producers emit stencil masks.
        let (_, q) = quadrants(
            &image_dict("/ImageMask true /Decode [1 0]"),
            &[0b0100_0000, 0b1000_0000],
            "0 0 1 rg",
        );
        assert_eq!(q[0], (255, 255, 255));
        assert_eq!(q[1], (0, 0, 255));
        assert_eq!(q[2], (0, 0, 255));
        assert_eq!(q[3], (255, 255, 255));
    }

    #[test]
    fn icc_based_image_falls_back_to_the_component_count() {
        // color__iccbased.md: pdfcer does not parse ICC profiles, so the
        // space is taken from the stream's /N (3 → RGB), which is what
        // /Alternate would have defaulted to anyway.
        let (doc, page) = doc_with_extra_objects(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            "/Resources << /XObject << /X1 5 0 R >> >>",
            &[
                (
                    5,
                    stream_object(
                        &image_dict("/ColorSpace [/ICCBased 6 0 R] /BitsPerComponent 8"),
                        &[
                            0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF,
                        ],
                    ),
                ),
                // A minimal ICC stream: pdfcer only ever reads its /N.
                (6, stream_object("/N 3", b"not-a-real-profile")),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(pixel(&out.pixmap, 25, 25), (255, 0, 0));
        assert_eq!(pixel(&out.pixmap, 75, 75), (255, 255, 255));
        assert_eq!(out.diagnostics.images_rendered, 1);
    }

    #[test]
    fn flate_encoded_image_decodes_through_the_filter_chain() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write as _;

        let raw = [0x00u8, 0xFF, 0xFF, 0x00];
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&raw).unwrap();
        let compressed = enc.finish().unwrap();

        let (_, q) = quadrants(
            &image_dict("/ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode"),
            &compressed,
            "",
        );
        assert_eq!(q, [(0, 0, 0), (255, 255, 255), (255, 255, 255), (0, 0, 0)]);
    }

    /// The synthetic JPEG codestreams from `pdfcer-core`'s DCT tests,
    /// pulled in by `include!` rather than copied.
    ///
    /// One provenance record, one generator
    /// (`tools/gen-jpeg-fixtures.py`), one set of bytes — a duplicated
    /// copy here would drift the moment either side regenerated, and
    /// `docs/LEGAL.md` §5 wants exactly one place where "where did this
    /// test data come from?" is answered. The included file is
    /// `#[cfg(test)]`-only inside `pdfcer-core`, so it is not reachable
    /// as a normal cross-crate path.
    ///
    /// Declared at FILE scope (see the ★ note above `mod tests`) and
    /// aliased here so every `jpeg::` call site below is unchanged. The
    /// declaration cannot live in this module: a `#[path]` inside an inline
    /// `mod tests` resolves through a phantom `src/tests/` directory, which
    /// Linux cannot traverse.
    use crate::jpeg_fixtures as jpeg;

    #[test]
    fn image_with_corrupt_codec_data_is_counted_not_fatal() {
        // "Fuzzy, never sneaky": nothing is drawn, the shortfall is
        // named, and the REST of the page still renders.
        //
        // The bytes are a bare JP2 signature box with no codestream
        // behind it. Before Pass 2.3 this test asserted the *codec* was
        // unimplemented; now that all four are built, the same input
        // exercises the fail-clean path instead — corrupt in, refused
        // out, never plausible-looking garbage (decision 001 §6.1.4).
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q 0 0 0 rg 0 0 10 10 re f"),
            &image_dict("/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /JPXDecode"),
            b"\x00\x00\x00\x0CjP  ",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_unsupported, 1);
        assert_eq!(
            out.diagnostics.images_codec_unsupported, 0,
            "the codec IS built; this is broken data, not a missing decoder"
        );
        assert_eq!(out.diagnostics.images_rendered, 0);
        assert!(
            out.diagnostics
                .image_notes
                .iter()
                .any(|n| n.contains("JPXDecode")),
            "the failing codec must still be named: {:?}",
            out.diagnostics.image_notes
        );
        // The image drew nothing…
        assert_eq!(pixel(&out.pixmap, 50, 50), (255, 255, 255));
        // …and the rest of the page is unaffected.
        assert_eq!(pixel(&out.pixmap, 5, 95), (0, 0, 0));
    }

    #[test]
    fn dct_rgb_jpeg_renders_through_the_codec_path() {
        // Pass 2.1's headline: a real JPEG, decoded and painted, with
        // §8.9.4's sample ordering intact. Source pixels are red,
        // green, blue, white at quality 90, so the assertions are
        // "recognizably that colour" rather than exact bytes.
        let (out, q) = quadrants(
            &image_dict("/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode"),
            jpeg::RGB_2X2,
            "",
        );
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(out.diagnostics.images_unsupported, 0);
        assert_eq!(out.diagnostics.images_codec_unsupported, 0);
        assert!(q[0].0 > 200 && q[0].1 < 60, "top-left red: {:?}", q[0]);
        assert!(q[1].1 > 200 && q[1].0 < 60, "top-right green: {:?}", q[1]);
        assert!(q[2].2 > 200 && q[2].0 < 60, "bottom-left blue: {:?}", q[2]);
        assert!(
            q[3].0 > 200 && q[3].2 > 200,
            "bottom-right white: {:?}",
            q[3]
        );
    }

    // ---------------------------------------------------------------
    // Bilevel codecs (§7.4.6 CCITT, §7.4.7 JBIG2) — Pass 2.2
    // ---------------------------------------------------------------

    /// The synthetic CCITT/JBIG2 codestreams from `pdfcer-core`'s bilevel
    /// tests, pulled in the same way and for the same reason as
    /// [`jpeg`]: one provenance record, one generator
    /// (`tools/gen-bilevel-fixtures.py`), one set of bytes.
    use crate::bilevel_fixtures as bilevel;

    /// A 16 × 4 bilevel image XObject dictionary with the given extras.
    ///
    /// The fixture is 16 × 4 rather than 2 × 2 because a fax codec's
    /// bugs are *stride* bugs: a 2-pixel row cannot tell a correct
    /// `ceil(columns / 8)` stride from an incorrect one, and a 4-row
    /// image is the smallest that exercises a 2-D coder's reference
    /// line.
    fn bilevel_dict(extra: &str) -> String {
        format!("/Type /XObject /Subtype /Image /Width 16 /Height 4 {extra}")
    }

    /// The four corners of the 16 × 4 fixture as rendered over the whole
    /// page: (left, right) of image row 0, then (left, right) of row 1.
    ///
    /// Row 0's samples are `00 FF` — left half black, right half white
    /// under DeviceGray's default `Decode [0 1]`, because `/BlackIs1`
    /// defaults to false and 0 is black. Row 1 is the mirror image, so
    /// the four probes together catch both an inverted polarity and a
    /// shifted row stride.
    fn bilevel_probes(out: &RenderedPage) -> [(u8, u8, u8); 4] {
        [
            pixel(&out.pixmap, 10, 10),
            pixel(&out.pixmap, 90, 10),
            pixel(&out.pixmap, 10, 35),
            pixel(&out.pixmap, 90, 35),
        ]
    }

    const BLACK: (u8, u8, u8) = (0, 0, 0);
    const WHITE: (u8, u8, u8) = (255, 255, 255);

    #[test]
    fn ccitt_group4_image_renders_through_the_codec_path() {
        // Pass 2.2's headline for CCITT: a real T.6 bit stream decoded,
        // unpacked at one bit per sample, and painted the right way up
        // and the right way round.
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            &bilevel_dict(
                "/ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /CCITTFaxDecode \
                 /DecodeParms << /K -1 /Columns 16 /Rows 4 >>",
            ),
            bilevel::CCITT_G4_16X4,
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(out.diagnostics.images_unsupported, 0);
        assert_eq!(out.diagnostics.images_codec_unsupported, 0);
        assert_eq!(
            bilevel_probes(&out),
            [BLACK, WHITE, WHITE, BLACK],
            "row 0 is black|white, row 1 is white|black"
        );
    }

    #[test]
    fn ccitt_black_is_1_inverts_the_rendered_page() {
        // The polarity trap, asserted where an operator would see it.
        // Same bytes, `/BlackIs1 true`, every probe flips.
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            &bilevel_dict(
                "/ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /CCITTFaxDecode \
                 /DecodeParms << /K -1 /Columns 16 /Rows 4 /BlackIs1 true >>",
            ),
            bilevel::CCITT_G4_16X4,
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(bilevel_probes(&out), [WHITE, BLACK, BLACK, WHITE]);
    }

    #[test]
    fn ccitt_group3_variants_render_identically() {
        // K = 0 and K > 0 are structurally different bit streams; the
        // picture is the same, so the page must be too.
        for (label, k, data) in [
            ("K = 0", "0", bilevel::CCITT_G3_1D_16X4),
            ("K > 0", "4", bilevel::CCITT_G3_2D_16X4),
        ] {
            let (doc, page) = doc_with_xobject(
                &format!("q {FULL_PAGE_CM} /X1 Do Q"),
                &bilevel_dict(&format!(
                    "/ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /CCITTFaxDecode \
                     /DecodeParms << /K {k} /Columns 16 /Rows 4 >>"
                )),
                data,
            );
            let out = render_page(&doc, &page, 1.0).unwrap();
            assert_eq!(out.diagnostics.images_rendered, 1, "{label}");
            assert_eq!(
                bilevel_probes(&out),
                [BLACK, WHITE, WHITE, BLACK],
                "{label}"
            );
        }
    }

    #[test]
    fn ccitt_stencil_mask_paints_the_fill_colour_where_the_sample_is_zero() {
        // §8.9.6.2: an image mask carries no colour — its 1-bit samples
        // say only "mark the page with the current non-stroking colour".
        // `/Decode` defaults to `[0 1]`, so **0 marks**, which composes
        // with `/BlackIs1` false to put ink exactly where the fax said
        // black. This is the shape most real CCITT images take.
        let (doc, page) = doc_with_xobject(
            &format!("q 1 0 0 rg {FULL_PAGE_CM} /X1 Do Q"),
            &bilevel_dict(
                "/ImageMask true /Filter /CCITTFaxDecode \
                 /DecodeParms << /K -1 /Columns 16 /Rows 4 >>",
            ),
            bilevel::CCITT_G4_16X4,
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(
            bilevel_probes(&out),
            [(255, 0, 0), WHITE, WHITE, (255, 0, 0)],
            "ink where the sample is 0, untouched page elsewhere"
        );
    }

    #[test]
    fn jbig2_image_renders_identically_to_the_ccitt_one() {
        // The two bilevel codecs reach PDF's "0 is black" convention by
        // completely different routes — CCITT through `/BlackIs1`'s
        // default, JBIG2 through the unconditional inverse of T.88's
        // "1 is black" — so rendering the same picture through both is
        // the strongest available check that neither is flipped.
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            &bilevel_dict("/ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /JBIG2Decode"),
            bilevel::JBIG2_MMR_16X4,
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(out.diagnostics.images_codec_unsupported, 0);
        assert_eq!(bilevel_probes(&out), [BLACK, WHITE, WHITE, BLACK]);
    }

    #[test]
    fn jbig2_is_still_refused_inside_an_inline_image() {
        // §7.4.7 / §8.9.7: "the JBIG2Decode filter shall not be used
        // with inline images." Implementing the codec must not relax
        // the construct-level rule.
        let (doc, page) = doc_with_content(
            "q 100 0 0 100 0 0 cm BI /W 16 /H 4 /BPC 1 /CS /G /F /JBIG2Decode ID \
             \x00\x00 EI Q",
            "",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_rendered, 0);
        assert_eq!(out.diagnostics.images_codec_unsupported, 1);
    }

    // ---------------------------------------------------------------
    // JPXDecode (§7.4.9, §8.9.5 Table 89) — Pass 2.3
    // ---------------------------------------------------------------

    /// The synthetic JPEG 2000 codestreams from `pdfcer-core`'s JPX
    /// tests, pulled in the same way and for the same reason as
    /// [`jpeg`] and [`bilevel`]: one provenance record, one generator
    /// (`tools/gen-jpx-fixtures.py`), one set of bytes.
    use crate::jpx_fixtures as jpx;

    /// A 4 × 2 JPX image XObject dictionary with the given extras.
    ///
    /// `/ColorSpace` and `/BitsPerComponent` are deliberately NOT in the
    /// base string: Table 89 makes both optional for this filter, so
    /// the bare dictionary is the conformant baseline and anything more
    /// is something a specific test chose to say.
    fn jpx_dict(extra: &str) -> String {
        format!("/Type /XObject /Subtype /Image /Width 4 /Height 2 /Filter /JPXDecode {extra}")
    }

    /// The eight pixels of a 4 × 2 image rendered over the whole page,
    /// in sample order (row 0 left to right, then row 1).
    fn jpx_probes(out: &RenderedPage) -> [(u8, u8, u8); 8] {
        let mut probes = [(0, 0, 0); 8];
        for (i, slot) in probes.iter_mut().enumerate() {
            let x = 12 + 25 * (i % 4);
            let y = if i < 4 { 25 } else { 75 };
            *slot = pixel(&out.pixmap, x as u32, y);
        }
        probes
    }

    #[test]
    fn jpx_renders_with_no_colour_space_in_the_dictionary() {
        // THE Pass 2.3 headline, and the half of Table 89's inversion a
        // conventional reader gets wrong: "/ColorSpace is Required for
        // images, EXCEPT those that use the JPXDecode filter". This
        // dictionary carries neither /ColorSpace nor /BitsPerComponent
        // and is fully conformant; a reader that hard-requires either
        // draws nothing.
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            &jpx_dict(""),
            jpx::JPX_RGB_8_JP2,
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(out.diagnostics.images_unsupported, 0);
        assert_eq!(out.diagnostics.images_codec_unsupported, 0);
        let q = jpx_probes(&out);
        assert!(q[0].0 > 200 && q[0].1 < 60, "sample 0 red: {:?}", q[0]);
        assert!(q[1].1 > 200 && q[1].0 < 60, "sample 1 green: {:?}", q[1]);
        assert!(q[2].2 > 200 && q[2].0 < 60, "sample 2 blue: {:?}", q[2]);
        assert_eq!(q[3], (255, 255, 255), "sample 3 white");
        assert_eq!(q[7], (0, 0, 0), "sample 7 black");
    }

    #[test]
    fn jpx_grayscale_codestream_supplies_devicegray() {
        // §7.4.9's terminal fallback rung: one channel means
        // DeviceGray. The fixture is a RAW codestream (no JP2 boxes, so
        // no colour specification at all), which is the case that can
        // ONLY be resolved by channel count.
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            "/Type /XObject /Subtype /Image /Width 16 /Height 4 /Filter /JPXDecode",
            jpx::JPX_GRAY_8_J2K,
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_rendered, 1);
        // Fixture row 0 is eight black samples then eight white; row 1
        // is the mirror. Probed the same way as the bilevel fixtures.
        assert_eq!(bilevel_probes(&out), [BLACK, WHITE, WHITE, BLACK]);
    }

    #[test]
    fn jpx_present_colour_space_still_wins() {
        // The OTHER direction of the same rule, and the one a
        // "codestream always wins for JPX" reading breaks: Table 89
        // says "If ColorSpace is present, any colour space
        // specifications in the JPEG2000 data shall be ignored."
        //
        // A DeviceGray dictionary over an RGB codestream therefore
        // paints ONE component per pixel. The disagreement is counted;
        // the dictionary is obeyed.
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            &jpx_dict("/ColorSpace /DeviceGray"),
            jpx::JPX_RGB_8_JP2,
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(out.diagnostics.codec_geometry_mismatch, 1);
        let q = jpx_probes(&out);
        // Sample 0's first component is 255 (the red channel), read as
        // grey — so white, not red. If the codestream had overridden
        // the dictionary this would be red instead.
        assert_eq!(q[0], (255, 255, 255), "R=255 read as grey: {:?}", q[0]);
    }

    #[test]
    fn jpx_ignores_bits_per_component_and_decode() {
        // Table 89, both halves: `/BitsPerComponent` "shall be ignored
        // if present", and `Decode` "shall be ignored" when
        // `ImageMask` is false. The dictionary below states a wrong
        // depth AND an inverting `/Decode`; obeying either produces a
        // visibly different picture, so a single render settles both.
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            &jpx_dict("/ColorSpace /DeviceRGB /BitsPerComponent 16 /Decode [1 0 1 0 1 0]"),
            jpx::JPX_RGB_8_JP2,
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(
            out.diagnostics.codec_geometry_mismatch, 1,
            "a stated /BitsPerComponent is counted even though it is ignored"
        );
        let q = jpx_probes(&out);
        assert!(
            q[0].0 > 200 && q[0].1 < 60,
            "sample 0 must still be red, not cyan: {:?}",
            q[0]
        );
        assert_eq!(q[3], (255, 255, 255), "white must not invert to black");
    }

    #[test]
    fn jpx_in_codestream_alpha_is_ignored_by_default_and_composited_when_switched_on() {
        // `/SMaskInData`'s default of 0 means "encoded soft-mask image
        // information shall be ignored", so an RGBA codestream with no
        // `/SMaskInData` is an ordinary OPAQUE image — a decoder that
        // always composites the alpha it found is wrong in exactly the
        // way that looks right. With `/SMaskInData 1` the same bytes
        // carry a soft mask, and since Pass 1.1 item 6.3 pdfcer
        // composites it.
        //
        // The fixture's alpha runs 255, 170, 85, 0, 0, 85, 170, 255
        // against colours red, green, blue, white, cyan, magenta,
        // yellow, black. Sample 4 (cyan at alpha 0) is the
        // discriminating probe: opaque it is cyan, composited it is the
        // white page.
        for (entry, expect_alpha) in [("", false), ("/SMaskInData 1", true)] {
            let (doc, page) = doc_with_xobject(
                &format!("q {FULL_PAGE_CM} /X1 Do Q"),
                &jpx_dict(entry),
                jpx::JPX_RGBA_8_JP2,
            );
            let out = render_page(&doc, &page, 1.0).unwrap();
            assert_eq!(out.diagnostics.images_rendered, 1, "{entry:?}");
            assert_eq!(
                out.diagnostics.mask_applied.get("jpx-embedded-alpha"),
                expect_alpha.then_some(&1),
                "{entry:?}: {:?}",
                out.diagnostics.mask_applied
            );
            assert_eq!(
                out.diagnostics.images_mask_unsupported, 0,
                "{entry:?}: nothing here is a refusal"
            );
            // Either way the colours are the colour channels, not the
            // colour channels shifted along by an interleaved alpha.
            let q = jpx_probes(&out);
            assert!(
                q[0].0 > 200 && q[0].1 < 60,
                "{entry:?} sample 0 (alpha 255 either way) must be red: {:?}",
                q[0]
            );
            // Sample 4: cyan at alpha 0.
            let (r, g, b) = q[4];
            if expect_alpha {
                assert!(
                    r > 250 && g > 250 && b > 250,
                    "/SMaskInData 1 must make sample 4 vanish into the white page: {:?}",
                    q[4]
                );
            } else {
                assert!(
                    r < 60 && g > 200 && b > 200,
                    "with no /SMaskInData sample 4 must stay cyan: {:?}",
                    q[4]
                );
            }
            // Sample 2: blue at alpha 85 — an INTERMEDIATE value, so it
            // is the one that separates "alpha applied" from "alpha
            // applied and premultiplied correctly". Over white,
            // 85/255 of blue leaves r = g = 170.
            if expect_alpha {
                let (r, g, b) = q[2];
                assert!(
                    r.abs_diff(170) <= 3 && g.abs_diff(170) <= 3 && b > 250,
                    "sample 2 must be blue at one third opacity over white: {:?}",
                    q[2]
                );
            }
        }
    }

    #[test]
    fn jpx_smask_in_data_two_draws_the_preblended_image_and_names_it() {
        // Recognize-and-defer: the picture is drawn from the preblended
        // channels (a real image, not a grey box) and the shortfall is
        // counted by name rather than silently approximated.
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            &jpx_dict("/SMaskInData 2"),
            jpx::JPX_RGBA_8_JP2,
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(out.diagnostics.jpx_smask_in_data_preblended, 1);
        assert!(
            out.diagnostics
                .image_notes
                .iter()
                .any(|n| n.contains("SMaskInData 2")),
            "the deferral must be named: {:?}",
            out.diagnostics.image_notes
        );
        let q = jpx_probes(&out);
        assert!(q[0].0 > 200 && q[0].1 < 60, "sample 0 red: {:?}", q[0]);
    }

    #[test]
    fn jpx_cmyk_renders_through_the_four_component_path() {
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            &jpx_dict(""),
            jpx::JPX_CMYK_8_JP2,
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_rendered, 1);
        let q = jpx_probes(&out);
        // Sample 0 is pure cyan ink; sample 3 is pure black ink; sample
        // 7 is no ink at all. The bounds are the CALIBRATED conversion's
        // (`pdfcer_core::color`): solid cyan is (0, 174, 239) — cyan-ish but
        // well short of the naive formula's (0, 255, 255) — and solid black
        // ink alone is the reference's warm near-black (35, 31, 32), NOT
        // #000000. Only "no ink" is still exactly paper white, which is
        // pinned in the table precisely because it is observable.
        assert!(
            q[0].0 < 40 && q[0].1 > 150 && q[0].2 > 200,
            "sample 0 cyan: {:?}",
            q[0]
        );
        assert!(
            q[3].0 < 50 && q[3].1 < 50 && q[3].2 < 50,
            "sample 3 K=255 (near-black, not pure black): {:?}",
            q[3]
        );
        assert_eq!(q[7], (255, 255, 255), "sample 7 no ink");
    }

    #[test]
    fn jpx_image_mask_honours_decode_and_thresholds_the_delivered_depth() {
        // The one case where a JPX image's `/Decode` IS honoured:
        // §7.4.9 says it "shall be ignored, except in the case where
        // the image is treated as a mask; that is, when `ImageMask` is
        // true". Both polarities are checked, so a stencil path that
        // ignored `/Decode` for JPX along with everything else would
        // fail on the second half.
        //
        // It also pins the threshold: §7.4.9 requires a mask
        // codestream to carry 1-bit samples, but pdfcer normalizes every
        // JPX depth to 8 bits, so those samples arrive as 0 and 255.
        // Reading them one bit at a time would unpack eight pixels out
        // of every one. The 16 × 4 grayscale fixture's first two rows
        // are exactly 0 and 255, which is what a conformant 1-bit mask
        // looks like after normalization.
        //
        // `/BitsPerComponent 8` is stated deliberately: §8.9.6.2 wants
        // 1 for an image mask, Table 89 says ignore it for this filter,
        // and the more specific rule wins — a reader that refuses here
        // rejects a conformant file.
        for (decode, expect) in [
            ("", [BLACK, WHITE, WHITE, BLACK]),
            ("/Decode [1 0]", [WHITE, BLACK, BLACK, WHITE]),
        ] {
            let (doc, page) = doc_with_xobject(
                &format!("q 0 0 0 rg {FULL_PAGE_CM} /X1 Do Q"),
                &format!(
                    "/Type /XObject /Subtype /Image /Width 16 /Height 4 /ImageMask true \
                     /BitsPerComponent 8 /Filter /JPXDecode {decode}"
                ),
                jpx::JPX_GRAY_8_JP2,
            );
            let out = render_page(&doc, &page, 1.0).unwrap();
            assert_eq!(out.diagnostics.images_rendered, 1, "{decode:?}");
            assert_eq!(bilevel_probes(&out), expect, "{decode:?}");
        }
    }

    #[test]
    fn jpx_is_still_refused_inside_an_inline_image() {
        // §7.4.9: "This filter shall only be applied to image XObjects,
        // and not to inline images." §8.9.7 says the same from the
        // other side. Implementing the codec must not relax the
        // construct-level rule — the same guarantee the JBIG2 test
        // above makes.
        let (doc, page) = doc_with_content(
            "q 100 0 0 100 0 0 cm BI /W 4 /H 2 /F /JPXDecode ID \x00\x00 EI Q",
            "",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_rendered, 0);
        assert_eq!(out.diagnostics.images_codec_unsupported, 1);
    }

    #[test]
    fn dct_progressive_jpeg_renders() {
        // 14% of the measured corpus (decision 005 §3.2). A
        // baseline-only decoder leaves one JPEG in seven undrawn.
        let (out, q) = quadrants(
            &image_dict("/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode"),
            jpeg::RGB_2X2_PROGRESSIVE,
            "",
        );
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert!(q[0].0 > 200 && q[0].1 < 60, "top-left red: {:?}", q[0]);
    }

    #[test]
    fn dct_grayscale_jpeg_renders_one_component_per_sample() {
        let (out, q) = quadrants(
            &image_dict("/ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode"),
            jpeg::GRAY_2X2,
            "",
        );
        assert_eq!(out.diagnostics.images_rendered, 1);
        // Source ramp 0, 85, 170, 255, monotonically increasing.
        assert!(q[0].0 < 25, "{:?}", q[0]);
        assert!(q[3].0 > 230, "{:?}", q[3]);
        assert!(q[0].0 < q[1].0 && q[1].0 < q[2].0);
    }

    #[test]
    fn dct_jpeg_behind_an_ascii85_prefix_renders() {
        // `/Filter [/ASCII85Decode /DCTDecode]` — the byte-stream
        // prefix runs first, then the terminal codec (decision 005
        // §4.6).
        let mut armoured = String::new();
        for chunk in jpeg::RGB_2X2.chunks(4) {
            let mut word = [0u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            let mut value = u32::from_be_bytes(word);
            let mut group = [0u8; 5];
            for slot in group.iter_mut().rev() {
                *slot = b'!' + (value % 85) as u8;
                value /= 85;
            }
            armoured.extend(group[..chunk.len() + 1].iter().map(|&b| b as char));
        }
        armoured.push_str("~>");
        let (out, q) = quadrants(
            &image_dict(
                "/ColorSpace /DeviceRGB /BitsPerComponent 8 \
                 /Filter [/ASCII85Decode /DCTDecode]",
            ),
            armoured.as_bytes(),
            "",
        );
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert!(q[0].0 > 200 && q[0].1 < 60, "top-left red: {:?}", q[0]);
    }

    #[test]
    fn dct_cmyk_jpeg_is_drawn_and_counted() {
        // CMYK_2X2 is APP14 transform 0 with no /Decode — decision 006
        // §4.4's R30 shape. Raw samples pass through (R29: no engine
        // inverts), `/Decode` owns polarity, and because nothing here
        // declares the polarity the R30 counter — not the benign YCCK
        // census — must fire, with its named note.
        let (out, _) = quadrants(
            &image_dict("/ColorSpace /DeviceCMYK /BitsPerComponent 8 /Filter /DCTDecode"),
            jpeg::CMYK_2X2,
            "",
        );
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(out.diagnostics.dct_cmyk_polarity_unverifiable, 1);
        assert_eq!(
            out.diagnostics.dct_cmyk_images, 0,
            "the benign census is YCCK (transform 1/2) only"
        );
        assert!(
            out.diagnostics
                .image_notes
                .iter()
                .any(|n| n.contains("polarity unverifiable")),
            "{:?}",
            out.diagnostics.image_notes
        );
    }

    #[test]
    fn dct_geometry_disagreement_is_counted_and_the_image_still_draws() {
        // The dictionary says 4x4, the codestream says 2x2. The pixmap
        // follows the dictionary (§8.9.4 placement); the row stride
        // follows the codestream. Neither shears nor moves the picture,
        // and the divergence is reported.
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            "/Type /XObject /Subtype /Image /Width 4 /Height 4 \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode",
            jpeg::RGB_2X2,
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(out.diagnostics.codec_geometry_mismatch, 1);
    }

    #[test]
    fn arithmetic_coded_jpeg_is_refused_by_feature_name() {
        // Rule R27: an operator must be able to tell WHICH feature is
        // missing without reading the code. SOF9 = extended sequential,
        // arithmetic coding.
        let sof9: &[u8] = &[
            0xFF, 0xD8, // SOI
            0xFF, 0xC9, 0x00, 0x0B, // SOF9, length 11
            0x08, 0x00, 0x02, 0x00, 0x02, 0x01, // 8-bit, 2x2, 1 component
            0x01, 0x11, 0x00, // component spec
            0xFF, 0xDA, // SOS
        ];
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            &image_dict("/ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode"),
            sof9,
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_unsupported, 1);
        assert_eq!(
            out.diagnostics
                .codec_feature_unsupported
                .get("DCT/arithmetic"),
            Some(&1),
            "counted BY NAME: {:?}",
            out.diagnostics.codec_feature_unsupported
        );
        // A named sub-feature is NOT "the codec is missing".
        assert_eq!(out.diagnostics.images_codec_unsupported, 0);
    }

    #[test]
    fn corrupt_jpeg_data_is_refused_without_drawing_anything() {
        // Fail-clean at the codec layer: SOI + EOI with no frame.
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            &image_dict("/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode"),
            b"\xFF\xD8\xFF\xD9",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_unsupported, 1);
        assert_eq!(out.diagnostics.images_rendered, 0);
        assert_eq!(pixel(&out.pixmap, 50, 50), (255, 255, 255));
    }

    #[test]
    fn lzw_image_renders_and_its_framing_anomaly_is_reported() {
        // LZW stays a BYTE-STREAM filter in the cascade (R23). This
        // stream starts straight at a literal with no ClearCode:
        // non-conformant, recovered, counted.
        //   'A' as a 9-bit code, then EndOfInformation (257).
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 \
             /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /LZWDecode",
            &[0x20, 0xC0, 0x40],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(out.diagnostics.lzw_framing_anomalies, 1);
        // 'A' = 0x41 as a DeviceGray sample.
        assert_eq!(pixel(&out.pixmap, 50, 50), (0x41, 0x41, 0x41));
    }

    #[test]
    fn runlength_image_renders() {
        // §7.4.5, ~40 lines in-house, no dependency: 0x01 → two literal
        // bytes, 0xFF → two copies of 0x80, 0x80 → EOD.
        let (out, q) = quadrants(
            &image_dict("/ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /RunLengthDecode"),
            b"\x01\x00\xFF\xFF\x80\x80",
            "",
        );
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(q[0], (0, 0, 0));
        assert_eq!(q[1], (255, 255, 255));
        assert_eq!(q[2], (0x80, 0x80, 0x80));
        assert_eq!(q[3], (0x80, 0x80, 0x80));
    }

    #[test]
    fn soft_masked_image_composites_its_alpha() {
        // §8.9.6 / §11.6.5.3, Pass 1.1 item 6.3. THE regression this
        // Pass exists to prevent: before it, this page rendered as a
        // solid black square because `/SMask` was recognized, counted,
        // and then ignored.
        //
        // A 2 x 2 all-black image with an `/SMask` of
        // `[0xFF, 0x00, 0xFF, 0x00]` — opaque, transparent, opaque,
        // transparent in sample order — over the white page. The
        // quadrant probes therefore alternate black and white, which no
        // opaque render can produce.
        let (doc, page) = doc_with_extra_objects(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            "/Resources << /XObject << /X1 5 0 R >> >>",
            &[
                (
                    5,
                    stream_object(
                        &image_dict("/ColorSpace /DeviceGray /BitsPerComponent 8 /SMask 6 0 R"),
                        &[0x00, 0x00, 0x00, 0x00],
                    ),
                ),
                (
                    6,
                    stream_object(
                        &image_dict("/ColorSpace /DeviceGray /BitsPerComponent 8"),
                        &[0xFF, 0x00, 0xFF, 0x00],
                    ),
                ),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(out.diagnostics.images_masked, 1);
        assert_eq!(out.diagnostics.mask_applied.get("smask"), Some(&1));
        assert_eq!(out.diagnostics.images_mask_unsupported, 0);
        assert_eq!(pixel(&out.pixmap, 25, 25), (0, 0, 0), "sample 0: opaque");
        assert_eq!(
            pixel(&out.pixmap, 75, 25),
            (255, 255, 255),
            "sample 1: fully transparent, the white page must show through"
        );
        assert_eq!(pixel(&out.pixmap, 25, 75), (0, 0, 0), "sample 2: opaque");
        assert_eq!(
            pixel(&out.pixmap, 75, 75),
            (255, 255, 255),
            "sample 3: fully transparent"
        );
        // A composited mask is verified-correct volume, not a shortfall
        // — no note attaches (decision 006 §4.4's cried-wolf lesson).
        assert!(
            !out.diagnostics
                .image_notes
                .iter()
                .any(|n| n.contains("drawn opaque")),
            "a mask that WAS applied must not be reported as deferred: {:?}",
            out.diagnostics.image_notes
        );
    }

    #[test]
    fn image_dimensions_past_the_guard_are_refused() {
        // ARCHITECTURE.md §10.1: Width/Height are attacker-controlled,
        // and the check happens BEFORE any allocation or decode.
        let (doc, page) = doc_with_xobject(
            &format!("q {FULL_PAGE_CM} /X1 Do Q"),
            "/Type /XObject /Subtype /Image /Width 65535 /Height 65535 \
             /ColorSpace /DeviceRGB /BitsPerComponent 8",
            b"\x00",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.images_unsupported, 1);
        assert_eq!(out.diagnostics.images_rendered, 0);
    }

    #[test]
    fn inline_image_ascii_hex_renders_through_the_same_path() {
        // §8.9.7: the BI/ID/EI parameters are "analogous to those in the
        // dictionary portion of an image XObject", already normalized
        // out of the Table 93/94 abbreviations by pdfcer-core — so this
        // exercises exactly the XObject image code with a different
        // source of bytes, plus the new ASCIIHexDecode.
        let (doc, page) = doc_with_content(
            &format!("q {FULL_PAGE_CM} BI /W 2 /H 2 /CS /G /BPC 8 /F /AHx ID 00FFFF40> EI Q"),
            "",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(pixel(&out.pixmap, 25, 25), (0, 0, 0));
        assert_eq!(pixel(&out.pixmap, 75, 25), (255, 255, 255));
        assert_eq!(pixel(&out.pixmap, 75, 75), (0x40, 0x40, 0x40));
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(out.diagnostics.deferred_ops, 0);
    }

    #[test]
    fn inline_image_ascii85_renders() {
        // 00 FF FF 00 encodes to `!<<'!` (§7.4.3's base-85 relation).
        let (doc, page) = doc_with_content(
            &format!("q {FULL_PAGE_CM} BI /W 2 /H 2 /CS /G /BPC 8 /F /A85 ID !<<'!~> EI Q"),
            "",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(pixel(&out.pixmap, 25, 25), (0, 0, 0));
        assert_eq!(pixel(&out.pixmap, 75, 25), (255, 255, 255));
        assert_eq!(pixel(&out.pixmap, 75, 75), (0, 0, 0));
        assert_eq!(out.diagnostics.images_rendered, 1);
    }

    #[test]
    fn inline_image_mask_uses_the_current_fill_colour() {
        let (doc, page) = doc_with_content(
            &format!(
                "q 0 0 1 rg {FULL_PAGE_CM} BI /W 2 /H 2 /IM true /D [1 0] /F /AHx ID 4080> EI Q"
            ),
            "",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(pixel(&out.pixmap, 75, 25), (0, 0, 255));
        assert_eq!(pixel(&out.pixmap, 25, 25), (255, 255, 255));
    }

    #[test]
    fn image_inside_a_form_is_counted_on_the_page() {
        // Nested diagnostics must MERGE: an operator looking at the
        // page's honesty report has to see what its forms did.
        let (doc, page) = doc_with_extra_objects(
            "/X1 Do",
            "/Resources << /XObject << /X1 5 0 R >> >>",
            &[
                (
                    5,
                    stream_object(
                        &form_dict(
                            "/BBox [0 0 100 100] /Resources << /XObject << /Im1 6 0 R >> >>",
                        ),
                        format!("q {FULL_PAGE_CM} /Im1 Do Q").as_bytes(),
                    ),
                ),
                (
                    6,
                    stream_object(
                        &image_dict("/ColorSpace /DeviceGray /BitsPerComponent 8"),
                        &[0x00, 0x00, 0x00, 0x00],
                    ),
                ),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.forms_rendered, 1);
        assert_eq!(out.diagnostics.images_rendered, 1);
        assert_eq!(pixel(&out.pixmap, 50, 50), (0, 0, 0));
    }

    #[test]
    fn postscript_xobject_is_silently_ignored() {
        // §8.8.2 + §8.8.1: "PostScript XObjects should not be used", and
        // a conforming non-PostScript reader ignores them. That is
        // CORRECT behaviour, so it must not be counted as a shortfall.
        let (doc, page) = doc_with_xobject(
            "/X1 Do",
            "/Type /XObject /Subtype /PS",
            b"% a PostScript fragment",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.deferred_ops, 0);
        assert_eq!(out.diagnostics.unknown_ops, 0);
        assert_eq!(out.diagnostics.tolerated, 0);
        assert_eq!(out.diagnostics.images_unsupported, 0);
    }

    #[test]
    fn do_with_an_unresolvable_name_is_tolerated() {
        // §8.8: spec-undefined. No-op plus a diagnostic, never a failed
        // page.
        let (doc, page) = doc_with_content("/Nope Do 0 0 0 rg 0 0 10 10 re f", "");
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert!(out.diagnostics.tolerated >= 1);
        assert_eq!(out.diagnostics.unknown_ops, 0);
        assert_eq!(pixel(&out.pixmap, 5, 95), (0, 0, 0));
    }

    #[test]
    fn bx_ex_silences_unknown_operators() {
        let (doc, page) = doc_with_content("BX 1 2 frob EX 3 4 nicate", "");
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.compat_skipped, 1);
        assert_eq!(out.diagnostics.unknown_ops, 1);
    }

    #[test]
    fn raster_size_guard_trips() {
        let (doc, page) = doc_with_content("", "");
        let e = render_page(&doc, &page, 1000.0).unwrap_err();
        assert!(matches!(e, RenderError::BadRasterSize { .. }));
    }

    // -- Pass 15.1: the R59 render-fidelity gate on REFLOWED output ------
    //
    // A within-block reflow (`pdfcer_core::text_edit::apply_reflow`, Pass
    // 15.1) re-emits a paragraph's own content stream at new per-line
    // origins/breaks (justified lines via `TJ` slack). The reflowed page
    // must render FAITHFULLY: the new operators are valid, the glyphs land
    // where the bytes say, and a justified full line's ink reaches the wrap
    // box's right margin. This is the R59 gate applied to 15.1's output —
    // the renderer reads the same §9.4.4 advance model the surgery emitted
    // against, so agreement here proves the emitted `Tm`/`TJ` are correct.

    /// Reflow-apply `content` (a single Helvetica block) at `width`/`align`
    /// on a 100×100 page, then load + render the output and return its ink
    /// bbox in device pixels (scale 1 ⇒ device x = user x, device y = 100 −
    /// user y). Panics on any failure — a reflow that does not render is a
    /// gate failure.
    fn reflow_then_render(
        content: &str,
        width: f64,
        align: Option<pdfcer_core::text_edit::BlockAlignment>,
    ) -> (u32, u32, u32, u32, Diagnostics) {
        use pdfcer_core::text_edit::{ReflowRequest, apply_reflow};
        let (doc, _page) = doc_with_font(content, HELVETICA);
        let req = ReflowRequest::new()
            .with_wrap_width(width)
            .with_alignment_opt(align);
        let out = apply_reflow(&doc, 0, 0, &req).expect("reflow applies");
        let doc2 = Document::from_bytes(out.bytes).expect("reflowed output loads");
        let pages = pdfcer_core::page_tree::pages(&doc2).expect("pages");
        let rendered = render_page(&doc2, &pages[0], 1.0).expect("reflowed page renders");
        let bbox = ink_bbox(&rendered.pixmap).expect("reflowed page is not blank");
        (bbox.0, bbox.1, bbox.2, bbox.3, rendered.diagnostics)
    }

    #[test]
    fn reflowed_page_renders_faithfully_no_unknown_ops() {
        // Three source lines, re-wrapped to a wide box: the reflowed content
        // renders with real glyphs, no unknown operators (every emitted
        // operator is valid), and real ink on the page (R59 faithful render).
        let (x0, _y0, x1, _y1, diag) = reflow_then_render(
            "BT /F1 10 Tf 5 80 Td (aa bb) Tj ET\n\
             BT /F1 10 Tf 5 70 Td (cc dd) Tj ET\n\
             BT /F1 10 Tf 5 60 Td (ee) Tj ET\n",
            90.0,
            None,
        );
        assert_eq!(diag.unknown_ops, 0, "every emitted operator is valid");
        assert!(
            diag.glyphs_substituted >= 1,
            "real glyphs painted (R20 counts)"
        );
        assert!(x1 > x0, "ink has horizontal extent");
    }

    #[test]
    fn justified_reflow_ink_reaches_the_box_right_margin() {
        // Four words that fill the first line, then a fifth ("end") that
        // overflows to a short SECOND line — width 60 keeps them apart
        // ("aa bb cc dd" ≈ 51.7pt fits 60; adding "end" ≈ 71pt does not).
        // JUSTIFIED, the full first line's ink must reach the wrap box's
        // right margin (llx 5 + width 60 = 65 user ⇒ device x ≈ 65); the last
        // line stays short at the left, so the flush ink on the top line is
        // what pushes the bbox right edge to the margin. This is the
        // render-side confirmation of the justify TJ slack (the exact glyph
        // positions are asserted in pdfcer-core).
        let (_x0, _y0, x1, _y1, diag) = reflow_then_render(
            "BT /F1 10 Tf 5 80 Td (aa bb) Tj ET\n\
             BT /F1 10 Tf 5 70 Td (cc dd end) Tj ET\n",
            60.0,
            Some(pdfcer_core::text_edit::BlockAlignment::Justified),
        );
        assert_eq!(diag.unknown_ops, 0);
        // llx 5 + width 60 = 65 user = device x 65. The last glyph's ink
        // right edge sits just inside the origin flush point; allow a small
        // tolerance for the face's right sidebearing.
        assert!(
            (61..=67).contains(&x1),
            "justified ink right edge {x1} should reach the ~65px box margin"
        );
    }
    // -----------------------------------------------------------------
    // The operator's layer override (`LayerVisibility`)
    // -----------------------------------------------------------------

    /// **Turning a layer ON that the document turns OFF paints it.**
    ///
    /// The direction that makes a Layers panel useful rather than
    /// decorative: the file says hide it, the operator says show it, and
    /// the operator wins for this render only — nothing in the document
    /// changes.
    #[test]
    fn an_override_can_show_a_layer_the_document_hides() {
        let (doc, page) = doc_with_oc_content(
            "/OC /oc1 BDC 0 0 0 rg 10 10 50 50 re f EMC",
            "/Order [5 0 R] /OFF [5 0 R]",
        );
        // Hiding NOTHING — which is what "turn every layer on" produces,
        // and is deliberately distinct from passing no override at all.
        let options = RenderOptions::default().with_layers(crate::LayerVisibility::hiding([]));
        let out = render_page_with(&doc, &page, 1.0, &options).unwrap();
        assert!(
            ink_bbox(&out.pixmap).is_some(),
            "an override that hides nothing must show a layer the document turned off"
        );
        assert_eq!(out.diagnostics.oc_sections_hidden, 0);
    }

    /// **Turning a layer OFF that the document leaves ON hides it.**
    #[test]
    fn an_override_can_hide_a_layer_the_document_shows() {
        let (doc, page) = doc_with_oc_content(
            "/OC /oc1 BDC 0 0 0 rg 10 10 50 50 re f EMC",
            "/Order [5 0 R]",
        );
        let options = RenderOptions::default()
            .with_layers(crate::LayerVisibility::hiding([ObjId::new(5, 0)]));
        let out = render_page_with(&doc, &page, 1.0, &options).unwrap();
        assert!(ink_bbox(&out.pixmap).is_none());
        assert_eq!(out.diagnostics.oc_sections_hidden, 1);
    }

    /// ★ **The override REPLACES the document's configuration; it does
    /// not merge with it.**
    ///
    /// Two groups, one of which the document turns off. The override
    /// names only the OTHER one. If the two sets were unioned, nothing
    /// would paint; because the override replaces, the document's
    /// hidden group comes back and the override's goes away — the two
    /// squares swap.
    ///
    /// This is the contract `layer_state`'s module docs argue for, and
    /// it is worth a test rather than a comment because a merge is the
    /// intuitive implementation and would pass both tests above.
    #[test]
    fn an_override_replaces_the_documents_configuration_rather_than_merging() {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /OCProperties \
                  << /OCGs [5 0 R 6 0 R] /D << /OFF [5 0 R] >> >> >>"
                    .to_vec(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            ),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
                  /Resources << /Properties << /a 5 0 R /b 6 0 R >> >> >>"
                    .to_vec(),
            ),
            (
                4,
                stream_object(
                    "",
                    b"/OC /a BDC 0 0 0 rg 5 5 20 20 re f EMC \
                      /OC /b BDC 0 0 0 rg 70 70 20 20 re f EMC",
                ),
            ),
            (5, b"<< /Type /OCG /Name (A) >>".to_vec()),
            (6, b"<< /Type /OCG /Name (B) >>".to_vec()),
        ];
        let (doc, page) = build_pdf(&objects);

        // The document alone: A hidden, B shown — ink only in the
        // upper-right (PDF y grows upward, so B's y=70 is near the top).
        let base = render_page(&doc, &page, 1.0).unwrap();
        let base_box = ink_bbox(&base.pixmap).expect("B must paint");

        // The override names B only. Under a UNION both would be hidden.
        let options = RenderOptions::default()
            .with_layers(crate::LayerVisibility::hiding([ObjId::new(6, 0)]));
        let out = render_page_with(&doc, &page, 1.0, &options).unwrap();
        let over_box = ink_bbox(&out.pixmap).expect(
            "A must paint under the override; if this is empty the sets were merged, not replaced",
        );
        assert_ne!(
            base_box, over_box,
            "the visible square must have CHANGED, not merely survived"
        );
        assert!(
            over_box.0 < base_box.0,
            "A sits left of B, so replacing the configuration moves the ink left"
        );
        assert_eq!(out.diagnostics.oc_sections_hidden, 1);
    }

    // -----------------------------------------------------------------
    // §8.11.3.2 — optional content in CONTENT STREAMS (BDC/EMC /OC)
    //
    // The half of §8.11 that was deferred through Pass 12.M2, when only
    // an annotation's /OC entry was honoured. Annotation /OC covers
    // pdfcer's OWN authored layers; content-stream /OC is how every CAD
    // exporter, and every "Layers" panel in a real drawing, works.
    // -----------------------------------------------------------------

    /// A one-page doc whose content stream is `content`, with one OCG
    /// (object 5) registered in `/OCProperties` under `d_config`, and
    /// reachable from the page's `/Properties` resource as `/oc1`.
    fn doc_with_oc_content(content: &str, d_config: &str) -> (Document, Page) {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                format!(
                    "<< /Type /Catalog /Pages 2 0 R /OCProperties \
                     << /OCGs [5 0 R] /D << {d_config} >> >> >>"
                )
                .into_bytes(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            ),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
                  /Resources << /Properties << /oc1 5 0 R >> >> >>"
                    .to_vec(),
            ),
            (4, stream_object("", content.as_bytes())),
            (5, b"<< /Type /OCG /Name (Layer 1) >>".to_vec()),
        ];
        build_pdf(&objects)
    }

    /// **Content inside an OFF layer is not drawn.**
    ///
    /// The base case, and the one the deferral cost: before this, every
    /// layer in a CAD drawing painted regardless of its state, so pdfcer
    /// showed construction geometry and title-block alternates that the
    /// producer had explicitly turned off.
    #[test]
    fn content_in_an_off_oc_section_is_not_painted() {
        let (doc, page) = doc_with_oc_content(
            "/OC /oc1 BDC 0 0 0 rg 10 10 50 50 re f EMC",
            "/Order [5 0 R] /OFF [5 0 R]",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert!(
            ink_bbox(&out.pixmap).is_none(),
            "a filled rectangle inside an OFF /OC section must leave no ink"
        );
        assert_eq!(
            out.diagnostics.oc_sections_hidden, 1,
            "the page must be able to say WHY it is empty"
        );
    }

    /// The same content with the layer ON paints normally — the guard
    /// suppresses by layer state, not by the presence of `BDC`.
    #[test]
    fn content_in_an_on_oc_section_is_painted() {
        let (doc, page) = doc_with_oc_content(
            "/OC /oc1 BDC 0 0 0 rg 10 10 50 50 re f EMC",
            "/Order [5 0 R]",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert!(ink_bbox(&out.pixmap).is_some(), "an ON layer paints");
        assert_eq!(out.diagnostics.oc_sections_hidden, 0);
    }

    /// ★ **A hidden section's CLIP still applies to what follows.**
    ///
    /// §8.11.3.1: hidden content "shall not be drawn", but the graphics
    /// state it establishes persists. Here the hidden section clips to
    /// the left half of the page and the VISIBLE rectangle after it
    /// spans the whole width — so if the clip were skipped along with
    /// the paint, ink would appear on the right.
    ///
    /// This is the test that distinguishes "do not draw it" from "do not
    /// run it", and getting it wrong makes a page's LAYOUT depend on
    /// layer state — the one thing a layer toggle must never do.
    #[test]
    fn a_hidden_sections_clip_still_applies_to_later_visible_content() {
        let (doc, page) = doc_with_oc_content(
            "/OC /oc1 BDC 0 0 50 100 re W n EMC 0 0 0 rg 0 0 100 100 re f",
            "/Order [5 0 R] /OFF [5 0 R]",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        let bbox = ink_bbox(&out.pixmap).expect("the visible fill must paint");
        assert!(
            bbox.2 <= 51,
            "the hidden section's clip must still bound the visible fill; \
             ink reached x={} which means the clip was skipped with the paint",
            bbox.2
        );
    }

    /// **A nested section inside a hidden one cannot un-hide itself.**
    ///
    /// Visibility is not a stack of independent answers: §8.11.3.1 says
    /// hidden content shall not be drawn, and an ON group nested inside
    /// an OFF one is still inside an OFF one. The inner `EMC` must
    /// restore "hidden", not "visible".
    #[test]
    fn an_on_section_nested_inside_an_off_one_stays_hidden() {
        let (doc, page) = doc_with_oc_content(
            "/OC /oc1 BDC /Span /oc1 BDC 0 0 0 rg 10 10 50 50 re f EMC \
             0 0 0 rg 60 60 30 30 re f EMC",
            "/Order [5 0 R] /OFF [5 0 R]",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert!(
            ink_bbox(&out.pixmap).is_none(),
            "nothing inside an OFF section paints, at any nesting depth"
        );
    }

    /// **A non-`/OC` `BDC` still balances the stack.**
    ///
    /// If `/Span` and friends were not pushed, their `EMC` would close
    /// the enclosing `/OC` section instead — un-hiding the rest of the
    /// page from the middle of a hidden layer. Tagged PDFs mix these
    /// constantly, so this is the common case, not a corner one.
    #[test]
    fn a_non_oc_marked_content_level_does_not_close_the_oc_section() {
        let (doc, page) = doc_with_oc_content(
            "/OC /oc1 BDC /Span << /ActualText (x) >> BDC EMC 0 0 0 rg 10 10 50 50 re f EMC",
            "/Order [5 0 R] /OFF [5 0 R]",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert!(
            ink_bbox(&out.pixmap).is_none(),
            "the /Span EMC must close the /Span, not the /OC section"
        );
    }

    /// **A surplus `EMC` does not un-hide an open section.**
    ///
    /// Malformed streams are real. Underflowing the hidden depth would
    /// make a stray operator reveal content the producer turned off,
    /// which is the worse of the two failure directions.
    #[test]
    fn a_surplus_emc_does_not_reveal_hidden_content() {
        let (doc, page) = doc_with_oc_content(
            "EMC /OC /oc1 BDC 0 0 0 rg 10 10 50 50 re f EMC",
            "/Order [5 0 R] /OFF [5 0 R]",
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert!(ink_bbox(&out.pixmap).is_none());
        assert!(
            out.diagnostics.tolerated >= 1,
            "the unmatched EMC is tolerated, and counted"
        );
    }

    /// **A `/Properties` entry that is not an indirect reference shows.**
    ///
    /// §8.11.3.2 requires the operand to name an indirect object,
    /// because visibility is keyed on object identity. A direct
    /// dictionary has no identity to key on, so pdfcer cannot classify it
    /// — and shows it. Content shown by mistake is visible and therefore
    /// arguable; content hidden by mistake is missing with nothing on
    /// screen to suggest it.
    #[test]
    fn an_unclassifiable_oc_property_shows_rather_than_hides() {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /OCProperties \
                  << /OCGs [5 0 R] /D << /OFF [5 0 R] >> >> >>"
                    .to_vec(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            ),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
                  /Resources << /Properties << /oc1 << /Type /OCG /Name (direct) >> >> >> >>"
                    .to_vec(),
            ),
            (
                4,
                stream_object("", b"/OC /oc1 BDC 0 0 0 rg 10 10 50 50 re f EMC"),
            ),
            (5, b"<< /Type /OCG /Name (Layer 1) >>".to_vec()),
        ];
        let (doc, page) = build_pdf(&objects);
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert!(
            ink_bbox(&out.pixmap).is_some(),
            "an /OC pdfcer cannot resolve to an object must not silently hide content"
        );
        assert!(out.diagnostics.tolerated >= 1, "and it is counted");
    }

    /// ★ **An image XObject inside a hidden section is not drawn.**
    ///
    /// This shipped BROKEN. The first cut of optional content gated the
    /// path blit and the glyph blit and nothing else, so an image inside
    /// a hidden `/OC` section painted normally — and `do_xobject`'s own
    /// comment claimed the visibility of the enclosing section was
    /// ORed in, which was true only of the FORM branch.
    ///
    /// The image here carries **no `/OC` of its own**, which is the
    /// whole point: its hiddenness can only come from the section it sits
    /// in. An image with its own OFF `/OC` was already handled and would
    /// have passed while this failed.
    ///
    /// §8.11.3.1's "shall not be drawn" is not media-typed.
    #[test]
    fn an_image_inside_a_hidden_section_is_not_drawn() {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /OCProperties \
                  << /OCGs [5 0 R] /D << /OFF [5 0 R] >> >> >>"
                    .to_vec(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            ),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << \
                  /Properties << /oc1 5 0 R >> /XObject << /Im0 6 0 R >> >> >>"
                    .to_vec(),
            ),
            (
                4,
                stream_object("", b"/OC /oc1 BDC q 50 0 0 50 10 10 cm /Im0 Do Q EMC"),
            ),
            (5, b"<< /Type /OCG /Name (Layer 1) >>".to_vec()),
            (
                6,
                stream_object(
                    "/Type /XObject /Subtype /Image /Width 1 /Height 1 \
                     /ColorSpace /DeviceGray /BitsPerComponent 8",
                    &[0u8],
                ),
            ),
        ];
        let (doc, page) = build_pdf(&objects);
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert!(
            ink_bbox(&out.pixmap).is_none(),
            "an image with no /OC of its own is still hidden by the section it is drawn inside"
        );
    }

    /// **An INLINE image inside a hidden section is not drawn either.**
    ///
    /// `BI`/`ID`/`EI` has no XObject dictionary, so it has no `/OC` to
    /// consult and can *only* be hidden by its enclosing section. It was
    /// the least gated path of all — the XObject branch at least checked
    /// the XObject's own `/OC`, and this one checked nothing.
    #[test]
    fn an_inline_image_inside_a_hidden_section_is_not_drawn() {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /OCProperties \
                  << /OCGs [5 0 R] /D << /OFF [5 0 R] >> >> >>"
                    .to_vec(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            ),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
                  /Resources << /Properties << /oc1 5 0 R >> >> >>"
                    .to_vec(),
            ),
            (
                4,
                stream_object(
                    "",
                    b"/OC /oc1 BDC q 50 0 0 50 10 10 cm \
                      BI /W 1 /H 1 /CS /G /BPC 8 ID \x00 EI Q EMC",
                ),
            ),
            (5, b"<< /Type /OCG /Name (Layer 1) >>".to_vec()),
        ];
        let (doc, page) = build_pdf(&objects);
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert!(
            ink_bbox(&out.pixmap).is_none(),
            "an inline image has no /OC of its own, so the section is the only thing that can hide it"
        );
    }

    /// The same inline image with the layer ON still paints — so the
    /// gate above is suppressing by layer state and has not simply
    /// broken inline images.
    ///
    /// Worth its own test rather than an inverted assertion: "nothing
    /// painted" passes just as well when the fixture never painted at
    /// all, and an inline image assembled by hand is exactly the kind of
    /// fixture that can silently fail to draw.
    #[test]
    fn the_same_inline_image_paints_when_its_layer_is_on() {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /OCProperties \
                  << /OCGs [5 0 R] /D << >> >> >>"
                    .to_vec(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            ),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
                  /Resources << /Properties << /oc1 5 0 R >> >> >>"
                    .to_vec(),
            ),
            (
                4,
                stream_object(
                    "",
                    b"/OC /oc1 BDC q 50 0 0 50 10 10 cm \
                      BI /W 1 /H 1 /CS /G /BPC 8 ID \x00 EI Q EMC",
                ),
            ),
            (5, b"<< /Type /OCG /Name (Layer 1) >>".to_vec()),
        ];
        let (doc, page) = build_pdf(&objects);
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert!(
            ink_bbox(&out.pixmap).is_some(),
            "the fixture must actually paint, or the hidden-case test above proves nothing"
        );
    }

    /// **An image XObject carrying its own OFF `/OC` is not drawn**
    /// (§8.11.3.3, the XObject half — deferred alongside the
    /// content-stream half until now).
    #[test]
    fn an_image_xobject_on_an_off_layer_is_not_drawn() {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /OCProperties \
                  << /OCGs [5 0 R] /D << /OFF [5 0 R] >> >> >>"
                    .to_vec(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            ),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
                  /Resources << /XObject << /Im0 6 0 R >> >> >>"
                    .to_vec(),
            ),
            (4, stream_object("", b"q 50 0 0 50 10 10 cm /Im0 Do Q")),
            (5, b"<< /Type /OCG /Name (Layer 1) >>".to_vec()),
            (
                6,
                stream_object(
                    "/Type /XObject /Subtype /Image /Width 1 /Height 1 \
                     /ColorSpace /DeviceGray /BitsPerComponent 8 /OC 5 0 R",
                    &[0u8],
                ),
            ),
        ];
        let (doc, page) = build_pdf(&objects);
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert!(
            ink_bbox(&out.pixmap).is_none(),
            "an image XObject whose own /OC is OFF must not be drawn"
        );
        assert_eq!(out.diagnostics.oc_sections_hidden, 1);
    }
}

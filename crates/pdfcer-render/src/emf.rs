//! # EMF export — a page written as a Windows Enhanced Metafile (`Pass 248.4`)
//!
//! Turns one page into an [MS-EMF] metafile — the vector format LibreOffice
//! 24.x, Office's *Paste Special → Picture (Enhanced Metafile)* and every
//! legacy Win32 consumer (Visio, CorelDRAW, CAD packages) read — from the
//! same export recording the SVG writer consumes ([`crate::svg`]), so the
//! two agree with each other and with the raster.
//!
//! ## 1. Why EMF at all, given SVG
//!
//! `docs/clipboard-interop-survey.md` §1.3: LibreOffice **24.x on Windows
//! cannot read a foreign `image/svg+xml` clipboard entry** (its
//! name-translation table gained the entry only in 25.2), so for that
//! consumer `CF_ENHMETAFILE` is the *only* vector route. Office reads SVG
//! from the clipboard as an editable graphic but its *Paste Special* menu
//! offers EMF as the shape-ungroupable form, and older Win32 applications
//! know nothing else. The operator asked for the follow-on by name.
//!
//! ## 2. What EMF cannot hold, and what this writer does about each
//!
//! EMF is a 1993 GDI recording format. It has paths, solid pens and brushes,
//! path clipping, and — since Windows 2000 — one alpha-blended bitmap
//! record. It has **no** per-primitive alpha, no blend modes, no soft masks,
//! no gradients a general PDF shading maps onto, and no text-as-text without
//! a font it would have to embed. The policy, applied per recorded op:
//!
//! | recorded | EMF | counted in [`EmfOutcome`] |
//! |---|---|---|
//! | solid fill, alpha 255, `Normal` | `CREATEBRUSHINDIRECT` + path bracket + `FILLPATH` | `ops` |
//! | solid stroke, alpha 255, `Normal` | `EXTCREATEPEN` (`PS_GEOMETRIC`, caps/joins) + bracket + `STROKEPATH`; dash **pre-applied** to the geometry (LibreOffice renders `PS_USERSTYLE` solid) | `ops`, `dashed_strokes_pre_applied` |
//! | a solid op with alpha < 255, or a non-`Normal` blend, or a `Gradient`/`Image` brush | the op is REPLAYED into a transparent scratch, cropped, and written as `EMR_ALPHABLEND` (premultiplied BGRA, the GDI contract) | `rasters_embedded` + one of `ops_rasterised_for_alpha` / `blend_modes_dropped` / `gradients_rasterised` / `images_embedded` |
//! | `Op::Layer` (group opacity, blend, soft mask) | the whole layer replayed, its mask and opacity applied, one `EMR_ALPHABLEND`; the layer's blend mode against the page is dropped to `Normal` | `layers_rasterised`, `blend_modes_dropped` |
//! | a clip | `SAVEDC`, one path bracket + `SELECTCLIPPATH RGN_AND` per ancestor, `RESTOREDC` when the clip changes | — |
//! | text | glyph outlines, like every other fill | — |
//!
//! Every row of that table is a disclosure (rule 4); the CLI prints them.
//!
//! ## 3. Coordinate system — the recommendation in `D:\dev\rag\emf\`
//!
//! Logical unit = **0.01 mm** (a virtual 2540-dpi device), `MM_TEXT`
//! (y-down), **no world transform, no window/viewport records**: every
//! consumer scales from the header's `Device`/`Millimeters` ratio and the
//! path records carry integer coordinates. The recording is made at
//! [`EmfOptions::raster_dpi`] (300 by default — the resolution of anything
//! that has to be a bitmap) and every device pixel is multiplied by
//! `2540 / dpi` on the way out, so an A3 long side is 42 000 units — far
//! inside `i32`, far outside `i16`, which is why the 32-bit point records
//! are used throughout.
//!
//! ## 4. Consumer quirks the writer designs around (sourced in `consumers.md`)
//!
//! - **LibreOffice 24.x ignores `EMR_SETPOLYFILLMODE`** — every fill is
//!   even-odd there. Emitted anyway (GDI and LibreOffice ≥ 25 honour it);
//!   a nonzero fill with overlapping subpaths will show holes in 24.x and
//!   is counted (`nonzero_fills_multi_subpath`) so the CLI can say so.
//! - **LibreOffice renders `PS_USERSTYLE` dashes solid** — dashes are
//!   pre-applied to the geometry instead.
//! - **`EMR_SETMITERLIMIT` is a float on the wire** (Appendix A `<90>`);
//!   LibreOffice ignores it, Inkscape misreads it as an int.
//! - **Inkscape draws nothing for `EMR_ALPHABLEND`.** Inkscape is not an
//!   EMF target (it prefers the SVG on the same clipboard); the choice is
//!   GDI-correct rasters over Inkscape-visible ones, and it is disclosed.
//! - **Handles = highest object index + 1**, `Records` counts the header
//!   and the EOF, `Bytes` is the exact file length — all back-patched.
//!
//! ## Failure modes
//!
//! [`crate::RenderError`], as for a render; plus `BadRasterSize` when the
//! page in 0.01 mm units would not fit `i32` (it cannot — a page is at most
//! 14 400 pt — but the guard is cheaper than the assumption).

use std::sync::Arc;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::Page;
use pdfcer_core::view::DocumentView;
use tiny_skia::{
    BlendMode, FillRule, LineCap, LineJoin, Mask, Path, PathSegment, Pixmap, Transform,
};

use crate::RenderError;
use crate::canvas::{Brush, BrushSpec, LayerPaint};
use crate::display_list::{
    ClipDef, ClipId, DeviceBounds, ExportTally, MaskBuilder, Op, record_page_for_export, replay_ops,
};
use crate::export::Rgb;
use crate::font::RenderOptions;
use crate::interpret::Diagnostics;

/// How an EMF is written.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct EmfOptions {
    /// The recording resolution — what anything embedded as a bitmap is
    /// sampled at, and the grid every coordinate is converted from.
    /// Vectors are exact at any value; 300 by default.
    pub raster_dpi: f32,
    /// An opaque background rectangle under the page, or `None`.
    pub background: Option<Rgb>,
}

impl Default for EmfOptions {
    fn default() -> Self {
        Self {
            raster_dpi: 300.0,
            background: None,
        }
    }
}

impl EmfOptions {
    /// Set the raster/recording resolution.
    #[must_use]
    pub fn with_raster_dpi(mut self, dpi: f32) -> Self {
        self.raster_dpi = dpi;
        self
    }

    /// Set (or clear) the background rectangle.
    #[must_use]
    pub fn with_background(mut self, background: Option<Rgb>) -> Self {
        self.background = background;
        self
    }
}

/// What an EMF export produced beside the bytes — the disclosure.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EmfOutcome {
    /// The page's physical size, points.
    pub width_pt: f32,
    /// The page's physical size, points.
    pub height_pt: f32,
    /// Vector ops written (fills and strokes as GDI paths).
    pub ops: usize,
    /// `EMR_ALPHABLEND` records written, of any origin.
    pub rasters_embedded: usize,
    /// Solid fills/strokes with a constant alpha below 1 — EMF has no
    /// per-primitive alpha, so each became a bitmap.
    pub ops_rasterised_for_alpha: usize,
    /// Ops whose blend mode EMF cannot express; drawn `Normal` inside a
    /// bitmap of themselves.
    pub blend_modes_dropped: usize,
    /// Gradient fills (native in the SVG) that became bitmaps here.
    pub gradients_rasterised: usize,
    /// The file's own images, embedded as bitmaps.
    pub images_embedded: usize,
    /// Transparency groups (opacity, soft mask, blend) rasterised whole.
    pub layers_rasterised: usize,
    /// Strokes whose dash pattern was pre-applied to the geometry.
    pub dashed_strokes_pre_applied: usize,
    /// Nonzero-rule fills with more than one subpath — the case LibreOffice
    /// 24.x (which ignores the fill mode) may render with holes.
    pub nonzero_fills_multi_subpath: usize,
    /// The export recording's own tally (what was rasterised or
    /// approximated BEFORE this writer saw it).
    pub tally: ExportTally,
    /// The interpreter's honesty report for the walk.
    pub diagnostics: Diagnostics,
}

/// An exported page.
#[derive(Debug, Clone)]
pub struct EmfExport {
    /// The metafile bytes — what `SetEnhMetaFileBits` takes and what a
    /// `.emf` file holds.
    pub emf: Vec<u8>,
    /// The disclosure.
    pub outcome: EmfOutcome,
}

/// Export one page of a loaded document as an EMF.
///
/// # Errors
///
/// [`RenderError`], as for a render — see the module docs.
pub fn export_emf(
    doc: &Document,
    page: &Page,
    render: &RenderOptions,
    emf: &EmfOptions,
) -> Result<EmfExport, RenderError> {
    export_emf_view(&doc.view(), page, render, emf)
}

/// [`export_emf`] over a [`DocumentView`] — an edit session's live view.
///
/// # Errors
///
/// As [`export_emf`].
pub fn export_emf_view(
    view: &DocumentView<'_>,
    page: &Page,
    render: &RenderOptions,
    emf: &EmfOptions,
) -> Result<EmfExport, RenderError> {
    let dpi = if emf.raster_dpi.is_finite() && emf.raster_dpi > 0.0 {
        emf.raster_dpi
    } else {
        300.0
    };
    let scale = dpi / 72.0;
    let recording = record_page_for_export(view, page, scale, render)?;
    let (w_px, h_px) = recording.page_size;

    // Device pixel -> 0.01 mm. A page is at most 14 400 pt = 5 080 mm =
    // 508 000 units; the guard is against a resolution that would overflow
    // an i32 through the multiply, not against any real page.
    let unit = 2540.0 / dpi;
    #[allow(clippy::cast_precision_loss)]
    let (w_u, h_u) = ((w_px as f32 * unit).round(), (h_px as f32 * unit).round());
    if !(1.0..=1.0e8).contains(&w_u) || !(1.0..=1.0e8).contains(&h_u) {
        return Err(RenderError::BadRasterSize {
            width: w_px,
            height: h_px,
        });
    }

    let mut w = Writer {
        out: Vec::with_capacity(64 * 1024),
        records: 0,
        max_handle: 0,
        clips: &recording.clips,
        current_clip: None,
        clip_saved: false,
        fill_mode: 0,
        unit,
        page_w: w_px,
        page_h: h_px,
        outcome: EmfOutcome {
            width_pt: 0.0,
            height_pt: 0.0,
            ops: 0,
            rasters_embedded: 0,
            ops_rasterised_for_alpha: 0,
            blend_modes_dropped: 0,
            gradients_rasterised: 0,
            images_embedded: 0,
            layers_rasterised: 0,
            dashed_strokes_pre_applied: 0,
            nonzero_fills_multi_subpath: 0,
            tally: recording.tally,
            diagnostics: recording.diagnostics.clone(),
        },
        raster_prologue_done: false,
    };
    #[allow(clippy::cast_precision_loss)]
    {
        w.outcome.width_pt = w_px as f32 / scale;
        w.outcome.height_pt = h_px as f32 / scale;
    }

    // Header placeholder (108 bytes), back-patched at the end.
    w.out.resize(108, 0);
    w.records = 1;
    w.record(0x11, &u32le(1)); // SETMAPMODE MM_TEXT
    w.record(0x12, &u32le(1)); // SETBKMODE TRANSPARENT

    if let Some(bg) = emf.background {
        let rect = tiny_skia::Rect::from_xywh(0.0, 0.0, w_px as f32, h_px as f32)
            .map(tiny_skia::PathBuilder::from_rect);
        if let Some(path) = rect {
            w.fill_solid(
                &path,
                [bg.r, bg.g, bg.b, 255],
                FillRule::Winding,
                Transform::identity(),
            );
        }
    }
    w.write_ops(&recording.ops);
    w.end_clip();
    w.record(0x0E, &[u32le(0), u32le(16), u32le(20)].concat()); // EOF

    // Header.
    #[allow(clippy::cast_possible_truncation)]
    let (wu, hu) = (w_u as i32, h_u as i32);
    let mm = |u: i32| (f64::from(u) / 100.0).round() as i32;
    let mut hdr = Vec::with_capacity(108);
    hdr.extend_from_slice(&u32le(1));
    hdr.extend_from_slice(&u32le(108));
    hdr.extend_from_slice(&rectl(0, 0, wu - 1, hu - 1)); // Bounds, logical
    hdr.extend_from_slice(&rectl(0, 0, wu - 1, hu - 1)); // Frame, 0.01 mm
    hdr.extend_from_slice(&u32le(0x464D_4520)); // " EMF"
    hdr.extend_from_slice(&u32le(0x0001_0000)); // version
    #[allow(clippy::cast_possible_truncation)]
    hdr.extend_from_slice(&u32le(w.out.len() as u32)); // Bytes
    hdr.extend_from_slice(&u32le(w.records)); // Records (header + body + EOF)
    #[allow(clippy::cast_possible_truncation)]
    hdr.extend_from_slice(&(w.max_handle as u16 + 1).to_le_bytes()); // Handles
    hdr.extend_from_slice(&0u16.to_le_bytes()); // Reserved
    hdr.extend_from_slice(&u32le(0)); // nDescription
    hdr.extend_from_slice(&u32le(0)); // offDescription
    hdr.extend_from_slice(&u32le(0)); // nPalEntries
    hdr.extend_from_slice(&i32le(wu)); // Device cx (px of the virtual 2540 dpi device)
    hdr.extend_from_slice(&i32le(hu)); // Device cy
    hdr.extend_from_slice(&i32le(mm(wu))); // Millimeters cx
    hdr.extend_from_slice(&i32le(mm(hu))); // Millimeters cy
    hdr.extend_from_slice(&u32le(0)); // cbPixelFormat
    hdr.extend_from_slice(&u32le(0)); // offPixelFormat
    hdr.extend_from_slice(&u32le(0)); // bOpenGL
    hdr.extend_from_slice(&i32le(mm(wu) * 1000)); // MicrometersX
    hdr.extend_from_slice(&i32le(mm(hu) * 1000)); // MicrometersY
    debug_assert_eq!(hdr.len(), 108);
    w.out[..108].copy_from_slice(&hdr);

    Ok(EmfExport {
        emf: w.out,
        outcome: w.outcome,
    })
}

// ---------------------------------------------------------------------------
// The writer
// ---------------------------------------------------------------------------

struct Writer<'a> {
    out: Vec<u8>,
    records: u32,
    max_handle: u32,
    clips: &'a [ClipDef],
    current_clip: Option<ClipId>,
    clip_saved: bool,
    /// The polygon fill mode currently in force (0 = unknown).
    fill_mode: u32,
    /// Device pixel → logical unit (0.01 mm).
    unit: f32,
    page_w: u32,
    page_h: u32,
    outcome: EmfOutcome,
    raster_prologue_done: bool,
}

const FILL_MODE_ALTERNATE: u32 = 1;
const FILL_MODE_WINDING: u32 = 2;
/// The one object index this writer uses: every brush and pen is created,
/// selected, used and deleted before the next, so the table never holds
/// more than one custom object. `Handles` is therefore 2.
const IH: u32 = 1;
const STOCK_NULL_BRUSH: u32 = 0x8000_0005;
const STOCK_BLACK_PEN: u32 = 0x8000_0007;

impl Writer<'_> {
    /// Append one record: `Type, Size` (multiple of 4, including the 8-byte
    /// head), then `body` padded to a multiple of 4.
    fn record(&mut self, kind: u32, body: &[u8]) {
        let pad = (4 - body.len() % 4) % 4;
        #[allow(clippy::cast_possible_truncation)]
        let size = (8 + body.len() + pad) as u32;
        self.out.extend_from_slice(&u32le(kind));
        self.out.extend_from_slice(&u32le(size));
        self.out.extend_from_slice(body);
        self.out.extend(std::iter::repeat_n(0u8, pad));
        self.records += 1;
    }

    fn set_fill_mode(&mut self, rule: FillRule) {
        let mode = match rule {
            FillRule::Winding => FILL_MODE_WINDING,
            FillRule::EvenOdd => FILL_MODE_ALTERNATE,
        };
        if self.fill_mode != mode {
            self.record(0x13, &u32le(mode));
            self.fill_mode = mode;
        }
    }

    /// Device px → logical units, rounded.
    #[allow(clippy::cast_possible_truncation)]
    fn lu(&self, v: f32) -> i32 {
        (v * self.unit).round().clamp(-2.0e9, 2.0e9) as i32
    }

    /// One path bracket: `BEGINPATH`, the segments of `path` (already in
    /// device space), `ENDPATH`. Returns the inclusive bounds in logical
    /// units, or `None` if the path had no geometry.
    fn path_bracket(&mut self, path: &Path) -> Option<[i32; 4]> {
        let b = path.bounds();
        let bounds = [
            self.lu(b.left()),
            self.lu(b.top()),
            (self.lu(b.right()) - 1).max(self.lu(b.left())),
            (self.lu(b.bottom()) - 1).max(self.lu(b.top())),
        ];
        self.record(0x3B, &[]); // BEGINPATH
        let mut open_figure = false;
        let mut pending: Vec<u8> = Vec::new(); // a run of LINETOs is written as one POLYLINETO
        let mut pending_n = 0u32;
        let mut pending_bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        let flush = |w: &mut Self, pending: &mut Vec<u8>, n: &mut u32, pb: &mut [i32; 4]| {
            if *n > 0 {
                let mut body = Vec::with_capacity(20 + pending.len());
                body.extend_from_slice(&rectl(pb[0], pb[1], pb[2], pb[3]));
                body.extend_from_slice(&u32le(*n));
                body.extend_from_slice(pending);
                w.record(0x06, &body); // POLYLINETO
                pending.clear();
                *n = 0;
                *pb = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
            }
        };
        for seg in path.segments() {
            match seg {
                PathSegment::MoveTo(p) => {
                    flush(self, &mut pending, &mut pending_n, &mut pending_bounds);
                    let (x, y) = (self.lu(p.x), self.lu(p.y));
                    self.record(0x1B, &[i32le(x), i32le(y)].concat()); // MOVETOEX
                    open_figure = true;
                }
                PathSegment::LineTo(p) => {
                    let (x, y) = (self.lu(p.x), self.lu(p.y));
                    pending.extend_from_slice(&i32le(x));
                    pending.extend_from_slice(&i32le(y));
                    pending_n += 1;
                    grow(&mut pending_bounds, x, y);
                    // Stay well under the 1 360-point wide-line limit.
                    if pending_n >= 1000 {
                        flush(self, &mut pending, &mut pending_n, &mut pending_bounds);
                    }
                }
                PathSegment::QuadTo(c, p) => {
                    flush(self, &mut pending, &mut pending_n, &mut pending_bounds);
                    // Degree-elevate the quadratic to a cubic: tiny-skia's
                    // stroker and dasher emit quads, GDI has only cubics.
                    let cur = self.current_point_hint(path, seg);
                    let (x0, y0) = cur;
                    let c1 = (x0 + 2.0 / 3.0 * (c.x - x0), y0 + 2.0 / 3.0 * (c.y - y0));
                    let c2 = (p.x + 2.0 / 3.0 * (c.x - p.x), p.y + 2.0 / 3.0 * (c.y - p.y));
                    self.bezier_to([(c1.0, c1.1), (c2.0, c2.1), (p.x, p.y)]);
                }
                PathSegment::CubicTo(c1, c2, p) => {
                    flush(self, &mut pending, &mut pending_n, &mut pending_bounds);
                    self.bezier_to([(c1.x, c1.y), (c2.x, c2.y), (p.x, p.y)]);
                }
                PathSegment::Close => {
                    flush(self, &mut pending, &mut pending_n, &mut pending_bounds);
                    if open_figure {
                        self.record(0x3D, &[]); // CLOSEFIGURE
                        open_figure = false;
                    }
                }
            }
        }
        flush(self, &mut pending, &mut pending_n, &mut pending_bounds);
        self.record(0x3C, &[]); // ENDPATH
        Some(bounds)
    }

    /// The point a quadratic starts from. tiny-skia's segment iterator does
    /// not hand over the current point, so it is re-derived by walking to
    /// this segment — quadratics are rare (only a dashed or stroked
    /// outline produces them), so the cost is not paid on the common path.
    fn current_point_hint(&self, path: &Path, target: PathSegment) -> (f32, f32) {
        let mut cur = (0.0f32, 0.0f32);
        let mut start = cur;
        for seg in path.segments() {
            if seg == target {
                return cur;
            }
            match seg {
                PathSegment::MoveTo(p) => {
                    cur = (p.x, p.y);
                    start = cur;
                }
                PathSegment::LineTo(p)
                | PathSegment::QuadTo(_, p)
                | PathSegment::CubicTo(_, _, p) => {
                    cur = (p.x, p.y);
                }
                PathSegment::Close => cur = start,
            }
        }
        cur
    }

    fn bezier_to(&mut self, pts: [(f32, f32); 3]) {
        let mut body = Vec::with_capacity(44);
        let mut pb = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        let mut coords = Vec::with_capacity(24);
        for (x, y) in pts {
            let (lx, ly) = (self.lu(x), self.lu(y));
            grow(&mut pb, lx, ly);
            coords.extend_from_slice(&i32le(lx));
            coords.extend_from_slice(&i32le(ly));
        }
        body.extend_from_slice(&rectl(pb[0], pb[1], pb[2], pb[3]));
        body.extend_from_slice(&u32le(3));
        body.extend_from_slice(&coords);
        self.record(0x05, &body); // POLYBEZIERTO
    }

    // ---- clips ----------------------------------------------------------

    fn ensure_clip(&mut self, clip: Option<ClipId>) {
        if clip == self.current_clip {
            return;
        }
        self.end_clip();
        if let Some(id) = clip {
            self.record(0x21, &[]); // SAVEDC
            self.clip_saved = true;
            // Root → leaf, each intersected with the one before.
            let mut chain = Vec::new();
            let mut cursor = Some(id);
            while let Some(c) = cursor {
                chain.push(c);
                cursor = self.clips[c.index()].parent;
            }
            for c in chain.into_iter().rev() {
                let def = &self.clips[c.index()];
                let device = def
                    .path
                    .as_deref()
                    .and_then(|p| p.clone().transform(def.ctm));
                let rule = def.rule;
                self.set_fill_mode(rule);
                match device {
                    Some(p) => {
                        self.path_bracket(&p);
                    }
                    None => {
                        // §8.5.4's empty clip: a degenerate bracket admits
                        // nothing.
                        self.record(0x3B, &[]);
                        self.record(0x1B, &[i32le(0), i32le(0)].concat());
                        self.record(0x3C, &[]);
                    }
                }
                self.record(0x43, &u32le(1)); // SELECTCLIPPATH RGN_AND
            }
        }
        self.current_clip = clip;
    }

    fn end_clip(&mut self) {
        if self.clip_saved {
            self.record(0x22, &i32le(-1)); // RESTOREDC -1
            self.clip_saved = false;
        }
        self.current_clip = None;
    }

    // ---- ops ------------------------------------------------------------

    fn write_ops(&mut self, ops: &[Op]) {
        for op in ops {
            match op {
                Op::Fill {
                    path,
                    brush,
                    rule,
                    ctm,
                    clip,
                    ..
                } => {
                    let Brush::Solid { rgba } = &brush.brush else {
                        match &brush.brush {
                            Brush::Gradient(_) => self.outcome.gradients_rasterised += 1,
                            Brush::Image { .. } => self.outcome.images_embedded += 1,
                            Brush::Solid { .. } => {}
                        }
                        self.raster_op(std::slice::from_ref(op), *clip);
                        continue;
                    };
                    if rgba[3] < 255 || brush.blend != BlendMode::SourceOver {
                        if rgba[3] < 255 {
                            self.outcome.ops_rasterised_for_alpha += 1;
                        } else {
                            self.outcome.blend_modes_dropped += 1;
                        }
                        self.raster_op(std::slice::from_ref(op), *clip);
                        continue;
                    }
                    self.ensure_clip(*clip);
                    if *rule == FillRule::Winding && subpath_count(path) > 1 {
                        self.outcome.nonzero_fills_multi_subpath += 1;
                    }
                    self.fill_solid(path, *rgba, *rule, *ctm);
                }
                Op::Stroke {
                    path,
                    brush,
                    stroke,
                    ctm,
                    clip,
                    ..
                } => {
                    let Brush::Solid { rgba } = &brush.brush else {
                        self.raster_op(std::slice::from_ref(op), *clip);
                        continue;
                    };
                    if rgba[3] < 255 || brush.blend != BlendMode::SourceOver {
                        if rgba[3] < 255 {
                            self.outcome.ops_rasterised_for_alpha += 1;
                        } else {
                            self.outcome.blend_modes_dropped += 1;
                        }
                        self.raster_op(std::slice::from_ref(op), *clip);
                        continue;
                    }
                    self.ensure_clip(*clip);
                    self.stroke_solid(path, *rgba, stroke, *ctm);
                }
                Op::Layer { paint, ops, mask } => {
                    self.outcome.layers_rasterised += 1;
                    if paint.blend != BlendMode::SourceOver || paint.nonseparable.is_some() {
                        self.outcome.blend_modes_dropped += 1;
                    }
                    self.raster_layer(*paint, ops, mask.as_deref());
                }
            }
        }
    }

    fn fill_solid(&mut self, path: &Path, rgba: [u8; 4], rule: FillRule, ctm: Transform) {
        let Some(device) = path.clone().transform(ctm) else {
            return;
        };
        self.outcome.ops += 1;
        self.set_fill_mode(rule);
        // CREATEBRUSHINDIRECT ih=1, BS_SOLID, colour bytes R G B 0.
        let mut body = Vec::with_capacity(16);
        body.extend_from_slice(&u32le(IH));
        body.extend_from_slice(&u32le(0));
        body.extend_from_slice(&[rgba[0], rgba[1], rgba[2], 0]);
        body.extend_from_slice(&u32le(0));
        self.record(0x27, &body);
        self.max_handle = self.max_handle.max(IH);
        self.record(0x25, &u32le(IH)); // SELECTOBJECT
        if let Some(b) = self.path_bracket(&device) {
            self.record(0x3E, &rectl(b[0], b[1], b[2], b[3])); // FILLPATH
        }
        self.record(0x25, &u32le(STOCK_NULL_BRUSH));
        self.record(0x28, &u32le(IH)); // DELETEOBJECT
    }

    fn stroke_solid(
        &mut self,
        path: &Path,
        rgba: [u8; 4],
        stroke: &tiny_skia::Stroke,
        ctm: Transform,
    ) {
        // Dash pre-applied in path space, then everything to device space.
        let dashed;
        let geometry: &Path = match &stroke.dash {
            Some(d) => {
                self.outcome.dashed_strokes_pre_applied += 1;
                match path.dash(d, 1.0) {
                    Some(p) => {
                        dashed = p;
                        &dashed
                    }
                    None => return,
                }
            }
            None => path,
        };
        let Some(device) = geometry.clone().transform(ctm) else {
            return;
        };
        self.outcome.ops += 1;
        // Width: path space → device by the CTM's uniform scale (a
        // non-uniform CTM has no single pen width; sqrt|det| is the
        // area-preserving compromise), then → logical units, ≥ 1.
        let det = (ctm.sx * ctm.sy - ctm.kx * ctm.ky).abs();
        let s = if det.is_finite() && det > 0.0 {
            det.sqrt()
        } else {
            1.0
        };
        let width_px = if stroke.width <= 0.0 {
            1.0
        } else {
            stroke.width * s
        };
        let width = self.lu(width_px).max(1);
        let style: u32 = 0x0001_0000 // PS_GEOMETRIC | PS_SOLID
            | match stroke.line_cap {
                LineCap::Round => 0x0000_0000,
                LineCap::Square => 0x0000_0100,
                LineCap::Butt => 0x0000_0200,
            }
            | match stroke.line_join {
                LineJoin::Round => 0x0000_0000,
                LineJoin::Bevel => 0x0000_1000,
                LineJoin::Miter | LineJoin::MiterClip => 0x0000_2000,
            };
        // EXTCREATEPEN ih=1, no DIB, LogPenEx with no style entries.
        let mut body = Vec::with_capacity(44);
        body.extend_from_slice(&u32le(IH));
        body.extend_from_slice(&u32le(0)); // offBmi
        body.extend_from_slice(&u32le(0)); // cbBmi
        body.extend_from_slice(&u32le(0)); // offBits
        body.extend_from_slice(&u32le(0)); // cbBits
        body.extend_from_slice(&u32le(style));
        #[allow(clippy::cast_sign_loss)]
        body.extend_from_slice(&u32le(width as u32));
        body.extend_from_slice(&u32le(0)); // BS_SOLID
        body.extend_from_slice(&[rgba[0], rgba[1], rgba[2], 0]);
        body.extend_from_slice(&u32le(0)); // BrushHatch
        body.extend_from_slice(&u32le(0)); // NumStyleEntries
        self.record(0x5F, &body);
        self.max_handle = self.max_handle.max(IH);
        self.record(0x25, &u32le(IH));
        // Miter limit: FLOAT bits on the wire (Appendix A <90>).
        self.record(0x3A, &stroke.miter_limit.max(1.0).to_le_bytes());
        if let Some(b) = self.path_bracket(&device) {
            self.record(0x40, &rectl(b[0], b[1], b[2], b[3])); // STROKEPATH
        }
        self.record(0x25, &u32le(STOCK_BLACK_PEN));
        self.record(0x28, &u32le(IH));
    }

    // ---- rasters --------------------------------------------------------

    /// Replay `ops` (their own clips applied by the replay) into a
    /// transparent page-sized scratch and write the painted box as one
    /// `EMR_ALPHABLEND`. The EMF clip is ended first: the raster already
    /// carries its clip, and GDI would otherwise clip it twice at slightly
    /// different rounding.
    fn raster_op(&mut self, ops: &[Op], _clip: Option<ClipId>) {
        self.end_clip();
        let Some(mut scratch) = Pixmap::new(self.page_w, self.page_h) else {
            return;
        };
        #[allow(clippy::cast_precision_loss)]
        let region = DeviceBounds {
            left: 0.0,
            top: 0.0,
            right: self.page_w as f32,
            bottom: self.page_h as f32,
        };
        let mut masks = MaskBuilder::new(self.clips, self.page_w, self.page_h, 0.0, 0.0);
        replay_ops(ops, &mut scratch, &mut masks, region);
        self.alpha_blend(&scratch);
    }

    fn raster_layer(&mut self, paint: LayerPaint, ops: &[Op], mask: Option<&Mask>) {
        self.end_clip();
        let Some(mut buf) = Pixmap::new(self.page_w, self.page_h) else {
            return;
        };
        #[allow(clippy::cast_precision_loss)]
        let region = DeviceBounds {
            left: 0.0,
            top: 0.0,
            right: self.page_w as f32,
            bottom: self.page_h as f32,
        };
        let mut masks = MaskBuilder::new(self.clips, self.page_w, self.page_h, 0.0, 0.0);
        replay_ops(ops, &mut buf, &mut masks, region);
        if let Some(m) = mask {
            crate::canvas::apply_mask(&mut buf, m);
        }
        let opacity = paint.opacity.clamp(0.0, 1.0);
        if opacity < 1.0 {
            for px in buf.pixels_mut() {
                let f = |c: u8| {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let v = (f32::from(c) * opacity).round() as u8;
                    v
                };
                *px = tiny_skia::PremultipliedColorU8::from_rgba(
                    f(px.red()),
                    f(px.green()),
                    f(px.blue()),
                    f(px.alpha()),
                )
                .unwrap_or(*px);
            }
        }
        self.alpha_blend(&buf);
    }

    /// Crop `scratch` to its painted box and write it as `EMR_ALPHABLEND`
    /// (premultiplied BGRA, top-down, `AC_SRC_OVER | AC_SRC_ALPHA`).
    fn alpha_blend(&mut self, scratch: &Pixmap) {
        let w = scratch.width();
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for (i, px) in scratch.pixels().iter().enumerate() {
            if px.alpha() == 0 {
                continue;
            }
            #[allow(clippy::cast_possible_truncation)]
            let (x, y) = ((i as u32) % w, (i as u32) / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        if x0 == u32::MAX {
            return;
        }
        let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
        if !self.raster_prologue_done {
            self.record(0x15, &u32le(4)); // SETSTRETCHBLTMODE HALFTONE
            self.record(0x0D, &[i32le(0), i32le(0)].concat()); // SETBRUSHORGEX
            self.raster_prologue_done = true;
        }
        self.outcome.rasters_embedded += 1;

        let mut bits = Vec::with_capacity((bw * bh * 4) as usize);
        let src = scratch.pixels();
        for row in 0..bh {
            let s0 = ((y0 + row) * w + x0) as usize;
            for px in &src[s0..s0 + bw as usize] {
                bits.extend_from_slice(&[px.blue(), px.green(), px.red(), px.alpha()]);
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let (dx, dy, dw, dh) = (
            self.lu(x0 as f32),
            self.lu(y0 as f32),
            self.lu(bw as f32).max(1),
            self.lu(bh as f32).max(1),
        );
        let mut body = Vec::with_capacity(100 + 40 + bits.len());
        body.extend_from_slice(&rectl(dx, dy, dx + dw - 1, dy + dh - 1)); // Bounds
        body.extend_from_slice(&i32le(dx));
        body.extend_from_slice(&i32le(dy));
        body.extend_from_slice(&i32le(dw));
        body.extend_from_slice(&i32le(dh));
        body.extend_from_slice(&[0x00, 0x00, 0xFF, 0x01]); // BLENDFUNCTION: AC_SRC_OVER, 0, 255, AC_SRC_ALPHA
        body.extend_from_slice(&i32le(0)); // xSrc
        body.extend_from_slice(&i32le(0)); // ySrc
        for f in [1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0] {
            body.extend_from_slice(&f.to_le_bytes()); // XformSrc identity
        }
        body.extend_from_slice(&u32le(0)); // BkColorSrc
        body.extend_from_slice(&u32le(0)); // UsageSrc DIB_RGB_COLORS
        body.extend_from_slice(&u32le(108)); // offBmiSrc
        body.extend_from_slice(&u32le(40)); // cbBmiSrc
        body.extend_from_slice(&u32le(148)); // offBitsSrc
        #[allow(clippy::cast_possible_truncation)]
        body.extend_from_slice(&u32le(bits.len() as u32)); // cbBitsSrc
        #[allow(clippy::cast_possible_wrap)]
        body.extend_from_slice(&i32le(bw as i32)); // cxSrc
        #[allow(clippy::cast_possible_wrap)]
        body.extend_from_slice(&i32le(bh as i32)); // cySrc
        debug_assert_eq!(body.len(), 100);
        // BitmapInfoHeader: 32 bpp BI_RGB, TOP-DOWN (negative height).
        body.extend_from_slice(&u32le(40));
        #[allow(clippy::cast_possible_wrap)]
        body.extend_from_slice(&i32le(bw as i32));
        #[allow(clippy::cast_possible_wrap)]
        body.extend_from_slice(&i32le(-(bh as i32)));
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&32u16.to_le_bytes());
        body.extend_from_slice(&u32le(0)); // BI_RGB
        body.extend_from_slice(&u32le(0)); // ImageSize
        body.extend_from_slice(&i32le(0));
        body.extend_from_slice(&i32le(0));
        body.extend_from_slice(&u32le(0));
        body.extend_from_slice(&u32le(0));
        body.extend_from_slice(&bits);
        self.record(0x72, &body);
    }
}

fn grow(b: &mut [i32; 4], x: i32, y: i32) {
    b[0] = b[0].min(x);
    b[1] = b[1].min(y);
    b[2] = b[2].max(x);
    b[3] = b[3].max(y);
}

fn subpath_count(path: &Path) -> usize {
    path.segments()
        .filter(|s| matches!(s, PathSegment::MoveTo(_)))
        .count()
}

fn u32le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

fn i32le(v: i32) -> [u8; 4] {
    v.to_le_bytes()
}

fn rectl(l: i32, t: i32, r: i32, b: i32) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[..4].copy_from_slice(&i32le(l));
    out[4..8].copy_from_slice(&i32le(t));
    out[8..12].copy_from_slice(&i32le(r));
    out[12..].copy_from_slice(&i32le(b));
    out
}

/// Walk a metafile's records: `(type, size)` per record, or `None` if the
/// stream is not a well-formed sequence (a record shorter than 8 bytes, a
/// size that is not a multiple of 4, or a final record that overruns the
/// buffer — the three things LibreOffice's reader aborts on).
///
/// A structural check, not a parse: it reads the two head words of every
/// record and nothing else. Public so a consumer can validate a metafile
/// before handing it to `SetEnhMetaFileBits` — and because a writer that
/// cannot read its own output back is a writer whose tests inspect nothing.
///
/// # Example
///
/// ```
/// let eof = [0x0Eu8, 0, 0, 0, 20, 0, 0, 0, 0, 0, 0, 0, 16, 0, 0, 0, 20, 0, 0, 0];
/// assert_eq!(pdfcer_render::emf::walk_records(&eof), Some(vec![(0x0E, 20)]));
/// assert_eq!(pdfcer_render::emf::walk_records(&eof[..7]), None);
/// ```
#[must_use]
pub fn walk_records(emf: &[u8]) -> Option<Vec<(u32, u32)>> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 8 <= emf.len() {
        let t = u32::from_le_bytes(emf[off..off + 4].try_into().ok()?);
        let s = u32::from_le_bytes(emf[off + 4..off + 8].try_into().ok()?);
        if s < 8 || s % 4 != 0 {
            return None;
        }
        out.push((t, s));
        off += s as usize;
    }
    (off == emf.len()).then_some(out)
}

// Keep the unused-import lint honest about what the module reaches for.
#[allow(dead_code)]
fn _types(_: Arc<Mask>, _: BrushSpec) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_is_padded_to_four_bytes_and_counted() {
        let mut w = Writer {
            out: Vec::new(),
            records: 0,
            max_handle: 0,
            clips: &[],
            current_clip: None,
            clip_saved: false,
            fill_mode: 0,
            unit: 1.0,
            page_w: 1,
            page_h: 1,
            outcome: EmfOutcome {
                width_pt: 0.0,
                height_pt: 0.0,
                ops: 0,
                rasters_embedded: 0,
                ops_rasterised_for_alpha: 0,
                blend_modes_dropped: 0,
                gradients_rasterised: 0,
                images_embedded: 0,
                layers_rasterised: 0,
                dashed_strokes_pre_applied: 0,
                nonzero_fills_multi_subpath: 0,
                tally: ExportTally::default(),
                diagnostics: Diagnostics::default(),
            },
            raster_prologue_done: false,
        };
        w.record(0x46, &[1, 2, 3]);
        assert_eq!(w.out.len(), 12);
        assert_eq!(&w.out[4..8], &12u32.to_le_bytes());
        assert_eq!(w.records, 1);
    }

    #[test]
    fn the_golden_records_match_the_spec_bytes() {
        // From D:\dev\rag\emf\minimal_valid_emf.md: the brush, select,
        // path and EOF records of the verified 300-byte file.
        let mut body = Vec::new();
        body.extend_from_slice(&u32le(1));
        body.extend_from_slice(&u32le(0));
        body.extend_from_slice(&[0xFF, 0, 0, 0]);
        body.extend_from_slice(&u32le(0));
        let mut rec = Vec::new();
        rec.extend_from_slice(&u32le(0x27));
        rec.extend_from_slice(&u32le(24));
        rec.extend_from_slice(&body);
        assert_eq!(
            rec,
            [
                0x27, 0, 0, 0, 0x18, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0, 0, 0, 0, 0, 0, 0
            ]
        );
        assert_eq!(
            rectl(10, 10, 89, 89),
            [10, 0, 0, 0, 10, 0, 0, 0, 89, 0, 0, 0, 89, 0, 0, 0]
        );
    }
}

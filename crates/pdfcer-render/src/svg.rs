//! # SVG export — a page written as vector graphics (`Pass 248.1`)
//!
//! Turns one page into an SVG 1.1 document (plus the one CSS Compositing
//! property, `mix-blend-mode`) that Inkscape, Word/PowerPoint, LibreOffice
//! and every browser open — **from the renderer's own recording**, so the
//! SVG contains exactly the geometry the raster painted, in the same device
//! space, at the same scale.
//!
//! ## 1. The one design decision
//!
//! Two routes were available (`docs/export-and-copy-out-plan.md` §1):
//! walk the editing model (`pdfcer_core::vector::PageObjects`, the way DXF
//! export does), or consume the display list ([`crate::display_list`]).
//! The editing model carries no images, clips, transparency, blend modes,
//! shadings or Type 3 glyphs, and writing an SVG from it would have been a
//! **second interpreter** of the content stream — the trap this project has
//! now written down more than three times. So this module consumes
//! [`crate::display_list::Op`]s: one interpreter, no drift, and the SVG
//! renders identically to `pdfcer export-image --format png` of the same
//! page (the oracle test in `tests/export_svg.rs` rasterises the SVG with
//! `resvg` and compares).
//!
//! The cost of that route is the display list's refusal list — shadings,
//! soft masks, overprint, non-separable per-paint blends, non-isolated
//! groups — and it is paid **without weakening the cache's posture**
//! (`R211`): the recorder has an *export* mode ([`crate::display_list::ExportState`])
//! in which every site that would refuse instead rasterises that ONE
//! operator at the recording scale and records it as an image fill (or,
//! since `Pass 248.3`, records an axial / focal-radial shading as a NATIVE
//! gradient brush), keeps
//! a soft mask with its layer, or records the isolated / `Normal`
//! approximation — and COUNTS it, so [`SvgOutcome::tally`] can say what is
//! raster inside the file and what was approximated. Rule 4 (fuzzy, never
//! sneaky): the SVG never refuses, and never pretends.
//!
//! ## 2. Coordinate system, units, resolution
//!
//! The recording scale is `raster_dpi / 72` ([`SvgOptions::raster_dpi`],
//! default 300). Every vector coordinate is written in **device pixels at
//! that scale**, and the root element carries `width`/`height` in **points**
//! plus a `viewBox` in device pixels, so a consumer places the page at its
//! physical size while the vectors keep full precision. Recording at the
//! raster DPI rather than at 72 is what makes an embedded raster (a
//! shading, a soft mask) print-grade instead of screen-grade, and it also
//! makes every device-dependent decision the interpreter takes — hairline
//! width, image minification filter — the right one for that resolution.
//!
//! `page_device_geometry` bakes `/MediaBox`/`/CropBox` origin, the y-flip
//! and `/Rotate` into the recording CTM, so SVG's y-down space is the
//! device space with no further transform.
//!
//! ## 3. What each op becomes
//!
//! | recorded | SVG |
//! |---|---|
//! | `Op::Fill` with a solid brush | `<path d="…" fill="rgb()" fill-opacity fill-rule>` — the path is pre-transformed into device space |
//! | `Op::Fill` with an image brush | `<clipPath>` of the fill path, then `<image>` with `transform="matrix(ctm ∘ image_to_user)"` and a PNG-with-alpha data URI |
//! | `Op::Fill` with a gradient brush (`Pass 248.3`: an axial or focal-radial shading) | `<linearGradient>` / `<radialGradient gradientUnits="userSpaceOnUse" gradientTransform=…>` with the ramp thinned to its informative stops, `spreadMethod="pad"`; an `/Extend` = false end becomes a `<clipPath>` (the band between the end perpendiculars, or the outer circle) |
//! | `Op::Stroke` | `<path transform="matrix(ctm)" fill="none" stroke=… stroke-width=…>` — path space kept so a non-uniform CTM scales the width the way §8.4.3.2 says; a hairline gets `vector-effect="non-scaling-stroke"`; a dash array is **pre-applied** with `Path::dash` (tiny-skia exposes no accessor for the array) and counted |
//! | `Op::Layer` | `<g opacity style="mix-blend-mode:…" mask="url(#m)">` — group opacity in SVG *is* isolated-group compositing, which is the semantics the layer recorded |
//! | a clip | `<clipPath id clip-path="url(#parent)">` — intersection with the enclosing clip is expressed on the `<clipPath>` element itself, so a leaf reference carries the whole chain |
//! | a kept soft mask | `<mask maskUnits="userSpaceOnUse" style="color-interpolation:sRGB"><image …grey PNG…>` — luminance from the coverage bytes, 1:1 |
//!
//! Text is glyph outlines. That is what "renders identically everywhere"
//! costs, and what every PDF→SVG converter people trust does by default;
//! the CLI says so once per export.
//!
//! ## 4. What Word's importer needs, and why the file is shaped this way
//!
//! Word/PowerPoint's SVG importer is not a browser: no `<style>` blocks,
//! no CSS classes, no external references, `<image>` must be a data URI,
//! `mix-blend-mode` is ignored (shown `Normal`). So everything is a
//! presentation attribute, every raster is inline, and the disclosure —
//! not the file — carries the fact that a blend mode will not survive
//! there. Inkscape and browsers honour all of it.
//!
//! ## Failure modes
//!
//! [`crate::RenderError`] — the same set a render produces. `BadRasterSize`
//! if the page has no extent or the scale would exceed the guard;
//! `Content` for an undecodable stream; `Cancelled`. A page whose
//! resolution would put the CTM past `f32` precision is refused as
//! `BadRasterSize` too, since an export cannot fall back the way the cache
//! does.

use std::fmt::Write as _;
use std::sync::Arc;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::Page;
use pdfcer_core::view::DocumentView;
use tiny_skia::{
    BlendMode, FillRule, LineCap, LineJoin, Mask, Path, PathSegment, Pixmap, Transform,
};

use crate::RenderError;
use crate::canvas::{Brush, BrushSpec, LayerPaint};
use crate::display_list::{ClipDef, ClipId, ExportTally, Op, record_page_for_export};
use crate::export::Rgb;
use crate::font::RenderOptions;
use crate::interpret::Diagnostics;

pub use crate::display_list::ExportTally as SvgTally;

/// How an SVG is written.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SvgOptions {
    /// The recording resolution — the DPI at which anything that has to
    /// be embedded as raster (a shading, a soft mask, an image the file
    /// carried) is sampled, and the scale every coordinate is written at.
    /// Vectors are exact at any value; **300** (print grade) is the
    /// default because an embedded raster cannot be re-sampled later.
    pub raster_dpi: f32,
    /// An opaque background rectangle under the page, or `None` for a
    /// transparent page — the default, because SVG carries transparency
    /// natively and painting paper into a vector file is a choice.
    pub background: Option<Rgb>,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            raster_dpi: 300.0,
            background: None,
        }
    }
}

impl SvgOptions {
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

/// What an export produced beside the bytes — the disclosure.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SvgOutcome {
    /// The page's physical size, points.
    pub width_pt: f32,
    /// The page's physical size, points.
    pub height_pt: f32,
    /// The recording scale (`raster_dpi / 72`) every coordinate is in.
    pub scale: f32,
    /// Drawing ops written (fills, strokes and groups, recursively).
    pub ops: usize,
    /// `<image>` elements — the file's own images plus every rasterised
    /// fallback in [`Self::tally`].
    pub images_embedded: usize,
    /// Strokes whose dash pattern was pre-applied to the geometry. The
    /// picture is exact; what is lost is the pattern's editability.
    pub dashed_strokes_pre_applied: usize,
    /// Elements written with a `mix-blend-mode` other than normal — the
    /// property Word's importer ignores.
    pub blend_modes_used: usize,
    /// What had to be rasterised or approximated. `is_exact()` when the
    /// whole page went out as geometry.
    pub tally: ExportTally,
    /// The interpreter's honesty report for the walk that produced this
    /// file — the same counters a render returns.
    pub diagnostics: Diagnostics,
}

/// An exported page.
#[derive(Debug, Clone)]
pub struct SvgExport {
    /// The SVG document, UTF-8, no XML declaration (a consumer pasting it
    /// onto a clipboard or into an HTML page does not want one; a file
    /// writer may prepend it).
    pub svg: String,
    /// The disclosure.
    pub outcome: SvgOutcome,
}

/// Export one page of a loaded document as SVG.
///
/// `render` carries every render-affecting choice — fonts, annotation
/// scope, layer visibility, the overprint and spot settings — exactly as
/// for a raster; `svg` carries the two SVG-specific ones. The backdrop
/// field of `render` is ignored: an SVG's background is
/// [`SvgOptions::background`].
///
/// # Errors
///
/// [`RenderError`], as for a render — see the module docs.
///
/// # Example
///
/// ```no_run
/// use pdfcer_core::document::Document;
/// use pdfcer_render::svg::{SvgOptions, export_svg};
/// use pdfcer_render::RenderOptions;
///
/// let doc = Document::from_bytes(std::fs::read("page.pdf")?)?;
/// let page = pdfcer_core::page_tree::pages(&doc)?.remove(0);
/// let out = export_svg(&doc, &page, &RenderOptions::default(), &SvgOptions::default())?;
/// std::fs::write("page.svg", out.svg)?;
/// println!("{} ops, exact: {}", out.outcome.ops, out.outcome.tally.is_exact());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn export_svg(
    doc: &Document,
    page: &Page,
    render: &RenderOptions,
    svg: &SvgOptions,
) -> Result<SvgExport, RenderError> {
    export_svg_view(&doc.view(), page, render, svg)
}

/// [`export_svg`] over a [`DocumentView`] — an edit session's live view,
/// so the SVG shows what the canvas shows.
///
/// # Errors
///
/// As [`export_svg`].
pub fn export_svg_view(
    view: &DocumentView<'_>,
    page: &Page,
    render: &RenderOptions,
    svg: &SvgOptions,
) -> Result<SvgExport, RenderError> {
    let scale = if svg.raster_dpi.is_finite() && svg.raster_dpi > 0.0 {
        svg.raster_dpi / 72.0
    } else {
        300.0 / 72.0
    };
    let recording = record_page_for_export(view, page, scale, render)?;
    let (w, h) = recording.page_size;

    let mut writer = Writer {
        clips: &recording.clips,
        clip_written: vec![false; recording.clips.len()],
        defs: String::new(),
        body: String::new(),
        next_id: 0,
        masks_written: std::collections::HashMap::new(),
        page_w: w,
        page_h: h,
        ops: 0,
        images: 0,
        dashed: 0,
        blends: 0,
    };
    if let Some(bg) = svg.background {
        let _ = writeln!(
            writer.body,
            r#"<rect x="0" y="0" width="{w}" height="{h}" fill="{}"/>"#,
            bg.to_hex()
        );
    }
    writer.write_ops(&recording.ops, 0);

    #[allow(clippy::cast_precision_loss)]
    let (width_pt, height_pt) = (w as f32 / scale, h as f32 / scale);
    let mut out = String::with_capacity(writer.defs.len() + writer.body.len() + 512);
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{}pt" height="{}pt" viewBox="0 0 {w} {h}">"#,
        num(width_pt),
        num(height_pt)
    );
    out.push('\n');
    if !writer.defs.is_empty() {
        out.push_str("<defs>\n");
        out.push_str(&writer.defs);
        out.push_str("</defs>\n");
    }
    out.push_str(&writer.body);
    out.push_str("</svg>\n");

    Ok(SvgExport {
        svg: out,
        outcome: SvgOutcome {
            width_pt,
            height_pt,
            scale,
            ops: writer.ops,
            images_embedded: writer.images,
            dashed_strokes_pre_applied: writer.dashed,
            blend_modes_used: writer.blends,
            tally: recording.tally,
            diagnostics: recording.diagnostics,
        },
    })
}

// ---------------------------------------------------------------------------
// The writer
// ---------------------------------------------------------------------------

struct Writer<'a> {
    clips: &'a [ClipDef],
    clip_written: Vec<bool>,
    defs: String,
    body: String,
    next_id: usize,
    /// `<mask>` ids already written, by the `Arc<Mask>`'s address — a
    /// run of glyphs under one soft mask shares one definition.
    masks_written: std::collections::HashMap<usize, String>,
    page_w: u32,
    page_h: u32,
    ops: usize,
    images: usize,
    dashed: usize,
    blends: usize,
}

impl Writer<'_> {
    fn fresh_id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}{}", self.next_id)
    }

    fn indent(&mut self, depth: usize) {
        for _ in 0..depth {
            self.body.push(' ');
        }
    }

    /// The `clip-path="url(#cN)"` attribute for a recorded clip, writing
    /// the definition (and, recursively, its parents) on first use.
    fn clip_attr(&mut self, clip: Option<ClipId>) -> String {
        let Some(id) = clip else {
            return String::new();
        };
        self.ensure_clip(id);
        format!(r#" clip-path="url(#c{})""#, id.index())
    }

    fn ensure_clip(&mut self, id: ClipId) {
        let i = id.index();
        if self.clip_written.get(i).copied().unwrap_or(true) {
            return;
        }
        self.clip_written[i] = true;
        let def = &self.clips[i];
        let parent_attr = match def.parent {
            Some(p) => {
                self.ensure_clip(p);
                format!(r#" clip-path="url(#c{})""#, p.index())
            }
            None => String::new(),
        };
        let def = &self.clips[i];
        let mut d = String::new();
        let _ = write!(d, r#"<clipPath id="c{i}"{parent_attr}>"#);
        match def
            .path
            .as_deref()
            .and_then(|p| p.clone().transform(def.ctm))
        {
            Some(path) => {
                let _ = write!(
                    d,
                    r#"<path d="{}" clip-rule="{}"/>"#,
                    path_data(&path),
                    rule_name(def.rule)
                );
            }
            // §8.5.4's empty clip admits nothing: a degenerate rectangle.
            None => d.push_str(r#"<rect x="0" y="0" width="0" height="0"/>"#),
        }
        d.push_str("</clipPath>\n");
        self.defs.push_str(&d);
    }

    fn write_ops(&mut self, ops: &[Op], depth: usize) {
        for op in ops {
            match op {
                Op::Fill {
                    path,
                    brush,
                    rule,
                    ctm,
                    clip,
                    ..
                } => self.write_fill(path, brush, *rule, *ctm, *clip, depth),
                Op::Stroke {
                    path,
                    brush,
                    stroke,
                    ctm,
                    clip,
                    ..
                } => self.write_stroke(path, brush, stroke, *ctm, *clip, depth),
                Op::Layer { paint, ops, mask } => {
                    self.write_layer(*paint, ops, mask.as_ref(), depth);
                }
            }
        }
    }

    fn write_fill(
        &mut self,
        path: &Path,
        brush: &BrushSpec,
        rule: FillRule,
        ctm: Transform,
        clip: Option<ClipId>,
        depth: usize,
    ) {
        self.ops += 1;
        let clip_attr = self.clip_attr(clip);
        let blend_attr = self.blend_attr(brush.blend);
        match &brush.brush {
            Brush::Solid { rgba } => {
                // Pre-transformed into device space: a fill has no width to
                // scale, so the transform can be folded into the numbers.
                let Some(device) = path.clone().transform(ctm) else {
                    return;
                };
                self.indent(depth);
                let _ = write!(
                    self.body,
                    r#"<path d="{}" fill="{}"{}{}{}{}{}/>"#,
                    path_data(&device),
                    rgb_css(rgba),
                    opacity_attr("fill-opacity", rgba[3]),
                    if rule == FillRule::EvenOdd {
                        r#" fill-rule="evenodd""#
                    } else {
                        ""
                    },
                    if brush.anti_alias {
                        ""
                    } else {
                        r#" shape-rendering="crispEdges""#
                    },
                    clip_attr,
                    blend_attr
                );
                self.body.push('\n');
            }
            Brush::Gradient(g) => {
                use crate::shading::GradientKind;
                // The path is pre-transformed to device space like any
                // fill; the gradient's own space reaches device through
                // ctm ∘ spec.transform, written as `gradientTransform`
                // with userSpaceOnUse units.
                let Some(device) = path.clone().transform(ctm) else {
                    return;
                };
                let to_device = ctm.pre_concat(g.transform);
                let id = self.fresh_id("g");
                let mut def = String::new();
                match g.kind {
                    GradientKind::Linear { x0, y0, x1, y1 } => {
                        let _ = write!(
                            def,
                            r#"<linearGradient id="{id}" gradientUnits="userSpaceOnUse" x1="{}" y1="{}" x2="{}" y2="{}" gradientTransform="{}" spreadMethod="pad">"#,
                            num(x0),
                            num(y0),
                            num(x1),
                            num(y1),
                            matrix(to_device)
                        );
                    }
                    GradientKind::Radial { cx, cy, r, fx, fy } => {
                        let _ = write!(
                            def,
                            r#"<radialGradient id="{id}" gradientUnits="userSpaceOnUse" cx="{}" cy="{}" r="{}" fx="{}" fy="{}" gradientTransform="{}" spreadMethod="pad">"#,
                            num(cx),
                            num(cy),
                            num(r),
                            num(fx),
                            num(fy),
                            matrix(to_device)
                        );
                    }
                }
                for (offset, c) in &g.stops {
                    let _ = write!(
                        def,
                        r#"<stop offset="{}" stop-color="rgb({},{},{})"{}/>"#,
                        num(*offset),
                        c[0],
                        c[1],
                        c[2],
                        if g.alpha < 1.0 {
                            format!(r#" stop-opacity="{}""#, num(g.alpha))
                        } else {
                            String::new()
                        }
                    );
                }
                def.push_str(match g.kind {
                    GradientKind::Linear { .. } => "</linearGradient>\n",
                    GradientKind::Radial { .. } => "</radialGradient>\n",
                });
                self.defs.push_str(&def);

                // `/Extend` false: SVG pads past the ends, PDF paints nothing
                // there. A clip in gradient space, carried to device by the
                // same transform: the band between the end perpendiculars
                // for linear, the outer circle for radial.
                let extent_clip =
                    extent_clip_path(&g.kind, g.extend).and_then(|p| p.transform(to_device));
                let extent_attr = match extent_clip {
                    Some(p) => {
                        let eid = self.fresh_id("e");
                        let _ = writeln!(
                            self.defs,
                            r#"<clipPath id="{eid}"><path d="{}"/></clipPath>"#,
                            path_data(&p)
                        );
                        format!(r#" clip-path="url(#{eid})""#)
                    }
                    None => String::new(),
                };
                self.indent(depth);
                let _ = write!(
                    self.body,
                    r#"<g{clip_attr}{blend_attr}><path d="{}" fill="url(#{id})"{}{}/></g>"#,
                    path_data(&device),
                    if rule == FillRule::EvenOdd {
                        r#" fill-rule="evenodd""#
                    } else {
                        ""
                    },
                    extent_attr
                );
                self.body.push('\n');
            }
            Brush::Image {
                texels,
                quality,
                transform,
            } => {
                self.images += 1;
                // The fill path clips the image (the paint's pattern shader
                // pads outward; the path is what bounds it).
                let Some(device) = path.clone().transform(ctm) else {
                    return;
                };
                let img_clip = self.fresh_id("i");
                let _ = writeln!(
                    self.defs,
                    r#"<clipPath id="{img_clip}"><path d="{}" clip-rule="{}"/></clipPath>"#,
                    path_data(&device),
                    rule_name(rule)
                );
                // device = ctm ∘ image_to_user ∘ texel.
                let placement = ctm.pre_concat(*transform);
                let Ok(png) = crate::export::encode_png(texels, None) else {
                    return;
                };
                self.indent(depth);
                let _ = write!(
                    self.body,
                    // ★ The clip goes on a WRAPPING group, never on the
                    // `<image>` itself: `clip-path` on an element that also
                    // carries `transform` is evaluated in the element's
                    // post-transform user space, i.e. in texel coordinates,
                    // where a device-space clip excludes everything. Found
                    // by rendering the first shading export in Inkscape:
                    // two harvested rasters, both invisible.
                    r#"<g{clip_attr}{blend_attr}><g clip-path="url(#{img_clip})"><image x="0" y="0" width="{}" height="{}" transform="{}"{}{} preserveAspectRatio="none" xlink:href="data:image/png;base64,{}"/></g></g>"#,
                    texels.width(),
                    texels.height(),
                    matrix(placement),
                    // `Nearest` is how the recorder placed a harvested raster
                    // 1:1 and how the interpreter draws a magnified image
                    // whose file asked for no interpolation.
                    if matches!(quality, tiny_skia::FilterQuality::Nearest) {
                        r#" style="image-rendering:pixelated""#
                    } else {
                        ""
                    },
                    opacity_attr("opacity", 255),
                    base64(&png)
                );
                self.body.push('\n');
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn write_stroke(
        &mut self,
        path: &Path,
        brush: &BrushSpec,
        stroke: &tiny_skia::Stroke,
        ctm: Transform,
        clip: Option<ClipId>,
        depth: usize,
    ) {
        self.ops += 1;
        let Brush::Solid { rgba } = &brush.brush else {
            // An image-brushed stroke does not occur: the interpreter only
            // strokes with a colour. Written as its outline if it ever did.
            return;
        };
        let clip_attr = self.clip_attr(clip);
        let blend_attr = self.blend_attr(brush.blend);
        // Dash: pre-applied. tiny-skia's `StrokeDash` exposes no accessor
        // for its array, and re-deriving it from the graphics state would
        // be a second reader of a value the recorder already consumed. The
        // dashed geometry is exactly what the rasteriser strokes.
        let dashed;
        let geometry: &Path = match &stroke.dash {
            Some(d) => {
                self.dashed += 1;
                match path.dash(d, 1.0) {
                    Some(p) => {
                        dashed = p;
                        &dashed
                    }
                    // Nothing survives the dash (a pattern of pure gaps);
                    // the rasteriser draws nothing either.
                    None => return,
                }
            }
            None => path,
        };
        let hairline = stroke.width <= 0.0;
        self.indent(depth);
        let _ = write!(
            self.body,
            r#"<g{clip_attr}{blend_attr}><path d="{}" transform="{}" fill="none" stroke="{}"{} stroke-width="{}"{} stroke-linecap="{}" stroke-linejoin="{}" stroke-miterlimit="{}"{}/></g>"#,
            path_data(geometry),
            matrix(ctm),
            rgb_css(rgba),
            opacity_attr("stroke-opacity", rgba[3]),
            if hairline {
                "1".to_owned()
            } else {
                num(stroke.width)
            },
            if hairline {
                r#" vector-effect="non-scaling-stroke""#
            } else {
                ""
            },
            match stroke.line_cap {
                LineCap::Butt => "butt",
                LineCap::Round => "round",
                LineCap::Square => "square",
            },
            match stroke.line_join {
                LineJoin::Miter | LineJoin::MiterClip => "miter",
                LineJoin::Round => "round",
                LineJoin::Bevel => "bevel",
            },
            num(stroke.miter_limit.max(1.0)),
            if brush.anti_alias {
                ""
            } else {
                r#" shape-rendering="crispEdges""#
            }
        );
        self.body.push('\n');
    }

    fn write_layer(
        &mut self,
        paint: LayerPaint,
        ops: &[Op],
        mask: Option<&Arc<Mask>>,
        depth: usize,
    ) {
        self.ops += 1;
        let mode = paint
            .nonseparable
            .map(|ns| match ns {
                crate::blend_nonsep::NonSeparableBlend::Hue => "hue",
                crate::blend_nonsep::NonSeparableBlend::Saturation => "saturation",
                crate::blend_nonsep::NonSeparableBlend::Color => "color",
                crate::blend_nonsep::NonSeparableBlend::Luminosity => "luminosity",
            })
            .or_else(|| blend_css(paint.blend));
        let blend_attr = match mode {
            Some(m) => {
                self.blends += 1;
                format!(r#" style="mix-blend-mode:{m}""#)
            }
            None => String::new(),
        };
        let mask_attr = match mask {
            Some(m) if self.masks_written.contains_key(&(Arc::as_ptr(m) as usize)) => {
                format!(
                    r#" mask="url(#{})""#,
                    self.masks_written[&(Arc::as_ptr(m) as usize)]
                )
            }
            Some(m) => {
                let id = self.fresh_id("m");
                self.masks_written
                    .insert(Arc::as_ptr(m) as usize, id.clone());
                match mask_png(m) {
                    Some(png) => {
                        self.images += 1;
                        let _ = writeln!(
                            self.defs,
                            r#"<mask id="{id}" maskUnits="userSpaceOnUse" x="0" y="0" width="{}" height="{}" style="color-interpolation:sRGB"><image x="0" y="0" width="{}" height="{}" xlink:href="data:image/png;base64,{}"/></mask>"#,
                            self.page_w,
                            self.page_h,
                            m.width(),
                            m.height(),
                            base64(&png)
                        );
                        format!(r#" mask="url(#{id})""#)
                    }
                    None => String::new(),
                }
            }
            None => String::new(),
        };
        let opacity = paint.opacity.clamp(0.0, 1.0);
        self.indent(depth);
        let _ = write!(
            self.body,
            "<g{}{}{}>",
            if opacity < 1.0 {
                format!(r#" opacity="{}""#, num(opacity))
            } else {
                String::new()
            },
            blend_attr,
            mask_attr
        );
        self.body.push('\n');
        self.write_ops(ops, depth + 1);
        self.indent(depth);
        self.body.push_str("</g>\n");
    }

    fn blend_attr(&mut self, blend: BlendMode) -> String {
        match blend_css(blend) {
            Some(m) => {
                self.blends += 1;
                format!(r#" style="mix-blend-mode:{m}""#)
            }
            None => String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Encoding helpers
// ---------------------------------------------------------------------------

/// The region a gradient with an `/Extend` = false end may paint, as a
/// path in GRADIENT space (`Pass 248.3`); `None` when both ends extend
/// (SVG's `pad` then matches PDF exactly).
///
/// Linear: the strip between the perpendiculars through the two end points,
/// extended `FAR` along the axis on any end that DOES extend and `FAR` to
/// either side. Radial (focal form): the outer circle when the outer end
/// does not extend; the inner "circle" is the focal point, so its end has
/// no area to withhold.
fn extent_clip_path(kind: &crate::shading::GradientKind, extend: [bool; 2]) -> Option<Path> {
    use crate::shading::GradientKind;
    const FAR: f32 = 1.0e5;
    match *kind {
        GradientKind::Linear { x0, y0, x1, y1 } => {
            if extend[0] && extend[1] {
                return None;
            }
            let (dx, dy) = (x1 - x0, y1 - y0);
            let len = (dx * dx + dy * dy).sqrt();
            if len <= 0.0 || !len.is_finite() {
                return None;
            }
            let (ux, uy) = (dx / len, dy / len);
            let (px, py) = (-uy * FAR, ux * FAR);
            let back = if extend[0] { FAR } else { 0.0 };
            let fwd = if extend[1] { FAR } else { 0.0 };
            let (sx, sy) = (x0 - ux * back, y0 - uy * back);
            let (ex, ey) = (x1 + ux * fwd, y1 + uy * fwd);
            let mut pb = tiny_skia::PathBuilder::new();
            pb.move_to(sx + px, sy + py);
            pb.line_to(ex + px, ey + py);
            pb.line_to(ex - px, ey - py);
            pb.line_to(sx - px, sy - py);
            pb.close();
            pb.finish()
        }
        GradientKind::Radial { cx, cy, r, .. } => {
            if extend[1] {
                return None;
            }
            tiny_skia::PathBuilder::from_circle(cx, cy, r)
        }
    }
}

/// A number with at most three decimals and no trailing zeros — a device
/// pixel at 300 DPI is 0.24 pt, so three decimals of a pixel is far below
/// anything a consumer can render.
fn num(v: f32) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

fn rgb_css(rgba: &[u8; 4]) -> String {
    format!("rgb({},{},{})", rgba[0], rgba[1], rgba[2])
}

/// ` name="0.5"` for a non-opaque alpha, nothing for opaque.
fn opacity_attr(name: &str, alpha: u8) -> String {
    if alpha == 255 {
        String::new()
    } else {
        format!(r#" {name}="{}""#, num(f32::from(alpha) / 255.0))
    }
}

fn rule_name(rule: FillRule) -> &'static str {
    match rule {
        FillRule::Winding => "nonzero",
        FillRule::EvenOdd => "evenodd",
    }
}

/// `matrix(a b c d e f)` from tiny-skia's row layout: `x' = sx·x + kx·y +
/// tx`, `y' = ky·x + sy·y + ty`, so `a=sx b=ky c=kx d=sy e=tx f=ty`.
fn matrix(t: Transform) -> String {
    format!(
        "matrix({} {} {} {} {} {})",
        num(t.sx),
        num(t.ky),
        num(t.kx),
        num(t.sy),
        num(t.tx),
        num(t.ty)
    )
}

/// The `d` attribute. Absolute commands only; SVG's path grammar is a
/// superset of tiny-skia's five verbs.
fn path_data(path: &Path) -> String {
    let mut d = String::with_capacity(path.len() * 16);
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                let _ = write!(d, "M{} {}", num(p.x), num(p.y));
            }
            PathSegment::LineTo(p) => {
                let _ = write!(d, "L{} {}", num(p.x), num(p.y));
            }
            PathSegment::QuadTo(c, p) => {
                let _ = write!(d, "Q{} {} {} {}", num(c.x), num(c.y), num(p.x), num(p.y));
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let _ = write!(
                    d,
                    "C{} {} {} {} {} {}",
                    num(c1.x),
                    num(c1.y),
                    num(c2.x),
                    num(c2.y),
                    num(p.x),
                    num(p.y)
                );
            }
            PathSegment::Close => d.push('Z'),
        }
    }
    d
}

/// The CSS `mix-blend-mode` keyword for a separable mode, or `None` for
/// normal. Every mode of ISO 32000-1 Tables 136/137 has a keyword.
fn blend_css(blend: BlendMode) -> Option<&'static str> {
    Some(match blend {
        BlendMode::SourceOver => return None,
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
        BlendMode::Darken => "darken",
        BlendMode::Lighten => "lighten",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::HardLight => "hard-light",
        BlendMode::SoftLight => "soft-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Color => "color",
        BlendMode::Luminosity => "luminosity",
        // Porter-Duff operators the interpreter never records for a paint.
        _ => return None,
    })
}

/// A coverage mask as an 8-bit greyscale PNG — the luminance an SVG
/// `<mask>` reads, 1:1 with the coverage bytes under `color-interpolation:
/// sRGB`.
fn mask_png(mask: &Mask) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, mask.width(), mask.height());
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().ok()?;
        w.write_image_data(mask.data()).ok()?;
    }
    Some(out)
}

/// Standard base64 (RFC 4648 §4), padded. Twenty lines beats a dependency
/// for one data-URI writer.
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).map_or(0, |&b| u32::from(b));
        let b2 = chunk.get(2).map_or(0, |&b| u32::from(b));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

// Keep `Arc` and `Pixmap` referenced for the image arm's types.
#[allow(dead_code)]
fn _types(_: Arc<Pixmap>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn numbers_are_short_and_never_negative_zero() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(0.5), "0.5");
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(12.3456), "12.346");
        assert_eq!(num(f32::NAN), "0");
    }

    #[test]
    fn matrix_uses_svg_column_order() {
        let t = Transform::from_row(2.0, 3.0, 4.0, 5.0, 6.0, 7.0);
        // sx=2 ky=3 kx=4 sy=5 tx=6 ty=7 → a=2 b=3 c=4 d=5 e=6 f=7
        assert_eq!(matrix(t), "matrix(2 3 4 5 6 7)");
    }

    #[test]
    fn every_pdf_blend_mode_has_a_css_keyword() {
        for m in [
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::Darken,
            BlendMode::Lighten,
            BlendMode::ColorDodge,
            BlendMode::ColorBurn,
            BlendMode::HardLight,
            BlendMode::SoftLight,
            BlendMode::Difference,
            BlendMode::Exclusion,
            BlendMode::Hue,
            BlendMode::Saturation,
            BlendMode::Color,
            BlendMode::Luminosity,
        ] {
            assert!(blend_css(m).is_some(), "{m:?}");
        }
        assert_eq!(blend_css(BlendMode::SourceOver), None);
    }
}

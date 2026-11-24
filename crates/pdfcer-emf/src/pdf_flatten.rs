//! [`RecordingFlattener`], the shipped
//! [`pdfcer_core::transparency::TransparencyFlattener`]: a page's
//! transparency (ISO 32000-2 §11.3 basic compositing, §11.4 groups, §11.6
//! the transparency parameters) composited into opaque content.
//!
//! The page is recorded for export at the run's resolution and its
//! composited paints are resolved by [`crate::vector`] (through
//! `pdfcer-flatten`) into opaque regions over white paper. The flattened
//! area is the union of everything that composited; inside it, every op is
//! redrawn opaquely in page order, and what is still not a flat colour (an
//! image, a shading, an unresolved group, a cut the resolver left) is
//! cropped from the page rendered with its transparency, and painted on
//! top. Annotations are left out: the run leaves them as they are.

mod content;

use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::page_tree::{self, Page, Rect};
use pdfcer_core::transparency::{
    FlattenFailure, FlattenMode, FlattenOptions, FlattenedPage, RasterPatch, RasterReason,
    RasterisedRegion, TransparencyFlattener,
};
use pdfcer_core::view::DocumentView;
use pdfcer_render::export::Rgb;
use pdfcer_render::font::RenderOptions;
use pdfcer_render::writer_seam::{Op, UNKNOWN, op_box, record_page_for_export};
use tiny_skia::{Pixmap, Point, Transform};

use crate::patch::{PxRect, composites, merge, px_rect};

/// Flattens a page's transparency by recording it, resolving its composited
/// paints into opaque regions and rasterising what cannot be resolved.
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::edit::EditSession;
/// use pdfcer_core::transparency::FlattenOptions;
/// use pdfcer_emf::RecordingFlattener;
///
/// let doc = Document::from_bytes(std::fs::read("../../fixtures/synthetic/hello.pdf")?)?;
/// let mut session = EditSession::new(doc);
/// let report =
///     session.flatten_transparency(&[0], &FlattenOptions::new(), &RecordingFlattener::new())?;
/// assert!(!report.changed, "hello.pdf is opaque");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct RecordingFlattener {
    /// How the page is recorded and rendered. Annotations are switched
    /// off whatever this says.
    pub render: RenderOptions,
}

impl RecordingFlattener {
    /// A flattener with the default render options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A flattener recording with `render`.
    #[must_use]
    pub fn with_render(render: RenderOptions) -> Self {
        Self { render }
    }
}

impl TransparencyFlattener for RecordingFlattener {
    fn flatten_page(
        &self,
        view: &DocumentView<'_>,
        page: usize,
        options: &FlattenOptions,
    ) -> Result<FlattenedPage, FlattenFailure> {
        let pages = page_tree::pages_in(view).map_err(failure)?;
        let p = pages
            .get(page)
            .ok_or_else(|| FlattenFailure::new(format!("there is no page {}", page + 1)))?;
        // ppi is range-checked by the session (36..=2400).
        #[allow(clippy::cast_possible_truncation)]
        let scale = (options.raster_ppi / 72.0) as f32;
        let render = self.render.clone().with_annotations(false);
        let mut rec =
            record_page_for_export(view, p, scale, &render, options.keep_text).map_err(failure)?;
        let (w, h) = rec.page_size;
        let mode = if options.keep_text {
            FlattenMode::Overlay
        } else {
            FlattenMode::Rebuilt
        };
        let spotted = spot_boxes(&rec.ops, w, h);
        let mut area: Vec<PxRect> = rec
            .ops
            .iter()
            .filter(|op| composites(op))
            .filter_map(|op| px_rect(op_box(op).unwrap_or(UNKNOWN), w, h))
            .collect();
        let (_, left) = crate::vector::vectorise(&mut rec, &render, Some(Rgb::WHITE), true, false);
        area.extend(left.iter().filter_map(|b| px_rect(*b, w, h)));
        let area = match mode {
            FlattenMode::Rebuilt => vec![(0, 0, w, h)],
            _ => merge(area),
        };
        let (_, _, base) = pdfcer_render::page_device_geometry(p, scale);
        let inverse = base
            .invert()
            .ok_or_else(|| FlattenFailure::new("the page's geometry is degenerate"))?;
        let mut emit = content::Emit::new(&rec, &area, options.keep_strokes);
        emit.open(inverse, mode);
        for op in &rec.ops {
            emit.op(op);
        }
        let cuts = left.iter().filter_map(|b| px_rect(*b, w, h));
        emit.rasters
            .extend(cuts.map(|r| (r, RasterReason::NotModelled)));
        let mut out = FlattenedPage::new(mode);
        out.vector_regions = emit.paints;
        out.text_runs_outlined = emit.text_runs;
        out.strokes_outlined = emit.strokes_outlined;
        if emit.dashed > 0 {
            out.notes.push(format!(
                "{} dashed stroke(s) drawn as filled outlines",
                emit.dashed
            ));
        }
        out.spot_inks = spot_names(&spotted, &area);
        let rasters = clip_rasters(&emit.rasters, &area);
        let mut body = std::mem::take(&mut emit.out);
        if !rasters.is_empty() {
            let truth = pdfcer_render::render_page_with_view(view, p, scale, &render)
                .map_err(failure)?
                .pixmap;
            let names = free_names(view, p, rasters.len());
            for ((r, reason), name) in rasters.into_iter().zip(names) {
                let patch = crop(&truth, r, name.clone())
                    .ok_or_else(|| FlattenFailure::new("the page render has the wrong size"))?;
                content::place(&mut body, r, &name);
                out.images.push(patch);
                out.rasterised
                    .push(RasterisedRegion::new(user_rect(r, inverse), reason));
            }
        }
        body.extend_from_slice(b"Q\n");
        out.content = body;
        Ok(out)
    }
}

fn failure(e: impl std::fmt::Display) -> FlattenFailure {
    FlattenFailure::new(e.to_string())
}

/// Each spot-ink op's box and its ink names, read before the resolver
/// replaces the ops it resolves.
fn spot_boxes(ops: &[Op], w: u32, h: u32) -> Vec<(PxRect, Vec<String>)> {
    let mut out = Vec::new();
    for op in ops {
        let names = op_spots(op);
        if names.is_empty() {
            continue;
        }
        if let Some(r) = px_rect(op_box(op).unwrap_or(UNKNOWN), w, h) {
            out.push((r, names));
        }
    }
    out
}

fn op_spots(op: &Op) -> Vec<String> {
    match op {
        Op::Fill { brush, .. } | Op::Stroke { brush, .. } => brush
            .spots
            .iter()
            .map(|s| String::from_utf8_lossy(&s.colorant).into_owned())
            .collect(),
        Op::Layer { ops, .. } | Op::Text { ops, .. } => ops.iter().flat_map(op_spots).collect(),
        Op::Shading(_) => Vec::new(),
    }
}

/// The inks of the spot paints that lie in the flattened area, sorted.
fn spot_names(spotted: &[(PxRect, Vec<String>)], area: &[PxRect]) -> Vec<String> {
    let mut names: Vec<String> = spotted
        .iter()
        .filter(|(r, _)| area.iter().any(|a| content::overlaps(*a, *r)))
        .flat_map(|(_, n)| n.iter().cloned())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The raster boxes cut to the flattened area and merged, each keeping the
/// most specific reason among those merged into it.
fn clip_rasters(
    rasters: &[(PxRect, RasterReason)],
    area: &[PxRect],
) -> Vec<(PxRect, RasterReason)> {
    let cut: Vec<(PxRect, RasterReason)> = rasters
        .iter()
        .flat_map(|(r, why)| {
            area.iter()
                .filter_map(move |a| content::intersect(*a, *r).map(|i| (i, *why)))
        })
        .collect();
    let merged = merge(cut.iter().map(|(r, _)| *r).collect());
    merged
        .into_iter()
        .map(|m| {
            let why = cut
                .iter()
                .filter(|(r, _)| content::overlaps(m, *r))
                .map(|(_, w)| *w)
                .min_by_key(|w| rank(*w))
                .unwrap_or(RasterReason::NotModelled);
            (m, why)
        })
        .collect()
}

const fn rank(why: RasterReason) -> u8 {
    match why {
        RasterReason::Image => 0,
        RasterReason::Shading => 1,
        RasterReason::Group => 2,
        _ => 3,
    }
}

/// `count` XObject names the page's resources do not already use.
fn free_names(view: &DocumentView<'_>, page: &Page, count: usize) -> Vec<String> {
    let taken = page
        .resources
        .get(b"XObject")
        .and_then(|x| view.resolve(x).as_dict().cloned())
        .unwrap_or_default();
    (1..)
        .map(|n| format!("TrRaster{n}"))
        .filter(|n| taken.get(n.as_bytes()).is_none())
        .take(count)
        .collect()
}

/// The pixels of `r`, white-backed, as RGB.
fn crop(truth: &Pixmap, r: PxRect, name: String) -> Option<RasterPatch> {
    let (x0, y0, x1, y1) = r;
    if x1 > truth.width() || y1 > truth.height() {
        return None;
    }
    let stride = truth.width() as usize;
    let data = truth.data();
    let mut rgb = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 3) as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y as usize * stride + x as usize) * 4;
            let &[r, g, b, a] = data.get(i..i + 4)? else {
                return None;
            };
            // Premultiplied over white: c + (1 - a).
            let paper = 255 - a;
            rgb.extend([r, g, b].map(|c| c.saturating_add(paper)));
        }
    }
    RasterPatch::new(name, x1 - x0, y1 - y0, rgb)
}

/// A device-pixel box in default user space.
fn user_rect(r: PxRect, inverse: Transform) -> Rect {
    #[allow(clippy::cast_precision_loss)]
    let (x0, y0, x1, y1) = (r.0 as f32, r.1 as f32, r.2 as f32, r.3 as f32);
    let mut pts = [
        Point::from_xy(x0, y0),
        Point::from_xy(x1, y0),
        Point::from_xy(x0, y1),
        Point::from_xy(x1, y1),
    ];
    inverse.map_points(&mut pts);
    let xs = pts.iter().map(|p| f64::from(p.x));
    let ys = pts.iter().map(|p| f64::from(p.y));
    Rect {
        llx: xs.clone().fold(f64::INFINITY, f64::min),
        lly: ys.clone().fold(f64::INFINITY, f64::min),
        urx: xs.fold(f64::NEG_INFINITY, f64::max),
        ury: ys.fold(f64::NEG_INFINITY, f64::max),
    }
}

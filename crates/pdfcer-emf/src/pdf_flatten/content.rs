//! Recorded ops as PDF content-stream operators (ISO 32000-2 §8.5 path
//! construction and painting, §8.4.4 graphics-state operators), in
//! page-device space under one `cm` back to default user space.

use std::io::Write as _;
use std::sync::Arc;

use pdfcer_core::transparency::{FlattenMode, RasterReason};
use pdfcer_render::writer_seam::{
    Brush, BrushSpec, ClipDef, ClipId, ExportRecording, Op, UNKNOWN, op_box,
};
use tiny_skia::{FillRule, LineCap, LineJoin, Path, PathSegment, Pixmap, Point, Stroke, Transform};

use crate::patch::{PxRect, px_rect};

/// The writer of one flattened page.
pub(super) struct Emit<'a> {
    clips: &'a [ClipDef],
    shadings: &'a [(Arc<Pixmap>, pdfcer_render::writer_seam::ShadingSource)],
    area: &'a [PxRect],
    size: (u32, u32),
    keep_strokes: bool,
    /// The content written so far.
    pub out: Vec<u8>,
    /// Opaque paints written.
    pub paints: usize,
    /// Text runs redrawn as glyph outlines.
    pub text_runs: usize,
    /// Strokes written as filled outlines.
    pub strokes_outlined: usize,
    /// Of those, dashed strokes kept as strokes would have been.
    pub dashed: usize,
    /// Boxes to rasterise, and why.
    pub rasters: Vec<(PxRect, RasterReason)>,
}

impl<'a> Emit<'a> {
    /// A writer for `rec`'s ops that touch `area`.
    pub(super) fn new(rec: &'a ExportRecording, area: &'a [PxRect], keep_strokes: bool) -> Self {
        Self {
            clips: &rec.clips,
            shadings: &rec.shadings,
            area,
            size: rec.page_size,
            keep_strokes,
            out: Vec::new(),
            paints: 0,
            text_runs: 0,
            strokes_outlined: 0,
            dashed: 0,
            rasters: Vec::new(),
        }
    }

    /// `q`, the device-to-user `cm`, and in overlay mode the clip to the
    /// area painted white. The caller closes with `Q`.
    pub(super) fn open(&mut self, inverse: Transform, mode: FlattenMode) {
        self.out.extend_from_slice(b"q\n");
        matrix(&mut self.out, inverse);
        if mode == FlattenMode::Overlay {
            for r in self.area {
                rect(&mut self.out, *r);
            }
            self.out.extend_from_slice(b"W n\n1 g\n");
            for r in self.area {
                rect(&mut self.out, *r);
            }
            self.out.extend_from_slice(b"f\n");
        }
    }

    /// Write `op` when it paints within the area: opaque solid paint as
    /// vectors, anything else as a box to rasterise.
    pub(super) fn op(&mut self, op: &Op) {
        let (w, h) = self.size;
        let Some(r) = px_rect(op_box(op).unwrap_or(UNKNOWN), w, h) else {
            return;
        };
        if !self.area.iter().any(|a| overlaps(*a, r)) {
            return;
        }
        match op {
            Op::Fill {
                path,
                brush,
                rule,
                ctm,
                clip,
                ..
            } if opaque(brush).is_some() => {
                if let Some(device) = path.as_ref().clone().transform(*ctm) {
                    self.fill(&device, *rule, brush, *clip);
                }
            }
            Op::Stroke {
                path,
                brush,
                stroke,
                ctm,
                clip,
                ..
            } if opaque(brush).is_some() => self.stroke(path, stroke, *ctm, brush, *clip),
            Op::Text { ops, .. } => {
                self.text_runs += 1;
                for glyph in ops {
                    self.op(glyph);
                }
            }
            _ => self.rasters.push((r, self.reason(op))),
        }
    }

    fn reason(&self, op: &Op) -> RasterReason {
        match op {
            Op::Fill { brush, .. } | Op::Stroke { brush, .. } => match &brush.brush {
                Brush::Image { texels, .. }
                    if !self.shadings.iter().any(|(t, _)| Arc::ptr_eq(t, texels)) =>
                {
                    RasterReason::Image
                }
                Brush::Image { .. } | Brush::Gradient(_) => RasterReason::Shading,
                Brush::Solid { .. } => RasterReason::NotModelled,
            },
            Op::Shading(_) => RasterReason::Shading,
            Op::Layer { .. } => RasterReason::Group,
            Op::Text { .. } => RasterReason::NotModelled,
        }
    }

    /// `device` filled with `brush`'s colour under `clip`.
    fn fill(&mut self, device: &Path, rule: FillRule, brush: &BrushSpec, clip: Option<ClipId>) {
        let Some(rgb) = opaque(brush) else {
            return;
        };
        if !self.begin(clip) {
            return;
        }
        colour(&mut self.out, rgb, b"rg");
        path(&mut self.out, device);
        self.out.extend_from_slice(match rule {
            FillRule::EvenOdd => b"f*\nQ\n",
            FillRule::Winding => b"f\nQ\n",
        });
        self.paints += 1;
    }

    /// A stroke kept as a stroke in its own space, or written as the
    /// filled outline of the stroke.
    fn stroke(
        &mut self,
        p: &Path,
        stroke: &Stroke,
        ctm: Transform,
        brush: &BrushSpec,
        clip: Option<ClipId>,
    ) {
        let Some(rgb) = opaque(brush) else {
            return;
        };
        if self.keep_strokes && stroke.dash.is_none() {
            if !self.begin(clip) {
                return;
            }
            matrix(&mut self.out, ctm);
            let _ = writeln!(
                self.out,
                "{} w {} J {} j {} M",
                num(stroke.width),
                cap(stroke.line_cap),
                join(stroke.line_join),
                num(stroke.miter_limit.max(1.0))
            );
            colour(&mut self.out, rgb, b"RG");
            path(&mut self.out, p);
            self.out.extend_from_slice(b"S\nQ\n");
            self.paints += 1;
            return;
        }
        let scale = (ctm.sx * ctm.sy - ctm.kx * ctm.ky).abs().sqrt().max(1e-3);
        let Some(outline) = p.stroke(stroke, scale).and_then(|o| o.transform(ctm)) else {
            return;
        };
        if self.keep_strokes {
            self.dashed += 1;
        }
        self.strokes_outlined += 1;
        self.fill(&outline, FillRule::Winding, brush, clip);
    }

    /// `q` and `clip`'s chain as clipping paths; false (nothing written)
    /// when the chain admits nothing.
    fn begin(&mut self, clip: Option<ClipId>) -> bool {
        let mut chain = Vec::new();
        let mut at = clip;
        while let Some(id) = at {
            let Some(def) = self.clips.get(id.index()) else {
                break;
            };
            if chain.len() > self.clips.len() {
                break;
            }
            chain.push(def);
            at = def.parent;
        }
        let mut paths = Vec::with_capacity(chain.len());
        for def in chain {
            let Some(device) = def
                .path
                .as_ref()
                .and_then(|p| p.as_ref().clone().transform(def.ctm))
            else {
                return false;
            };
            paths.push((device, def.rule));
        }
        self.out.extend_from_slice(b"q\n");
        for (device, rule) in paths {
            path(&mut self.out, &device);
            self.out.extend_from_slice(match rule {
                FillRule::EvenOdd => b"W* n\n",
                FillRule::Winding => b"W n\n",
            });
        }
        true
    }
}

/// The colour of a solid brush that paints opaquely with `Normal`.
fn opaque(brush: &BrushSpec) -> Option<[u8; 3]> {
    match brush.brush {
        Brush::Solid { rgba }
            if rgba[3] == 255 && brush.blend == tiny_skia::BlendMode::SourceOver =>
        {
            Some([rgba[0], rgba[1], rgba[2]])
        }
        _ => None,
    }
}

/// Paint image `name` over device box `r`: the unit square's top row at
/// the box's top edge.
pub(super) fn place(out: &mut Vec<u8>, r: PxRect, name: &str) {
    let (x0, y0, x1, y1) = r;
    let _ = writeln!(
        out,
        "q {} 0 0 -{} {x0} {y1} cm /{name} Do Q",
        x1 - x0,
        y1 - y0
    );
    debug_assert!(y0 < y1);
}

/// Whether two boxes share any pixel.
pub(super) fn overlaps(a: PxRect, b: PxRect) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

/// The pixels two boxes share.
pub(super) fn intersect(a: PxRect, b: PxRect) -> Option<PxRect> {
    overlaps(a, b).then(|| (a.0.max(b.0), a.1.max(b.1), a.2.min(b.2), a.3.min(b.3)))
}

fn rect(out: &mut Vec<u8>, r: PxRect) {
    let _ = writeln!(out, "{} {} {} {} re", r.0, r.1, r.2 - r.0, r.3 - r.1);
}

fn matrix(out: &mut Vec<u8>, t: Transform) {
    let _ = writeln!(
        out,
        "{} {} {} {} {} {} cm",
        fine(t.sx),
        fine(t.ky),
        fine(t.kx),
        fine(t.sy),
        fine(t.tx),
        fine(t.ty)
    );
}

fn colour(out: &mut Vec<u8>, rgb: [u8; 3], op: &[u8]) {
    let c = |v: u8| num(f32::from(v) / 255.0);
    let _ = write!(out, "{} {} {} ", c(rgb[0]), c(rgb[1]), c(rgb[2]));
    out.extend_from_slice(op);
    out.push(b'\n');
}

/// §8.5.2 path construction; quadratics raised to cubics.
fn path(out: &mut Vec<u8>, p: &Path) {
    let mut at = Point::zero();
    let mut start = Point::zero();
    for seg in p.segments() {
        let _ = match seg {
            PathSegment::MoveTo(q) => {
                (at, start) = (q, q);
                writeln!(out, "{} {} m", num(q.x), num(q.y))
            }
            PathSegment::LineTo(q) => {
                at = q;
                writeln!(out, "{} {} l", num(q.x), num(q.y))
            }
            PathSegment::QuadTo(c, q) => {
                let c1 = Point::from_xy(
                    at.x + (c.x - at.x) * 2.0 / 3.0,
                    at.y + (c.y - at.y) * 2.0 / 3.0,
                );
                let c2 =
                    Point::from_xy(q.x + (c.x - q.x) * 2.0 / 3.0, q.y + (c.y - q.y) * 2.0 / 3.0);
                at = q;
                cubic(out, c1, c2, q)
            }
            PathSegment::CubicTo(c1, c2, q) => {
                at = q;
                cubic(out, c1, c2, q)
            }
            PathSegment::Close => {
                at = start;
                writeln!(out, "h")
            }
        };
    }
}

fn cubic(out: &mut Vec<u8>, c1: Point, c2: Point, q: Point) -> std::io::Result<()> {
    writeln!(
        out,
        "{} {} {} {} {} {} c",
        num(c1.x),
        num(c1.y),
        num(c2.x),
        num(c2.y),
        num(q.x),
        num(q.y)
    )
}

const fn cap(c: LineCap) -> u8 {
    match c {
        LineCap::Butt => 0,
        LineCap::Round => 1,
        LineCap::Square => 2,
    }
}

const fn join(j: LineJoin) -> u8 {
    match j {
        LineJoin::Miter | LineJoin::MiterClip => 0,
        LineJoin::Round => 1,
        LineJoin::Bevel => 2,
    }
}

/// A real for a content stream: at most four decimals, no exponent.
fn num(v: f32) -> String {
    real(v, 4)
}

/// A matrix entry: six decimals, since it scales every coordinate.
fn fine(v: f32) -> String {
    real(v, 6)
}

fn real(v: f32, places: usize) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{v:.places$}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" || s == "-0" {
        "0".into()
    } else {
        s.to_owned()
    }
}

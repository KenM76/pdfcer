//! Flat transparency written as opaque vector regions
//! ([`super::EmfOptions::vector_transparency`]).
//!
//! An op the metafile cannot draw exactly — a constant alpha, a blend, a
//! transparency group, an overprint over other ink, any compositing on a
//! CMYK page — is rebuilt with every earlier op whose box meets it as a
//! `pdfcer_flatten` scene, resolved into atomic regions, and replaced by
//! one opaque fill per region it covers, each composited over the paper.
//! A soft mask whose group is flat colour is split into constant-value
//! pieces (`mask`). An op that cannot be modelled (an image or gradient in
//! the stack or in a mask, a spot ink, a blend the recording did not keep)
//! or that passes the flattener's ceiling is returned as a box for the
//! pixel patch.

use std::sync::Arc;

use pdfcer_core::color::{CmykIntent, cmyk_to_srgb_with, srgb_to_cmyk_fast};
use pdfcer_flatten::{
    Blend, Element, FlattenError, Group, Inks, Limits, Overprint, Paint, Region, Space, flatten,
};
use pdfcer_render::export::Rgb;
use pdfcer_render::font::RenderOptions;
use pdfcer_render::writer_seam::{Brush, BrushSpec, LayerPaint};
use pdfcer_render::writer_seam::{DeviceBounds, ExportRecording, Op, SiteCause, UNKNOWN, op_box};
use tiny_skia::{BlendMode, FillRule, Transform};

use crate::patch::within;
use crate::vector_census::{Cause, EmfNotFlat};
use crate::vector_geom::{self, Clips};

mod mask;

/// What [`super::EmfOptions::with_vector_transparency`] wrote as opaque
/// vector regions, and what it left to the pixel patch. All zero with the
/// switch off.
///
/// An op counts once in [`Self::ops`] and under each cause it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct EmfVectorised {
    /// Top-level ops replaced by opaque regions.
    pub ops: usize,
    /// Opaque fills written in their place.
    pub regions: usize,
    /// Of [`Self::ops`]: paints with a constant alpha below 1.
    pub alpha: usize,
    /// Of [`Self::ops`]: transparency groups, isolated or not, knockout or
    /// not.
    pub groups: usize,
    /// Of [`Self::ops`]: overprinted paints over other ink.
    pub overprint: usize,
    /// Of [`Self::ops`]: paints with a blend mode other than `Normal`.
    pub blends: usize,
    /// Patched because something in the op or beneath it is not a flat
    /// colour: an image, a gradient, a soft mask, a spot ink, a paint on a
    /// CMYK page with no CMYK colour, or kept text.
    pub patched_not_flat: usize,
    /// Patched because the scene passed the flattener's ceiling.
    pub patched_too_complex: usize,
    /// Patched because the recording logged a blend or blending space there
    /// that it did not keep, so the scene cannot be rebuilt.
    pub patched_unrecorded: usize,
    /// [`Self::patched_not_flat`] broken down by cause.
    pub not_flat: EmfNotFlat,
}

impl EmfVectorised {
    /// Ops left to the pixel patch, every reason summed.
    #[must_use]
    pub fn patched(&self) -> usize {
        self.patched_not_flat + self.patched_too_complex + self.patched_unrecorded
    }
}

/// Why an op was left to the patch.
#[derive(Clone, Copy)]
enum Why {
    /// The cause, and whether it was in an op beneath.
    NotFlat(Cause, bool),
    TooComplex,
    Unrecorded,
}

/// What the writer cannot draw exactly about one top-level op.
#[derive(Default)]
struct Needs {
    alpha: bool,
    group: bool,
    overprint: bool,
    blend: bool,
}

impl Needs {
    fn any(&self) -> bool {
        self.alpha || self.group || self.overprint || self.blend
    }
}

/// The page's ink pairs, as `pdfcer-render`'s overprint picks them: model
/// 0 is the exactly invertible naive pair, model 1 the calibrated one, used
/// for a paint that states its own process tints.
struct PageInks(CmykIntent);

impl Inks for PageInks {
    fn to_cmyk(&self, model: usize, [r, g, b]: [f32; 3]) -> [f32; 4] {
        if model == 1 {
            srgb_to_cmyk_fast(self.0, r, g, b)
        } else {
            pdfcer_render::overprint::rgb_to_cmyk(r, g, b)
        }
    }

    fn to_rgb(&self, model: usize, cmyk: [f32; 4]) -> [f32; 3] {
        if model == 1 {
            let [c, m, y, k] = cmyk;
            cmyk_to_srgb_with(self.0, c, m, y, k)
        } else {
            let (r, g, b) = pdfcer_render::overprint::cmyk_to_rgb(cmyk);
            [r, g, b]
        }
    }
}

fn blend_of(mode: BlendMode) -> Option<Blend> {
    Some(match mode {
        BlendMode::SourceOver => Blend::Normal,
        BlendMode::Multiply => Blend::Multiply,
        BlendMode::Screen => Blend::Screen,
        BlendMode::Overlay => Blend::Overlay,
        BlendMode::Darken => Blend::Darken,
        BlendMode::Lighten => Blend::Lighten,
        BlendMode::ColorDodge => Blend::ColorDodge,
        BlendMode::ColorBurn => Blend::ColorBurn,
        BlendMode::HardLight => Blend::HardLight,
        BlendMode::SoftLight => Blend::SoftLight,
        BlendMode::Difference => Blend::Difference,
        BlendMode::Exclusion => Blend::Exclusion,
        BlendMode::Hue => Blend::Hue,
        BlendMode::Saturation => Blend::Saturation,
        BlendMode::Color => Blend::Color,
        BlendMode::Luminosity => Blend::Luminosity,
        _ => return None,
    })
}

fn layer_blend(paint: &LayerPaint) -> Option<Blend> {
    use pdfcer_render::blend_nonsep::NonSeparableBlend as N;
    match paint.nonseparable {
        Some(N::Hue) => Some(Blend::Hue),
        Some(N::Saturation) => Some(Blend::Saturation),
        Some(N::Color) => Some(Blend::Color),
        Some(N::Luminosity) => Some(Blend::Luminosity),
        None => blend_of(paint.blend),
    }
}

/// Recorded ops as flatten elements, each computed once.
struct Model<'a> {
    space: Space,
    clips: Clips<'a>,
    rec: &'a ExportRecording,
    memo: Vec<Option<Result<Vec<Element>, Cause>>>,
    /// Per entry of `rec.masks`, its pieces once built.
    masks: Vec<Option<Result<mask::Pieces, Cause>>>,
}

impl Model<'_> {
    /// Top-level op `i` as elements, or the first content that is not
    /// flat.
    fn top(&mut self, i: usize, op: &Op) -> Result<Vec<Element>, Cause> {
        let slot = self.memo.get(i).ok_or(Cause::Geometry)?;
        if slot.is_none() {
            let mut out = Vec::new();
            let got = self.element(op, &mut out).map(|()| out);
            *self.memo.get_mut(i).ok_or(Cause::Geometry)? = Some(got);
        }
        self.memo
            .get(i)
            .and_then(Option::clone)
            .ok_or(Cause::Geometry)?
    }

    fn element(&mut self, op: &Op, out: &mut Vec<Element>) -> Result<(), Cause> {
        match op {
            Op::Fill {
                path,
                brush,
                rule,
                ctm,
                clip,
                ..
            } => {
                self.paintable(brush)?;
                let shape = vector_geom::fill(path, *rule, *ctm).ok_or(Cause::Geometry)?;
                let shape = self.clips.apply(shape, *clip).ok_or(Cause::Geometry)?;
                out.push(Element::Paint(self.paint(brush, shape)?));
            }
            Op::Stroke {
                path,
                brush,
                stroke,
                ctm,
                clip,
                ..
            } => {
                self.paintable(brush)?;
                let shape = vector_geom::stroke(path, stroke, *ctm).ok_or(Cause::Geometry)?;
                let shape = self.clips.apply(shape, *clip).ok_or(Cause::Geometry)?;
                out.push(Element::Paint(self.paint(brush, shape)?));
            }
            Op::Layer {
                paint,
                ops,
                mask,
                isolated,
                knockout,
            } => {
                let mut inner = Vec::with_capacity(ops.len());
                for o in ops {
                    self.element(o, &mut inner)?;
                }
                let group = Group {
                    elements: inner,
                    isolated: *isolated,
                    knockout: *knockout,
                    alpha: paint.opacity,
                    blend: layer_blend(paint).ok_or(Cause::Blend)?,
                };
                match mask {
                    Some(m) => out.extend(self.masked(m, group)?),
                    None => out.push(Element::Group(group)),
                }
            }
            Op::Text { ops, .. } => {
                for o in ops {
                    self.element(o, out)?;
                }
            }
        }
        Ok(())
    }

    /// Why `brush` is not a flat colour, before its geometry is built.
    fn paintable(&self, brush: &BrushSpec) -> Result<(), Cause> {
        match &brush.brush {
            Brush::Solid { .. } => {}
            Brush::Gradient(_) => return Err(Cause::Shading),
            Brush::Image { texels, .. } => {
                let shading = self
                    .rec
                    .shadings
                    .iter()
                    .any(|(t, _)| Arc::ptr_eq(t, texels));
                return Err(if shading {
                    Cause::Shading
                } else {
                    Cause::Image
                });
            }
        }
        if !brush.spots.is_empty() || brush.all_colorants.is_some() {
            return Err(Cause::Spot);
        }
        if self.space == Space::Cmyk && brush.cmyk.is_none() {
            return Err(Cause::Cmyk);
        }
        blend_of(brush.blend).map(|_| ()).ok_or(Cause::Blend)
    }

    fn paint(&self, brush: &BrushSpec, shape: pdfcer_flatten::Shape) -> Result<Paint, Cause> {
        self.paintable(brush)?;
        let Brush::Solid { rgba } = brush.brush else {
            return Err(Cause::Image);
        };
        let colour = match self.space {
            Space::Cmyk => brush.cmyk.ok_or(Cause::Cmyk)?,
            _ => {
                let f = |v: u8| f32::from(v) / 255.0;
                [f(rgba[0]), f(rgba[1]), f(rgba[2]), 0.0]
            }
        };
        Ok(Paint {
            shape,
            colour,
            alpha: f32::from(rgba[3]) / 255.0,
            blend: blend_of(brush.blend).ok_or(Cause::Blend)?,
            overprint: brush.overprint_process.map(|tints| Overprint {
                tints,
                model: usize::from(brush.cmyk.is_some()),
            }),
        })
    }
}

fn overlaps(a: DeviceBounds, b: DeviceBounds) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

fn overprints(op: &Op) -> bool {
    match op {
        Op::Fill { brush, .. } | Op::Stroke { brush, .. } => brush.overprint_process.is_some(),
        Op::Layer { ops, .. } | Op::Text { ops, .. } => ops.iter().any(overprints),
    }
}

/// What about `op` the writer would draw inexactly. Text is left as it is.
fn needs(op: &Op, bx: DeviceBounds, rec: &ExportRecording) -> Needs {
    match op {
        Op::Fill { brush, .. } | Op::Stroke { brush, .. } => {
            let Brush::Solid { rgba } = brush.brush else {
                return Needs::default();
            };
            Needs {
                alpha: rgba[3] < 255,
                blend: brush.blend != BlendMode::SourceOver,
                overprint: brush.overprint_process.is_some()
                    && rec
                        .sites
                        .iter()
                        .any(|s| s.cause == SiteCause::Overprint && within(s.bounds, bx)),
                group: false,
            }
        }
        Op::Layer { paint, .. } => Needs {
            group: true,
            blend: paint.blend != BlendMode::SourceOver || paint.nonseparable.is_some(),
            overprint: overprints(op),
            alpha: false,
        },
        Op::Text { .. } => Needs::default(),
    }
}

fn count_paints(elements: &[Element]) -> usize {
    elements
        .iter()
        .map(|e| match e {
            Element::Paint(_) => 1,
            Element::Group(g) => count_paints(&g.elements),
        })
        .sum()
}

/// One region as an opaque fill: converted to sRGB, composited over
/// `paper`.
fn region_op(r: &Region, space: Space, intent: CmykIntent, paper: [f32; 3]) -> Option<Op> {
    let [c0, c1, c2, c3] = r.colour;
    let rgb = match space {
        Space::Cmyk => cmyk_to_srgb_with(intent, c0, c1, c2, c3),
        _ => [c0, c1, c2],
    };
    let a = r.alpha.clamp(0.0, 1.0);
    // Clamped to 0..=255 before the cast.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let q = |c: f32, p: f32| (a.mul_add(c, (1.0 - a) * p).clamp(0.0, 1.0) * 255.0).round() as u8;
    let [p0, p1, p2] = paper;
    let rgba = [q(rgb[0], p0), q(rgb[1], p1), q(rgb[2], p2), 255];
    let (path, bounds) = vector_geom::path_of(&r.contours)?;
    Some(Op::Fill {
        path: Arc::new(path),
        brush: BrushSpec {
            brush: Brush::Solid { rgba },
            blend: BlendMode::SourceOver,
            anti_alias: true,
            spots: Vec::new(),
            process_tints: None,
            all_colorants: None,
            overprint_process: None,
            cmyk: None,
        },
        rule: FillRule::EvenOdd,
        ctm: Transform::identity(),
        clip: None,
        bounds: Some(bounds),
    })
}

/// The scene under and including op `i`, and how many of its paints lie
/// beneath `i`.
fn stack(
    model: &mut Model<'_>,
    ops: &[Op],
    boxes: &[DeviceBounds],
    i: usize,
) -> Result<(Group, usize), Why> {
    let lost = Why::NotFlat(Cause::Geometry, false);
    let bx = *boxes.get(i).ok_or(lost)?;
    let mut elements = Vec::new();
    for (j, (op, b)) in ops.iter().zip(boxes).enumerate().take(i) {
        if overlaps(*b, bx) {
            elements.extend(model.top(j, op).map_err(|c| Why::NotFlat(c, true))?);
        }
    }
    let beneath = count_paints(&elements);
    let own = ops.get(i).ok_or(lost)?;
    elements.extend(model.top(i, own).map_err(|c| Why::NotFlat(c, false))?);
    Ok((
        Group {
            elements,
            isolated: true,
            knockout: false,
            alpha: 1.0,
            blend: Blend::Normal,
        },
        beneath,
    ))
}

/// Op `i` as opaque fills.
fn resolve(
    model: &mut Model<'_>,
    rec: &ExportRecording,
    boxes: &[DeviceBounds],
    i: usize,
    intent: CmykIntent,
    paper: [f32; 3],
) -> Result<Vec<Op>, Why> {
    let bx = *boxes.get(i).ok_or(Why::NotFlat(Cause::Geometry, false))?;
    let unrecorded = rec.sites.iter().any(|s| {
        matches!(s.cause, SiteCause::Blend | SiteCause::BlendingSpace) && overlaps(s.bounds, bx)
    });
    if unrecorded {
        return Err(Why::Unrecorded);
    }
    let (scene, beneath) = stack(model, &rec.ops, boxes, i)?;
    let regions = match flatten(&scene, model.space, &PageInks(intent), &Limits::default()) {
        Ok(r) => r,
        Err(FlattenError::TooComplex(_)) => return Err(Why::TooComplex),
        Err(_) => return Err(Why::NotFlat(Cause::Knockout, false)),
    };
    Ok(regions
        .iter()
        .filter(|r| r.paints.iter().any(|&p| p >= beneath) && r.alpha > 0.0)
        .filter_map(|r| region_op(r, model.space, intent, paper))
        .collect())
}

/// Replace every op the writer would draw inexactly with opaque regions
/// where it can be modelled. Returns the disclosure and the boxes left to
/// the pixel patch.
pub(super) fn vectorise(
    rec: &mut ExportRecording,
    render: &RenderOptions,
    background: Option<Rgb>,
) -> (EmfVectorised, Vec<DeviceBounds>) {
    let mut out = EmfVectorised::default();
    let boxes: Vec<DeviceBounds> = rec
        .ops
        .iter()
        .map(|op| op_box(op).unwrap_or(UNKNOWN))
        .collect();
    let needs = page_needs(rec, &boxes);
    if !needs.iter().any(Needs::any) {
        return (out, Vec::new());
    }
    let paper = background.map_or([1.0; 3], |b| [b.r, b.g, b.b].map(|v| f32::from(v) / 255.0));
    let space = if rec.subtractive_page {
        Space::Cmyk
    } else {
        Space::Rgb
    };
    let mut model = Model {
        space,
        clips: Clips::new(&rec.clips),
        rec,
        memo: vec![None; rec.ops.len()],
        masks: vec![None; rec.masks.len()],
    };
    let mut replaced: Vec<Option<Vec<Op>>> = vec![None; rec.ops.len()];
    let mut left = Vec::new();
    for (i, n) in needs.iter().enumerate() {
        if !n.any() {
            continue;
        }
        let Some(bx) = boxes.get(i).copied() else {
            continue;
        };
        match resolve(&mut model, rec, &boxes, i, render.cmyk_intent, paper) {
            Ok(fills) => {
                out.count(n, fills.len());
                if let Some(slot) = replaced.get_mut(i) {
                    *slot = Some(fills);
                }
            }
            Err(why) => {
                out.count_patched(why);
                left.push(bx);
            }
        }
    }
    drop(model);
    splice(rec, &boxes, replaced);
    (out, left)
}

/// What each op of the page needs vectorised for.
fn page_needs(rec: &ExportRecording, boxes: &[DeviceBounds]) -> Vec<Needs> {
    rec.ops
        .iter()
        .zip(boxes)
        .map(|(op, b)| {
            let mut n = needs(op, *b, rec);
            n.alpha |= rec.subtractive_page && crate::patch::composites(op) && !n.any();
            n
        })
        .collect()
}

/// Put each replaced op's fills where it stood, and drop the approximation
/// sites inside the replaced boxes.
fn splice(rec: &mut ExportRecording, boxes: &[DeviceBounds], replaced: Vec<Option<Vec<Op>>>) {
    let done: Vec<DeviceBounds> = boxes
        .iter()
        .zip(&replaced)
        .filter(|(_, r)| r.is_some())
        .map(|(b, _)| *b)
        .collect();
    rec.sites.retain(|s| {
        !(matches!(s.cause, SiteCause::Overprint | SiteCause::NonIsolatedGroup)
            && done.iter().any(|d| within(s.bounds, *d)))
    });
    let ops = std::mem::take(&mut rec.ops);
    for (op, r) in ops.into_iter().zip(replaced) {
        match r {
            Some(fills) => rec.ops.extend(fills),
            None => rec.ops.push(op),
        }
    }
}

impl EmfVectorised {
    fn count(&mut self, n: &Needs, regions: usize) {
        self.ops += 1;
        self.regions += regions;
        self.alpha += usize::from(n.alpha);
        self.groups += usize::from(n.group);
        self.overprint += usize::from(n.overprint);
        self.blends += usize::from(n.blend);
    }

    fn count_patched(&mut self, why: Why) {
        match why {
            Why::NotFlat(cause, beneath) => {
                self.patched_not_flat += 1;
                self.not_flat.count(cause, beneath);
            }
            Why::TooComplex => self.patched_too_complex += 1,
            Why::Unrecorded => self.patched_unrecorded += 1,
        }
    }
}

//! An axial or radial shading fill as flat-colour band paints, so it
//! composites with the paint around it as vector regions instead of
//! patching the stack (ISO 32000-2 §8.7.4.5.3–4). A print RIP flattens a
//! smooth shade under transparency the same way: bands at the device's
//! colour resolution. Each band is within half a level of the smooth shade
//! (`pdfcer_render::writer_seam::BAND_STEP`).
//!
//! An overprinted shading on an ink page bands as overprinted flat paints:
//! each band carries the raster's per-component rules with its own tints,
//! so the flattener composites it against what is beneath, as a press does
//! per separation (§11.7.4.3; `OPM` never applies to a shading, §8.6.7).
//!
//! Left to the patch, as [`Cause::Shading`]: type 1 and the meshes, and a
//! radial whose bands overlap (circles that do not nest) under a constant
//! alpha or a blend, which would composite the overlap twice. A spot shading is [`Cause::Spot`] (a gradient one only
//! on a CMYK page: on an RGB page the raster paints its alternate too); one
//! on a CMYK page without process ink is [`Cause::Cmyk`].

use pdfcer_flatten::{
    Blend, Colour, Element, FillRule, Overprint, Paint, Point, SPOT_PLANES, Shape, Space,
};
use pdfcer_render::overprint::ComponentRule;
use pdfcer_render::writer_seam::{
    Bands, Brush, BrushSpec, ClipId, DeviceBounds, Op, ShadingSource, band, band_gradient,
};
use tiny_skia::{BlendMode, Transform};

use super::{Model, blend_of};
use crate::vector_census::Cause;
use crate::vector_geom;

/// The most quadrilaterals one shading may band into.
const MAX_QUADS: usize = 16384;

/// What the paint state said about a shading when it was recorded.
struct Facts {
    alpha: f32,
    overprint: Option<[ComponentRule; 4]>,
    spot: bool,
    blend: BlendMode,
}

impl Model<'_> {
    /// A gradient or harvested-shading fill or stroke as one paint per band.
    pub(super) fn shaded(&mut self, op: &Op, out: &mut Vec<Element>) -> Result<(), Cause> {
        let (brush, shape, clip, ctm, bounds) = parts(op)?;
        if !brush.spots.is_empty() || brush.all_colorants.is_some() {
            return Err(Cause::Spot);
        }
        if brush.overprint_process.is_some() {
            return Err(Cause::Shading);
        }
        let (bands, paint, inner) = self.bands_of(brush, ctm, bounds)?;
        if paint.spot && self.space == Space::Cmyk {
            return Err(Cause::Spot);
        }
        let alpha = paint.alpha;
        // The recorded brush is `SourceOver`; the mode in force rides on
        // the shading.
        let blend = if brush.blend == BlendMode::SourceOver {
            paint.blend
        } else {
            brush.blend
        };
        let blend = blend_of(blend).ok_or(Cause::Blend)?;
        if bands.overlapping && (alpha < 1.0 || blend != Blend::Normal) {
            return Err(Cause::Shading);
        }
        let mut shape = shape.ok_or(Cause::Geometry)?;
        for s in inner {
            shape = shape.intersection(&s);
        }
        let shape = self.clips.apply(shape, clip).ok_or(Cause::Geometry)?;
        // An additive page's raster paints an overprinted shading normally.
        let rules = paint.overprint.filter(|_| self.space == Space::Cmyk);
        for b in &bands.bands {
            let colour = match self.space {
                Space::Cmyk => b.cmyk.ok_or(Cause::Cmyk)?,
                Space::Rgb => [b.rgb[0], b.rgb[1], b.rgb[2], 0.0],
            };
            let cut = quads_shape(&b.quads).intersection(&shape);
            if cut.contours.is_empty() {
                continue;
            }
            out.push(Element::Paint(Paint {
                shape: cut,
                colour,
                alpha,
                blend,
                overprint: rules.map(|r| overprinted(r, colour)),
                spots: [0.0; SPOT_PLANES],
            }));
        }
        Ok(())
    }

    /// The brush's shading as bands, its paint state, and the shading's
    /// own clips (a pattern's fill area, `/BBox`) in device space.
    fn bands_of(
        &self,
        brush: &BrushSpec,
        ctm: Transform,
        bounds: Option<DeviceBounds>,
    ) -> Result<(Bands, Facts, Vec<Shape>), Cause> {
        match &brush.brush {
            Brush::Solid { .. } => Err(Cause::Geometry),
            Brush::Gradient(spec) => {
                let b = bounds.ok_or(Cause::Geometry)?;
                let (w, h) = self.rec.page_size;
                #[allow(clippy::cast_precision_loss)]
                let region = [
                    b.left.max(0.0),
                    b.top.max(0.0),
                    b.right.min(w as f32),
                    b.bottom.min(h as f32),
                ];
                let to_device = ctm.pre_concat(spec.transform);
                let got = band_gradient(spec, to_device, region, MAX_QUADS);
                let facts = Facts {
                    alpha: spec.alpha,
                    overprint: spec.overprint,
                    spot: spec.spot,
                    blend: spec.blend,
                };
                Ok((got.map_err(|_| Cause::Shading)?, facts, Vec::new()))
            }
            Brush::Image { texels, .. } => {
                let src = self
                    .rec
                    .shadings
                    .iter()
                    .find(|(t, _)| std::sync::Arc::ptr_eq(t, texels))
                    .map(|(_, s)| s)
                    .ok_or(Cause::Image)?;
                let got = band(src, MAX_QUADS).map_err(|_| Cause::Shading)?;
                // A spot rides on the brush (`BrushSpec::spots`).
                let facts = Facts {
                    alpha: src.alpha,
                    overprint: src.overprint,
                    spot: false,
                    blend: src.blend,
                };
                Ok((got, facts, own_clips(src)?))
            }
        }
    }
}

/// A band in `colour` under the shading's overprint `rules`: every spot
/// plane keeps the backdrop, as the colorant buffer's shading route does.
fn overprinted(rules: [ComponentRule; 4], colour: Colour) -> Overprint {
    let mut tints = [None; 4];
    for ((t, rule), c) in tints.iter_mut().zip(rules).zip(colour) {
        *t = match rule {
            ComponentRule::Source => Some(c),
            ComponentRule::Zero => Some(0.0),
            ComponentRule::Backdrop => None,
        };
    }
    Overprint {
        tints,
        model: 0,
        spots: [None; SPOT_PLANES],
    }
}

/// A fill or stroke's brush, device shape, clip, CTM and bounds.
#[allow(clippy::type_complexity)] // a one-call destructuring, not a concept
fn parts(
    op: &Op,
) -> Result<
    (
        &BrushSpec,
        Option<Shape>,
        Option<ClipId>,
        Transform,
        Option<DeviceBounds>,
    ),
    Cause,
> {
    Ok(match op {
        Op::Fill {
            path,
            brush,
            rule,
            ctm,
            clip,
            bounds,
        } => (
            brush,
            vector_geom::fill(path, *rule, *ctm),
            *clip,
            *ctm,
            *bounds,
        ),
        Op::Stroke {
            path,
            brush,
            stroke,
            ctm,
            clip,
            bounds,
        } => (
            brush,
            vector_geom::stroke(path, stroke, *ctm),
            *clip,
            *ctm,
            *bounds,
        ),
        Op::Layer { .. } | Op::Text { .. } => return Err(Cause::Geometry),
    })
}

/// A shading's fill area and `/BBox`, device space.
fn own_clips(src: &ShadingSource) -> Result<Vec<Shape>, Cause> {
    let mut out = Vec::new();
    if let Some((area, rule)) = &src.area {
        out.push(vector_geom::fill(area, *rule, Transform::identity()).ok_or(Cause::Geometry)?);
    }
    if let Some(q) = src.bbox_device() {
        out.push(quads_shape(&[q]));
    }
    Ok(out)
}

/// The union of `quads`, each turned to wind positively.
fn quads_shape(quads: &[[[f32; 2]; 4]]) -> Shape {
    let contours = quads
        .iter()
        .map(|q| {
            let mut c: Vec<Point> = q
                .iter()
                .map(|p| [f64::from(p[0]), f64::from(p[1])])
                .collect();
            if area2(&c) < 0.0 {
                c.reverse();
            }
            c
        })
        .collect();
    Shape {
        contours,
        rule: FillRule::NonZero,
    }
}

/// Twice the signed area of `c`.
fn area2(c: &[Point]) -> f64 {
    c.iter()
        .zip(c.iter().cycle().skip(1))
        .map(|(a, b)| a[0].mul_add(b[1], -(b[0] * a[1])))
        .sum()
}

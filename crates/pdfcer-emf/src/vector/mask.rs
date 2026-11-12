//! A soft-masked layer as constant-mask pieces (ISO 32000-2 §11.6.5).
//!
//! A mask whose group is flat colour takes one value over each region of
//! its flattened group, so the masked layer becomes one group per region:
//! its content clipped to the region, its opacity scaled by the value.
//! Exact, because compositing is pointwise and the mask is constant inside
//! a region. A mask with an image, a gradient or a varying nested mask in
//! its group stays a patch, as does a luminosity mask whose subtractive
//! group composites anything translucent.

use std::collections::BTreeMap;
use std::sync::Arc;

use pdfcer_flatten::{
    Blend, Element, FillRule, Group, Limits, NaiveInks, Paint, Point, Shape, Space, flatten,
};
use pdfcer_render::writer_seam::MaskSource;
use tiny_skia::Mask;

use super::{Model, count_paints};
use crate::vector_census::Cause;

/// Disjoint shapes and the mask value in `0..=1` over each; value zero is
/// left out.
pub(super) type Pieces = Arc<Vec<(Shape, f32)>>;

/// The most `pieces × paints` one masked layer may expand into.
const MAX_EXPANSION: usize = 4096;

/// §11.5.3 NOTE 3: the luminosity of an sRGB colour, with no gamma.
fn luminosity([r, g, b, _]: [f32; 4]) -> f32 {
    0.30f32.mul_add(r, 0.59f32.mul_add(g, 0.11 * b))
}

impl Model<'_> {
    /// `group` (a layer's content and compositing) under `mask`, as one
    /// group per constant-mask piece.
    pub(super) fn masked(&mut self, mask: &Arc<Mask>, group: Group) -> Result<Vec<Element>, Cause> {
        let pieces = self.mask_pieces(mask)?;
        let paints = count_paints(&group.elements).max(1);
        if pieces.len().saturating_mul(paints) > MAX_EXPANSION {
            return Err(Cause::MaskFlat);
        }
        Ok(pieces
            .iter()
            .map(|(shape, v)| {
                Element::Group(Group {
                    elements: clip_all(&group.elements, shape),
                    isolated: group.isolated,
                    knockout: group.knockout,
                    alpha: group.alpha * v,
                    blend: group.blend,
                })
            })
            .collect())
    }

    /// The pieces of recorded mask `mask`, each built once.
    fn mask_pieces(&mut self, mask: &Arc<Mask>) -> Result<Pieces, Cause> {
        let rec = self.rec;
        let i = rec
            .masks
            .iter()
            .position(|(m, _)| Arc::ptr_eq(m, mask))
            .ok_or(Cause::MaskUnrecorded)?;
        if let Some(Some(got)) = self.masks.get(i) {
            return got.clone();
        }
        let source = rec.masks.get(i).ok_or(Cause::MaskUnrecorded)?;
        let got = self.build_pieces(&source.1).map(Arc::new);
        if let Some(slot) = self.masks.get_mut(i) {
            *slot = Some(got.clone());
        }
        got
    }

    /// The mask group's content as flattener elements, in sRGB.
    ///
    /// A subtractive group (§11.3.4) composites in ink and is converted
    /// to sRGB only afterwards. Its alpha, which is all an alpha mask
    /// reads, does not depend on the space; its colour does, unless every
    /// paint and group is opaque and blends normally, when each point
    /// shows the topmost paint's own colour, or the backdrop where none
    /// covers it. A luminosity mask on a subtractive group outside that
    /// case stays a patch.
    fn mask_content(&mut self, src: &MaskSource) -> Result<Vec<Element>, Cause> {
        let space = std::mem::replace(&mut self.space, Space::Rgb);
        let mut content = Vec::new();
        let built = src
            .ops
            .iter()
            .try_for_each(|op| self.element(op, &mut content));
        self.space = space;
        built.map_err(|c| match c {
            Cause::Image | Cause::Shading | Cause::MaskVarying => Cause::MaskVarying,
            Cause::MaskUnrecorded => Cause::MaskUnrecorded,
            _ => Cause::MaskFlat,
        })?;
        if src.subtractive && src.luminosity && !opaque_normal(&content) {
            return Err(Cause::MaskFlat);
        }
        Ok(content)
    }

    fn build_pieces(&mut self, src: &MaskSource) -> Result<Vec<(Shape, f32)>, Cause> {
        let mut content = self.mask_content(src)?;
        // §11.5.2: an alpha mask is the group's alpha alone. Painting the
        // group white over black makes its luminosity that alpha.
        let backdrop = if src.luminosity {
            src.backdrop
        } else {
            whiten(&mut content);
            [0.0; 3]
        };
        let (w, h) = self.rec.page_size;
        let scene = mask_scene(content, backdrop, f64::from(w), f64::from(h));
        let regions = flatten(&scene, Space::Rgb, &NaiveInks, &Limits::default())
            .map_err(|_| Cause::MaskFlat)?;
        let mut by_value: BTreeMap<u8, Vec<Vec<Point>>> = BTreeMap::new();
        for r in regions {
            // Clamped to 0..=255 before the cast.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let key = (src.transfer(luminosity(r.colour)) * 255.0).round() as u8;
            if key > 0 {
                by_value.entry(key).or_default().extend(r.contours);
            }
        }
        Ok(by_value
            .into_iter()
            .map(|(k, contours)| {
                let shape = Shape {
                    contours,
                    rule: FillRule::EvenOdd,
                };
                (shape, f32::from(k) / 255.0)
            })
            .collect())
    }
}

/// §11.5.3: the mask group, isolated, composited over an opaque page of
/// `backdrop` (`/BC`).
fn mask_scene(content: Vec<Element>, [r, g, b]: [f32; 3], w: f64, h: f64) -> Group {
    let page = Shape {
        contours: vec![vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]],
        rule: FillRule::NonZero,
    };
    let group = |elements| Group {
        elements,
        isolated: true,
        knockout: false,
        alpha: 1.0,
        blend: Blend::Normal,
    };
    group(vec![
        Element::Paint(Paint {
            shape: page,
            colour: [r, g, b, 0.0],
            alpha: 1.0,
            blend: Blend::Normal,
            ..Paint::default()
        }),
        Element::Group(group(content)),
    ])
}

/// Every paint white and blended normally, so the composite's colour is
/// its alpha. Shape, alpha and grouping, which decide alpha, are kept.
fn whiten(elements: &mut [Element]) {
    for e in elements {
        match e {
            Element::Paint(p) => {
                p.colour = [1.0, 1.0, 1.0, 0.0];
                p.blend = Blend::Normal;
                p.overprint = None;
            }
            Element::Group(g) => {
                g.blend = Blend::Normal;
                whiten(&mut g.elements);
            }
        }
    }
}

/// Every paint and group fully opaque, blending normally, with no
/// overprint: the composite at a point is its topmost paint's colour in
/// any blending space.
fn opaque_normal(elements: &[Element]) -> bool {
    elements.iter().all(|e| match e {
        Element::Paint(p) => p.alpha >= 1.0 && p.blend == Blend::Normal && p.overprint.is_none(),
        Element::Group(g) => {
            g.alpha >= 1.0 && g.blend == Blend::Normal && opaque_normal(&g.elements)
        }
    })
}

/// `elements` with every paint cut to `region`; paints left empty dropped.
fn clip_all(elements: &[Element], region: &Shape) -> Vec<Element> {
    elements
        .iter()
        .filter_map(|e| match e {
            Element::Paint(p) => {
                let shape = p.shape.intersection(region);
                (!shape.contours.is_empty()).then(|| Element::Paint(Paint { shape, ..p.clone() }))
            }
            Element::Group(g) => Some(Element::Group(Group {
                elements: clip_all(&g.elements, region),
                isolated: g.isolated,
                knockout: g.knockout,
                alpha: g.alpha,
                blend: g.blend,
            })),
        })
        .collect()
}

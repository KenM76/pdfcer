//! An axial or radial shading as flat-colour bands (ISO 32000-2
//! §8.7.4.5.3, §8.7.4.5.4), for a writer that composites it with other
//! paint as vector regions: what a print RIP does with a smooth shade it
//! cannot hand to the device as a gradient.
//!
//! The band geometry is the Gouraud tessellation's, with each knot span
//! first split so no band's colour changes by more than [`BAND_STEP`]
//! (where the bands are wider than [`MIN_BAND_PX`]). A band is painted in
//! the colour at its middle, so it is within half a step of the smooth
//! shade everywhere.

use tiny_skia::Transform;

use super::{
    Decline, Frame, Gouraud, Knot, MIN_BAND_PX, ShadingSource, axial, dist, lerp3, ramp_knots,
    rings, split, stop_knots,
};
use crate::shading::{Geometry, GradientKind, GradientSpec, SpotTints};

/// The most a flat band's colour changes across it, in any sRGB channel.
pub const BAND_STEP: f32 = 1.0 / 255.0 + 1e-6;

/// One flat-colour band, in device pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatBand {
    /// Quadrilaterals covering the band, each in boundary order. An axial
    /// band is one; a radial band is a ring of them.
    pub quads: Vec<[[f32; 2]; 4]>,
    /// The colour at the band's middle, sRGB `0..=1`.
    pub rgb: [f32; 3],
    /// For a ramp in process CMYK, the authored ink there
    /// (`ColorRamp::at_cmyk`, or [`GradientSpec::cmyk`] interpolated);
    /// `None` otherwise.
    pub cmyk: Option<[f32; 4]>,
    /// For a ramp whose spots the source deposits on planes of their own
    /// ([`ShadingSource::spots`], [`GradientSpec::spots`]), the authored
    /// tints there; `None` otherwise.
    pub authored: Option<SpotTints>,
}

/// A shading as flat bands, in paint order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bands {
    /// The bands; a later one paints over an earlier one where they meet.
    pub bands: Vec<FlatBand>,
    /// Whether bands overlap (a radial whose circles do not nest,
    /// §8.7.4.5.4): only an opaque, normally blended paint composites
    /// them as the shading does.
    pub overlapping: bool,
}

impl Bands {
    /// The number of quadrilaterals across every band.
    #[must_use]
    pub fn quads(&self) -> usize {
        self.bands.iter().map(|b| b.quads.len()).sum()
    }
}

/// [`super::tessellate`]'s shading as flat bands covering `src.region`,
/// at most `max` quadrilaterals. The constant alpha is the caller's.
///
/// # Errors
///
/// [`Decline::FunctionBased`] for type 1, [`Decline::Unpaintable`] for a
/// mesh (types 4–7) or degenerate geometry, [`Decline::IncompleteRamp`],
/// and [`Decline::TooManyTriangles`] past `max`.
pub fn band(src: &ShadingSource, max: usize) -> Result<Bands, Decline> {
    let to_device = src.to_target.invert().ok_or(Decline::Unpaintable)?;
    let sh = &*src.shading;
    #[allow(clippy::cast_precision_loss)]
    let region = [
        src.region.0 as f32,
        src.region.1 as f32,
        src.region.2 as f32,
        src.region.3 as f32,
    ];
    let frame = Frame::new(to_device, region).ok_or(Decline::Unpaintable)?;
    let ramp = sh.ramp.as_ref().ok_or(Decline::IncompleteRamp)?;
    let knots = ramp_knots(ramp)?;
    let [t0, t1] = ramp.domain();
    let planed = !src.spots.is_empty();
    let colour = |s: f32| {
        let t = s.clamp(0.0, 1.0).mul_add(t1 - t0, t0);
        let c = ramp.at(t).ok_or(Decline::IncompleteRamp)?;
        let authored = planed
            .then(|| ramp.at_authored(t))
            .flatten()
            .map(|(process, spots)| SpotTints {
                process,
                spots: spots.to_vec(),
            });
        Ok(([c.r, c.g, c.b], ramp.at_cmyk(t), authored))
    };
    match sh.geometry {
        Geometry::FunctionBased { .. } => Err(Decline::FunctionBased),
        Geometry::Mesh { .. } => Err(Decline::Unpaintable),
        Geometry::Axial { coords, extend, .. } => {
            let per_s = axial_per_s(coords, &frame);
            let g = axial(coords, extend, &by_s(&knots, per_s), &frame, max * 2)?;
            collect(&g, false, colour)
        }
        Geometry::Radial { coords, extend, .. } => {
            let (per_s, overlapping) = radial_per_s(coords, &frame);
            let g = rings(coords, extend, &by_s(&knots, per_s), &frame, max * 2)?;
            collect(&g, overlapping, colour)
        }
    }
}

/// [`super::tessellate_gradient`]'s gradient as flat bands. `to_device`
/// maps gradient space to device pixels; `region` is `[x0, y0, x1, y1]`.
///
/// # Errors
///
/// As [`band`].
pub fn band_gradient(
    spec: &GradientSpec,
    to_device: Transform,
    region: [f32; 4],
    max: usize,
) -> Result<Bands, Decline> {
    let frame = Frame::new(to_device, region).ok_or(Decline::Unpaintable)?;
    let knots = stop_knots(&spec.stops).ok_or(Decline::IncompleteRamp)?;
    let cmyk = spec.cmyk.as_deref();
    let colour = |s: f32| {
        Ok((
            colour_at(&knots, s),
            cmyk.and_then(|c| cmyk_at(c, s)),
            spec.spots.as_ref().and_then(|r| r.at(s)),
        ))
    };
    match spec.kind {
        GradientKind::Linear { x0, y0, x1, y1 } => {
            let coords = [x0, y0, x1, y1];
            let per_s = axial_per_s(coords, &frame);
            let g = axial(coords, spec.extend, &by_s(&knots, per_s), &frame, max * 2)?;
            collect(&g, false, colour)
        }
        GradientKind::Radial { cx, cy, r, fx, fy } => {
            let coords = [fx, fy, 0.0, cx, cy, r];
            let (per_s, overlapping) = radial_per_s(coords, &frame);
            let g = rings(coords, spec.extend, &by_s(&knots, per_s), &frame, max * 2)?;
            collect(&g, overlapping, colour)
        }
    }
}

/// Bands per unit `s` that keep each at least [`MIN_BAND_PX`] wide.
fn axial_per_s([x0, y0, x1, y1]: [f32; 4], frame: &Frame) -> f32 {
    dist([x0, y0], [x1, y1]) * frame.min_scale() / MIN_BAND_PX
}

/// As [`axial_per_s`] where the rings are widest, and whether the circles
/// overlap. Where the circles crowd a band is thinner; a floor there would
/// leave the wide side a few coarse steps when the circles nearly touch.
fn radial_per_s(coords: [f32; 6], frame: &Frame) -> (f32, bool) {
    let c = super::Circles(coords);
    let [x0, y0, r0, x1, y1, r1] = coords;
    let widest = dist([x0, y0], [x1, y1]) + (r1 - r0).abs();
    let most = (widest * frame.min_scale()).max(1.0);
    (most / MIN_BAND_PX, c.spread() >= 0.0)
}

/// `knots` split to [`BAND_STEP`], each knot's colour replaced by its `s`,
/// so the tessellation's vertex colours carry the parameter.
fn by_s(knots: &[Knot], per_s: f32) -> Vec<Knot> {
    split(knots, BAND_STEP, per_s)
        .into_iter()
        .map(|(s, _)| (s, [s, 0.0, 0.0]))
        .collect()
}

/// The colour of a stop ramp at `s`.
fn colour_at(knots: &[Knot], s: f32) -> [f32; 3] {
    let s = s.clamp(0.0, 1.0);
    let mut prev: Option<Knot> = None;
    for &(ks, kc) in knots {
        if ks >= s {
            return match prev {
                Some((ps, pc)) if ks > ps => lerp3(pc, kc, (s - ps) / (ks - ps)),
                _ => kc,
            };
        }
        prev = Some((ks, kc));
    }
    prev.map_or([0.0; 3], |k| k.1)
}

/// The authored ink of a sampled CMYK ramp at `s`.
fn cmyk_at(samples: &[(f32, [f32; 4])], s: f32) -> Option<[f32; 4]> {
    let s = s.clamp(0.0, 1.0);
    let i = samples.iter().position(|&(u, _)| u >= s)?;
    let (ub, kb) = *samples.get(i)?;
    let Some(&(ua, ka)) = i.checked_sub(1).and_then(|j| samples.get(j)) else {
        return Some(kb);
    };
    let f = if ub > ua { (s - ua) / (ub - ua) } else { 0.0 };
    let mut out = ka;
    for (o, (a, b)) in out.iter_mut().zip(ka.iter().zip(kb)) {
        *o = (b - a).mul_add(f, *a);
    }
    Some(out)
}

type Colour = Result<([f32; 3], Option<[f32; 4]>, Option<SpotTints>), Decline>;

/// `g`'s quads (two triangles each, [`Gouraud::quad`]) as bands: the first
/// vertex colour carries `sa`, the third `sb`.
fn collect(
    g: &Gouraud,
    overlapping: bool,
    colour: impl Fn(f32) -> Colour,
) -> Result<Bands, Decline> {
    let mut out = Bands {
        bands: Vec::new(),
        overlapping,
    };
    let mut current: Option<(f32, f32)> = None;
    for [[a0, a1, b0], [_, b1, _]] in g.triangles.as_chunks::<2>().0.iter().copied() {
        let at = |i: u32| g.points.get(i as usize).copied().unwrap_or_default();
        let s = |i: u32| g.colours.get(i as usize).map_or(0.0, |c| c[0]);
        let key = (s(a0), s(b0));
        let quad = [at(a0), at(a1), at(b1), at(b0)];
        if current != Some(key) {
            let (rgb, cmyk, authored) = colour((key.0 + key.1) / 2.0)?;
            out.bands.push(FlatBand {
                quads: Vec::new(),
                rgb,
                cmyk,
                authored,
            });
            current = Some(key);
        }
        if let Some(b) = out.bands.last_mut() {
            b.quads.push(quad);
        }
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn spec(kind: GradientKind) -> GradientSpec {
        GradientSpec {
            kind,
            stops: vec![(0.0, [0, 0, 0]), (1.0, [255, 255, 255])],
            extend: [true, true],
            transform: Transform::identity(),
            alpha: 1.0,
            cmyk: None,
            spot: false,
            spots: None,
            overprint: None,
            blend: tiny_skia::BlendMode::SourceOver,
        }
    }

    /// Black to white across 1000 device pixels: about four per level.
    const ACROSS: GradientKind = GradientKind::Linear {
        x0: 0.0,
        y0: 0.0,
        x1: 1000.0,
        y1: 0.0,
    };

    const REGION: [f32; 4] = [0.0, 0.0, 1000.0, 10.0];

    fn linear() -> Bands {
        band_gradient(&spec(ACROSS), Transform::identity(), REGION, 4096).unwrap()
    }

    #[test]
    fn an_axial_band_is_within_half_a_level_of_the_smooth_shade() {
        let b = linear();
        assert!(!b.overlapping);
        assert!((255..=260).contains(&b.bands.len()), "{}", b.bands.len());
        for band in &b.bands {
            assert_eq!(band.quads.len(), 1);
            for p in band.quads[0] {
                let smooth = (p[0] / 1000.0).clamp(0.0, 1.0);
                let err = (band.rgb[0] - smooth).abs() * 255.0;
                assert!(err <= 0.51, "x {}: {} levels off", p[0], err);
            }
        }
    }

    #[test]
    fn axial_bands_cover_the_region_in_order() {
        let b = linear();
        let xs: Vec<f32> = b.bands.iter().map(|b| b.quads[0][0][0]).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]));
        let all = || b.bands.iter().flat_map(|b| b.quads[0]).map(|p| p[0]);
        assert!(all().fold(f32::MAX, f32::min) <= 0.0);
        assert!(all().fold(f32::MIN, f32::max) >= 1000.0);
    }

    #[test]
    fn radial_bands_overlap_only_when_the_circles_do_not_nest() {
        let region = [0.0, 0.0, 100.0, 100.0];
        let nested = GradientKind::Radial {
            cx: 50.0,
            cy: 50.0,
            r: 40.0,
            fx: 55.0,
            fy: 50.0,
        };
        let got = band_gradient(&spec(nested), Transform::identity(), region, 1 << 20).unwrap();
        assert!(!got.overlapping);
        // The rings are 35 pixels apart where they crowd: a band a pixel
        // wide there, so fewer bands than levels.
        assert!((20..=40).contains(&got.bands.len()), "{}", got.bands.len());
        assert!(got.bands.iter().all(|b| b.quads.len() >= 16));
        let mut apart = spec(GradientKind::Radial {
            cx: 80.0,
            cy: 50.0,
            r: 10.0,
            fx: 20.0,
            fy: 50.0,
        });
        apart.extend = [false, false];
        let got = band_gradient(&apart, Transform::identity(), region, 1 << 20).unwrap();
        assert!(got.overlapping);
    }

    #[test]
    fn a_cmyk_gradient_band_carries_its_authored_ink() {
        let mut cyan = spec(ACROSS);
        cyan.cmyk = Some(vec![(0.0, [0.0; 4]), (1.0, [1.0, 0.0, 0.0, 0.0])]);
        let b = band_gradient(&cyan, Transform::identity(), REGION, 4096).unwrap();
        for band in &b.bands {
            let k = band.cmyk.unwrap();
            assert!((k[0] - band.rgb[0]).abs() < 1e-4, "{k:?} vs {:?}", band.rgb);
            assert_eq!(k[1..], [0.0; 3]);
        }
        assert!(linear().bands.iter().all(|b| b.cmyk.is_none()));
    }

    #[test]
    fn past_the_ceiling_banding_declines() {
        let got = band_gradient(&spec(ACROSS), Transform::identity(), REGION, 8);
        assert_eq!(got, Err(Decline::TooManyTriangles));
    }
}

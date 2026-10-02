//! Gradients as shading patterns and SVG patterns as tiling patterns.
//!
//! ISO 32000-1 §8.7.4.5.3 (axial, type 2) and §8.7.4.5.4 (radial, type 3)
//! shadings with `/Extend [true true]` reproduce `spreadMethod="pad"`;
//! `repeat` and `reflect` extend the shading's `/Domain` over whole periods
//! and stitch copies of the stop function (§7.10.4, type 3), reversed on odd
//! periods for `reflect`. Colour stops become type 2 (§7.10.3) segments.

use usvg::{Paint, SpreadMethod, Transform};

use super::emit::{Emitter, Mode, invoke};
use super::objects::{
    Canvas, GroupKind, Res, SvgObject, finish_form, matrix_obj, name, real, reals,
};
use super::{SvgFeature, SvgImportError};
use crate::object::{Dict, Name, ObjId, Object};

/// Most whole periods a `repeat`/`reflect` gradient is unrolled to.
const MAX_PERIODS: i64 = 64;
/// Patterns nested deeper than this are skipped.
const MAX_PATTERN_DEPTH: usize = 4;

#[derive(Clone, Copy)]
enum Kind {
    Linear {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    },
    Radial {
        cx: f64,
        cy: f64,
        r: f64,
        fx: f64,
        fy: f64,
    },
}

struct Geom {
    shading_type: i64,
    coords: Vec<f64>,
    domain: [f64; 2],
    /// `(first, last, reflect)` periods when unrolled.
    periods: Option<(i64, i64, bool)>,
}

fn gradient_kind(paint: &Paint) -> (&usvg::BaseGradient, Kind) {
    match paint {
        Paint::LinearGradient(g) => (
            g,
            Kind::Linear {
                x1: f64::from(g.x1()),
                y1: f64::from(g.y1()),
                x2: f64::from(g.x2()),
                y2: f64::from(g.y2()),
            },
        ),
        Paint::RadialGradient(g) => (
            g,
            Kind::Radial {
                cx: f64::from(g.cx()),
                cy: f64::from(g.cy()),
                r: f64::from(g.r().get()),
                fx: f64::from(g.fx()),
                fy: f64::from(g.fy()),
            },
        ),
        Paint::Color(_) | Paint::Pattern(_) => unreachable!("gradient() takes gradient paints"),
    }
}

impl Emitter {
    /// A shading pattern for a gradient paint, plus the constant alpha to
    /// set with it and, when stop opacities vary, a luminosity soft mask.
    pub(super) fn gradient(
        &mut self,
        paint: &Paint,
        opacity: f32,
        m: Transform,
        bbox: usvg::Rect,
        canvas: &Canvas,
    ) -> Result<(ObjId, f32, Option<Object>), SvgImportError> {
        let (base, kind) = gradient_kind(paint);
        let geom = self.geometry(kind, base.transform(), base.spread_method(), bbox);
        let colours: Vec<(f64, Vec<f64>)> = base
            .stops()
            .iter()
            .map(|s| {
                let c = s.color();
                let rgb = [c.red, c.green, c.blue].map(|v| f64::from(v) / 255.0);
                (f64::from(s.offset().get()), rgb.to_vec())
            })
            .collect();
        let alphas: Vec<f32> = base
            .stops()
            .iter()
            .map(|s| s.opacity().get() * opacity)
            .collect();
        let pm = m.pre_concat(base.transform());

        let function = self.function(&colours, &geom)?;
        let mut pattern = Dict::new();
        pattern.insert(Name::from(b"Type"), name(b"Pattern"));
        pattern.insert(Name::from(b"PatternType"), Object::Integer(2));
        pattern.insert(
            Name::from(b"Shading"),
            Object::Dict(shading(b"DeviceRGB", &geom, function)),
        );
        pattern.insert(Name::from(b"Matrix"), matrix_obj(pm));
        let pid = self.table.push(SvgObject::Dict(pattern))?;

        let first = alphas.first().copied().unwrap_or(1.0);
        if alphas.iter().all(|a| *a == first) {
            return Ok((pid, first, None));
        }
        let mask = self.alpha_mask(&colours, &alphas, &geom, pm, canvas)?;
        Ok((pid, 1.0, Some(mask)))
    }

    /// Varying stop opacity: the same geometry, shaded in gray, as a
    /// luminosity soft mask (§11.6.5.2) in canvas space.
    fn alpha_mask(
        &mut self,
        colours: &[(f64, Vec<f64>)],
        alphas: &[f32],
        geom: &Geom,
        pm: Transform,
        canvas: &Canvas,
    ) -> Result<Object, SvgImportError> {
        let gray: Vec<(f64, Vec<f64>)> = colours
            .iter()
            .zip(alphas)
            .map(|((o, _), a)| (*o, vec![f64::from(*a)]))
            .collect();
        let function = self.function(&gray, geom)?;
        let sh = self
            .table
            .push(SvgObject::Dict(shading(b"DeviceGray", geom, function)))?;
        let mut sub = canvas.sibling();
        sub.op("q");
        sub.cm(pm);
        let n = sub.name(Res::Shading, sh);
        sub.name_op(&n, "sh");
        sub.op("Q");
        let form = finish_form(&mut self.table, sub, GroupKind::Luminosity)?;
        let mut sm = Dict::new();
        sm.insert(Name::from(b"Type"), name(b"Mask"));
        sm.insert(Name::from(b"S"), name(b"Luminosity"));
        sm.insert(Name::from(b"G"), Object::Reference(form));
        Ok(Object::Dict(sm))
    }

    /// Coordinates and domain, unrolling `repeat`/`reflect` over the
    /// periods that cover `bbox` (user space).
    fn geometry(
        &mut self,
        kind: Kind,
        gt: Transform,
        spread: SpreadMethod,
        bbox: usvg::Rect,
    ) -> Geom {
        let pad = match kind {
            Kind::Linear { x1, y1, x2, y2 } => Geom {
                shading_type: 2,
                coords: vec![x1, y1, x2, y2],
                domain: [0.0, 1.0],
                periods: None,
            },
            Kind::Radial { cx, cy, r, fx, fy } => Geom {
                shading_type: 3,
                coords: vec![fx, fy, 0.0, cx, cy, r],
                domain: [0.0, 1.0],
                periods: None,
            },
        };
        if spread == SpreadMethod::Pad {
            return pad;
        }
        let Some(range) = gt.invert().and_then(|inv| period_range(kind, inv, bbox)) else {
            self.notes.approximate(SvgFeature::GradientSpread);
            return pad;
        };
        let (k0, k1) = range;
        let reflect = spread == SpreadMethod::Reflect;
        let (a, b) = (k0 as f64, k1 as f64);
        let coords = match kind {
            Kind::Linear { x1, y1, x2, y2 } => {
                let (dx, dy) = (x2 - x1, y2 - y1);
                vec![x1 + a * dx, y1 + a * dy, x1 + b * dx, y1 + b * dy]
            }
            Kind::Radial { cx, cy, r, fx, fy } => {
                vec![fx, fy, 0.0, fx + b * (cx - fx), fy + b * (cy - fy), b * r]
            }
        };
        Geom {
            coords,
            domain: [a, b],
            periods: Some((k0, k1, reflect)),
            ..pad
        }
    }

    /// The stop function, unrolled over the geometry's periods.
    fn function(
        &mut self,
        stops: &[(f64, Vec<f64>)],
        geom: &Geom,
    ) -> Result<Object, SvgImportError> {
        let base = stop_function(stops);
        let Some((k0, k1, reflect)) = geom.periods else {
            return Ok(base);
        };
        let id = self.table.push(SvgObject::Dict(match base {
            Object::Dict(d) => d,
            _ => Dict::new(),
        }))?;
        let mut encode = Vec::new();
        for k in k0..k1 {
            let odd = reflect && k.rem_euclid(2) == 1;
            encode.extend(if odd { [1.0, 0.0] } else { [0.0, 1.0] });
        }
        let bounds: Vec<f64> = (k0 + 1..k1).map(|k| k as f64).collect();
        Ok(stitching(
            [k0 as f64, k1 as f64],
            (k0..k1).map(|_| Object::Reference(id)).collect(),
            &bounds,
            &encode,
        ))
    }

    /// A tiling pattern (§8.7.3) for an SVG pattern paint; `None` when
    /// nested too deep.
    pub(super) fn tiling(
        &mut self,
        pat: &usvg::Pattern,
        m: Transform,
    ) -> Result<Option<ObjId>, SvgImportError> {
        if self.pattern_depth >= MAX_PATTERN_DEPTH {
            self.notes.skip(SvgFeature::NestedPattern, 1);
            return Ok(None);
        }
        let rect = pat.rect();
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let mut tile = Canvas::new(Transform::identity(), [0.0, 0.0, w, h]);
        self.pattern_depth += 1;
        let drawn = self.children(pat.root(), Transform::identity(), &mut tile, Mode::Normal);
        self.pattern_depth -= 1;
        drawn?;
        // Wrap the tile in a form so its resources stay its own.
        let form = finish_form(&mut self.table, tile, GroupKind::None)?;
        let mut cell = Canvas::new(Transform::identity(), [0.0, 0.0, w, h]);
        invoke(&mut cell, None, form);
        let mut d = Dict::new();
        d.insert(Name::from(b"Type"), name(b"Pattern"));
        d.insert(Name::from(b"PatternType"), Object::Integer(1));
        d.insert(Name::from(b"PaintType"), Object::Integer(1));
        d.insert(Name::from(b"TilingType"), Object::Integer(1));
        d.insert(Name::from(b"BBox"), reals(&[0.0, 0.0, w, h]));
        d.insert(Name::from(b"XStep"), real(w));
        d.insert(Name::from(b"YStep"), real(h));
        d.insert(Name::from(b"Resources"), Object::Dict(cell.resources()));
        let pm = m
            .pre_concat(pat.transform())
            .pre_concat(Transform::from_translate(rect.x(), rect.y()));
        d.insert(Name::from(b"Matrix"), matrix_obj(pm));
        Ok(Some(self.table.push(SvgObject::Stream {
            dict: d,
            data: cell.out,
        })?))
    }
}

/// The whole periods `[k0, k1]` of gradient parameter `t` that cover
/// `bbox`, in gradient space via `inv`; `None` when unbounded or over
/// [`MAX_PERIODS`].
fn period_range(kind: Kind, inv: Transform, bbox: usvg::Rect) -> Option<(i64, i64)> {
    let mut pts = [
        usvg::tiny_skia_path::Point::from_xy(bbox.left(), bbox.top()),
        usvg::tiny_skia_path::Point::from_xy(bbox.right(), bbox.top()),
        usvg::tiny_skia_path::Point::from_xy(bbox.left(), bbox.bottom()),
        usvg::tiny_skia_path::Point::from_xy(bbox.right(), bbox.bottom()),
    ];
    inv.map_points(&mut pts);
    let (lo, hi) = match kind {
        Kind::Linear { x1, y1, x2, y2 } => {
            let (dx, dy) = (x2 - x1, y2 - y1);
            let len2 = dx * dx + dy * dy;
            if len2 <= 0.0 {
                return None;
            }
            let ts = pts.map(|p| ((f64::from(p.x) - x1) * dx + (f64::from(p.y) - y1) * dy) / len2);
            let lo = ts.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = ts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            (lo.floor().min(0.0), hi.ceil().max(1.0))
        }
        Kind::Radial { cx, cy, r, fx, fy } => {
            // Circle t has centre f + t(c - f) and radius t·r, so it holds
            // p once t >= |p - f| / (r - |c - f|).
            let denom = r - (cx - fx).hypot(cy - fy);
            if denom <= r * 1e-3 {
                return None;
            }
            let far = pts
                .iter()
                .map(|p| (f64::from(p.x) - fx).hypot(f64::from(p.y) - fy))
                .fold(0.0, f64::max);
            (0.0, (far / denom).ceil().max(1.0))
        }
    };
    if !(lo.is_finite() && hi.is_finite()) || hi - lo > MAX_PERIODS as f64 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)] // integral and within ±2^53, checked above
    Some((lo as i64, hi as i64))
}

/// Stops `(offset, components)` as a function on `[0 1]`: one type 2
/// segment per pair of distinct offsets, constant pads before the first
/// and after the last stop.
fn stop_function(stops: &[(f64, Vec<f64>)]) -> Object {
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else {
        return interpolate(&[0.0], &[0.0]);
    };
    let mut segs: Vec<(f64, &[f64], &[f64])> = Vec::new();
    if first.0 > 0.0 {
        segs.push((first.0, &first.1, &first.1));
    }
    for w in stops.windows(2) {
        if let [a, b] = w
            && b.0 > a.0
        {
            segs.push((b.0, &a.1, &b.1));
        }
    }
    if last.0 < 1.0 || segs.is_empty() {
        segs.push((1.0, &last.1, &last.1));
    }
    if let [only] = segs.as_slice() {
        return interpolate(only.1, only.2);
    }
    let bounds: Vec<f64> = segs
        .iter()
        .take(segs.len().saturating_sub(1))
        .map(|s| s.0)
        .collect();
    let encode: Vec<f64> = segs.iter().flat_map(|_| [0.0, 1.0]).collect();
    stitching(
        [0.0, 1.0],
        segs.iter().map(|s| interpolate(s.1, s.2)).collect(),
        &bounds,
        &encode,
    )
}

/// §7.10.3 type 2 function, `N 1`.
fn interpolate(c0: &[f64], c1: &[f64]) -> Object {
    let mut d = Dict::new();
    d.insert(Name::from(b"FunctionType"), Object::Integer(2));
    d.insert(Name::from(b"Domain"), reals(&[0.0, 1.0]));
    d.insert(Name::from(b"C0"), reals(c0));
    d.insert(Name::from(b"C1"), reals(c1));
    d.insert(Name::from(b"N"), Object::Integer(1));
    Object::Dict(d)
}

/// §7.10.4 type 3 function.
fn stitching(domain: [f64; 2], functions: Vec<Object>, bounds: &[f64], encode: &[f64]) -> Object {
    let mut d = Dict::new();
    d.insert(Name::from(b"FunctionType"), Object::Integer(3));
    d.insert(Name::from(b"Domain"), reals(&domain));
    d.insert(Name::from(b"Functions"), Object::Array(functions));
    d.insert(Name::from(b"Bounds"), reals(bounds));
    d.insert(Name::from(b"Encode"), reals(encode));
    Object::Dict(d)
}

/// A shading dictionary (§8.7.4.5 Tables 78, 80, 81).
fn shading(cs: &[u8], geom: &Geom, function: Object) -> Dict {
    let mut d = Dict::new();
    d.insert(
        Name::from(b"ShadingType"),
        Object::Integer(geom.shading_type),
    );
    d.insert(Name::from(b"ColorSpace"), name(cs));
    d.insert(Name::from(b"Coords"), reals(&geom.coords));
    d.insert(Name::from(b"Domain"), reals(&geom.domain));
    d.insert(Name::from(b"Function"), function);
    d.insert(
        Name::from(b"Extend"),
        Object::Array(vec![Object::Boolean(true), Object::Boolean(true)]),
    );
    d
}

//! Smooth shadings as Gouraud triangles, for a vector writer (`Pass 700.14`).
//!
//! A writer that can fill a triangle with three vertex colours (EMF's
//! `EMR_GRADIENTFILL`, MS-EMF §2.3.5.12) can draw axial, radial and mesh
//! shadings without a bitmap. This module turns a shading into such
//! triangles **in device pixels**, reproducing [`crate::shading`]'s and
//! [`crate::mesh`]'s raster semantics:
//!
//! - **Colour.** The ramp the raster paints from ([`ColorRamp`], already
//!   through the shading's colour space into sRGB) is a step function of
//!   [`RAMP_SAMPLES`] entries. It becomes a piecewise-linear list of knots
//!   through the sample centres, with a hard step wherever two adjacent
//!   samples differ by more than [`STEP`], then merged while every dropped
//!   knot stays within [`COLOUR_TOL`] of the line. Error against the raster:
//!   at most half an adjacent-sample difference, i.e. ≤ 1/255 per channel.
//!   Knots closer than [`MIN_BAND_PX`] on the device are then merged, so a
//!   ramp that bends within a pixel is drawn straight across it.
//! - **Axial** (§8.7.4.5.3): one quad per colour band, spanning the region's
//!   projection on the perpendicular. Colour is affine across a quad, so
//!   Gouraud interpolation is exact.
//! - **Radial** (§8.7.4.5.4): one ring of quads per band. For a fixed angle
//!   the point `c(s) + r(s)·(cos θ, sin θ)` is linear in `s`, and the chords
//!   of every circle at one angle pair are parallel, so colour is affine
//!   across each quad too. Bands are emitted in increasing `s`, which is the
//!   standard's painting order (greatest `s` wins). Where the circles are
//!   not nested (`|Δc| > |Δr|`) a band can overlap itself, so its colour
//!   change is capped at [`OVERLAP_STEP`]. The angle count, sized for the
//!   largest circle drawn, bounds chord sag to [`SAG_PX`].
//! - **Extend** becomes a flat band grown until its circles cover the
//!   region (nested) or leave it for good (cone, focal outside).
//! - **Meshes** (types 4–7) use the raster's own patch subdivision; a
//!   parametric mesh (`/Function`) is split further until the ramp is
//!   linear within [`COLOUR_TOL`] across each triangle.
//!
//! Clipping (the current clip, a pattern's fill area, `/BBox`) is the
//! writer's job: the triangles cover at least the region and the writer
//! clips them. A shading this cannot express returns a [`Decline`] and the
//! writer keeps its raster.

use std::f32::consts::PI;
use std::sync::Arc;

use tiny_skia::{FillRule, Path, Transform};

mod mesh;

use crate::mesh::map;
use crate::shading::{ColorRamp, Geometry, GradientKind, GradientSpec, RAMP_SAMPLES, Shading};

/// A dropped knot may sit this far (sRGB, 0–1) from the line that replaces
/// it — half an 8-bit level. The epsilon keeps a byte-quantised linear
/// ramp, whose staircase sits exactly half a level off its line, from
/// failing the test on float noise and keeping every knot.
pub const COLOUR_TOL: f32 = 0.5 / 255.0 + 1e-4;
/// Adjacent ramp samples further apart than this are a hard step, not a
/// slope: the raster's nearest-sample lookup paints them as one.
pub const STEP: f32 = 2.0 / 255.0 + 1e-6;
/// The largest colour change allowed across one band of a radial shading
/// whose circles overlap each other (cone, focal point outside).
pub const OVERLAP_STEP: f32 = 1.0 / 255.0 + 1e-6;
/// The largest distance, in device pixels, a circle's polygon may fall
/// inside the circle.
pub const SAG_PX: f32 = 0.25;
/// The narrowest an axial or radial band may be, in device pixels, where
/// the bands crowd: knots closer are merged, and the cap on colour change
/// per overlapping radial band yields to it. GDI's `GradientFill` fails on
/// a diagonal triangle thinner than about a third of a pixel, and a band
/// that thin paints at most one row of pixels anyway.
pub const MIN_BAND_PX: f32 = 1.0;

/// A shading the export recorder painted into a raster, with what a vector
/// writer needs to draw it instead.
#[derive(Debug, Clone)]
pub struct ShadingSource {
    /// The shading.
    pub shading: Arc<Shading>,
    /// Device space → the shading's target space.
    pub to_target: Transform,
    /// The device-pixel region the raster painted, `(x0, y0, x1, y1)`.
    pub region: (i32, i32, i32, i32),
    /// For a pattern fill, the filled path in device space and its rule.
    pub area: Option<(Arc<Path>, FillRule)>,
    /// The constant alpha in force.
    pub alpha: f32,
}

impl ShadingSource {
    /// Captures the shading behind a rasterised shading image, so the
    /// EMF writer can re-emit it as triangles instead of the bitmap.
    pub(crate) fn new(
        shading: &Shading,
        to_target: Transform,
        region: (i32, i32, i32, i32),
        area: Option<(Arc<Path>, FillRule)>,
        alpha: f32,
    ) -> Self {
        Self {
            shading: Arc::new(shading.clone()),
            to_target,
            region,
            area,
            alpha,
        }
    }

    /// `/BBox` as a device-space quadrilateral, if the shading has one.
    #[must_use]
    pub fn bbox_device(&self) -> Option<[[f32; 2]; 4]> {
        let [x0, y0, x1, y1] = self.shading.bbox?;
        let to_device = self.to_target.invert()?;
        Some([[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|p| map(to_device, p)))
    }
}

/// Triangles with per-vertex colour, in device pixels.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gouraud {
    /// Vertex positions, device pixels.
    pub points: Vec<[f32; 2]>,
    /// Vertex colours, sRGB `0..=1`, matching [`Self::points`].
    pub colours: Vec<[f32; 3]>,
    /// Indices into [`Self::points`], in paint order.
    pub triangles: Vec<[u32; 3]>,
}

impl Gouraud {
    /// The number of triangles.
    #[must_use]
    pub fn len(&self) -> usize {
        self.triangles.len()
    }

    /// Whether there is nothing to paint.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    fn push(&mut self, p: [[f32; 2]; 3], c: [[f32; 3]; 3], max: usize) -> Result<(), Decline> {
        if self.triangles.len() >= max {
            return Err(Decline::TooManyTriangles);
        }
        if p.iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1.0e6)
        {
            return Err(Decline::Unpaintable);
        }
        #[allow(clippy::cast_possible_truncation)]
        let base = self.points.len() as u32;
        self.points.extend_from_slice(&p);
        self.colours.extend_from_slice(&c);
        self.triangles.push([base, base + 1, base + 2]);
        Ok(())
    }

    fn quad(
        &mut self,
        q: [[f32; 2]; 4],
        ca: [f32; 3],
        cb: [f32; 3],
        max: usize,
    ) -> Result<(), Decline> {
        // q = [a0, a1, b0, b1]: a-side at colour ca, b-side at cb.
        self.push([q[0], q[1], q[2]], [ca, ca, cb], max)?;
        self.push([q[1], q[3], q[2]], [ca, cb, cb], max)
    }
}

/// Why a shading was left as a raster.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decline {
    /// `ShadingType 1`: its colour varies in two dimensions, which no
    /// triangle mesh here is built for.
    FunctionBased,
    /// The ramp has holes (a `/Function` that failed at some sample).
    IncompleteRamp,
    /// A constant alpha below 1: the triangles would composite per
    /// triangle, not once.
    Translucent,
    /// More triangles than the writer's ceiling.
    TooManyTriangles,
    /// Degenerate or out-of-range geometry.
    Unpaintable,
}

impl Decline {
    /// A short lower-case name, for disclosure lines.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FunctionBased => "function-based",
            Self::IncompleteRamp => "incomplete-ramp",
            Self::Translucent => "translucent",
            Self::TooManyTriangles => "too-many-triangles",
            Self::Unpaintable => "unpaintable",
        }
    }
}

/// `(s, colour)`: the geometry parameter (0 at the first circle or axis
/// end, 1 at the second) and the sRGB colour there. Two knots at one `s`
/// are a hard step.
type Knot = (f32, [f32; 3]);

/// What one band of a parametric shading paints: `[sa, sb]` from colour
/// `ca` to `cb`.
type Band = (f32, f32, [f32; 3], [f32; 3]);

/// Tessellate a shading the recorder harvested as a raster.
///
/// # Errors
///
/// A [`Decline`] when the shading cannot be expressed within `max`
/// triangles or at all; the caller keeps the raster.
pub fn tessellate(src: &ShadingSource, max: usize) -> Result<Gouraud, Decline> {
    if src.alpha < 1.0 - 1e-6 {
        return Err(Decline::Translucent);
    }
    let to_device = src.to_target.invert().ok_or(Decline::Unpaintable)?;
    let sh = &*src.shading;
    if let Some(mesh) = sh.mesh.as_ref() {
        return mesh::mesh_triangles(mesh, sh.ramp.as_ref(), to_device, max);
    }
    #[allow(clippy::cast_precision_loss)]
    let region = [
        src.region.0 as f32,
        src.region.1 as f32,
        src.region.2 as f32,
        src.region.3 as f32,
    ];
    let frame = Frame::new(to_device, region).ok_or(Decline::Unpaintable)?;
    let ramp = sh.ramp.as_ref().ok_or(Decline::IncompleteRamp)?;
    match sh.geometry {
        Geometry::FunctionBased { .. } => Err(Decline::FunctionBased),
        Geometry::Mesh { .. } => Err(Decline::Unpaintable),
        Geometry::Axial { coords, extend, .. } => {
            axial(coords, extend, &ramp_knots(ramp)?, &frame, max)
        }
        Geometry::Radial { coords, extend, .. } => {
            radial(coords, extend, ramp_knots(ramp)?, &frame, max)
        }
    }
}

/// Tessellate a recorded native gradient. `to_device` maps gradient space
/// to device pixels; `region` is the device rectangle `[x0, y0, x1, y1]`
/// the triangles must cover.
///
/// # Errors
///
/// As [`tessellate`].
pub fn tessellate_gradient(
    spec: &GradientSpec,
    to_device: Transform,
    region: [f32; 4],
    max: usize,
) -> Result<Gouraud, Decline> {
    if spec.alpha < 1.0 - 1e-6 {
        return Err(Decline::Translucent);
    }
    let frame = Frame::new(to_device, region).ok_or(Decline::Unpaintable)?;
    let knots = stop_knots(&spec.stops).ok_or(Decline::IncompleteRamp)?;
    match spec.kind {
        GradientKind::Linear { x0, y0, x1, y1 } => {
            axial([x0, y0, x1, y1], spec.extend, &knots, &frame, max)
        }
        GradientKind::Radial { cx, cy, r, fx, fy } => {
            radial([fx, fy, 0.0, cx, cy, r], spec.extend, knots, &frame, max)
        }
    }
}

/// The device region and its image in target space.
struct Frame {
    to_device: Transform,
    region: [f32; 4],
    corners: [[f32; 2]; 4],
}

impl Frame {
    fn new(to_device: Transform, region: [f32; 4]) -> Option<Self> {
        let to_target = to_device.invert()?;
        let [x0, y0, x1, y1] = region;
        let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|p| map(to_target, p));
        Some(Self {
            to_device,
            region,
            corners,
        })
    }

    /// Whether a device-space quad can touch the region.
    fn touches(&self, q: &[[f32; 2]; 4]) -> bool {
        let [x0, y0, x1, y1] = self.region;
        let lo = q
            .iter()
            .fold([f32::INFINITY; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
        let hi = q.iter().fold([f32::NEG_INFINITY; 2], |a, p| {
            [a[0].max(p[0]), a[1].max(p[1])]
        });
        hi[0] >= x0 - 1.0 && lo[0] <= x1 + 1.0 && hi[1] >= y0 - 1.0 && lo[1] <= y1 + 1.0
    }

    /// A circle (target space) holding the region.
    fn bounding_circle(&self) -> ([f32; 2], f32) {
        let m = self
            .corners
            .iter()
            .fold([0.0f32; 2], |a, p| [a[0] + p[0] / 4.0, a[1] + p[1] / 4.0]);
        let rho = self
            .corners
            .iter()
            .map(|p| dist(*p, m))
            .fold(0.0f32, f32::max);
        (m, rho * 1.01 + 1e-3)
    }

    /// An upper bound on how many device pixels one target unit spans.
    fn scale(&self) -> f32 {
        let t = self.to_device;
        (t.sx * t.sx + t.ky * t.ky + t.kx * t.kx + t.sy * t.sy).sqrt()
    }

    /// A lower bound on how many device pixels one target unit spans in
    /// any direction (`|det| / ‖M‖_F` ≤ the smallest singular value).
    fn min_scale(&self) -> f32 {
        let t = self.to_device;
        let frob = self.scale();
        if frob > 0.0 {
            t.sx.mul_add(t.sy, -(t.kx * t.ky)).abs() / frob
        } else {
            0.0
        }
    }
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn lerp3([a0, a1, a2]: [f32; 3], [b0, b1, b2]: [f32; 3], f: f32) -> [f32; 3] {
    [
        f.mul_add(b0 - a0, a0),
        f.mul_add(b1 - a1, a1),
        f.mul_add(b2 - a2, a2),
    ]
}

fn max_diff(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter()
        .zip(&b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

/// The ramp's samples as knots: centres at `k / (N − 1)`, a step at the
/// half-way point between samples further apart than [`STEP`].
fn ramp_knots(ramp: &ColorRamp) -> Result<Vec<Knot>, Decline> {
    let [t0, t1] = ramp.domain();
    let last = RAMP_SAMPLES - 1;
    #[allow(clippy::cast_precision_loss)]
    let at = |k: usize| k as f32 / last as f32;
    let mut raw: Vec<Knot> = Vec::with_capacity(RAMP_SAMPLES + 8);
    let mut prev: Option<[f32; 3]> = None;
    for k in 0..RAMP_SAMPLES {
        let c = ramp
            .at(at(k).mul_add(t1 - t0, t0))
            .ok_or(Decline::IncompleteRamp)?;
        let q = [c.r, c.g, c.b].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() / 255.0);
        if let Some(p) = prev
            && max_diff(p, q) > STEP
        {
            let mid = (at(k - 1) + at(k)) / 2.0;
            raw.push((mid, p));
            raw.push((mid, q));
        }
        raw.push((at(k), q));
        prev = Some(q);
    }
    Ok(simplify(&raw))
}

/// A native gradient's stops as knots over `[0, 1]`.
fn stop_knots(stops: &[(f32, [u8; 3])]) -> Option<Vec<Knot>> {
    let col = |c: [u8; 3]| c.map(|v| f32::from(v) / 255.0);
    let (first, last) = (stops.first()?, stops.last()?);
    let mut out = Vec::with_capacity(stops.len() + 2);
    if first.0 > 0.0 {
        out.push((0.0, col(first.1)));
    }
    out.extend(stops.iter().map(|&(s, c)| (s.clamp(0.0, 1.0), col(c))));
    if last.0 < 1.0 {
        out.push((1.0, col(last.1)));
    }
    Some(out)
}

/// Greedily drop knots that lie within [`COLOUR_TOL`] of the line between
/// their kept neighbours. A zero-width step is never merged across.
fn simplify(raw: &[Knot]) -> Vec<Knot> {
    let Some(&first) = raw.first() else {
        return Vec::new();
    };
    let fits = |i: usize, k: usize| {
        let (Some(&(sa, ca)), Some(&(sb, cb)), Some(inner), Some(before)) =
            (raw.get(i), raw.get(k), raw.get(i + 1..k), raw.get(i..k))
        else {
            return false;
        };
        let span = sb - sa;
        span > 0.0
            && inner.iter().zip(before).all(|(&(s, c), prev)| {
                s > prev.0 && max_diff(c, lerp3(ca, cb, (s - sa) / span)) <= COLOUR_TOL
            })
            && before.last().is_some_and(|p| sb > p.0)
    };
    let mut out = vec![first];
    let mut i = 0;
    while i + 1 < raw.len() {
        let mut j = i + 1;
        while j + 1 < raw.len() && fits(i, j + 1) {
            j += 1;
        }
        let Some(&knot) = raw.get(j) else {
            break;
        };
        out.push(knot);
        i = j;
    }
    out
}

/// Split every band whose colour changes by more than `cap`, into no more
/// than `per_s` bands per unit of `s`.
fn split(knots: &[Knot], cap: f32, per_s: f32) -> Vec<Knot> {
    let mut out: Vec<Knot> = Vec::with_capacity(knots.len());
    for pair in knots.windows(2) {
        let &[(sa, ca), (sb, cb)] = pair else {
            continue;
        };
        out.push((sa, ca));
        if sb > sa {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let n = (max_diff(ca, cb) / cap)
                .ceil()
                .min(((sb - sa) * per_s).floor())
                .max(1.0) as usize;
            for i in 1..n {
                #[allow(clippy::cast_precision_loss)]
                let f = i as f32 / n as f32;
                out.push((f.mul_add(sb - sa, sa), lerp3(ca, cb, f)));
            }
        }
    }
    if let Some(&k) = knots.last() {
        out.push(k);
    }
    out
}

/// `knots` without the interior ones closer than `gap` (in `s`) to the last
/// kept, so no band is thinner than `gap` unless a hard step (two knots at
/// one `s`) forces it. The line between the kept knots replaces a dropped
/// one: an error bounded by the ramp's bend over one band.
fn thin(knots: &[Knot], gap: f32) -> Vec<Knot> {
    let mut out: Vec<Knot> = Vec::with_capacity(knots.len());
    let last = knots.len().saturating_sub(1);
    for (k, &(s, c)) in knots.iter().enumerate() {
        let step = |j: usize| knots.get(j).is_some_and(|n| n.0 == s);
        let prev = out.last().map(|p| p.0);
        let keep =
            k == 0 || k == last || step(k + 1) || prev.is_some_and(|p| p == s || s - p >= gap);
        // The end is fixed: drop the knot before it instead, if thin and
        // not half of a step.
        if k == last
            && let [.., a, b] = out.as_slice()
            && s - b.0 < gap
            && a.0 != b.0
        {
            out.pop();
        }
        if keep {
            out.push((s, c));
        }
    }
    out
}

/// The colour bands between consecutive knots, clipped to `[lo, hi]`.
fn bands_within(knots: &[Knot], lo: f32, hi: f32) -> Vec<Band> {
    knots
        .windows(2)
        .filter_map(|w| {
            let &[(sa, ca), (sb, cb)] = w else {
                return None;
            };
            let (a, b) = (sa.max(lo), sb.min(hi));
            if b <= a || sb <= sa {
                return None;
            }
            let f = |s: f32| lerp3(ca, cb, (s - sa) / (sb - sa));
            Some((a, b, f(a), f(b)))
        })
        .collect()
}

/// §8.7.4.5.3: quads across the axis, one per band, plus the extends.
fn axial(
    coords: [f32; 4],
    extend: [bool; 2],
    knots: &[Knot],
    frame: &Frame,
    max: usize,
) -> Result<Gouraud, Decline> {
    let [x0, y0, x1, y1] = coords;
    let (dx, dy) = (x1 - x0, y1 - y0);
    let denom = dx.mul_add(dx, dy * dy);
    let mut out = Gouraud::default();
    let (Some(&(_, c0)), Some(&(_, c1))) = (knots.first(), knots.last()) else {
        return Ok(out);
    };
    if denom <= f32::EPSILON {
        return Ok(out);
    }
    let len = denom.sqrt();
    let knots = &thin(knots, MIN_BAND_PX / (len * frame.min_scale()).max(1.0));
    let perp = [-dy / len, dx / len];
    let (mut p_lo, mut p_hi, mut q_lo, mut q_hi) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for c in frame.corners {
        let (px, py) = (c[0] - x0, c[1] - y0);
        let p = dx.mul_add(px, dy * py) / denom;
        let q = perp[0].mul_add(px, perp[1] * py);
        (p_lo, p_hi, q_lo, q_hi) = (p_lo.min(p), p_hi.max(p), q_lo.min(q), q_hi.max(q));
    }
    let pad_q = (q_hi - q_lo).mul_add(0.01, 1e-3);
    let pad_p = (p_hi - p_lo).mul_add(0.01, 1e-3);
    let (q_lo, q_hi) = (q_lo - pad_q, q_hi + pad_q);
    let mut bands = Vec::new();
    if extend[0] && p_lo < 0.0 {
        bands.push((p_lo - pad_p, 0.0, c0, c0));
    }
    bands.extend(bands_within(knots, p_lo, p_hi));
    if extend[1] && p_hi > 1.0 {
        bands.push((1.0, p_hi + pad_p, c1, c1));
    }
    let at = |s: f32, q: f32| {
        let p = [x0 + s * dx + q * perp[0], y0 + s * dy + q * perp[1]];
        map(frame.to_device, p)
    };
    for (sa, sb, ca, cb) in bands {
        out.quad(
            [at(sa, q_lo), at(sa, q_hi), at(sb, q_lo), at(sb, q_hi)],
            ca,
            cb,
            max,
        )?;
    }
    Ok(out)
}

/// The circle family of a radial shading.
#[derive(Clone, Copy)]
struct Circles([f32; 6]);

impl Circles {
    fn centre(self, s: f32) -> [f32; 2] {
        let [x0, y0, _, x1, y1, _] = self.0;
        [s.mul_add(x1 - x0, x0), s.mul_add(y1 - y0, y0)]
    }

    fn radius(self, s: f32) -> f32 {
        let [_, _, r0, _, _, r1] = self.0;
        s.mul_add(r1 - r0, r0)
    }

    /// The least a point at any fixed angle moves per unit `s`,
    /// `||Δc| − |Δr||` in target units: where the circles crowd.
    fn least_step(self) -> f32 {
        let [x0, y0, r0, x1, y1, r1] = self.0;
        ((x1 - x0).hypot(y1 - y0) - (r1 - r0).abs()).abs()
    }

    /// `|Δc|² − Δr²`: negative when the circles nest.
    fn spread(self) -> f32 {
        let [x0, y0, r0, x1, y1, r1] = self.0;
        let (dx, dy, dr) = (x1 - x0, y1 - y0, r1 - r0);
        dx.mul_add(dx, dy * dy) - dr * dr
    }

    /// How far past `base` (in direction `dir`) an Extend band must reach:
    /// until a nested family covers the region, a cone has left it for
    /// good, or the radius reaches zero. `None` if no such point is found.
    fn extend_to(self, base: f32, dir: f32, frame: &Frame, cos_n: f32) -> Option<f32> {
        let [_, _, r0, _, _, r1] = self.0;
        let dr = r1 - r0;
        let limit = (dr.abs() > f32::EPSILON)
            .then(|| -r0 / dr)
            .filter(|z| (z - base) * dir >= 0.0);
        let (m, rho) = frame.bounding_circle();
        let nested = self.spread() < 0.0;
        let (mut step, mut prev) = (1.0f32, None::<f32>);
        for _ in 0..40 {
            let s = dir.mul_add(step, base);
            if let Some(z) = limit
                && (s - z) * dir >= 0.0
            {
                return Some(z);
            }
            let (d, r) = (dist(self.centre(s), m), self.radius(s));
            if nested && r * cos_n >= d + rho {
                return Some(s);
            }
            let g = d - rho - r;
            if !nested && g > 0.0 && prev.is_some_and(|p| g > p) {
                return Some(s);
            }
            prev = Some(g);
            step *= 2.0;
        }
        None
    }
}

/// How many segments keep a circle of device radius `r_dev` within `sag`
/// pixels of its polygon (`r·(1 − cos(π/n)) ≈ r·π²/(2n²)`).
fn angle_count(r_dev: f32, sag: f32) -> usize {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = (PI * (r_dev.max(0.0) / (2.0 * sag.max(1e-3))).sqrt())
        .ceil()
        .clamp(16.0, 1024.0) as usize;
    n
}

/// §8.7.4.5.4: rings of quads, one ring per band, in increasing `s`.
fn radial(
    coords: [f32; 6],
    extend: [bool; 2],
    knots: Vec<Knot>,
    frame: &Frame,
    max: usize,
) -> Result<Gouraud, Decline> {
    let circles = Circles(coords);
    let [_, _, r0, _, _, r1] = coords;
    let mut out = Gouraud::default();
    if r0 < 0.0 || r1 < 0.0 {
        return Err(Decline::Unpaintable);
    }
    if r0 <= 0.0 && r1 <= 0.0 {
        return Ok(out);
    }
    // Device pixels a point moves per unit `s` where the circles crowd: no
    // band is thinner than MIN_BAND_PX there.
    let least = (circles.least_step() * frame.min_scale()).max(1.0);
    let knots = if circles.spread() >= 0.0 {
        split(&knots, OVERLAP_STEP, least / MIN_BAND_PX)
    } else {
        knots
    };
    let knots = thin(&knots, MIN_BAND_PX / least);
    let (Some(&(_, c0)), Some(&(_, c1))) = (knots.first(), knots.last()) else {
        return Ok(out);
    };
    #[allow(clippy::cast_precision_loss)]
    let cos_n = (PI / angle_count(r0.max(r1) * frame.scale(), SAG_PX) as f32).cos();
    let mut bands = Vec::new();
    if extend[0] {
        let lo = circles
            .extend_to(0.0, -1.0, frame, cos_n)
            .ok_or(Decline::Unpaintable)?;
        if lo < 0.0 {
            bands.push((lo, 0.0, c0, c0));
        }
    }
    bands.extend(bands_within(&knots, 0.0, 1.0));
    if extend[1] {
        let hi = circles
            .extend_to(1.0, 1.0, frame, cos_n)
            .ok_or(Decline::Unpaintable)?;
        if hi > 1.0 {
            bands.push((1.0, hi, c1, c1));
        }
    }
    // Sized for the largest circle drawn, an Extend's included.
    let r_max = bands
        .iter()
        .flat_map(|b| [b.0, b.1])
        .map(|s| circles.radius(s))
        .fold(r0.max(r1), f32::max);
    let n = angle_count(r_max * frame.scale(), SAG_PX);
    #[allow(clippy::cast_precision_loss)]
    let unit: Vec<[f32; 2]> = (0..=n)
        .map(|k| {
            let th = 2.0 * PI * k as f32 / n as f32;
            [th.cos(), th.sin()]
        })
        .collect();
    let at = |s: f32, u: [f32; 2]| {
        let (c, r) = (circles.centre(s), circles.radius(s).max(0.0));
        map(
            frame.to_device,
            [r.mul_add(u[0], c[0]), r.mul_add(u[1], c[1])],
        )
    };
    for (sa, sb, ca, cb) in bands {
        for w in unit.windows(2) {
            let &[u0, u1] = w else {
                continue;
            };
            let q = [at(sa, u0), at(sa, u1), at(sb, u0), at(sb, u1)];
            if frame.touches(&q) {
                out.quad(q, ca, cb, max)?;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn simplify_keeps_a_straight_ramp_as_two_knots() {
        let raw: Vec<Knot> = (0..=10)
            .map(|k| (k as f32 / 10.0, [k as f32 / 10.0; 3]))
            .collect();
        let out = simplify(&raw);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].0, 1.0);
    }

    #[test]
    fn simplify_never_merges_across_a_step() {
        let raw = vec![
            (0.0, [0.0; 3]),
            (0.5, [0.0; 3]),
            (0.5, [1.0; 3]),
            (1.0, [1.0; 3]),
        ];
        let out = simplify(&raw);
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn split_caps_each_band() {
        let out = split(
            &[(0.0, [0.0; 3]), (1.0, [10.0 / 255.0; 3])],
            OVERLAP_STEP,
            f32::INFINITY,
        );
        assert_eq!(out.len(), 11);
        for w in out.windows(2) {
            assert!(max_diff(w[0].1, w[1].1) <= OVERLAP_STEP);
        }
    }

    #[test]
    fn split_keeps_bands_at_least_a_pixel_wide() {
        let out = split(
            &[(0.0, [0.0; 3]), (1.0, [10.0 / 255.0; 3])],
            OVERLAP_STEP,
            4.0,
        );
        assert_eq!(out.len(), 5);
    }

    #[test]
    fn thin_merges_crowded_knots_and_keeps_steps_and_ends() {
        let k = |s: f32| (s, [s; 3]);
        let knots = [
            k(0.0),
            k(0.05),
            k(0.2),
            k(0.5),
            k(0.5),
            k(0.55),
            k(0.95),
            k(1.0),
        ];
        let s: Vec<f32> = thin(&knots, 0.1).iter().map(|n| n.0).collect();
        // 0.05 and 0.55 are too close to the knot before; 0.95 to the end.
        assert_eq!(s, [0.0, 0.2, 0.5, 0.5, 1.0]);
    }

    #[test]
    fn a_degenerate_axis_paints_nothing() {
        let frame = Frame::new(Transform::identity(), [0.0, 0.0, 10.0, 10.0]).unwrap();
        let knots = vec![(0.0, [0.0; 3]), (1.0, [1.0; 3])];
        let out = axial([5.0, 5.0, 5.0, 5.0], [true; 2], &knots, &frame, 100).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn the_ceiling_declines() {
        let frame = Frame::new(Transform::identity(), [0.0, 0.0, 100.0, 100.0]).unwrap();
        let knots = vec![(0.0, [0.0; 3]), (1.0, [1.0; 3])];
        let got = radial(
            [50.0, 50.0, 0.0, 50.0, 50.0, 40.0],
            [false; 2],
            knots,
            &frame,
            3,
        );
        assert_eq!(got, Err(Decline::TooManyTriangles));
    }
}

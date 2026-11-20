//! Illuminance over every drawable triangle of a model, sampled on a
//! lattice no coarser than a stated spacing: [`LightMeter::map`], for a
//! view coloured by the light falling on it.

use super::{IlluminanceError, IlluminanceNote, IlluminanceUnit, LightMeter, RelativeReason};
use super::{Sample, for_chunks};
use crate::render::{at3, corners, stored_normals};
use crate::vec3::{add, dot, length, normalize, scale, span, sub, unit_cross};
use crate::{Camera, Projection, TriangleMesh};
use std::collections::HashMap;

mod gpu;
pub use gpu::GpuLightRead;

pub use gpu::GpuLightMap;

/// The most distinct points [`LightMeter::map`] measures.
pub const MAX_MAP_POINTS: usize = 4_000_000;
/// The most rays [`LightMeter::map`] casts.
pub const MAX_MAP_RAYS: usize = 2_000_000_000;
/// Floats per vertex of [`IlluminanceMap::vertex_buffer`]: x, y, z, figure.
pub const MAP_VERTEX_FLOATS: usize = 4;
/// The most parts a triangle's edges are cut into.
const MAX_DIVISIONS: usize = 1024;
/// Points measured between two progress reports.
const CHUNK: usize = 4096;

/// Which side of each triangle [`LightMeter::map`] measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MapSide {
    /// The side the meter's camera sees, as a reading and the picture
    /// take it: right for an open sheet wound either way, for that view.
    #[default]
    FacingCamera,
    /// The side the stored normals (else the winding) face, whatever the
    /// view: right for a closed solid wound outward, from any view.
    AlongNormals,
}

/// How far a [`LightMeter::map`] has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct MapProgress {
    /// Points measured so far.
    pub done: usize,
    /// Points to measure.
    pub total: usize,
}

/// One triangle's figures: its edges cut into `d` = [`Self::divisions`]
/// equal parts, a figure at every lattice point `(i, j)`, `i + j <= d`,
/// at `corners[0] + (i/d)(corners[1] - corners[0]) + (j/d)(corners[2] - corners[0])`.
/// The figures run row by row, row `j` holding `d + 1 - j` points
/// ([`Self::index`]).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MapPatch {
    /// The triangle's corners, in model coordinates, in its own order.
    pub corners: [[f64; 3]; 3],
    /// Parts each edge is cut into, at least 1 (the corners alone).
    pub divisions: usize,
    /// The lattice's figures.
    pub figures: Vec<f64>,
}

impl MapPatch {
    /// Lattice point `(i, j)`'s index into [`Self::figures`].
    #[must_use]
    pub fn index(&self, i: usize, j: usize) -> usize {
        lattice_index(self.divisions, i, j)
    }

    /// The patch's small triangles, each as its corners and their figures,
    /// wound as the triangle is.
    pub fn triangles(&self) -> impl Iterator<Item = ([[f64; 3]; 3], [f64; 3])> + '_ {
        let d = self.divisions;
        let point = move |(i, j): (usize, usize)| -> ([f64; 3], f64) {
            let p = lattice_point(self.corners, d, i, j);
            (
                p,
                self.figures.get(self.index(i, j)).copied().unwrap_or(0.0),
            )
        };
        sub_triangles(d).map(move |t| {
            let [p, q, r] = t.map(point);
            ([p.0, q.0, r.0], [p.1, q.1, r.1])
        })
    }
}

fn lattice_index(d: usize, i: usize, j: usize) -> usize {
    j * (d + 1) - j * j.saturating_sub(1) / 2 + i
}

fn lattice_point([a, b, c]: [[f64; 3]; 3], d: usize, i: usize, j: usize) -> [f64; 3] {
    let (u, v) = (i as f64 / d as f64, j as f64 / d as f64);
    add(add(a, scale(span(a, b), u)), scale(span(a, c), v))
}

/// The lattice's small triangles as `(i, j)` corners: per cell one
/// pointing as the triangle does, and one pointing back between two.
fn sub_triangles(d: usize) -> impl Iterator<Item = [(usize, usize); 3]> {
    (0..d).flat_map(move |j| {
        (0..d - j).flat_map(move |i| {
            let up = [(i, j), (i + 1, j), (i, j + 1)];
            let down = [(i + 1, j), (i + 1, j + 1), (i, j + 1)];
            std::iter::once(up).chain((i + j + 1 < d).then_some(down))
        })
    })
}

/// The illuminance over every drawable triangle of a model
/// ([`LightMeter::map`]). Each figure is the
/// [`Illuminance::total`](super::Illuminance::total) a point reading there
/// gives: direct, bounced, ambient and emitted, in [`Self::unit`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct IlluminanceMap {
    /// What every figure is in.
    pub unit: IlluminanceUnit,
    /// Why the figures are relative; empty for lux.
    pub reasons: Vec<RelativeReason>,
    /// What the meter assumed.
    pub notes: Vec<IlluminanceNote>,
    /// By mesh, then by triangle; `None` for a triangle that is not drawn
    /// (a missing or non-finite corner, no area, or a mesh drawn fully
    /// transparent).
    pub meshes: Vec<Vec<Option<MapPatch>>>,
}

impl IlluminanceMap {
    /// Triangle `triangle` of mesh `mesh`'s patch.
    #[must_use]
    pub fn patch(&self, mesh: usize, triangle: usize) -> Option<&MapPatch> {
        self.meshes.get(mesh)?.get(triangle)?.as_ref()
    }

    /// The lowest and highest figure; `None` for a map with none.
    #[must_use]
    pub fn range(&self) -> Option<(f64, f64)> {
        let patches = self.meshes.iter().flatten().flatten();
        let mut all = patches.flat_map(|p| p.figures.iter().copied());
        let first = all.next()?;
        Some(all.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v))))
    }

    /// Every small triangle of every patch, mesh by mesh.
    pub fn triangles(&self) -> impl Iterator<Item = ([[f64; 3]; 3], [f64; 3])> + '_ {
        self.meshes
            .iter()
            .flatten()
            .flatten()
            .flat_map(MapPatch::triangles)
    }

    /// The map as a triangle list for the graphics card:
    /// [`MAP_VERTEX_FLOATS`] floats per vertex (x, y, z in model
    /// coordinates, then the figure), three vertices per triangle, for
    /// [`GLSL_ES_300_LIGHT_MAP_VERTEX`](crate::GLSL_ES_300_LIGHT_MAP_VERTEX).
    #[must_use]
    pub fn vertex_buffer(&self) -> Vec<f32> {
        let mut out = Vec::new();
        for (points, figures) in self.triangles() {
            for (p, f) in points.iter().zip(figures) {
                out.extend(p.iter().map(|&c| c as f32));
                out.push(f as f32);
            }
        }
        out
    }
}

/// Triangle `t` of `mesh`'s corners and the normal each is lit along, as
/// the picture draws it; `None` when it is not drawn.
fn lit_corners(mesh: &TriangleMesh, t: usize) -> Option<[([f64; 3], [f64; 3]); 3]> {
    let world = corners(mesh, mesh.triangles.get(t)?)?;
    let face = unit_cross(
        span(at3(&world, 0), at3(&world, 1)),
        span(at3(&world, 0), at3(&world, 2)),
    )?;
    let stored = stored_normals(mesh, t);
    Some([0, 1, 2].map(|k| {
        let n = stored.and_then(|n| n.get(k).copied().flatten());
        (at3(&world, k), n.unwrap_or(face))
    }))
}

/// `lit` turned to the side of the triangle `camera` sees.
fn facing(lit: [([f64; 3], [f64; 3]); 3], camera: &Camera) -> [([f64; 3], [f64; 3]); 3] {
    let [(a, _), (b, _), (c, _)] = lit;
    let Some(face) = unit_cross(span(a, b), span(a, c)) else {
        return lit;
    };
    let towards = match camera.projection {
        Projection::Orthographic { .. } => sub(camera.eye, camera.target),
        _ => sub(camera.eye, scale(add(add(a, b), c), 1.0 / 3.0)),
    };
    match dot(face, towards) < 0.0 {
        true => lit.map(|(p, n)| (p, scale(n, -1.0))),
        false => lit,
    }
}

/// Parts to cut a triangle's edges into so none is longer than `spacing`.
fn divisions(world: [[f64; 3]; 3], spacing: Option<f64>) -> usize {
    let Some(s) = spacing.filter(|s| s.is_finite() && *s > 0.0) else {
        return 1;
    };
    let longest = [(0, 1), (1, 2), (2, 0)]
        .map(|(a, b)| length(span(at3(&world, a), at3(&world, b))))
        .into_iter()
        .fold(0.0f64, f64::max);
    ((longest / s).ceil() as usize).clamp(1, MAX_DIVISIONS)
}

/// A measured point: mesh, position and normal, quantised so a point
/// shared by triangles lit along the same normal is measured once.
type Key = (usize, [i64; 3], [i64; 3]);

fn key(mesh: usize, p: [f64; 3], n: [f64; 3]) -> Key {
    (
        mesh,
        p.map(|c| (c * 1e9).round() as i64),
        n.map(|c| (c * 1e6).round() as i64),
    )
}

/// A laid-out patch, its figures still to come, and where each lattice
/// point's figure is among the measured points.
type Laid = (MapPatch, Vec<usize>);

/// Every mesh's triangles, each laid out or left out.
type Layout = Vec<Vec<Option<Laid>>>;

/// Each laid patch with its figures taken from the measured `figures`.
fn fill_patches(laid: Layout, figures: &[f64]) -> Vec<Vec<Option<MapPatch>>> {
    let fill = |(mut patch, slots): Laid| {
        let at = |i: &usize| figures.get(*i).copied().unwrap_or(0.0);
        patch.figures = slots.iter().map(at).collect();
        patch
    };
    laid.into_iter()
        .map(|tris| tris.into_iter().map(|t| t.map(fill)).collect())
        .collect()
}

/// The points to measure.
#[derive(Default)]
struct Plan {
    index: HashMap<Key, usize>,
    points: Vec<([f64; 3], [f64; 3])>,
}

impl Plan {
    /// Lay `lit`'s lattice of `d` parts, refusing past the cap.
    fn lay(
        &mut self,
        mesh: usize,
        lit: [([f64; 3], [f64; 3]); 3],
        d: usize,
    ) -> Result<Laid, IlluminanceError> {
        let corners = lit.map(|(p, _)| p);
        let [(_, na), (_, nb), (_, nc)] = lit;
        let mut slots = Vec::with_capacity((d + 1) * (d + 2) / 2);
        for j in 0..=d {
            for i in 0..=d - j {
                let (u, v) = (i as f64 / d as f64, j as f64 / d as f64);
                let n = add(add(scale(na, 1.0 - u - v), scale(nb, u)), scale(nc, v));
                let n = normalize(n).unwrap_or(na);
                let p = lattice_point(corners, d, i, j);
                let next = self.points.len();
                let slot = *self.index.entry(key(mesh, p, n)).or_insert(next);
                if slot == next {
                    self.points.push((p, n));
                }
                slots.push(slot);
            }
            if self.points.len() > MAX_MAP_POINTS {
                return Err(IlluminanceError::TooManySamples {
                    asked: self.points.len(),
                    limit: MAX_MAP_POINTS,
                });
            }
        }
        let patch = MapPatch {
            corners,
            divisions: d,
            figures: Vec::new(),
        };
        Ok((patch, slots))
    }
}

impl LightMeter<'_> {
    /// The illuminance over every drawable triangle of the model, each
    /// figure as [`Self::read`] gives it for a point there, lit along the
    /// normal the picture shades with: the stored normals blended across
    /// the triangle, else the triangle's own, on the [`MapSide`] asked. Each triangle's edges are
    /// cut so no part is longer than `spacing` model units (`None`: its
    /// corners alone), and a view blends between the points. A point
    /// shared by triangles lit along the same normal is measured once.
    /// Meshes drawn fully transparent are left out.
    ///
    /// `progress` is called once before the first point and after every
    /// few thousand with how far the map has got; returning `false` stops
    /// the work.
    ///
    /// # Errors
    ///
    /// [`IlluminanceError::Cancelled`] when `progress` returned `false`;
    /// [`IlluminanceError::TooManySamples`] past [`MAX_MAP_POINTS`] (ask
    /// for a coarser `spacing`) and [`IlluminanceError::TooMuchWork`] past
    /// [`MAX_MAP_RAYS`] (fewer
    /// [`MeterSettings::gather_rays`](super::MeterSettings::gather_rays)).
    pub fn map(
        &self,
        spacing: Option<f64>,
        side: MapSide,
        progress: &mut dyn FnMut(MapProgress) -> bool,
    ) -> Result<IlluminanceMap, IlluminanceError> {
        let (plan, laid) = self.lay_out(spacing, side)?;
        let figures = self.measure_all(&plan.points, progress)?;
        Ok(self.filled(laid, &figures))
    }

    /// The points [`Self::map`] measures and every triangle's patch,
    /// refusing past the caps.
    fn lay_out(
        &self,
        spacing: Option<f64>,
        side: MapSide,
    ) -> Result<(Plan, Layout), IlluminanceError> {
        let mut plan = Plan::default();
        let mut laid: Layout = Vec::with_capacity(self.meshes.len());
        for (m, mesh) in self.meshes.iter().enumerate() {
            let hidden = self.hidden.get(m).copied().unwrap_or(false);
            let mut tris = Vec::with_capacity(mesh.triangles.len());
            for t in 0..mesh.triangles.len() {
                tris.push(match lit_corners(mesh, t).filter(|_| !hidden) {
                    Some(lit) => {
                        let lit = match side {
                            MapSide::FacingCamera => facing(lit, &self.camera),
                            MapSide::AlongNormals => lit,
                        };
                        let d = divisions(lit.map(|(p, _)| p), spacing);
                        Some(plan.lay(m, lit, d)?)
                    }
                    None => None,
                });
            }
            laid.push(tris);
        }
        let rays = plan.points.len().saturating_mul(self.rays_per_sample());
        if rays > MAX_MAP_RAYS {
            return Err(IlluminanceError::TooMuchWork {
                rays,
                limit: MAX_MAP_RAYS,
            });
        }
        Ok((plan, laid))
    }

    /// The map with each patch's figures taken from the measured `figures`.
    fn filled(&self, laid: Layout, figures: &[f64]) -> IlluminanceMap {
        IlluminanceMap {
            unit: self.unit,
            reasons: self.reasons.clone(),
            notes: self.notes.clone(),
            meshes: fill_patches(laid, figures),
        }
    }

    /// Each point's total, chunk by chunk, reporting between chunks.
    fn measure_all(
        &self,
        points: &[([f64; 3], [f64; 3])],
        progress: &mut dyn FnMut(MapProgress) -> bool,
    ) -> Result<Vec<f64>, IlluminanceError> {
        let total = points.len();
        let mut out = Vec::with_capacity(total);
        let report = |done| MapProgress { done, total };
        if !progress(report(0)) {
            return Err(IlluminanceError::Cancelled);
        }
        for (c, chunk) in points.chunks(CHUNK).enumerate() {
            let mut samples = vec![Sample::default(); chunk.len()];
            for_chunks(&mut samples, 0, self.threads, &|i, slot: &mut Sample| {
                if let Some(&(p, n)) = chunk.get(i) {
                    *slot = self.measure(p, n);
                }
            });
            let totals = samples
                .iter()
                .map(|s| self.mean(std::slice::from_ref(s), 0).total);
            out.extend(totals);
            if !progress(report((c * CHUNK + chunk.len()).min(total))) {
                return Err(IlluminanceError::Cancelled);
            }
        }
        Ok(out)
    }
}

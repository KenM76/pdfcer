//! Cast shadows from one directional light: which light casts, the
//! orthographic light view fitted to the model, and the CPU shadow map.
//! [`crate::FrameShadow`] publishes the same numbers to a GPU, so both paths
//! test the same texels with the same arithmetic. Design:
//! `docs/research/3d-shadows-and-occlusion-design.md`.

use crate::TriangleMesh;
use crate::lighting::Shader;
use crate::raster::{Paint, at};
use crate::render::{Bounds, at3, corners};
use crate::vec3::{cross, dot, normalize, sub};

/// [`crate::RenderOptions::shadow_map_size`]'s default, texels per side.
pub const DEFAULT_SHADOW_MAP_SIZE: u32 = 2048;
/// The largest shadow map side used; a larger request is clamped to it.
pub const MAX_SHADOW_MAP_SIZE: u32 = 8192;
/// The smallest shadow map side used.
const MIN_SHADOW_MAP_SIZE: u32 = 64;

/// The share of the shade kept in the shadow of the shadow key (below).
pub(crate) const SHADOW_DIM: f64 = 0.5;
/// The shadow key: towards the light, camera frame (x right, y up, z into
/// the scene). Above, left of and in front of the model.
const SHADOW_KEY: [f64; 3] = [-0.5, 1.0, -0.6];
/// The normal offset, in texels.
const NORMAL_OFFSET_TEXELS: f64 = 1.5;
/// The depth bias, in texels' worth of depth.
const BIAS_TEXELS: f64 = 2.0;

/// Which way a lighting scheme's direction vector points, for shadows only:
/// two-sided lighting is the same either way. ISO 32000-1 §13.6.4.5 does not
/// say (spec RAG `iso32000__s__13.6.md` LT-N1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum LightSense {
    /// From the surface towards the light, as OpenGL's directional light
    /// position. Table 310's vectors then form a key, fill and back light.
    /// The default.
    #[default]
    TowardsLight,
    /// The direction the light travels ("emitted along", LT-1).
    Emitted,
}

/// How the casting light's shadow changes a corner's shade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CasterKind {
    /// Light `i` of the scheme: its term is removed where shadowed.
    Light(usize),
    /// The shadow key: the whole shade is scaled by [`SHADOW_DIM`].
    Dim,
}

/// The light casting shadows this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Caster {
    /// Unit, model space, towards the light.
    pub(crate) to_light: [f64; 3],
    pub(crate) kind: CasterKind,
}

impl Caster {
    /// The scheme's brightest light on the viewer's side, else its brightest,
    /// not counting lights at the camera; the shadow key when every light is
    /// at the camera; `None` unlit. `basis` is the camera's right, up and
    /// forward.
    pub(crate) fn choose(
        shader: &Shader,
        sense: LightSense,
        basis: &[[f64; 3]; 3],
    ) -> Option<Caster> {
        let [right, up, forward] = basis;
        let lights: &[(Option<[f64; 3]>, [f64; 3])] = match shader {
            Shader::Unlit => return None,
            Shader::Headlight => &[],
            Shader::Set { lights, .. } => lights,
        };
        let brightness = |c: &[f64; 3]| c.iter().sum::<f64>();
        let candidates = lights.iter().enumerate().filter_map(|(i, (dir, colour))| {
            let d = (*dir)?;
            let to_light = match sense {
                LightSense::Emitted => d.map(|c| -c),
                _ => d,
            };
            Some((i, to_light, brightness(colour)))
        });
        let best = |front: bool| {
            candidates
                .clone()
                .filter(|(_, l, _)| !front || dot(*l, *forward) < 0.0)
                .fold(None, |acc: Option<(usize, [f64; 3], f64)>, c| match acc {
                    Some(a) if a.2 >= c.2 => Some(a),
                    _ => Some(c),
                })
        };
        if let Some((i, to_light, _)) = best(true).or_else(|| best(false)) {
            return Some(Caster {
                to_light,
                kind: CasterKind::Light(i),
            });
        }
        let [x, y, z] = SHADOW_KEY;
        let key = [0, 1, 2].map(|i| x * at(*right, i) + y * at(*up, i) + z * at(*forward, i));
        Some(Caster {
            to_light: normalize(key)?,
            kind: CasterKind::Dim,
        })
    }

    /// `shader`'s shade at a corner split into what the caster's shadow
    /// leaves and what it removes: the lit shade is `rest + key`, the
    /// shadowed one `rest`.
    pub(crate) fn split(
        &self,
        shader: &Shader,
        normal: [f64; 3],
        towards: Option<[f64; 3]>,
    ) -> ([f64; 3], [f64; 3]) {
        let total = shader.shade(normal, towards);
        let key = match (self.kind, shader) {
            (CasterKind::Dim, _) => total.map(|c| c * (1.0 - SHADOW_DIM)),
            (CasterKind::Light(i), Shader::Set { lights, .. }) => match lights.get(i) {
                Some((Some(d), colour)) => {
                    let k = dot(normal, *d).abs();
                    colour.map(|c| c * k)
                }
                _ => [0.0; 3],
            },
            _ => [0.0; 3],
        };
        ([0, 1, 2].map(|i| at(total, i) - at(key, i)), key)
    }
}

/// The orthographic view from the casting light over the model's bounding
/// sphere, `size` texels square.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LightView {
    centre: [f64; 3],
    right: [f64; 3],
    up: [f64; 3],
    to_light: [f64; 3],
    reach: f64,
    pub(crate) size: u32,
}

impl LightView {
    /// `None` for a model with no extent.
    pub(crate) fn new(to_light: [f64; 3], bounds: &Bounds, size: u32) -> Option<LightView> {
        let reach = bounds.radius() * 1.01;
        if !(reach.is_finite() && reach > 0.0) {
            return None;
        }
        let helper = if to_light[2].abs() < 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let right = normalize(cross(helper, to_light))?;
        Some(LightView {
            centre: bounds.centre(),
            right,
            up: cross(to_light, right),
            to_light,
            reach,
            size: size.clamp(MIN_SHADOW_MAP_SIZE, MAX_SHADOW_MAP_SIZE),
        })
    }

    /// The light's clip coordinates of model point `p`: x, y in −1..1 across
    /// the map, z −1 nearest the light.
    pub(crate) fn clip(&self, p: [f64; 3]) -> [f64; 3] {
        let d = sub(p, self.centre);
        [
            dot(d, self.right) / self.reach,
            dot(d, self.up) / self.reach,
            -dot(d, self.to_light) / self.reach,
        ]
    }

    /// [`LightView::clip`] as a column-major matrix.
    pub(crate) fn matrix(&self) -> [f32; 16] {
        let mut m = [0.0f32; 16];
        let rows = [self.right, self.up, self.to_light.map(|c| -c)];
        for (r, axis) in rows.into_iter().enumerate() {
            for (c, v) in axis.into_iter().enumerate() {
                if let Some(slot) = m.get_mut(c * 4 + r) {
                    *slot = (v / self.reach) as f32;
                }
            }
            if let Some(slot) = m.get_mut(12 + r) {
                *slot = (-dot(axis, self.centre) / self.reach) as f32;
            }
        }
        if let Some(slot) = m.get_mut(15) {
            *slot = 1.0;
        }
        m
    }

    /// How far a point moves along its normal before the lookup, model units.
    pub(crate) fn normal_offset(&self) -> f64 {
        NORMAL_OFFSET_TEXELS * 2.0 * self.reach / f64::from(self.size)
    }

    /// Subtracted from a point's depth (0–1) before comparing.
    pub(crate) fn bias(&self) -> f64 {
        BIAS_TEXELS / f64::from(self.size)
    }

    /// Texel x, texel y (row 0 at the bottom) and depth 0–1 of model point
    /// `p` moved by the normal offset along `normal` turned to the light.
    pub(crate) fn lookup_point(&self, p: [f64; 3], normal: [f64; 3]) -> [f64; 3] {
        let side = if dot(normal, self.to_light) < 0.0 {
            -1.0
        } else {
            1.0
        };
        let k = side * self.normal_offset();
        let moved = [0, 1, 2].map(|i| at(p, i) + at(normal, i) * k);
        let [x, y, z] = self.clip(moved);
        let s = f64::from(self.size);
        [(x + 1.0) / 2.0 * s, (y + 1.0) / 2.0 * s, (z + 1.0) / 2.0]
    }
}

/// The CPU shadow map: depth 0–1 per texel, row 0 at the bottom, 1.0 where
/// nothing is.
pub(crate) struct ShadowMap {
    pub(crate) view: LightView,
    depth: Vec<f32>,
}

impl ShadowMap {
    /// Every opaque mesh's triangles drawn from the light, in row bands on
    /// `workers` threads. A texel keeps its nearest depth, so the map is the
    /// same at any count.
    pub(crate) fn build(
        view: LightView,
        meshes: &[TriangleMesh],
        paints: &[Paint<'_>],
        opaque: &[bool],
        workers: usize,
    ) -> ShadowMap {
        let s = f64::from(view.size);
        let mut tris = Vec::new();
        for (i, mesh) in meshes.iter().enumerate() {
            let casts = opaque.get(i).copied().unwrap_or(false)
                && paints.get(i).is_some_and(|p| p.colour[3] != 0);
            if casts {
                tris.extend(mesh.triangles.iter().filter_map(|tri| {
                    let world = corners(mesh, tri)?;
                    Some([0, 1, 2].map(|k| {
                        let [x, y, z] = view.clip(at3(&world, k));
                        [(x + 1.0) / 2.0 * s, (y + 1.0) / 2.0 * s, (z + 1.0) / 2.0]
                    }))
                }));
            }
        }
        let side = view.size as usize;
        let mut depth = vec![1.0f32; side * side];
        let workers = workers.max(1);
        let rows = side.div_ceil(workers * 4).max(1);
        let mut jobs: Vec<Vec<(usize, &mut [f32])>> = (0..workers).map(|_| Vec::new()).collect();
        for (k, band) in depth.chunks_mut(rows * side).enumerate() {
            if let Some(job) = jobs.get_mut(k % workers) {
                job.push((k * rows, band));
            }
        }
        crate::parallel::run_each(jobs, &|job| {
            for (top, band) in job {
                for &t in &tris {
                    fill(band, side, top, t);
                }
            }
        });
        ShadowMap { view, depth }
    }

    /// The lit share (0–1) of a [`LightView::lookup_point`]: 3 × 3 texels
    /// around it, each lit when the point is no deeper than it plus the
    /// bias; a texel off the map is lit.
    pub(crate) fn visibility(&self, at: [f64; 3]) -> f64 {
        let side = i64::from(self.view.size);
        let (cx, cy) = (at[0].floor() as i64, at[1].floor() as i64);
        let depth = at[2] - self.view.bias();
        let mut lit = 0u32;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (x, y) = (cx + dx, cy + dy);
                let stored = (x >= 0 && y >= 0 && x < side && y < side)
                    .then(|| self.depth.get((y * side + x) as usize).copied())
                    .flatten();
                if stored.is_none_or(|d| depth <= f64::from(d)) {
                    lit += 1;
                }
            }
        }
        f64::from(lit) / 9.0
    }
}

/// Keeps the nearest depth of map triangle `t` at each texel centre inside
/// it, within `band`: the rows from `top` of a map `side` texels wide.
fn fill(band: &mut [f32], side: usize, top: usize, t: [[f64; 3]; 3]) {
    let [a, b, c] = t;
    let area = edge(a, b, c);
    if !area.is_finite() || area.abs() < 1e-12 || side == 0 {
        return;
    }
    let lo = |i: usize| t.iter().map(|p| at(*p, i)).fold(f64::INFINITY, f64::min);
    let hi = |i: usize| {
        t.iter()
            .map(|p| at(*p, i))
            .fold(f64::NEG_INFINITY, f64::max)
    };
    let bottom = (top + band.len() / side) as f64 - 1.0;
    let (x0, x1) = (lo(0).floor().max(0.0), hi(0).ceil().min(side as f64 - 1.0));
    let (y0, y1) = (lo(1).floor().max(top as f64), hi(1).ceil().min(bottom));
    if !(x0 <= x1 && y0 <= y1) {
        return;
    }
    for y in y0 as usize..=y1 as usize {
        for x in x0 as usize..=x1 as usize {
            let p = [x as f64 + 0.5, y as f64 + 0.5, 0.0];
            let w = [edge(b, c, p), edge(c, a, p), edge(a, b, p)].map(|e| e / area);
            if w.iter().any(|&k| k < 0.0) {
                continue;
            }
            let z = (w[0] * a[2] + w[1] * b[2] + w[2] * c[2]) as f32;
            if let Some(d) = band.get_mut((y - top) * side + x)
                && z < *d
            {
                *d = z;
            }
        }
    }
}

/// Twice the signed area of (a, b, p) in the xy plane.
fn edge(a: [f64; 3], b: [f64; 3], p: [f64; 3]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::LightingScheme;

    const BASIS: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    fn caster(scheme: LightingScheme, sense: LightSense) -> Option<Caster> {
        let shader =
            crate::lighting::Shader::new(scheme.lights(), crate::LightFrame::Camera, &BASIS);
        Caster::choose(&shader, sense, &BASIS)
    }

    #[test]
    fn the_key_light_casts() {
        // Day and White: light 2, up and to the right on the viewer's side.
        for scheme in [LightingScheme::Day, LightingScheme::White] {
            let c = caster(scheme, LightSense::TowardsLight).unwrap();
            assert_eq!(c.kind, CasterKind::Light(1), "{scheme:?}");
            assert!(c.to_light[1] > 0.0 && c.to_light[2] < 0.0);
        }
        // CAD: its camera light never casts; its brighter front light does.
        let cad = caster(LightingScheme::Cad, LightSense::TowardsLight).unwrap();
        assert_eq!(cad.kind, CasterKind::Light(2));
        // Read the other way, Day's front lights are its first and third.
        let emitted = caster(LightingScheme::Day, LightSense::Emitted).unwrap();
        assert_eq!(emitted.kind, CasterKind::Light(2));
    }

    #[test]
    fn camera_lights_use_the_shadow_key_and_unlit_has_none() {
        for scheme in [LightingScheme::Artwork, LightingScheme::Headlamp] {
            let c = caster(scheme, LightSense::TowardsLight).unwrap();
            assert_eq!(c.kind, CasterKind::Dim);
            assert!(c.to_light[1] > 0.0, "from above");
        }
        assert!(caster(LightingScheme::None, LightSense::TowardsLight).is_none());
    }

    #[test]
    fn the_matrix_is_the_clip_function() {
        let bounds = Bounds {
            min: [-1.0, -2.0, 0.0],
            max: [3.0, 1.0, 2.0],
        };
        let view = LightView::new(normalize([0.3, 0.8, -0.5]).unwrap(), &bounds, 512).unwrap();
        let m = view.matrix();
        for p in [[0.0, 0.0, 0.0], [3.0, 1.0, 2.0], [-1.0, -2.0, 0.5]] {
            let want = view.clip(p);
            for (r, w) in want.iter().enumerate() {
                let got: f64 = (0..3).map(|c| f64::from(m[c * 4 + r]) * p[c]).sum::<f64>()
                    + f64::from(m[12 + r]);
                assert!((got - w).abs() < 1e-5);
                assert!(w.abs() <= 1.0, "inside the map");
            }
        }
    }

    #[test]
    fn a_square_shadows_what_is_behind_it_from_the_light() {
        let bounds = Bounds {
            min: [-2.0; 3],
            max: [2.0; 3],
        };
        let view = LightView::new([0.0, 0.0, 1.0], &bounds, 256).unwrap();
        let square = TriangleMesh {
            positions: vec![
                [-1.0, -1.0, 1.0],
                [1.0, -1.0, 1.0],
                [1.0, 1.0, 1.0],
                [-1.0, 1.0, 1.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            ..TriangleMesh::default()
        };
        let paint = Paint {
            colour: [0, 0, 0, 255],
            texture: None,
        };
        let map = ShadowMap::build(view, &[square], &[paint], &[true], 3);
        let up = [0.0, 0.0, 1.0];
        let under = map.visibility(view.lookup_point([0.0, 0.0, -1.0], up));
        let beside = map.visibility(view.lookup_point([1.8, 0.0, -1.0], up));
        let on_top = map.visibility(view.lookup_point([0.0, 0.0, 1.0], up));
        assert_eq!((under, beside, on_top), (0.0, 1.0, 1.0));
    }

    fn square(half: f64, z: f64) -> TriangleMesh {
        TriangleMesh {
            positions: vec![
                [-half, -half, z],
                [half, -half, z],
                [half, half, z],
                [-half, half, z],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            ..TriangleMesh::default()
        }
    }

    #[test]
    fn a_raised_square_shadows_the_floor_away_from_the_key() {
        use crate::{Camera, Projection, RenderOptions, render};
        let meshes = [square(2.0, 0.0), square(0.5, 1.0)];
        let camera = Camera {
            eye: [0.0, 0.0, 10.0],
            target: [0.0; 3],
            up: [0.0, 1.0, 0.0],
            projection: Projection::Orthographic { height: 5.0 },
        };
        let options = RenderOptions {
            width: 50,
            height: 50,
            ..RenderOptions::default()
        };
        let red = |options: &RenderOptions, x: f64, y: f64| {
            let image = render(&meshes, &camera, options).unwrap();
            let (px, py) = ((25.0 + 10.0 * x) as usize, (25.0 - 10.0 * y) as usize);
            image.rgba[(py * 50 + px) * 4]
        };
        // The shadow key is up-left and in front, so the square's shadow
        // falls down-right of it: (0.83, -1.67) at height 1.
        assert_eq!(red(&options, 0.8, -1.6), 95, "in the shadow: half shade");
        assert_eq!(red(&options, -1.5, 1.5), 190, "lit");
        assert_eq!(red(&options, 0.0, 0.0), 190, "the square itself");
        let off = RenderOptions {
            shadows: false,
            ..options
        };
        assert_eq!(red(&off, 0.8, -1.6), 190);
    }
}

//! Cast shadows from the key directional light: which light is the key, the
//! orthographic light view fitted to the model, and the CPU shadow map.
//! Further directional casters are in `shadow_set`.
//! [`crate::FrameShadow`] publishes the same numbers to a GPU, so both paths
//! test the same texels with the same arithmetic. Design:
//! `docs/research/3d-shadows-and-occlusion-design.md`.

use crate::lighting::Shader;
use crate::raster::{at, edge};
use crate::render::{Bounds, at3};
use crate::shadow_cast::{Cast, Occluder, Surface, Texels, Veil, runs};
use crate::vec3::{cross, dot, normalize, sub};
use crate::work::box_tests;
use crate::{NamedView, UpAxis};

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

/// Where the shadow-casting light is ([`crate::RenderOptions::shadow_light`]).
/// pdfcer's: the standard defines no shadows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ShadowLight {
    /// One of the scheme's lights (a model's own directional ones under
    /// [`crate::LightingScheme::Artwork`]), chosen each frame as
    /// [`crate::RenderOptions::shadows`] says. In
    /// [`crate::LightFrame::Camera`] the shadow turns with the view. The
    /// default.
    #[default]
    FollowsScheme,
    /// A sun fixed in the model, whatever the scheme or camera: above, left
    /// of and in front of the model as seen from the iso view of a model
    /// whose vertical axis is `up`. Where blocked, half the shade is kept.
    /// The shadow stays in one model place as the camera moves, so a GPU
    /// shadow map is redrawn only when the model or options change.
    Sun {
        /// The model's vertical axis.
        up: UpAxis,
        /// The model's up is the negative of `up` (the sun is on the
        /// negative side): the iso placement turned 180 degrees about the
        /// axis after `up` (X to Y, Y to Z, Z to X).
        negative: bool,
    },
    /// A sun the operator aims ([`crate::SunLight`]), added to the scheme
    /// as one more directional light and casting as the key light: its own
    /// share is removed where blocked. Fixed in the model, so a GPU shadow
    /// map is redrawn only when the sun or the model changes.
    AimedSun(crate::SunLight),
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
    /// at the camera; `None` unlit. Only the first
    /// [`crate::gpu_frame::MAX_LIGHTS`] lights are candidates, the ones a GPU
    /// frame draws, so both paths shadow the same light. `basis` is the
    /// camera's right, up and forward.
    pub(crate) fn choose(
        shader: &Shader,
        sense: LightSense,
        basis: &[[f64; 3]; 3],
    ) -> Option<Caster> {
        let forward = basis[2];
        let lights: &[(Option<[f64; 3]>, [f64; 3])] = match shader {
            Shader::Unlit => return None,
            Shader::Headlight { .. } => &[],
            Shader::Set { lights, .. } => lights,
        };
        let brightness = |c: &[f64; 3]| c.iter().sum::<f64>();
        let candidates = lights
            .iter()
            .take(crate::gpu_frame::MAX_LIGHTS)
            .enumerate()
            .filter_map(|(i, (dir, colour))| {
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
                .filter(|(_, l, _)| !front || dot(*l, forward) < 0.0)
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
        Some(Caster {
            to_light: shadow_key(basis)?,
            kind: CasterKind::Dim,
        })
    }

    /// The caster for `light`: [`Caster::choose`] for
    /// [`ShadowLight::FollowsScheme`], else the sun; `None` unlit.
    pub(crate) fn for_light(
        light: ShadowLight,
        shader: &Shader,
        sense: LightSense,
        basis: &[[f64; 3]; 3],
    ) -> Option<Caster> {
        match light {
            ShadowLight::Sun { up, negative } if !matches!(shader, Shader::Unlit) => Some(Caster {
                to_light: sun(up, negative)?,
                kind: CasterKind::Dim,
            }),
            ShadowLight::Sun { .. } => None,
            ShadowLight::FollowsScheme => Self::choose(shader, sense, basis),
            // `Shader::for_frame` added the sun as the last light.
            ShadowLight::AimedSun(sun) => match shader {
                Shader::Set { lights, .. } => lights.len().checked_sub(1).map(|i| Caster {
                    to_light: sun.direction(),
                    kind: CasterKind::Light(i),
                }),
                _ => None,
            },
        }
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
        let total = shader.shade_far(normal, towards);
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

/// [`SHADOW_KEY`] in the camera `basis` (right, up, forward), unit, model
/// space.
fn shadow_key([right, up, forward]: &[[f64; 3]; 3]) -> Option<[f64; 3]> {
    let [x, y, z] = SHADOW_KEY;
    normalize([0, 1, 2].map(|i| x * at(*right, i) + y * at(*up, i) + z * at(*forward, i)))
}

/// [`ShadowLight::Sun`]'s direction: the shadow key of the iso view about
/// `up`, turned half a revolution about the next axis when `negative`.
fn sun(up: UpAxis, negative: bool) -> Option<[f64; 3]> {
    let (dir, image_up) = NamedView::Iso.direction(up);
    let forward = normalize(dir)?;
    let right = normalize(cross(forward, image_up))?;
    let v = shadow_key(&[right, cross(right, forward), forward])?;
    Some(if negative { turned(up, v) } else { v })
}

/// `v` turned half a revolution about the axis after `up` (X to Y, Y to
/// Z, Z to X): a sun's place for a model whose up is minus `up`.
pub(crate) fn turned(up: UpAxis, v: [f64; 3]) -> [f64; 3] {
    let a = match up {
        UpAxis::X => 1,
        UpAxis::Y => 2,
        UpAxis::Z => 0,
    };
    // v' = 2(a.v)a - v for the unit axis a.
    [0, 1, 2].map(|i| if i == a { at(v, i) } else { -at(v, i) })
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

    /// Occluder `o`'s corners on the map: x, y in texels, depth 0–1.
    pub(crate) fn texels(&self, o: &Occluder<'_>) -> [[f64; 3]; 3] {
        let s = f64::from(self.size);
        [0, 1, 2].map(|k| {
            let [x, y, z] = self.clip(at3(&o.world, k));
            [(x + 1.0) / 2.0 * s, (y + 1.0) / 2.0 * s, (z + 1.0) / 2.0]
        })
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

    /// The map's centre, model space, and its half-width, model units.
    pub(crate) fn frame(&self) -> ([f64; 3], f64) {
        (self.centre, self.reach)
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
/// nothing is; with veil layers when something casts partly.
pub(crate) struct ShadowMap {
    pub(crate) view: LightView,
    depth: Vec<f32>,
    veil: Option<Veil>,
}

impl ShadowMap {
    /// `occluders` drawn from the light, in row bands on `workers` threads;
    /// veil layers when `veiled`. A texel keeps its nearest depth and
    /// multiplies its transmission, so the map is the same at any count.
    pub(crate) fn build(
        view: LightView,
        occluders: &[Occluder<'_>],
        veiled: bool,
        workers: usize,
    ) -> ShadowMap {
        let tris: Vec<_> = occluders
            .iter()
            .map(|o| (view.texels(o), o.surface))
            .collect();
        let side = view.size as usize;
        let mut depth = vec![1.0f32; side * side];
        let mut veil = veiled.then(|| Veil::new(side * side));
        let workers = workers.max(1);
        let rows = side.div_ceil(workers * 4).max(1);
        let mut jobs: Vec<Vec<(usize, Texels)>> = (0..workers).map(|_| Vec::new()).collect();
        for (k, band) in runs(&mut depth, veil.as_mut(), rows * side)
            .into_iter()
            .enumerate()
        {
            if let Some(job) = jobs.get_mut(k % workers) {
                job.push((k * rows, band));
            }
        }
        crate::parallel::run_each(jobs, &|job| {
            for (top, mut band) in job {
                for (t, surface) in &tris {
                    fill(&mut band, side, top, *t, surface.as_ref());
                }
            }
        });
        ShadowMap { view, depth, veil }
    }

    /// The texel tests [`Self::build`] makes drawing `occluders` in `view`.
    pub(crate) fn tests(view: &LightView, occluders: &[Occluder<'_>]) -> u64 {
        occluders.iter().fold(0u64, |sum, o| {
            sum.saturating_add(box_tests(view.texels(o), view.size, view.size))
        })
    }

    /// The lit share (0–1) of a [`LightView::lookup_point`]: 3 × 3 texels
    /// around it, each lit when the point is no deeper than it plus the
    /// bias, times what its veil lets through; a texel off the map is lit.
    pub(crate) fn visibility(&self, at: [f64; 3]) -> f64 {
        let side = i64::from(self.view.size);
        let (cx, cy) = (at[0].floor() as i64, at[1].floor() as i64);
        let depth = at[2] - self.view.bias();
        let mut lit = 0.0;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (x, y) = (cx + dx, cy + dy);
                let i =
                    (x >= 0 && y >= 0 && x < side && y < side).then_some((y * side + x) as usize);
                let stored = i.and_then(|i| self.depth.get(i).copied());
                if stored.is_none_or(|d| depth <= f64::from(d)) {
                    lit += match (i, &self.veil) {
                        (Some(i), Some(veil)) => veil.through(i, depth),
                        _ => 1.0,
                    };
                }
            }
        }
        lit / 9.0
    }
}

/// Draws map triangle `t` at each texel centre inside it, within `band`:
/// the rows from `top` of a map `side` texels wide. An opaque triangle
/// (`surface` `None`) casts fully; a see-through one per fragment.
fn fill(
    band: &mut Texels<'_>,
    side: usize,
    top: usize,
    t: [[f64; 3]; 3],
    surface: Option<&Surface<'_>>,
) {
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
    let bottom = (top + band.depth.len() / side) as f64 - 1.0;
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
            let cast = surface.map_or(Cast::Solid, |s| s.cast(w));
            band.put((y - top) * side + x, z, cast);
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::LightingScheme;
    use crate::TriangleMesh;
    use crate::raster::Paint;
    use crate::shadow_cast::{ShadowAlpha, occluders};

    const BASIS: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    fn caster(scheme: LightingScheme, sense: LightSense) -> Option<Caster> {
        let shader =
            crate::lighting::Shader::new(scheme.lights(), crate::LightFrame::Camera, &BASIS);
        Caster::choose(&shader, sense, &BASIS)
    }

    #[test]
    fn the_key_light_is_one_the_gpu_draws() {
        let dim = (Some([0.0, 0.0, -1.0]), [0.1; 3]);
        let mut lights = vec![dim; crate::gpu_frame::MAX_LIGHTS];
        lights.push((Some([0.0, 0.0, -1.0]), [1.0; 3]));
        lights[3].1 = [0.5; 3];
        let shader = crate::lighting::Shader::Set {
            ambient: [0.0; 3],
            lights,
            local: Vec::new(),
        };
        let c = Caster::choose(&shader, LightSense::TowardsLight, &BASIS).unwrap();
        assert_eq!(c.kind, CasterKind::Light(3));
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
        let casts = occluders(&[square], &[paint], &[true], ShadowAlpha::Transmit);
        let map = ShadowMap::build(view, &casts, false, 3);
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

    #[test]
    fn the_sun_is_fixed_in_the_model_and_above_it() {
        let tilted = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]];
        let sun = |basis: &[[f64; 3]; 3], scheme: LightingScheme, up: UpAxis| {
            let shader = Shader::new(scheme.lights(), crate::LightFrame::Camera, basis);
            let light = ShadowLight::Sun {
                up,
                negative: false,
            };
            Caster::for_light(light, &shader, LightSense::Emitted, basis)
        };
        let a = sun(&BASIS, LightingScheme::Day, UpAxis::Z).unwrap();
        assert_eq!(a, sun(&tilted, LightingScheme::Artwork, UpAxis::Z).unwrap());
        assert_eq!(a.kind, CasterKind::Dim);
        assert!(a.to_light[2] > 0.9, "high above a z-up model: {a:?}");
        let y = sun(&BASIS, LightingScheme::Day, UpAxis::Y).unwrap();
        assert!(y.to_light[1] > 0.9, "{y:?}");
        assert!(sun(&BASIS, LightingScheme::None, UpAxis::Z).is_none());
    }

    #[test]
    fn a_negative_up_puts_the_sun_on_the_negative_side() {
        let shader = Shader::new(
            LightingScheme::Day.lights(),
            crate::LightFrame::Camera,
            &BASIS,
        );
        for (up, i) in [(UpAxis::X, 0), (UpAxis::Y, 1), (UpAxis::Z, 2)] {
            let get = |negative| {
                let light = ShadowLight::Sun { up, negative };
                Caster::for_light(light, &shader, LightSense::Emitted, &BASIS)
                    .unwrap()
                    .to_light
            };
            let (pos, neg) = (get(false), get(true));
            assert!(neg[i] < -0.9, "{up:?}: {neg:?}");
            assert!((neg[i] + pos[i]).abs() < 1e-12, "{up:?}");
            let len: f64 = neg.iter().map(|c| c * c).sum();
            assert!((len - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn a_sun_shadow_stays_on_the_floor_as_the_camera_turns() {
        use crate::{Camera, Projection, RenderOptions, render};
        let meshes = [square(2.0, 0.0), square(0.5, 1.0)];
        let red = |light: ShadowLight, turn: f64, p: [f64; 2]| {
            let up = [turn.cos(), turn.sin(), 0.0];
            let camera = Camera {
                eye: [0.0, 0.0, 10.0],
                target: [0.0; 3],
                up,
                projection: Projection::Orthographic { height: 5.0 },
            };
            let options = RenderOptions {
                width: 100,
                height: 100,
                shadow_light: light,
                ambient_occlusion: false,
                ..RenderOptions::default()
            };
            let image = render(&meshes, &camera, &options).unwrap();
            let (x, y) = (p[0] * up[1] - p[1] * up[0], p[0] * up[0] + p[1] * up[1]);
            let (px, py) = ((50.0 + 20.0 * x) as usize, (50.0 - 20.0 * y) as usize);
            image.rgba[(py * 100 + px) * 4]
        };
        // The sun is high, from -x and -y: the shadow reaches (0.36, 0.25)
        // past the square, so (0.7, 0.3) is in it from every turn.
        let sun = ShadowLight::Sun {
            up: UpAxis::Z,
            negative: false,
        };
        let mut followed = Vec::new();
        for quarter in 0..4 {
            let turn = f64::from(quarter) * std::f64::consts::FRAC_PI_2;
            assert_eq!(red(sun, turn, [0.7, 0.3]), 95, "turn {quarter}");
            assert_eq!(red(sun, turn, [-0.7, -0.3]), 190, "turn {quarter}");
            followed.push(red(ShadowLight::FollowsScheme, turn, [0.7, 0.3]));
        }
        assert!(
            followed.contains(&190),
            "the camera key moves: {followed:?}"
        );
    }
}

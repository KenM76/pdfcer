//! Cast shadows from a model's point and spot lights: per lamp, a cube of
//! six perspective depth maps around it, each texel holding the face's view
//! depth (the largest coordinate of point − lamp) over the lamp's reach.
//! [`crate::FrameLampShadows`] publishes the same faces to a GPU, so both
//! paths test the same texels with the same arithmetic.
//!
//! Face `f` looks along axis `a = f / 2`, positive for even `f`. With
//! `o = p − lamp`, `b = (a + 1) % 3` and `c = (a + 2) % 3`, the face's view
//! depth is `z = |o_a|`, its texel x is `(o_b / z + 1) / 2 · size` and its
//! texel y `(o_c / z + 1) / 2 · size`, row 0 at the bottom. A point belongs
//! to the face of its largest `|o_i|`, ties to the lower axis.

use crate::lighting::shade::Local;
use crate::render::raster::{at, edge};
use crate::render::work::{box_tests, fan};
use crate::render::{Bounds, at3};
use crate::shadow::cast::{Cast, Occluder, Surface, Texels, Veil, runs};
use crate::vec3::{dot, sub};

/// [`crate::RenderOptions::lamp_shadow_map_size`]'s default, texels per
/// cube face side.
pub const DEFAULT_LAMP_SHADOW_MAP_SIZE: u32 = 512;
/// The largest lamp shadow face side used; a larger request is clamped.
pub const MAX_LAMP_SHADOW_MAP_SIZE: u32 = 4096;
const MIN_LAMP_SHADOW_MAP_SIZE: u32 = 64;
/// The near clip, as a share of the reach.
pub(crate) const NEAR_SHARE: f64 = 1e-4;
/// The normal offset and depth bias, in texels at the point's depth.
const NORMAL_OFFSET_TEXELS: f64 = 1.5;
const BIAS_TEXELS: f64 = 2.0;
/// Faces per lamp.
pub(crate) const FACES: usize = 6;

/// The lamp shadow face side `size` asks for, clamped.
pub(crate) fn face_size(size: u32) -> u32 {
    size.clamp(MIN_LAMP_SHADOW_MAP_SIZE, MAX_LAMP_SHADOW_MAP_SIZE)
}

/// How far a lamp at `position` must see to reach every corner of `bounds`;
/// `None` when that is not a positive finite distance.
pub(crate) fn reach(position: [f64; 3], bounds: &Bounds) -> Option<f64> {
    let mut far: f64 = 0.0;
    for k in 0..8 {
        let corner = [0, 1, 2].map(|i| {
            if k >> i & 1 == 0 {
                at(bounds.min, i)
            } else {
                at(bounds.max, i)
            }
        });
        let d = sub(corner, position);
        far = far.max(dot(d, d).sqrt());
    }
    let far = far * 1.01;
    (far.is_finite() && far > 0.0).then_some(far)
}

/// The face `o` (point − lamp) falls on, its −1..1 face coordinates and its
/// view depth; `None` at the lamp.
pub(crate) fn face_of(o: [f64; 3]) -> Option<(usize, [f64; 2], f64)> {
    let m = o.map(f64::abs);
    let a = if m[0] >= m[1] && m[0] >= m[2] {
        0
    } else if m[1] >= m[2] {
        1
    } else {
        2
    };
    let z = at(m, a);
    if !(z > 0.0 && z.is_finite()) {
        return None;
    }
    let face = 2 * a + usize::from(at(o, a) < 0.0);
    Some((face, [at(o, (a + 1) % 3) / z, at(o, (a + 2) % 3) / z], z))
}

/// Face `face`'s view coordinates of `o`: x, y and the depth along its axis
/// (negative behind it).
fn face_view(face: usize, o: [f64; 3]) -> [f64; 3] {
    let a = face / 2;
    let s = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
    [at(o, (a + 1) % 3), at(o, (a + 2) % 3), s * at(o, a)]
}

/// Face `face` of a lamp at `position` reaching `far`: model to clip space,
/// column-major. Clip w is the face's view depth, so a GPU's
/// perspective-correct varying of it is the CPU's interpolated depth.
pub(crate) fn face_matrix(position: [f64; 3], far: f64, face: usize) -> [f32; 16] {
    let near = far * NEAR_SHARE;
    let (za, zb) = (
        (far + near) / (far - near),
        -2.0 * far * near / (far - near),
    );
    let a = face / 2;
    let s = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
    let unit = |i: usize| [0, 1, 2].map(|k| if k == i { 1.0 } else { 0.0 });
    let rows: [([f64; 3], f64); 4] = [
        (unit((a + 1) % 3), 0.0),
        (unit((a + 2) % 3), 0.0),
        (unit(a).map(|c| c * s * za), zb),
        (unit(a).map(|c| c * s), 0.0),
    ];
    let mut m = [0.0f32; 16];
    for (r, (axis, extra)) in rows.into_iter().enumerate() {
        for (c, v) in axis.into_iter().enumerate() {
            if let Some(slot) = m.get_mut(c * 4 + r) {
                *slot = v as f32;
            }
        }
        if let Some(slot) = m.get_mut(12 + r) {
            *slot = (extra - dot(axis, position)) as f32;
        }
    }
    m
}

/// One lamp's cube: six `size` × `size` faces of depth over `far`, 1.0
/// where nothing is.
pub(crate) struct LampCube {
    position: [f64; 3],
    far: f64,
    size: u32,
    depth: Vec<f32>,
    veil: Option<Veil>,
}

impl LampCube {
    /// The lit share (0–1) at model point `p` on a face with normal `face`:
    /// the point moved off its face towards the lamp, then 3 × 3 texels of
    /// its cube face around it, clamped to that face, each lit when the
    /// point is no deeper than it plus the bias.
    pub(crate) fn visibility(&self, p: [f64; 3], face: [f64; 3]) -> f64 {
        let o = sub(p, self.position);
        let Some((_, _, z0)) = face_of(o) else {
            return 1.0;
        };
        let size = f64::from(self.size);
        let texel = 2.0 * z0 / size;
        let side = if dot(face, o) > 0.0 { -1.0 } else { 1.0 };
        let k = side * NORMAL_OFFSET_TEXELS * texel;
        let moved = [0, 1, 2].map(|i| at(o, i) + at(face, i) * k);
        let Some((f, [u, v], z)) = face_of(moved) else {
            return 1.0;
        };
        let depth = (z - BIAS_TEXELS * texel) / self.far;
        let n = i64::from(self.size);
        let (cx, cy) = (
            ((u + 1.0) / 2.0 * size).floor() as i64,
            ((v + 1.0) / 2.0 * size).floor() as i64,
        );
        let base = f * (self.size as usize).pow(2);
        let mut lit = 0.0;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (x, y) = ((cx + dx).clamp(0, n - 1), (cy + dy).clamp(0, n - 1));
                let i = base + (y * n + x) as usize;
                if self.depth.get(i).is_none_or(|&d| depth <= f64::from(d)) {
                    lit += self.veil.as_ref().map_or(1.0, |v| v.through(i, depth));
                }
            }
        }
        lit / 9.0
    }
}

/// A face to draw: its index, lamp position and reach, and its texels.
type FaceJob<'a> = (usize, [f64; 3], f64, Texels<'a>);

/// The cubes of a frame's point and spot lights, one per
/// [`crate::lighting::Shader::local`] entry (`None` where a lamp casts no
/// shadow).
pub(crate) struct LampShadows {
    pub(crate) cubes: Vec<Option<LampCube>>,
}

impl LampShadows {
    /// `occluders` drawn into each face of each of `lamps`' cubes (none for
    /// a `None`, keeping the cubes parallel to the frame's lamps), with
    /// veil layers when `veiled`, faces on `workers` threads; each texel
    /// keeps its nearest depth and multiplies its transmission, so the maps
    /// are the same at any count.
    pub(crate) fn build(
        lamps: &[Option<Local>],
        bounds: &Bounds,
        size: u32,
        (occluders, veiled): (&[Occluder<'_>], bool),
        workers: usize,
    ) -> LampShadows {
        let size = face_size(size);
        let face_len = (size as usize).pow(2);
        let mut cubes: Vec<Option<LampCube>> = lamps
            .iter()
            .map(|l| {
                let l = l.as_ref()?;
                Some(LampCube {
                    position: l.position,
                    far: reach(l.position, bounds)?,
                    size,
                    depth: vec![1.0; face_len * FACES],
                    veil: veiled.then(|| Veil::new(face_len * FACES)),
                })
            })
            .collect();
        let mut jobs: Vec<Vec<FaceJob<'_>>> = (0..workers.max(1)).map(|_| Vec::new()).collect();
        let mut k = 0;
        for cube in cubes.iter_mut().flatten() {
            let (position, far) = (cube.position, cube.far);
            for (face, slot) in runs(&mut cube.depth, cube.veil.as_mut(), face_len)
                .into_iter()
                .enumerate()
            {
                if let Some(job) = jobs.get_mut(k % workers.max(1)) {
                    job.push((face, position, far, slot));
                }
                k += 1;
            }
        }
        crate::render::parallel::run_each(jobs, &|job| {
            for (face, position, far, mut texels) in job {
                for o in occluders {
                    let v = [0, 1, 2].map(|k| face_view(face, sub(at3(&o.world, k), position)));
                    fill_face(&mut texels, size, far, v, o.surface.as_ref());
                }
            }
        });
        LampShadows { cubes }
    }

    /// The texel tests [`Self::build`] makes drawing `occluders` into each
    /// of `lamps`' cubes over `bounds`, `size` texels a face side.
    pub(crate) fn tests(
        lamps: &[Option<Local>],
        bounds: &Bounds,
        size: u32,
        occluders: &[Occluder<'_>],
    ) -> u64 {
        let size = face_size(size);
        lamps
            .iter()
            .flatten()
            .filter_map(|l| reach(l.position, bounds).map(|far| (l.position, far)))
            .fold(0u64, |sum, cube| {
                sum.saturating_add(cube_tests(size, cube, occluders))
            })
    }

    /// Lamp `i`'s lit share at `p` on a face whose normal `face` is on the
    /// viewer's side; 1 when it has no cube, 0 from behind the face.
    pub(crate) fn visibility(&self, i: usize, p: [f64; 3], face: [f64; 3]) -> f64 {
        match self.cubes.get(i) {
            Some(Some(cube)) if dot(face, sub(cube.position, p)) < 0.0 => 0.0,
            Some(Some(cube)) => cube.visibility(p, face),
            _ => 1.0,
        }
    }
}

/// The texels every face of a `size`-square cube at `position` reaching
/// `far` tests for `occluders`, as [`fill_face`] clips and scans them.
fn cube_tests(size: u32, (position, far): ([f64; 3], f64), occluders: &[Occluder<'_>]) -> u64 {
    let mut sum = 0u64;
    for face in 0..FACES {
        for o in occluders {
            let v = [0, 1, 2].map(|k| face_view(face, sub(at3(&o.world, k), position)));
            let screen: Vec<_> = face_polygon(size, far, v)
                .into_iter()
                .map(|c| c.0)
                .collect();
            for t in fan(&screen) {
                sum = sum.saturating_add(box_tests(t, size, size));
            }
        }
    }
    sum
}

/// Draws face-view triangle `t` at each texel centre of a `size`-square
/// face, depth over `far`, clipped to the near plane. An opaque triangle
/// (`surface` `None`) casts fully; a see-through one per fragment, its
/// texture coordinates perspective-correct.
fn fill_face(
    texels: &mut Texels<'_>,
    size: u32,
    far: f64,
    t: [[f64; 3]; 3],
    surface: Option<&Surface<'_>>,
) {
    let screen = face_polygon(size, far, t);
    for [a, b, c] in fan(&screen) {
        let tri = ([a.0, b.0, c.0], [a.1, b.1, c.1]);
        fill_triangle(texels, size, far, tri, surface);
    }
}

/// Face-view triangle `t` clipped to the near plane of a cube reaching
/// `far`, projected onto a `size`-square face: screen `[x, y, 1/z]` per
/// corner, with its weights over `t`'s corners; empty when wholly behind.
fn face_polygon(size: u32, far: f64, t: [[f64; 3]; 3]) -> Vec<([f64; 3], [f64; 3])> {
    let near = far * NEAR_SHARE;
    if t.iter().all(|v| at(*v, 2) < near) {
        return Vec::new();
    }
    let unit = |k: usize| [0, 1, 2].map(|i| if i == k { 1.0 } else { 0.0 });
    // Each clipped corner with its weights over `t`'s corners.
    let mut poly: Vec<([f64; 3], [f64; 3])> = Vec::with_capacity(4);
    for (k, &p) in t.iter().enumerate() {
        let next = (k + 1) % 3;
        let q = t.get(next).copied().unwrap_or(p);
        let (p_in, q_in) = (at(p, 2) >= near, at(q, 2) >= near);
        if p_in {
            poly.push((p, unit(k)));
        }
        if p_in != q_in {
            let s = (near - at(p, 2)) / (at(q, 2) - at(p, 2));
            let lerp =
                |a: [f64; 3], b: [f64; 3]| [0, 1, 2].map(|i| at(a, i) + s * (at(b, i) - at(a, i)));
            poly.push((lerp(p, q), lerp(unit(k), unit(next))));
        }
    }
    let n = f64::from(size);
    // Screen x, y and 1/z, which is linear in screen space.
    poly.iter()
        .map(|&(v, w)| {
            let z = at(v, 2);
            let x = (at(v, 0) / z + 1.0) / 2.0 * n;
            ([x, (at(v, 1) / z + 1.0) / 2.0 * n, 1.0 / z], w)
        })
        .collect()
}

/// One screen triangle of [`fill_face`]: corners `[x, y, 1/z]` and their
/// weights over the original triangle's corners.
type ScreenTriangle = ([[f64; 3]; 3], [[f64; 3]; 3]);

fn fill_triangle(
    texels: &mut Texels<'_>,
    size: u32,
    far: f64,
    (t, weights): ScreenTriangle,
    surface: Option<&Surface<'_>>,
) {
    let [a, b, c] = t;
    let area = edge(a, b, c);
    if !area.is_finite() || area.abs() < 1e-12 {
        return;
    }
    let n = f64::from(size);
    let lo = |i: usize| t.iter().map(|p| at(*p, i)).fold(f64::INFINITY, f64::min);
    let hi = |i: usize| {
        t.iter()
            .map(|p| at(*p, i))
            .fold(f64::NEG_INFINITY, f64::max)
    };
    let (x0, x1) = (lo(0).floor().max(0.0), hi(0).ceil().min(n - 1.0));
    let (y0, y1) = (lo(1).floor().max(0.0), hi(1).ceil().min(n - 1.0));
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
            let inverse = w[0] * a[2] + w[1] * b[2] + w[2] * c[2];
            if inverse <= 0.0 {
                continue;
            }
            let z = (1.0 / inverse / far) as f32;
            let cast = surface.map_or(Cast::Solid, |s| {
                let q = [w[0] * a[2], w[1] * b[2], w[2] * c[2]];
                s.cast([0, 1, 2].map(|j| {
                    let sum: f64 = q.iter().zip(&weights).map(|(k, c)| k * at(*c, j)).sum();
                    sum / inverse
                }))
            });
            texels.put(y * size as usize + x, z, cast);
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::TriangleMesh;
    use crate::render::raster::Paint;
    use crate::shadow::cast::{ShadowAlpha, occluders};

    #[test]
    fn every_face_matrix_agrees_with_face_of() {
        let lamp = [0.3, -0.2, 1.1];
        let far = 10.0;
        let points = [
            [3.0, 0.5, 1.0],
            [-3.0, 0.4, 1.5],
            [0.1, 4.0, 0.0],
            [0.5, -4.0, 2.0],
            [0.7, 0.2, 5.0],
            [-0.4, 0.1, -3.0],
        ];
        for (want, p) in points.into_iter().enumerate() {
            let o = sub(p, lamp);
            let (face, [u, v], z) = face_of(o).unwrap();
            assert_eq!(face, want, "{p:?}");
            let m = face_matrix(lamp, far, face);
            let clip: Vec<f64> = (0..4)
                .map(|r| {
                    (0..3).map(|c| f64::from(m[c * 4 + r]) * p[c]).sum::<f64>()
                        + f64::from(m[12 + r])
                })
                .collect();
            let w = clip[3];
            assert!((w - z).abs() < 1e-5, "w is the view depth");
            assert!((clip[0] / w - u).abs() < 1e-5 && (clip[1] / w - v).abs() < 1e-5);
            assert!(clip[2] / w > -1.0 && clip[2] / w < 1.0, "inside near..far");
        }
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
    fn a_card_between_the_lamp_and_the_floor_shadows_it() {
        let meshes = [square(4.0, 0.0), square(0.5, 1.0)];
        let paint = Paint {
            colour: [0, 0, 0, 255],
            texture: None,
        };
        let bounds = Bounds::of(&meshes).unwrap();
        let casts = occluders(
            &meshes,
            &[paint, paint],
            &[true, true],
            ShadowAlpha::Transmit,
        );
        let lamp = Local {
            position: [0.0, 0.0, 2.0],
            colour: [1.0; 3],
            attenuation: [1.0, 0.0, 0.0],
            cone: None,
            reach: f64::INFINITY,
            model: 0,
            slot: Some(0),
        };
        let shadows = LampShadows::build(&[Some(lamp)], &bounds, 256, (&casts, false), 3);
        let up = [0.0, 0.0, 1.0];
        // Under the card's centre, and its shadow's edge at x = 1.0.
        assert_eq!(shadows.visibility(0, [0.0, 0.0, 0.0], up), 0.0);
        assert_eq!(shadows.visibility(0, [0.9, 0.0, 0.0], up), 0.0);
        assert_eq!(shadows.visibility(0, [1.2, 0.0, 0.0], up), 1.0);
        assert_eq!(shadows.visibility(0, [-3.0, 3.0, 0.0], up), 1.0);
        // The card's top faces the lamp.
        assert_eq!(shadows.visibility(0, [0.2, 0.2, 1.0], up), 1.0);
        // The same at one worker.
        let one = LampShadows::build(&[Some(lamp)], &bounds, 256, (&casts, false), 1);
        let (a, b) = (&shadows.cubes[0], &one.cubes[0]);
        assert!(a.as_ref().unwrap().depth == b.as_ref().unwrap().depth);
    }
}

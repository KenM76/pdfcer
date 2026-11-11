//! Per-frame GPU bounce light (decision 224): the CPU path's probe volume
//! packed as one texture, and the main fragment shader's lookup of it, a
//! mirror of [`crate::indirect`]'s blend so both paths add the same light.

use crate::camera::Camera;
use crate::render::{RenderError, RenderOptions};
use crate::{AssembledModel, Bounce, BounceStats, PreparedModel};

/// The probe lookup the main fragment shader splices in before `main`:
/// its uniforms and `bounce(p, n)`, the irradiance at model point `p` on
/// the side unit normal `n` faces.
macro_rules! bounce_glsl {
    () => {
        r"uniform bool u_bounce;
uniform highp sampler2D u_bounce_atlas;
uniform vec3 u_bounce_origin;
uniform vec3 u_bounce_spacing;
uniform highp ivec3 u_bounce_dims;
uniform float u_bounce_strength;

// d as a point of the unit square of a probe's octahedral map.
vec2 bounce_encode(vec3 d) {
    float s = abs(d.x) + abs(d.y) + abs(d.z);
    if (!(s > 0.0)) return vec2(0.5);
    vec2 o = d.xy / s;
    if (d.z < 0.0)
        o = vec2(1.0 - abs(o.y), 1.0 - abs(o.x)) * vec2(o.x < 0.0 ? -1.0 : 1.0, o.y < 0.0 ? -1.0 : 1.0);
    return (o + 1.0) * 0.5;
}

// Map texel t, one step outside the map at most, folded across its seam.
ivec2 bounce_fold(ivec2 t) {
    if (t.x < 0 || t.x > 7) { t.x = clamp(t.x, 0, 7); t.y = 7 - t.y; }
    if (t.y < 0 || t.y > 7) { t.y = clamp(t.y, 0, 7); t.x = 7 - t.x; }
    return clamp(t, ivec2(0), ivec2(7));
}

// Probe i's tile in the atlas.
ivec2 bounce_tile(int i) {
    return ivec2((i % 64) * 16, (i / 64) * 8);
}

// The bilinear irradiance of the probe at tile towards d.
vec3 bounce_irradiance(ivec2 tile, vec3 d) {
    vec2 f = bounce_encode(d) * 8.0 - 0.5;
    vec2 o = floor(f);
    vec2 a = f - o;
    ivec2 t = ivec2(o);
    return texelFetch(u_bounce_atlas, tile + bounce_fold(t), 0).rgb * ((1.0 - a.x) * (1.0 - a.y))
        + texelFetch(u_bounce_atlas, tile + bounce_fold(t + ivec2(1, 0)), 0).rgb * (a.x * (1.0 - a.y))
        + texelFetch(u_bounce_atlas, tile + bounce_fold(t + ivec2(0, 1)), 0).rgb * ((1.0 - a.x) * a.y)
        + texelFetch(u_bounce_atlas, tile + bounce_fold(t + ivec2(1, 1)), 0).rgb * (a.x * a.y);
}

// The share of the view of the probe at tile reaching the point offset from it.
float bounce_visibility(ivec2 tile, vec3 offset) {
    float d = length(offset);
    ivec2 t = min(ivec2(bounce_encode(offset) * 8.0), ivec2(7));
    vec2 m = texelFetch(u_bounce_atlas, tile + ivec2(8, 0) + t, 0).rg;
    if (d <= m.x) return 1.0;
    float g = d - m.x;
    float r = m.y / (m.y + g * g);
    return r * r * r;
}

vec3 bounce(vec3 p, vec3 n) {
    vec3 cf = clamp((p - u_bounce_origin) / u_bounce_spacing, vec3(0.0), vec3(u_bounce_dims - 1));
    ivec3 base = min(ivec3(floor(cf)), u_bounce_dims - 2);
    vec3 frac = cf - vec3(base);
    float ms = min(u_bounce_spacing.x, min(u_bounce_spacing.y, u_bounce_spacing.z));
    vec3 biased = p + n * (0.25 * ms);
    vec3 sum = vec3(0.0);
    float total = 0.0;
    for (int k = 0; k < 8; k++) {
        ivec3 bits = ivec3(k & 1, (k >> 1) & 1, (k >> 2) & 1);
        ivec3 c = base + bits;
        ivec2 tile = bounce_tile(c.x + u_bounce_dims.x * (c.y + u_bounce_dims.y * c.z));
        if (texelFetch(u_bounce_atlas, tile, 0).a < 0.5) continue;
        vec3 q = u_bounce_origin + vec3(c) * u_bounce_spacing;
        vec3 tri = mix(1.0 - frac, frac, vec3(bits));
        vec3 qp = q - p;
        float ql = length(qp);
        float wrap = (dot(ql > 0.0 ? qp / ql : n, n) + 1.0) * 0.5;
        float w = wrap * wrap + 0.2;
        w *= bounce_visibility(tile, biased - q);
        if (w < 0.2) w *= w * w / 0.04;
        w *= tri.x * tri.y * tri.z;
        if (dot(qp, n) < 0.0) w *= 1e-9;
        if (w <= 0.0) continue;
        sum += bounce_irradiance(tile, n) * w;
        total += w;
    }
    return total > 1e-12 ? sum / total : vec3(0.0);
}

"
    };
}

/// One frame's bounce light for the main fragment shader: the probe
/// volume [`PreparedModel::render`] builds for the same frame, so the GPU
/// adds the light the CPU adds.
///
/// Upload [`Self::atlas`] as an `RGBA32F` texture, [`Self::atlas_width`] ×
/// [`Self::atlas_height`], in the order given (row 0 first), read with
/// `texelFetch` only; then set `u_bounce` true and `u_bounce_atlas`,
/// `u_bounce_origin`, `u_bounce_spacing`, `u_bounce_dims` and
/// `u_bounce_strength` from the fields of the same names. Without one, and
/// for the unlit second pass of [`crate::DrawData::emissive`], set
/// `u_bounce` false.
///
/// It depends on the model, its paint and the frame's lights, not on the
/// camera unless [`Self::follows_camera`]: rebuild it when those change.
/// A [`crate::FrameLens`] frame draws every face with this one, built
/// from the fisheye camera, never from a face's camera.
/// Building one traces up to about 800,000 rays, so a host builds it off
/// its frame loop.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FrameBounce {
    /// RGBA texels, [`Self::atlas_width`] × [`Self::atlas_height`]. Probe
    /// `i`'s 16 × 8 tile starts at texel (`16 (i mod 64)`, `8 (i div 64)`):
    /// its left 8 × 8 hold the irradiance (alpha 1 for a probe in use, else
    /// 0), its right 8 × 8 the mean distance and its variance.
    pub atlas: Vec<f32>,
    /// The atlas's width in texels (1,024).
    pub atlas_width: u32,
    /// The atlas's height in texels, a multiple of 8.
    pub atlas_height: u32,
    /// The lattice's lowest probe, model units.
    pub origin: [f32; 3],
    /// The distance between neighbouring probes along each axis.
    pub spacing: [f32; 3],
    /// The probes along each axis, at least 2 each.
    pub dims: [i32; 3],
    /// [`crate::IndirectOptions::strength`].
    pub strength: f32,
    /// Whether a light that follows the camera bounces
    /// ([`Bounce::EveryLight`] with a headlight or camera-frame light), so
    /// a new camera needs a new bounce.
    pub follows_camera: bool,
}

impl PreparedModel {
    /// The bounce light of a frame of `model` drawn from `camera` with
    /// `options`, as [`PreparedModel::render`] adds it; `None` when that
    /// frame adds none (bounce off, strength 0, unlit, or no light that
    /// bounces).
    ///
    /// ```
    /// use pdfcer_3d::{AssembledModel, Camera, PreparedModel, Projection, RenderOptions};
    /// let model = AssembledModel::default();
    /// let camera = Camera {
    ///     eye: [0.0, 0.0, 10.0],
    ///     target: [0.0; 3],
    ///     up: [0.0, 1.0, 0.0],
    ///     projection: Projection::Perspective { fov_y: 30.0 },
    /// };
    /// let prepared = PreparedModel::new(&model);
    /// // Lit from the camera only: nothing bounces by default.
    /// assert!(prepared.frame_bounce(&model, &camera, &RenderOptions::default())?.is_none());
    /// # Ok::<(), pdfcer_3d::RenderError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// As [`PreparedModel::render`] for the camera and the lights.
    pub fn frame_bounce(
        &self,
        model: &AssembledModel,
        camera: &Camera,
        options: &RenderOptions,
    ) -> Result<Option<FrameBounce>, RenderError> {
        let plan = crate::render::mode::Plan::of(options);
        let painted = self.paints(model, options);
        let pass = crate::render::mode_frame::face_pass(&plan, model, painted);
        let lit = crate::render::mode::frame_options(options);
        let scene = (&pass.meshes[..], &model.lights[..], &[][..]);
        let Some(light) = crate::lighting::bounce::frame_bounce(scene, &pass.paints, camera, &lit)?
        else {
            return Ok(None);
        };
        let stats = BounceStats::for_model(camera, &lit, &model.lights)?;
        let atlas = light.atlas();
        Ok(Some(FrameBounce {
            atlas_width: atlas.width as u32,
            atlas_height: atlas.height as u32,
            atlas: atlas.texels,
            origin: atlas.origin.map(|v| v as f32),
            spacing: atlas.spacing.map(|v| v as f32),
            dims: atlas.dims.map(|d| d as i32),
            strength: light.strength() as f32,
            follows_camera: lit.indirect.bounce == Bounce::EveryLight && stats.camera_lights > 0,
        }))
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
#[allow(clippy::single_range_in_vec_init)] // One face spanning every triangle is intended.
mod tests {
    use super::*;
    use crate::{IndirectOptions, LightingScheme, ModelLight, ModelLightKind, Projection};
    use crate::{TriangleMesh, render::raster::Paint};

    fn square(corners: [[f64; 3]; 4]) -> TriangleMesh {
        TriangleMesh {
            positions: corners.to_vec(),
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            faces: vec![0..2],
            normals_recalculated: true,
            ..TriangleMesh::default()
        }
    }

    /// A white floor between a red and a green wall, a lamp over it.
    fn room() -> AssembledModel {
        let wall = |x: f64| square([[x, 0., 0.], [x, 1., 0.], [x, 1., 1.], [x, 0., 1.]]);
        AssembledModel {
            meshes: vec![
                square([[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]]),
                wall(0.0),
                wall(1.0),
            ],
            colours: vec![
                Some([230, 230, 230, 255]),
                Some([220, 30, 30, 255]),
                Some([30, 220, 30, 255]),
            ],
            finishes: vec![None; 3],
            lights: vec![ModelLight::new(
                ModelLightKind::Point {
                    position: [0.5, 0.5, 0.9],
                    attenuation: [1.0, 0.0, 0.0],
                },
                [0.8; 3],
            )],
            ..AssembledModel::default()
        }
    }

    fn camera() -> Camera {
        Camera {
            eye: [0.5, -2.0, 1.5],
            target: [0.5, 0.5, 0.3],
            up: [0.0, 0.0, 1.0],
            projection: Projection::Perspective { fov_y: 50.0 },
        }
    }

    fn options() -> RenderOptions {
        RenderOptions {
            lighting: LightingScheme::Artwork,
            ..RenderOptions::default()
        }
    }

    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// `bounce_glsl!` in f32, reading the atlas as `texelFetch` does.
    struct Mirror<'a>(&'a FrameBounce);

    impl Mirror<'_> {
        fn fetch(&self, x: i32, y: i32) -> [f32; 4] {
            let at = 4 * (y as usize * self.0.atlas_width as usize + x as usize);
            self.0.atlas[at..at + 4].try_into().unwrap()
        }

        fn encode(d: [f32; 3]) -> [f32; 2] {
            let s = d[0].abs() + d[1].abs() + d[2].abs();
            if s.is_nan() || s <= 0.0 {
                return [0.5; 2];
            }
            let (mut x, mut y) = (d[0] / s, d[1] / s);
            if d[2] < 0.0 {
                let sign = |v: f32| if v < 0.0 { -1.0 } else { 1.0 };
                (x, y) = ((1.0 - y.abs()) * sign(x), (1.0 - x.abs()) * sign(y));
            }
            [(x + 1.0) * 0.5, (y + 1.0) * 0.5]
        }

        fn fold(mut i: i32, mut j: i32) -> (i32, i32) {
            if !(0..=7).contains(&i) {
                i = i.clamp(0, 7);
                j = 7 - j;
            }
            if !(0..=7).contains(&j) {
                j = j.clamp(0, 7);
                i = 7 - i;
            }
            (i.clamp(0, 7), j.clamp(0, 7))
        }

        fn irradiance(&self, tile: (i32, i32), d: [f32; 3]) -> [f32; 3] {
            let [u, v] = Self::encode(d);
            let (fx, fy) = (u * 8.0 - 0.5, v * 8.0 - 0.5);
            let (a, b) = (fx - fx.floor(), fy - fy.floor());
            let (i, j) = (fx.floor() as i32, fy.floor() as i32);
            let mut out = [0.0; 3];
            for (di, dj, w) in [
                (0, 0, (1.0 - a) * (1.0 - b)),
                (1, 0, a * (1.0 - b)),
                (0, 1, (1.0 - a) * b),
                (1, 1, a * b),
            ] {
                let (x, y) = Self::fold(i + di, j + dj);
                let t = self.fetch(tile.0 + x, tile.1 + y);
                for k in 0..3 {
                    out[k] += t[k] * w;
                }
            }
            out
        }

        fn visibility(&self, tile: (i32, i32), o: [f32; 3]) -> f32 {
            let d = dot(o, o).sqrt();
            let [u, v] = Self::encode(o);
            let (x, y) = (((u * 8.0) as i32).min(7), ((v * 8.0) as i32).min(7));
            let [mean, var, ..] = self.fetch(tile.0 + 8 + x, tile.1 + y);
            if d <= mean {
                return 1.0;
            }
            let g = d - mean;
            (var / (var + g * g)).powi(3)
        }

        fn bounce(&self, p: [f32; 3], n: [f32; 3]) -> [f32; 3] {
            let b = self.0;
            let mut base = [0i32; 3];
            let mut frac = [0f32; 3];
            for k in 0..3 {
                let f = ((p[k] - b.origin[k]) / b.spacing[k]).clamp(0.0, (b.dims[k] - 1) as f32);
                base[k] = (f.floor() as i32).min(b.dims[k] - 2);
                frac[k] = f - base[k] as f32;
            }
            let ms = b.spacing.iter().copied().fold(f32::INFINITY, f32::min);
            let biased: [f32; 3] = std::array::from_fn(|k| p[k] + n[k] * (0.25 * ms));
            let (mut sum, mut total) = ([0f32; 3], 0f32);
            for corner in 0..8 {
                let bits = [corner & 1, (corner >> 1) & 1, (corner >> 2) & 1];
                let c: [i32; 3] = std::array::from_fn(|k| base[k] + bits[k]);
                let i = c[0] + b.dims[0] * (c[1] + b.dims[1] * c[2]);
                let tile = ((i % 64) * 16, (i / 64) * 8);
                if self.fetch(tile.0, tile.1)[3] < 0.5 {
                    continue;
                }
                let q: [f32; 3] = std::array::from_fn(|k| b.origin[k] + c[k] as f32 * b.spacing[k]);
                let qp: [f32; 3] = std::array::from_fn(|k| q[k] - p[k]);
                let ql = dot(qp, qp).sqrt();
                let t = if ql > 0.0 { qp.map(|v| v / ql) } else { n };
                let wrap = (dot(t, n) + 1.0) * 0.5;
                let mut w = wrap * wrap + 0.2;
                w *= self.visibility(tile, std::array::from_fn(|k| biased[k] - q[k]));
                if w < 0.2 {
                    w *= w * w / 0.04;
                }
                w *= (0..3)
                    .map(|k| if bits[k] == 1 { frac[k] } else { 1.0 - frac[k] })
                    .product::<f32>();
                if dot(qp, n) < 0.0 {
                    w *= 1e-9;
                }
                if w <= 0.0 {
                    continue;
                }
                let e = self.irradiance(tile, n);
                for k in 0..3 {
                    sum[k] += e[k] * w;
                }
                total += w;
            }
            if total > 1e-12 {
                sum.map(|s| s / total)
            } else {
                [0.0; 3]
            }
        }
    }

    #[test]
    fn the_shader_lookup_reads_the_light_the_cpu_adds() {
        let model = room();
        let prepared = PreparedModel::new(&model);
        let gpu = prepared
            .frame_bounce(&model, &camera(), &options())
            .unwrap()
            .unwrap();
        assert!(!gpu.follows_camera);
        assert_eq!(
            gpu.atlas.len(),
            4 * (gpu.atlas_width * gpu.atlas_height) as usize
        );
        let paints: Vec<Paint<'_>> = prepared.paints(&model, &options()).0;
        let scene = (&model.meshes[..], &model.lights[..], &[][..]);
        let cpu = crate::lighting::bounce::frame_bounce(scene, &paints, &camera(), &options())
            .unwrap()
            .unwrap();
        let mirror = Mirror(&gpu);
        let (mut worst, mut lit) = (0f64, 0usize);
        type Surface = fn(f64, f64) -> [f64; 3];
        let sides: [([f64; 3], Surface); 3] = [
            ([0., 0., 1.], |a, b| [a, b, 0.0]),
            ([1., 0., 0.], |a, b| [0.0, a, b]),
            ([-1., 0., 0.], |a, b| [1.0, a, b]),
        ];
        for (n, at) in sides {
            for i in 0..=20 {
                for j in 0..=20 {
                    let p = at(f64::from(i) / 20.0, f64::from(j) / 20.0);
                    let mut want = [0.0; 3];
                    cpu.add(&mut want, p, n, n);
                    let got = mirror.bounce(p.map(|v| v as f32), n.map(|v| v as f32));
                    for (w, g) in want.iter().zip(got) {
                        worst = worst.max((w - f64::from(g) * f64::from(gpu.strength)).abs());
                    }
                    lit += usize::from(want.iter().any(|&w| w > 0.01));
                }
            }
        }
        assert!(lit > 300, "the room bounces: {lit}");
        assert!(worst < 0.5 / 255.0, "worst {worst}");
    }

    #[test]
    fn every_lens_reads_the_same_bounce_as_the_planar_view() {
        use crate::FisheyeMapping;
        let model = room();
        let prepared = PreparedModel::new(&model);
        let planar = prepared
            .frame_bounce(&model, &camera(), &options())
            .unwrap();
        for (fov, mapping) in [
            (180.0, FisheyeMapping::Equidistant),
            (360.0, FisheyeMapping::Equirectangular),
        ] {
            let lens = Camera {
                projection: Projection::Fisheye { fov, mapping },
                ..camera()
            };
            let got = prepared.frame_bounce(&model, &lens, &options()).unwrap();
            assert_eq!(got, planar, "{mapping:?}");
        }
    }

    #[test]
    fn a_frame_with_no_bounce_has_no_bounce_light() {
        let model = room();
        let prepared = PreparedModel::new(&model);
        let mut off = options();
        off.indirect = IndirectOptions {
            bounce: Bounce::Off,
            ..IndirectOptions::default()
        };
        assert!(
            prepared
                .frame_bounce(&model, &camera(), &off)
                .unwrap()
                .is_none()
        );
        let mut every = options();
        every.indirect.bounce = Bounce::EveryLight;
        every.lighting = LightingScheme::Headlamp;
        let follows = prepared.frame_bounce(&model, &camera(), &every).unwrap();
        assert!(follows.unwrap().follows_camera);
    }
}

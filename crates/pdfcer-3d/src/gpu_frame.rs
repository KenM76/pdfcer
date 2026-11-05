//! Per-frame GPU inputs: the camera's matrices and the lights resolved
//! against it, matching the CPU path's projection and shading.

use crate::lighting::Shader;
use crate::raster::View;
use crate::render::{Bounds, Camera, Projection, RenderError, RenderOptions};
use crate::vec3::{dot, length, sub};

/// The most directional lights, and separately the most point and spot
/// lights, a [`FrameLights`] carries: its [`FrameLights::light_table`]
/// stays within the 2,048 rows WebGL 2 guarantees a texture.
pub const MAX_LIGHTS: usize = 1024;

/// The most lights casting a shadow in one frame, on both paths: the key
/// light and [`crate::MAX_EXTRA_SHADOWS`] further directional lights, and
/// separately the first this many point and spot lights
/// ([`crate::FrameLampShadows`]). Each caster holds depth maps of its own,
/// so this bounds memory, not the shader.
pub const MAX_SHADOW_CASTERS: usize = 16;

/// Texels per row of [`FrameLights::light_table`].
pub const LIGHT_TABLE_WIDTH: usize = 5;

/// How [`FrameLights`] shades a corner, as the CPU path does. With `n` the
/// unit normal and `v` the unit direction towards the eye (eye − point in
/// perspective, −[`FrameLights::forward`] in orthographic):
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShadeMode {
    /// `0.3 + 0.7·|n·v|` on every channel.
    Headlight,
    /// 1 on every channel.
    Unlit,
    /// `ambient + Σ colourᵢ·|n·dᵢ|` per channel, `dᵢ` = `v` when the
    /// light's `w` is 1.
    Lights,
}

impl ShadeMode {
    /// The shader's `u_mode` value: 0, 1 or 2 in declaration order.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            ShadeMode::Headlight => 0,
            ShadeMode::Unlit => 1,
            ShadeMode::Lights => 2,
        }
    }
}

/// One frame's lighting uniforms: a [`crate::LightingScheme`] resolved
/// against a camera into model space. The shaders read the lights from
/// [`FrameLights::light_table`]; the fields hold the same values.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FrameLights {
    /// The formula.
    pub mode: ShadeMode,
    /// The ambient term, for [`ShadeMode::Lights`].
    pub ambient: [f32; 3],
    /// How many directional lights: the length of `directions`, `colours`
    /// and `specular`.
    pub count: usize,
    /// Unit model-space direction in xyz; `w` = 1 means "along the line of
    /// sight" (xyz then zero).
    pub directions: Vec<[f32; 4]>,
    /// Each light's colour, 0–1.
    pub colours: Vec<[f32; 3]>,
    /// Each light's specular colour, for a [`crate::DrawFinish`]: its
    /// colour unless a model light names another
    /// ([`crate::ModelLight::specular`]).
    pub specular: Vec<[f32; 3]>,
    /// The camera position, model units.
    pub eye: [f32; 3],
    /// The unit view direction.
    pub forward: [f32; 3],
    /// Whether the projection is orthographic (then `v` = −`forward`).
    pub orthographic: bool,
    /// How many point and spot lights: the length of the `local_` lists. A
    /// model's lamps under [`crate::LightingScheme::Artwork`]
    /// ([`FrameLights::for_model`]), each adding `colour·fade·cone·|n·l|`
    /// with `l` the unit direction from the surface point to the light,
    /// added per pixel by the fragment shader.
    pub local_count: usize,
    /// Model-space position in xyz; `w` the cosine of a spot's cut-off
    /// half-angle (a point with `−l·direction` below it gets nothing), −2
    /// for a point light.
    pub local_positions: Vec<[f32; 4]>,
    /// Colour times intensity.
    pub local_colours: Vec<[f32; 3]>,
    /// Specular colour times intensity, as `specular`.
    pub local_specular: Vec<[f32; 3]>,
    /// Constant, linear and quadratic attenuation: `fade = 1 / (c + l·d +
    /// q·d²)`, 1 when that denominator is not positive.
    pub local_attenuation: Vec<[f32; 3]>,
    /// A spot's unit direction in xyz, its exponent in `w`
    /// (`cone = max(−l·direction, 0)^w` inside the cut-off, 1 when `w` is
    /// 0); zero for a point light.
    pub local_cones: Vec<[f32; 4]>,
    /// The distance past which each lamp adds nothing: where its fade
    /// times its brightest channel falls to
    /// [`RenderOptions::lamp_cut`](crate::RenderOptions::lamp_cut) divided by
    /// `local_count` ([`LampCut`](crate::LampCut)). `f32::MAX` when its fade
    /// never falls that far or nothing is cut.
    pub local_reach: Vec<f32>,
    /// Lights beyond [`MAX_LIGHTS`] in either list, not drawn; for the host
    /// to disclose.
    pub dropped: usize,
}

impl FrameLights {
    /// `options.lighting` in `options.light_frame`, seen from `camera`.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] for a degenerate camera, as [`crate::render`].
    pub fn new(camera: &Camera, options: &RenderOptions) -> Result<Self, RenderError> {
        Self::for_model(camera, options, &[])
    }

    /// As [`FrameLights::new`], with `lights` (a model's
    /// [`crate::AssembledModel::lights`]) drawn under
    /// [`crate::LightingScheme::Artwork`], as [`crate::render_model`] draws
    /// them.
    ///
    /// # Errors
    ///
    /// As [`FrameLights::new`].
    pub fn for_model(
        camera: &Camera,
        options: &RenderOptions,
        lights: &[crate::ModelLight],
    ) -> Result<Self, RenderError> {
        let basis = camera.basis()?;
        let [_, _, forward] = basis;
        let mut out = FrameLights {
            mode: ShadeMode::Headlight,
            ambient: [0.0; 3],
            count: 0,
            directions: Vec::new(),
            colours: Vec::new(),
            specular: Vec::new(),
            eye: camera.eye.map(|c| c as f32),
            forward: forward.map(|c| c as f32),
            orthographic: matches!(camera.projection, crate::Projection::Orthographic { .. }),
            local_count: 0,
            local_positions: Vec::new(),
            local_colours: Vec::new(),
            local_specular: Vec::new(),
            local_attenuation: Vec::new(),
            local_cones: Vec::new(),
            local_reach: Vec::new(),
            dropped: 0,
        };
        let options = &crate::render_mode::frame_options(options);
        let shader = Shader::for_frame(options, lights, &basis);
        let gloss = crate::gloss::GlossLights::new(&shader, options, lights);
        match shader {
            Shader::Headlight => {}
            Shader::Unlit => out.mode = ShadeMode::Unlit,
            Shader::Set {
                ambient,
                lights,
                local,
            } => {
                out.dropped = lights.len().saturating_sub(MAX_LIGHTS)
                    + local.len().saturating_sub(MAX_LIGHTS);
                out.set_local(&local);
                out.mode = ShadeMode::Lights;
                out.ambient = ambient.map(|c| c as f32);
                for (dir, c) in lights.iter().take(MAX_LIGHTS) {
                    out.directions.push(match dir {
                        Some([x, y, z]) => [*x as f32, *y as f32, *z as f32, 0.0],
                        None => [0.0, 0.0, 0.0, 1.0],
                    });
                    out.colours.push(c.map(|v| v as f32));
                }
                out.count = out.directions.len();
            }
        }
        out.set_specular(&gloss);
        Ok(out)
    }

    /// The rows of [`FrameLights::light_table`]: `count + local_count`, at
    /// least 1 so the texture is never empty.
    #[must_use]
    pub fn light_table_rows(&self) -> usize {
        (self.count + self.local_count).max(1)
    }

    /// The lights as the shaders' `u_lights`: an RGBA32F texture
    /// [`LIGHT_TABLE_WIDTH`] texels wide and
    /// [`FrameLights::light_table_rows`] high, row 0 first, four floats a
    /// texel. Upload it with `NEAREST` filtering and no mipmaps (WebGL 2
    /// cannot filter 32-bit floats); the shaders read it with `texelFetch`.
    ///
    /// Rows `0..count` are the directional lights: texel 0 its direction
    /// (as `directions`), 1 its colour, 2 its specular colour. Rows
    /// `count..count + local_count` are the point and spot lights: texel 0
    /// its position and reach (`local_reach`), 1 its colour, 2 its specular
    /// colour, 3 its attenuation and cone exponent, 4 its cone direction
    /// and cut-off cosine (`local_positions`' `w`). A lamp's reach and cone
    /// sit in the texels read first, so the shader skips a lamp that cannot
    /// light a point after two reads. Unused channels and texels are zero.
    #[must_use]
    pub fn light_table(&self) -> Vec<f32> {
        let mut out = vec![0.0; self.light_table_rows() * LIGHT_TABLE_WIDTH * 4];
        let three = |c: &[f32; 3]| [c[0], c[1], c[2], 0.0];
        let far = (0..self.count).map(|i| {
            [
                self.directions.get(i).copied(),
                self.colours.get(i).map(three),
                self.specular.get(i).map(three),
                None,
                None,
            ]
        });
        let near = (0..self.local_count).map(|i| {
            let at = self.local_positions.get(i);
            let cone = self.local_cones.get(i);
            let reach = self.local_reach.get(i).copied().unwrap_or(f32::MAX);
            [
                at.map(|p| [p[0], p[1], p[2], reach]),
                self.local_colours.get(i).map(three),
                self.local_specular.get(i).map(three),
                self.local_attenuation
                    .get(i)
                    .map(|a| [a[0], a[1], a[2], cone.map_or(0.0, |c| c[3])]),
                cone.map(|c| [c[0], c[1], c[2], at.map_or(-2.0, |p| p[3])]),
            ]
        });
        for (row, texels) in out
            .chunks_exact_mut(LIGHT_TABLE_WIDTH * 4)
            .zip(far.chain(near))
        {
            for (slot, texel) in row.chunks_exact_mut(4).zip(texels) {
                slot.copy_from_slice(&texel.unwrap_or([0.0; 4]));
            }
        }
        out
    }
}

impl FrameLights {
    fn set_local(&mut self, local: &[crate::model_shade::Local]) {
        let f = |v: [f64; 3]| v.map(|c| c as f32);
        for l in local.iter().take(MAX_LIGHTS) {
            let [x, y, z] = f(l.position);
            let (cut, [dx, dy, dz], e) = l.cone.map_or((-2.0, [0.0; 3], 0.0), |c| {
                (c.cos_cutoff as f32, f(c.direction), c.exponent as f32)
            });
            self.local_positions.push([x, y, z, cut]);
            self.local_colours.push(f(l.colour));
            self.local_attenuation.push(f(l.attenuation));
            self.local_cones.push([dx, dy, dz, e]);
            self.local_reach.push(if l.reach < f64::from(f32::MAX) {
                l.reach as f32
            } else {
                f32::MAX
            });
        }
        self.local_count = self.local_positions.len();
    }
}

impl Camera {
    /// The model-to-eye matrix, column-major as OpenGL takes it: x right,
    /// y up, looking down −z.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] for a degenerate camera.
    pub fn view_matrix(&self) -> Result<[f32; 16], RenderError> {
        let [right, up, forward] = self.basis()?;
        let back = forward.map(|c| -c);
        let mut m = [0.0f32; 16];
        for (row, axis) in [right, up, back].into_iter().enumerate() {
            for (col, c) in axis.into_iter().enumerate() {
                set(&mut m, row, col, c);
            }
            set(&mut m, row, 3, -dot(axis, self.eye));
        }
        set(&mut m, 3, 3, 1.0);
        Ok(m)
    }

    /// The eye-to-clip matrix for an image `aspect` (width / height) wide,
    /// column-major. It frames exactly as the CPU path frames; its depth
    /// range is [`Camera::depth_range`], which the CPU path, having no far
    /// plane, does not need.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] for a field of view outside (0, 180)
    /// degrees or too small for a finite focal length, a non-positive
    /// orthographic height, a degenerate camera, or a fisheye lens, which
    /// no matrix draws ([`crate::FrameLens`] draws it).
    pub fn projection_matrix(
        &self,
        aspect: f64,
        bounds: &Bounds,
    ) -> Result<[f32; 16], RenderError> {
        let (view, near, far) = self.clip_range(aspect, bounds)?;
        let mut m = [0.0f32; 16];
        match view {
            View::Perspective { focal, aspect, .. } => {
                set(&mut m, 0, 0, focal / aspect);
                set(&mut m, 1, 1, focal);
                set(&mut m, 2, 2, (far + near) / (near - far));
                set(&mut m, 2, 3, 2.0 * far * near / (near - far));
                set(&mut m, 3, 2, -1.0);
            }
            View::Orthographic {
                half_height,
                aspect,
            } => {
                set(&mut m, 0, 0, 1.0 / (half_height * aspect));
                set(&mut m, 1, 1, 1.0 / half_height);
                set(&mut m, 2, 2, -2.0 / (far - near));
                set(&mut m, 2, 3, -(far + near) / (far - near));
                set(&mut m, 3, 3, 1.0);
            }
        }
        Ok(m)
    }

    /// `[near, far]`, the eye-space distances [`Camera::projection_matrix`]
    /// clips to, for the fragment shader's `u_depth_range`.
    ///
    /// In perspective the reference fragment shader writes logarithmic
    /// depth, `log2(w / near) / log2(far / near)` with `w` the fragment's
    /// view distance, instead of the matrix's hyperbolic depth. In a 24-bit
    /// depth buffer that resolves about `w · ln(far / near) / 2²⁴`: some
    /// 3 µm at 5 m in a 30 m room with a sub-millimetre near plane, where
    /// hyperbolic depth resolves millimetres and lets a texture panel and
    /// the wall behind it fight. Orthographic depth is already linear and is
    /// left as the matrix gives it.
    ///
    /// A fisheye camera has no projection matrix; its range is of the
    /// distance from the eye, which the lens resample and line shaders
    /// write in place of `w` ([`crate::FrameLens`]).
    ///
    /// # Errors
    ///
    /// As [`Camera::projection_matrix`], but a fisheye camera is accepted
    /// unless its field is refused ([`RenderError::FisheyeField`]).
    pub fn depth_range(&self, aspect: f64, bounds: &Bounds) -> Result<[f32; 2], RenderError> {
        if let Projection::Fisheye { .. } = self.projection {
            return self.lens_range(bounds);
        }
        let (_, near, far) = self.clip_range(aspect, bounds)?;
        Ok([near as f32, far as f32])
    }

    /// [`Camera::depth_range`] for a fisheye: the lens's near distance
    /// unless the whole model lies further from the eye.
    fn lens_range(&self, bounds: &Bounds) -> Result<[f32; 2], RenderError> {
        self.basis()?;
        let lens = crate::lens::Lens::for_camera(self, 1, 1)?
            .ok_or(RenderError::Camera("the camera is not a fisheye"))?;
        let centre = length(sub(bounds.centre(), self.eye));
        let reach = bounds.radius() * 1.01 + length(sub(self.target, self.eye)) * 1e-6;
        let near = lens.near.max(centre - reach);
        Ok([near as f32, (centre + reach).max(near * 2.0) as f32])
    }

    /// The CPU path's view and the depth range enclosing `bounds`. The
    /// perspective near plane keeps the CPU path's unless the whole model
    /// lies further away.
    fn clip_range(&self, aspect: f64, bounds: &Bounds) -> Result<(View, f64, f64), RenderError> {
        let [_, _, forward] = self.basis()?;
        let centre = dot(sub(bounds.centre(), self.eye), forward);
        let reach = bounds.radius() * 1.01 + length(sub(self.target, self.eye)) * 1e-6;
        let view = View::new(self, aspect)?;
        let (near, far) = match view {
            View::Perspective { near, .. } => {
                let near = near.max(centre - reach);
                (near, (centre + reach).max(near * 2.0))
            }
            View::Orthographic { .. } => (centre - reach, centre + reach),
        };
        Ok((view, near, far))
    }
}

/// `m[row][col] = v` in a column-major 4×4.
fn set(m: &mut [f32; 16], row: usize, col: usize, v: f64) {
    if let Some(slot) = m.get_mut(col * 4 + row) {
        *slot = v as f32;
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::lighting::{LightFrame, LightingScheme};
    use crate::{Projection, vec3::normalize};

    fn mul(m: &[f32; 16], p: [f64; 3]) -> [f64; 4] {
        let v = [p[0], p[1], p[2], 1.0];
        [0, 1, 2, 3].map(|r| (0..4).map(|c| f64::from(m[c * 4 + r]) * v[c]).sum())
    }

    fn cameras() -> [Camera; 2] {
        let base = Camera {
            eye: [3.0, -4.0, 5.0],
            target: [0.5, 0.25, 0.0],
            up: [0.0, 0.0, 1.0],
            projection: Projection::Perspective { fov_y: 40.0 },
        };
        let ortho = Camera {
            projection: Projection::Orthographic { height: 3.0 },
            ..base
        };
        [base, ortho]
    }

    #[test]
    fn the_matrices_put_a_point_where_the_cpu_path_does() {
        let bounds = Bounds {
            min: [-1.0; 3],
            max: [2.0; 3],
        };
        for camera in cameras() {
            let aspect = 1.5;
            let (v, p) = (
                camera.view_matrix().unwrap(),
                camera.projection_matrix(aspect, &bounds).unwrap(),
            );
            let view = View::new(&camera, aspect).unwrap();
            let [right, up, forward] = camera.basis().unwrap();
            for point in [[0.0, 0.0, 0.0], [2.0, -1.0, 1.5], [-1.0, 2.0, 2.0]] {
                let eye_space = mul(&v, point);
                let clip = mul(&p, [eye_space[0], eye_space[1], eye_space[2]]);
                let ndc = [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]];
                let d = sub(point, camera.eye);
                let cpu = view.project(
                    crate::raster::view_point(dot(d, right), dot(d, up), dot(d, forward)),
                    2,
                    2,
                );
                // CPU screen x = (nx + 1), y = (1 − ny) at a 2 × 2 image.
                assert!(
                    (ndc[0] - (cpu.at[0] - 1.0)).abs() < 1e-5,
                    "{camera:?} {point:?}"
                );
                assert!(
                    (ndc[1] - (1.0 - cpu.at[1])).abs() < 1e-5,
                    "{camera:?} {point:?}"
                );
                assert!(
                    ndc[2] > -1.0 && ndc[2] < 1.0,
                    "inside the depth range: {}",
                    ndc[2]
                );
            }
        }
    }

    /// G256: a walker's camera in a 25 m building (`near` ≈ 0.6 mm).
    fn walker() -> (Camera, Bounds) {
        let camera = Camera {
            eye: [3.0, -10.0, 1.6],
            target: [3.0, -3.6, 1.6],
            up: [0.0, 0.0, 1.0],
            projection: Projection::Perspective { fov_y: 60.0 },
        };
        let bounds = Bounds {
            min: [-0.29, -23.12, -0.03],
            max: [7.72, 0.39, 3.56],
        };
        (camera, bounds)
    }

    /// The depth a 24-bit buffer stores, as an integer step.
    fn quantised(depth: f64) -> i64 {
        (depth * f64::from(1u32 << 24)).round() as i64
    }

    #[test]
    fn the_depth_range_is_where_the_matrix_clips() {
        let (camera, bounds) = walker();
        for camera in [camera, cameras()[0], cameras()[1]] {
            let p = camera.projection_matrix(1.5, &bounds).unwrap();
            let [near, far] = camera.depth_range(1.5, &bounds).unwrap().map(f64::from);
            assert!(near > 0.0 || matches!(camera.projection, Projection::Orthographic { .. }));
            for (w, ndc) in [(near, -1.0), (far, 1.0)] {
                let clip = mul(&p, [0.0, 0.0, -w]);
                assert!((clip[2] / clip[3] - ndc).abs() < 1e-3, "{camera:?} {w}");
            }
        }
    }

    #[test]
    fn log_depth_separates_a_panel_a_millimetre_off_its_wall() {
        let (camera, bounds) = walker();
        let [near, far] = camera.depth_range(1.5, &bounds).unwrap().map(f64::from);
        assert!(
            near < 1e-3,
            "the walker's near plane is sub-millimetre: {near}"
        );
        // The reference fragment shader's mapping, and the matrix's.
        let log = |w: f64| (w / near).log2() / (far / near).log2();
        let hyperbolic = |w: f64| (far / (far - near)) * (1.0 - near / w);
        for w in [5.0, 10.0, 20.0] {
            let (panel, wall) = (w, w + 1e-3);
            assert!(quantised(log(panel)) < quantised(log(wall)), "log at {w} m");
        }
        // The defect the shader replaces: hyperbolic depth merges them at 10 m.
        assert_eq!(quantised(hyperbolic(10.0)), quantised(hyperbolic(10.001)));
    }

    #[test]
    fn frame_lights_shade_as_the_cpu_shader_does() {
        let normal = normalize([0.3, -0.5, 0.8]).unwrap();
        for camera in cameras() {
            let basis = camera.basis().unwrap();
            let towards = match camera.projection {
                Projection::Orthographic { .. } => basis[2].map(|c| -c),
                _ => normalize(sub(camera.eye, [0.0; 3])).unwrap(),
            };
            for scheme in LightingScheme::ALL {
                for frame in [LightFrame::Camera, LightFrame::World] {
                    let options = RenderOptions {
                        lighting: scheme,
                        light_frame: frame,
                        ..RenderOptions::default()
                    };
                    let want = Shader::new(scheme.lights(), frame, &basis).shade(
                        [0.0; 3],
                        normal,
                        Some(towards),
                    );
                    let l = FrameLights::new(&camera, &options).unwrap();
                    let n = normal.map(|c| c as f32);
                    let v = towards.map(|c| c as f32);
                    let facing = |d: [f32; 3]| (n[0] * d[0] + n[1] * d[1] + n[2] * d[2]).abs();
                    let got = match l.mode {
                        ShadeMode::Headlight => [0.3 + 0.7 * facing(v); 3],
                        ShadeMode::Unlit => [1.0; 3],
                        ShadeMode::Lights => {
                            let mut out = l.ambient;
                            for i in 0..l.count {
                                let d = l.directions[i];
                                let k = facing(if d[3] == 1.0 { v } else { [d[0], d[1], d[2]] });
                                for (o, c) in out.iter_mut().zip(l.colours[i]) {
                                    *o += c * k;
                                }
                            }
                            out
                        }
                    };
                    for (g, w) in got.iter().zip(want) {
                        assert!((f64::from(*g) - w).abs() < 1e-5, "{scheme:?} {frame:?}");
                    }
                }
            }
        }
    }

    /// The reference shader's local-light loop, in f32.
    fn local(l: &FrameLights, p: [f32; 3], n: [f32; 3]) -> [f32; 3] {
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let mut s = l.ambient;
        for i in 0..l.local_count {
            let [x, y, z, cut] = l.local_positions[i];
            let t = [x - p[0], y - p[1], z - p[2]];
            let d = dot(t, t).sqrt();
            let v = t.map(|c| c / d);
            let [a, b, c] = l.local_attenuation[i];
            let den = a + b * d + c * d * d;
            let fade = if den > 0.0 { 1.0 / den } else { 1.0 };
            let [dx, dy, dz, e] = l.local_cones[i];
            let mut cone = 1.0;
            if cut > -1.5 {
                let k = -dot(v, [dx, dy, dz]);
                cone = if k < cut {
                    0.0
                } else if e > 0.0 {
                    k.max(0.0).powf(e)
                } else {
                    1.0
                };
            }
            for (o, c) in s.iter_mut().zip(l.local_colours[i]) {
                *o += c * fade * cone * dot(n, v).abs();
            }
        }
        s
    }

    #[test]
    fn model_lamps_shade_as_the_cpu_shader_does() {
        use crate::{ModelLight, ModelLightKind};
        let lamp = ModelLight::new(
            ModelLightKind::Point {
                position: [0.0, 0.0, 2.0],
                attenuation: [0.2, 0.1, 0.5],
            },
            [1.0, 0.8, 0.6],
        );
        let spot = ModelLight::new(
            ModelLightKind::Spot {
                position: [1.0, 0.0, 1.0],
                direction: [0.0, 0.0, -1.0],
                attenuation: [1.0, 0.0, 0.0],
                angle: 0.6,
                exponent: 3.0,
            },
            [0.3, 0.3, 1.0],
        );
        let fill = ModelLight::new(ModelLightKind::Ambient, [0.1; 3]);
        let lights = [lamp, spot, fill];
        let camera = cameras()[0];
        let basis = camera.basis().unwrap();
        let options = RenderOptions::default();
        let l = FrameLights::for_model(&camera, &options, &lights).unwrap();
        assert_eq!(
            (l.mode, l.count, l.local_count, l.dropped),
            (ShadeMode::Lights, 0, 2, 0)
        );
        let shader = Shader::for_frame(&options, &lights, &basis);
        let n = normalize([0.3, -0.5, 0.8]).unwrap();
        for p in [[0.0; 3], [1.0, 0.1, 0.0], [1.5, 0.0, 0.2], [-1.0, 2.0, 0.5]] {
            let want = shader.shade(p, n, None);
            let got = local(&l, p.map(|c| c as f32), n.map(|c| c as f32));
            for (g, w) in got.iter().zip(want) {
                assert!((f64::from(*g) - w).abs() < 1e-5, "{p:?}: {got:?} {want:?}");
            }
        }
    }

    /// The reference shaders' lighting read from `table` as they read
    /// `u_lights`: directional rows, then lamp rows.
    fn from_table(l: &FrameLights, p: [f32; 3], n: [f32; 3], v: [f32; 3]) -> [f32; 3] {
        let t = l.light_table();
        let texel = |r: usize, k: usize| -> [f32; 4] {
            let at = (r * LIGHT_TABLE_WIDTH + k) * 4;
            [t[at], t[at + 1], t[at + 2], t[at + 3]]
        };
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let mut s = l.ambient;
        for i in 0..l.count {
            let [x, y, z, w] = texel(i, 0);
            let k = dot(n, if w > 0.5 { v } else { [x, y, z] }).abs();
            for (o, c) in s.iter_mut().zip(texel(i, 1)) {
                *o += c * k;
            }
        }
        for i in 0..l.local_count {
            let r = l.count + i;
            let [x, y, z, reach] = texel(r, 0);
            let to = [x - p[0], y - p[1], z - p[2]];
            let d = dot(to, to).sqrt();
            if d > reach {
                continue;
            }
            let u = to.map(|c| c / d);
            let [a, b, c, e] = texel(r, 3);
            let den = a + b * d + c * d * d;
            let mut k = if den > 0.0 { 1.0 / den } else { 1.0 };
            let [dx, dy, dz, cut] = texel(r, 4);
            if cut > -1.5 {
                let c = -dot(u, [dx, dy, dz]);
                k *= if c < cut {
                    0.0
                } else if e > 0.0 {
                    c.max(0.0).powf(e)
                } else {
                    1.0
                };
            }
            for (o, col) in s.iter_mut().zip(texel(r, 1)) {
                *o += col * k * dot(n, u).abs();
            }
        }
        s
    }

    /// `n` suns and `n` alternating point and spot lamps, all different.
    fn crowd(n: usize) -> Vec<crate::ModelLight> {
        use crate::{ModelLight, ModelLightKind};
        let mut out = Vec::new();
        for i in 0..n {
            let a = i as f64 * 0.37;
            let grey = 0.01 + 0.0005 * i as f64;
            out.push(ModelLight::new(
                ModelLightKind::Directional {
                    direction: [a.cos(), a.sin(), -0.5],
                },
                [grey, grey * 0.5, 0.02],
            ));
            let position = [2.0 * a.cos(), 2.0 * a.sin(), 1.0 + 0.01 * i as f64];
            let attenuation = [1.0, 0.05, 0.01];
            let kind = if i % 2 == 0 {
                ModelLightKind::Point {
                    position,
                    attenuation,
                }
            } else {
                ModelLightKind::Spot {
                    position,
                    direction: position.map(|c| -c),
                    attenuation,
                    angle: 0.9,
                    exponent: 2.0,
                }
            };
            out.push(ModelLight::new(kind, [0.02, grey, 0.01]));
        }
        out
    }

    #[test]
    fn the_light_table_carries_more_than_sixteen_lights_as_the_cpu_shades_them() {
        let lights = crowd(40);
        let camera = cameras()[0];
        let basis = camera.basis().unwrap();
        let options = RenderOptions::default();
        let l = FrameLights::for_model(&camera, &options, &lights).unwrap();
        assert_eq!((l.count, l.local_count, l.dropped), (40, 40, 0));
        assert_eq!(l.light_table().len(), 80 * LIGHT_TABLE_WIDTH * 4);
        assert_eq!(l.light_table_rows(), 80);
        let shader = Shader::for_frame(&options, &lights, &basis);
        let n = normalize([0.3, -0.5, 0.8]).unwrap();
        let v = normalize(sub(camera.eye, [0.0; 3])).unwrap();
        for p in [[0.0; 3], [1.0, 0.1, 0.0], [-0.5, 0.7, 0.2]] {
            let want = shader.shade(p, n, Some(v));
            let f = |a: [f64; 3]| a.map(|c| c as f32);
            let got = from_table(&l, f(p), f(n), f(v));
            for (g, w) in got.iter().zip(want) {
                assert!((f64::from(*g) - w).abs() < 1e-4, "{p:?}: {got:?} {want:?}");
            }
        }
        // The specular colours sit in texel 2 of each row.
        let t = l.light_table();
        let row = |r: usize| &t[(r * LIGHT_TABLE_WIDTH + 2) * 4..][..3];
        assert_eq!(row(39), l.specular[39]);
        assert_eq!(row(40 + 39), l.local_specular[39]);
    }

    #[test]
    fn lights_past_the_table_are_dropped_and_counted() {
        let lights = crowd(MAX_LIGHTS + 3);
        let l = FrameLights::for_model(&cameras()[0], &RenderOptions::default(), &lights).unwrap();
        assert_eq!(
            (l.count, l.local_count, l.dropped),
            (MAX_LIGHTS, MAX_LIGHTS, 6)
        );
        // WebGL 2 guarantees 2,048 rows.
        assert!(l.light_table_rows() <= 2048);
        let unlit = FrameLights::new(&cameras()[0], &RenderOptions::default()).unwrap();
        assert!(unlit.light_table_rows() >= 1);
        assert_eq!(
            unlit.light_table().len(),
            unlit.light_table_rows() * LIGHT_TABLE_WIDTH * 4
        );
    }
}

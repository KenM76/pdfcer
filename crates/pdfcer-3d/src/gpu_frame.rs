//! Per-frame GPU inputs: the camera's matrices and the lights resolved
//! against it, matching the CPU path's projection and shading.

use crate::lighting::Shader;
use crate::raster::View;
use crate::render::{Bounds, Camera, RenderError, RenderOptions};
use crate::vec3::{dot, length, sub};

/// The most lights a [`FrameLights`] carries; every standard scheme fits.
pub const MAX_LIGHTS: usize = 8;

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
/// against a camera into model space.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FrameLights {
    /// The formula.
    pub mode: ShadeMode,
    /// The ambient term, for [`ShadeMode::Lights`].
    pub ambient: [f32; 3],
    /// How many entries of `directions` and `colours` are used.
    pub count: usize,
    /// Unit model-space direction in xyz; `w` = 1 means "along the line of
    /// sight" (xyz then zero).
    pub directions: [[f32; 4]; MAX_LIGHTS],
    /// Each light's colour, 0–1.
    pub colours: [[f32; 3]; MAX_LIGHTS],
    /// The camera position, model units.
    pub eye: [f32; 3],
    /// The unit view direction.
    pub forward: [f32; 3],
    /// Whether the projection is orthographic (then `v` = −`forward`).
    pub orthographic: bool,
}

impl FrameLights {
    /// `options.lighting` in `options.light_frame`, seen from `camera`.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] for a degenerate camera, as [`crate::render`].
    pub fn new(camera: &Camera, options: &RenderOptions) -> Result<Self, RenderError> {
        let basis = camera.basis()?;
        let [_, _, forward] = basis;
        let mut out = FrameLights {
            mode: ShadeMode::Headlight,
            ambient: [0.0; 3],
            count: 0,
            directions: [[0.0; 4]; MAX_LIGHTS],
            colours: [[0.0; 3]; MAX_LIGHTS],
            eye: camera.eye.map(|c| c as f32),
            forward: forward.map(|c| c as f32),
            orthographic: matches!(camera.projection, crate::Projection::Orthographic { .. }),
        };
        match Shader::new(options.lighting.lights(), options.light_frame, &basis) {
            Shader::Headlight => {}
            Shader::Unlit => out.mode = ShadeMode::Unlit,
            Shader::Set { ambient, lights } => {
                out.mode = ShadeMode::Lights;
                out.ambient = ambient.map(|c| c as f32);
                for ((slot, colour), (dir, c)) in out
                    .directions
                    .iter_mut()
                    .zip(out.colours.iter_mut())
                    .zip(lights)
                {
                    *slot = match dir {
                        Some([x, y, z]) => [x as f32, y as f32, z as f32, 0.0],
                        None => [0.0, 0.0, 0.0, 1.0],
                    };
                    *colour = c.map(|v| v as f32);
                    out.count += 1;
                }
            }
        }
        Ok(out)
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
    /// range encloses `bounds` (from [`Bounds::of`] over the model's
    /// meshes), which the CPU path, having no far plane, does not need.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] for a field of view outside (0, 180)
    /// degrees, a non-positive orthographic height, or a degenerate camera.
    pub fn projection_matrix(
        &self,
        aspect: f64,
        bounds: &Bounds,
    ) -> Result<[f32; 16], RenderError> {
        let [_, _, forward] = self.basis()?;
        let centre = dot(sub(bounds.centre(), self.eye), forward);
        let reach = bounds.radius() * 1.01 + length(sub(self.target, self.eye)) * 1e-6;
        let mut m = [0.0f32; 16];
        match View::new(self, aspect)? {
            View::Perspective {
                focal,
                aspect,
                near,
            } => {
                let near = near.max(centre - reach);
                let far = (centre + reach).max(near * 2.0);
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
                let (near, far) = (centre - reach, centre + reach);
                set(&mut m, 0, 0, 1.0 / (half_height * aspect));
                set(&mut m, 1, 1, 1.0 / half_height);
                set(&mut m, 2, 2, -2.0 / (far - near));
                set(&mut m, 2, 3, -(far + near) / (far - near));
                set(&mut m, 3, 3, 1.0);
            }
        }
        Ok(m)
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
                    [
                        dot(d, right),
                        dot(d, up),
                        dot(d, forward),
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                    ],
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

    #[test]
    fn frame_lights_shade_as_the_cpu_shader_does() {
        let normal = normalize([0.3, -0.5, 0.8]).unwrap();
        for camera in cameras() {
            let basis = camera.basis().unwrap();
            let towards = match camera.projection {
                Projection::Orthographic { .. } => basis[2].map(|c| -c),
                Projection::Perspective { .. } => normalize(sub(camera.eye, [0.0; 3])).unwrap(),
            };
            for scheme in LightingScheme::ALL {
                for frame in [LightFrame::Camera, LightFrame::World] {
                    let options = RenderOptions {
                        lighting: scheme,
                        light_frame: frame,
                        ..RenderOptions::default()
                    };
                    let want =
                        Shader::new(scheme.lights(), frame, &basis).shade(normal, Some(towards));
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
}

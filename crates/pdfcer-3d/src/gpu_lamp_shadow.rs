//! Per-frame GPU lamp shadow inputs and their depth pass's reference
//! shaders: the CPU path's cube faces ([`crate::lamp_shadow`]), so both paths
//! shadow a model's point and spot lights with the same texels.

use crate::gpu_frame::MAX_SHADOW_CASTERS;
use crate::lamp_shadow::{FACES, face_matrix, face_size, reach};
use crate::lighting::Shader;
use crate::render::{Bounds, Camera, RenderError, RenderOptions};

/// The lamp depth pass's vertex shader: location 0 position (vec3) from
/// [`crate::DrawData::vertices`], the rest ignored; uniform `u_lamp_matrix`
/// ([`FrameLampShadows::face_matrix`]).
pub const GLSL_ES_300_LAMP_SHADOW_VERTEX: &str = r"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
uniform mat4 u_lamp_matrix;
out float v_z;

void main() {
    gl_Position = u_lamp_matrix * vec4(a_position, 1.0);
    v_z = gl_Position.w;
}
";

/// The lamp depth pass's fragment shader: the face's view depth over the
/// lamp's reach; uniform `u_lamp_far` ([`FrameLampShadows::far`] of the
/// lamp drawn).
pub const GLSL_ES_300_LAMP_SHADOW_FRAGMENT: &str = r"#version 300 es
precision highp float;
in float v_z;
uniform float u_lamp_far;

void main() {
    gl_FragDepth = v_z / u_lamp_far;
}
";

/// One frame's lamp shadows: the cube faces a host renders before the main
/// pass and the main fragment shader's lamp uniforms.
///
/// The host allocates a `size` × `size` × [`FrameLampShadows::layers`]
/// `DEPTH_COMPONENT32F` 2D array texture (`NEAREST` both ways, no compare
/// mode). For each lamp `i` below `count` whose `far[i]` is above zero and
/// each face `f` in 0–5, it draws every draw whose
/// [`crate::Draw::translucent`] is false into layer `6 i + f` with
/// [`GLSL_ES_300_LAMP_SHADOW_VERTEX`] / [`GLSL_ES_300_LAMP_SHADOW_FRAGMENT`],
/// `u_lamp_matrix` = [`FrameLampShadows::face_matrix`]`(i, f)` and
/// `u_lamp_far` = `far[i]`: viewport `size` × `size`, depth cleared to 1.0,
/// `LESS`, no culling. The main pass binds it as `u_lamp_shadow_map` with
/// `u_lamp_shadows` true, `u_lamp_shadow_size` = `size` (a float) and
/// `u_lamp_far[16]` = `far`; the lamp positions are
/// [`crate::FrameLights::local_positions`]. Lamps are fixed in the model,
/// so the faces are redrawn only when the model or options change. At the
/// default 512 texels each lamp takes 6 MiB.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FrameLampShadows {
    /// Each face's side, texels.
    pub size: u32,
    /// How many lamps cast, the first of [`crate::FrameLights::local_count`],
    /// at most [`MAX_SHADOW_CASTERS`].
    pub count: usize,
    /// Each lamp's reach, model units: the depth a face stores is its view
    /// depth over this. Zero for a lamp casting no shadow (`u_lamp_far[16]`).
    pub far: [f32; MAX_SHADOW_CASTERS],
    /// Lamps past the first [`MAX_SHADOW_CASTERS`], lit without a shadow;
    /// for the host to disclose.
    pub dropped: usize,
    positions: [[f64; 3]; MAX_SHADOW_CASTERS],
}

impl FrameLampShadows {
    /// The lamp shadows `options` casts over a model of `bounds` with
    /// `lights`, its own, seen from `camera`; `None` when
    /// [`RenderOptions::shadows`] or [`RenderOptions::lamp_shadows`] is
    /// off or no point or spot light lights the frame.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] for a degenerate camera, as [`crate::render`].
    pub fn for_model(
        camera: &Camera,
        options: &RenderOptions,
        bounds: &Bounds,
        lights: &[crate::ModelLight],
    ) -> Result<Option<Self>, RenderError> {
        let basis = camera.basis()?;
        if !(options.shadows && options.lamp_shadows) {
            return Ok(None);
        }
        let shader = Shader::for_frame(options, lights, &basis)?;
        let lamps = shader.local();
        if lamps.is_empty() {
            return Ok(None);
        }
        let mut out = FrameLampShadows {
            size: face_size(options.lamp_shadow_map_size),
            count: 0,
            far: [0.0; MAX_SHADOW_CASTERS],
            dropped: lamps.len().saturating_sub(MAX_SHADOW_CASTERS),
            positions: [[0.0; 3]; MAX_SHADOW_CASTERS],
        };
        for (i, lamp) in lamps.iter().take(MAX_SHADOW_CASTERS).enumerate() {
            if let (Some(far), Some(position)) = (out.far.get_mut(i), out.positions.get_mut(i)) {
                *far = reach(lamp.position, bounds).map_or(0.0, |r| r as f32);
                *position = lamp.position;
            }
            out.count = i + 1;
        }
        Ok(Some(out))
    }

    /// The depth texture's layer count, six per lamp.
    #[must_use]
    pub fn layers(&self) -> u32 {
        (self.count * FACES) as u32
    }

    /// Face `face` (0–5: +x, −x, +y, −y, +z, −z) of lamp `lamp`: model to
    /// its clip space, column-major; `None` past `count`, past face 5, or
    /// for a lamp casting no shadow.
    #[must_use]
    pub fn face_matrix(&self, lamp: usize, face: usize) -> Option<[f32; 16]> {
        let far = f64::from(*self.far.get(lamp).filter(|_| lamp < self.count)?);
        let position = *self.positions.get(lamp)?;
        (face < FACES && far > 0.0).then(|| face_matrix(position, far, face))
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{GLSL_ES_300_FRAGMENT, ModelLight, ModelLightKind, Projection};

    fn camera() -> Camera {
        Camera {
            eye: [3.0, -4.0, 5.0],
            target: [0.0; 3],
            up: [0.0, 0.0, 1.0],
            projection: Projection::Perspective { fov_y: 40.0 },
        }
    }

    const BOUNDS: Bounds = Bounds {
        min: [-1.0; 3],
        max: [1.0; 3],
    };

    fn lamp(position: [f64; 3]) -> ModelLight {
        ModelLight::new(
            ModelLightKind::Point {
                position,
                attenuation: [1.0, 0.0, 0.0],
            },
            [1.0; 3],
        )
    }

    #[test]
    fn frame_lamp_shadows_follow_the_switches_and_the_lights() {
        let options = RenderOptions::default();
        let lights = [lamp([0.0, 0.0, 3.0]), lamp([2.0, 0.0, 0.0])];
        let on = FrameLampShadows::for_model(&camera(), &options, &BOUNDS, &lights)
            .unwrap()
            .unwrap();
        assert_eq!((on.count, on.layers(), on.size), (2, 12, 512));
        // From (0, 0, 3) the far corners are sqrt(1 + 1 + 16) away.
        assert!((f64::from(on.far[0]) - 18f64.sqrt() * 1.01).abs() < 1e-5);
        assert!(on.face_matrix(1, 5).is_some());
        assert!(on.face_matrix(1, 6).is_none() && on.face_matrix(2, 0).is_none());
        for off in [
            RenderOptions {
                shadows: false,
                ..options
            },
            RenderOptions {
                lamp_shadows: false,
                ..options
            },
        ] {
            let got = FrameLampShadows::for_model(&camera(), &off, &BOUNDS, &lights).unwrap();
            assert_eq!(got, None);
        }
        let none = FrameLampShadows::for_model(&camera(), &options, &BOUNDS, &[]).unwrap();
        assert_eq!(none, None);
    }

    #[test]
    fn lamps_past_the_caster_limit_light_without_a_shadow_on_both_paths() {
        let lights: Vec<ModelLight> = (0..MAX_SHADOW_CASTERS + 4)
            .map(|i| lamp([0.1 * i as f64, 0.0, 3.0]))
            .collect();
        let options = RenderOptions::default();
        let gpu = FrameLampShadows::for_model(&camera(), &options, &BOUNDS, &lights)
            .unwrap()
            .unwrap();
        assert_eq!((gpu.count, gpu.dropped), (MAX_SHADOW_CASTERS, 4));
        let basis = camera().basis().unwrap();
        let shader = Shader::for_frame(&options, &lights, &basis).unwrap();
        let cpu = crate::shadow_set::lamp_casters(&shader);
        assert_eq!(cpu.len(), MAX_SHADOW_CASTERS);
        // Every lamp still lights the GPU frame.
        let drawn = crate::FrameLights::for_model(&camera(), &options, &lights).unwrap();
        assert_eq!(drawn.local_count, MAX_SHADOW_CASTERS + 4);
    }

    #[test]
    fn the_shaders_declare_every_lamp_shadow_uniform() {
        assert!(GLSL_ES_300_LAMP_SHADOW_VERTEX.contains(" u_lamp_matrix;"));
        assert!(GLSL_ES_300_LAMP_SHADOW_FRAGMENT.contains(" u_lamp_far;"));
        for name in [
            "u_lamp_shadows",
            "u_lamp_shadow_map",
            "u_lamp_shadow_size",
            "u_lamp_far[16]",
        ] {
            assert!(
                GLSL_ES_300_FRAGMENT.contains(&format!(" {name};")),
                "{name}"
            );
        }
    }
}

//! Per-frame GPU shadow inputs and the depth pass's reference shaders: the
//! CPU path's caster, light view and map lookup ([`crate::shadow`]), so both
//! paths shadow the same texels.

use crate::lighting::Shader;
use crate::render::{Bounds, Camera, RenderError, RenderOptions};
use crate::shadow::{Caster, CasterKind, LightView, SHADOW_DIM};

/// The depth pass's vertex shader: location 0 position (vec3) from
/// [`crate::DrawData::vertices`], the rest of the vertex ignored; uniform
/// `u_shadow_matrix` ([`FrameShadow::matrix`]).
pub const GLSL_ES_300_SHADOW_VERTEX: &str = r"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
uniform mat4 u_shadow_matrix;

void main() {
    gl_Position = u_shadow_matrix * vec4(a_position, 1.0);
}
";

/// The depth pass's fragment shader: depth only.
pub const GLSL_ES_300_SHADOW_FRAGMENT: &str = r"#version 300 es
precision highp float;

void main() {}
";

/// One frame's shadow map: what a host renders before the main pass and the
/// main shaders' shadow uniforms.
///
/// The host allocates a `size` × `size` `DEPTH_COMPONENT32F` texture
/// (`NEAREST` both ways, no compare mode), and before the main pass draws
/// every draw whose [`crate::Draw::translucent`] is false into it with
/// [`GLSL_ES_300_SHADOW_VERTEX`] / [`GLSL_ES_300_SHADOW_FRAGMENT`]: viewport
/// `size` × `size`, depth cleared to 1.0, `LESS`, no culling, default depth
/// range. The main pass then binds it as `u_shadow_map` with `u_shadows`
/// true and the uniforms named on each field. A light in
/// [`crate::LightFrame::Camera`] moves with the eye, so the map is redrawn
/// whenever the camera moves; in [`crate::LightFrame::World`], or with
/// [`crate::ShadowLight::Sun`], only when the model or options change.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FrameShadow {
    /// Model to the light's clip space, column-major (`u_shadow_matrix`).
    /// Orthographic over the model's bounding sphere.
    pub matrix: [f32; 16],
    /// The map's side, texels (`u_shadow_size`, as a float).
    pub size: u32,
    /// The casting light, an index into [`crate::FrameLights::directions`];
    /// −1 when no scheme light casts and the whole shade is scaled by
    /// `dim` in shadow instead (`u_shadow_light`).
    pub light: i32,
    /// The share of the shade kept in shadow when `light` is −1
    /// (`u_shadow_dim`).
    pub dim: f32,
    /// Unit, model space, towards the casting light (`u_shadow_to_light`).
    pub to_light: [f32; 3],
    /// How far a point moves along its face normal, turned to the light,
    /// before the lookup, model units (`u_shadow_offset`).
    pub normal_offset: f32,
    /// Subtracted from a point's map depth (0–1) before comparing
    /// (`u_shadow_bias`).
    pub bias: f32,
}

impl FrameShadow {
    /// The shadow `options` casts over a model of `bounds` seen from
    /// `camera`; `None` when [`RenderOptions::shadows`] is off, the scheme
    /// is unlit, or the model has no extent.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] for a degenerate camera, as [`crate::render`].
    pub fn new(
        camera: &Camera,
        options: &RenderOptions,
        bounds: &Bounds,
    ) -> Result<Option<Self>, RenderError> {
        Self::for_model(camera, options, bounds, &[])
    }

    /// As [`FrameShadow::new`] with `lights`, a model's own, under
    /// [`crate::LightingScheme::Artwork`]: its brightest directional light
    /// is the key, as in [`crate::render_model`]; with none, the shadow key.
    /// The others are [`crate::FrameExtraShadows`].
    ///
    /// # Errors
    ///
    /// As [`FrameShadow::new`].
    pub fn for_model(
        camera: &Camera,
        options: &RenderOptions,
        bounds: &Bounds,
        lights: &[crate::ModelLight],
    ) -> Result<Option<Self>, RenderError> {
        let basis = camera.basis()?;
        if !options.shadows {
            return Ok(None);
        }
        let shader = Shader::for_frame(options, lights, &basis);
        let Some(caster) =
            Caster::for_light(options.shadow_light, &shader, options.light_sense, &basis)
        else {
            return Ok(None);
        };
        let Some(view) = LightView::new(caster.to_light, bounds, options.shadow_map_size) else {
            return Ok(None);
        };
        Ok(Some(Self::of(&caster, &view)))
    }

    fn of(caster: &Caster, view: &LightView) -> Self {
        let (light, dim) = match caster.kind {
            CasterKind::Light(i) => (i32::try_from(i).unwrap_or(-1), 0.0),
            CasterKind::Dim => (-1, SHADOW_DIM as f32),
        };
        FrameShadow {
            matrix: view.matrix(),
            size: view.size,
            light,
            dim,
            to_light: caster.to_light.map(|c| c as f32),
            normal_offset: view.normal_offset() as f32,
            bias: view.bias() as f32,
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{GLSL_ES_300_FRAGMENT, GLSL_ES_300_VERTEX, LightingScheme, Projection};

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

    #[test]
    fn frame_shadow_follows_the_switch_and_the_scheme() {
        let off = RenderOptions {
            shadows: false,
            ..RenderOptions::default()
        };
        assert_eq!(FrameShadow::new(&camera(), &off, &BOUNDS).unwrap(), None);
        let unlit = RenderOptions {
            lighting: LightingScheme::None,
            ..RenderOptions::default()
        };
        assert_eq!(FrameShadow::new(&camera(), &unlit, &BOUNDS).unwrap(), None);
        let artwork = FrameShadow::new(&camera(), &RenderOptions::default(), &BOUNDS)
            .unwrap()
            .unwrap();
        assert_eq!((artwork.light, artwork.dim), (-1, 0.5));
        assert_eq!(artwork.size, crate::DEFAULT_SHADOW_MAP_SIZE);
        let day = RenderOptions {
            lighting: LightingScheme::Day,
            shadow_map_size: 100_000,
            ..RenderOptions::default()
        };
        let day = FrameShadow::new(&camera(), &day, &BOUNDS).unwrap().unwrap();
        assert_eq!((day.light, day.size), (1, crate::MAX_SHADOW_MAP_SIZE));
    }

    #[test]
    fn the_shaders_declare_every_shadow_uniform() {
        assert!(GLSL_ES_300_SHADOW_VERTEX.contains(" u_shadow_matrix;"));
        for name in ["u_shadows", "u_shadow_light", "u_shadow_dim"] {
            assert!(GLSL_ES_300_VERTEX.contains(&format!(" {name};")), "{name}");
        }
        for name in [
            "u_shadows",
            "u_shadow_matrix",
            "u_shadow_map",
            "u_shadow_size",
            "u_shadow_to_light",
            "u_shadow_offset",
            "u_shadow_bias",
        ] {
            assert!(
                GLSL_ES_300_FRAGMENT.contains(&format!(" {name};")),
                "{name}"
            );
        }
    }

    #[test]
    fn a_sun_frame_shadow_is_the_same_from_every_camera() {
        let options = RenderOptions {
            lighting: LightingScheme::Cad,
            shadow_light: crate::ShadowLight::Sun {
                up: crate::UpAxis::Z,
                negative: false,
            },
            ..RenderOptions::default()
        };
        let other = Camera {
            eye: [-4.0, 4.0, 2.0],
            ..camera()
        };
        let a = FrameShadow::new(&camera(), &options, &BOUNDS)
            .unwrap()
            .unwrap();
        assert_eq!(
            Some(a),
            FrameShadow::new(&other, &options, &BOUNDS).unwrap()
        );
        assert_eq!((a.light, a.dim), (-1, SHADOW_DIM as f32));
    }
}

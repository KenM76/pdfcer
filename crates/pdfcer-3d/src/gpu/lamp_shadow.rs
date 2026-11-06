//! Per-frame GPU lamp shadow inputs and their depth pass's reference
//! shaders: the CPU path's cube faces ([`crate::shadow::lamp`]), so both paths
//! shadow a model's point and spot lights with the same texels.

use crate::camera::Camera;
use crate::gpu::frame::MAX_LAMP_SHADOWS;
use crate::render::{Bounds, RenderError, RenderOptions};
use crate::shadow::lamp::{FACES, face_matrix, face_size, reach};
use crate::shadow::lamp_choice::{NO_SLOTS, Slots};

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
/// mode). For each slot `i` below `count` whose `far[i]` is above zero and
/// each face `f` in 0–5, it draws every draw whose
/// [`crate::Draw::translucent`] is false into layer `6 i + f` with
/// [`GLSL_ES_300_LAMP_SHADOW_VERTEX`] / [`GLSL_ES_300_LAMP_SHADOW_FRAGMENT`],
/// `u_lamp_matrix` = [`FrameLampShadows::face_matrix`]`(i, f)` and
/// `u_lamp_far` = `far[i]`: viewport `size` × `size`, depth cleared to 1.0,
/// `LESS`, no culling. The main pass binds it as `u_lamp_shadow_map` with
/// `u_lamp_shadows` true, `u_lamp_shadow_size` = `size` (a float) and
/// `u_lamp_far[16]` = `far`; each lamp's row of
/// [`crate::FrameLights::light_table`] names its slot
/// ([`crate::FrameLights::local_slots`]). A slot holds a lamp only while
/// its reach meets the view and it ranks near the top
/// ([`FrameLampShadows::for_model_after`]), so redraw the faces whenever
/// this value changes (it is `PartialEq`), not only when the model or
/// options do. At the default 512 texels each slot takes 6 MiB.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FrameLampShadows {
    /// Each face's side, texels.
    pub size: u32,
    /// The slots in use, at most [`MAX_LAMP_SHADOWS`]: one past the
    /// highest slot holding a lamp.
    pub count: usize,
    /// Each slot's lamp's reach, model units: the depth a face stores is its
    /// view depth over this. Zero for a slot casting no shadow
    /// (`u_lamp_far[16]`).
    pub far: [f32; MAX_LAMP_SHADOWS],
    /// Each slot's lamp, by its index among the model's point and spot
    /// lights in model order; `None` for an empty slot.
    pub lamps: [Option<usize>; MAX_LAMP_SHADOWS],
    /// The frame's lamps lit without a shadow, for the host to disclose:
    /// `would_cast - casting`.
    pub dropped: usize,
    /// The frame's lamps that would cast with no cap
    /// ([`crate::LampStats::would_cast`]).
    pub would_cast: usize,
    /// Those casting, the slots holding a lamp
    /// ([`crate::LampStats::casting`]).
    pub casting: usize,
    positions: [[f64; 3]; MAX_LAMP_SHADOWS],
}

impl FrameLampShadows {
    /// The lamp shadows `options` casts over a model of `bounds` with
    /// `lights`, its own, seen from `camera`; `None` when
    /// [`RenderOptions::shadows`] or [`RenderOptions::lamp_shadows`] is
    /// off or no point or spot light lights the frame. Its slots are
    /// [`crate::FrameLights::local_slots`] for the same arguments, and the
    /// CPU path's ([`crate::render_model`]): the [`MAX_LAMP_SHADOWS`] lamps
    /// adding most at the camera target, brightest first.
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
        Self::for_model_after(camera, options, bounds, lights, None)
    }

    /// [`FrameLampShadows::for_model`] for a live view, `previous` being
    /// the frame before's: a lamp casting there keeps its slot while it
    /// ranks within the top [`MAX_LAMP_SHADOWS`] and a margin of 4, so two
    /// lamps of nearly equal claim do not trade shadows (and cube redraws)
    /// on every small camera move. Pass the result to
    /// [`crate::FrameLights::use_lamp_shadows`]. The picture can then
    /// differ from [`crate::render_model`]'s, which always chooses afresh.
    ///
    /// # Errors
    ///
    /// As [`FrameLampShadows::for_model`].
    pub fn for_model_after(
        camera: &Camera,
        options: &RenderOptions,
        bounds: &Bounds,
        lights: &[crate::ModelLight],
        previous: Option<&FrameLampShadows>,
    ) -> Result<Option<Self>, RenderError> {
        camera.basis()?;
        if !(options.shadows && options.lamp_shadows) {
            return Ok(None);
        }
        let options = crate::render::mode::frame_options(options);
        let previous: &Slots = previous.map_or(&NO_SLOTS, |p| &p.lamps);
        let (shader, stats) = crate::lighting::lamp_view::frame_shader_after(
            &options, lights, camera, None, previous,
        )?;
        let lamps = shader.local();
        if lamps.is_empty() {
            return Ok(None);
        }
        let mut out = FrameLampShadows {
            size: face_size(options.lamp_shadow_map_size),
            count: 0,
            far: [0.0; MAX_LAMP_SHADOWS],
            lamps: NO_SLOTS,
            dropped: stats.would_cast - stats.casting,
            would_cast: stats.would_cast,
            casting: stats.casting,
            positions: [[0.0; 3]; MAX_LAMP_SHADOWS],
        };
        for (lamp, i) in lamps.iter().filter_map(|l| Some((l, l.slot?))) {
            if let (Some(far), Some(position), Some(m)) = (
                out.far.get_mut(i),
                out.positions.get_mut(i),
                out.lamps.get_mut(i),
            ) {
                *far = reach(lamp.position, bounds).map_or(0.0, |r| r as f32);
                *position = lamp.position;
                *m = Some(lamp.model);
            }
            out.count = out.count.max(i + 1);
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
        // Lamp 1, 2 from the target, outranks lamp 0, 3 from it.
        assert_eq!(on.lamps[..3], [Some(1), Some(0), None]);
        // From (0, 0, 3) the far corners are sqrt(1 + 1 + 16) away.
        assert!((f64::from(on.far[1]) - 18f64.sqrt() * 1.01).abs() < 1e-5);
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
        // Lamp i stands 0.1 (19 - i) off the target's axis: the last 16
        // are the nearest, so they cast, brightest (lamp 19) in slot 0.
        let n = MAX_LAMP_SHADOWS + 4;
        let lights: Vec<ModelLight> = (0..n)
            .map(|i| lamp([0.1 * (n - 1 - i) as f64, 0.0, 3.0]))
            .collect();
        let options = RenderOptions::default();
        let gpu = FrameLampShadows::for_model(&camera(), &options, &BOUNDS, &lights)
            .unwrap()
            .unwrap();
        assert_eq!((gpu.count, gpu.dropped), (MAX_LAMP_SHADOWS, 4));
        assert_eq!((gpu.would_cast, gpu.casting), (n, MAX_LAMP_SHADOWS));
        let want: Vec<Option<usize>> = (4..n).rev().map(Some).collect();
        assert_eq!(gpu.lamps.to_vec(), want);
        let (shader, stats) = crate::lighting::lamp_view::frame_shader(
            &crate::render::mode::frame_options(&options),
            &lights,
            &camera(),
            None,
        )
        .unwrap();
        assert_eq!((stats.would_cast, stats.casting), (n, MAX_LAMP_SHADOWS));
        // The CPU path casts from the same lamps, in the same slots.
        let cpu = crate::shadow::set::lamp_casters(&shader);
        let cpu: Vec<Option<usize>> = cpu.iter().map(|l| l.map(|l| l.model)).collect();
        let expect: Vec<Option<usize>> = (0..n).map(|m| (m >= 4).then_some(m)).collect();
        assert_eq!(cpu, expect);
        // Every lamp still lights the GPU frame, its row naming its slot.
        let drawn = crate::FrameLights::for_model(&camera(), &options, &lights).unwrap();
        assert_eq!(drawn.local_count, n);
        let slots: Vec<Option<usize>> = (0..n).map(|m| (m >= 4).then(|| n - 1 - m)).collect();
        assert_eq!(drawn.local_slots, slots);
        let off = RenderOptions {
            lamp_shadows: false,
            ..options
        };
        let none = crate::LampStats::for_model(&camera(), &off, &lights).unwrap();
        assert_eq!((none.would_cast, none.casting), (0, 0));
    }

    #[test]
    fn a_live_view_keeps_a_casting_lamp_within_the_margin() {
        // 21 lamps; the camera target moves so lamp 0, first, ranks 18th.
        let n = MAX_LAMP_SHADOWS + 5;
        let lights: Vec<ModelLight> = (0..n).map(|i| lamp([0.1 * i as f64, 0.0, 3.0])).collect();
        let options = RenderOptions::default();
        let fresh = |camera: &Camera| {
            FrameLampShadows::for_model(camera, &options, &BOUNDS, &lights)
                .unwrap()
                .unwrap()
        };
        let before = fresh(&camera());
        assert_eq!(before.lamps[0], Some(0));
        let moved = Camera {
            target: [0.1 * (n - 1) as f64, 0.0, 0.0],
            ..camera()
        };
        let after =
            FrameLampShadows::for_model_after(&moved, &options, &BOUNDS, &lights, Some(&before))
                .unwrap()
                .unwrap();
        // Ranked from the moved target lamp 0 is 21st, past 16 + 4: a
        // fresh choice leaves it out, and so does the live one.
        assert!(!fresh(&moved).lamps.contains(&Some(0)));
        assert!(!after.lamps.contains(&Some(0)));
        // Lamp 3, 18th, keeps its slot live but not fresh.
        assert_eq!(after.lamps[3], Some(3));
        assert!(!fresh(&moved).lamps.contains(&Some(3)));
        assert_eq!(after.casting, MAX_LAMP_SHADOWS);
        // The light table follows the live choice.
        let mut table = crate::FrameLights::for_model(&moved, &options, &lights).unwrap();
        table.use_lamp_shadows(&after);
        assert_eq!(table.local_slots[3], Some(3));
        let fresh_table = crate::FrameLights::for_model(&moved, &options, &lights).unwrap();
        assert_eq!(fresh_table.local_slots[3], None);
        let mut same = fresh_table.clone();
        same.use_lamp_shadows(&fresh(&moved));
        assert_eq!(same.local_slots, fresh_table.local_slots);
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

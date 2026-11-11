//! [`RenderOptions::light_gain`]: one multiplier on every light of a frame.

use crate::lighting::Shader;
use crate::render::{RenderError, RenderOptions};

/// The smallest [`RenderOptions::light_gain`] drawn.
pub const MIN_LIGHT_GAIN: f64 = 0.25;

/// The largest [`RenderOptions::light_gain`] drawn.
pub const MAX_LIGHT_GAIN: f64 = 4.0;

/// `options.light_gain`, refused when not finite or outside
/// [`MIN_LIGHT_GAIN`]–[`MAX_LIGHT_GAIN`].
pub(crate) fn checked_gain(options: &RenderOptions) -> Result<f64, RenderError> {
    let g = options.light_gain;
    if g.is_finite() && (MIN_LIGHT_GAIN..=MAX_LIGHT_GAIN).contains(&g) {
        Ok(g)
    } else {
        Err(RenderError::LightGain)
    }
}

impl Shader {
    /// Every light and the ambient times `gain`.
    pub(crate) fn gained(self, gain: f64) -> Shader {
        match self {
            Shader::Headlight { gain: g } => Shader::Headlight { gain: g * gain },
            Shader::Unlit => Shader::Unlit,
            Shader::Set {
                ambient,
                lights,
                mut local,
            } => {
                for l in &mut local {
                    l.colour = l.colour.map(|c| c * gain);
                }
                Shader::Set {
                    ambient: ambient.map(|c| c * gain),
                    lights: lights
                        .into_iter()
                        .map(|(d, c)| (d, c.map(|v| v * gain)))
                        .collect(),
                    local,
                }
            }
        }
    }
}

impl PartialEq for RenderOptions {
    /// Equal when every field is; [`RenderOptions::light_gain`] compared by
    /// its bits, so the type stays [`Eq`].
    fn eq(&self, other: &Self) -> bool {
        let RenderOptions {
            width,
            height,
            background,
            colour,
            lighting,
            artwork_headlight,
            light_frame,
            threads,
            shadows,
            light_sense,
            shadow_light,
            shadow_map_size,
            ambient_occlusion,
            lamp_shadows,
            lamp_shadow_map_size,
            lamp_cut,
            lamp_cost,
            shadow_casters,
            shadow_alpha,
            shininess,
            line_width,
            render_mode,
            light_gain,
            indirect,
        } = self;
        (
            width,
            height,
            background,
            colour,
            lighting,
            artwork_headlight,
            light_frame,
            threads,
            shadows,
            light_sense,
        ) == (
            &other.width,
            &other.height,
            &other.background,
            &other.colour,
            &other.lighting,
            &other.artwork_headlight,
            &other.light_frame,
            &other.threads,
            &other.shadows,
            &other.light_sense,
        ) && (
            shadow_light,
            shadow_map_size,
            ambient_occlusion,
            lamp_shadows,
            lamp_shadow_map_size,
            (lamp_cut, lamp_cost),
            shadow_casters,
            shadow_alpha,
            shininess,
            line_width,
            render_mode,
        ) == (
            &other.shadow_light,
            &other.shadow_map_size,
            &other.ambient_occlusion,
            &other.lamp_shadows,
            &other.lamp_shadow_map_size,
            (&other.lamp_cut, &other.lamp_cost),
            &other.shadow_casters,
            &other.shadow_alpha,
            &other.shininess,
            &other.line_width,
            &other.render_mode,
        ) && light_gain.to_bits() == other.light_gain.to_bits()
            && *indirect == other.indirect
    }
}

impl Eq for RenderOptions {}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{
        AssembledModel, Camera, FrameLights, LightingScheme, ModelLight, ModelLightKind,
        Projection, ShadeMode, TriangleMesh, render, render_model,
    };

    /// A tilted square, so its shade varies, over a floor it shadows.
    fn meshes() -> Vec<TriangleMesh> {
        let quad = |p: [[f64; 3]; 4]| TriangleMesh {
            positions: p.to_vec(),
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            ..TriangleMesh::default()
        };
        vec![
            quad([
                [-2.0, -2.0, 0.0],
                [2.0, -2.0, 0.0],
                [2.0, 2.0, 0.0],
                [-2.0, 2.0, 0.0],
            ]),
            quad([
                [-0.5, -0.5, 0.8],
                [0.5, -0.5, 1.2],
                [0.5, 0.5, 1.2],
                [-0.5, 0.5, 0.8],
            ]),
        ]
    }

    fn camera() -> Camera {
        Camera {
            eye: [2.0, -3.0, 8.0],
            target: [0.0; 3],
            up: [0.0, 0.0, 1.0],
            projection: Projection::Perspective { fov_y: 40.0 },
        }
    }

    fn options(lighting: LightingScheme, gain: f64) -> RenderOptions {
        RenderOptions {
            width: 64,
            height: 48,
            colour: [60, 70, 80],
            lighting,
            light_gain: gain,
            ..RenderOptions::default()
        }
    }

    fn sum(rgba: &[u8]) -> u64 {
        rgba.iter().map(|&c| u64::from(c)).sum()
    }

    /// FNV-1a over the frame of every scheme.
    fn digest(gain: f64) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for scheme in LightingScheme::ALL {
            let image = render(&meshes(), &camera(), &options(scheme, gain)).unwrap();
            for b in image.rgba {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        }
        h
    }

    /// [`digest`] of the tree before `light_gain` existed, with the far side
    /// of a face taking no casting light.
    const DIGEST_BEFORE_THE_GAIN: u64 = 0xa4e6_ef80_a0ca_b953;

    #[test]
    fn a_gain_of_one_draws_every_scheme_as_before_the_gain() {
        assert_eq!(digest(1.0), DIGEST_BEFORE_THE_GAIN);
    }

    #[test]
    fn a_gain_of_two_brightens_every_lit_scheme() {
        for scheme in LightingScheme::ALL {
            let at = |g| {
                sum(&render(&meshes(), &camera(), &options(scheme, g))
                    .unwrap()
                    .rgba)
            };
            if scheme == LightingScheme::None {
                assert_eq!(at(1.0), at(2.0), "unlit draws no light");
            } else {
                assert!(at(2.0) > at(1.0), "{scheme:?}");
                assert!(at(0.5) < at(1.0), "{scheme:?}");
            }
        }
    }

    fn sun_and_lamp() -> Vec<ModelLight> {
        let sun = ModelLight::new(
            ModelLightKind::Directional {
                direction: [0.0, 0.0, -1.0],
            },
            [0.2, 0.25, 0.3],
        );
        let lamp = ModelLight::new(
            ModelLightKind::Point {
                position: [0.0, 0.0, 3.0],
                attenuation: [1.0, 0.0, 0.0],
            },
            [0.1, 0.1, 0.05],
        );
        let mut fill = ModelLight::new(ModelLightKind::Ambient, [0.05; 3]);
        fill.ambient = [0.05; 3];
        vec![sun, lamp, fill]
    }

    #[test]
    fn a_gain_of_two_doubles_a_model_lights_share() {
        let basis = camera().basis().unwrap();
        let lights = sun_and_lamp();
        let at = |gain, artwork_headlight| {
            let options = RenderOptions {
                artwork_headlight,
                ..options(LightingScheme::Artwork, gain)
            };
            Shader::for_frame(&options, &lights, &basis).unwrap().shade(
                [0.2, 0.1, 0.0],
                [0.3, 0.2, 0.9],
                Some([0.0, 0.0, 1.0]),
            )
        };
        for headlight in [false, true] {
            let (one, two) = (at(1.0, headlight), at(2.0, headlight));
            for (a, b) in one.iter().zip(two) {
                assert!(*a > 0.0 && (b - 2.0 * a).abs() < 1e-12, "{one:?} {two:?}");
            }
        }
        let model = AssembledModel {
            meshes: meshes(),
            lights: lights.clone(),
            ..AssembledModel::default()
        };
        let draw = |g| {
            let o = options(LightingScheme::Artwork, g);
            sum(&render_model(&model, &camera(), &o).unwrap().rgba)
        };
        assert!(draw(2.0) > draw(1.0));
    }

    #[test]
    fn frame_lights_carry_the_gain_into_the_uniforms() {
        let camera = camera();
        let mut lights = sun_and_lamp();
        lights[0].specular = Some([0.4; 3]);
        lights[1].specular = Some([0.3; 3]);
        let frame = |scheme, gain, lights: &[ModelLight]| {
            FrameLights::for_model(&camera, &options(scheme, gain), lights).unwrap()
        };
        let double = |a: &[[f32; 3]], b: &[[f32; 3]]| {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                for (a, b) in a.iter().zip(b) {
                    assert!((2.0 * a - b).abs() < 1e-6, "{a} {b}");
                }
            }
        };
        for (scheme, lights) in [
            (LightingScheme::Artwork, &lights[..]),
            (LightingScheme::Artwork, &[][..]),
            (LightingScheme::Hard, &[][..]),
            (LightingScheme::Cad, &[][..]),
        ] {
            let (one, two) = (frame(scheme, 1.0, lights), frame(scheme, 2.0, lights));
            double(&[one.ambient], &[two.ambient]);
            double(&one.colours, &two.colours);
            double(&one.specular, &two.specular);
            double(&one.local_colours, &two.local_colours);
            double(&one.local_specular, &two.local_specular);
            assert!(one.light_table() != two.light_table(), "{scheme:?}");
        }
        // The headlight's uniforms: at gain 1, the constants its shader
        // lines drew with.
        let h = frame(LightingScheme::Artwork, 1.0, &[]);
        assert_eq!((h.mode, h.count), (ShadeMode::Headlight, 1));
        assert_eq!(
            (h.ambient, h.colours[0], h.specular[0]),
            ([0.3; 3], [0.7; 3], [0.7; 3])
        );
        assert_eq!(h.directions[0], [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn a_gain_out_of_range_is_refused_everywhere() {
        let lights = sun_and_lamp();
        for gain in [f64::NAN, f64::INFINITY, -1.0, 0.0, 0.249, 4.001] {
            let o = options(LightingScheme::Day, gain);
            assert_eq!(
                render(&meshes(), &camera(), &o).err(),
                Some(RenderError::LightGain)
            );
            let o = options(LightingScheme::Artwork, gain);
            assert_eq!(
                FrameLights::for_model(&camera(), &o, &lights).err(),
                Some(RenderError::LightGain)
            );
        }
        for gain in [MIN_LIGHT_GAIN, MAX_LIGHT_GAIN] {
            let o = options(LightingScheme::Day, gain);
            assert!(render(&meshes(), &camera(), &o).is_ok());
        }
    }

    #[test]
    fn the_gain_is_compared_by_its_bits() {
        let a = options(LightingScheme::Day, 1.0);
        assert_eq!(a, a);
        assert_ne!(a, options(LightingScheme::Day, 2.0));
        let nan = options(LightingScheme::Day, f64::NAN);
        assert_eq!(nan, nan);
    }
}

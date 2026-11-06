//! [`crate::ShadowLight::AimedSun`]: a sun aimed by compass bearing and
//! height, coloured by its temperature. pdfcer's: the standard defines no
//! sun.

use std::hash::{Hash, Hasher};

use crate::UpAxis;
use crate::lighting::Shader;
use crate::render::RenderError;

/// The coolest [`SunColour::Kelvin`] drawn.
pub const MIN_SUN_KELVIN: f64 = 1000.0;
/// The hottest [`SunColour::Kelvin`] drawn.
pub const MAX_SUN_KELVIN: f64 = 40000.0;

/// A sun fixed in the model ([`crate::ShadowLight::AimedSun`]). It is added
/// to the lighting scheme as one more directional light, of colour
/// [`SunLight::rgb`] times [`crate::RenderOptions::light_gain`], and it is
/// the key light: it casts the shadow, and where blocked its own share of
/// the shade is removed. Under [`crate::LightingScheme::None`] there is no
/// sun and no shadow.
///
/// Aim, for `up` [`UpAxis::Z`]: `azimuth_deg` 0 is the model's front (−Y,
/// the side [`crate::NamedView::Front`] looks from), 90 its right (+X), 180
/// its back (+Y), 270 its left (−X): counter-clockwise seen from above.
/// `elevation_deg` 0 is the horizon, 90 overhead. Towards the sun is
/// `(sin a·cos e, −cos a·cos e, sin e)`, turned onto `up` by
/// [`UpAxis::orient`]: for Y up the front is +Z and the right +X; for X up
/// the front is −Z and the right +Y. `negative` turns it half a revolution
/// about the axis after `up`, as [`crate::ShadowLight::Sun`]'s.
///
/// Equal, and hashed, by the bits of its numbers.
#[derive(Debug, Clone, Copy)]
pub struct SunLight {
    /// The model's vertical axis.
    pub up: UpAxis,
    /// The model's up is the negative of `up`.
    pub negative: bool,
    /// Compass bearing, degrees; any finite value (taken modulo 360).
    pub azimuth_deg: f64,
    /// Height above the horizon, degrees, 0 to 90.
    pub elevation_deg: f64,
    /// The sun's colour.
    pub colour: SunColour,
    /// The sun's strength, [`crate::MIN_LIGHT_GAIN`] to
    /// [`crate::MAX_LIGHT_GAIN`]; 1 is a white sun of full strength.
    pub gain: f64,
}

/// The colour of a [`SunLight`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum SunColour {
    /// A colour temperature, kelvin, [`MIN_SUN_KELVIN`] to
    /// [`MAX_SUN_KELVIN`]: 6500 is white, lower is warmer.
    Kelvin(f64),
    /// The temperature of a sun at this height:
    /// `2000 + 3500·√(sin e)` kelvin, 2000 K on the horizon rising
    /// steadily to 5500 K overhead.
    FromElevation,
}

impl SunLight {
    /// The colour temperature drawn, kelvin.
    #[must_use]
    pub fn kelvin(&self) -> f64 {
        match self.colour {
            SunColour::Kelvin(k) => k,
            SunColour::FromElevation => {
                2000.0 + 3500.0 * self.elevation_deg.to_radians().sin().max(0.0).sqrt()
            }
        }
    }

    /// The sun's colour times its `gain`, before
    /// [`crate::RenderOptions::light_gain`]: [`kelvin_rgb`] of
    /// [`SunLight::kelvin`].
    #[must_use]
    pub fn rgb(&self) -> [f64; 3] {
        kelvin_rgb(self.kelvin()).map(|c| c * self.gain)
    }

    /// Unit, model space, from the model towards the sun.
    #[must_use]
    pub fn direction(&self) -> [f64; 3] {
        let (a, e) = (
            self.azimuth_deg.to_radians(),
            self.elevation_deg.to_radians(),
        );
        let v = self
            .up
            .orient([a.sin() * e.cos(), -a.cos() * e.cos(), e.sin()]);
        if self.negative {
            crate::shadow::turned(self.up, v)
        } else {
            v
        }
    }

    /// `Ok` when every number is one the sun draws.
    pub(crate) fn check(&self) -> Result<(), RenderError> {
        let k = match self.colour {
            SunColour::Kelvin(k) => k,
            SunColour::FromElevation => MIN_SUN_KELVIN,
        };
        let within = |v: f64, lo: f64, hi: f64| v.is_finite() && (lo..=hi).contains(&v);
        if !self.azimuth_deg.is_finite() {
            Err(RenderError::Sun(
                "the azimuth must be a finite number of degrees",
            ))
        } else if !within(self.elevation_deg, 0.0, 90.0) {
            Err(RenderError::Sun(
                "the elevation must be from 0 to 90 degrees",
            ))
        } else if !within(k, MIN_SUN_KELVIN, MAX_SUN_KELVIN) {
            Err(RenderError::Sun(
                "the colour temperature must be from 1000 to 40000 K",
            ))
        } else if !within(self.gain, crate::MIN_LIGHT_GAIN, crate::MAX_LIGHT_GAIN) {
            Err(RenderError::Sun("the gain must be from 0.25 to 4"))
        } else {
            Ok(())
        }
    }

    fn bits(&self) -> (UpAxis, bool, [u64; 4], u8) {
        let (tag, k) = match self.colour {
            SunColour::Kelvin(k) => (0, k),
            SunColour::FromElevation => (1, 0.0),
        };
        let n = [self.azimuth_deg, self.elevation_deg, k, self.gain].map(f64::to_bits);
        (self.up, self.negative, n, tag)
    }
}

impl PartialEq for SunLight {
    fn eq(&self, other: &Self) -> bool {
        self.bits() == other.bits()
    }
}

impl Eq for SunLight {}

impl Hash for SunLight {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bits().hash(state);
    }
}

impl PartialEq for SunColour {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (SunColour::Kelvin(a), SunColour::Kelvin(b)) => a.to_bits() == b.to_bits(),
            (SunColour::FromElevation, SunColour::FromElevation) => true,
            _ => false,
        }
    }
}

impl Eq for SunColour {}

impl Hash for SunColour {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            SunColour::Kelvin(k) => (0u8, k.to_bits()).hash(state),
            SunColour::FromElevation => 1u8.hash(state),
        }
    }
}

/// The colour of a black body at `kelvin`, 1 per channel at 6500 K.
///
/// Tanner Helland's fit to the CIE 1964 10° blackbody table (2012): for
/// `t = kelvin / 100`, red is 255 up to `t` 66, then
/// `329.698727446·(t − 60)^−0.1332047592`; green is
/// `99.4708025861·ln t − 161.1195681661` up to 66, then
/// `288.1221695283·(t − 60)^−0.0755148492`; blue is 255 from 66, 0 up to
/// 19, between them `138.5177312231·ln(t − 10) − 305.0447927307`; each
/// clamped to 0–255, then divided by its value at 6500 K. Above 6500 K
/// blue exceeds 1 slightly and red falls.
#[must_use]
pub fn kelvin_rgb(kelvin: f64) -> [f64; 3] {
    let [r0, g0, b0] = helland(6500.0);
    let [r, g, b] = helland(kelvin);
    [r / r0, g / g0, b / b0]
}

fn helland(kelvin: f64) -> [f64; 3] {
    let t = kelvin / 100.0;
    let r = if t <= 66.0 {
        255.0
    } else {
        329.698_727_446 * (t - 60.0).powf(-0.133_204_759_2)
    };
    let g = if t <= 66.0 {
        99.470_802_586_1 * t.ln() - 161.119_568_166_1
    } else {
        288.122_169_528_3 * (t - 60.0).powf(-0.075_514_849_2)
    };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_731_223_1 * (t - 10.0).ln() - 305.044_792_730_7
    };
    [r, g, b].map(|c| c.clamp(0.0, 255.0))
}

impl Shader {
    /// `self` with `sun` added as its last directional light, times
    /// `gain`. A headlight becomes the same light as a set (ambient
    /// 0.3·gain, a camera light 0.7·gain) so the sun can join it; unlit
    /// stays unlit.
    pub(crate) fn with_sun(self, sun: &SunLight, gain: f64) -> Shader {
        let light = (Some(sun.direction()), sun.rgb().map(|c| c * gain));
        match self {
            Shader::Unlit => Shader::Unlit,
            Shader::Headlight { gain: g } => Shader::Set {
                ambient: [0.3 * g; 3],
                lights: vec![(None, [0.7 * g; 3]), light],
                local: Vec::new(),
            },
            Shader::Set {
                ambient,
                mut lights,
                local,
            } => {
                lights.push(light);
                Shader::Set {
                    ambient,
                    lights,
                    local,
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{
        Camera, FrameLights, FrameShadow, LightingScheme, Projection, RenderOptions, ShadowCasters,
        ShadowLight, TriangleMesh, render,
    };

    fn mesh(positions: Vec<[f64; 3]>, triangles: Vec<[u32; 3]>) -> TriangleMesh {
        TriangleMesh {
            positions,
            triangles,
            ..TriangleMesh::default()
        }
    }

    /// A ground square and a unit box standing on its middle.
    fn scene() -> Vec<TriangleMesh> {
        let ground = mesh(
            vec![
                [-4.0, -4.0, 0.0],
                [4.0, -4.0, 0.0],
                [4.0, 4.0, 0.0],
                [-4.0, 4.0, 0.0],
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        );
        let mut p = Vec::new();
        for z in [0.0, 1.0] {
            for (x, y) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
                p.push([x, y, z]);
            }
        }
        let t = vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        vec![ground, mesh(p, t)]
    }

    fn oblique() -> Camera {
        Camera {
            eye: [3.0, -5.0, 8.0],
            target: [0.0; 3],
            up: [0.0, 0.0, 1.0],
            projection: Projection::Perspective { fov_y: 45.0 },
        }
    }

    /// Straight down: image right is +X, image up +Y.
    fn top() -> Camera {
        Camera {
            eye: [0.0, 0.0, 10.0],
            target: [0.0; 3],
            up: [0.0, 1.0, 0.0],
            projection: Projection::Orthographic { height: 8.0 },
        }
    }

    fn sun(azimuth_deg: f64, elevation_deg: f64, colour: SunColour) -> SunLight {
        SunLight {
            up: UpAxis::Z,
            negative: false,
            azimuth_deg,
            elevation_deg,
            colour,
            gain: 1.0,
        }
    }

    fn options(lighting: LightingScheme, shadow_light: ShadowLight) -> RenderOptions {
        RenderOptions {
            width: 64,
            height: 48,
            colour: [60, 70, 80],
            lighting,
            shadow_light,
            shadow_map_size: 256,
            ..RenderOptions::default()
        }
    }

    fn digest(light: ShadowLight) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for scheme in LightingScheme::ALL {
            let image = render(&scene(), &oblique(), &options(scheme, light)).unwrap();
            for b in image.rgba {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        }
        h
    }

    /// [`digest`]s of the tree before the aimed sun existed, with the far
    /// side of a face taking no casting light.
    #[test]
    fn no_aimed_sun_draws_as_before_it() {
        assert_eq!(digest(ShadowLight::FollowsScheme), 0x15ba_6f3d_ac07_197e);
        let fixed = |up, negative| ShadowLight::Sun { up, negative };
        assert_eq!(digest(fixed(UpAxis::Z, false)), 0x4956_5dfe_10e5_620e);
        assert_eq!(digest(fixed(UpAxis::Y, true)), 0xbfb3_7b24_4bf7_d473);
    }

    /// Where `sun` shadows the ground seen from [`top`]: the shadowed
    /// pixels' count and their mean offset from the box, (+X, +Y) pixels.
    fn shadow_of(sun: SunLight) -> (usize, [f64; 2]) {
        let o = RenderOptions {
            width: 160,
            height: 160,
            shadow_casters: ShadowCasters::KeyLight,
            ambient_occlusion: false,
            ..options(LightingScheme::Cad, ShadowLight::AimedSun(sun))
        };
        let lit = render(
            &scene(),
            &top(),
            &RenderOptions {
                shadows: false,
                ..o
            },
        )
        .unwrap();
        let shaded = render(&scene(), &top(), &o).unwrap();
        let (mut n, mut sum) = (0, [0.0; 2]);
        for (i, (a, b)) in lit.rgba.chunks(4).zip(shaded.rgba.chunks(4)).enumerate() {
            if a.iter().zip(b).take(3).any(|(a, b)| b + 8 < *a) {
                n += 1;
                sum[0] += (i % 160) as f64 + 0.5 - 80.0;
                sum[1] += 80.0 - ((i / 160) as f64 + 0.5);
            }
        }
        assert!(n > 0);
        (n, sum.map(|s| s / n as f64))
    }

    #[test]
    fn a_quarter_turn_of_azimuth_turns_the_shadow_a_quarter_turn() {
        let (_, front) = shadow_of(sun(0.0, 40.0, SunColour::FromElevation));
        let (_, right) = shadow_of(sun(90.0, 40.0, SunColour::FromElevation));
        // A sun in front throws the shadow back (+Y); one on the right,
        // left (−X): the first turned a quarter counter-clockwise.
        assert!(front[1] > 5.0 && front[0].abs() < 1.0, "{front:?}");
        let turned = [-front[1], front[0]];
        for (a, b) in turned.iter().zip(right) {
            assert!((a - b).abs() < 1.0, "{turned:?} {right:?}");
        }
    }

    #[test]
    fn a_higher_sun_casts_a_shorter_shadow() {
        let (low, at_low) = shadow_of(sun(30.0, 30.0, SunColour::Kelvin(6500.0)));
        let (high, at_high) = shadow_of(sun(30.0, 60.0, SunColour::Kelvin(6500.0)));
        assert!(high < low, "{high} {low}");
        assert!(at_high[0].hypot(at_high[1]) < at_low[0].hypot(at_low[1]));
    }

    #[test]
    fn a_low_sun_is_warmer_than_a_white_one() {
        assert_eq!(kelvin_rgb(6500.0), [1.0; 3]);
        let ratio = |c: [f64; 3]| c[0] / c[2];
        assert!(ratio(kelvin_rgb(2000.0)) > 4.0 * ratio(kelvin_rgb(6500.0)));
        assert!(ratio(kelvin_rgb(10000.0)) < ratio(kelvin_rgb(6500.0)));
        let drawn = |k| {
            let o = options(
                LightingScheme::Cad,
                ShadowLight::AimedSun(sun(0.0, 60.0, SunColour::Kelvin(k))),
            );
            let rgba = render(&scene(), &oblique(), &o).unwrap().rgba;
            let sum = |j: usize| rgba.chunks(4).map(|p| u64::from(p[j])).sum::<u64>();
            sum(0) as f64 / sum(2) as f64
        };
        assert!(drawn(2000.0) > drawn(6500.0));
        let at = |e| sun(0.0, e, SunColour::FromElevation).kelvin();
        assert_eq!((at(0.0), at(90.0)), (2000.0, 5500.0));
        assert!((0..90).all(|e| at(f64::from(e)) < at(f64::from(e + 1))));
    }

    #[test]
    fn the_sun_is_an_added_key_light_fixed_in_the_model() {
        let s = SunLight {
            gain: 0.5,
            ..sun(120.0, 35.0, SunColour::Kelvin(3000.0))
        };
        let o = RenderOptions {
            light_gain: 2.0,
            ..options(LightingScheme::Day, ShadowLight::AimedSun(s))
        };
        let plain = options(LightingScheme::Day, ShadowLight::FollowsScheme);
        let bounds = crate::Bounds::of(&scene()).unwrap();
        let mut shadows = Vec::new();
        for camera in [oblique(), top()] {
            let with = FrameLights::for_model(&camera, &o, &[]).unwrap();
            let without = FrameLights::for_model(&camera, &plain, &[]).unwrap();
            assert_eq!(with.count, without.count + 1);
            let last = with.count - 1;
            let d = s.direction();
            assert_eq!(
                with.directions[last],
                [d[0] as f32, d[1] as f32, d[2] as f32, 0.0]
            );
            let c = s.rgb().map(|c| (c * 2.0) as f32);
            assert_eq!(with.colours[last], c);
            let shadow = FrameShadow::new(&camera, &o, &bounds).unwrap().unwrap();
            assert_eq!(shadow.light, i32::try_from(last).unwrap());
            shadows.push(shadow);
        }
        // The map need not be redrawn as the camera moves.
        assert_eq!(shadows[0], shadows[1]);
        let unlit = options(LightingScheme::None, ShadowLight::AimedSun(s));
        assert_eq!(FrameShadow::new(&top(), &unlit, &bounds), Ok(None));
    }

    #[test]
    fn the_aim_is_as_documented_on_every_up_axis() {
        let close = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-12);
        let aim = |up, a, e| {
            SunLight {
                up,
                ..sun(a, e, SunColour::FromElevation)
            }
            .direction()
        };
        assert!(close(aim(UpAxis::Z, 0.0, 0.0), [0.0, -1.0, 0.0]));
        assert!(close(aim(UpAxis::Z, 90.0, 0.0), [1.0, 0.0, 0.0]));
        assert!(close(aim(UpAxis::Z, 0.0, 90.0), [0.0, 0.0, 1.0]));
        assert!(close(aim(UpAxis::Y, 0.0, 0.0), [0.0, 0.0, 1.0]));
        assert!(close(aim(UpAxis::Y, 90.0, 0.0), [1.0, 0.0, 0.0]));
        assert!(close(aim(UpAxis::X, 0.0, 0.0), [0.0, 0.0, -1.0]));
        assert!(close(aim(UpAxis::X, 90.0, 0.0), [0.0, 1.0, 0.0]));
        let low = SunLight {
            negative: true,
            ..sun(0.0, 90.0, SunColour::FromElevation)
        };
        assert!(close(low.direction(), [0.0, 0.0, -1.0]));
    }

    #[test]
    fn a_sun_out_of_range_is_refused() {
        let ok = sun(0.0, 45.0, SunColour::FromElevation);
        let bad = [
            SunLight {
                azimuth_deg: f64::INFINITY,
                ..ok
            },
            SunLight {
                elevation_deg: -0.1,
                ..ok
            },
            SunLight {
                elevation_deg: 90.1,
                ..ok
            },
            SunLight {
                elevation_deg: f64::NAN,
                ..ok
            },
            SunLight {
                colour: SunColour::Kelvin(999.0),
                ..ok
            },
            SunLight {
                colour: SunColour::Kelvin(40_001.0),
                ..ok
            },
            SunLight {
                colour: SunColour::Kelvin(f64::NAN),
                ..ok
            },
            SunLight { gain: 0.2, ..ok },
            SunLight {
                gain: f64::NAN,
                ..ok
            },
        ];
        let bounds = crate::Bounds::of(&scene()).unwrap();
        for s in bad {
            let o = options(LightingScheme::Cad, ShadowLight::AimedSun(s));
            let refused = |e: Option<RenderError>| matches!(e, Some(RenderError::Sun(_)));
            assert!(refused(render(&scene(), &top(), &o).err()), "{s:?}");
            assert!(
                refused(FrameLights::for_model(&top(), &o, &[]).err()),
                "{s:?}"
            );
            assert!(
                refused(FrameShadow::new(&top(), &o, &bounds).err()),
                "{s:?}"
            );
        }
        for s in [
            ok,
            SunLight {
                azimuth_deg: -725.0,
                elevation_deg: 0.0,
                ..ok
            },
        ] {
            let o = options(LightingScheme::Cad, ShadowLight::AimedSun(s));
            assert!(render(&scene(), &top(), &o).is_ok());
        }
    }

    #[test]
    fn a_sun_compares_by_its_bits() {
        let a = sun(10.0, 20.0, SunColour::Kelvin(f64::NAN));
        assert_eq!(a, a);
        assert_ne!(a, sun(10.0, 20.0, SunColour::Kelvin(3000.0)));
        assert_ne!(
            sun(0.0, 0.0, SunColour::Kelvin(0.0)),
            sun(0.0, 0.0, SunColour::FromElevation)
        );
        let hash = |s: SunLight| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            ShadowLight::AimedSun(s).hash(&mut h);
            h.finish()
        };
        assert_eq!(hash(a), hash(a));
    }
}

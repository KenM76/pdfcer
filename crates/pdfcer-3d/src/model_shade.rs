//! A model's own lights ([`ModelLight`]) as a [`Shader`], for
//! [`LightingScheme::Artwork`]: the scheme ISO 32000-1 Table 310 defines as
//! "lights specified in the 3D artwork".
//!
//! Lights are in model space whatever [`crate::LightFrame`] says: they are
//! part of the model. A point or spot light fades as
//! `1 / (c + l·d + q·d²)` with `d` the distance to it, and a spot light is
//! cut off outside its half-angle and falls off inside it as
//! `cos(θ)^exponent` (PRC [WD 7.5.16, 7.5.19]; the exponent's reading is
//! OpenGL's).

use crate::lighting::{LightingScheme, Shader};
use crate::model_light::unit;
use crate::render::RenderOptions;
use crate::vec3::{dot, length, sub};
use crate::{ModelLight, ModelLightKind};

/// The headlight [`RenderOptions::artwork_headlight`] adds: the default
/// light's `0.3 + 0.7·|n·v|`.
const HEADLIGHT_AMBIENT: f64 = 0.3;
const HEADLIGHT: f64 = 0.7;

/// A point or spot light, resolved for shading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Local {
    pub(crate) position: [f64; 3],
    /// Colour times intensity.
    pub(crate) colour: [f64; 3],
    pub(crate) attenuation: [f64; 3],
    /// A spot's cone.
    pub(crate) cone: Option<Cone>,
    /// The distance past which the light adds nothing ([`reach_limit`]).
    pub(crate) reach: f64,
}

/// A spot light's cone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Cone {
    /// Unit, the way the light shines.
    pub(crate) direction: [f64; 3],
    /// The cosine of the half-angle; −1 (or less) cuts nothing.
    pub(crate) cos_cutoff: f64,
    pub(crate) exponent: f64,
}

impl Local {
    /// The light's per-channel factor at point `p` with unit normal `n`,
    /// lit on both sides.
    pub(crate) fn at(&self, p: [f64; 3], n: [f64; 3]) -> [f64; 3] {
        let Some((l, reach)) = self.reach(p) else {
            return self.colour;
        };
        let k = reach * dot(n, l).abs();
        self.colour.map(|c| c * k)
    }

    /// The unit direction from `p` to the light and the share of it that
    /// reaches `p` (fade times cone); `None` at the light itself.
    pub(crate) fn reach(&self, p: [f64; 3]) -> Option<([f64; 3], f64)> {
        let to_light = sub(self.position, p);
        let d = length(to_light);
        let l = unit(to_light)?;
        if d > self.reach {
            return Some((l, 0.0));
        }
        let [c, k1, k2] = self.attenuation;
        let denominator = c + k1 * d + k2 * d * d;
        let fade = if denominator > 0.0 && denominator.is_finite() {
            1.0 / denominator
        } else {
            1.0
        };
        let cone = self.cone.map_or(1.0, |cone| {
            let cos = -dot(l, cone.direction);
            if cos < cone.cos_cutoff {
                0.0
            } else if cone.exponent > 0.0 {
                cos.max(0.0).powf(cone.exponent)
            } else {
                1.0
            }
        });
        Some((l, fade * cone))
    }
}

/// Add each of `local` at model point `p` with normal `n` (renormalised
/// here) to `shade`, light `i` scaled by `lit(i)`, its unshadowed share.
pub(crate) fn add_local(
    shade: &mut [f64; 3],
    local: &[Local],
    (p, n): ([f64; 3], [f64; 3]),
    lit: &dyn Fn(usize) -> f64,
) {
    let Some(n) = unit(n) else {
        return;
    };
    for (i, l) in local.iter().enumerate() {
        let k = lit(i);
        if k <= 0.0 {
            continue;
        }
        for (o, c) in shade.iter_mut().zip(l.at(p, n)) {
            *o += k * c;
        }
    }
}

impl Shader {
    /// The shader a frame uses: the model's lights for
    /// [`LightingScheme::Artwork`] when it has any, else the scheme's.
    pub(crate) fn for_frame(
        options: &RenderOptions,
        model: &[ModelLight],
        basis: &[[f64; 3]; 3],
    ) -> Shader {
        if options.lighting == LightingScheme::Artwork && !model.is_empty() {
            return Shader::from_model(model, options.artwork_headlight, options.lamp_cut);
        }
        Shader::new(options.lighting.lights(), options.light_frame, basis)
    }

    fn from_model(model: &[ModelLight], headlight: bool, cut: LampCut) -> Shader {
        let mut ambient = [0.0; 3];
        let mut lights = Vec::new();
        let mut local = Vec::new();
        let add = |to: &mut [f64; 3], c: [f64; 3]| {
            for (o, c) in to.iter_mut().zip(c) {
                *o += c;
            }
        };
        for l in model {
            let colour = l.colour.map(|c| c.max(0.0) * l.intensity.max(0.0));
            add(&mut ambient, l.ambient.map(|c| c.max(0.0)));
            match l.kind {
                ModelLightKind::Ambient => add(&mut ambient, colour),
                ModelLightKind::Directional { direction } => {
                    if let Some(d) = unit(direction) {
                        lights.push((Some(d), colour));
                    }
                }
                ModelLightKind::Point {
                    position,
                    attenuation,
                } => local.push(Local {
                    position,
                    colour,
                    attenuation,
                    cone: None,
                    reach: f64::INFINITY,
                }),
                ModelLightKind::Spot {
                    position,
                    direction,
                    attenuation,
                    angle,
                    exponent,
                } => local.push(Local {
                    position,
                    colour,
                    attenuation,
                    cone: unit(direction).map(|direction| Cone {
                        direction,
                        cos_cutoff: if angle >= std::f64::consts::PI {
                            -2.0
                        } else {
                            angle.cos()
                        },
                        exponent,
                    }),
                    reach: f64::INFINITY,
                }),
            }
        }
        set_reach(&mut local, model, cut);
        if headlight {
            add(&mut ambient, [HEADLIGHT_AMBIENT; 3]);
            lights.push((None, [HEADLIGHT; 3]));
        }
        Shader::Set {
            ambient,
            lights,
            local,
        }
    }
}

/// Where a model's point and spot lights stop
/// ([`RenderOptions::lamp_cut`]). Each lamp stops at the distance where its
/// fade times its brightest channel falls to the cut divided by the
/// model's lamp count, so the lamps stopped remove at most the cut from
/// any point together (for material colours of at most 1), and a fragment
/// skips the lamps out of reach without reading them. Each lamp's distance
/// is [`FrameLights::local_reach`](crate::FrameLights::local_reach).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LampCut {
    /// The cut that changes no pixel, half an 8-bit level
    /// ([`LampCut::AUTO_HUNDREDTHS`]), set for the loaded model's lamps.
    #[default]
    Auto,
    /// The most light to leave out, in hundredths of an 8-bit level: larger
    /// draws a model with many lamps faster, and darker far from them.
    /// 0 stops no lamp.
    Threshold(u16),
    /// Every lamp reaches everywhere.
    Off,
}

impl LampCut {
    /// [`LampCut::Auto`]'s cut: half an 8-bit level.
    pub const AUTO_HUNDREDTHS: u16 = 50;

    /// The most light left out, in hundredths of an 8-bit level; 0 for
    /// [`LampCut::Off`].
    #[must_use]
    pub fn hundredths(self) -> u16 {
        match self {
            LampCut::Auto => LampCut::AUTO_HUNDREDTHS,
            LampCut::Threshold(h) => h,
            LampCut::Off => 0,
        }
    }
}

/// Give each of `local` its [`Local::reach`]: the distance where its
/// fade times its brightest diffuse or specular channel ([`ModelLight::specular`],
/// walked in `model`'s order) falls to its share of `cut` ([`LampCut`]).
fn set_reach(local: &mut [Local], model: &[ModelLight], cut: LampCut) {
    let cut = cut.hundredths();
    if cut == 0 {
        return;
    }
    let floor = f64::from(cut) / (100.0 * 255.0) / local.len().max(1) as f64;
    let specular = model
        .iter()
        .filter(|l| {
            matches!(
                l.kind,
                ModelLightKind::Point { .. } | ModelLightKind::Spot { .. }
            )
        })
        .map(|l| l.specular.map_or(0.0, |s| max3(s) * l.intensity.max(0.0)));
    for (l, s) in local.iter_mut().zip(specular) {
        l.reach = reach_limit(l.attenuation, max3(l.colour).max(s), floor);
    }
}

/// The distance where `bright / (c + l·d + q·d²)` falls to `floor`;
/// infinite when it never does.
pub(crate) fn reach_limit([c, k1, k2]: [f64; 3], bright: f64, floor: f64) -> f64 {
    let k = bright / floor - c;
    let d = if k <= 0.0 {
        0.0
    } else if k2 > 0.0 {
        (-k1 + k1.mul_add(k1, 4.0 * k2 * k).sqrt()) / (2.0 * k2)
    } else if k1 > 0.0 {
        k / k1
    } else {
        f64::INFINITY
    };
    if d.is_nan() { f64::INFINITY } else { d }
}

fn max3(c: [f64; 3]) -> f64 {
    c.iter().fold(0.0, |a, &v| a.max(v))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;

    const BASIS: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]];
    const UP: [f64; 3] = [0.0, 0.0, 1.0];

    fn shader(model: &[ModelLight]) -> Shader {
        Shader::for_frame(&RenderOptions::default(), model, &BASIS)
    }

    fn lamps(shader: Shader) -> Vec<Local> {
        match shader {
            Shader::Set { local, .. } => Some(local),
            _ => None,
        }
        .unwrap()
    }

    #[test]
    fn a_lamps_reach_is_where_its_light_falls_to_the_floor() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9 * b.max(1.0);
        assert!(close(reach_limit([1.0, 0.0, 1.0], 1.0, 0.01), 99f64.sqrt()));
        assert!(close(reach_limit([1.0, 1.0, 0.0], 1.0, 0.01), 99.0));
        assert_eq!(reach_limit([1.0, 0.0, 0.0], 1.0, 0.01), f64::INFINITY);
        assert_eq!(reach_limit([1.0, 1.0, 1.0], 0.001, 0.01), 0.0);
        assert_eq!(reach_limit([0.0; 3], 1.0, 0.01), f64::INFINITY);
    }

    #[test]
    fn the_lamps_past_their_reach_together_remove_under_half_a_level() {
        let model: Vec<ModelLight> = (0..64)
            .map(|i| {
                let position = [f64::from(i) * 10.0, 0.0, 1.0];
                let kind = ModelLightKind::Point {
                    position,
                    attenuation: [0.0, 0.0, 1.0],
                };
                ModelLight::new(kind, [1.0, 0.0, 0.0])
            })
            .collect();
        let local = lamps(shader(&model));
        assert_eq!(local.len(), 64);
        let p = [0.0; 3];
        let mut cut = 0.0;
        for l in &local {
            let (_, k) = l.reach(p).unwrap();
            let d = length(sub(l.position, p));
            if d > l.reach {
                assert_eq!(k, 0.0);
                cut += 1.0 / (d * d);
            } else {
                assert!(k > 0.0);
            }
        }
        assert!(cut > 0.0, "some lamps are cut");
        let reach = |lamp_cut| {
            let options = RenderOptions {
                lamp_cut,
                ..RenderOptions::default()
            };
            lamps(Shader::for_frame(&options, &model, &BASIS))
                .first()
                .unwrap()
                .reach
        };
        assert_eq!(reach(LampCut::Off), f64::INFINITY, "off stops no lamp");
        assert_eq!(reach(LampCut::Threshold(0)), f64::INFINITY);
        assert_eq!(reach(LampCut::Threshold(50)), reach(LampCut::Auto));
        assert!(
            reach(LampCut::Threshold(800)) < local.first().unwrap().reach,
            "a larger cut stops lamps sooner"
        );
        assert!(
            cut < f64::from(LampCut::AUTO_HUNDREDTHS) / 25_500.0,
            "{cut}"
        );
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn a_lamp_fades_with_distance() {
        let lamp = ModelLight::new(
            ModelLightKind::Point {
                position: [0.0, 0.0, 2.0],
                attenuation: [0.0, 0.0, 1.0],
            },
            [1.0, 0.5, 0.0],
        );
        let s = shader(&[lamp]);
        assert!(close(s.shade([0.0; 3], UP, None), [0.25, 0.125, 0.0]));
        // Off to the side: farther, and at a slant.
        let p = [2.0, 0.0, 0.0];
        let d2: f64 = 8.0;
        let slant = 2.0 / d2.sqrt();
        let want = [1.0, 0.5, 0.0].map(|c| c * slant / d2);
        assert!(close(s.shade(p, UP, None), want));
    }

    #[test]
    fn a_spot_is_cut_off_outside_its_cone() {
        let spot = ModelLight::new(
            ModelLightKind::Spot {
                position: [0.0, 0.0, 1.0],
                direction: [0.0, 0.0, -1.0],
                attenuation: [1.0, 0.0, 0.0],
                angle: 0.5,
                exponent: 2.0,
            },
            [1.0; 3],
        );
        let s = shader(&[spot]);
        assert!(close(s.shade([0.0; 3], UP, None), [1.0; 3]));
        // 45 degrees off axis is outside a 0.5 rad cone.
        assert_eq!(s.shade([1.0, 0.0, 0.0], UP, None), [0.0; 3]);
        // 0.3 rad off axis: cos^2 of the angle, times the slant.
        let p = [0.3f64.tan(), 0.0, 0.0];
        let k = 0.3f64.cos().powi(2) * 0.3f64.cos();
        assert!(close(s.shade(p, UP, None), [k; 3]));
    }

    #[test]
    fn ambient_parts_and_intensity_add_up() {
        let mut sun = ModelLight::new(
            ModelLightKind::Directional {
                direction: [0.0, 0.0, -1.0],
            },
            [1.0; 3],
        );
        sun.intensity = 0.5;
        sun.ambient = [0.1; 3];
        let fill = ModelLight::new(ModelLightKind::Ambient, [0.2; 3]);
        let s = shader(&[sun, fill]);
        assert!(close(s.shade([0.0; 3], UP, None), [0.8; 3]));
    }

    #[test]
    fn no_model_lights_keeps_the_headlight_and_the_option_adds_it() {
        assert!(matches!(shader(&[]), Shader::Headlight));
        let fill = ModelLight::new(ModelLightKind::Ambient, [0.2; 3]);
        let options = RenderOptions {
            artwork_headlight: true,
            ..RenderOptions::default()
        };
        let s = Shader::for_frame(&options, std::slice::from_ref(&fill), &BASIS);
        assert!(close(s.shade([0.0; 3], UP, Some(UP)), [1.2; 3]));
        // Another scheme ignores the model's lights.
        let day = RenderOptions {
            lighting: LightingScheme::Day,
            ..RenderOptions::default()
        };
        let s = Shader::for_frame(&day, &[fill], &BASIS);
        assert!(matches!(s, Shader::Set { ref local, .. } if local.is_empty()));
    }
}

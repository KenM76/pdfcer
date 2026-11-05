//! Lights for [`crate::render`]: the reader-supplied lighting schemes of
//! ISO 32000-1 §13.6.4.5 Table 310 (ISO 32000-2 Table 321, same values),
//! and the default light at the camera.
//!
//! Every face is lit on both sides (a producer's winding is not trusted), so
//! a light's direction counts only up to sign: the standard does not say
//! whether its vectors point from the light or towards it.

/// A lighting scheme: the `Subtype` of a `/3DLightingScheme` dictionary
/// (ISO 32000-1 Table 309), as [`crate::RenderOptions::lighting`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum LightingScheme {
    /// `/Artwork`: the model's own lights ([`crate::AssembledModel::lights`]),
    /// drawn by [`crate::render_model`] and [`crate::PreparedModel::render`];
    /// [`Lights::Headlight`] when the model has none, and for the mesh-only
    /// [`crate::render`]. The default.
    #[default]
    Artwork,
    /// `/None`: no lights; every surface shows its own colour, unshaded.
    None,
    /// `/White`: three white lights.
    White,
    /// `/Day`: three daylight lights.
    Day,
    /// `/Night`: three night-coloured lights.
    Night,
    /// `/Hard`: three lights and an ambient term.
    Hard,
    /// `/Primary`: red, green and blue lights.
    Primary,
    /// `/Blue`: three blue lights.
    Blue,
    /// `/Red`: three red lights.
    Red,
    /// `/Cube`: six grey lights along the axes.
    Cube,
    /// `/CAD`: a light at the camera and three more.
    Cad,
    /// `/Headlamp`: one light at the camera and an ambient term.
    Headlamp,
}

/// Which frame a scheme's light directions are given in. ISO 32000 does not
/// say; [`LightFrame::Camera`] keeps the lights fixed to the viewer as a
/// model turns, as conforming readers do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum LightFrame {
    /// Directions are camera axes: x right, y up, z into the scene
    /// (ISO 32000-1 §13.6.5). The default.
    #[default]
    Camera,
    /// Directions are model axes.
    World,
}

/// One infinite (directional) light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    /// DeviceRGB colour, each channel 0–1.
    pub colour: [f64; 3],
    /// The direction, in the scene's [`LightFrame`]; not unit length. Zero
    /// means attached to the camera: it shines along the line of sight, as
    /// [`Lights::Headlight`] does.
    pub direction: [f64; 3],
}

/// A fixed set of lights and an ambient term (Table 310).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightSet {
    /// The lights.
    pub lights: &'static [Light],
    /// Added to every light's contribution, each channel 0–1.
    pub ambient: [f64; 3],
}

/// How a scheme lights a surface of colour `c` with unit normal `n`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Lights {
    /// `c × (0.3 + 0.7·|n·v|)`, where `v` points from the surface to the
    /// eye (perspective) or against the view direction (orthographic).
    Headlight,
    /// `c` unchanged.
    Unlit,
    /// `c × (ambient + Σ colourᵢ·|n·dᵢ|)` per channel, with each `dᵢ` the
    /// light's unit direction (or `v` when it is zero), clamped to 0–255.
    Set(LightSet),
}

const fn light(colour: [f64; 3], direction: [f64; 3]) -> Light {
    Light { colour, direction }
}

const NO_AMBIENT: [f64; 3] = [0.0; 3];
const D1: [f64; 3] = [-2.0, -1.5, -0.5];
const D2: [f64; 3] = [2.0, 1.1, -2.5];
const UP: [f64; 3] = [0.0, 0.0, 2.0];
const CAMERA: [f64; 3] = [0.0; 3];

const WHITE: [Light; 3] = [
    light([0.38, 0.38, 0.45], D1),
    light([0.6, 0.6, 0.67], D2),
    light([0.5, 0.5, 0.57], [-0.5, 0.0, 2.0]),
];
const DAY: [Light; 3] = [
    light([0.5, 0.5, 0.5], D1),
    light([0.8, 0.8, 0.9], D2),
    light([0.9, 0.9, 0.9], [0.02, 0.01, 2.0]),
];
const NIGHT: [Light; 3] = [
    light([1.0, 0.75, 0.39], D1),
    light([0.31, 0.47, 0.55], D2),
    light([0.5, 0.5, 1.0], UP),
];
const HARD: [Light; 3] = [
    light([0.5, 0.5, 0.5], [-1.5, -1.5, -1.5]),
    light([0.8, 0.8, 0.9], [1.5, 1.5, -1.5]),
    light([0.9, 0.9, 0.9], [-0.5, 0.0, 2.0]),
];
const PRIMARY: [Light; 3] = [
    light([1.0, 0.2, 0.5], D1),
    light([0.2, 1.0, 0.5], D2),
    light([0.0, 0.0, 1.0], UP),
];
const BLUE: [Light; 3] = [
    light([0.4, 0.4, 0.7], D1),
    light([0.75, 0.75, 0.95], D2),
    light([0.7, 0.7, 0.95], UP),
];
const RED: [Light; 3] = [
    light([0.8, 0.3, 0.4], D1),
    light([0.95, 0.5, 0.7], D2),
    light([0.95, 0.4, 0.5], UP),
];
const GREY: [f64; 3] = [0.4, 0.4, 0.4];
const CUBE: [Light; 6] = [
    light(GREY, [1.0, 0.01, 0.01]),
    light(GREY, [0.01, 1.0, 0.01]),
    light(GREY, [0.01, 0.01, 1.0]),
    light(GREY, [-1.0, 0.01, 0.01]),
    light(GREY, [0.01, -1.0, 0.01]),
    light(GREY, [0.01, 0.01, -1.0]),
];
const CAD: [Light; 4] = [
    light([0.72, 0.72, 0.81], CAMERA),
    light([0.2, 0.2, 0.2], D1),
    light([0.32, 0.32, 0.32], D2),
    light([0.36, 0.36, 0.36], [0.04, 0.01, 2.0]),
];
const HEADLAMP: [Light; 1] = [light([0.8, 0.8, 0.9], CAMERA)];

impl LightingScheme {
    /// Every scheme, in Table 310's order.
    pub const ALL: [LightingScheme; 12] = [
        LightingScheme::Artwork,
        LightingScheme::None,
        LightingScheme::White,
        LightingScheme::Day,
        LightingScheme::Night,
        LightingScheme::Hard,
        LightingScheme::Primary,
        LightingScheme::Blue,
        LightingScheme::Red,
        LightingScheme::Cube,
        LightingScheme::Cad,
        LightingScheme::Headlamp,
    ];

    /// How [`crate::render`] lights a surface under this scheme.
    ///
    /// ```
    /// use pdfcer_3d::{LightingScheme, Lights};
    /// let Lights::Set(set) = LightingScheme::Headlamp.lights() else { unreachable!() };
    /// assert_eq!(set.ambient, [0.1, 0.1, 0.1]);
    /// assert_eq!(LightingScheme::None.lights(), Lights::Unlit);
    /// ```
    #[must_use]
    pub fn lights(self) -> Lights {
        let set = |lights: &'static [Light], ambient| Lights::Set(LightSet { lights, ambient });
        match self {
            LightingScheme::Artwork => Lights::Headlight,
            LightingScheme::None => Lights::Unlit,
            LightingScheme::White => set(&WHITE, NO_AMBIENT),
            LightingScheme::Day => set(&DAY, NO_AMBIENT),
            LightingScheme::Night => set(&NIGHT, NO_AMBIENT),
            LightingScheme::Hard => set(&HARD, [0.5; 3]),
            LightingScheme::Primary => set(&PRIMARY, NO_AMBIENT),
            LightingScheme::Blue => set(&BLUE, NO_AMBIENT),
            LightingScheme::Red => set(&RED, NO_AMBIENT),
            LightingScheme::Cube => set(&CUBE, NO_AMBIENT),
            LightingScheme::Cad => set(&CAD, NO_AMBIENT),
            LightingScheme::Headlamp => set(&HEADLAMP, [0.1; 3]),
        }
    }

    /// The scheme's PDF name (`/Subtype` value), e.g. `CAD`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            LightingScheme::Artwork => "Artwork",
            LightingScheme::None => "None",
            LightingScheme::White => "White",
            LightingScheme::Day => "Day",
            LightingScheme::Night => "Night",
            LightingScheme::Hard => "Hard",
            LightingScheme::Primary => "Primary",
            LightingScheme::Blue => "Blue",
            LightingScheme::Red => "Red",
            LightingScheme::Cube => "Cube",
            LightingScheme::Cad => "CAD",
            LightingScheme::Headlamp => "Headlamp",
        }
    }
}

/// A scheme resolved against one camera: unit light directions in model
/// space, ready to shade a corner.
pub(crate) enum Shader {
    /// `gain · (0.3 + 0.7·|n·v|)` on every channel.
    Headlight {
        gain: f64,
    },
    Unlit,
    Set {
        ambient: [f64; 3],
        /// Unit direction (`None`: along the line of sight) and colour.
        lights: Vec<(Option<[f64; 3]>, [f64; 3])>,
        /// A model's point and spot lights.
        local: Vec<crate::model_shade::Local>,
    },
}

impl Shader {
    /// Resolves `lights` once per frame: camera-frame directions are turned
    /// into world space through `basis` (right, up, forward).
    pub(crate) fn new(lights: Lights, frame: LightFrame, basis: &[[f64; 3]; 3]) -> Shader {
        use crate::vec3::normalize;
        match lights {
            Lights::Headlight => Shader::Headlight { gain: 1.0 },
            Lights::Unlit => Shader::Unlit,
            Lights::Set(set) => {
                let to_model = |d: [f64; 3]| match frame {
                    LightFrame::World => d,
                    _ => {
                        let [x, y, z] = d;
                        let [r, u, f] = basis;
                        [0, 1, 2].map(|i| {
                            let at = |v: &[f64; 3]| v.get(i).copied().unwrap_or(0.0);
                            x * at(r) + y * at(u) + z * at(f)
                        })
                    }
                };
                Shader::Set {
                    ambient: set.ambient,
                    lights: set
                        .lights
                        .iter()
                        .map(|l| (normalize(to_model(l.direction)), l.colour))
                        .collect(),
                    local: Vec::new(),
                }
            }
        }
    }

    /// The per-channel factor at model point `p` for a surface with unit
    /// normal `normal`; `towards` points to the eye.
    #[cfg(test)]
    pub(crate) fn shade(
        &self,
        p: [f64; 3],
        normal: [f64; 3],
        towards: Option<[f64; 3]>,
    ) -> [f64; 3] {
        let mut out = self.shade_far(normal, towards);
        crate::model_shade::add_local(&mut out, self.local(), (p, normal), &|_| 1.0);
        out
    }

    /// The point and spot lights, which the renderers apply per pixel.
    pub(crate) fn local(&self) -> &[crate::model_shade::Local] {
        match self {
            Shader::Set { local, .. } => local,
            _ => &[],
        }
    }

    /// [`Shader::shade`] without the point and spot lights: the part
    /// interpolated across a triangle from its corners.
    pub(crate) fn shade_far(&self, normal: [f64; 3], towards: Option<[f64; 3]>) -> [f64; 3] {
        use crate::vec3::dot;
        let facing = |d: Option<[f64; 3]>| d.map_or(0.0, |l| dot(normal, l).abs());
        match self {
            Shader::Headlight { gain } => [(0.3 + 0.7 * facing(towards)) * gain; 3],
            Shader::Unlit => [1.0; 3],
            Shader::Set {
                ambient, lights, ..
            } => {
                let mut out = *ambient;
                for (dir, colour) in lights {
                    let k = facing(dir.or(towards));
                    for (o, c) in out.iter_mut().zip(colour) {
                        *o += c * k;
                    }
                }
                out
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;

    const BASIS: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]];

    #[test]
    fn every_scheme_but_artwork_and_none_carries_table_310_lights() {
        let counts = LightingScheme::ALL.map(|s| match s.lights() {
            Lights::Set(set) => set.lights.len(),
            _ => 0,
        });
        assert_eq!(counts, [0, 0, 3, 3, 3, 3, 3, 3, 3, 6, 4, 1]);
        assert_eq!(LightingScheme::Artwork.lights(), Lights::Headlight);
    }

    #[test]
    fn a_camera_light_shines_along_the_line_of_sight() {
        let shader = Shader::new(
            LightingScheme::Headlamp.lights(),
            LightFrame::Camera,
            &BASIS,
        );
        let facing = shader.shade([0.0; 3], [0.0, 0.0, 1.0], Some([0.0, 0.0, 1.0]));
        assert!((facing[0] - 0.9).abs() < 1e-12 && (facing[2] - 1.0).abs() < 1e-12);
        let edge_on = shader.shade([0.0; 3], [1.0, 0.0, 0.0], Some([0.0, 0.0, 1.0]));
        assert_eq!(edge_on, [0.1; 3], "only the ambient term");
    }

    #[test]
    fn the_light_frame_decides_which_way_a_light_points() {
        // A camera looking straight down the model's z: camera axes are
        // model axes up to sign, so the frames agree. Tilted, they differ.
        let normal = [0.0, 0.0, 1.0];
        let day = |frame, basis: &[[f64; 3]; 3]| {
            Shader::new(LightingScheme::Day.lights(), frame, basis).shade([0.0; 3], normal, None)
        };
        assert_eq!(
            day(LightFrame::Camera, &BASIS),
            day(LightFrame::World, &BASIS)
        );
        let tilted = [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]];
        assert_ne!(
            day(LightFrame::Camera, &tilted),
            day(LightFrame::World, &tilted)
        );
        assert_eq!(
            day(LightFrame::World, &tilted),
            day(LightFrame::World, &BASIS)
        );
    }
}

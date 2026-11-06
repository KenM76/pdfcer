//! A model's own lights: PRC 731-734 in a product occurrence's or a
//! view's scene display parameters (741) [WD 7.5.15-7.5.19], placed in
//! model space.

/// A light the model carries, in model space (the space
/// [`crate::AssembledModel::meshes`] are placed in). Colours are 0-1 RGB.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ModelLight {
    /// What kind of light, and where.
    pub kind: ModelLightKind,
    /// The colour it lights a surface facing it with (its diffuse colour;
    /// an ambient light's ambient colour). White when the file names none.
    pub colour: [f64; 3],
    /// What it adds everywhere, regardless of facing (its ambient colour);
    /// black when the file names none. Equal to [`Self::colour`] for
    /// [`ModelLightKind::Ambient`].
    pub ambient: [f64; 3],
    /// A scale on [`Self::colour`]: a directional light's stored intensity
    /// (PRC authoring version 8030 on), 1 for every other light.
    pub intensity: f64,
    /// The colour of the highlights it makes, scaled like [`Self::colour`],
    /// when the file names one apart from it; `None` for the same colour.
    /// A specular index equal to the ambient one is read as unused
    /// (`prc__8137__lights_semantics.md` §2).
    pub specular: Option<[f64; 3]>,
    /// The light's name, if the file gives one.
    pub name: Option<String>,
}

/// A [`ModelLight`]'s kind and placement. Attenuation is
/// `[constant, linear, quadratic]`: a light at distance `d` is scaled by
/// `1 / (constant + linear d + quadratic d^2)` [WD 7.5.16].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ModelLightKind {
    /// Light from every direction equally.
    Ambient,
    /// A lamp radiating from `position`.
    Point {
        /// Where it is.
        position: [f64; 3],
        /// How it fades with distance.
        attenuation: [f64; 3],
    },
    /// Parallel light, like the sun.
    Directional {
        /// The direction the light travels, unit. PRC does not say which
        /// sense its vector has; pdfcer reads it as the way the light
        /// travels, as glTF and pdfcer's writer use it. Shading is
        /// two-sided, so only a shadow would see the difference.
        direction: [f64; 3],
    },
    /// A lamp at `position` shining along `direction` in a cone.
    Spot {
        /// Where it is.
        position: [f64; 3],
        /// The cone's axis, the way the light travels, unit.
        direction: [f64; 3],
        /// How it fades with distance.
        attenuation: [f64; 3],
        /// The cone's half-angle in radians (PRC stores degrees): nothing
        /// is lit outside it.
        angle: f64,
        /// How fast it fades away from the axis inside the cone:
        /// `cos(theta)^exponent` at `theta` from the axis.
        exponent: f64,
    },
}

impl ModelLight {
    /// A light of `kind` and `colour`, intensity 1, no ambient part, no
    /// name.
    ///
    /// ```
    /// use pdfcer_3d::{ModelLight, ModelLightKind};
    /// let sun = ModelLight::new(
    ///     ModelLightKind::Directional { direction: [0.0, 0.0, -1.0] },
    ///     [1.0, 0.95, 0.9],
    /// );
    /// assert_eq!(sun.intensity, 1.0);
    /// ```
    #[must_use]
    pub fn new(kind: ModelLightKind, colour: [f64; 3]) -> ModelLight {
        ModelLight {
            kind,
            colour,
            ambient: [0.0; 3],
            intensity: 1.0,
            specular: None,
            name: None,
        }
    }
}

impl ModelLightKind {
    /// The kind's name: `ambient`, `point`, `directional` or `spot`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            ModelLightKind::Ambient => "ambient",
            ModelLightKind::Point { .. } => "point",
            ModelLightKind::Directional { .. } => "directional",
            ModelLightKind::Spot { .. } => "spot",
        }
    }

    /// This kind moved by `m` (its position mapped, its direction turned
    /// and renormalised).
    #[must_use]
    pub(crate) fn transformed(self, m: &crate::Matrix) -> ModelLightKind {
        let point = |p| crate::transform_point(m, p);
        let turn = |d: [f64; 3]| {
            let o = crate::transform_point(m, [0.0; 3]);
            let t = crate::transform_point(m, d);
            unit([t[0] - o[0], t[1] - o[1], t[2] - o[2]]).unwrap_or(d)
        };
        match self {
            ModelLightKind::Ambient => self,
            ModelLightKind::Point {
                position,
                attenuation,
            } => ModelLightKind::Point {
                position: point(position),
                attenuation,
            },
            ModelLightKind::Directional { direction } => ModelLightKind::Directional {
                direction: turn(direction),
            },
            ModelLightKind::Spot {
                position,
                direction,
                attenuation,
                angle,
                exponent,
            } => ModelLightKind::Spot {
                position: point(position),
                direction: turn(direction),
                attenuation,
                angle,
                exponent,
            },
        }
    }
}

/// A PRC view's (`MKP_View`, 501) own lights, placed in model space. A
/// viewer applies them when the view is shown, in place of the scene's.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ViewLights {
    /// The view's name, if the file gives one.
    pub view: Option<String>,
    /// `is_default_view`: the view the model opens on (authoring version
    /// 8016 on; false before).
    pub default: bool,
    /// `is_active` of the view's scene display parameters.
    pub active: bool,
    /// Its lights.
    pub lights: Vec<ModelLight>,
}

/// Whether a PRC default view's lights light the model
/// ([`crate::AssembleOptions::view_lights`]). PRC lets both an occurrence
/// and a view carry lights and does not say which a viewer opens with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ViewLightUse {
    /// The first default view whose scene parameters are active and hold a
    /// light supplies the lights, in place of the occurrences': the scene
    /// the model opens on. With no such view, the occurrences' lights.
    #[default]
    DefaultView,
    /// The occurrences' lights only; views' lights are listed, not drawn.
    Ignore,
}

impl ViewLightUse {
    /// Which of `views` supplies the lights, if any.
    pub(crate) fn pick(self, views: &[ViewLights]) -> Option<usize> {
        match self {
            ViewLightUse::DefaultView => views
                .iter()
                .position(|v| v.default && v.active && !v.lights.is_empty()),
            ViewLightUse::Ignore => None,
        }
    }
}

/// A view's lights as stored: its name, default flag and scene parameters.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct StoredView {
    pub(crate) name: Option<String>,
    pub(crate) default: bool,
    pub(crate) scene: SceneLights,
}

impl StoredView {
    /// The view's lights, colours looked up in `globals`, placed by `m`
    /// unless absolute.
    pub(crate) fn resolve(&self, globals: &crate::tree::Globals, m: &crate::Matrix) -> ViewLights {
        let at = if self.scene.absolute {
            &crate::tree::IDENTITY
        } else {
            m
        };
        ViewLights {
            view: self.name.clone(),
            default: self.default,
            active: self.scene.active,
            lights: self
                .scene
                .lights
                .iter()
                .map(|l| l.resolve(globals, at))
                .collect(),
        }
    }
}

/// `v` scaled to length 1; `None` for a zero or non-finite vector, or one
/// whose unscaled square overflows or underflows to zero. Not
/// [`crate::vec3::normalize`]: this divides by the length where that
/// multiplies by its reciprocal (they can differ in the last bit), and it
/// refuses the tiny and huge vectors that one rescues.
pub(crate) fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let n = crate::vec3::dot(v, v).sqrt();
    (n.is_finite() && n > 0.0).then(|| v.map(|c| c / n))
}

/// A light as stored, before its colours are looked up: the three biased
/// colour indices (ambient, diffuse, specular).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoredLight {
    pub(crate) kind: ModelLightKind,
    pub(crate) colours: [u32; 3],
    pub(crate) intensity: f64,
    pub(crate) name: Option<String>,
}

/// One scene display parameters entity's lights.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SceneLights {
    /// `is_active`: an inactive entity's lights are not used.
    pub(crate) active: bool,
    /// `is_absolute`: the lights are in model space, not the occurrence's.
    pub(crate) absolute: bool,
    pub(crate) lights: Vec<StoredLight>,
}

impl StoredLight {
    /// The light with its colours looked up in `globals` and placed by `m`.
    pub(crate) fn resolve(&self, globals: &crate::tree::Globals, m: &crate::Matrix) -> ModelLight {
        let [a, d, s] = self.colours.map(|c| globals.colour(c));
        let ambient_only = matches!(self.kind, ModelLightKind::Ambient);
        let colour = if ambient_only { a.or(d) } else { d }.unwrap_or([1.0; 3]);
        ModelLight {
            kind: self.kind.transformed(m),
            colour,
            ambient: if ambient_only {
                colour
            } else {
                a.unwrap_or([0.0; 3])
            },
            intensity: self.intensity,
            specular: s.filter(|&s| self.colours[2] != self.colours[0] && s != colour),
            name: self.name.clone(),
        }
    }
}

//! A material's lighting terms beyond its diffuse colour: PRC `Material`
//! (702) ambient, specular, emissive and shininess [WD 7.5.4], which "are
//! identical to the definitions in OpenGL" (`prc__8137__graphics_materials.md`
//! §4). The renderers draw them as OpenGL's fixed-function model with
//! Blinn-Phong highlights; see [`crate::RenderOptions::shininess`].

/// A mesh's material terms beyond its diffuse colour, each colour 0–1 RGB,
/// as [`crate::AssembledModel::finishes`] carries them. Only a material
/// that changes the picture has one: a material with black specular and
/// emissive colours and an ambient colour equal to its diffuse one draws
/// exactly as a plain colour does, so it carries none.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct MaterialFinish {
    /// The colour that multiplies a scheme's or the model's ambient light,
    /// where a plain colour uses its diffuse colour. The diffuse colour
    /// when the file names none.
    pub ambient: [f64; 3],
    /// The highlight colour; black for none.
    pub specular: [f64; 3],
    /// Light the surface gives off itself, added unlit; black for none.
    pub emissive: [f64; 3],
    /// The stored shininess, read by [`ShininessScale`].
    pub shininess: f64,
}

impl MaterialFinish {
    /// A finish with `specular` highlights of `shininess`, no emission, and
    /// the ambient colour `ambient` (pass the diffuse colour for none).
    ///
    /// ```
    /// use pdfcer_3d::MaterialFinish;
    /// let chrome = MaterialFinish::new([0.8; 3], [1.0; 3], 0.6);
    /// assert_eq!(chrome.emissive, [0.0; 3]);
    /// ```
    #[must_use]
    pub fn new(ambient: [f64; 3], specular: [f64; 3], shininess: f64) -> MaterialFinish {
        MaterialFinish {
            ambient,
            specular,
            emissive: [0.0; 3],
            shininess,
        }
    }

    /// The finish of a material with diffuse colour `diffuse`, or `None`
    /// when it would draw as the plain colour (see [`MaterialFinish`]).
    pub(crate) fn of(self, diffuse: [f64; 3]) -> Option<MaterialFinish> {
        // NaN counts as black, as the renderers draw it.
        let black = |c: [f64; 3]| !c.iter().any(|&v| v > 0.0);
        let plain = black(self.specular) && black(self.emissive) && self.ambient == diffuse;
        (!plain).then_some(self)
    }

    /// Whether it draws a highlight.
    #[must_use]
    pub fn is_specular(&self) -> bool {
        self.specular.iter().any(|&c| c > 0.0)
    }

    /// Whether it gives off light.
    #[must_use]
    pub fn is_emissive(&self) -> bool {
        self.emissive.iter().any(|&c| c > 0.0)
    }
}

/// How a stored PRC shininess becomes the Blinn-Phong exponent `e` in
/// `specular · max(n·h, 0)^e`. PRC defers to OpenGL, whose `GL_SHININESS`
/// is an exponent in 0–128; producers that come from U3D or similar
/// formats store a 0–1 fraction of that range instead. The standard does
/// not say which a file holds. Every reading is clamped to 0–128.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ShininessScale {
    /// A value of at most 1 is a fraction (× 128), a larger one an
    /// exponent: the only reading that draws both kinds of file with a
    /// highlight of sensible size. The default.
    #[default]
    Auto,
    /// The value is the exponent, as OpenGL reads it.
    Exponent,
    /// The value is a 0–1 fraction of 128.
    Fraction,
}

/// OpenGL's largest `GL_SHININESS`.
const MAX_EXPONENT: f64 = 128.0;

impl ShininessScale {
    /// The exponent for stored shininess `s`.
    ///
    /// ```
    /// use pdfcer_3d::ShininessScale;
    /// assert_eq!(ShininessScale::Auto.exponent(0.25), 32.0);
    /// assert_eq!(ShininessScale::Auto.exponent(40.0), 40.0);
    /// assert_eq!(ShininessScale::Exponent.exponent(0.25), 0.25);
    /// assert_eq!(ShininessScale::Fraction.exponent(40.0), 128.0);
    /// ```
    #[must_use]
    pub fn exponent(self, s: f64) -> f64 {
        let e = match self {
            ShininessScale::Exponent => s,
            ShininessScale::Fraction => s * MAX_EXPONENT,
            ShininessScale::Auto if s <= 1.0 => s * MAX_EXPONENT,
            ShininessScale::Auto => s,
        };
        if e.is_nan() {
            return 0.0;
        }
        e.clamp(0.0, MAX_EXPONENT)
    }

    /// The setting's name: `auto`, `exponent` or `fraction`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            ShininessScale::Auto => "auto",
            ShininessScale::Exponent => "exponent",
            ShininessScale::Fraction => "fraction",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_material_carries_no_finish() {
        let grey = [0.5; 3];
        let plain = MaterialFinish::new(grey, [0.0; 3], 0.3);
        assert_eq!(plain.of(grey), None);
        let dark_ambient = MaterialFinish::new([0.0; 3], [0.0; 3], 0.0);
        assert!(dark_ambient.of(grey).is_some());
        let shiny = MaterialFinish::new(grey, [1.0; 3], 0.3);
        assert!(shiny.of(grey).is_some_and(|f| f.is_specular()));
    }

    #[test]
    fn shininess_never_leaves_the_opengl_range() {
        for scale in [
            ShininessScale::Auto,
            ShininessScale::Exponent,
            ShininessScale::Fraction,
        ] {
            for s in [f64::NAN, -1.0, 0.0, 0.5, 1.0, 64.0, 1e9, f64::INFINITY] {
                let e = scale.exponent(s);
                assert!((0.0..=128.0).contains(&e), "{scale:?} {s} {e}");
            }
        }
    }
}

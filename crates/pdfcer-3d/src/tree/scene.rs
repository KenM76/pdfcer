//! Scene display parameters (741), their camera, lights and clipping
//! planes, read past [WD 7.5.15-7.5.20, 7.11.13; ISS #658 #745; PRCRS].
//! Field order follows the spec RAG's
//! `threed/prc__8137__markup_view_entities.md` §7-8.

use crate::PrcError;
use crate::model_light::{ModelLightKind, SceneLights, StoredLight, unit};
use crate::tess::{Ctx, malformed};

use super::{ABSOLUTE_SCENE_FROM, CAMERA, SCENE_DISPLAY_PARAMETERS};

const AMBIENT_LIGHT: u32 = 731;
const POINT_LIGHT: u32 = 732;
const DIRECTIONAL_LIGHT: u32 = 733;
const SPOT_LIGHT: u32 = 734;
const SURF_PLANE: u32 = 86;
/// A directional light's intensity exists from this authoring version
/// [PRCRS].
const LIGHT_INTENSITY_FROM: u32 = 8030;

impl Ctx<'_, '_> {
    /// `SceneDisplayParameters` (741) [WD 7.5.19; PRCRS]: its lights; the
    /// rest read past.
    pub(super) fn scene_display_parameters(&mut self) -> Result<SceneLights, PrcError> {
        self.expect_type(SCENE_DISPLAY_PARAMETERS)?;
        self.content_prc_ref_base()?;
        let mut out = SceneLights {
            active: self.r.bit()?,
            ..SceneLights::default()
        };
        let n = self.count(8, "scene lights")?;
        for _ in 0..n {
            out.lights.push(self.light()?);
        }
        if self.r.bit()? {
            self.camera()?;
        }
        if self.r.bit()? {
            self.vector3()?; // rotation centre
        }
        let n = self.count(8, "scene clipping planes")?;
        for _ in 0..n {
            self.surface_plane()?;
        }
        self.r.unsigned_integer()?; // background style + 1
        self.r.unsigned_integer()?; // default style + 1
        let n = self.count(2, "scene default styles")?;
        for _ in 0..2 * n {
            self.r.unsigned_integer()?;
        }
        if self.version >= ABSOLUTE_SCENE_FROM {
            out.absolute = self.r.bit()?;
        }
        self.skip_added(SCENE_DISPLAY_PARAMETERS)?;
        Ok(out)
    }

    /// `Camera` (742) [WD 7.5.20; PRCRS]: read past.
    fn camera(&mut self) -> Result<(), PrcError> {
        self.expect_type(CAMERA)?;
        self.content_prc_ref_base()?;
        self.r.bit()?; // is_orthographic
        for _ in 0..3 {
            self.vector3()?; // position, look, up
        }
        for _ in 0..6 {
            self.r.double()?; // x, y, ratio, near, far, zoom
        }
        self.skip_added(CAMERA)
    }

    /// A light (731-734), type code first: three colour indices, then its
    /// own fields. No user data [PRCRS; ISS #745 corrects the WD's table].
    /// The fall-off angle is stored in degrees.
    fn light(&mut self) -> Result<StoredLight, PrcError> {
        let t = self.r.unsigned_integer()?;
        if !(AMBIENT_LIGHT..=SPOT_LIGHT).contains(&t) {
            return Err(malformed(format!("entity type {t} as a light")));
        }
        self.content_prc_ref_base()?;
        let name = self.name.clone();
        let mut colours = [0; 3];
        for c in &mut colours {
            *c = self.r.unsigned_integer()?; // ambient, diffuse, specular + 1
        }
        let mut intensity = 1.0;
        let kind = match t {
            POINT_LIGHT => ModelLightKind::Point {
                position: self.vector3()?,
                attenuation: self.vector3()?,
            },
            DIRECTIONAL_LIGHT => {
                let direction = self.vector3()?;
                if self.version >= LIGHT_INTENSITY_FROM {
                    intensity = self.r.double()?;
                }
                ModelLightKind::Directional {
                    direction: unit(direction).unwrap_or([0.0, 0.0, -1.0]),
                }
            }
            SPOT_LIGHT => ModelLightKind::Spot {
                position: self.vector3()?,
                attenuation: self.vector3()?,
                direction: unit(self.vector3()?).unwrap_or([0.0, 0.0, -1.0]),
                angle: self.r.double()?.to_radians(),
                exponent: self.r.double()?,
            },
            _ => ModelLightKind::Ambient,
        };
        self.skip_added(t)?;
        Ok(StoredLight {
            kind,
            colours,
            intensity,
            name,
        })
    }

    /// `SURF_Plane` (86), read with its own type code: a view's plane or a
    /// clipping plane. Its transformation has no presence bit (ISS #658);
    /// no user data.
    pub(super) fn surface_plane(&mut self) -> Result<(), PrcError> {
        self.expect_type(SURF_PLANE)?;
        if self.r.bit()? {
            self.content_prc_base()?; // attributes and name
            self.r.unsigned_integer()?; // CAD identifier
        }
        self.r.unsigned_integer()?; // extend type
        self.transformation_3d()?;
        self.skip_doubles(4)?; // domain: min, max
        self.skip_doubles(4)?; // u, v coefficients a then b
        self.skip_added(SURF_PLANE)
    }

    fn skip_doubles(&mut self, n: usize) -> Result<(), PrcError> {
        for _ in 0..n {
            self.r.double()?;
        }
        Ok(())
    }
}

//! Scene display parameters (741), their camera, lights and clipping
//! planes, read past [WD 7.5.15-7.5.20, 7.11.13; ISS #658 #745; PRCRS].
//! Field order follows the spec RAG's
//! `threed/prc__8137__markup_view_entities.md` §7-8.

use crate::PrcError;
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
    /// `SceneDisplayParameters` (741) [WD 7.5.19; PRCRS]: read past.
    pub(super) fn scene_display_parameters(&mut self) -> Result<(), PrcError> {
        self.expect_type(SCENE_DISPLAY_PARAMETERS)?;
        self.content_prc_ref_base()?;
        self.r.bit()?; // is_active
        let n = self.count(8, "scene lights")?;
        for _ in 0..n {
            self.light()?;
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
            self.r.bit()?; // is_absolute
        }
        self.schema
            .skip_added_fields(SCENE_DISPLAY_PARAMETERS, &mut self.r)
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
        self.schema.skip_added_fields(CAMERA, &mut self.r)
    }

    /// A light (731-734), type code first: three colour indices, then its
    /// own fields. No user data [PRCRS; ISS #745 corrects the WD's table].
    fn light(&mut self) -> Result<(), PrcError> {
        let t = self.r.unsigned_integer()?;
        if !(AMBIENT_LIGHT..=SPOT_LIGHT).contains(&t) {
            return Err(malformed(format!("entity type {t} as a light")));
        }
        self.content_prc_ref_base()?;
        for _ in 0..3 {
            self.r.unsigned_integer()?; // ambient, diffuse, specular + 1
        }
        match t {
            POINT_LIGHT => {
                self.vector3()?; // location
                self.skip_doubles(3)?; // attenuation
            }
            DIRECTIONAL_LIGHT => {
                self.vector3()?; // direction
                if self.version >= LIGHT_INTENSITY_FROM {
                    self.r.double()?;
                }
            }
            SPOT_LIGHT => {
                self.vector3()?; // location
                self.skip_doubles(3)?; // attenuation
                self.vector3()?; // direction
                self.skip_doubles(2)?; // fall-off angle and exponent
            }
            _ => {}
        }
        self.schema.skip_added_fields(t, &mut self.r)
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
        self.schema.skip_added_fields(SURF_PLANE, &mut self.r)
    }

    fn skip_doubles(&mut self, n: usize) -> Result<(), PrcError> {
        for _ in 0..n {
            self.r.double()?;
        }
        Ok(())
    }
}

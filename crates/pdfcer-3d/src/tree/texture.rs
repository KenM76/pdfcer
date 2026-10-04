//! Pictures, texture definitions and texture transformations as far as
//! drawing [WD 7.5.5, 7.5.7, 7.5.8; `prc__8137__graphics_materials.md` §13].

use super::style::Material;
use super::*;
use std::sync::Arc;

/// A `Picture` (703) [WD 7.5.5]: `file` is the uncompressed-file index + 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Picture {
    pub(crate) format: u32,
    pub(crate) file: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// A `TextureDefinition` (712) as far as drawing; enumerations as stored.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct TextureDef {
    /// `picture_index + 1`.
    pub(crate) picture: u32,
    pub(crate) mapping: i32,
    pub(crate) function: i32,
    pub(crate) blend: [f64; 4],
    /// `texture_mapping_attributes`: which channels the texture supplies.
    pub(crate) channels: u32,
    /// Wrapping modes S and T, as stored (T is 0 when not stored).
    pub(crate) wrap: [i32; 2],
    pub(crate) transform: Option<UvTransform>,
}

/// A `TextureTransformation` (713) [WD 7.5.8, 7.4.11.1]: the parts pdfcer
/// applies, and whether any other part was present.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct UvTransform {
    pub(crate) invert: [bool; 2],
    pub(crate) translate: [f64; 2],
    pub(crate) scale: [f64; 2],
    /// The rotation's one vector (ISS #819); `None` when not set.
    pub(crate) rotate: Option<[f64; 2]>,
    /// Non-orthogonal, homogeneous or mirror bits: not applied.
    pub(crate) unsupported: bool,
}

impl UvTransform {
    /// The 2 × 3 matrix taking stored (u, v) to picture (u, v): invert,
    /// then scale, rotate and translate [WD 7.4.11].
    pub(crate) fn matrix(&self) -> [[f64; 3]; 2] {
        let [is, it] = self.invert;
        // Inverting a parameter maps p to 1 - p.
        let (su, ou) = if is { (-1.0, 1.0) } else { (1.0, 0.0) };
        let (sv, ov) = if it { (-1.0, 1.0) } else { (1.0, 0.0) };
        let [kx, ky] = self.scale;
        let (c, s) = match self.rotate {
            Some([x, y]) if x.hypot(y) > 0.0 => (x / x.hypot(y), y / x.hypot(y)),
            _ => (1.0, 0.0),
        };
        let [tx, ty] = self.translate;
        // p' = R · K · (S·p + O) + T.
        let (a, b) = (c * kx, -s * ky);
        let (d, e) = (s * kx, c * ky);
        [
            [a * su, b * sv, a * ou + b * ov + tx],
            [d * su, e * sv, d * ou + e * ov + ty],
        ]
    }
}

impl Ctx<'_, '_> {
    /// `Picture` (703) [WD 7.5.5; `prc__8137__graphics_materials.md` §13.1].
    pub(crate) fn picture(&mut self) -> Result<Picture, PrcError> {
        self.expect_type(PICTURE)?;
        self.content_prc_base()?;
        let p = Picture {
            format: self.r.unsigned_integer()?,
            file: self.r.unsigned_integer()?,
            width: self.r.unsigned_integer()?,
            height: self.r.unsigned_integer()?,
        };
        self.schema.skip_added_fields(PICTURE, &mut self.r)?;
        Ok(p)
    }

    /// `TextureDefinition` (712) [WD 7.5.7; ISS #485 Table 97;
    /// `prc__8137__graphics_materials.md` §13.3].
    pub(crate) fn texture_definition(&mut self) -> Result<TextureDef, PrcError> {
        self.expect_type(TEXTURE_DEFINITION)?;
        self.content_prc_ref_base()?;
        let mut d = TextureDef {
            picture: self.r.unsigned_integer()?,
            ..TextureDef::default()
        };
        let dimension = self.r.character()?;
        d.mapping = self.r.integer()?;
        if d.mapping == TEXTURE_MAPPING_OPERATOR {
            self.r.integer()?; // mapping operator
            if self.r.bit()? {
                self.transformation()?;
            }
        }
        d.channels = self.r.unsigned_integer()?;
        let n = self.count(1, "texture intensities")?;
        for _ in 0..n {
            self.r.double()?;
        }
        let n = self.count(1, "texture components")?;
        for _ in 0..n {
            self.r.character()?;
        }
        d.function = self.r.integer()?;
        if d.function == TEXTURE_FUNCTION_BLEND {
            for c in &mut d.blend {
                *c = self.r.double()?;
            }
        }
        for _ in 0..2 {
            // RGB, then alpha: a source blend, and a destination when set
            if self.r.integer()? != 0 {
                self.r.integer()?;
            }
        }
        if self.r.character()? & TEXTURE_ALPHA_TEST != 0 {
            self.r.integer()?; // alpha test function
            self.r.double()?; // reference
        }
        for k in 0..usize::from(dimension.clamp(1, 3)) {
            let w = self.r.integer()?;
            if let Some(slot) = d.wrap.get_mut(k) {
                *slot = w;
            }
        }
        if self.r.bit()? {
            d.transform = Some(self.texture_transformation()?);
        }
        self.schema
            .skip_added_fields(TEXTURE_DEFINITION, &mut self.r)?;
        Ok(d)
    }

    /// `TextureTransformation` (713), type-tagged [WD 7.5.8, 7.4.11.1;
    /// `prc__8137__graphics_materials.md` §7b].
    pub(crate) fn texture_transformation(&mut self) -> Result<UvTransform, PrcError> {
        self.expect_type(TEXTURE_TRANSFORMATION)?;
        let mut t = UvTransform {
            scale: [1.0, 1.0],
            ..UvTransform::default()
        };
        t.invert = [self.r.bit()?, self.r.bit()?];
        self.r.bit()?; // is 2D, always set
        let behaviour = self.r.character()?;
        if behaviour & 0x01 != 0 {
            t.translate = [self.r.double()?, self.r.double()?];
        }
        if behaviour & 0x20 != 0 {
            for _ in 0..4 {
                self.r.double()?; // non-orthogonal axes
            }
        } else if behaviour & 0x02 != 0 {
            // one vector in a texture (ISS #819)
            t.rotate = Some([self.r.double()?, self.r.double()?]);
        }
        if behaviour & 0x10 != 0 {
            t.scale = [self.r.double()?, self.r.double()?];
        } else if behaviour & 0x08 != 0 {
            let k = self.r.double()?;
            t.scale = [k, k];
        }
        if behaviour & 0x40 != 0 {
            for _ in 0..3 {
                self.r.double()?; // homogeneous
            }
        }
        t.unsupported = behaviour & (0x04 | 0x20 | 0x40) != 0;
        self.schema
            .skip_added_fields(TEXTURE_TRANSFORMATION, &mut self.r)?;
        Ok(t)
    }
}

/// Which uncompressed-file list a picture's `file` index is looked up in
/// first. ISO 14739-1 does not say whether it names the owning file
/// structure's header files or the file header's
/// (`prc__8137__graphics_materials.md` §13.1); an index past the first list
/// falls through to the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum PictureFiles {
    /// The owning file structure's header files, then the file header's.
    #[default]
    StructureFirst,
    /// The file header's, then the owning file structure's.
    HeaderFirst,
}

/// How a stored wrapping mode is numbered. ISO 14739-1's table counts from
/// 1; the revision draft numbers its sibling enumerations from 0, and the
/// stored base is unverified (`prc__8137__graphics_materials.md` §13.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum WrapBase {
    /// 0 unknown, 1 repeat, 2 clamp to border, 3 clamp, 4 clamp to edge,
    /// 5 mirrored repeat.
    #[default]
    ZeroBased,
    /// The same modes numbered 1–6.
    OneBased,
}

/// The texture choices [`crate::assemble_with_options`] passes down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct TextureRules {
    pub(crate) files: PictureFiles,
    pub(crate) wrap: WrapBase,
    pub(crate) origin: crate::TextureOrigin,
}

/// What a style's texture resolved to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Skin {
    /// Drawable; `more` = further texture levels follow, not drawn.
    Drawn {
        texture: Arc<crate::Texture>,
        more: bool,
    },
    /// Not drawable, and why; the surface draws its base colour.
    Undrawn(&'static str),
}

/// Why a texture draws its base colour instead.
pub(crate) mod why {
    pub(crate) const PICTURE: &str = "base colour drawn: the picture could not be found";
    pub(crate) const MAPPING: &str =
        "base colour drawn: the texture is not mapped by stored coordinates";
    pub(crate) const TRANSFORM: &str = "base colour drawn: the texture's coordinate transform \
         is non-orthogonal, homogeneous or mirrored";
    pub(crate) const NO_UVS: &str =
        "base colour drawn: the mesh stores no coordinates for the texture";
    pub(crate) const MORE_LEVELS: &str = "only the first texture level is drawn";
}

/// `TextureDefinition` mapping type "stored coordinates", 0-based
/// [ISS #485].
const MAPPING_STORED: i32 = 1;

impl Globals {
    /// Entry `b` is the texture of biased style `b`, as
    /// [`Self::style_skin`]; a picture several styles share is decoded once.
    pub(crate) fn skins(
        &self,
        own: &[Vec<u8>],
        header: &[Vec<u8>],
        rules: TextureRules,
    ) -> Vec<Option<Skin>> {
        let mut decoded: Vec<(u32, Option<Skin>)> = Vec::new();
        (0..=self.styles.len() as u32)
            .map(|b| {
                let key = b
                    .checked_sub(1)
                    .and_then(|i| self.styles.get(i as usize))
                    .filter(|s| s.is_material)
                    .map(|s| s.index)?;
                if let Some((_, skin)) = decoded.iter().find(|(k, _)| *k == key) {
                    return skin.clone();
                }
                let skin = self.style_skin(b, own, header, rules);
                decoded.push((key, skin.clone()));
                skin
            })
            .collect()
    }

    /// The texture style `biased` draws, `None` when it is not textured
    /// [WD 7.5.3, 7.5.6, 7.5.7, 7.5.5]. `own` and `header` are the owning
    /// file structure's and the file header's uncompressed files.
    pub(crate) fn style_skin(
        &self,
        biased: u32,
        own: &[Vec<u8>],
        header: &[Vec<u8>],
        rules: TextureRules,
    ) -> Option<Skin> {
        let style = self.styles.get(biased.checked_sub(1)? as usize)?;
        if !style.is_material {
            return None;
        }
        let material = self.materials.get(style.index.checked_sub(1)? as usize)?;
        let Material::Textured {
            texture, next, uv, ..
        } = *material
        else {
            return None;
        };
        let Some(def) = texture
            .checked_sub(1)
            .and_then(|i| self.textures.get(i as usize))
        else {
            return Some(Skin::Undrawn(why::PICTURE));
        };
        Some(
            match self.texture(def, uv.saturating_sub(1), own, header, rules) {
                Ok(texture) => Skin::Drawn {
                    texture: Arc::new(texture),
                    more: next != 0,
                },
                Err(why) => Skin::Undrawn(why),
            },
        )
    }

    fn texture(
        &self,
        def: &TextureDef,
        uv_set: u32,
        own: &[Vec<u8>],
        header: &[Vec<u8>],
        rules: TextureRules,
    ) -> Result<crate::Texture, &'static str> {
        if def.mapping != MAPPING_STORED {
            return Err(why::MAPPING);
        }
        if def.transform.is_some_and(|t| t.unsupported) {
            return Err(why::TRANSFORM);
        }
        let picture = def
            .picture
            .checked_sub(1)
            .and_then(|i| self.pictures.get(i as usize))
            .ok_or(why::PICTURE)?;
        let bytes = picture_file(picture.file, own, header, rules.files).ok_or(why::PICTURE)?;
        let (width, height, mut rgba) =
            crate::texture::decode_picture(picture.format, bytes, picture.width, picture.height)?;
        crate::texture::keep_channels(&mut rgba, def.channels);
        Ok(crate::Texture {
            width,
            height,
            rgba,
            wrap: def.wrap.map(|w| wrap_mode(w, rules.wrap)),
            function: function(def),
            uv_matrix: def
                .transform
                .map_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], |t| t.matrix()),
            uv_set: uv_set as usize,
            origin: rules.origin,
        })
    }
}

/// The uncompressed file a picture's `file` (index + 1) names: one list
/// counted first, then the other after it [WD 6.1.1, 6.2.2].
fn picture_file<'a>(
    file: u32,
    own: &'a [Vec<u8>],
    header: &'a [Vec<u8>],
    order: PictureFiles,
) -> Option<&'a Vec<u8>> {
    let (first, second) = match order {
        PictureFiles::StructureFirst => (own, header),
        PictureFiles::HeaderFirst => (header, own),
    };
    let index = file.checked_sub(1)? as usize;
    first
        .get(index)
        .or_else(|| second.get(index.checked_sub(first.len())?))
}

/// Stored wrapping mode `w` [WD 7.5.7]; unknown and out-of-range repeat.
fn wrap_mode(w: i32, base: WrapBase) -> crate::TextureWrap {
    let zero = match base {
        WrapBase::ZeroBased => w,
        WrapBase::OneBased => w - 1,
    };
    match zero {
        2..=4 => crate::TextureWrap::Clamp,
        5 => crate::TextureWrap::MirroredRepeat,
        _ => crate::TextureWrap::Repeat,
    }
}

/// The texture function, 0-based [ISS #485]; unknown replaces.
fn function(def: &TextureDef) -> crate::TextureFunction {
    match def.function {
        1 => crate::TextureFunction::Modulate,
        TEXTURE_FUNCTION_BLEND => crate::TextureFunction::Blend { colour: def.blend },
        4 => crate::TextureFunction::Decal,
        _ => crate::TextureFunction::Replace,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{TextureFunction, TextureWrap};

    #[test]
    fn a_picture_file_counts_one_list_then_the_other() {
        let own = [b"own".to_vec()];
        let header = [b"h0".to_vec(), b"h1".to_vec()];
        let at = |file, order| picture_file(file, &own, &header, order).map(|v| v.as_slice());
        let s = PictureFiles::StructureFirst;
        assert_eq!(at(1, s), Some(&b"own"[..]));
        assert_eq!(at(2, s), Some(&b"h0"[..]));
        assert_eq!(at(3, s), Some(&b"h1"[..]));
        assert_eq!(at(4, s), None);
        assert_eq!(at(0, s), None, "0 names no file");
        let h = PictureFiles::HeaderFirst;
        assert_eq!(at(1, h), Some(&b"h0"[..]));
        assert_eq!(at(3, h), Some(&b"own"[..]));
    }

    #[test]
    fn wrap_modes_follow_the_chosen_numbering() {
        let zero = [0, 1, 2, 3, 4, 5, 6].map(|w| wrap_mode(w, WrapBase::ZeroBased));
        use TextureWrap::{Clamp, MirroredRepeat, Repeat};
        assert_eq!(
            zero,
            [Repeat, Repeat, Clamp, Clamp, Clamp, MirroredRepeat, Repeat]
        );
        let one = [1, 2, 3, 4, 5, 6].map(|w| wrap_mode(w, WrapBase::OneBased));
        assert_eq!(one, [Repeat, Repeat, Clamp, Clamp, Clamp, MirroredRepeat]);
    }

    #[test]
    fn functions_map_and_unknown_replaces() {
        let f = |n| {
            function(&TextureDef {
                function: n,
                blend: [0.25; 4],
                ..TextureDef::default()
            })
        };
        assert_eq!(f(1), TextureFunction::Modulate);
        assert_eq!(f(2), TextureFunction::Replace);
        assert_eq!(f(3), TextureFunction::Blend { colour: [0.25; 4] });
        assert_eq!(f(4), TextureFunction::Decal);
        assert_eq!(f(0), TextureFunction::Replace);
        assert_eq!(f(9), TextureFunction::Replace);
    }
}

//! Graphics, styles, materials and the globals that hold them
//! [WD 7.2.4, 7.3.5].

use super::*;

/// One entity's `GraphicsContent`: `style` is `line_style_index + 1`
/// (0 = none), `bits` the behaviour bits [WD 7.2.4]. `fs` is the file
/// structure whose tree holds the entity, so whose globals `style` indexes
/// (`prc__8137__model_tree_asm.md` §10: no clause says otherwise); the walk
/// sets it, a reader leaves it 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Graphics {
    pub(crate) style: u32,
    pub(crate) bits: u16,
    pub(crate) fs: usize,
}

/// How a style's `transparency` combines with its material's diffuse alpha.
/// ISO 14739-1 defines both and not their combination, and gives a diffuse
/// alpha of 0.0 no "unset" meaning (`prc__8137__graphics_materials.md` §12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StyleAlpha {
    /// As [`Self::StyleWins`], except that a material diffuse alpha of
    /// exactly 0.0 under a style with no transparency is read as unset, so
    /// opaque. Producers write 0.0 on every material of models meant to be
    /// seen, with or without a style transparency; read literally, every
    /// part is invisible. [`crate::AssembledModel::alpha_unset`] counts the
    /// meshes this reading changed.
    #[default]
    ZeroUnset,
    /// The style's transparency, when it has one, is the opacity; the
    /// material alpha, read literally (0.0 invisible, as OpenGL reads it),
    /// applies only to a style without one. SolidWorks writes every
    /// material alpha as 0.0 beside an opaque style, which
    /// [`Self::Multiply`] draws invisible.
    StyleWins,
    /// The opacity is material alpha × style transparency.
    Multiply,
}

/// A style's resolved colour: straight RGBA, each 0–1, and whether its
/// opacity comes from reading a zero material alpha as unset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Paint {
    pub(crate) rgba: [f64; 4],
    pub(crate) alpha_unset: bool,
}

/// A `Style` (701) as far as colour: `index` is `colour_or_material + 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Style {
    pub(crate) is_material: bool,
    pub(crate) index: u32,
    pub(crate) transparency: Option<u8>,
}

/// A `materials` entry as far as colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Material {
    /// `Material` (702): `diffuse + 1` (a double-scaled colour index) and
    /// the diffuse alpha.
    Plain { diffuse: u32, alpha: f64 },
    /// `TextureApplication` (711) [WD 7.5.6]: `material_generic_index`,
    /// `texture_definition_index`, `next_texture_index` and
    /// `UV_coordinates_index`, each + 1.
    Textured {
        base: u32,
        texture: u32,
        next: u32,
        uv: u32,
    },
}

/// What `FileStructureGlobals` contributes to placing and colouring.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Globals {
    /// Entry `i` places an item with biased local-CS index `i + 1`.
    pub(crate) systems: Vec<Matrix>,
    pub(crate) colours: Vec<[f64; 3]>,
    pub(crate) materials: Vec<Material>,
    pub(crate) styles: Vec<Style>,
    pub(crate) pictures: Vec<super::texture::Picture>,
    pub(crate) textures: Vec<super::texture::TextureDef>,
}

impl Globals {
    /// Colour index `i + 1` of a style or material. Indices are
    /// double-scaled: stored `i + 1` names `colours[i / 3]`
    /// (`prc__8137__graphics_materials.md` §2, ISS #816); one that is not a
    /// multiple of three names nothing.
    fn colour(&self, biased: u32) -> Option<[f64; 3]> {
        let i = biased.checked_sub(1)?;
        if i % 3 != 0 {
            return None;
        }
        self.colours.get(i as usize / 3).copied()
    }

    /// The RGBA of style `biased` (`line_style_index + 1`).
    #[cfg(test)]
    pub(crate) fn style_colour(&self, biased: u32, rule: StyleAlpha) -> Option<[f64; 4]> {
        self.style_paint(biased, rule).map(|p| p.rgba)
    }

    /// The colour of style `biased`, and whether `rule` read its material
    /// alpha as unset.
    pub(crate) fn style_paint(&self, biased: u32, rule: StyleAlpha) -> Option<Paint> {
        let style = self.styles.get(biased.checked_sub(1)? as usize)?;
        let (rgb, mut alpha) = if style.is_material {
            let mut m = self.materials.get(style.index.checked_sub(1)? as usize)?;
            if let Material::Textured { base, .. } = *m {
                m = self.materials.get(base.checked_sub(1)? as usize)?;
            }
            match *m {
                Material::Plain { diffuse, alpha } => (self.colour(diffuse)?, alpha),
                Material::Textured { .. } => return None,
            }
        } else {
            (self.colour(style.index)?, 1.0)
        };
        let mut alpha_unset = false;
        match style.transparency {
            Some(t) => {
                let t = f64::from(t) / 255.0;
                alpha = match rule {
                    StyleAlpha::ZeroUnset | StyleAlpha::StyleWins => t,
                    StyleAlpha::Multiply => alpha * t,
                };
            }
            None if rule == StyleAlpha::ZeroUnset && style.is_material && alpha == 0.0 => {
                alpha = 1.0;
                alpha_unset = true;
            }
            None => {}
        }
        let [r, g, b] = rgb;
        Some(Paint {
            rgba: [r, g, b, alpha.clamp(0.0, 1.0)],
            alpha_unset,
        })
    }
}

/// The style a chain of graphics resolves to, outermost first: a son's own
/// style is used unless an ancestor set `FatherHeritColor` (the oldest such
/// wins), and a son setting `SonHeritColor` overrides that
/// [WD 7.2.4.2]. Entities with no style inherit. Returns the winning
/// entity's graphics, style 0 when none.
pub(crate) fn resolve_style(chain: &[Graphics]) -> Graphics {
    let (mut won, mut forced) = (Graphics::default(), false);
    for g in chain {
        if g.style == 0 {
            continue;
        }
        if !forced || g.bits & SON_HERIT_COLOR != 0 {
            won = *g;
            forced = g.bits & FATHER_HERIT_COLOR != 0;
        }
    }
    won
}

impl Ctx<'_, '_> {
    /// `FileStructureGlobals` (303) read as far as its reference coordinate
    /// systems [WD 7.3.5, 7.3.5.2; PRCRS]: colours, materials, styles and
    /// the systems.
    ///
    /// Fonts and fill patterns are [`PrcError::Unsupported`]; line
    /// patterns are read past.
    pub(crate) fn globals(&mut self) -> Result<Globals, PrcError> {
        let mut g = Globals::default();
        self.expect_type(FILE_STRUCTURE_GLOBALS)?;
        self.content_prc_base()?;
        let n = self.count(4, "referenced file structures")?;
        for _ in 0..n {
            self.unique_id()?;
        }
        self.r.double()?; // tessellation chord-height ratio
        self.r.double()?; // tessellation angle
        self.r.string()?; // default font family
        if self.r.unsigned_integer()? != 0 {
            return Err(PrcError::Unsupported("PRC global fonts"));
        }
        let n = self.count(3, "colours")?;
        for _ in 0..n {
            let c = self.vector3()?;
            g.colours.push(c);
        }
        let n = self.count(1, "pictures")?;
        for _ in 0..n {
            let p = self.picture()?;
            g.pictures.push(p);
        }
        let n = self.count(1, "texture definitions")?;
        for _ in 0..n {
            let t = self.texture_definition()?;
            g.textures.push(t);
        }
        let n = self.count(1, "materials")?;
        for _ in 0..n {
            let m = self.material()?;
            g.materials.push(m);
        }
        let n = self.count(1, "line patterns")?;
        for _ in 0..n {
            self.expect_type(LINE_PATTERN)?;
            self.content_prc_ref_base()?;
            let k = self.count(1, "line pattern lengths")?;
            for _ in 0..k {
                self.r.double()?;
            }
            self.r.double()?; // start offset
            self.r.bit()?; // scale
            self.schema.skip_added_fields(LINE_PATTERN, &mut self.r)?;
        }
        let n = self.count(1, "styles")?;
        for _ in 0..n {
            let st = self.style()?;
            g.styles.push(st);
        }
        if self.r.unsigned_integer()? != 0 {
            return Err(PrcError::Unsupported("PRC fill patterns"));
        }
        let n = self.count(1, "reference coordinate systems")?;
        for _ in 0..n {
            self.expect_type(RI_COORDINATE_SYSTEM)?;
            self.base_with_graphics()?;
            self.r.unsigned_integer()?; // local CS + 1
            self.r.unsigned_integer()?; // tessellation + 1
            let m = self.transformation()?;
            g.systems.push(m);
            self.schema
                .skip_added_fields(RI_COORDINATE_SYSTEM, &mut self.r)?;
            self.user_data()?;
        }
        self.schema
            .skip_added_fields(FILE_STRUCTURE_GLOBALS, &mut self.r)?;
        self.user_data()?;
        Ok(g)
    }

    /// One `materials` entry: `Material` (702) or `TextureApplication` (711),
    /// type-tagged [WD 7.5.4, 7.5.6; PRCRS].
    fn material(&mut self) -> Result<Material, PrcError> {
        let t = self.r.unsigned_integer()?;
        self.content_prc_ref_base()?;
        let m = match t {
            MATERIAL => {
                self.r.unsigned_integer()?; // ambient + 1
                let diffuse = self.r.unsigned_integer()?;
                self.r.unsigned_integer()?; // emissive + 1
                self.r.unsigned_integer()?; // specular + 1
                self.r.double()?; // shininess
                self.r.double()?; // ambient alpha
                let alpha = self.r.double()?;
                self.r.double()?; // emissive alpha
                self.r.double()?; // specular alpha
                Material::Plain { diffuse, alpha }
            }
            TEXTURE_APPLICATION => Material::Textured {
                base: self.r.unsigned_integer()?,
                texture: self.r.unsigned_integer()?,
                next: self.r.unsigned_integer()?,
                uv: self.r.unsigned_integer()?,
            },
            t => return Err(malformed(format!("entity type {t} as a material"))),
        };
        self.schema.skip_added_fields(t, &mut self.r)?;
        Ok(m)
    }

    /// `Style` (701) [WD 7.5.3; PRCRS].
    fn style(&mut self) -> Result<Style, PrcError> {
        self.expect_type(STYLE)?;
        self.content_prc_ref_base()?;
        self.r.double()?; // line width
        self.r.bit()?; // is_vpicture
        self.r.unsigned_integer()?; // pattern + 1
        let is_material = self.r.bit()?;
        let index = self.r.unsigned_integer()?;
        let transparency = if self.r.bit()? {
            Some(self.r.character()?)
        } else {
            None
        };
        for _ in 0..3 {
            // rendering parameters 1-3
            if self.r.bit()? {
                self.r.character()?;
            }
        }
        self.schema.skip_added_fields(STYLE, &mut self.r)?;
        Ok(Style {
            is_material,
            index,
            transparency,
        })
    }
}

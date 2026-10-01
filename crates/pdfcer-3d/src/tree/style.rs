//! Graphics, styles, materials and the globals that hold them
//! [WD 7.2.4, 7.3.5].

use super::*;

/// One entity's `GraphicsContent`: `style` is `line_style_index + 1`
/// (0 = none), `bits` the behaviour bits [WD 7.2.4].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Graphics {
    pub(crate) style: u32,
    pub(crate) bits: u16,
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
    /// `TextureApplication` (711): `material_generic_index + 1`.
    Textured { base: u32 },
}

/// What `FileStructureGlobals` contributes to placing and colouring.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Globals {
    /// Entry `i` places an item with biased local-CS index `i + 1`.
    pub(crate) systems: Vec<Matrix>,
    pub(crate) colours: Vec<[f64; 3]>,
    pub(crate) materials: Vec<Material>,
    pub(crate) styles: Vec<Style>,
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
    pub(crate) fn style_colour(&self, biased: u32) -> Option<[f64; 4]> {
        let style = self.styles.get(biased.checked_sub(1)? as usize)?;
        let (rgb, mut alpha) = if style.is_material {
            let mut m = self.materials.get(style.index.checked_sub(1)? as usize)?;
            if let Material::Textured { base } = *m {
                m = self.materials.get(base.checked_sub(1)? as usize)?;
            }
            match *m {
                Material::Plain { diffuse, alpha } => (self.colour(diffuse)?, alpha),
                Material::Textured { .. } => return None,
            }
        } else {
            (self.colour(style.index)?, 1.0)
        };
        if let Some(t) = style.transparency {
            alpha *= f64::from(t) / 255.0;
        }
        let [r, g, b] = rgb;
        Some([r, g, b, alpha.clamp(0.0, 1.0)])
    }
}

/// The style a chain of graphics resolves to, outermost first: a son's own
/// style is used unless an ancestor set `FatherHeritColor` (the oldest such
/// wins), and a son setting `SonHeritColor` overrides that
/// [WD 7.2.4.2]. Entities with no style inherit.
pub(crate) fn resolve_style(chain: &[Graphics]) -> u32 {
    let (mut style, mut forced) = (0, false);
    for g in chain {
        if g.style == 0 {
            continue;
        }
        if !forced || g.bits & SON_HERIT_COLOR != 0 {
            style = g.style;
            forced = g.bits & FATHER_HERIT_COLOR != 0;
        }
    }
    style
}

impl Ctx<'_, '_> {
    /// `FileStructureGlobals` (303) read as far as its reference coordinate
    /// systems [WD 7.3.5, 7.3.5.2; PRCRS]: colours, materials, styles and
    /// the systems.
    ///
    /// Fonts, pictures, texture definitions and fill patterns are
    /// [`PrcError::Unsupported`]; line patterns are read past.
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
        if self.r.unsigned_integer()? != 0 {
            return Err(PrcError::Unsupported("PRC global pictures"));
        }
        if self.r.unsigned_integer()? != 0 {
            return Err(PrcError::Unsupported("PRC texture definitions"));
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
            TEXTURE_APPLICATION => {
                let base = self.r.unsigned_integer()?;
                for _ in 0..3 {
                    self.r.unsigned_integer()?; // texture, next, UV set + 1
                }
                Material::Textured { base }
            }
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

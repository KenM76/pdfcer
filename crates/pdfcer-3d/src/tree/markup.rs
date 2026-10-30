//! Tree markups (PMI) and views, read past: `MarkupData` [WD 7.3.10.4]
//! and `MKP_View` (501) [WD 7.7.3-7.7.8, 7.4.5; ISS #698 #722 #723;
//! PRCRS]. Field order follows the spec RAG's
//! `threed/prc__8137__markup_view_entities.md`, which records where the
//! published draft is wrong. Only counts are kept; the markups' own
//! drawing is their `TESS_Markup` tessellations.

use crate::PrcError;
use crate::tess::{Ctx, malformed};

use super::REFERENCE_ON_PRC_BASE;

const MARKUP_LINKED_ITEM: u32 = 204;
const VIEW: u32 = 501;
const MARKUP: u32 = 502;
const LEADER: u32 = 503;
const ANNOTATION_ITEM: u32 = 504;
const ANNOTATION_SET: u32 = 505;
const ANNOTATION_REFERENCE: u32 = 506;
/// `is_annotation_view` exists from this authoring version, the view's
/// remaining optional fields from [`VIEW_DEFAULT_FROM`]: the dates the
/// writer's own schema program for 501 gives them (spec RAG §6; PRCRS
/// gates all of them at 8137, which differs only for files authored in
/// 7309..8137).
const VIEW_ANNOTATION_FROM: u32 = 7309;
const VIEW_DEFAULT_FROM: u32 = 8016;
/// Annotation sets nest; deeper than this is hostile.
const MAX_ANNOTATION_DEPTH: usize = 32;

impl Ctx<'_, '_> {
    /// `MarkupData`: linked items, leaders, markups, then annotation
    /// entities. Returns the number of markups.
    pub(super) fn markup_data(&mut self) -> Result<usize, PrcError> {
        let n = self.count(8, "markup linked items")?;
        for _ in 0..n {
            self.markup_linked_item()?;
        }
        let n = self.count(8, "markup leaders")?;
        for _ in 0..n {
            self.leader()?;
        }
        let markups = self.count(8, "markups")?;
        for _ in 0..markups {
            self.mkp_markup()?;
        }
        let n = self.count(8, "annotation entities")?;
        for _ in 0..n {
            self.annotation_entity(0)?;
        }
        Ok(markups)
    }

    /// A list of views; returns how many.
    pub(super) fn views(&mut self) -> Result<usize, PrcError> {
        let n = self.count(8, "views")?;
        for _ in 0..n {
            self.view()?;
        }
        Ok(n)
    }

    /// `MISC_MarkupLinkedItem` (204): a `ContentExtendedEntityReference`
    /// (an entity-reference body, then a second optional reference, ISS
    /// #722), then the four show/delete flags.
    fn markup_linked_item(&mut self) -> Result<(), PrcError> {
        self.expect_type(MARKUP_LINKED_ITEM)?;
        self.entity_reference_body()?;
        self.reference_data()?;
        for _ in 0..4 {
            self.r.bit()?; // show/delete markup, show/delete leader
        }
        self.end_entity(MARKUP_LINKED_ITEM)
    }

    /// `MKP_Leader` (503) [ISS #698]: two optional references and its
    /// tessellation.
    fn leader(&mut self) -> Result<(), PrcError> {
        self.expect_type(LEADER)?;
        self.base_with_graphics()?;
        for _ in 0..2 {
            if self.r.bit()? {
                self.reference_on_prc_base()?;
            }
        }
        self.r.unsigned_integer()?; // tessellation index + 1
        self.end_entity(LEADER)
    }

    /// `MKP_Markup` (502).
    fn mkp_markup(&mut self) -> Result<(), PrcError> {
        self.expect_type(MARKUP)?;
        self.base_with_graphics()?;
        self.r.unsigned_integer()?; // type
        self.r.unsigned_integer()?; // subtype
        self.references("markup linked-item references")?;
        self.references("markup leader references")?;
        self.r.unsigned_integer()?; // tessellation index + 1
        self.end_entity(MARKUP)
    }

    /// An annotation item (504), set (505) or reference (506), type code
    /// first. A set holds annotation entities, so `depth` caps nesting.
    fn annotation_entity(&mut self, depth: usize) -> Result<(), PrcError> {
        if depth > MAX_ANNOTATION_DEPTH {
            return Err(malformed("annotation sets nest too deep".into()));
        }
        let t = self.r.unsigned_integer()?;
        self.base_with_graphics()?;
        match t {
            ANNOTATION_ITEM => self.reference_on_prc_base()?,
            ANNOTATION_SET => {
                let n = self.count(8, "annotation set entities")?;
                for _ in 0..n {
                    self.annotation_entity(depth + 1)?;
                }
            }
            ANNOTATION_REFERENCE => self.references("annotation references")?,
            t => return Err(malformed(format!("entity type {t} as an annotation"))),
        }
        self.end_entity(t)
    }

    /// `MKP_View` (501): annotations, its plane, optional scene
    /// parameters, then the fields later authoring versions add.
    fn view(&mut self) -> Result<(), PrcError> {
        self.expect_type(VIEW)?;
        self.base_with_graphics()?;
        self.references("view annotations")?;
        self.surface_plane()?;
        if self.r.bit()? {
            self.scene_display_parameters()?;
        }
        if self.version >= VIEW_ANNOTATION_FROM {
            self.r.bit()?; // is_annotation_view
        }
        if self.version >= VIEW_DEFAULT_FROM {
            self.r.bit()?; // is_default_view
            self.r.bit()?; // is_direction
            self.references("view linked items")?;
            let n = self.count(8, "view filters")?;
            for _ in 0..n {
                self.filter()?;
            }
        }
        self.end_entity(VIEW)
    }

    /// A counted list of `ReferenceOnPRCBase`.
    fn references(&mut self, what: &'static str) -> Result<(), PrcError> {
        let n = self.count(8, what)?;
        for _ in 0..n {
            self.reference_on_prc_base()?;
        }
        Ok(())
    }

    /// `ReferenceOnPRCBase` (205) as a standalone entity: type code, the
    /// target's type, its file structure, its id. No schema, no user data
    /// [PRCRS].
    fn reference_on_prc_base(&mut self) -> Result<(), PrcError> {
        self.expect_type(REFERENCE_ON_PRC_BASE)?;
        self.r.unsigned_integer()?; // target type
        self.file_identifier()?;
        self.r.unsigned_integer()?; // target unique id
        Ok(())
    }

    /// Schema additions, then user data.
    fn end_entity(&mut self, t: u32) -> Result<(), PrcError> {
        self.schema.skip_added_fields(t, &mut self.r)?;
        self.user_data()
    }
}

#[cfg(test)]
pub(super) mod tests;

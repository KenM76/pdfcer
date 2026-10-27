//! Writing a derived look as a real `/AP` for markup annotations that have
//! none (decision 203, the opt-in half; pdfcer-gui request G236).

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite, PermissionBit};
use crate::annot::Appearance;
use crate::annot_author::{self, DERIVED_SUBTYPES, DerivedAppearance, FreeTextC};
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::page_tree::Rect;

/// One annotation [`EditSession::bake_missing_appearances`] gave an `/AP`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct BakedAppearance {
    /// The annotation, unchanged in identity.
    pub annot_id: ObjId,
    /// Its `/Subtype`.
    pub subtype: String,
    /// The new `/AP /N` form XObject.
    pub ap_id: ObjId,
    /// `/Rect` was rewritten to hold the look (e.g. a line's end caps); see
    /// [`annot_author::DerivedAppearance::place`].
    pub rect_changed: bool,
    /// What was settled rather than read (font substitution, `/C` reading).
    pub notes: Vec<String>,
}

/// One annotation [`EditSession::bake_missing_appearances`] left as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SkippedAppearance {
    /// The annotation.
    pub annot_id: ObjId,
    /// Its `/Subtype`.
    pub subtype: String,
    /// Why, in words.
    pub reason: String,
}

/// What [`EditSession::bake_missing_appearances`] did.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct BakeAppearancesOutcome {
    /// Annotations that now carry an `/AP`, in page order.
    pub baked: Vec<BakedAppearance>,
    /// Annotations of a derivable subtype that were left without one.
    pub skipped: Vec<SkippedAppearance>,
}

/// An in-class annotation with no `/AP /N`, and its dictionary.
struct Candidate {
    id: ObjId,
    subtype: String,
    rect: Option<Rect>,
    locked: bool,
    dict: Dict,
}

impl EditSession {
    /// Write the look pdfcer draws for an `/AP`-less markup annotation
    /// (Ink, Highlight, Underline, StrikeOut, Squiggly, Square, Circle,
    /// Polygon, PolyLine, Line, FreeText) into the file as its `/AP /N`, for
    /// a consumer that does not draw such annotations itself. One undo entry.
    ///
    /// The look is [`annot_author::derive_appearance`]'s — the one
    /// `pdfcer-render` paints without this verb — so a page renders the same
    /// before and after. Every key the file had is kept; `/AP` is added, and
    /// `/Rect` changes only when the look extends past it
    /// ([`BakedAppearance::rect_changed`]), because ISO 32000-1 §12.5.5 maps
    /// the appearance's `/BBox` onto `/Rect`. Only annotations with no `/AP
    /// /N` at all are touched; a pdf dimension, a callout and anything else
    /// [`annot_author::derive_appearance`] declines, and a locked annotation
    /// (Table 165 bit 8), are reported in
    /// [`BakeAppearancesOutcome::skipped`]. Nothing to bake commits nothing.
    ///
    /// # Errors
    ///
    /// [`EditError::DocumentEncrypted`] (no annotate permission), the
    /// certification gate, [`EditError::ObjectCreationWouldExposeHiddenObjects`]
    /// when there is something to bake, [`EditError::ObjectNumbersExhausted`],
    /// and page-tree errors.
    pub fn bake_missing_appearances(
        &mut self,
        free_text_c: FreeTextC,
    ) -> Result<BakeAppearancesOutcome, EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::Annotate]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification_for_annotation()?;
        let mut outcome = BakeAppearancesOutcome::default();
        let mut derived = Vec::new();
        for c in self.bake_candidates()? {
            let skip = |reason: String| SkippedAppearance {
                annot_id: c.id,
                subtype: c.subtype.clone(),
                reason,
            };
            if c.locked {
                outcome.skipped.push(skip("it is locked".to_owned()));
                continue;
            }
            match annot_author::derive_appearance(&self.graph(), &c.dict, free_text_c) {
                Ok(look) => derived.push((c, look)),
                Err(why) => outcome.skipped.push(skip(why.reason)),
            }
        }
        if derived.is_empty() {
            return Ok(outcome);
        }
        let suppressed = self.base.suppressed_object_count();
        if suppressed > 0 {
            return Err(EditError::ObjectCreationWouldExposeHiddenObjects { count: suppressed });
        }
        let mut objects = Vec::new();
        for (c, look) in derived {
            outcome.baked.push(self.bake_one(c, look, &mut objects)?);
        }
        self.commit(Command {
            kind: CommandKind::BakeAppearances {
                count: outcome.baked.len(),
            },
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(outcome)
    }

    /// Every indirect, derivable-subtype annotation with no `/AP /N`.
    fn bake_candidates(&self) -> Result<Vec<Candidate>, EditError> {
        let graph = self.graph();
        let mut out = Vec::new();
        for slot in self.page_slots()? {
            for annot in crate::annot::page_annotations(&graph, slot.id) {
                let subtype = annot.subtype_label();
                if !DERIVED_SUBTYPES.contains(&subtype.as_str())
                    || !matches!(annot.appearance, Appearance::None)
                {
                    continue;
                }
                let Some(id) = annot.id else { continue };
                let Some(Object::Dict(dict)) = self.value(id) else {
                    continue;
                };
                out.push(Candidate {
                    id,
                    subtype,
                    rect: annot.rect,
                    locked: annot.flags.locked(),
                    dict: dict.clone(),
                });
            }
        }
        Ok(out)
    }

    /// Stage the `/AP /N` stream and the annotation's new dictionary.
    fn bake_one(
        &mut self,
        c: Candidate,
        look: DerivedAppearance,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<BakedAppearance, EditError> {
        let placed = look.place(c.rect);
        let ap_id = ObjId::new(self.alloc_number()?, 0);
        let mut ap_dict = placed.ap_dict;
        ap_dict.insert(
            Name::from(b"Length"),
            Object::Integer(i64::try_from(look.ap_content.len()).unwrap_or(i64::MAX)),
        );
        let data_span = self.stage_bytes(&look.ap_content);
        objects.push(ObjectWrite {
            id: ap_id,
            before: None,
            after: Some(Object::Stream(Stream {
                dict: ap_dict,
                data_span,
            })),
        });
        let mut updated = c.dict;
        if placed.rect_changed {
            updated.insert(Name::from(b"Rect"), super::rect_array(placed.rect));
        }
        let mut ap = Dict::new();
        ap.insert(Name::from(b"N"), Object::Reference(ap_id));
        updated.insert(Name::from(b"AP"), Object::Dict(ap));
        objects.push(ObjectWrite {
            id: c.id,
            before: self.state.get(&c.id).cloned(),
            after: Some(Object::Dict(updated)),
        });
        Ok(BakedAppearance {
            annot_id: c.id,
            subtype: c.subtype,
            ap_id,
            rect_changed: placed.rect_changed,
            notes: look.notes,
        })
    }
}

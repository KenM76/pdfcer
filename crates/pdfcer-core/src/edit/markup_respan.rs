//! Replace a text markup's `/QuadPoints` and re-bake its appearance
//! (pdfcer-gui request G152): the drag-an-end-grip gesture on a Highlight,
//! Underline, StrikeOut or Squiggly.

use super::{
    AppearanceWrite, CommandKind, DroppedProperty, EditError, EditSession, MarkupSpec,
    PermissionBit, dropped_properties,
};
use crate::annot_author::{self, Quad};
use crate::object::{Name, ObjId, Object};
use crate::page_tree::Rect;

/// What [`EditSession::respan_text_markup`] did.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TextMarkupRespan {
    /// The annotation, unchanged in identity.
    pub annot_id: ObjId,
    /// Its `/Subtype`: `Highlight`, `Underline`, `StrikeOut` or `Squiggly`.
    pub subtype: String,
    /// Quadrilaterals before the call.
    pub quads_before: usize,
    /// Quadrilaterals after the call.
    pub quads_after: usize,
    /// The `/Rect` before, when the file had a readable one.
    pub rect_before: Option<Rect>,
    /// The `/Rect` after: the bound of the new quads.
    pub rect_after: Rect,
    /// How the appearance stream was written.
    pub appearance: AppearanceWrite,
    /// Properties the re-baked appearance does not reproduce. A foreign
    /// appearance (one pdfcer would not have drawn from the old quads) is
    /// always named here.
    pub dropped: Vec<DroppedProperty>,
    /// Whether `/M` was rewritten: only when the caller supplied a date.
    pub mod_date_written: bool,
}

impl EditSession {
    /// Replace a text markup's `/QuadPoints` with `quads` and re-bake its
    /// appearance, keeping colour, opacity, `/Contents`, replies and object
    /// identity (§12.5.6.10). One undo entry,
    /// [`CommandKind::RespanTextMarkup`].
    ///
    /// `quads` are in unrotated page space, y-up, one per line of covered
    /// text — the same quads a shell passes to `add_markup` when it creates
    /// a markup from a character selection, so a re-span from a new
    /// character span is "compute the span's quads, pass them here". They
    /// are written in the session's [`EditSession::quad_point_order`].
    ///
    /// `/M` is rewritten only when `modified` is `Some`; pdfcer reads no
    /// clock.
    ///
    /// # Errors
    ///
    /// - [`EditError::TextMarkupVerbOnOther`] — not a Highlight, Underline,
    ///   StrikeOut or Squiggly.
    /// - [`EditError::EmptyGeometry`] — `quads` is empty.
    /// - [`EditError::AnnotationVertexNotPlaceable`] — a non-finite corner.
    /// - [`EditError::AnnotationLocked`] — Table 165 bit 8 forbids changing
    ///   position and size. LockedContents (bit 10) does not refuse.
    /// - [`EditError::AnnotationNotFound`], [`EditError::DocumentEncrypted`],
    ///   [`EditError::CertificationForbidsChange`],
    ///   [`EditError::ObjectCreationWouldExposeHiddenObjects`],
    ///   [`EditError::ObjectNumbersExhausted`], [`EditError::NotADictionary`],
    ///   [`EditError::MarkupSpec`] — as for [`EditSession::reshape_annotation`].
    pub fn respan_text_markup(
        &mut self,
        annot_id: ObjId,
        quads: &[Quad],
        modified: Option<&str>,
    ) -> Result<TextMarkupRespan, EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::Annotate]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification_for_annotation()?;
        let (target, _all) = self.locate_annotation(annot_id)?;
        let subtype = String::from_utf8_lossy(&target.subtype).into_owned();
        let Some(Object::Dict(current)) = self.value(annot_id) else {
            return Err(EditError::NotADictionary {
                id: annot_id,
                key: "Subtype",
            });
        };
        let current = current.clone();
        let original = annot_author::spec_from_dict(&self.graph(), &current)?;
        let MarkupSpec::TextMarkup {
            kind,
            quads: old,
            color,
        } = &original
        else {
            return Err(EditError::TextMarkupVerbOnOther {
                id: annot_id,
                subtype,
            });
        };
        if target.flags.locked() {
            return Err(EditError::AnnotationLocked {
                id: annot_id,
                subtype,
            });
        }
        check_quads(annot_id, quads)?;

        let options = annot_author::AppearanceOptions {
            quad_order: self.quad_point_order,
            dash: None,
        };
        let was_pdfces = self.appearance_matches(
            &current,
            &annot_author::build_appearance_opts(&original, &options).ap_content,
        );
        let respanned = MarkupSpec::TextMarkup {
            kind: *kind,
            quads: quads.to_vec(),
            color: *color,
        };
        let quads_before = old.len();
        let authored = annot_author::build_appearance_opts(&respanned, &options);
        let mut regen = self.regenerate_markup_appearance(annot_id, &current, authored)?;
        if let Some(m) = modified {
            regen
                .updated
                .insert(Name::from(b"M"), Object::String(m.as_bytes().to_vec()));
        }
        let report = TextMarkupRespan {
            annot_id,
            subtype,
            quads_before,
            quads_after: quads.len(),
            rect_before: target.rect,
            rect_after: regen.rect_after,
            appearance: regen.appearance,
            dropped: dropped_properties(&self.graph(), &current, was_pdfces, false),
            mod_date_written: modified.is_some(),
        };
        self.commit_regenerated_markup(annot_id, regen, CommandKind::RespanTextMarkup);
        Ok(report)
    }
}

/// Refuse an empty quad list or a corner that is not a finite coordinate.
fn check_quads(id: ObjId, quads: &[Quad]) -> Result<(), EditError> {
    if quads.is_empty() {
        return Err(EditError::EmptyGeometry);
    }
    for q in quads {
        for (x, y) in [q.ul, q.ur, q.ll, q.lr] {
            if !x.is_finite() || !y.is_finite() {
                return Err(EditError::AnnotationVertexNotPlaceable { id, x, y });
            }
        }
    }
    Ok(())
}

//! Opacity for any annotation, and colour for the icon-style marker
//! subtypes (pdfcer-gui request G150).

use super::{
    AppearanceWrite, Command, CommandKind, EditError, EditSession, ObjectWrite, PermissionBit,
    StyleEdit,
};
use crate::annot_author::{
    self, AttachmentIcon, AuthoredTextAnnot, CaretSpec, CaretSymbol, Color, FileAttachmentSpec,
    ScreenSpec, SoundIcon, SoundSpec,
};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::Rect;

/// What [`EditSession::set_annot_opacity`] did.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct AnnotOpacityChange {
    /// The annotation, unchanged in identity.
    pub annot_id: ObjId,
    /// `/CA` before the call; `None` when absent (Table 164 default 1.0).
    pub previous: Option<f64>,
    /// `/CA` after the call; `None` when the key was removed.
    pub current: Option<f64>,
    /// The requested value lay outside `0.0..=1.0` and was clamped.
    pub clamped: bool,
}

/// A colour restyle of a marker annotation, applied by
/// [`EditSession::set_marker_style`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MarkerStyle {
    /// The new icon colour, written as `/C` and drawn into the appearance.
    pub color: Color,
    /// Redraw an appearance another program drew with pdfcer's own icon.
    /// Without it such a marker is refused
    /// ([`EditError::MarkerAppearanceForeign`]) and left unchanged.
    pub redraw_as_plain: bool,
}

impl MarkerStyle {
    /// Recolour to `color`, refusing a foreign appearance.
    #[must_use]
    pub fn new(color: Color) -> Self {
        Self {
            color,
            redraw_as_plain: false,
        }
    }
}

/// What [`EditSession::set_marker_style`] did.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MarkerStyleChange {
    /// The annotation, unchanged in identity.
    pub annot_id: ObjId,
    /// Its `/Subtype`.
    pub subtype: String,
    /// How the new `/AP /N` stream was written.
    pub appearance: AppearanceWrite,
    /// The appearance on disk was not pdfcer's drawing and was replaced
    /// (only possible with [`MarkerStyle::redraw_as_plain`]).
    pub appearance_was_foreign: bool,
}

/// The subtypes [`EditSession::set_marker_style`] recolours.
pub const MARKER_SUBTYPES: [&[u8]; 4] = [b"Caret", b"FileAttachment", b"Sound", b"Screen"];

impl EditSession {
    /// Set or remove an annotation's **constant opacity** `/CA`
    /// (ISO 32000-2 §12.5.2 Table 166 gives it to every annotation; ISO
    /// 32000-1 Table 170 to markup annotations). One undo entry.
    ///
    /// `/CA` is written to the annotation dictionary only, never into the
    /// appearance stream, so the appearance is not re-baked and its bytes
    /// are untouched. A finite value outside `0.0..=1.0` is clamped and
    /// reported in [`AnnotOpacityChange::clamped`], as
    /// [`super::MarkupStyle::opacity`] does. A call that changes nothing
    /// commits nothing.
    ///
    /// # Errors
    ///
    /// - [`EditError::MarkupOpacityOutOfRange`] for a NaN or infinite value.
    /// - [`EditError::AnnotationLocked`] — Table 165 bit 8.
    /// - [`EditError::AnnotationNotFound`], [`EditError::NotADictionary`],
    ///   [`EditError::DocumentEncrypted`] and the certification gate.
    pub fn set_annot_opacity(
        &mut self,
        annot_id: ObjId,
        opacity: StyleEdit<f64>,
    ) -> Result<AnnotOpacityChange, EditError> {
        let (current, _) = self.restyle_target(annot_id)?;
        let (wanted, clamped) = match opacity {
            StyleEdit::Set(a) if !a.is_finite() => {
                return Err(EditError::MarkupOpacityOutOfRange { value: a });
            }
            StyleEdit::Set(a) => (Some(a.clamp(0.0, 1.0)), !(0.0..=1.0).contains(&a)),
            StyleEdit::Clear => (None, false),
        };
        let graph = self.graph();
        let on_disk = current.get(b"CA").map(|o| graph.resolve(o));
        let previous = on_disk.and_then(Object::as_number);
        let malformed = on_disk.is_some() && previous.is_none();
        let change = AnnotOpacityChange {
            annot_id,
            previous,
            current: wanted,
            clamped,
        };
        if previous == wanted && !malformed {
            return Ok(change);
        }
        let mut updated = current;
        match wanted {
            Some(a) => {
                updated.insert(Name::from(b"CA"), Object::Real(a));
            }
            None => {
                updated.remove(b"CA");
            }
        }
        self.commit(Command {
            kind: CommandKind::SetAnnotOpacity,
            objects: vec![ObjectWrite {
                id: annot_id,
                before: self.state.get(&annot_id).cloned(),
                after: Some(Object::Dict(updated)),
            }],
            removals: Vec::new(),
            trailer: None,
        });
        Ok(change)
    }

    /// Recolour a **marker** annotation — `/Caret` (§12.5.6.11),
    /// `/FileAttachment` (§12.5.6.15), `/Sound` (§12.5.6.16) or `/Screen`
    /// (§12.5.6.18) — writing `/C` and re-baking its `/AP /N` icon, keeping
    /// its object identity, icon name, symbol and every other key. One undo
    /// entry.
    ///
    /// The appearance on disk is first compared with pdfcer's drawing of
    /// the marker's current colour and icon. A mismatch means another
    /// program drew it; that is refused unless
    /// [`MarkerStyle::redraw_as_plain`], the same contract as
    /// [`EditSession::set_text_annot_style`]. A marker with no `/AP` gains
    /// one.
    ///
    /// # Errors
    ///
    /// - [`EditError::StylePropertyNotApplicable`] — not one of
    ///   [`MARKER_SUBTYPES`].
    /// - [`EditError::MarkerAppearanceForeign`] — see above.
    /// - [`EditError::AnnotationRectMissing`], [`EditError::AnnotationLocked`],
    ///   [`EditError::AnnotationNotFound`], [`EditError::NotADictionary`],
    ///   [`EditError::DocumentEncrypted`], the certification gate,
    ///   [`EditError::AppearanceHasStates`] and
    ///   [`EditError::ObjectCreationWouldExposeHiddenObjects`].
    pub fn set_marker_style(
        &mut self,
        annot_id: ObjId,
        style: &MarkerStyle,
    ) -> Result<MarkerStyleChange, EditError> {
        let (current, target) = self.restyle_target(annot_id)?;
        let subtype = target.subtype_label();
        if !MARKER_SUBTYPES.contains(&target.subtype.as_slice()) {
            return Err(EditError::StylePropertyNotApplicable {
                id: annot_id,
                subtype,
                property: "marker colour",
            });
        }
        let rect = target
            .rect
            .ok_or_else(|| EditError::AnnotationRectMissing {
                subtype: subtype.clone(),
            })?;
        let as_is = self.marker_appearance(&current, &target.subtype, rect, None);
        let foreign = current.contains_key(b"AP")
            && !as_is.is_some_and(|a| self.appearance_matches(&current, &a.ap_content));
        if foreign && !style.redraw_as_plain {
            return Err(EditError::MarkerAppearanceForeign { id: annot_id });
        }
        let authored = self
            .marker_appearance(&current, &target.subtype, rect, Some(style.color))
            .ok_or_else(|| EditError::StylePropertyNotApplicable {
                id: annot_id,
                subtype: subtype.clone(),
                property: "marker colour",
            })?;
        let regen = self.regenerate_markup_appearance(
            annot_id,
            &current,
            annot_author::AuthoredAppearance {
                annot: authored.annot,
                ap_dict: authored.ap_dict,
                ap_content: authored.ap_content,
                rect: authored.rect,
            },
        )?;
        let appearance = regen.appearance;
        self.commit_regenerated_markup(annot_id, regen, CommandKind::SetMarkerStyle);
        Ok(MarkerStyleChange {
            annot_id,
            subtype,
            appearance,
            appearance_was_foreign: foreign,
        })
    }

    /// The guards both restyles share, then the annotation's dictionary.
    fn restyle_target(
        &self,
        annot_id: ObjId,
    ) -> Result<(Dict, crate::annot::Annotation), EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::Annotate]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification_for_annotation()?;
        let (target, _all) = self.locate_annotation(annot_id)?;
        if target.flags.locked() {
            return Err(EditError::AnnotationLocked {
                id: annot_id,
                subtype: target.subtype_label(),
            });
        }
        let Some(Object::Dict(current)) = self.value(annot_id) else {
            return Err(EditError::NotADictionary {
                id: annot_id,
                key: "Subtype",
            });
        };
        Ok((current.clone(), target))
    }

    /// pdfcer's drawing of the marker described by `dict`, in `color` or
    /// else its own `/C` (else the authoring default). `None` for a
    /// subtype outside [`MARKER_SUBTYPES`].
    fn marker_appearance(
        &self,
        dict: &Dict,
        subtype: &[u8],
        rect: Rect,
        color: Option<Color>,
    ) -> Option<AuthoredTextAnnot> {
        let graph = self.graph();
        let own = annot_author::read_color(&graph, dict, b"C");
        let name = |key: &[u8]| match dict.get(key).map(|o| graph.resolve(o)) {
            Some(Object::Name(n)) => Some(n.0.clone()),
            _ => None,
        };
        let pick = |default: Color| color.or(own).unwrap_or(default);
        match subtype {
            b"Caret" => {
                let mut spec = CaretSpec::new(rect);
                spec.color = pick(spec.color);
                spec.symbol = name(b"Sy")
                    .and_then(|n| CaretSymbol::from_name(&n))
                    .unwrap_or_default();
                Some(annot_author::caret(&spec))
            }
            b"FileAttachment" => {
                let mut spec = FileAttachmentSpec::new(rect, "", Vec::new());
                spec.color = pick(spec.color);
                spec.icon = name(b"Name").map_or(AttachmentIcon::PushPin, |n| {
                    AttachmentIcon::from_name_lossless(&n)
                });
                Some(annot_author::file_attachment(&spec))
            }
            b"Sound" => {
                let mut spec = SoundSpec::new(rect, crate::sound::SoundData::empty());
                spec.color = pick(spec.color);
                spec.icon =
                    name(b"Name").map_or(SoundIcon::Speaker, |n| SoundIcon::from_name_lossless(&n));
                Some(annot_author::sound(&spec))
            }
            b"Screen" => {
                let mut spec = ScreenSpec::new(rect, "", "", Vec::new());
                spec.color = pick(spec.color);
                Some(annot_author::screen(&spec))
            }
            _ => None,
        }
    }
}

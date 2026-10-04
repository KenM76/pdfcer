//! Replacing a button's foreign appearance whole, under
//! `WidgetEdit::foreign_appearance` (§12.7.4.2.2, §12.7.4.2.3).
//!
//! The ownership test in `regen_button_appearance` refuses to redraw artwork
//! pdfcer did not draw. This is the opt-in past it: the widget gets pdfcer's
//! own `Off` and on-state streams under NEW object ids, keyed by the on-state
//! name the foreign artwork already used, so `/AS`, `/V` and every sibling
//! widget's state names stay valid. The foreign streams are left unreferenced
//! rather than overwritten, and `/D` and `/R` are dropped with them: a down
//! or rollover state drawn in the old style would contradict the new one. A
//! push button, which has no states, gets one new `/N` stream.

use super::{ButtonApPlan, EditError, EditSession, ObjectWrite};
use crate::annot_author::CheckBoxStateAppearance;
use crate::forms::{self, ButtonKind};
use crate::object::{Dict, Name, ObjId, Object, Stream};

/// When a widget edit may replace button artwork another producer drew
/// ([`WidgetEdit::foreign_appearance`](super::WidgetEdit::foreign_appearance)).
///
/// pdfcer always redraws artwork it drew itself. Foreign artwork is replaced
/// whole: a check box or radio button gets pdfcer's own on and off states
/// from its `/MK` and `/BS`, a push button one new `/AP /N`, and foreign `/D`
/// (down) and `/R` (rollover) states are dropped.
/// [`WidgetEditOutcome::foreign_appearance_replaced`](super::WidgetEditOutcome::foreign_appearance_replaced)
/// reports it. A check box whose `/AP /N` names more than one on state is
/// never replaced. Kept artwork is reported as
/// [`AppearanceOutcome::RecordedNotPainted`](super::AppearanceOutcome::RecordedNotPainted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ForeignAppearance {
    /// Never replace foreign artwork; the edit is recorded in `/MK` only.
    Keep,
    /// Replace a foreign push button when the edit sets or clears its icon
    /// or sets its caption position: viewers paint `/AP`, not `/MK /I`, so
    /// an icon kept out of the appearance never shows. Other edits keep
    /// foreign artwork, which pdfcer's plain style would otherwise discard.
    #[default]
    ReplaceOnIconEdit,
    /// Replace foreign artwork whenever the edit needs a redraw.
    Replace,
}

impl ForeignAppearance {
    /// Whether an edit that does (`touches_icon`) or does not touch the
    /// icon layout replaces foreign artwork.
    pub(super) const fn replaces(self, touches_icon: bool) -> bool {
        match self {
            Self::Keep => false,
            Self::ReplaceOnIconEdit => touches_icon,
            Self::Replace => true,
        }
    }
}

/// What pass 2 of `regen_button_appearance` does with one widget.
pub(super) enum ButtonSlot {
    /// pdfcer's own artwork: rewrite these streams in place.
    Own(ButtonApPlan),
    /// Foreign artwork the operator opted to replace: new `Off` and this
    /// on-state, or for a push button (`None`) one new stream.
    Foreign(Option<Vec<u8>>),
}

impl EditSession {
    /// The on-state name a foreign check box or radio widget's replacement is
    /// keyed by, or `None` when it cannot be named unambiguously.
    ///
    /// The single non-`Off` key of `/AP /N`; with none, a non-`Off` `/AS`.
    /// Several on-states (a widget that can show more than one value) are
    /// not pdfcer's two-state shape, so they are not replaced.
    pub(super) fn foreign_on_state(widget: &forms::Widget, kind: ButtonKind) -> Option<Vec<u8>> {
        if kind == ButtonKind::Push {
            return None;
        }
        match widget.on_states.as_slice() {
            [one] => Some(one.clone()),
            [] => widget
                .appearance_state
                .clone()
                .filter(|s| !s.is_empty() && s.as_slice() != b"Off"),
            _ => None,
        }
    }

    /// Stage `redrawn` (`[off, on]`, or a push button's `[plate]` when `on`
    /// is `None`) as new streams and point `widget_id`'s `/AP` at them,
    /// replacing the whole `/AP`.
    ///
    /// Patches the widget write this command has already staged, if any:
    /// a second whole-dictionary write to the same object would discard the
    /// first one's edits.
    pub(super) fn replace_foreign_button(
        &mut self,
        widget_id: ObjId,
        on: Option<&[u8]>,
        redrawn: Vec<CheckBoxStateAppearance>,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<(), EditError> {
        let mut states = Dict::default();
        let mut single = None;
        let names: Vec<&[u8]> = on.map_or_else(Vec::new, |on| vec![b"Off".as_slice(), on]);
        for (i, content) in redrawn.into_iter().enumerate() {
            let id = ObjId::new(self.alloc_number()?, 0);
            let mut dict = content.ap_dict;
            self.bind_dr_fonts(&mut dict);
            dict.insert(
                Name::from(b"Length"),
                Object::Integer(i64::try_from(content.content.len()).unwrap_or(i64::MAX)),
            );
            let span = self.stage_bytes(&content.content);
            objects.push(ObjectWrite {
                id,
                before: None,
                after: Some(Object::Stream(Stream {
                    dict,
                    data_span: span,
                })),
            });
            match names.get(i) {
                Some(name) => states.insert(Name(name.to_vec()), Object::Reference(id)),
                None => single = Some(Object::Reference(id)),
            }
        }
        let mut ap = Dict::default();
        ap.insert(Name::from(b"N"), single.unwrap_or(Object::Dict(states)));
        if let Some(Object::Dict(existing)) = objects
            .iter_mut()
            .find(|w| w.id == widget_id)
            .and_then(|w| w.after.as_mut())
        {
            existing.insert(Name::from(b"AP"), Object::Dict(ap));
            return Ok(());
        }
        let Some(Object::Dict(dict)) = self.value(widget_id) else {
            return Err(EditError::NotADictionary {
                id: widget_id,
                key: "AP",
            });
        };
        let mut updated = dict.clone();
        updated.insert(Name::from(b"AP"), Object::Dict(ap));
        objects.push(ObjectWrite {
            id: widget_id,
            before: self.state.get(&widget_id).cloned(),
            after: Some(Object::Dict(updated)),
        });
        Ok(())
    }
}

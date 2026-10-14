//! Moving a radio member from one radio group into another (§12.7.4.2.3:
//! a member's identity is the non-`Off` key of its `/AP /N`).

use super::radio_state::rename_appearance_state;
use super::{Command, CommandKind, EditError, EditSession, ObjectWrite, Removal};
use crate::forms;
use crate::forms_author::{self, FieldShape};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};

/// What [`EditSession::move_widget_to_field`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WidgetRegroup {
    /// The moved widget. Its object number is unchanged.
    pub widget: ObjId,
    /// The on-state (export value) it carries in its new group: its old one,
    /// or a fresh one when the old one was already a member's there.
    pub on_state: Option<String>,
    /// Whether the source group was left with no buttons and removed.
    pub source_removed: bool,
    /// Action target names (`/ResetForm`, `/SubmitForm`, `/Hide`) that named
    /// the removed source group and now name nothing. Zero when the source
    /// survives.
    pub action_targets_orphaned: usize,
    /// One line per thing the move decided on its own (a renamed on-state,
    /// a removed source group, a cleared selection).
    pub disclosures: Vec<String>,
}

/// Field keys a merged field+widget sheds when it becomes only a widget.
/// `/AA` stays: a widget's own triggers (`/E`, `/X`, `/D`, `/U`) live there.
const SHED_KEYS: &[&[u8]] = &[
    b"T", b"FT", b"Kids", b"Ff", b"V", b"DV", b"Opt", b"TU", b"TM", b"MaxLen", b"Q", b"DS", b"RV",
    b"TI", b"I",
];

impl EditSession {
    /// **Move one radio button into another radio group** as one undoable
    /// command.
    ///
    /// Widget `index` of radio field `fqn` (numbered as
    /// [`forms::Field::widgets`] lists them) becomes a member of radio field
    /// `target_fqn`. The widget keeps its object number and its place in its
    /// page's `/Annots`; only its `/Parent` and the two fields' member lists
    /// change.
    ///
    /// - Its on-state is kept unless a member of the target already exports
    ///   it; then it is renamed to the lowest free `stem` + `n` throughout
    ///   its `/AP` (`/N`, `/D`, `/R`), copying a shared appearance
    ///   dictionary inline rather than renaming it for its other users.
    /// - It shows selected only if the target's `/V` is its on-state.
    /// - A source left with no buttons is removed from the form, as
    ///   [`Self::delete_field`] would remove it; a source that survives and
    ///   was selected on this button becomes unselected.
    /// - A target that is one merged field+widget is first split into a
    ///   field with one kid, as a paste into it would.
    ///
    /// # Errors
    ///
    /// - [`EditError::FieldNotFound`] when either name is not a field.
    /// - [`EditError::NotARadioGroup`] when either is not a radio group.
    /// - [`EditError::WidgetAlreadyInField`] when they are the same field.
    /// - [`EditError::RadioGroupUsesPositionalOpt`] when either carries
    ///   `/Opt`, whose entries the on-states index (§12.7.4.2.4).
    /// - [`EditError::WidgetIndexOutOfRange`] for a bad `index`.
    /// - The encryption and strict certification guards.
    pub fn move_widget_to_field(
        &mut self,
        fqn: &str,
        index: usize,
        target_fqn: &str,
    ) -> Result<WidgetRegroup, EditError> {
        let (source, _) = self.deletion_preflight(fqn)?;
        let (target, _) = self.deletion_preflight(target_fqn)?;
        self.refuse_regroup(&source, &target)?;
        let Some(member) = source.widgets.get(index).cloned() else {
            return Err(EditError::WidgetIndexOutOfRange {
                name: fqn.to_owned(),
                index,
                widgets: source.widgets.len(),
            });
        };
        let mut disclosures = Vec::new();
        let (widget, on_state) = self.regrouped_widget(&member, &target, &mut disclosures)?;
        let source_removed = source.widgets.len() == 1;
        let (mut objects, removals, orphaned) =
            self.release_from_source(&source, &member, source_removed, &mut disclosures)?;
        objects.extend(self.adopt_into_target(&target, member.id)?);
        objects.push(ObjectWrite {
            id: member.id,
            before: self.state.get(&member.id).cloned(),
            after: Some(Object::Dict(widget)),
        });
        self.commit(Command {
            kind: CommandKind::MoveWidgetToField,
            objects,
            removals,
            trailer: None,
        });
        Ok(WidgetRegroup {
            widget: member.id,
            on_state: on_state.map(|s| String::from_utf8_lossy(&s).into_owned()),
            source_removed,
            action_targets_orphaned: orphaned,
            disclosures,
        })
    }

    fn refuse_regroup(
        &self,
        source: &forms::Field,
        target: &forms::Field,
    ) -> Result<(), EditError> {
        for f in [source, target] {
            let fqn = f.fully_qualified_name.clone();
            if f.button_kind != Some(forms::ButtonKind::Radio) {
                return Err(EditError::NotARadioGroup { fqn });
            }
            if self.field_has_opt(f.id) {
                return Err(EditError::RadioGroupUsesPositionalOpt { fqn });
            }
        }
        if source.id == target.id {
            return Err(EditError::WidgetAlreadyInField {
                fqn: target.fully_qualified_name.clone(),
            });
        }
        Ok(())
    }

    /// The widget dictionary as a member of `target`: field keys shed when
    /// it was merged, `/Parent` set, on-state claimed, `/AS` agreeing with
    /// the target's `/V`.
    fn regrouped_widget(
        &self,
        member: &forms::Widget,
        target: &forms::Field,
        disclosures: &mut Vec<String>,
    ) -> Result<(Dict, Option<Vec<u8>>), EditError> {
        let Some(Object::Dict(dict)) = self.value(member.id) else {
            return Err(EditError::NotAWidget { id: member.id.num });
        };
        let mut widget = dict.clone();
        if member.merged {
            for key in SHED_KEYS {
                widget.remove(key);
            }
        }
        widget.insert(Name::from(b"Parent"), Object::Reference(target.id));
        if let Some(taken) = self.radio_export_collision(target.id, &widget, &[]) {
            let fresh = self.fresh_radio_state(target.id, &taken);
            self.inline_appearance_dicts(&mut widget);
            rename_appearance_state(&mut widget, &mut [], &taken, &fresh);
            disclosures.push(format!(
                "move: \"{}\" already exports \"{}\", so the moved button exports \"{}\".",
                target.fully_qualified_name,
                String::from_utf8_lossy(&taken),
                String::from_utf8_lossy(&fresh),
            ));
        }
        let on_state = self.pending_on_states(&widget, &[]).into_iter().next();
        let selected = matches!(
            (&target.value, &on_state),
            (forms::FieldValue::Name(v), Some(s)) if v == s
        );
        let shown: &[u8] = match (&on_state, selected) {
            (Some(s), true) => s,
            _ => b"Off",
        };
        widget.insert(Name::from(b"AS"), Object::Name(Name::from(shown)));
        Ok((widget, on_state))
    }

    /// Copy `/AP` and its `/N`, `/D`, `/R` sub-dictionaries inline, so a
    /// rename touches this widget alone and never an object another widget
    /// shares. The appearance streams themselves stay shared.
    fn inline_appearance_dicts(&self, widget: &mut Dict) {
        let graph = self.graph();
        let Some(mut ap) = widget
            .get(b"AP")
            .and_then(|o| graph.resolve(o).as_dict().cloned())
        else {
            return;
        };
        for key in [&b"N"[..], b"D", b"R"] {
            if let Some(Object::Reference(_)) = ap.get(key)
                && let Some(d) = ap
                    .get(key)
                    .and_then(|o| graph.resolve(o).as_dict().cloned())
            {
                ap.insert(Name::from(key), Object::Dict(d));
            }
        }
        widget.insert(Name::from(b"AP"), Object::Dict(ap));
    }

    /// The source side: either the whole source field leaves the form (the
    /// widget was its last), or the widget leaves its `/Kids`.
    fn release_from_source(
        &mut self,
        source: &forms::Field,
        member: &forms::Widget,
        source_removed: bool,
        disclosures: &mut Vec<String>,
    ) -> Result<(Vec<ObjectWrite>, Vec<Removal>, usize), EditError> {
        let name = source.fully_qualified_name.clone();
        if source_removed {
            let (orphaned, _) = self.sweep_action_targets(&|n: &str| n == name, None);
            let (writes, emptied) = self.remove_fields_from_form(&[source.id])?;
            // A merged member IS the source dict, and it lives on as the widget.
            let doomed: Vec<ObjId> = (!member.merged)
                .then_some(source.id)
                .into_iter()
                .chain(emptied)
                .collect();
            self.refuse_if_in_page_tree(&format!("the form field {name:?}"), &doomed)?;
            let removals = doomed
                .into_iter()
                .map(|id| Removal {
                    id,
                    was_deleted: self.deleted.contains(&id),
                    is_deleted: true,
                })
                .collect();
            disclosures.push(format!(
                "move: \"{name}\" had no other buttons, so it was removed from the form."
            ));
            return Ok((writes, removals, orphaned));
        }
        let Some(Object::Dict(dict)) = self.value(source.id) else {
            return Err(EditError::NotADictionary {
                id: source.id,
                key: "Kids",
            });
        };
        let mut updated = dict.clone();
        if let Some(Object::Array(kids)) = updated.get(b"Kids").cloned() {
            let kept: Vec<Object> = kids
                .into_iter()
                .filter(|o| o.as_reference() != Some(member.id))
                .collect();
            updated.insert(Name::from(b"Kids"), Object::Array(kept));
        }
        if matches!(&source.value, forms::FieldValue::Name(v) if member.on_states.contains(v)) {
            updated.insert(Name::from(b"V"), Object::Name(Name::from(b"Off")));
            disclosures.push(format!(
                "move: \"{name}\" was selected on the moved button, so it is now unselected."
            ));
        }
        let write = ObjectWrite {
            id: source.id,
            before: self.state.get(&source.id).cloned(),
            after: Some(Object::Dict(updated)),
        };
        Ok((vec![write], Vec::new(), 0))
    }

    /// The target side: `widget` appended to its `/Kids`, splitting a merged
    /// target into field + one kid first.
    fn adopt_into_target(
        &mut self,
        target: &forms::Field,
        widget: ObjId,
    ) -> Result<Vec<ObjectWrite>, EditError> {
        let shape =
            match forms_author::resolve_field_path(&self.graph(), &target.fully_qualified_name) {
                Ok(forms_author::FieldPath::Terminal { shape, .. }) => shape,
                _ => {
                    return Err(EditError::FieldNotFound {
                        name: target.fully_qualified_name.clone(),
                    });
                }
            };
        let Some(Object::Dict(dict)) = self.value(target.id) else {
            return Err(EditError::NotADictionary {
                id: target.id,
                key: "Kids",
            });
        };
        let dict = dict.clone();
        let mut updated = dict.clone();
        let (mut writes, mut kids) = match shape {
            FieldShape::MergedSingleWidget => {
                let slots = self.page_slots()?;
                let (writes, promoted, _) =
                    self.promote_merged_field(target.id, &dict, &mut updated, &slots, None)?;
                (writes, vec![Object::Reference(promoted)])
            }
            FieldShape::KidsWidgets { .. } => {
                let kids = dict
                    .get(b"Kids")
                    .and_then(|o| self.graph().resolve(o).as_array().map(<[Object]>::to_vec))
                    .unwrap_or_default();
                (Vec::new(), kids)
            }
        };
        kids.push(Object::Reference(widget));
        updated.insert(Name::from(b"Kids"), Object::Array(kids));
        writes.push(ObjectWrite {
            id: target.id,
            before: self.state.get(&target.id).cloned(),
            after: Some(Object::Dict(updated)),
        });
        Ok(writes)
    }
}

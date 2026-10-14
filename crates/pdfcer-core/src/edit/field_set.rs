//! Several form fields copied and pasted as one: one shared closure on the
//! way out, one all-or-nothing undo entry on the way in.

use std::collections::{BTreeMap, BTreeSet};

use super::{CommandKind, EditError, EditSession, copyable_field, unique_field_name};
use crate::formclip::{
    FieldClipSet, FieldPasteOutcome, FieldPastePolicy, FieldSetPasteOutcome, FieldSetPastePolicy,
    MAX_CLIP_SET_FIELDS,
};
use crate::forms;
use crate::page_tree::Rect;
use crate::vector::ClipError;

/// One single-field paste of a batch.
struct Step {
    member: usize,
    widget: Option<usize>,
    rect: Rect,
    policy: FieldPastePolicy,
}

fn moved(rect: Rect, (dx, dy): (f64, f64)) -> Rect {
    Rect {
        llx: rect.llx + dx,
        lly: rect.lly + dy,
        urx: rect.urx + dx,
        ury: rect.ury + dy,
    }
}

impl EditSession {
    /// **Copy several form fields as one clip** (`Pass 587.0`).
    ///
    /// The fields share one object closure, so a font, appearance or action
    /// two of them use travels once ([`FieldClipSet::object_count`]) and
    /// [`Self::paste_fields`] lands it once. A name given twice is copied
    /// once; order is first appearance.
    ///
    /// # Errors
    ///
    /// The same as [`Self::copy_field`], for the first field that fails —
    /// [`EditError::NoInteractiveForm`], [`EditError::FieldNotFound`],
    /// [`EditError::FieldHasNoWidget`], [`EditError::SignedFieldNotCopyable`]
    /// — and [`EditError::Clip`] with `ClipTooLarge` for more than
    /// [`MAX_CLIP_SET_FIELDS`] fields or a closure over the object ceiling.
    pub fn copy_fields(&self, fqns: &[&str]) -> Result<FieldClipSet, EditError> {
        let mut seen = BTreeSet::new();
        let wanted: Vec<&str> = fqns.iter().copied().filter(|f| seen.insert(*f)).collect();
        if wanted.len() > MAX_CLIP_SET_FIELDS {
            return Err(ClipError::ClipTooLarge {
                found: wanted.len(),
                limit: MAX_CLIP_SET_FIELDS,
            }
            .into());
        }
        let form = forms::parse_acroform(&self.graph()).ok_or(EditError::NoInteractiveForm)?;
        let view = self.view();
        let mut closure = crate::formclip::Closure::new(&view);
        let mut fields = Vec::with_capacity(wanted.len());
        for fqn in wanted {
            let field = form
                .field_by_name(fqn)
                .ok_or_else(|| EditError::FieldNotFound {
                    name: fqn.to_owned(),
                })?;
            copyable_field(field, fqn)?;
            fields.push(crate::formclip::build_field_clip(
                &mut closure,
                &view,
                &form,
                field,
            )?);
        }
        Ok(FieldClipSet {
            fields,
            objects: closure.objects,
        })
    }

    /// **Paste several copied form fields onto a page** as ONE undoable
    /// command, all or nothing (`Pass 587.0`).
    ///
    /// The selection keeps its layout: everything is translated so the
    /// lower-left of [`FieldClipSet::bbox`] lands on `origin` (page user
    /// space, points). Each field then lands exactly as [`Self::paste_field`]
    /// would place it at its translated rectangle.
    ///
    /// - [`FieldSetPastePolicy::NewFields`] creates one field per member. The
    ///   names are chosen here — the source name when free, else the first
    ///   free `_2`, `_3`, … — against the document and the batch together;
    ///   every rename is disclosed and [`FieldSetPasteOutcome::names`] lists
    ///   them. With `copy_actions`, a calculate script naming another member
    ///   (a quoted literal) is re-pointed at that member's copy, disclosed.
    /// - [`FieldSetPastePolicy::AdditionalWidgets`] adds every copied widget
    ///   to the field bearing its source name here. Radio members are
    ///   committed one after another, so each claims an on-state the group
    ///   and the batch do not already use.
    ///
    /// A font several members' `/DA` name is installed in `/DR` once.
    /// Disclosures are collected into [`FieldSetPasteOutcome::disclosures`],
    /// each sentence once. An empty set pastes nothing and returns an empty
    /// outcome.
    ///
    /// # Errors
    ///
    /// Any refusal of [`Self::paste_field`], for the first member that
    /// refuses; nothing is written and the undo stack is unchanged.
    /// [`EditError::SelectionTooLargeForOneUndo`] when the batch needs more
    /// than [`MAX_UNDO_DEPTH`](super::MAX_UNDO_DEPTH) steps.
    pub fn paste_fields(
        &mut self,
        set: &FieldClipSet,
        page_index: usize,
        origin: (f64, f64),
        policy: &FieldSetPastePolicy,
    ) -> Result<FieldSetPasteOutcome, EditError> {
        let mut disclosures = Vec::new();
        let Some(bbox) = set.bbox() else {
            return Ok(FieldSetPasteOutcome {
                fields: Vec::new(),
                names: Vec::new(),
                disclosures,
            });
        };
        let delta = (origin.0 - bbox.llx, origin.1 - bbox.lly);
        let mut set = set.clone();
        let (steps, names) = match policy {
            FieldSetPastePolicy::NewFields {
                tooltip,
                copy_value,
                copy_actions,
            } => {
                let names = self.batch_field_names(&set, &mut disclosures);
                if *copy_actions {
                    repoint(&mut set, &names, &mut disclosures);
                }
                let steps = set
                    .fields
                    .iter()
                    .zip(&names)
                    .enumerate()
                    .filter_map(|(member, (clip, name))| {
                        Some(Step {
                            member,
                            widget: None,
                            rect: moved(clip.widgets().first()?.rect(), delta),
                            policy: FieldPastePolicy::NewField {
                                name: name.clone(),
                                tooltip: tooltip.clone(),
                                copy_value: *copy_value,
                                copy_actions: *copy_actions,
                            },
                        })
                    })
                    .collect();
                (steps, names)
            }
            FieldSetPastePolicy::AdditionalWidgets => additional_widget_steps(&set, delta),
        };

        let mut imported = BTreeMap::new();
        let mut fields =
            self.all_or_nothing(&steps, CommandKind::PasteFormField, |session, step| {
                set.with_member(step.member, |clip| {
                    session.paste_field_with(
                        clip,
                        page_index,
                        step.rect,
                        &step.policy,
                        &mut imported,
                        step.widget,
                    )
                })
                .transpose()
            })?;
        collect_disclosures(&mut fields, &mut disclosures);
        Ok(FieldSetPasteOutcome {
            fields,
            names,
            disclosures,
        })
    }

    /// The name each member of a `NewFields` paste lands under, disclosing
    /// every one that differs from its source.
    fn batch_field_names(&self, set: &FieldClipSet, disclosures: &mut Vec<String>) -> Vec<String> {
        let mut taken: BTreeSet<String> = forms::parse_acroform(&self.graph())
            .map(|f| {
                f.fields
                    .into_iter()
                    .map(|x| x.fully_qualified_name)
                    .chain(f.groups.into_iter().map(|g| g.fully_qualified_name))
                    .collect()
            })
            .unwrap_or_default();
        let mut names = Vec::with_capacity(set.len());
        for source in set.source_names() {
            let name = if taken.contains(source) {
                let renamed = unique_field_name(source, &taken);
                disclosures.push(format!(
                    "paste: \"{source}\" is already a field name here, so its copy is named \"{renamed}\"."
                ));
                renamed
            } else {
                source.to_owned()
            };
            taken.insert(name.clone());
            names.push(name);
        }
        names
    }
}

/// Re-point calculate scripts at the copies, disclosing each script changed.
fn repoint(set: &mut FieldClipSet, names: &[String], disclosures: &mut Vec<String>) {
    let renames: BTreeMap<String, String> = set
        .source_names()
        .into_iter()
        .zip(names)
        .filter(|(source, name)| source != name)
        .map(|(source, name)| (source.to_owned(), name.clone()))
        .collect();
    if renames.is_empty() {
        return;
    }
    for source in set.repoint_calculations(&renames) {
        disclosures.push(format!(
            "paste: the calculation copied from \"{source}\" now refers to the pasted copies of the fields it named, not the originals."
        ));
    }
}

/// One step per copied widget, each added to the field of its source name.
fn additional_widget_steps(set: &FieldClipSet, delta: (f64, f64)) -> (Vec<Step>, Vec<String>) {
    let mut steps = Vec::new();
    for (member, clip) in set.fields.iter().enumerate() {
        for (widget, w) in clip.widgets().iter().enumerate() {
            steps.push(Step {
                member,
                widget: Some(widget),
                rect: moved(w.rect(), delta),
                policy: FieldPastePolicy::AdditionalWidget {
                    existing: clip.source_name().to_owned(),
                },
            });
        }
    }
    let names = set.source_names().into_iter().map(str::to_owned).collect();
    (steps, names)
}

/// Move every step's sentences into `disclosures`, each once, in order.
fn collect_disclosures(fields: &mut [FieldPasteOutcome], disclosures: &mut Vec<String>) {
    for outcome in fields {
        for sentence in std::mem::take(&mut outcome.disclosures) {
            if !disclosures.contains(&sentence) {
                disclosures.push(sentence);
            }
        }
    }
}

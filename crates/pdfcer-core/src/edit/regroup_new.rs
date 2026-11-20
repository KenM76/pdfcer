//! Moving a radio button into a radio group that does not exist yet: the
//! group is created, then the move is the ordinary one, folded into one
//! undo entry.

use super::regroup::WidgetRegroup;
use super::{Command, CommandKind, EditError, EditSession, ObjectWrite};
use crate::forms;
use crate::forms_author::{self, FieldPath};
use crate::object::{Dict, Name, ObjId, Object};

impl EditSession {
    /// Whether `fqn` resolves to no node at all, so a field may be made there.
    pub(super) fn names_no_field(&self, fqn: &str) -> bool {
        matches!(
            forms_author::resolve_field_path(&self.graph(), fqn),
            Ok(FieldPath::Vacant { .. })
        )
    }

    /// [`Self::move_widget_to_field`] when `target_fqn` names no field.
    pub(super) fn move_widget_to_new_field(
        &mut self,
        fqn: &str,
        index: usize,
        target_fqn: &str,
    ) -> Result<WidgetRegroup, EditError> {
        let (source, _) = self.deletion_preflight(fqn)?;
        self.refuse_unless_plain_radio(&source)?;
        let Some(member) = source.widgets.get(index) else {
            return Err(EditError::WidgetIndexOutOfRange {
                name: fqn.to_owned(),
                index,
                widgets: source.widgets.len(),
            });
        };
        let selected = match &source.value {
            forms::FieldValue::Name(v) => member.on_states.iter().find(|s| *s == v).cloned(),
            _ => None,
        };
        let before = self.checkpoint();
        self.create_radio_group(target_fqn, source.flags, selected.as_deref())?;
        match self.move_widget_to_existing_field(fqn, index, target_fqn) {
            Ok(mut out) => {
                self.coalesce_last(2, CommandKind::MoveWidgetToField);
                out.target_created = true;
                out.disclosures.push(format!(
                    "move: no field was named \"{target_fqn}\", so a new radio group was \
                     made with the button its only member."
                ));
                Ok(out)
            }
            Err(e) => {
                self.abandon(before);
                Err(e)
            }
        }
    }

    /// Commit an empty radio group at `fqn`, with `flags` as its `/Ff` and
    /// `/V` either `selected` or `Off`.
    fn create_radio_group(
        &mut self,
        fqn: &str,
        flags: forms::FieldFlags,
        selected: Option<&[u8]>,
    ) -> Result<ObjId, EditError> {
        let FieldPath::Vacant { deepest, remaining } =
            forms_author::resolve_field_path(&self.graph(), fqn)?
        else {
            return Err(forms_author::FormAuthorError::NameIsGroupingNode {
                fqn: fqn.to_owned(),
            }
            .into());
        };
        let field_id = ObjId::new(self.alloc_number()?, 0);
        let (mut objects, parent, partial) = self.place_new_field(deepest, &remaining, field_id)?;
        let mut d = Dict::new();
        d.insert(Name::from(b"FT"), Object::Name(Name::from(b"Btn")));
        d.insert(Name::from(b"Ff"), Object::Integer(i64::from(flags.0)));
        d.insert(
            Name::from(b"T"),
            Object::String(super::encode_text_string(&partial)),
        );
        d.insert(
            Name::from(b"V"),
            Object::Name(Name::from(selected.unwrap_or(b"Off"))),
        );
        d.insert(Name::from(b"Kids"), Object::Array(Vec::new()));
        if let Some(p) = parent {
            d.insert(Name::from(b"Parent"), Object::Reference(p));
        }
        objects.push(ObjectWrite {
            id: field_id,
            before: None,
            after: Some(Object::Dict(d)),
        });
        self.commit(Command {
            kind: CommandKind::MoveWidgetToField,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(field_id)
    }
}

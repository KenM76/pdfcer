//! Fixture writes for the `xcut` gate test: shape an authored object the way
//! a foreign producer would, through the command log so a save carries it.

#![cfg(test)]

use super::{Command, CommandKind, EditSession, ObjectWrite};
use crate::object::{Dict, Name, ObjId, Object, Stream};

impl EditSession {
    /// Replace object `id`'s value as one command.
    pub(crate) fn xcut_put_object(&mut self, id: ObjId, value: Object) {
        self.commit(Command {
            kind: CommandKind::PasteObjects,
            objects: vec![ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(value),
            }],
            removals: Vec::new(),
            trailer: None,
        });
    }

    /// Add a new stream object holding `data` unfiltered.
    pub(crate) fn xcut_add_stream(&mut self, mut dict: Dict, data: &[u8]) -> ObjId {
        let id = ObjId::new(self.alloc_number().unwrap_or(0), 0);
        dict.insert(
            Name::from(b"Length"),
            Object::Integer(i64::try_from(data.len()).unwrap_or(0)),
        );
        let data_span = self.stage_bytes(data);
        self.xcut_put_object(id, Object::Stream(Stream { dict, data_span }));
        id
    }
}

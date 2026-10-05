//! The edit command for a page whose fold keeps its pdfcer OCR layers as their
//! own `/Contents` streams; see [`crate::ocr::refold`].

use super::{Command, CommandKind, DecoupledContent, EditSession, ObjectWrite};
use crate::object::ObjId;
use crate::ocr::refold::{Refold, restate};
use crate::text_edit::edit::make_raw_stream;

impl EditSession {
    /// One command writing `r` across the page: `/Contents[0]` gets the edited
    /// content outside the layers, each changed stream its new payload.
    pub(super) fn refolded_command(
        &mut self,
        kind: CommandKind,
        content_id: ObjId,
        r: Refold,
        mut prior: Vec<ObjectWrite>,
        disclosures: &mut Vec<String>,
    ) -> (Command, Option<DecoupledContent>) {
        restate(disclosures, &r);
        let mut objects = Vec::with_capacity(r.others.len() + 1 + prior.len());
        for (id, payload) in std::iter::once((content_id, r.first)).chain(r.others) {
            let span = self.stage_bytes(&payload);
            objects.push(ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(make_raw_stream(span, payload.len())),
            });
        }
        objects.append(&mut prior);
        let command = Command {
            kind,
            objects,
            removals: Vec::new(),
            trailer: None,
        };
        let report = DecoupledContent {
            content_object: content_id.num,
            emptied: r.emptied,
        };
        (command, Some(report))
    }
}

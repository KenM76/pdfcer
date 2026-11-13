//! [`EditSession::clear_page_content`]: empty every page of a printed copy
//! so only chosen annotations, and decoration added afterwards, print.

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite};
use crate::graph::ObjectGraph;
use crate::object::{Name, Object};

impl EditSession {
    /// **Clear page content**: remove every page's `/Contents` and every
    /// annotation whose `/Subtype` name `keep_annotation` rejects. Returns
    /// how many annotations were removed.
    ///
    /// A page with no `/Contents` is empty (ISO 32000-1 §7.7.3.3 Table 30),
    /// so a shell can print "form fields only" with print-time decoration:
    /// clear with a widgets-only predicate, then
    /// [`Self::decorate_for_print`], then render with page content on. The
    /// decoration is then the only page content and the kept annotations
    /// the only annotations. `/Resources` and the page boxes are kept, and
    /// a removed annotation is only unlinked from `/Annots`.
    ///
    /// Meant for a session that is printed and then dropped, like
    /// [`Self::fork_for_print`]'s. One [`CommandKind::ClearPageContent`]
    /// undo entry; nothing is recorded when no page changes.
    ///
    /// # Errors
    ///
    /// The [`Self::stamp_running_text`] session refusals (encrypted against
    /// content changes, certified, hidden objects), checked before any
    /// write, and a page-tree read failure.
    pub fn clear_page_content(
        &mut self,
        keep_annotation: impl Fn(&[u8]) -> bool,
    ) -> Result<usize, EditError> {
        self.check_page_stamp()?;
        let pages = self.pages()?;
        let mut objects = Vec::new();
        let mut removed = 0;
        for page in &pages {
            let Some(mut dict) = self.graph().resolved(page.id).as_dict().cloned() else {
                continue;
            };
            let had_content = dict.remove(b"Contents").is_some();
            let mut dropped = 0;
            if let Some(annots) = dict.get(b"Annots") {
                let items = match self.graph().resolve(annots) {
                    Object::Array(items) => items.clone(),
                    _ => Vec::new(),
                };
                let kept: Vec<Object> = items
                    .iter()
                    .filter(|a| keep_annotation(&self.subtype_of(a)))
                    .cloned()
                    .collect();
                dropped = items.len() - kept.len();
                if dropped > 0 {
                    dict.insert(Name::from(b"Annots"), Object::Array(kept));
                }
            }
            if !had_content && dropped == 0 {
                continue;
            }
            removed += dropped;
            objects.push(ObjectWrite {
                id: page.id,
                before: self.value(page.id).cloned(),
                after: Some(Object::Dict(dict)),
            });
        }
        if !objects.is_empty() {
            self.commit(Command {
                kind: CommandKind::ClearPageContent,
                objects,
                removals: Vec::new(),
                trailer: None,
            });
        }
        Ok(removed)
    }

    /// The `/Subtype` name bytes of annotation `annot`; empty when absent.
    fn subtype_of(&self, annot: &Object) -> Vec<u8> {
        let graph = self.graph();
        graph
            .resolve(annot)
            .as_dict()
            .and_then(|d| d.get(b"Subtype"))
            .and_then(|s| graph.resolve(s).as_name())
            .map(|n| n.as_bytes().to_vec())
            .unwrap_or_default()
    }
}

//! The smooth-lines step of an optimize run (Pass 616.3): one page's
//! content rewritten with its line runs fitted as curves, redone against
//! the content as the run now holds it.

use super::{CommandKind, EditError, EditSession};
use crate::object::ObjId;
use crate::optimize::smooth;

impl EditSession {
    /// Smooth `page`'s line runs at `tol` points as one command, or do
    /// nothing when none is worth rewriting now.
    pub(super) fn smooth_page(&mut self, page: ObjId, tol: f64) -> Result<(), EditError> {
        let pages = self.pages()?;
        let Some(page) = pages.iter().find(|p| p.id == page) else {
            return Ok(());
        };
        let Some(&content_id) = page.contents.first() else {
            return Ok(());
        };
        let (stream, _) = self.page_content_and_objects(page)?;
        let mut s = smooth::smooth_content(&self.view(), &stream, &page.resources, tol);
        if s.edits.is_empty() {
            return Ok(());
        }
        let content = crate::text_edit::edit::splice(&stream.buf, &mut s.edits);
        let mut disclosures = Vec::new();
        let (command, _) = self.text_edit_command(
            CommandKind::Optimize { count: 0 },
            content_id,
            page,
            content,
            Vec::new(),
            &mut disclosures,
        )?;
        self.commit(command);
        Ok(())
    }
}

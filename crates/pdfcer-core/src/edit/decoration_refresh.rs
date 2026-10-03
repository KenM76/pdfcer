//! Keeps underline/strikethrough rules on the glyphs they decorate: after
//! any command rewrites a page's content, the rules are recomputed from the
//! markers (`crate::text_edit::decoration`) and the result folded into the
//! same undo entry.

use super::{Command, EditSession};
use crate::object::Object;
use crate::text_edit::decoration;
use crate::text_edit::edit::make_raw_stream;
use crate::text_extract::{ExtractOptions, extract_page_view};

impl EditSession {
    /// Recompute the decoration rules of every page whose first content
    /// stream `command` rewrote. A page is refreshed only when its other
    /// content streams are empty, so the decoded page buffer is exactly the
    /// rewritten stream's bytes.
    pub(super) fn refresh_decorations(&mut self, command: &mut Command) {
        let Ok(pages) = self.pages() else { return };
        for i in 0..command.objects.len() {
            let Some(write) = command.objects.get(i) else {
                continue;
            };
            let Some(Object::Stream(s)) = &write.after else {
                continue;
            };
            let id = write.id;
            let Some(raw) = self.view().slice(s.data_span).map(<[u8]>::to_vec) else {
                continue;
            };
            if !raw
                .windows(decoration::DECORATION_TAG.len())
                .any(|w| w == decoration::DECORATION_TAG)
            {
                continue;
            }
            let Some((index, page)) = pages
                .iter()
                .enumerate()
                .find(|(_, p)| p.contents.first() == Some(&id))
            else {
                continue;
            };
            let Some(new) = self.recompute(page, index, &raw) else {
                continue;
            };
            let len = new.len();
            let span = self.stage_bytes(&new);
            let after = Some(make_raw_stream(span, len));
            Self::write_state(&mut self.state, id, after.clone());
            if let Some(w) = command.objects.get_mut(i) {
                w.after = after;
            }
        }
    }

    fn recompute(
        &self,
        page: &crate::page_tree::Page,
        index: usize,
        raw: &[u8],
    ) -> Option<Vec<u8>> {
        let view = self.view();
        let cs = crate::content::ContentStream::from_page(&view, page).ok()?;
        if cs.buf != raw {
            return None;
        }
        let options = ExtractOptions::default().with_provenance(true);
        let text = extract_page_view(&view, page, index, &options).ok()?;
        let glyphs: Vec<_> = text.runs.iter().flat_map(|r| r.glyphs.iter()).collect();
        decoration::refresh(&view, page, &cs, &glyphs).map(|r| r.content)
    }
}

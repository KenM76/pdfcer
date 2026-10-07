//! Keeps underline/strikethrough rules on the glyphs they decorate: after
//! any command rewrites a page's content, the rules are recomputed from the
//! markers (`crate::text_edit::decoration`) and the result folded into the
//! same undo entry.

use super::{Command, EditSession};
use crate::graph::ObjectGraph;
use crate::object::{Name, Object, Stream};
use crate::text_edit::decoration;
use crate::text_extract::{ContentStreamRef, ExtractOptions, extract_page_view};

impl EditSession {
    /// Recompute the decoration rules in every content stream `command`
    /// rewrote: a page's first stream, refreshed only when its other streams
    /// are empty (so the decoded page buffer is exactly the rewritten
    /// bytes), or a form XObject painted by some page.
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
            let mut dict = s.dict.clone();
            let Some(raw) = self.view().slice(s.data_span).map(<[u8]>::to_vec) else {
                continue;
            };
            if !raw
                .windows(decoration::DECORATION_TAG.len())
                .any(|w| w == decoration::DECORATION_TAG)
            {
                continue;
            }
            let Some(new) = self.recompute(&pages, id, &raw) else {
                continue;
            };
            dict.insert(
                Name::from(b"Length"),
                Object::Integer(i64::try_from(new.len()).unwrap_or(i64::MAX)),
            );
            let data_span = self.stage_bytes(&new);
            // The written dictionary is kept: a form's `/Subtype`, `/BBox`
            // and `/Resources` live there.
            let after = Some(Object::Stream(Stream { dict, data_span }));
            self.put_state(id, after.clone());
            if let Some(w) = command.objects.get_mut(i) {
                w.after = after;
            }
        }
        self.sync_structure(command, &pages);
    }

    /// Record each page's decorations in the structure tree
    /// (`decoration::tagged`) for every page whose content `command`
    /// rewrote, folding the element rewrites into the same undo entry and
    /// leaving the notes in `structure_notes`.
    fn sync_structure(&mut self, command: &mut Command, pages: &[crate::page_tree::Page]) {
        self.structure_notes.clear();
        let written: Vec<_> = command.objects.iter().map(|w| w.id).collect();
        let mut writes = Vec::new();
        let mut notes = Vec::new();
        {
            let view = self.view();
            if view
                .catalog_dict()
                .and_then(|c| c.get(b"StructTreeRoot"))
                .is_none()
            {
                return;
            }
            let base = |id| self.base.get(id).map(|io| io.value.clone());
            for (index, page) in pages.iter().enumerate() {
                if !page.contents.iter().any(|c| written.contains(c)) {
                    continue;
                }
                let sync = decoration::tagged::sync_page(&view, &base, pages, index);
                writes.extend(sync.writes);
                notes.extend(sync.notes);
            }
            if written
                .iter()
                .any(|id| self.is_decorated_form(&view, pages, *id))
            {
                notes.push(
                    "decoration inside a form XObject is not recorded in the structure tree"
                        .to_owned(),
                );
            }
        }
        self.structure_notes = notes;
        for (id, after) in writes {
            if self.view().value(id) == Some(&after) {
                continue;
            }
            let before = self.state.get(&id).cloned();
            self.put_state(id, Some(after.clone()));
            command.objects.push(super::ObjectWrite {
                id,
                before,
                after: Some(after),
            });
        }
    }

    /// Whether `id` is a form XObject (no page's content stream) whose
    /// current bytes carry a decoration marker.
    fn is_decorated_form(
        &self,
        view: &crate::view::DocumentView<'_>,
        pages: &[crate::page_tree::Page],
        id: crate::object::ObjId,
    ) -> bool {
        if pages.iter().any(|p| p.contents.contains(&id)) {
            return false;
        }
        let Some(Object::Stream(s)) = view.value(id) else {
            return false;
        };
        let is_form = s
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_some_and(|n| n.as_bytes() == b"Form");
        is_form
            && crate::content::ContentStream::from_form(view, id)
                .is_ok_and(|cs| !decoration::Scan::of(&cs).markers.is_empty())
    }

    /// The refreshed bytes of stream `id` with decoded content `raw`, found
    /// as a page's first stream or else as a form some page paints.
    fn recompute(
        &self,
        pages: &[crate::page_tree::Page],
        id: crate::object::ObjId,
        raw: &[u8],
    ) -> Option<Vec<u8>> {
        let view = self.view();
        let (index, page, source, resources, cs) = pages
            .iter()
            .enumerate()
            .find(|(_, p)| p.contents.first() == Some(&id))
            .and_then(|(index, page)| {
                let cs = crate::content::ContentStream::from_page(&view, page).ok()?;
                Some((
                    index,
                    page,
                    ContentStreamRef::Page,
                    page.resources.clone(),
                    cs,
                ))
            })
            .or_else(|| {
                pages.iter().enumerate().find_map(|(index, page)| {
                    let scan = crate::text_edit::forms::scan_page_forms(&view, page);
                    let form = scan.find(id.num)?;
                    let cs = crate::content::ContentStream::from_form(&view, form.id).ok()?;
                    let source = ContentStreamRef::Form { object: id.num };
                    Some((index, page, source, form.resources.clone(), cs))
                })
            })?;
        if cs.buf != raw {
            return None;
        }
        let options = ExtractOptions::default().with_provenance(true);
        let text = extract_page_view(&view, page, index, &options).ok()?;
        let glyphs: Vec<_> = text.runs.iter().flat_map(|r| r.glyphs.iter()).collect();
        decoration::refresh(&view, &resources, source, &cs, &glyphs).map(|r| r.content)
    }
}

//! The image-fragment merge of an optimize run (Pass 616.3), applied one
//! run at a time through [`EditSession::repoint_image_placement`], then the
//! strips' names unbound so nothing keeps them in the file.

use std::collections::{BTreeSet, HashSet};

use super::optimize_cleanup::Fold;
use super::{Command, CommandKind, EditError, EditSession, ObjectWrite, Removal};
use crate::object::{ObjId, Object, Stream};
use crate::optimize::fragments::{self, Referrers};

/// A ceiling on merges per page; each merge removes at least one placement,
/// so a page cannot need more than it has `Do` operators.
const MAX_MERGES: usize = 100_000;

impl EditSession {
    /// Merge every run of image strips on `page` as the run now holds it,
    /// each into one new image, folding each merge into `fold`. A run that
    /// no longer merges (not smaller, or undecodable) is left as it is, and
    /// so is a page whose `/XObject` dictionary something other than a page
    /// draws from (`refs` says who holds what).
    pub(super) fn merge_page_fragments(
        &mut self,
        page: ObjId,
        refs: &Referrers,
        fold: &mut Fold,
    ) -> Result<(), EditError> {
        let Some(sharers) = fragments::sharers(&self.view(), refs, page) else {
            return Ok(());
        };
        let mut merged = BTreeSet::new();
        let mut refused: BTreeSet<Vec<ObjId>> = BTreeSet::new();
        for _ in 0..MAX_MERGES {
            let pages = self.pages()?;
            let Some((index, page)) = pages.iter().enumerate().find(|(_, p)| p.id == page) else {
                break;
            };
            let (content, _) = self.page_content_and_objects(page)?;
            let view = self.view();
            let mut next = None;
            for run in fragments::runs(&view, &content, &page.resources) {
                if refused.contains(&run.ids) {
                    continue;
                }
                match fragments::merge(&view, &run, &page.resources) {
                    Some(m) => {
                        next = Some((run, m));
                        break;
                    }
                    None => {
                        refused.insert(run.ids);
                    }
                }
            }
            let Some((run, image)) = next else {
                break;
            };
            merged.extend(run.ids.iter().copied());
            let id = ObjId::new(self.alloc_number()?, 0);
            let data_span = self.stage_bytes(&image.data);
            let prior = vec![ObjectWrite {
                id,
                before: None,
                after: Some(Object::Stream(Stream {
                    dict: image.dict,
                    data_span,
                })),
            }];
            self.repoint_image_placement(
                CommandKind::ReplaceImage,
                index,
                run.span,
                id,
                prior,
                |name| fragments::invocation(run.matrix, name),
            )?;
            fold.absorb(self);
        }
        self.unbind_strips(page, &merged, &sharers)?;
        fold.absorb(self);
        self.delete_unreachable(&merged);
        fold.absorb(self);
        Ok(())
    }

    /// Delete `strips`, and what only they reached, that nothing live still
    /// reaches: a full save writes every object the session holds.
    fn delete_unreachable(&mut self, strips: &BTreeSet<ObjId>) {
        let seeds: Vec<ObjId> = strips.iter().copied().collect();
        let candidates = super::reachable(&self.graph(), &seeds, &HashSet::new());
        let live = self.live_objects();
        let removals: Vec<Removal> = candidates
            .into_iter()
            .filter(|id| {
                !live.contains(id) && !self.deleted.contains(id) && self.value(*id).is_some()
            })
            .map(|id| Removal {
                id,
                was_deleted: false,
                is_deleted: true,
            })
            .collect();
        if removals.is_empty() {
            return;
        }
        self.commit(Command {
            kind: CommandKind::ReplaceImage,
            objects: Vec::new(),
            removals,
            trailer: None,
        });
    }

    /// Remove from `page`'s `/XObject` dictionary every name bound to one of
    /// `strips` that no page of `sharers` still draws.
    fn unbind_strips(
        &mut self,
        page: ObjId,
        strips: &BTreeSet<ObjId>,
        sharers: &BTreeSet<ObjId>,
    ) -> Result<(), EditError> {
        if strips.is_empty() {
            return Ok(());
        }
        let pages = self.pages()?;
        let Some(page) = pages.iter().find(|p| p.id == page) else {
            return Ok(());
        };
        let view = self.view();
        let Some(names) = fragments::unused_names(&view, page, strips, sharers) else {
            return Ok(());
        };
        if names.is_empty() {
            return Ok(());
        }
        let (writes, _) = crate::text_edit::addtext::patch_resource(
            &self.graph(),
            page.id,
            true,
            b"XObject",
            |entries| {
                for n in &names {
                    entries.remove(n);
                }
            },
        );
        let objects = writes
            .into_iter()
            .map(|(id, value)| ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(value),
            })
            .collect();
        self.commit(Command {
            kind: CommandKind::ReplaceImage,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(())
    }
}

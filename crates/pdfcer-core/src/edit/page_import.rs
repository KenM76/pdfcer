//! The rules [`EditSession`]'s cross-document page import applies while a
//! page insert or merge is in flight, mirroring `pageops::assemble`'s copier.
//!
//! - **Page barrier.** A reference to a source page that is not being copied
//!   (a link to page 50 on an inserted page 1) is refused rather than
//!   followed: an array holding one is refused whole, a dictionary drops the
//!   entry. Followed, it copied the page — content, resources and all — as an
//!   unreachable phantom, and the link pointed at it.
//! - **No structure tree.** The source's tags do not travel (there is no
//!   structure-tree merge), so a structure destination (ISO 32000-2
//!   §12.3.2.3) is re-pointed at its element's page, or the source's first
//!   page when the element marks none; a GoTo `/SD` (§12.6.4.2) becomes the
//!   `/D` when there is none and is dropped otherwise; and `/StructParents` /
//!   `/StructParent` (§14.7.5.4) are dropped, because they index the SOURCE
//!   parent tree and in a tagged target would name an unrelated element.

use std::collections::{BTreeMap, BTreeSet};

use super::EditSession;
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::PageSlot;
use crate::view::DocumentView;

/// State of one page import, live from the page copy to the outcome.
#[derive(Debug, Default)]
pub(super) struct PageImportGuard {
    /// Source pages NOT being copied.
    barrier: BTreeSet<ObjId>,
    /// The source's first page, the fallback target of a structure link.
    first_page: Option<ObjId>,
    /// Element → its page, memoised.
    element_pages: BTreeMap<ObjId, Option<ObjId>>,
    pub(super) report: PageImportReport,
}

/// What the guard did, for the outcome structs.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct PageImportReport {
    /// References refused because they named a page left behind.
    pub(super) links_to_pages_left_behind: usize,
    /// Structure destinations re-pointed at a page.
    pub(super) structure_links_to_pages: usize,
    /// Of those, the ones whose element marked no page.
    pub(super) structure_links_to_first_page: usize,
}

impl EditSession {
    /// Start the page import of `selected` (indices into `slots`, the
    /// source's pages in order); the public verb ends it.
    pub(super) fn begin_page_import(&mut self, slots: &[PageSlot], selected: &[usize]) {
        let chosen: BTreeSet<ObjId> = selected
            .iter()
            .filter_map(|i| slots.get(*i).map(|s| s.id))
            .collect();
        self.page_import = Some(PageImportGuard {
            barrier: slots
                .iter()
                .map(|s| s.id)
                .filter(|p| !chosen.contains(p))
                .collect(),
            first_page: slots.first().map(|s| s.id),
            ..PageImportGuard::default()
        });
    }

    /// Whether a page import is in flight and `id` is a page it refuses;
    /// counts the refusal.
    pub(super) fn import_refuses(&mut self, id: ObjId) -> bool {
        let Some(guard) = self.page_import.as_mut() else {
            return false;
        };
        let refused = guard.barrier.contains(&id);
        guard.report.links_to_pages_left_behind += usize::from(refused);
        refused
    }

    /// `items` re-pointed at its element's page when it is a structure
    /// destination and a page import is in flight; `None` otherwise.
    pub(super) fn import_struct_dest(
        &mut self,
        source: &DocumentView<'_>,
        items: &[Object],
    ) -> Option<Vec<Object>> {
        self.page_import.as_ref()?;
        let elem = crate::struct_page::struct_target(source.graph(), items)?;
        let guard = self.page_import.as_mut()?;
        let found = *guard
            .element_pages
            .entry(elem)
            .or_insert_with(|| crate::struct_page::element_page(source.graph(), elem));
        guard.report.structure_links_to_pages += 1;
        let page = found.or_else(|| {
            guard.report.structure_links_to_first_page += 1;
            guard.first_page
        });
        let mut out = vec![page.map_or(Object::Null, Object::Reference)];
        out.extend(items.iter().skip(1).cloned());
        Some(out)
    }

    /// The key `key` of `dict` takes in the copy, or `None` to drop it.
    pub(super) fn import_key(&self, dict: &Dict, key: &Name) -> Option<Name> {
        if self.page_import.is_none() {
            return Some(key.clone());
        }
        match key.as_bytes() {
            b"StructParents" | b"StructParent" => None,
            b"SD" if crate::struct_page::goto_like(dict) => {
                dict.get(b"D").is_none().then(|| Name::from(b"D"))
            }
            _ => Some(key.clone()),
        }
    }
}

impl EditSession {
    /// The live import's report, and whether `source` was tagged.
    pub(super) fn page_import_report(&self, source: &DocumentView<'_>) -> (PageImportReport, bool) {
        let report = self
            .page_import
            .as_ref()
            .map(|g| g.report)
            .unwrap_or_default();
        let tagged = source
            .graph()
            .catalog_dict()
            .is_some_and(|c| c.contains_key(b"StructTreeRoot"));
        (report, tagged)
    }
}

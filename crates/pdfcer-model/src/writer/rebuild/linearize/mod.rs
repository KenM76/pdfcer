//! Linearized output ("Fast Web View", ISO 32000-2 Annex F): the rebuilt
//! objects reordered and renumbered so a reader can show page 1 before the
//! rest of the file arrives, with the hint stream that says where each page
//! is.
//!
//! The layout is F.3.1's parts 1-9 and 11, with no overflow hint stream
//! (part 10) and no object or cross-reference streams. Classification
//! follows the reference implementation qpdf (`--check-linearization` is
//! the test oracle), which differs from a literal reading of F.3 in two
//! places, both recorded in the spec RAG's linearization writer reference:
//! objects reached from the catalog's document-level keys go to part 4
//! even when the first page uses them, and every hint-table column is
//! byte-aligned (`LIN-A1`).
//!
//! Inheritable page attributes (§7.7.3.4) are pushed down to every page and
//! removed from the page-tree nodes, as F.3.7 requires the first page's to
//! be explicit and qpdf requires of every page.

mod classify;
mod hint;
mod layout;
mod pages;

use std::collections::{BTreeMap, BTreeSet};

use super::RebuildReport;
use crate::PdfVersion;
use crate::document::Document;
use crate::object::{Dict, ObjId, Object};
use crate::writer::WriteError;

const PDF_1_2: PdfVersion = PdfVersion { major: 1, minor: 2 };

/// The rebuilt objects as one graph: merged objects resolve to their kept
/// twin, and page dictionaries carry their pushed-down attributes.
pub(super) struct Graph<'a> {
    live: &'a BTreeMap<ObjId, &'a Object>,
    canon: &'a BTreeMap<ObjId, ObjId>,
    kept: &'a BTreeSet<ObjId>,
    overrides: BTreeMap<ObjId, Object>,
}

impl<'a> Graph<'a> {
    /// The kept object a reference to `r` names, if the output defines one.
    fn target(&self, r: ObjId) -> Option<ObjId> {
        let id = self.canon.get(&r).copied().unwrap_or(r);
        self.kept.contains(&id).then_some(id)
    }

    fn value(&self, id: ObjId) -> Option<&Object> {
        self.overrides
            .get(&id)
            .or_else(|| self.live.get(&id).copied())
    }

    fn dict(&self, id: ObjId) -> Option<&Dict> {
        match self.value(id)? {
            Object::Dict(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }

    /// The kept object `value` refers to, when it is a reference.
    fn referent(&self, value: Option<&Object>) -> Option<ObjId> {
        match value? {
            Object::Reference(r) => self.target(*r),
            _ => None,
        }
    }
}

/// Write `live` (less the objects `canon` merges away) as a linearized
/// file. Fills `report.objects_out` and `report.version_raised`.
///
/// # Errors
///
/// [`WriteError::NothingToLinearize`] when the trailer's `/Root` is not a
/// kept catalog or the page tree has no page.
pub(super) fn write(
    doc: &Document,
    live: &BTreeMap<ObjId, &Object>,
    canon: &BTreeMap<ObjId, ObjId>,
    kept: &BTreeSet<ObjId>,
    report: &mut RebuildReport,
) -> Result<Vec<u8>, WriteError> {
    let mut graph = Graph {
        live,
        canon,
        kept,
        overrides: BTreeMap::new(),
    };
    let root = graph
        .referent(doc.trailer().get(b"Root"))
        .filter(|&id| graph.dict(id).is_some())
        .ok_or(WriteError::NothingToLinearize)?;
    let pages = pages::push_down(&mut graph, root);
    if pages.is_empty() {
        return Err(WriteError::NothingToLinearize);
    }
    let info = graph.referent(doc.trailer().get(b"Info"));
    let parts = classify::parts(&graph, root, info, &pages);
    let mut version = doc.version();
    if version < PDF_1_2 {
        version = PDF_1_2;
        report.version_raised = Some(version);
    }
    let trailer = layout::TrailerKeys {
        root,
        info,
        id: doc.trailer().get(b"ID"),
    };
    let (bytes, objects) = layout::write(&graph, &parts, &trailer, version, doc.bytes(), report);
    report.objects_out = objects;
    Ok(bytes)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code: a failed lookup should fail the test loudly
mod tests;

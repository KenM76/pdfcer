//! Markup snap targets for a page of the session (pdfcer-gui request G281).

use std::collections::BTreeSet;

use super::{EditError, EditSession};
use crate::object::ObjId;
use crate::vector::MarkupSnapGeometry;

impl EditSession {
    /// The snap targets of the markups on `page_index`, as the session
    /// currently holds them, for
    /// [`snap_candidates_with_markups`](crate::vector::snap_candidates_with_markups).
    ///
    /// [`markup_snap_geometry`](crate::vector::markup_snap_geometry) with the
    /// session's ce dimensions excluded: a ce dimension is placed by its own
    /// points, not snapped to as a markup. `hidden_layers` is the set of
    /// layers switched off, as [`crate::annot::oc_is_hidden`] takes it.
    ///
    /// Reads only. Build once per page and again after any edit to the page's
    /// annotations.
    ///
    /// # Errors
    ///
    /// [`EditError::PageOutOfRange`]; a page tree that cannot be read.
    pub fn markup_snap_geometry(
        &self,
        page_index: usize,
        hidden_layers: &BTreeSet<ObjId>,
    ) -> Result<Vec<MarkupSnapGeometry>, EditError> {
        let pages = self.pages()?;
        let count = pages.len();
        let page = pages.get(page_index).ok_or(EditError::PageOutOfRange {
            index: page_index,
            count,
        })?;
        let ce_dimensions: BTreeSet<ObjId> = self
            .dimension_model()
            .dimensions()
            .iter()
            .filter_map(|d| d.annot)
            .collect();
        Ok(crate::vector::markup_snap_geometry(
            &self.graph(),
            page.id,
            hidden_layers,
            &ce_dimensions,
        ))
    }
}

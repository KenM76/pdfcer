//! A ce dimension measures physical length: the `/UserUnit` (ISO 32000-1
//! §14.11.2, Table 30) of its page enters every value the session bakes or
//! reports (pdfcer-gui request G277).

use super::{EditError, EditSession};
use crate::dimension::{
    DimensionId, DimensionKind, DimensionPreview, DimensionRecord, DimensionStyle, GroupId,
    MeasurementDisplay, author_dimension, resolve_style,
};
use crate::graph::ObjectGraph as _;
use crate::object::{ObjId, Object};

impl EditSession {
    /// Table 30 `/UserUnit` of the page object `page`; `1.0` when absent or
    /// not a finite positive number. The entry is not inheritable.
    pub(crate) fn page_user_unit(&self, page: ObjId) -> f64 {
        self.resolved(page)
            .as_dict()
            .and_then(|d| d.get(b"UserUnit"))
            .and_then(|o| self.resolve(o).as_number())
            .filter(|u| u.is_finite() && *u > 0.0)
            .unwrap_or(1.0)
    }

    /// The `/UserUnit` of the page `record`'s annotation names in `/P`
    /// (§12.5.2 Table 164); `1.0` for an unwired record or one with no `/P`.
    pub(crate) fn dimension_user_unit(&self, record: &DimensionRecord) -> f64 {
        let page = record
            .annot
            .and_then(|annot| self.resolved(annot).as_dict())
            .and_then(|d| d.get(b"P"))
            .and_then(Object::as_reference);
        page.map_or(1.0, |id| self.page_user_unit(id))
    }

    /// The value ce dimension `id` shows: its group's scale and format (with
    /// the ce dimension's own overrides) applied to its physical length, so
    /// it agrees with the label baked into the page on a `/UserUnit` page,
    /// where [`crate::dimension::DimensionModel::display`] reads box units.
    /// `None` for an unknown ce dimension or one whose group is missing.
    #[must_use]
    pub fn dimension_display(&self, id: DimensionId) -> Option<MeasurementDisplay> {
        let model = self.read_dimension_model();
        let record = model.dimension(id)?;
        let style = resolve_style(model.group(record.group)?, &record.style);
        Some(
            record
                .kind
                .display_on(self.dimension_user_unit(record), style.scale, style.format),
        )
    }

    /// [`Self::new_dimension_preview`] for a ce dimension being placed on
    /// page `page_index`: the label and `/Measure` carry that page's
    /// `/UserUnit`, byte-identical to what `add_dimension(page_index, group,
    /// kind)` writes.
    ///
    /// # Errors
    ///
    /// [`EditError::PageOutOfRange`] for an unknown page;
    /// [`EditError::DimensionGroupNotFound`] for an unknown `group`.
    pub fn new_dimension_preview_on(
        &self,
        page_index: usize,
        group: GroupId,
        kind: &DimensionKind,
    ) -> Result<DimensionPreview, EditError> {
        let slots = self.page_slots()?;
        let page = slots
            .get(page_index)
            .ok_or(EditError::PageOutOfRange {
                index: page_index,
                count: slots.len(),
            })?
            .id;
        let model = self.read_dimension_model();
        let group = model
            .group(group)
            .ok_or(EditError::DimensionGroupNotFound { id: group.0 })?;
        let style = DimensionStyle::from(group).on_user_unit(self.page_user_unit(page));
        Ok(DimensionPreview {
            appearance: author_dimension(kind, style),
        })
    }
}

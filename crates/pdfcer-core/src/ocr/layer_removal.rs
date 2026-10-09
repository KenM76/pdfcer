//! What removing one OCR layer left behind ([`super::layer`]).

use crate::object::ObjId;

/// What removing one OCR layer left behind.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct OcrLayerRemoval {
    /// The optional-content group the removed text was on
    /// ([`OcrLayerOptions::on_layer`](super::layer::OcrLayerOptions::on_layer)), if any.
    pub optional_content: Option<ObjId>,
    /// Whether that group now has no content anywhere in the document, so a
    /// shell can offer to delete it. Content pdfcer cannot decode counts as
    /// using the group.
    pub group_emptied: bool,
    /// Each region group's optional-content group the removed text used
    /// ([`OcrLayerOptions::on_region_layer`](super::layer::OcrLayerOptions::on_region_layer)), in stream order, with the same
    /// "now empty" test as [`Self::group_emptied`]. Excludes
    /// [`Self::optional_content`].
    pub region_groups: Vec<RegionLayerRemoval>,
}

/// One region group's optional-content group, after an OCR layer that used
/// it was removed ([`OcrLayerRemoval::region_groups`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RegionLayerRemoval {
    /// The optional-content group.
    pub group: ObjId,
    /// Whether it now has no content anywhere in the document.
    pub emptied: bool,
}

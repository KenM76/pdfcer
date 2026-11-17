//! What a merge or insert did with the ce dimensions it copied, as report
//! tokens and as stderr disclosure.

use std::path::Path;

use pdfcer_core::edit::{InsertOutcome, MergeOutcome};
use pdfcer_core::pageops::AssembleReport;

/// The ce-dimension counts of one merge, insert or assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CeCarry {
    carried: usize,
    as_lines: usize,
    groups_carried: usize,
    groups_matched: usize,
    groups_renamed: usize,
    newer: bool,
}

impl From<&MergeOutcome> for CeCarry {
    fn from(o: &MergeOutcome) -> Self {
        Self {
            carried: o.ce_dimensions_carried,
            as_lines: o.ce_dimensions_as_lines,
            groups_carried: o.ce_dimension_groups_carried,
            groups_matched: o.ce_dimension_groups_matched,
            groups_renamed: o.ce_dimension_groups_renamed,
            newer: o.ce_dimension_sidecar_newer,
        }
    }
}

impl From<&InsertOutcome> for CeCarry {
    fn from(o: &InsertOutcome) -> Self {
        Self {
            carried: o.ce_dimensions_carried,
            as_lines: o.ce_dimensions_as_lines,
            groups_carried: o.ce_dimension_groups_carried,
            groups_matched: o.ce_dimension_groups_matched,
            groups_renamed: o.ce_dimension_groups_renamed,
            newer: o.ce_dimension_sidecar_newer,
        }
    }
}

impl From<&AssembleReport> for CeCarry {
    fn from(r: &AssembleReport) -> Self {
        Self {
            carried: r.ce_dimensions_carried,
            as_lines: r.ce_dimensions_as_lines.iter().sum(),
            groups_carried: r.ce_dimension_groups_carried,
            groups_matched: r.ce_dimension_groups_matched,
            groups_renamed: r.ce_dimension_groups_renamed,
            newer: r.ce_dimension_sidecar_newer,
        }
    }
}

impl CeCarry {
    /// `ce_dim_groups_carried= ce_dim_groups_matched= ce_dim_groups_renamed=`.
    pub(crate) fn group_tokens(&self) -> String {
        format!(
            "ce_dim_groups_carried={} ce_dim_groups_matched={} ce_dim_groups_renamed={}",
            self.groups_carried, self.groups_matched, self.groups_renamed
        )
    }

    /// `ce_dims_carried= ce_dims_as_lines=` followed by [`Self::group_tokens`].
    pub(crate) fn tokens(&self) -> String {
        format!(
            "ce_dims_carried={} ce_dims_as_lines={} {}",
            self.carried,
            self.as_lines,
            self.group_tokens()
        )
    }

    /// Say on stderr which dimension groups were joined, brought or renamed,
    /// and how many ce dimensions arrived as plain line annotations. Silent
    /// when there is nothing an operator could not see in the file.
    pub(crate) fn report(&self, path: &Path) {
        let path = path.display();
        if self.groups_carried + self.groups_matched > 0 {
            eprintln!(
                "pdfcer: {path}: {} ce dimension(s) were brought across as ce dimensions: {} dimension group(s) came with them, and {} joined a group of the same name and settings.",
                self.carried, self.groups_carried, self.groups_matched
            );
        }
        if self.groups_renamed > 0 {
            eprintln!(
                "pdfcer: {path}: {} dimension group(s) were renamed with a numbered suffix such as \"Arch (2)\": a group of that name with different settings was already there.",
                self.groups_renamed
            );
        }
        if self.as_lines == 0 {
            return;
        }
        if self.newer {
            eprintln!(
                "pdfcer: {path}: {} ce dimension(s) arrived as plain line annotations: dimension data on one side was written by a newer pdfcer than this one reads, so they keep their look and are not editable as ce dimensions in this file.",
                self.as_lines
            );
        } else {
            eprintln!(
                "pdfcer: {path}: {} ce dimension(s) arrived as plain line annotations: their dimension records were not brought across, so they keep their look and no longer follow a group or re-measure.",
                self.as_lines
            );
        }
    }
}

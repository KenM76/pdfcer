//! `page-paste`'s stderr disclosures (rule 4): what the paste did that the
//! result does not show.

use std::path::Path;

use pdfcer_core::edit::InsertOutcome;

use crate::sign::report_separations;

/// Print every non-empty disclosure of `pasted` against `input`.
pub(crate) fn report_page_paste(input: &Path, pasted: &InsertOutcome) {
    // Off-canvas, on stderr. The widget counters are the ones that are
    // invisible in the result: boxes that draw like form fields and that
    // nothing can fill.
    if pasted.orphaned_widgets > 0 {
        eprintln!(
            "pdfcer: {}: {} form-field box(es) arrived WITHOUT the field that owns them ({} field(s) had boxes on pages that were not pasted, so were not copied; {} box(es) cannot be adopted into a field afterwards, because the widget carries no name or type of its own). They draw like fields and nothing can fill them.",
            input.display(),
            pasted.orphaned_widgets,
            pasted.fields_split,
            pasted.orphaned_widgets_unrecoverable,
        );
    }
    if pasted.fields_renamed > 0 {
        eprintln!(
            "pdfcer: {}: {} pasted form field(s) took a name already used in this document and were renamed with a numeric suffix.",
            input.display(),
            pasted.fields_renamed,
        );
    }
    if pasted.source_outline_dropped {
        eprintln!(
            "pdfcer: {}: the copied pages' document had an outline (bookmarks); it did not travel. A bookmark tree describes a DOCUMENT, and half of one grafted into another is a claim nobody made.",
            input.display()
        );
    }
    if pasted.source_page_labels_dropped || pasted.page_labels_stale {
        eprintln!(
            "pdfcer: {}: page labels -- source_dropped={} this_document_now_stale={}.",
            input.display(),
            u32::from(pasted.source_page_labels_dropped),
            u32::from(pasted.page_labels_stale),
        );
    }
    report_separations(input, &pasted.separations);
}

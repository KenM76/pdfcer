//! `inspect`'s print-production lines: overprint use and the PDF/X claim.
//! Printed only when there is something to say, so a plain file's
//! `path: PDF version` output is unchanged.

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::prepress::{overprint_use, pdfx_claim};

/// Stdout lines for overprint use (a request, or "unknown" when part of
/// the drawing could not be walked) and any PDF/X claim with its gaps.
/// A walk fault goes to stderr. Nothing is printed for "no overprint".
pub(crate) fn print_prepress(file: &Path, doc: &Document) {
    let view = doc.view();
    let overprint = overprint_use(&view);
    if let Some(first) = overprint.first {
        println!("  overprint: requested (first by object {first})");
    } else if !overprint.is_complete() {
        println!(
            "  overprint: unknown ({} stream(s) could not be walked)",
            overprint.unwalked.len()
        );
        for why in &overprint.unwalked {
            eprintln!("pdfcer: {}: overprint walk: {why}", file.display());
        }
    }
    let Some(claim) = pdfx_claim(&view) else {
        return;
    };
    let mut notes = Vec::new();
    if !claim.is_recognised() {
        notes.push("not a registered level");
    }
    if !claim.carriers_agree() {
        notes.push("XMP and Info disagree");
    }
    if !claim.has_output_intent() {
        notes.push("no GTS_PDFX output intent");
    }
    let suffix = if notes.is_empty() {
        String::new()
    } else {
        format!(" ({})", notes.join("; "))
    };
    println!("  PDF/X claim: {}{suffix}", claim.level);
}

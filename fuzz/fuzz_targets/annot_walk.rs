//! Fuzz target 11: annotation walk + appearance selection
//! (`pdfcer_core::annot`, ISO 32000-1 §12.5; docs/decisions/008 Pass 6.0).
//!
//! Feeds arbitrary bytes to `Document::from_bytes`, then — for every page
//! the tree walk yields — drives [`pdfcer_core::annot::page_annotations`]
//! and [`pdfcer_core::annot::need_appearances`]. This exercises the entire
//! read-side annotation model against untrusted input: the `/Annots`
//! array walk (absent, null, non-array, shared-indirect, entries that are
//! null/non-dictionary), `/Subtype`/`/Rect`/`/F` decoding, and the
//! §12.5.5 `/AP` `/N` selection with its full negative-result taxonomy —
//! including the specific hostile shapes decision 008 calls out for this
//! target:
//!
//! - **cyclic `/AP`** — an `/AP` whose `/N` reference resolves back
//!   through the annotation (or an `/N` that is a self-referential
//!   reference chain): resolution is depth-guarded by `ObjectGraph`, so
//!   selection must terminate, not loop;
//! - **degenerate and inverted `/Rect`** — `Rect::from_corners`
//!   normalises corners in any order, and a zero-area rect must model
//!   without panicking (its degenerate-placement consequence is a
//!   `pdfcer-render` concern, but the model must not choke on it);
//! - **`/AS` naming a missing state** and **`/AP` `/N` that is neither
//!   stream nor dictionary** — both are named negative results
//!   (`StateUnresolved` / `None`) the selector must reach without
//!   indexing past an entry or unwrapping a `None`.
//!
//! Invariant asserted (the crate's panic-free policy): for ANY input, the
//! whole walk returns normally — `Ok`/`Err` from the loader, then a
//! bounded `Vec<Annotation>` per page — and never panics, never aborts,
//! never loops. The per-page result is bounded by `MAX_ANNOTS_PER_PAGE`;
//! libFuzzer's `-rss_limit_mb`/`-timeout` turn any OOM or hang into a
//! finding.
//!
//! This shares the loader entry point with `load_document`, so the
//! existing corpus keeps its value: any input that loads and has a page
//! tree now also drives the annotation model for free.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::annot::{need_appearances, page_annotations};
use pdfcer_core::document::Document;
use pdfcer_core::page_tree::pages;

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::from_bytes(data.to_vec()) else {
        return;
    };
    // Document-scoped /NeedAppearances disclosure (must not panic on a
    // malformed /AcroForm).
    let _ = need_appearances(&doc);
    // Per-page annotation model. The page-tree walk is itself guarded; if
    // it errors on damaged structure there is nothing to walk.
    let Ok(pages) = pages(&doc) else {
        return;
    };
    for page in &pages {
        for annot in page_annotations(&doc, page.id) {
            // Touch every modelled field so a lazily-constructed value
            // cannot hide a panic behind an unused branch.
            let _ = annot.is_widget();
            let _ = annot.subtype_label();
            let _ = annot.flags.suppressed_on_screen();
            let _ = &annot.appearance;
            let _ = annot.rect;
        }
    }
});

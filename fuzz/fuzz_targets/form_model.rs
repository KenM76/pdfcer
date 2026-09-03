//! Fuzz target 13: interactive-form field model
//! (`pdfcer_core::forms`, ISO 32000-1 §12.7; docs/decisions/008 Pass 7).
//!
//! Feeds arbitrary bytes to `Document::from_bytes`, then drives
//! [`pdfcer_core::forms::parse_acroform`] over the whole document. This
//! exercises the entire read-side form model against untrusted input: the
//! `/AcroForm` walk (absent, non-dictionary, malformed `/Fields`), the
//! `/Kids` field-tree DFS with its inheritance context, the field-vs-widget
//! MERGE classification, fully-qualified-name construction, per-type `/V`
//! decoding, `/Opt`/`/I` reading, and XFA/`SigFlags` detection — including
//! the specific hostile shapes decision 008 / the Pass 7 brief call out:
//!
//! - **cyclic `/Parent` / `/Kids`** — a field tree with a reference loop:
//!   the walk's `visited`-id set and `MAX_FIELD_TREE_DEPTH` cap must make
//!   it terminate, never loop;
//! - **a huge `/Kids` array** — bounded by `MAX_FORM_FIELDS` so a hostile
//!   fan-out cannot pin unbounded allocation;
//! - **merge-shape edge cases** — a `/T`-less widget kid, a kid that is
//!   both a field and a widget, a non-terminal carrying `/FT`, a terminal
//!   with no resolvable `/FT`;
//! - **malformed values** — a `/V` of the wrong COS type for the field
//!   type, a `/T` that is not a string, a `/Rect` that is not four numbers.
//!
//! Invariant asserted (the crate's panic-free policy): for ANY input,
//! `parse_acroform` returns normally — `None` (no form) or an `AcroForm`
//! whose field list is bounded by `MAX_FORM_FIELDS` — and never panics,
//! never aborts, never loops. Every modelled field is touched so a
//! lazily-evaluated helper cannot hide a panic behind an unused branch.
//!
//! Shares the loader entry point with `load_document`, so the existing
//! corpus keeps its value: any input that loads now also drives the form
//! model for free.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::forms::parse_acroform;

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::from_bytes(data.to_vec()) else {
        return;
    };
    let Some(form) = parse_acroform(&doc) else {
        return;
    };
    // Touch the document-level disclosures.
    let _ = form.need_appearances;
    let _ = form.sig_flags;
    let _ = form.calc_order_count;
    let _ = form.xfa.is_present();
    let _ = form.fillable_fields().count();
    // Touch every modelled field so a lazily-constructed value cannot hide
    // a panic behind an unused branch.
    for field in &form.fields {
        let _ = field.is_fillable();
        let _ = field.has_appearance();
        let _ = field.value.display_text();
        let _ = field.default_value.display_text();
        let _ = &field.fully_qualified_name;
        let _ = field.flags.read_only();
        for widget in &field.widgets {
            let _ = &widget.on_states;
            let _ = widget.rect;
        }
        // The same-FQN lookup path (may match several representations).
        let _ = form.fields_named(&field.fully_qualified_name).count();
    }
});

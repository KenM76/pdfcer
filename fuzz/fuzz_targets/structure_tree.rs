//! Fuzz target: the structure-tree reader (`pdfcer_core::structure_tree`,
//! G053). Untrusted input: `/StructTreeRoot /K` graphs (cycles, shared
//! kids, malformed items), `/RoleMap` and `/RoleMapNS` chains, `/A` and
//! `/ClassMap` attribute objects; and the painted-bounds walk over the
//! same pages (`pdfcer_core::structure_bounds`): hostile content streams,
//! self-invoking forms, non-finite geometry.
//!
//! Invariants: no panic; every `Element` kid points forward (pre-order) and
//! in bounds; every run index names a run on its page; `element_text` and
//! `element_bbox`, `element_painted_bounds` and
//! `elements_without_painted_bounds` terminate; every no-bounds entry names
//! an element in range.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::structure_bounds::read_painted_bounds;
use pdfcer_core::structure_tree::{StructKid, read_structure_tree};
use pdfcer_core::text_extract::ExtractOptions;

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::from_bytes(data.to_vec()) else {
        return;
    };
    let view = doc.view();
    let Ok(tree) = read_structure_tree(&view, &ExtractOptions::default()) else {
        return;
    };
    for (i, e) in tree.elements.iter().enumerate() {
        for kid in &e.kids {
            match kid {
                StructKid::Element(c) => {
                    assert!(*c > i && *c < tree.elements.len(), "kid order");
                }
                StructKid::MarkedContent {
                    page_index: Some(p),
                    runs,
                    ..
                } => {
                    let page = tree.text.pages.iter().find(|pg| pg.page_index == *p);
                    let len = page.map_or(0, |pg| pg.runs.len());
                    assert!(runs.iter().all(|&r| r < len), "run index in bounds");
                }
                _ => {}
            }
        }
    }
    for i in 0..tree.elements.len().min(256) {
        let _ = tree.element_text(i);
        let _ = tree.element_bbox(i);
    }
    let Ok(painted) = read_painted_bounds(&view, &tree) else {
        return;
    };
    for i in 0..tree.elements.len().min(256) {
        let _ = tree.element_painted_bounds(i, &painted);
    }
    let unbounded = tree.elements_without_painted_bounds(&painted);
    assert!(unbounded.iter().all(|u| u.element < tree.elements.len()));
});

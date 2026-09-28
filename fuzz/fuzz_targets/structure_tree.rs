//! Fuzz target: the structure-tree reader (`pdfcer_core::structure_tree`,
//! G053). Untrusted input: `/StructTreeRoot /K` graphs (cycles, shared
//! kids, malformed items), `/RoleMap` and `/RoleMapNS` chains, `/A` and
//! `/ClassMap` attribute objects.
//!
//! Invariants: no panic; every `Element` kid points forward (pre-order) and
//! in bounds; every run index names a run on its page; `element_text` and
//! `element_bbox` terminate for every element.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::structure_tree::{StructKid, read_structure_tree};
use pdfcer_core::text_extract::ExtractOptions;

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::from_bytes(data.to_vec()) else {
        return;
    };
    let Ok(tree) = read_structure_tree(&doc.view(), &ExtractOptions::default()) else {
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
});

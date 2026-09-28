//! Fuzz target: untagged block layout (`pdfcer_core::block_layout`, G054).
//! Untrusted input: run geometry from any content stream (degenerate,
//! inverted, huge or NaN-adjacent boxes, rotated pages, odd /CropBox),
//! plus the numeric thresholds in `LayoutOptions`.
//!
//! Invariants: no panic; every block's line indices and every line's run
//! indices are in bounds; every line belongs to exactly one block; heading
//! levels are 1..=6.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::block_layout::{BlockKind, LayoutOptions, analyze_layout};
use pdfcer_core::document::Document;
use pdfcer_core::text_extract::ExtractOptions;

fuzz_target!(|data: &[u8]| {
    let (knobs, pdf) = data.split_at(data.len().min(2));
    let options = LayoutOptions::default()
        .with_running_min_fraction(f32::from(knobs.first().copied().unwrap_or(102)) / 255.0)
        .with_margin_band(f32::from(knobs.get(1).copied().unwrap_or(38)) / 255.0);
    let Ok(doc) = Document::from_bytes(pdf.to_vec()) else {
        return;
    };
    let Ok(layout) = analyze_layout(&doc.view(), &ExtractOptions::default(), &options) else {
        return;
    };
    for (page, text) in layout.pages.iter().zip(&layout.text.pages) {
        let mut seen = vec![0u8; page.lines.len()];
        for block in &page.blocks {
            for &l in &block.lines {
                assert!(l < page.lines.len(), "line index in bounds");
                seen[l] += 1;
            }
            if let BlockKind::Heading { level } = block.kind {
                assert!((1..=6).contains(&level), "heading level");
            }
            let _ = block.text(page);
        }
        assert!(seen.iter().all(|&n| n == 1), "each line in one block");
        for line in &page.lines {
            assert!(line.runs.iter().all(|&r| r < text.runs.len()), "run index");
        }
    }
});

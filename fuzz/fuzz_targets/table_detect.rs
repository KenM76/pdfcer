//! Fuzz target: ruled table detection (`pdfcer_core::table_detect`, G055).
//! Untrusted input: any content stream's paths (degenerate, huge, dense
//! or rotated grids, form XObjects) and its text, plus the snap and join
//! tolerances.
//!
//! Invariants: no panic; every cell lies inside its table's grid (span
//! at least 1, `row + row_span <= rows`, `col + col_span <= columns`);
//! every glyph reference is in bounds of the extraction returned.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::table_detect::{TableOptions, detect_tables};
use pdfcer_core::text_extract::ExtractOptions;

fuzz_target!(|data: &[u8]| {
    let (knobs, pdf) = data.split_at(data.len().min(2));
    let options = TableOptions::default()
        .with_snap_tolerance(f32::from(knobs.first().copied().unwrap_or(48)) / 16.0)
        .with_join_tolerance(f32::from(knobs.get(1).copied().unwrap_or(48)) / 16.0);
    let Ok(doc) = Document::from_bytes(pdf.to_vec()) else {
        return;
    };
    let Ok(found) = detect_tables(&doc.view(), &ExtractOptions::default(), &options) else {
        return;
    };
    for table in &found.tables {
        let page = &found.text.pages[table.page_index];
        for cell in &table.cells {
            assert!(cell.row_span >= 1 && cell.col_span >= 1, "span");
            assert!(cell.row + cell.row_span <= table.rows.len(), "row in grid");
            assert!(
                cell.col + cell.col_span <= table.columns.len(),
                "col in grid"
            );
            for g in &cell.glyphs {
                assert!(
                    page.runs
                        .get(g.run)
                        .is_some_and(|r| g.glyph < r.glyphs.len()),
                    "glyph ref"
                );
            }
        }
    }
});

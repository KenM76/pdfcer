//! # Pass 14.0 integration test — editable text model + block recognition
//!
//! Drives the whole `pdfcer_core::text_edit` pipeline over the synthetic
//! `fixtures/synthetic/textblocks/multi-column.pdf` (provenance: that
//! directory's `PROVENANCE.md`): a two-column, four-paragraph, ten-line
//! page. The organising claims, from decision 014 §5.2's acceptance
//! criteria for the 13.0 slice, are:
//!
//! 1. blocks / lines / columns are recognised and COUNTED on a
//!    multi-paragraph / multi-column page (all DERIVED, §14.8 S1-S9);
//! 2. the **sourced-only** accessor is byte-for-byte identical whether or
//!    not provenance capture is enabled — i.e. Pass 4's extraction output
//!    is unperturbed by the new read path;
//! 3. per-glyph **provenance** (operator span, font resource, `Tf` size,
//!    fill colour, matrices) is captured on demand and is SOURCED;
//! 4. hit-test and caret/selection resolve over the recognised structure;
//! 5. nothing is written (there is no write API to call — this Pass is
//!    read-only by construction).

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{BlockRecognitionOptions, EditableTextModel, TextPosition};
use pdfcer_core::text_extract::{self, ContentStreamRef, ExtractOptions, PageText, TextColor};

/// The multi-column fixture path.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/textblocks")
        .join("multi-column.pdf")
}

/// Extract page 0 with the given options.
fn extract_page0(options: &ExtractOptions) -> PageText {
    let doc = Document::load(&fixture()).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    text_extract::extract_page(&doc, &pages[0], 0, options).expect("extraction runs")
}

#[test]
fn recognises_two_columns_four_paragraphs_ten_lines() {
    let page = extract_page0(&ExtractOptions::default());
    let model = EditableTextModel::recognize(&page, &BlockRecognitionOptions::default());

    assert_eq!(model.columns(), 2, "two x-bands 250 units apart");
    assert_eq!(model.lines().len(), 10, "ten separate baselines");
    assert_eq!(
        model.blocks().len(),
        4,
        "two paragraphs per column, split by the 28-unit leading gap"
    );

    let d = model.diagnostics();
    assert_eq!(d.lines_recognized, 10);
    assert_eq!(d.columns_recognized, 2);
    assert_eq!(d.blocks_recognized, 4);
    assert!(d.is_multi_column());
    // Two paragraph breaks, both by the leading gap (no indented lines).
    assert_eq!(d.paragraph_breaks_by_leading, 2);
    assert_eq!(d.paragraph_breaks_by_indent, 0);
    assert!(d.glyphs_clustered > 0);
    // The disclosure note is present — the guessing is visible, not hidden.
    assert!(
        d.notes.iter().any(|n| n.contains("DERIVED")),
        "the derived-structure disclosure must be emitted"
    );
}

#[test]
fn blocks_are_ordered_left_column_then_right() {
    let page = extract_page0(&ExtractOptions::default());
    let model = EditableTextModel::recognize(&page, &BlockRecognitionOptions::default());

    // Column 0 is the LEFT band even though its text sits at x=72; the two
    // left-column blocks come first, then the two right-column blocks.
    let columns: Vec<usize> = model.blocks().iter().map(|b| b.column).collect();
    assert_eq!(columns, vec![0, 0, 1, 1]);

    // The first block reads as the left column's first paragraph.
    let first = model.block_text(&model.blocks()[0]);
    assert!(first.starts_with("Left column paragraph one"), "{first:?}");
    // A right-column block reads as a right-column paragraph.
    let right = model.block_text(&model.blocks()[2]);
    assert!(right.starts_with("Right column paragraph one"), "{right:?}");
}

#[test]
fn sourced_view_is_unchanged_by_provenance_capture() {
    // The whole point of gating provenance behind an option: the Pass 4
    // extraction output must be identical with and without it.
    let plain = extract_page0(&ExtractOptions::default());
    let with_prov = extract_page0(&ExtractOptions::default().with_provenance(true));

    assert_eq!(
        plain.sourced_text(),
        with_prov.sourced_text(),
        "sourced text must not depend on provenance capture"
    );
    assert_eq!(
        plain.plain_text(),
        with_prov.plain_text(),
        "plain text must not depend on provenance capture"
    );
    // Same run structure, same per-glyph geometry — only `provenance`
    // differs (None vs Some), which does not affect these accessors.
    assert_eq!(plain.runs.len(), with_prov.runs.len());
    assert_eq!(
        plain.diagnostics.codes_total,
        with_prov.diagnostics.codes_total
    );

    // And the model's sourced view is exactly the extraction it borrows.
    let model = EditableTextModel::recognize(&plain, &BlockRecognitionOptions::default());
    assert_eq!(model.sourced_view().sourced_text(), plain.sourced_text());
}

#[test]
fn provenance_carries_operator_font_size_and_fill_colour() {
    let page = extract_page0(&ExtractOptions::default().with_provenance(true));
    let model = EditableTextModel::recognize(&page, &BlockRecognitionOptions::default());

    // Every clustered glyph on this page carries provenance from the page's
    // own content stream, the /F1 font at size 10.
    let mut saw_black = false;
    let mut saw_blue = false;
    for line in model.lines() {
        for &gref in &line.glyphs {
            let prov = model
                .provenance(gref)
                .expect("provenance captured for every glyph");
            assert_eq!(prov.content_stream, ContentStreamRef::Page);
            assert_eq!(prov.font_resource.as_deref(), Some(&b"F1"[..]));
            assert!((prov.tf_size - 10.0).abs() < 1e-6, "Tf size is 10");
            assert!(!prov.operator_span.is_empty(), "a real Tj span");
            match prov.fill_color {
                // The blue paragraph: 0 0 1 rg.
                Some(TextColor::Rgb(0.0, 0.0, 1.0)) => saw_blue = true,
                // Black paragraphs: the first left paragraph sets NO colour
                // (the §8.6.8 default, None), and everything after the
                // blue paragraph is reset with an explicit `0 g`
                // (Gray(0.0)). Both are black; provenance faithfully
                // distinguishes "unset default" from "explicitly set".
                None | Some(TextColor::Gray(0.0)) => saw_black = true,
                other => panic!("unexpected fill colour {other:?}"),
            }
        }
    }
    assert!(
        saw_black,
        "the black paragraphs are the default/explicit black"
    );
    assert!(saw_blue, "the second left paragraph is painted 0 0 1 rg");
}

#[test]
fn hit_test_and_selection_resolve_over_the_structure() {
    let page = extract_page0(&ExtractOptions::default().with_provenance(true));
    let model = EditableTextModel::recognize(&page, &BlockRecognitionOptions::default());

    // A point on the very first line (left column, top, baseline y=740)
    // near its left edge lands at the start of that line's first run.
    let caret = model.hit_test(73.0, 742.0).expect("a caret near the text");
    let glyph = model.glyph(model.lines()[0].glyphs[0]).unwrap();
    assert_eq!(caret.run, model.lines()[0].glyphs[0].run);
    assert_eq!(caret.byte_offset, glyph.text_start as usize);

    // A selection from the first to the third glyph of that line covers
    // exactly the glyphs between them.
    let line0 = &model.lines()[0];
    let g0 = model.glyph(line0.glyphs[0]).unwrap();
    let g2 = model.glyph(line0.glyphs[2]).unwrap();
    let start = TextPosition::new(line0.glyphs[0].run, g0.text_start as usize);
    let end = TextPosition::new(line0.glyphs[2].run, g2.text_start as usize);
    let covered = model.resolve_range(start, end);
    assert_eq!(
        covered.len(),
        2,
        "two glyphs lie between the first and third boundaries"
    );
}

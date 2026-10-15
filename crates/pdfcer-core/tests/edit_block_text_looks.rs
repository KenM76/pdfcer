//! `edit_block_text` keeps each surviving character's look and gives an
//! inserted character the look of the one before it (pdfcer-gui request
//! G199, Pass 596.0).
//!
//! Fixture: `fixtures/synthetic/reflow/block_mixed_looks.pdf` (generator
//! `tools/gen-block-mixed-looks-fixture.py`, provenance beside it).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{BlockEditOptions, TextEditPreview};
use pdfcer_core::writer::SaveOptions;

const NEW: &str = "Plain words then boldest and red words, now on a second line.";

fn session() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/reflow/block_mixed_looks.pdf");
    let bytes = std::fs::read(path).expect("fixture: run tools/gen-block-mixed-looks-fixture.py");
    EditSession::new(Document::from_bytes(bytes).expect("parses"))
}

fn chars(run: &TextEditPreview) -> String {
    run.glyphs.iter().filter_map(|g| g.ch).collect()
}

/// The page's content after a full save, as `(operator, operand bytes)`.
fn ops(s: &EditSession) -> Vec<(String, String)> {
    let bytes = s.to_full_bytes(&SaveOptions::default()).expect("save").0;
    let d = Document::from_bytes(bytes).expect("reload");
    let pages = page_tree::pages(&d).expect("pages");
    let view = d.view();
    let cs = ContentStream::from_page(&view, &pages[0]).expect("parses");
    cs.operations()
        .map(|op| {
            let name = String::from_utf8_lossy(op.operator_name(&cs.buf).unwrap_or(b"")).into();
            let start = op
                .operands
                .first()
                .map_or(op.operator.span.start, |o| o.span.start);
            let operands = String::from_utf8_lossy(&cs.buf[start..op.operator.span.start]);
            (name, operands.trim().to_owned())
        })
        .collect()
}

#[test]
fn each_look_previews_as_its_own_run() {
    let s = session();
    let p = s
        .edit_block_text_preview(0, 0, NEW, &BlockEditOptions::default())
        .expect("preview");
    assert_eq!(p.report.looks, 3, "{:?}", p.report);
    assert_eq!(p.runs.len(), 3, "one run per look");
    let bold = p
        .runs
        .iter()
        .find(|r| r.base_font == "Helvetica-Bold")
        .expect("a bold run");
    assert_eq!(chars(bold).trim(), "boldest", "inserted `est` takes bold");
    let red: Vec<_> = p
        .runs
        .iter()
        .filter(|r| r.base_font == "Helvetica")
        .collect();
    assert_eq!(red.len(), 2, "black and red Helvetica runs");
    assert!(
        red.iter().any(|r| chars(r).trim() == "red"),
        "{:?}",
        red.iter().map(|r| chars(r)).collect::<Vec<_>>()
    );
    assert_eq!(p.report.inherited_characters, 7, "`est`, `,` and `now`");
    assert!(
        p.report
            .disclosures
            .iter()
            .any(|d| d.contains("3 looks are kept")),
        "{:?}",
        p.report.disclosures
    );
}

#[test]
fn the_commit_writes_each_look_and_equals_the_preview() {
    let mut s = session();
    let opts = BlockEditOptions::default();
    let p = s
        .edit_block_text_preview(0, 0, NEW, &opts)
        .expect("preview");
    let r = s.edit_block_text(0, 0, NEW, &opts).expect("edit");
    assert_eq!(p.report, r);
    let ops = ops(&s);
    let shown = |font: &str| {
        let mut cur = String::new();
        let mut out = String::new();
        for (name, operands) in &ops {
            if name == "Tf" {
                cur.clone_from(operands);
            } else if (name == "Tj" || name == "TJ") && cur.starts_with(font) {
                out.push_str(operands);
            }
        }
        out
    };
    assert!(shown("/F2").contains("boldest"), "{ops:?}");
    assert!(!shown("/F1").contains("bold"), "{ops:?}");
    assert!(
        ops.iter().any(|(n, o)| n == "rg" && o == "1 0 0"),
        "red is set: {ops:?}"
    );
}

#[test]
fn unchanged_text_inherits_nothing() {
    let s = session();
    let p = s
        .edit_block_text_preview(
            0,
            0,
            "Plain words then bold and red words on a second line.",
            &BlockEditOptions::default(),
        )
        .expect("preview");
    assert_eq!(p.report.inherited_characters, 0, "{:?}", p.report);
    assert_eq!(p.runs.len(), 3);
}

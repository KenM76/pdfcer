//! A Type 0 font's `/FontDescriptor` lives on the descendant CIDFont
//! (ISO 32000-1 §9.7.4.1, Table 117) — never on the parent. `classify_font`
//! used to read it from the parent, so EVERY composite run was classified
//! non-embedded: a false "bundled substitute renders your letters"
//! disclosure on most real documents, and — worse — the embedded-subset
//! floor (R-INV-1) never ran for composite fonts, so a character in a
//! `/ToUnicode` broader than the retained glyph set went through to the
//! writer (pdfcer-gui, 2026-09-05, §4).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::{EditError, EditGlyphSource, EditOptions, EditRequest, RInvTrigger};

fn session(name: &str) -> EditSession {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name);
    EditSession::new(Document::load(&p).unwrap())
}

#[test]
fn a_composite_run_with_an_embedded_descendant_is_reported_embedded() {
    let mut s = session("cidfonttype2-partially-injective-tounicode.pdf");
    let r = s
        .edit_text(
            &EditRequest::find_replace(0, "A", "B"),
            &EditOptions::default(),
        )
        .unwrap();
    assert!(r.subset, "ABCDEF+ tag");
    assert_eq!(
        r.glyph_source,
        EditGlyphSource::Embedded,
        "the descendant's /FontFile2 is the font program; {:?}",
        r.disclosures
    );
    assert!(
        !r.disclosures.iter().any(|d| d.contains("NON-embedded")),
        "{:?}",
        r.disclosures
    );
}

#[test]
fn the_embedded_subset_floor_now_guards_composite_runs() {
    // /ToUnicode maps A, B, C; the page paints only A (CID 1) and C (CID 3).
    let mut s = session("cidfonttype2-subset-floor.pdf");
    // C is carried: fine.
    s.edit_text(
        &EditRequest::find_replace(0, "A", "C"),
        &EditOptions::default(),
    )
    .expect("C's CID is painted on the page");
    // B is in the map but no glyph for CID 2 is painted anywhere on this
    // page — nothing proves the subset carries it. Refused by the floor,
    // as a simple embedded subset would be; before the fix this went to the
    // writer.
    let mut s = session("cidfonttype2-subset-floor.pdf");
    let err = s
        .edit_text(
            &EditRequest::find_replace(0, "A", "B"),
            &EditOptions::default(),
        )
        .expect_err("the floor");
    match err {
        EditError::Refused(r) => {
            assert_eq!(r.trigger, RInvTrigger::TargetAbsent, "{}", r.message);
            assert_eq!(r.character, Some('B'));
            assert!(r.message.contains("R-INV-1"), "{}", r.message);
            assert!(r.message.contains("code 2"), "{}", r.message);
        }
        other => panic!("{other:?}"),
    }
}

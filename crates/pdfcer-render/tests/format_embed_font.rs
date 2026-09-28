//! `format-text` restyles a run into a donor face the document does not
//! carry, embedding a subset of it (`Pass 142.0`).
//!
//! Lives in `pdfcer-render`'s tests for the reason `embed_font_roundtrip.rs`
//! gives: only this crate can both subset a real donor and drive core's
//! format surgery. Every assertion is on the SAVED file re-read, not on the
//! report the edit printed about itself.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::addtext::{self, AddTextRequest};
use pdfcer_core::text_edit::{self, FormatError, FormatOptions, FormatRequest};
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::font::subset::plan_subset;

/// The synthetic donor carrying outlines for exactly `A`, `B`, `C`.
fn donor() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/text/subset-donor.ttf"
    ))
    .expect("donor fixture; run tools/gen-subset-font-fixtures.py")
}

/// `hello.pdf` with a standard-14 run reading `CAB` added to page 0.
fn base_with_run() -> Document {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/hello.pdf"
    ))
    .unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    let out = addtext::add_text(&doc, &AddTextRequest::new(0, (72.0, 600.0), "CAB")).unwrap();
    Document::from_bytes(out.bytes).unwrap()
}

fn plan_for(chars: &[char]) -> pdfcer_core::font_embed::FontEmbedPlan {
    plan_subset(&donor(), 0, chars, "pdfceSubsetDemo", "ABCDEF").expect("donor covers A-C")
}

fn page0_text(bytes: &[u8]) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    pdfcer_core::text_extract::extract_page(&doc, &pages[0], 0, &Default::default())
        .unwrap()
        .sourced_text()
}

/// The five embedded objects are in the saved file, the run is re-encoded as
/// two-byte CIDs under the new key, and the text still reads.
fn assert_embedded(saved: &[u8]) {
    let s = String::from_utf8_lossy(saved);
    for needle in [
        "/Type0",
        "/CIDFontType2",
        "/Identity-H",
        "/FontFile2",
        "/ToUnicode",
        "+pdfceSubsetDemo",
    ] {
        assert!(s.contains(needle), "saved file lacks {needle}");
    }
    assert!(
        page0_text(saved).contains("CAB"),
        "the restyled run must still read: {}",
        page0_text(saved)
    );
}

#[test]
fn a_one_shot_restyle_embeds_the_donor_and_the_run_still_reads() {
    let doc = base_with_run();
    let req = FormatRequest::new(0, "CAB").embedded_font(plan_for(&['A', 'B', 'C']));
    let out = text_edit::set_format(&doc, &req, &FormatOptions::default()).unwrap();
    assert!(out.report.subset, "an embedded subset is reported as one");
    assert!(
        out.report
            .disclosures
            .iter()
            .any(|d| d.contains("EMBEDDED a subset")),
        "the embedding must be disclosed: {:?}",
        out.report.disclosures
    );
    assert_embedded(&out.bytes);
}

#[test]
fn the_session_restyle_embeds_and_undo_removes_every_object() {
    let doc = base_with_run();
    let before = doc.bytes().to_vec();
    let mut session = EditSession::new(doc);
    let req = FormatRequest::new(0, "CAB").embedded_font(plan_for(&['A', 'B', 'C']));
    session
        .format_text(&req, &FormatOptions::default())
        .unwrap();
    let saved = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0;
    assert_embedded(&saved);

    session.undo().expect("the restyle is one undoable command");
    let reverted = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0;
    assert_eq!(
        reverted, before,
        "undo nets to nothing: no font object survives it"
    );
}

#[test]
fn a_donor_lacking_a_glyph_of_the_run_is_refused_before_any_write() {
    let doc = base_with_run();
    // Planned for A and B only; the run also needs C.
    let req = FormatRequest::new(0, "CAB").embedded_font(plan_for(&['A', 'B']));
    let err = text_edit::set_format(&doc, &req, &FormatOptions::default()).unwrap_err();
    assert!(
        matches!(err, FormatError::CoverageFailure(_)),
        "a missing glyph is a coverage refusal: {err:?}"
    );
}

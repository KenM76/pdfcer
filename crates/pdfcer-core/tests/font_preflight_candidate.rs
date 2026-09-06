//! `Pass 142.2` — the font pre-flight answers for the text ABOUT TO BE TYPED.
//!
//! The operator asked (2026-09-05): *"if the character isn't available in a
//! pdf are we able to change to a different font?"* `preview_font_resources`
//! could only test the text already there. `preview_font_resources_for`
//! tests a candidate string against every page face AND the standard 14,
//! through the same gate `set_font` applies, so a chooser can be exact.
//!
//! Fixture: `subset-simple-embedded.pdf` — an embedded SUBSET simple font
//! that carries exactly `A`, `B`, `C` (see `tools/gen-subset-font-fixtures.py`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::{FontAcceptance, Std14Presence};

fn session() -> EditSession {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text/subset-simple-embedded.pdf");
    EditSession::new(Document::load(&p).unwrap())
}

#[test]
fn a_candidate_the_subset_cannot_hold_is_refused_by_character_and_the_std14_can() {
    let s = session();
    let pre = s
        .preview_font_resources_for(0, "A", None, "€")
        .expect("locates by the text there, tests the text to come");
    assert_eq!(pre.candidate.as_deref(), Some("€"));
    assert_eq!(
        pre.text, "A",
        "the located text is still reported as located"
    );

    // The run's own embedded subset has no euro — refused, naming it.
    let run = pre.run_entry().expect("the run's own face is an entry");
    match &run.acceptance {
        FontAcceptance::Refused { character, .. } => assert_eq!(*character, Some('€')),
        other => panic!("the subset should refuse €: {other:?}"),
    }

    // Standard 14: all fourteen are listed, none is on this page.
    assert_eq!(pre.standard_14.len(), 14);
    assert!(
        pre.standard_14
            .iter()
            .all(|e| matches!(e.presence, Std14Presence::WouldBeAdded))
    );
    let by = |name: &str| {
        pre.standard_14
            .iter()
            .find(|e| e.base_font == name)
            .unwrap_or_else(|| panic!("{name} missing"))
    };
    // WinAnsiEncoding carries the euro at 0o200 — every text face accepts.
    for name in [
        "Helvetica",
        "Helvetica-Bold",
        "Times-Roman",
        "Courier-BoldOblique",
    ] {
        assert!(
            by(name).acceptance.is_accepted(),
            "{name}: {:?}",
            by(name).acceptance
        );
    }
    // ZapfDingbats has no euro; Symbol's built-in encoding does carry `Euro`.
    assert!(
        matches!(
            by("ZapfDingbats").acceptance,
            FontAcceptance::Refused { .. }
        ),
        "{:?}",
        by("ZapfDingbats").acceptance
    );
    assert!(
        by("Symbol").acceptance.is_accepted(),
        "{:?}",
        by("Symbol").acceptance
    );
}

#[test]
fn a_candidate_inside_the_subset_is_accepted_and_z_is_named() {
    let s = session();
    let pre = s.preview_font_resources_for(0, "A", None, "CAB").unwrap();
    assert!(pre.run_entry().unwrap().acceptance.is_accepted());

    let pre = s.preview_font_resources_for(0, "A", None, "AZ").unwrap();
    match &pre.run_entry().unwrap().acceptance {
        FontAcceptance::Refused { character, message } => {
            assert_eq!(*character, Some('Z'));
            assert!(message.contains("SUBSET"), "{message}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn without_a_candidate_the_old_query_is_unchanged_and_the_std14_block_is_still_there() {
    let s = session();
    let old = s.preview_font_resources(0, "A", None).unwrap();
    assert_eq!(old.candidate, None);
    assert!(old.run_entry().unwrap().acceptance.is_accepted());
    assert_eq!(old.standard_14.len(), 14, "tested against the located text");
    // An empty candidate means "the located text" — identical verdicts.
    let empty = s.preview_font_resources_for(0, "A", None, "").unwrap();
    assert_eq!(empty.candidate, None);
    assert_eq!(empty.entries, old.entries);
    assert_eq!(empty.standard_14, old.standard_14);
}

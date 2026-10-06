//! `Pass 256.1` — `/ToUnicode` inversion refuses PER CHARACTER, not per font.
//!
//! Before: one collision anywhere in a composite font's `/ToUnicode` refused
//! every edit in that font (`R-INV-4`, "cannot be inverted"), including
//! edits that never touched the colliding character. Now the unambiguous
//! characters edit, and a replacement that needs an ambiguous one is refused
//! by name with the candidate codes.
//!
//! Fixture `cidfonttype2-partially-injective-tounicode.pdf`: CIDs 1 and 2
//! both mean `A`, CID 3 means `B`; the page shows `A` (CID 1) and `B` (CID 3),
//! so `B` is carried by the embedded subset.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::{EditError, EditOptions, EditRequest, RInvTrigger};
use pdfcer_core::writer::SaveOptions;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name)
}

fn session() -> EditSession {
    EditSession::new(
        Document::load(&fixture("cidfonttype2-partially-injective-tounicode.pdf")).unwrap(),
    )
}

fn page_text(s: &EditSession) -> String {
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let text = pdfcer_core::text_extract::extract_page(
        &doc,
        &pages[0],
        0,
        &pdfcer_core::text_extract::ExtractOptions::default(),
    )
    .unwrap();
    text.runs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("|")
}

#[test]
fn the_unambiguous_character_edits_even_though_another_collides() {
    let mut s = session();
    let r = s
        .edit_text(
            &EditRequest::find_replace(0, "A", "B"),
            &EditOptions::default(),
        )
        .expect("B is produced by exactly one code and is carried on the page");
    assert_eq!(r.base_font, "ABCDEF+pdfceSyntheticBox");
    // The font's ambiguity is disclosed even though this edit avoided it.
    assert!(
        r.disclosures
            .iter()
            .any(|d| d.contains("font map:") && d.contains("'A'") && d.contains("1/2")),
        "{:?}",
        r.disclosures
    );
    let text = page_text(&s);
    assert!(text.starts_with('B'), "{text}");
}

#[test]
fn the_ambiguous_character_is_refused_by_name_with_its_codes() {
    let mut s = session();
    let err = s
        .edit_text(
            &EditRequest::find_replace(0, "B", "A"),
            &EditOptions::default(),
        )
        .expect_err("A has two codes; pdfcer will not pick one");
    match err {
        EditError::Refused(r) => {
            assert_eq!(r.trigger, RInvTrigger::Ambiguous);
            assert_eq!(r.character, Some('A'));
            assert!(
                r.message.contains("1, 2"),
                "names both codes: {}",
                r.message
            );
            assert!(r.message.contains("every other character"), "{}", r.message);
            assert!(!r.message.contains("  "), "{}", r.message);
        }
        other => panic!("expected the per-character refusal, got {other:?}"),
    }
    // A mixed replacement fails at the ambiguous character, not before.
    let err = s
        .edit_text(
            &EditRequest::find_replace(0, "B", "BA"),
            &EditOptions::default(),
        )
        .expect_err("the A in BA");
    assert!(
        matches!(err, EditError::Refused(ref r) if r.character == Some('A')),
        "{err:?}"
    );
}

#[test]
fn a_map_with_nothing_invertible_still_refuses_the_whole_font() {
    // The no-/ToUnicode sibling: the font-level refusal survives for the
    // case that genuinely has nothing to invert (its text does not even
    // decode, so the honest answer is NoMatch — see composite_refusal_reachable).
    let mut s =
        EditSession::new(Document::load(&fixture("cidfonttype2-nocmap-embedded.pdf")).unwrap());
    let err = s
        .edit_text(
            &EditRequest::find_replace(0, "A", "B"),
            &EditOptions::default(),
        )
        .expect_err("nothing decodes");
    assert!(
        matches!(err, EditError::NoMatch { .. } | EditError::Refused(_)),
        "{err:?}"
    );
}

/// Every glyph's code on the saved page, in content order.
fn page_codes(s: &EditSession) -> Vec<u32> {
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let opts = pdfcer_core::text_extract::ExtractOptions::default();
    let page = pdfcer_core::text_extract::extract_page(&doc, &pages[0], 0, &opts).unwrap();
    page.runs
        .iter()
        .flat_map(|r| r.glyphs.iter().map(|g| g.code))
        .collect()
}

/// The operator span of the glyph drawn with `code`, the way a shell gets one.
fn operator_span_of(s: &EditSession, code: u32) -> pdfcer_core::span::ByteSpan {
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let opts = pdfcer_core::text_extract::ExtractOptions::default().with_provenance(true);
    let page = pdfcer_core::text_extract::extract_page(&doc, &pages[0], 0, &opts).unwrap();
    page.runs
        .iter()
        .flat_map(|r| r.glyphs.iter())
        .find(|g| g.code == code)
        .and_then(|g| g.provenance.as_ref().map(|p| p.operator_span))
        .expect("the glyph is on the page")
}

#[test]
fn the_repertoire_names_the_letters_drawn_two_ways() {
    let s = session();
    let pin = operator_span_of(&s, 3);
    let rep = s.run_repertoire(0, "", Some(pin)).unwrap();
    assert_eq!(rep.text, "B");
    assert_eq!(
        rep.ambiguous,
        std::collections::BTreeMap::from([('A', vec![1, 2])])
    );
    assert!(!rep.accepts('A'), "{:?}", rep.accepted);
    assert!(rep.accepts('B'), "{:?}", rep.accepted);
}

#[test]
fn an_unchanged_ambiguous_letter_keeps_its_code() {
    let mut s = session();
    let pin = operator_span_of(&s, 1);
    s.edit_text(
        &EditRequest::whole_operator(0, pin, "AB"),
        &EditOptions::default(),
    )
    .expect("the A is carried over unchanged, so its code 1 is kept");
    // The edited operator shows codes 1 and 3 (`<00010003>`); the other
    // operator's B is untouched.
    assert_eq!(page_codes(&s), vec![1, 3, 3]);
    assert!(page_text(&s).starts_with("AB|"), "{}", page_text(&s));
}

#[test]
fn an_added_ambiguous_letter_is_still_refused() {
    let mut s = session();
    let pin = operator_span_of(&s, 1);
    let err = s
        .edit_text(
            &EditRequest::whole_operator(0, pin, "AAB"),
            &EditOptions::default(),
        )
        .expect_err("the second A is new, and two codes produce it");
    match err {
        EditError::Refused(r) => {
            assert_eq!(r.trigger, RInvTrigger::Ambiguous);
            assert_eq!(r.character, Some('A'));
        }
        other => panic!("expected the ambiguity refusal, got {other:?}"),
    }
}

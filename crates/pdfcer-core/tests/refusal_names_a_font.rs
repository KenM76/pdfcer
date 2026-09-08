//! # A refusal that names no remedy is a refusal the operator cannot act on
//!
//! ## The gap, which was discoverability rather than capability
//!
//! `R71` refuses a keystroke whose glyph the run's embedded **subset** lacks.
//! The refusal ended *"or choose a font that covers it"* — correct, and on its
//! own **unusable**: it names no font, and the operator is there precisely
//! because they cannot tell which font would work.
//!
//! ★ The remedy already shipped. `format_text`'s `set_font` **authors** a
//! standard-14 resource on a page that lacks one — no embedding, no `fsType`
//! question, no licensing question (§9.6.2.2). Measured on a real 36-sheet
//! SolidWorks drawing: the two-command sequence works **today**, and nothing
//! in the refusal said so.
//!
//! That is the shape this project has hit before under its own name — a
//! capability nobody can find is not shipped, and no gate detects it, because
//! the code is right, the test is green and the sentence is true.
//!
//! ## Why the standard 14 and not the page's own fonts
//!
//! The page's fonts are exactly the ones that just failed, or subsets of the
//! same drawing equally likely to lack the character. Any face outside the
//! standard 14 still needs embedding, which is a separate decision with its
//! own gate — naming one would point at a door that may not open.
//!
//! ## The measurement behind it
//!
//! Six embedded subsets on that drawing, and the coverage is not the *"every
//! lowercase is absent"* it was reported as:
//!
//! | fonts | printable ASCII | missing |
//! |---|---|---|
//! | four of them | 72/95 | exactly `h j l q z` and `Z` |
//! | two of them | 38/95, 24/95 | all lowercase |
//!
//! ★★ Those are precisely the letters the drawing never used. **A subset can
//! draw exactly what the document already contains**, so the failing edits are
//! the ones introducing a NOVEL character — which is why it presents as
//! working sometimes and not others.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::text_edit::encoding::std14_faces_covering;

/// ★★ An ordinary letter is covered by the twelve TEXT faces, and by neither
/// symbol face.
///
/// The two exclusions are the assertion that matters. They are not
/// special-cased anywhere — `Symbol` and `ZapfDingbats` drop out because their
/// own built-in encodings genuinely do not map `'l'`, which is only true
/// because coverage is *computed* from the same tables the encoder uses rather
/// than from a remembered claim about what "the standard 14" contain.
#[test]
fn an_ordinary_letter_names_the_text_faces_and_not_the_symbol_ones() {
    let faces = std14_faces_covering('l');
    assert_eq!(faces.len(), 12, "twelve text faces cover 'l': {faces:?}");
    assert!(faces.contains(&"Helvetica"));
    assert!(faces.contains(&"Times-Roman"));
    assert!(faces.contains(&"Courier"));
    assert!(
        !faces.contains(&"Symbol"),
        "Symbol's built-in encoding does not map 'l', and nothing hard-codes \
         that -- it falls out of the computation: {faces:?}"
    );
    assert!(
        !faces.contains(&"ZapfDingbats"),
        "nor does ZapfDingbats: {faces:?}"
    );
}

/// ★★ A character NO standard-14 face can show returns nothing.
///
/// The honest-empty case, and the one that stops the list from being
/// decoration. If this returned a face for CJK the refusal would send an
/// operator to a font that fails the same way, which is worse than saying
/// nothing.
#[test]
fn a_character_outside_every_standard_face_names_nothing() {
    for ch in ['中', 'д', '\u{5E78}'] {
        assert!(
            std14_faces_covering(ch).is_empty(),
            "no standard-14 face shows {ch:?}, so none may be offered"
        );
    }
}

/// ★ Greek is covered — by `Symbol` alone — and this test exists because I
/// expected the opposite.
///
/// `'ω'` was written into the empty-case list above on the assumption that
/// "the standard 14 are Latin". They are not: `Symbol`'s built-in encoding
/// carries the Greek alphabet, so a refusal on `'ω'` correctly offers a face
/// that really can show it.
///
/// The computation was right and the expectation was wrong, which is the only
/// reason it surfaced — a test written to agree with the assumption would have
/// pinned the assumption instead.
#[test]
fn greek_is_covered_by_symbol_alone() {
    let faces = std14_faces_covering('ω');
    assert_eq!(
        faces,
        vec!["Symbol"],
        "Symbol carries Greek; no text face does: {faces:?}"
    );
}

/// A ZapfDingbats-only glyph names ZapfDingbats and nothing else.
///
/// The mirror of the first test: it proves the symbol faces are being
/// *consulted*, not merely filtered out. An implementation that skipped them
/// entirely would satisfy every assertion above and fail here.
#[test]
fn a_dingbat_names_only_the_dingbat_face() {
    // U+2701 UPPER BLADE SCISSORS — `a1` in the ZapfDingbats encoding.
    let faces = std14_faces_covering('\u{2701}');
    assert_eq!(
        faces,
        vec!["ZapfDingbats"],
        "the symbol faces are consulted, not skipped: {faces:?}"
    );
}

/// Every returned name is a real `/BaseFont` a caller can pass straight to
/// `set_font`.
///
/// The list is only useful if the strings are the ones the other verb accepts.
/// A refusal naming `"helvetica"` or `"Helvetica Regular"` would read fine and
/// fail when acted on.
#[test]
fn every_named_face_is_spelled_the_way_set_font_expects() {
    let all = std14_faces_covering('A');
    assert!(!all.is_empty());
    for name in all {
        assert!(
            pdfcer_core::fontdata::std14_by_base_font(name).is_some(),
            "{name:?} must round-trip through the standard-14 name table that \
             set_font matches on"
        );
    }
}

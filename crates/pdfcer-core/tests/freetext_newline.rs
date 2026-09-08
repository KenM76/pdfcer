//! # Pressing Enter in a text box must produce a line, not a `?`
//!
//! ## The operator, verbatim
//!
//! > *"when I use the 'Text box' Markup tool, making new lines by pressing
//! > enter just has the items show up as one line with a `?` for each new line
//! > instead."*
//!
//! ## ★★ Two correct functions, composed in the wrong order
//!
//! `vartext::wrap_lines` handles `\n` exactly right — it splits paragraphs on
//! it. `vartext::encode_winansi` is also right on its own terms — a character
//! with no WinAnsi code becomes `?`, counted and disclosed rather than
//! silently dropped.
//!
//! But `winansi_code` returns `None` for **U+000A**: its fast path is
//! `'\u{20}'..='\u{7E}'` and its slow path scans `0x80..=0xFF`, so nothing
//! covers the C0 controls. And the encode ran **first**:
//!
//! ```text
//! "FIRST\nSECOND"  --encode-->  "FIRST?SECOND"  --wrap-->  ONE line
//!                                     ^ the separator, destroyed
//! ```
//!
//! So `split(|&b| b == b'\n')` found nothing, yielded one paragraph, and the
//! multiline branch produced a single line with a `?` in it. **The multiline
//! path was unreachable for any text an operator typed a newline into** —
//! which is every text box with more than one line.
//!
//! ## ★ And a disclosure that fired and misled
//!
//! `miss` counted the newline as a substituted character, so the report said
//! *"N characters had no WinAnsi code and were substituted"*. True, and it
//! points a reader at a character-repertoire problem when the actual loss is a
//! line break. An operator told *"1 character was substituted"* after pressing
//! Enter would not connect the two.
//!
//! ## Why nothing caught it
//!
//! `wrap_lines`' own tests build byte slices directly, so they contain real
//! `0x0A`. The fault lives in the **composition**, and each half is correct in
//! isolation — the same shape as the `unwrap_or(0)` found the same day: the
//! defect is one call earlier than the symptom.
//!
//! ★ Reported with a measurement worth keeping: `add_text` — the *page
//! content* route — handles the same string perfectly. Only the annotation
//! route was affected, so a probe aimed at the wrong verb reports "cannot
//! reproduce" with total confidence.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot_author::TextAnnotSpec;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::fontdata::Std14;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/annot/demo-annotated.pdf")
}

fn session() -> EditSession {
    EditSession::new(Document::load(&fixture()).expect("fixture parses"))
}

fn free_text(text: &str, multiline: bool) -> TextAnnotSpec {
    TextAnnotSpec::FreeText {
        rect: Rect {
            llx: 40.0,
            lly: 40.0,
            urx: 300.0,
            ury: 200.0,
        },
        text: text.to_owned(),
        font: Std14::Helvetica,
        font_size: 12.0,
        color: pdfcer_core::vartext::TextColor::Gray(0.0),
        quadding: pdfcer_core::vartext::Quadding::Left,
        multiline,
        border: None,
        border_width: 0.0,
    }
}

/// The appearance stream bytes of the annotation's `/AP` `/N`.
fn appearance(s: &EditSession, id: ObjId) -> String {
    // Measured on the SAVED bytes rather than the session overlay: the
    // question is what another program would draw (`R159`), and the operator
    // was reporting what he SAW.
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    let doc = Document::from_bytes(bytes).expect("re-parse the saved bytes");
    let Object::Dict(annot) = &doc.get(id).expect("annotation present").value else {
        panic!("annotation is not a dictionary")
    };
    let Some(Object::Dict(ap)) = annot.get(b"AP").map(|o| doc.resolve(o)) else {
        panic!("no /AP")
    };
    let Object::Stream(stream) = doc.resolve(ap.get(b"N").expect("/AP /N")) else {
        panic!("/N is not a stream")
    };
    let raw = stream
        .data_span
        .slice(doc.bytes())
        .expect("appearance bytes")
        .to_vec();
    String::from_utf8_lossy(&raw).into_owned()
}

/// How many text-showing operators the appearance draws.
///
/// One `Tj` per laid-out line, so this counts LINES — the quantity the
/// operator was complaining about, rather than a proxy for it.
fn shown_lines(ap: &str) -> usize {
    ap.matches(") Tj").count()
}

/// ★★★ The report: two lines typed, two lines drawn, no `?`.
#[test]
fn a_newline_in_a_multiline_text_box_makes_a_line() {
    let mut s = session();
    let id = s
        .add_text_annotation(0, &free_text("FIRST LINE\nSECOND LINE", true))
        .expect("author a multiline text box");

    let ap = appearance(&s, id);
    assert!(
        !ap.contains('?'),
        "the newline must not survive as a substituted character; the \
         appearance reads: {ap}"
    );
    assert_eq!(
        shown_lines(&ap),
        2,
        "two paragraphs must draw as two lines; the appearance reads: {ap}"
    );
    assert!(
        ap.contains("FIRST LINE") && ap.contains("SECOND LINE"),
        "and both must be present verbatim: {ap}"
    );
}

/// ★★ `\r\n` gets the same treatment.
///
/// Called out by the report, and it is the half a fix can easily miss: the
/// multiline branch filtered `\r` *per paragraph*, so moving the split
/// upstream without moving that filter leaves a `?` at the end of every line.
#[test]
fn a_windows_line_ending_makes_a_line_too() {
    let mut s = session();
    let id = s
        .add_text_annotation(0, &free_text("FIRST LINE\r\nSECOND LINE", true))
        .expect("author");

    let ap = appearance(&s, id);
    assert!(
        !ap.contains('?'),
        "a CR must not become a substituted character either: {ap}"
    );
    assert_eq!(shown_lines(&ap), 2, "{ap}");
}

/// ★★ A single-line box still FLATTENS newlines to spaces.
///
/// The report flagged this as behaviour to keep, and it is right: a field that
/// cannot wrap has nowhere to put a second line, so a space is the honest
/// rendering. This is the direction a fix could break by treating every
/// newline as a paragraph break everywhere.
#[test]
fn a_single_line_box_still_flattens_the_newline_to_a_space() {
    let mut s = session();
    let id = s
        .add_text_annotation(0, &free_text("FIRST LINE\nSECOND LINE", false))
        .expect("author");

    let ap = appearance(&s, id);
    assert!(!ap.contains('?'), "still not a substitution: {ap}");
    assert_eq!(
        shown_lines(&ap),
        1,
        "a non-multiline box draws exactly one line: {ap}"
    );
    assert!(
        ap.contains("FIRST LINE SECOND LINE"),
        "and the newline reads as a space: {ap}"
    );
}

/// A genuinely unencodable character is STILL substituted and still counted.
///
/// The guard against over-correcting. `?` for a character WinAnsi cannot show
/// is the documented, disclosed behaviour; only the newline was ever wrong.
/// A fix that made `encode_winansi` permissive in general would satisfy every
/// test above and quietly stop reporting real repertoire losses.
#[test]
fn a_real_unencodable_character_is_still_substituted() {
    let mut s = session();
    let id = s
        .add_text_annotation(0, &free_text("BEFORE \u{4E2D} AFTER", true))
        .expect("author");

    let ap = appearance(&s, id);
    assert!(
        ap.contains('?'),
        "CJK has no WinAnsi code and must still become '?': {ap}"
    );
}

/// Consecutive newlines make a blank line, not a run of `?`.
///
/// The shape an operator produces by pressing Enter twice to separate
/// paragraphs — and the case where the old behaviour was most visible,
/// because every blank line became another `?` on the single surviving line.
#[test]
fn two_newlines_make_a_blank_line() {
    let mut s = session();
    let id = s
        .add_text_annotation(0, &free_text("FIRST\n\nTHIRD", true))
        .expect("author");

    let ap = appearance(&s, id);
    assert!(!ap.contains('?'), "{ap}");
    // ★ THREE, not two — and this test was written expecting two.
    //
    // The blank paragraph emits an EMPTY `() Tj` and its own `Td` advance,
    // which is the honest way to occupy a line: the baseline moves whether or
    // not anything is drawn on it, so the following line lands where the
    // operator put it. Skipping the empty show operator would close the gap
    // the operator deliberately typed.
    assert_eq!(
        shown_lines(&ap),
        3,
        "the blank paragraph draws an empty Tj and still advances: {ap}"
    );
    assert!(
        ap.contains("() Tj"),
        "the blank line is an EMPTY show operator, not an omitted one: {ap}"
    );
}

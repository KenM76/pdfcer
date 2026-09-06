//! `Pass 257.0` — the text-edit planners resolve names through the SESSION
//! graph, not the base revision.
//!
//! Reported by `pdfcer-gui` (2026-09-05): `format_text` may create a new
//! `/Font` object and binds it against the session overlay, but `edit_text`
//! then planned with `&self.base`, so the run's `Tf` name dereferenced to
//! `None` and the edit was refused — in two voices depending on how the run
//! was located (pinned: "unresolvable"; by text: `NoMatch`, which is untrue
//! of a page plainly printing the text). The only remedy pdfcer has for a
//! subset-font refusal (swap the face, then type) was unreachable inside a
//! session; it worked only across a save and reopen.
//!
//! The three measurements below are the shell's own (`facewall.rs`), plus
//! one for the preview verbs the same planners serve.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::span::ByteSpan;
use pdfcer_core::text_edit::{
    EditOptions, EditRequest, EditTarget, FontSelector, FormatOptions, FormatRequest,
};
use pdfcer_core::text_extract::{ExtractOptions, extract_page_view};
use pdfcer_core::writer::SaveOptions;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

/// The wall: an embedded subset with no code for `q`.
const SUBSET: &str = "text/subset-simple-embedded.pdf";

fn typed() -> EditRequest {
    EditRequest::find_replace(0, "ABC", "ABCq")
}

fn swap_to_helvetica() -> FormatRequest {
    FormatRequest::new(0, "ABC").font(FontSelector::new("Helvetica"))
}

/// The first show operator's span AS THE SESSION SEES IT — a caret pin.
fn first_operator_span(s: &EditSession) -> ByteSpan {
    let pages = s.pages().expect("pages");
    let view = s.view();
    let page = extract_page_view(
        &view,
        &pages[0],
        0,
        &ExtractOptions::default().with_provenance(true),
    )
    .expect("extract");
    page.runs
        .iter()
        .flat_map(|r| r.glyphs.iter())
        .find_map(|g| g.provenance.as_ref().map(|p| p.operator_span))
        .expect("a glyph with provenance")
}

fn reopened_text(s: &EditSession) -> String {
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
fn a_face_swapped_in_this_session_is_editable_in_this_session_by_find() {
    let mut s = EditSession::new(Document::load(&fixture(SUBSET)).unwrap());
    // 1. The wall stands before the swap (the fixture is what it claims).
    s.edit_text(&typed(), &EditOptions::default())
        .expect_err("the subset carries no code for 'q'");
    // 2. The remedy.
    s.format_text(&swap_to_helvetica(), &FormatOptions::default())
        .expect("Helvetica is a standard face; the swap creates a /Font object in the overlay");
    // 3. The same character, same run, same session — the report's second voice
    //    used to be NoMatch("ABC") here, about text plainly on the page.
    let r = s
        .edit_text(&typed(), &EditOptions::default())
        .expect("the swapped face resolves through the session graph");
    assert_eq!(r.base_font, "Helvetica");
    assert!(reopened_text(&s).contains("ABCq"), "{}", reopened_text(&s));
}

#[test]
fn a_face_swapped_in_this_session_is_editable_in_this_session_by_pin() {
    let mut s = EditSession::new(Document::load(&fixture(SUBSET)).unwrap());
    s.format_text(&swap_to_helvetica(), &FormatOptions::default())
        .unwrap();
    // What a GUI caret produces: a pin on the operator, in the SESSION's bytes.
    let mut req = typed();
    req.pinned_span = Some(first_operator_span(&s));
    req.target = EditTarget::PageContents;
    let r = s.edit_text(&req, &EditOptions::default()).expect(
        "the first voice was Unsupported(\"…unresolvable in the target stream's resources\")",
    );
    assert_eq!(r.base_font, "Helvetica");
    assert!(reopened_text(&s).contains("ABCq"));
}

#[test]
fn the_preview_verbs_see_the_swapped_face_too() {
    // The same planners serve the previews, so the shell's face list and
    // style preview were blind to the swap as well.
    let mut s = EditSession::new(Document::load(&fixture(SUBSET)).unwrap());
    s.format_text(&swap_to_helvetica(), &FormatOptions::default())
        .unwrap();
    let p = s
        .preview_font_resources(0, "ABC", None)
        .expect("locating by text decodes through the swapped face");
    assert_eq!(p.text, "ABC");
}

#[test]
fn the_save_and_reopen_control_still_works() {
    // The shell's control: the identical pair with a save between them
    // succeeded before the fix and must keep succeeding after it.
    let mut a = EditSession::new(Document::load(&fixture(SUBSET)).unwrap());
    a.format_text(&swap_to_helvetica(), &FormatOptions::default())
        .unwrap();
    let (bytes, _) = a.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let mut b = EditSession::new(Document::from_bytes(bytes).unwrap());
    b.edit_text(&typed(), &EditOptions::default())
        .expect("across a save the face is in the base");
    assert!(reopened_text(&b).contains("ABCq"));
}

#[test]
fn a_swap_to_a_face_the_file_already_carries_stays_editable() {
    // The shell's third measurement bounds the defect: when the swap binds
    // to a /Font resource already in the file, no new object is created and
    // the edit was fine even before — it must stay fine.
    let mut s = EditSession::new(Document::load(&fixture("textedit/format_twins.pdf")).unwrap());
    s.format_text(
        &FormatRequest::new(0, "hello").font(FontSelector::new("FB2")),
        &FormatOptions::default(),
    )
    .expect("/FB2 is the plain Times-Bold the file already carries (FB1 is the /Differences twin)");
    let r = s
        .edit_text(
            &EditRequest::find_replace(0, "hello", "jello"),
            &EditOptions::default(),
        )
        .expect("an already-present face resolves either way");
    assert_eq!(r.base_font, "Times-Bold");
    assert!(reopened_text(&s).contains("jello"));
}

//! # Pass 16.0 integration test — add NEW page text (FF-D)
//!
//! Drives `pdfcer_core::text_edit::add_text` (the free engine) and
//! `pdfcer_core::edit::EditSession::add_text` (the undo-able session command)
//! over the three committed `fixtures/synthetic/addtext/` fixtures
//! (provenance: that directory's `PROVENANCE.md`). Each acceptance clause of
//! decision 016 §6's 16.0 slice has a test here:
//!
//! - a new run is ADDED and the ORIGINAL content stream is byte-verbatim (the
//!   input is a byte-prefix of the incremental output — R32/R46);
//! - the added run re-extracts and is recognised as a first-class
//!   `Run`/`Line`/`Block` (routed into the Pass 14.0 model) AND is editable by
//!   the Pass 14.1 surgery;
//! - the §7.7.3.4 inheritance trap is handled: the shared ancestor `/Pages`
//!   `/Resources` object is NOT re-emitted; the page gets its own;
//! - a tagged page emits the R73 untagged disclosure;
//! - a glyph the face lacks is refused-and-disclosed (R71), no output produced;
//! - one undo-able `CommandKind::AddText`; undo restores the byte-identical
//!   original (an empty dirty set).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    // The Pass 16.2 preview-vs-commit test drives a table of 9-field cases; a
    // named struct would only move the same fields, so the tuple stays.
    clippy::type_complexity
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditSession};
use pdfcer_core::fontdata::Std14;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{
    AddTextError, AddTextRequest, BlockAlignment, BlockRecognitionOptions, EditOptions,
    EditRequest, EditableTextModel, FontProvenance, NewTextColor, add_text, edit_text,
    preview_wrap,
};
use pdfcer_core::text_extract::{self, ExtractOptions};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/addtext")
        .join(name)
}

fn load(name: &str) -> Document {
    Document::load(&fixture(name)).expect("fixture loads")
}

/// The full recognised block text of page 0 of `bytes` — proves the added run
/// is clustered into the Pass 14.0 hierarchy, not just present in the stream.
fn page0_block_text(bytes: &[u8]) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).expect("output reloads");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    let page = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default())
        .expect("extract");
    let model = EditableTextModel::recognize(&page, &BlockRecognitionOptions::default());
    model
        .blocks()
        .iter()
        .map(|b| model.block_text(b))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn plain_add_keeps_original_byte_identical_and_renders_new_run() {
    let doc = load("plain.pdf");
    let orig = std::fs::read(fixture("plain.pdf")).unwrap();

    let req = AddTextRequest::new(0, (100.0, 650.0), "Hello world").with_size(14.0);
    let out = add_text(&doc, &req).expect("add succeeds");

    // The original content stream is byte-verbatim: the input is a prefix of
    // the incremental output (only the page dict + two new objects are added).
    assert!(
        out.bytes.starts_with(&orig),
        "the original bytes must be an untouched prefix of the incremental output"
    );
    assert!(out.bytes.len() > orig.len());
    assert_eq!(out.report.base_font, "Helvetica");
    assert!(matches!(out.report.provenance, FontProvenance::Bundled));
    assert!(
        !out.report.gave_page_own_resources,
        "plain page has own /Resources"
    );
    assert!(!out.report.tagged_untagged, "plain page is not tagged");

    // The new run re-extracts and is recognised as a block (editable/formattable).
    let text = page0_block_text(&out.bytes);
    assert!(
        text.contains("Original page text"),
        "original text survives: {text:?}"
    );
    assert!(
        text.contains("Hello world"),
        "new run recognised as a block: {text:?}"
    );
}

#[test]
fn added_run_is_editable_by_the_14_1_surgery() {
    // "Once added, it IS page text" (decision 016 §3.4): the new run must be
    // editable by the same in-place mechanism as any other run.
    let doc = load("plain.pdf");
    let out = add_text(&doc, &AddTextRequest::new(0, (100.0, 650.0), "Howdy")).expect("add");

    let reloaded = Document::from_bytes(out.bytes).expect("reload");
    let req = EditRequest::find_replace(0, "Howdy", "Hiya");
    let edited = edit_text(&reloaded, &req, &EditOptions::default())
        .expect("the freshly-added run is editable in place");
    let text = page0_block_text(&edited.bytes);
    assert!(
        text.contains("Hiya"),
        "the added run was edited in place: {text:?}"
    );
}

#[test]
fn colour_and_font_choice_are_honoured() {
    let doc = load("plain.pdf");
    let req = AddTextRequest::new(0, (100.0, 650.0), "Red Times")
        .with_font(pdfcer_core::fontdata::Std14::TimesRoman)
        .with_color(NewTextColor::Rgb(1.0, 0.0, 0.0));
    let out = add_text(&doc, &req).expect("add");
    assert_eq!(out.report.base_font, "Times-Roman");
    // The written dict names the chosen Standard-14 face, and the run paints in
    // DeviceRGB (`rg`) — both visible in the appended update section.
    let orig = std::fs::read(fixture("plain.pdf")).unwrap();
    let tail = &out.bytes[orig.len()..];
    let tail_s = String::from_utf8_lossy(tail);
    assert!(
        tail_s.contains("/BaseFont /Times-Roman"),
        "chosen face written"
    );
    assert!(tail_s.contains("1 0 0 rg"), "RGB fill emitted: {tail_s}");
}

#[test]
fn inherited_resources_add_does_not_mutate_the_shared_ancestor() {
    let doc = load("inherited-resources.pdf");
    let req = AddTextRequest::new(0, (120.0, 600.0), "Added here");
    let out = add_text(&doc, &req).expect("add");

    assert!(
        out.report.gave_page_own_resources,
        "page inherited /Resources"
    );
    assert!(
        out.report
            .disclosures
            .iter()
            .any(|d| d.contains("INHERITED") && d.contains("NOT modified")),
        "the inheritance-safe disclosure must be present"
    );

    // The shared /Pages node is object 2. It must appear exactly ONCE in the
    // file (the original) — never re-emitted into the incremental update.
    let count = count_obj_definitions(&out.bytes, 2);
    assert_eq!(
        count, 1,
        "the shared ancestor /Resources object was re-emitted"
    );
    // The page dict (object 3) IS re-emitted (its /Resources now its own).
    assert_eq!(count_obj_definitions(&out.bytes, 3), 2);
    // Sibling page (object 4) untouched.
    assert_eq!(count_obj_definitions(&out.bytes, 4), 1);
}

#[test]
fn tagged_page_add_emits_the_untagged_disclosure() {
    let doc = load("tagged.pdf");
    let out = add_text(
        &doc,
        &AddTextRequest::new(0, (120.0, 600.0), "Added tagged"),
    )
    .expect("add");
    assert!(out.report.tagged_untagged, "tagged page detected");
    assert!(
        out.report
            .disclosures
            .iter()
            .any(|d| d.contains("untagged page content") && d.contains("R73")),
        "the R73 untagged disclosure must be present: {:?}",
        out.report.disclosures
    );
    // The new run is still real, extractable page content.
    assert!(page0_block_text(&out.bytes).contains("Added tagged"));
}

#[test]
fn missing_glyph_is_refused_by_name_and_produces_no_output() {
    let doc = load("plain.pdf");
    // U+4E2D is outside WinAnsi's single-byte repertoire -> refuse (R71).
    let err = add_text(&doc, &AddTextRequest::new(0, (100.0, 650.0), "hi \u{4E2D}")).unwrap_err();
    match err {
        AddTextError::Refused(r) => {
            assert_eq!(r.character, Some('\u{4E2D}'));
            assert!(r.base_font.contains("Helvetica"));
        }
        other => panic!("expected a named refusal, got {other:?}"),
    }
}

#[test]
fn empty_text_and_bad_size_are_named_refusals() {
    let doc = load("plain.pdf");
    assert!(matches!(
        add_text(&doc, &AddTextRequest::new(0, (10.0, 10.0), "")),
        Err(AddTextError::EmptyText)
    ));
    assert!(matches!(
        add_text(
            &doc,
            &AddTextRequest::new(0, (10.0, 10.0), "x").with_size(0.0)
        ),
        Err(AddTextError::InvalidSize(_))
    ));
    assert!(matches!(
        add_text(&doc, &AddTextRequest::new(9, (10.0, 10.0), "x")),
        Err(AddTextError::PageIndex(9))
    ));
}

#[test]
fn supplied_provenance_is_echoed_in_the_report() {
    // The core engine records the caller's provenance choice verbatim; the
    // written dict is identical (no embedding).
    let doc = load("plain.pdf");
    let req = AddTextRequest::new(0, (10.0, 10.0), "x").with_provenance(FontProvenance::Supplied);
    let out = add_text(&doc, &req).expect("add");
    assert!(matches!(out.report.provenance, FontProvenance::Supplied));
    assert!(
        out.report
            .disclosures
            .iter()
            .any(|d| d.contains("supplied Standard-14"))
    );
}

#[test]
fn session_add_text_is_one_undoable_command_that_restores_the_original() {
    let doc = load("plain.pdf");
    let mut session = EditSession::new(doc);

    let report = session
        .add_text(&AddTextRequest::new(0, (100.0, 650.0), "Session run"))
        .expect("session add succeeds");
    assert_eq!(report.base_font, "Helvetica");

    // One undo-able command of the AddText kind.
    assert_eq!(session.undo_kind(), Some(CommandKind::AddText));
    // Before undo, the save-time diff is non-empty (page dict + 2 new objects).
    assert!(!session.dirty_set().is_empty());

    // Undo removes the created objects and restores the page dict to its base
    // value -> the diff is empty -> a save would be byte-identical.
    assert_eq!(session.undo(), Some(CommandKind::AddText));
    assert!(
        session.dirty_set().is_empty(),
        "undo must restore the byte-identical original (empty dirty set)"
    );
    assert!(!session.dirty_set().changes_content());

    // Redo re-applies the same command.
    assert_eq!(session.redo(), Some(CommandKind::AddText));
    assert!(!session.dirty_set().is_empty());
}

// ===================================================================
// Pass 16.1 — boxed add + wrap via the shipped 15.x reflow breaker.
// ===================================================================
//
// These use **Courier** (Std-14 monospace: every glyph, and the space, is 600
// units = 0.6·size wide) so wrap breaks and per-line origins are hand-
// computable. At size 10, each char and the space are 6.0pt.

/// The tail of an incremental output = the bytes AFTER the original file
/// (the appended update section holding the new content stream + font dict +
/// re-emitted page dict). All the 16.1 emission lives here; the original is a
/// byte-verbatim prefix.
fn incremental_tail(orig: &[u8], out: &[u8]) -> String {
    assert!(
        out.starts_with(orig),
        "output must be an incremental append"
    );
    String::from_utf8_lossy(&out[orig.len()..]).into_owned()
}

#[test]
fn boxed_wrap_matches_hand_computed_breaks_and_keeps_original_byte_identical() {
    let doc = load("plain.pdf");
    let orig = std::fs::read(fixture("plain.pdf")).unwrap();

    // Six "aa" words (each 12pt) with 6pt spaces; box width 30 fits exactly
    // TWO words per line ("aa aa" = 12+6+12 = 30; a third would be 48 > 30),
    // so six words wrap to THREE lines — the hand-computed break.
    let req = AddTextRequest::new(0, (0.0, 0.0), "aa aa aa aa aa aa")
        .with_font(Std14::Courier)
        .with_size(10.0)
        .with_box(72.0, 300.0, 30.0, 200.0);
    let out = add_text(&doc, &req).expect("boxed add succeeds");

    // Original content stream byte-verbatim (R32/R46): input is a prefix.
    assert!(out.bytes.starts_with(&orig));
    assert_eq!(out.report.wrapped_lines, Some(3), "6 words / 2-per-line");
    assert_eq!(out.report.box_overflow_lines, 0, "fits the box height");
    assert_eq!(out.report.page_overflow_pt, 0.0, "on the page");
    assert_eq!(out.report.alignment, Some(BlockAlignment::Left));

    // Three lines ⇒ three absolute-Tm placements in the appended stream.
    let tail = incremental_tail(&orig, &out.bytes);
    assert_eq!(
        tail.matches(" Tm\n").count(),
        3,
        "one Tm per wrapped line: {tail}"
    );

    // The added block re-extracts as recognised editable text (routed into the
    // 14.0 model): all six words survive as page content.
    let text = page0_block_text(&out.bytes);
    assert!(text.contains("aa aa"), "wrapped run recognised: {text:?}");
}

#[test]
fn boxed_left_center_right_place_the_line_correctly() {
    // One word "ab" (Courier size 10 ⇒ 12pt) in a 100pt-wide box at llx=72.
    //   left   → origin_x = 72
    //   center → 72 + (100-12)/2 = 116
    //   right  → 72 + (100-12)   = 160
    let doc = load("plain.pdf");
    let orig = std::fs::read(fixture("plain.pdf")).unwrap();
    let mk = |align: BlockAlignment| {
        let req = AddTextRequest::new(0, (0.0, 0.0), "ab")
            .with_font(Std14::Courier)
            .with_size(10.0)
            .with_box(72.0, 400.0, 100.0, 50.0)
            .with_alignment(align);
        let out = add_text(&doc, &req).expect("add");
        incremental_tail(&orig, &out.bytes)
    };
    assert!(
        mk(BlockAlignment::Left).contains("1 0 0 1 72 "),
        "left flush"
    );
    assert!(
        mk(BlockAlignment::Center).contains("1 0 0 1 116 "),
        "centred origin"
    );
    assert!(
        mk(BlockAlignment::Right).contains("1 0 0 1 160 "),
        "right flush"
    );
}

#[test]
fn boxed_justified_distributes_slack_and_reaches_the_right_edge() {
    // Four "aa" words (12pt each, 6pt space), box width 60.
    //   greedy: "aa aa aa" = 12+6+12+6+12 = 48 ≤ 60; +"aa" = 66 > 60 → break.
    //   line0 = 3 words (natural 48, 2 gaps), line1 = 1 word (the last line).
    // Justified: line0 gets slack 60-48 = 12 across 2 gaps = 6pt/gap. The TJ
    // number that ADDS 6pt at size 10 is -(6)*1000/10 = -600 (negative opens
    // the gap, §9.4.3). natural 48 + 2×6 = 60 = box width ⇒ the line reaches
    // the right edge. The LAST line is a plain Tj, never stretched (§4.1).
    let doc = load("plain.pdf");
    let orig = std::fs::read(fixture("plain.pdf")).unwrap();
    let req = AddTextRequest::new(0, (0.0, 0.0), "aa aa aa aa")
        .with_font(Std14::Courier)
        .with_size(10.0)
        .with_box(72.0, 400.0, 60.0, 80.0)
        .with_alignment(BlockAlignment::Justified);
    let out = add_text(&doc, &req).expect("justified add");
    assert_eq!(out.report.wrapped_lines, Some(2));
    assert_eq!(out.report.alignment, Some(BlockAlignment::Justified));

    let tail = incremental_tail(&orig, &out.bytes);
    // Exactly one justified line (a TJ array) with the -600 per-gap number,
    // and the last line as a plain (aa) Tj (un-stretched).
    assert_eq!(
        tail.matches("] TJ").count(),
        1,
        "one justified line: {tail}"
    );
    assert!(tail.contains("-600"), "per-gap slack number: {tail}");
    assert!(tail.contains("(aa) Tj"), "last line un-stretched: {tail}");
    // The justified line starts flush-left at the box llx (the right flush is
    // the TJ slack, not an origin shift).
    assert!(
        tail.contains("1 0 0 1 72 "),
        "justified starts at llx: {tail}"
    );
}

#[test]
fn boxed_overflow_is_disclosed_and_every_line_is_emitted_not_clipped() {
    // A narrow, SHORT box low on the page: eight one-word lines forced by a
    // 12pt width, a 20pt-tall box (only ~2 lines fit), placed so growth also
    // passes the page bottom. R76: everything is DISCLOSED and EMITTED.
    let doc = load("plain.pdf");
    let orig = std::fs::read(fixture("plain.pdf")).unwrap();
    let req = AddTextRequest::new(0, (0.0, 0.0), "aa aa aa aa aa aa aa aa")
        .with_font(Std14::Courier)
        .with_size(10.0)
        // width 12 ⇒ one "aa" per line ⇒ 8 lines; box bottom at y=10, height 20.
        .with_box(40.0, 10.0, 12.0, 20.0);
    let out = add_text(&doc, &req).expect("overflowing add still succeeds");
    let r = &out.report;
    assert_eq!(r.wrapped_lines, Some(8), "one word per 12pt line");
    assert!(r.box_overflow_lines > 0, "text taller than the box");
    assert!(r.page_overflow_pt > 0.0, "grows past the page bottom");
    assert!(
        r.disclosures
            .iter()
            .any(|d| d.contains("overflows the box")),
        "box overflow disclosed: {:?}",
        r.disclosures
    );
    assert!(
        r.disclosures.iter().any(|d| d.contains("past the page")),
        "page overflow disclosed: {:?}",
        r.disclosures
    );

    // NOT clipped: all eight lines are emitted as real placements, including
    // the off-page ones (Tm count == wrapped line count).
    let tail = incremental_tail(&orig, &out.bytes);
    assert_eq!(tail.matches(" Tm\n").count(), 8, "no line dropped: {tail}");
    // And the output still reloads as a valid document.
    assert!(Document::from_bytes(out.bytes).is_ok());
}

#[test]
fn boxed_session_add_is_one_undoable_command_restoring_the_original() {
    let doc = load("plain.pdf");
    let mut session = EditSession::new(doc);

    let report = session
        .add_text(
            &AddTextRequest::new(0, (0.0, 0.0), "boxed session run wraps here")
                .with_font(Std14::Courier)
                .with_size(10.0)
                .with_box(72.0, 500.0, 60.0, 120.0),
        )
        .expect("boxed session add succeeds");
    assert!(report.wrapped_lines.unwrap() >= 2, "it wrapped");

    // The SAME single undo-able AddText command as the point path.
    assert_eq!(session.undo_kind(), Some(CommandKind::AddText));
    assert!(!session.dirty_set().is_empty());
    assert_eq!(session.undo(), Some(CommandKind::AddText));
    assert!(
        session.dirty_set().is_empty(),
        "undo restores the byte-identical original (empty dirty set)"
    );
    assert_eq!(session.redo(), Some(CommandKind::AddText));
    assert!(!session.dirty_set().is_empty());
}

#[test]
fn boxed_missing_glyph_is_refused_by_name() {
    // A glyph the face lacks is refused (R71) in boxed mode too, before any
    // content is built.
    let doc = load("plain.pdf");
    let err = add_text(
        &doc,
        &AddTextRequest::new(0, (0.0, 0.0), "ok \u{4E2D} bad")
            .with_font(Std14::Courier)
            .with_box(72.0, 400.0, 200.0, 100.0),
    )
    .unwrap_err();
    assert!(
        matches!(err, AddTextError::Refused(ref r) if r.character == Some('\u{4E2D}')),
        "expected a named refusal, got {err:?}"
    );
}

#[test]
fn boxed_invalid_box_and_whitespace_only_are_named_refusals() {
    let doc = load("plain.pdf");
    // Zero-width box: nothing can be wrapped into it.
    assert!(matches!(
        add_text(
            &doc,
            &AddTextRequest::new(0, (0.0, 0.0), "x").with_box(72.0, 400.0, 0.0, 50.0)
        ),
        Err(AddTextError::InvalidBox(..))
    ));
    // Whitespace-only text: no words to wrap.
    assert!(matches!(
        add_text(
            &doc,
            &AddTextRequest::new(0, (0.0, 0.0), "   ").with_box(72.0, 400.0, 50.0, 50.0)
        ),
        Err(AddTextError::NoWordsToWrap)
    ));
}

/// Count `N 0 obj` definitions in `bytes` — how many times object `n` is
/// written (1 = original only; 2 = original + one incremental update).
///
/// The needle is delimited (`\nN 0 obj\n`, the exact shape the generator
/// emits) so object 2 is never falsely counted inside `12 0 obj`.
fn count_obj_definitions(bytes: &[u8], n: u32) -> usize {
    let needle = format!("\n{n} 0 obj\n");
    let needle = needle.as_bytes();
    bytes.windows(needle.len()).filter(|w| *w == needle).count()
}

// ---------------------------------------------------------------------------
// Pass 16.2 — the PURE, read-only wrap PREVIEW shares ONE layout path with the
// mutating boxed `add_text`, so the ghost the GUI draws is the run it commits
// (decision 016 §0.3 / Pass 16.2 spec §4.2 / §0.3 "no GUI-side approximation").
// ---------------------------------------------------------------------------

/// Every absolute `1 0 0 1 x y Tm` placement in the appended stream, as
/// `(x, y)` pairs in emission order — the exact per-line origins the committed
/// boxed add wrote, so a preview can be checked against them.
fn tm_placements(tail: &str) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for line in tail.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_suffix(" Tm")
            && let Some(nums) = rest.strip_prefix("1 0 0 1 ")
        {
            let parts: Vec<f64> = nums
                .split_whitespace()
                .filter_map(|t| t.parse::<f64>().ok())
                .collect();
            if parts.len() == 2 {
                out.push((parts[0], parts[1]));
            }
        }
    }
    out
}

/// The 0-based page cropbox of the fixture, so the preview's `page_overflow_pt`
/// is computed against the SAME page the commit uses.
fn page0_crop(doc: &Document) -> page_tree::Rect {
    let pages = page_tree::pages(doc).expect("page tree walks");
    pages[0].crop_box
}

#[test]
fn preview_wrap_lines_match_committed_boxed_add_for_identical_inputs() {
    let doc = load("plain.pdf");
    let orig = std::fs::read(fixture("plain.pdf")).unwrap();
    let crop = page0_crop(&doc);

    // A representative spread: every alignment, a couple of faces/sizes, an
    // explicit and a derived leading, a box that fits and one that overflows.
    let cases: &[(
        &str,
        f64,
        f64,
        f64,
        f64,
        Std14,
        f64,
        BlockAlignment,
        Option<f64>,
    )] = &[
        // (text, box llx, lly, w, h, font, size, alignment, leading)
        (
            "wrap this run into the box now",
            72.0,
            300.0,
            120.0,
            200.0,
            Std14::Helvetica,
            12.0,
            BlockAlignment::Left,
            None,
        ),
        (
            "wrap this run into the box now",
            72.0,
            300.0,
            120.0,
            200.0,
            Std14::Helvetica,
            12.0,
            BlockAlignment::Center,
            None,
        ),
        (
            "wrap this run into the box now",
            72.0,
            300.0,
            120.0,
            200.0,
            Std14::TimesRoman,
            14.0,
            BlockAlignment::Right,
            Some(18.0),
        ),
        (
            "justify me across the whole width please",
            72.0,
            300.0,
            140.0,
            200.0,
            Std14::Helvetica,
            12.0,
            BlockAlignment::Justified,
            None,
        ),
        // A narrow, short box: overflows both the box height and (near the page
        // bottom) the page — the R76 disclose-and-emit path.
        (
            "one two three four five six seven eight",
            40.0,
            10.0,
            24.0,
            20.0,
            Std14::Courier,
            10.0,
            BlockAlignment::Left,
            None,
        ),
    ];

    for (i, &(text, llx, lly, w, h, font, size, align, leading)) in cases.iter().enumerate() {
        let req = AddTextRequest::new(0, (0.0, 0.0), text)
            .with_font(font)
            .with_size(size)
            .with_box(llx, lly, w, h)
            .with_alignment(align)
            .with_leading(leading);
        let out = add_text(&doc, &req).unwrap_or_else(|e| panic!("case {i} commits: {e}"));
        let tail = incremental_tail(&orig, &out.bytes);
        let committed = tm_placements(&tail);

        let bx = page_tree::Rect::from_corners(llx, lly, llx + w, lly + h);
        let preview = preview_wrap(text, bx, crop, font, size, align, leading)
            .unwrap_or_else(|e| panic!("case {i} previews: {e}"));

        // Report/overflow measures agree exactly.
        assert_eq!(
            preview.wrapped_lines,
            out.report.wrapped_lines.unwrap(),
            "case {i}: wrapped_lines"
        );
        assert_eq!(
            preview.box_overflow_lines, out.report.box_overflow_lines,
            "case {i}: box_overflow_lines"
        );
        assert!(
            (preview.page_overflow_pt - out.report.page_overflow_pt).abs() < 1e-6,
            "case {i}: page_overflow_pt {} vs {}",
            preview.page_overflow_pt,
            out.report.page_overflow_pt
        );
        assert_eq!(
            Some(preview.alignment),
            out.report.alignment,
            "case {i}: alignment"
        );

        // Per-line origins agree with the committed `Tm` operands. The preview
        // includes blank lines (empty text); the commit emits a `Tm` only for
        // a NON-blank line — so compare the preview's non-blank lines, in order,
        // to the committed placements one-for-one.
        let non_blank: Vec<&pdfcer_core::text_edit::WrapPreviewLine> = preview
            .lines
            .iter()
            .filter(|l| !l.text.is_empty())
            .collect();
        assert_eq!(
            non_blank.len(),
            committed.len(),
            "case {i}: one preview line per committed Tm ({tail})"
        );
        for (li, (pl, &(cx, cy))) in non_blank.iter().zip(committed.iter()).enumerate() {
            assert!(
                (pl.origin_x - cx).abs() < 1e-4,
                "case {i} line {li}: origin_x {} vs committed {cx}",
                pl.origin_x
            );
            assert!(
                (pl.baseline_y - cy).abs() < 1e-4,
                "case {i} line {li}: baseline_y {} vs committed {cy}",
                pl.baseline_y
            );
        }
    }
}

#[test]
fn preview_wrap_refuses_where_the_commit_would_refuse() {
    let doc = load("plain.pdf");
    let crop = page0_crop(&doc);
    let bx = page_tree::Rect::from_corners(72.0, 400.0, 272.0, 500.0);

    // Empty text → EmptyText (same guard order as plan_add_text).
    assert!(matches!(
        preview_wrap(
            "",
            bx,
            crop,
            Std14::Helvetica,
            12.0,
            BlockAlignment::Left,
            None
        ),
        Err(AddTextError::EmptyText)
    ));
    // Non-positive size → InvalidSize.
    assert!(matches!(
        preview_wrap(
            "x",
            bx,
            crop,
            Std14::Helvetica,
            0.0,
            BlockAlignment::Left,
            None
        ),
        Err(AddTextError::InvalidSize(_))
    ));
    // Degenerate box → InvalidBox.
    let bad = page_tree::Rect::from_corners(72.0, 400.0, 72.0, 500.0);
    assert!(matches!(
        preview_wrap(
            "x",
            bad,
            crop,
            Std14::Helvetica,
            12.0,
            BlockAlignment::Left,
            None
        ),
        Err(AddTextError::InvalidBox(_, _))
    ));
    // Whitespace-only → NoWordsToWrap.
    assert!(matches!(
        preview_wrap(
            "   ",
            bx,
            crop,
            Std14::Helvetica,
            12.0,
            BlockAlignment::Left,
            None
        ),
        Err(AddTextError::NoWordsToWrap)
    ));
    // A glyph the Latin face lacks (a CJK scalar) → Refused (R71), exactly like
    // the committed boxed add.
    let refused = preview_wrap(
        "\u{4e2d}",
        bx,
        crop,
        Std14::Helvetica,
        12.0,
        BlockAlignment::Left,
        None,
    );
    assert!(matches!(refused, Err(AddTextError::Refused(_))));
}

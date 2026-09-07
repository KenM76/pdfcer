//! # Integration test — `EditSession::place_text`, the plain-text import
//!
//! Drives the verb that answers `pdfcer-gui`'s
//! *"there is no route from a text file back into a pdf"*: a `&str` plus a
//! [`PageTemplate`] in, as many created pages as the text needs out, as ONE
//! undo entry.
//!
//! What each test here is actually defending, because "it produced some pages"
//! is not a claim worth asserting:
//!
//! | test | the failure it would catch |
//! |---|---|
//! | `an_import_creates_the_pages_its_text_needs` | the pagination collapsing to one page — the exact R76 failure the request was filed about |
//! | `every_word_of_the_input_reaches_the_document` | text placed off the sheet, or a page silently skipped: it re-extracts every page and checks the words are *there* |
//! | `no_placed_line_falls_outside_the_column_it_was_paginated_into` | this module's line-fitting arithmetic drifting from `addtext`'s placement arithmetic — the two are separate code and agree only by construction |
//! | `the_whole_import_is_one_undo_entry` | the `1 + N` commands leaking to the operator, which the requester asked about by name |
//! | `an_unencodable_character_refuses_the_whole_import_and_names_every_one` | a silent drop — the failure this project treats as worst — and a refusal that touched the session before refusing |
//! | `dropping_unencodable_characters_is_opt_in_and_says_exactly_what_was_lost` | the opt-in dropping text without saying which text |
//! | `every_input_character_is_accounted_for` | the disclosure counts drifting from the input: it asserts the buckets sum to the input length |
//! | `a_form_feed_starts_a_new_page` | the `export_text` round trip losing its pagination |
//! | `an_empty_or_whitespace_only_import_is_refused_by_name` | an empty `.txt` producing blank pages that look deliberate |
//! | `an_import_appends_and_leaves_the_existing_page_alone` | the import overwriting or reordering what was already open |
//!
//! The fixture is `fixtures/synthetic/hello.pdf` — one 200×120 page whose
//! `/MediaBox` and `/Resources` are both INHERITED from the `/Pages` node, so
//! every test here also drives the §7.7.3.4 inheritance trap on the way past.
//! (`minimal.pdf` cannot be used: its page inherits no `/Resources` from
//! anywhere, and `page_tree::pages` refuses it as `MissingRequired`.)

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::fontdata::Std14;
use pdfcer_core::page_tree;
use pdfcer_core::pageops::InsertPosition;
use pdfcer_core::text_edit::{
    BlockAlignment, PageTemplate, PlaceTextError, PlaceTextReport, Unmappable,
};
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::writer::SaveOptions;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf")
}

fn session() -> EditSession {
    EditSession::new(Document::load(&fixture()).expect("fixture loads"))
}

/// `n` distinct words, so a missing one is identifiable rather than merely
/// countable. `w0 w1 w2 …` — short enough that many fit a line, distinct
/// enough that "every word arrived" is a real assertion.
fn words(n: usize) -> String {
    (0..n)
        .map(|i| format!("w{i}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The extracted plain text of every page of `bytes`, concatenated.
fn all_text(bytes: &[u8]) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).expect("output reloads");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    let mut out = String::new();
    for (i, page) in pages.iter().enumerate() {
        let text =
            text_extract::extract_page(&doc, page, i, &ExtractOptions::default()).expect("extract");
        out.push_str(&text.plain_text());
        out.push('\n');
    }
    out
}

fn saved(session: &EditSession) -> Vec<u8> {
    session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0
}

/// A template whose pagination is easy to reason about by hand: a 200 pt tall
/// column at 10 pt/12 pt leading fits a known small number of lines, so a test
/// can assert a page COUNT rather than "more than one".
fn small_template() -> PageTemplate {
    PageTemplate::new()
        .with_media_box(page_tree::Rect::from_corners(0.0, 0.0, 300.0, 300.0))
        .with_margins(50.0, 50.0, 50.0, 50.0)
        .with_font(Std14::Helvetica)
        .with_size(10.0)
        .with_leading(Some(12.0))
}

#[test]
fn an_import_creates_the_pages_its_text_needs() {
    let template = small_template();
    // A 200 pt column (y 50..250) at 12 pt leading and 10 pt text: the first
    // baseline is 250 - 0.75*10 = 242.5, line i's bottom is
    // 242.5 - 12i - 0.25*10 = 240 - 12i, and that stays >= 50 while
    // i <= 15.83 — so i runs 0..=15 and SIXTEEN lines fit.
    let mut s = session();
    let report = s
        .place_text(&words(400), &template, InsertPosition::End)
        .expect("import succeeds");

    assert_eq!(report.lines_per_page, 16, "column arithmetic");
    assert!(
        report.pages_created > 1,
        "400 words at 10 pt in a 200 x 200 pt column cannot be one page — this is the exact \
         R76 collapse the request was filed about, got {report:?}"
    );
    // The pages exist in the document, not just in the report.
    let pages = s.pages().expect("page tree").len();
    assert_eq!(pages, 1 + report.pages_created);
    assert_eq!(
        report.first_page_index, 1,
        "appended after the fixture page"
    );
    assert_eq!(
        report.lines_placed,
        report.pages_created * 16 - spare(&report)
    );
}

/// The last page is normally short; `lines_placed` is therefore
/// `pages * per_page` minus whatever the last page did not use.
fn spare(report: &PlaceTextReport) -> usize {
    report.pages_created * report.lines_per_page - report.lines_placed
}

#[test]
fn every_word_of_the_input_reaches_the_document() {
    let template = small_template();
    let text = words(400);
    let mut s = session();
    let report = s
        .place_text(&text, &template, InsertPosition::End)
        .expect("import succeeds");
    assert!(
        report.pages_created >= 3,
        "needs a multi-page case to matter"
    );

    let extracted = all_text(&saved(&s));
    for word in text.split(' ') {
        assert!(
            extracted.contains(word),
            "{word:?} did not reach the document — a word placed off the sheet, or a page \
             skipped. {report:?}"
        );
    }
    // And nothing was placed twice: the count of `w399` occurrences is one.
    assert_eq!(extracted.matches("w399").count(), 1);
}

#[test]
fn no_placed_line_falls_outside_the_column_it_was_paginated_into() {
    // The self-check that makes the whole design safe: `placetext` decides how
    // many lines fit and `addtext` decides where they go, and they are separate
    // arithmetic. Run over several sizes/leadings so an off-by-one that only
    // shows at one line height cannot hide.
    for (size, leading) in [(8.0, 9.6), (10.0, 12.0), (12.0, 14.4), (18.0, 21.0)] {
        let template = small_template().with_size(size).with_leading(Some(leading));
        let mut s = session();
        let report = s
            .place_text(&words(300), &template, InsertPosition::End)
            .expect("import succeeds");
        assert_eq!(
            report.box_overflow_lines, 0,
            "at {size} pt / {leading} pt leading the pagination and the placement disagree"
        );
    }
}

#[test]
fn the_whole_import_is_one_undo_entry() {
    let template = small_template();
    let mut s = session();
    let report = s
        .place_text(&words(400), &template, InsertPosition::End)
        .expect("import succeeds");
    assert!(report.pages_created > 1);
    assert!(
        report.coalesced,
        "the fold must succeed for a normal import"
    );
    assert_eq!(report.undo_entries, 1);

    // ONE undo takes the whole import off — not one page of it.
    assert!(s.undo().is_some());
    assert_eq!(s.pages().expect("page tree").len(), 1);
    assert!(
        s.dirty_set().is_empty(),
        "undoing the import must leave the document as it was found"
    );
}

#[test]
fn an_unencodable_character_refuses_the_whole_import_and_names_every_one() {
    let template = small_template();
    let mut s = session();
    let before = s.pages().expect("page tree").len();

    // Two distinct un-encodable characters, one of them twice: the refusal has
    // to name BOTH, because fixing a text file one refusal at a time is a loop.
    let err = s
        .place_text(
            "alpha \u{3b1} beta \u{4e2d} gamma \u{3b1}",
            &template,
            InsertPosition::End,
        )
        .expect_err("must refuse");

    match err {
        PlaceTextError::Unmappable {
            total,
            ref chars,
            ref listing,
            ..
        } => {
            assert_eq!(total, 3);
            assert_eq!(
                chars.len(),
                2,
                "both distinct characters, not just the first"
            );
            assert!(chars.contains(&('\u{3b1}', 2)));
            assert!(chars.contains(&('\u{4e2d}', 1)));
            assert!(listing.contains("U+03B1"), "listing was {listing:?}");
            assert!(listing.contains("U+4E2D"), "listing was {listing:?}");
        }
        other => panic!("wrong refusal: {other}"),
    }

    // The refusal happened before anything existed.
    assert_eq!(s.pages().expect("page tree").len(), before);
    assert!(
        s.dirty_set().is_empty(),
        "a refusal must not touch the session"
    );
    assert!(s.undo().is_none());
}

#[test]
fn dropping_unencodable_characters_is_opt_in_and_says_exactly_what_was_lost() {
    let template = small_template().with_unmappable(Unmappable::Drop);
    let mut s = session();
    let report = s
        .place_text("alpha \u{3b1} beta", &template, InsertPosition::End)
        .expect("drop policy places the rest");

    assert_eq!(report.chars_dropped_unmappable, 1);
    assert_eq!(report.dropped_unmappable_chars, vec![('\u{3b1}', 1)]);
    assert!(
        report
            .disclosures
            .iter()
            .any(|d| d.contains("DROPPED") && d.contains("U+03B1")),
        "the loss must be disclosed by character, not only counted: {:?}",
        report.disclosures
    );
    let extracted = all_text(&saved(&s));
    assert!(extracted.contains("alpha"));
    assert!(extracted.contains("beta"));
}

#[test]
fn every_input_character_is_accounted_for() {
    // Every scalar of the input must land in exactly one disclosed bucket.
    // Without this the counts are decoration: a report can say "1,000
    // characters placed" about a 2,000-character file and nothing objects.
    let text = "\u{feff}Hello\tworld\r\n\r\nsecond \u{7}paragraph\u{c}third \u{3b1} page\n";
    let template = small_template().with_unmappable(Unmappable::Drop);
    let mut s = session();
    let report = s
        .place_text(text, &template, InsertPosition::End)
        .expect("import succeeds");

    assert_eq!(
        report.chars_input,
        report.chars_placed
            + report.whitespace_normalised
            + report.chars_dropped_control
            + report.chars_dropped_unmappable
            + usize::from(report.bom_stripped),
        "the buckets must sum to the input: {report:?}"
    );
    assert!(report.bom_stripped);
    assert_eq!(report.tabs_collapsed, 1);
    assert_eq!(report.chars_dropped_control, 1, "the U+0007");
    assert_eq!(report.chars_dropped_unmappable, 1, "the alpha");
    assert_eq!(report.explicit_page_breaks, 1);
    assert_eq!(report.crlf_normalised, 2);
    // Each of those five decisions is stated, not merely counted.
    let joined = report.disclosures.join("\n");
    for needle in [
        "byte-order mark",
        "carriage return",
        "tab(s) were collapsed",
        "form feed",
        "control character",
    ] {
        assert!(joined.contains(needle), "no disclosure mentions {needle:?}");
    }
}

#[test]
fn a_form_feed_starts_a_new_page() {
    // `export_text` writes U+000C between pages, so this is the one property
    // that makes export-then-import a round trip rather than a coincidence.
    let template = small_template();
    let mut s = session();
    let report = s
        .place_text("front\u{c}back", &template, InsertPosition::End)
        .expect("import succeeds");
    assert_eq!(report.pages_created, 2);

    let doc = Document::from_bytes(saved(&s)).expect("reload");
    let pages = page_tree::pages(&doc).expect("pages");
    let text_of = |i: usize| {
        text_extract::extract_page(&doc, &pages[i], i, &ExtractOptions::default())
            .expect("extract")
            .plain_text()
    };
    assert!(text_of(1).contains("front"));
    assert!(!text_of(1).contains("back"), "the break must separate them");
    assert!(text_of(2).contains("back"));
}

#[test]
fn an_empty_or_whitespace_only_import_is_refused_by_name() {
    let template = small_template();
    let mut s = session();
    assert!(matches!(
        s.place_text("", &template, InsertPosition::End),
        Err(PlaceTextError::EmptyText)
    ));
    assert!(matches!(
        s.place_text("  \n\n\t \n", &template, InsertPosition::End),
        Err(PlaceTextError::NoWordsToPlace)
    ));
    assert_eq!(s.pages().expect("page tree").len(), 1);
}

#[test]
fn an_import_appends_and_leaves_the_existing_page_alone() {
    let template = small_template();
    let mut s = session();
    let before = s.pages().expect("pages")[0].id;

    let report = s
        .place_text(&words(50), &template, InsertPosition::Start)
        .expect("import succeeds");

    assert_eq!(report.first_page_index, 0, "Start puts them first");
    let after = s.pages().expect("pages");
    assert_eq!(after.len(), 1 + report.pages_created);
    assert_eq!(
        after[report.pages_created].id, before,
        "the document's own page must survive, in order, unchanged"
    );
}

#[test]
fn a_run_of_blank_lines_keeps_its_pages_rather_than_being_swallowed() {
    // 16 lines per page. 20 blank lines then a word: the blank lines must
    // consume a whole page, and that page must exist and be disclosed rather
    // than tidied away — the document has to have the input's line structure.
    let template = small_template();
    let mut s = session();
    let text = format!("{}word", "\n".repeat(20));
    let report = s
        .place_text(&text, &template, InsertPosition::End)
        .expect("import succeeds");

    assert_eq!(report.pages_created, 2);
    assert_eq!(report.blank_pages, 1);
    assert!(
        report.disclosures.iter().any(|d| d.contains("no text")),
        "a blank page must be disclosed: {:?}",
        report.disclosures
    );
}

#[test]
fn a_justified_import_discloses_the_paragraphs_it_had_to_cut() {
    // Justification is the one place the "re-wrap each page independently"
    // design is not identity: a paragraph cut across a page break becomes two,
    // and a paragraph's last line is never stretched. That is disclosed.
    let template = small_template().with_alignment(BlockAlignment::Justified);
    let mut s = session();
    let report = s
        .place_text(&words(400), &template, InsertPosition::End)
        .expect("import succeeds");

    assert!(report.paragraphs_split_across_pages > 0);
    assert!(
        report.disclosures.iter().any(|d| d.contains("FLUSH LEFT")),
        "justification's page-boundary artefact must be stated: {:?}",
        report.disclosures
    );
}

#[test]
fn a_justified_line_is_actually_justified_in_the_content_stream() {
    // ★ The test that pins the design, and the only one that does.
    //
    // Each page is handed back the WORDS of its share and `add_text` re-derives
    // the line breaks. That works because greedy first-fit is prefix-stable —
    // but only if consecutive lines of one paragraph are rejoined with a SPACE.
    // Rejoining them with a newline instead produces a document that is
    // pixel-identical under left/centre/right alignment, so every other test
    // here stays green: the words are all present, at the right widths, on the
    // right pages, inside the column.
    //
    // The one place the difference surfaces is justification. §4.1 never
    // stretches a paragraph's LAST line, so if every wrapped line arrives as
    // its own paragraph, every line is a last line and nothing is ever
    // justified — no `[ … ] TJ` slack is emitted anywhere. Found by
    // deliberately breaking the rejoin and watching the whole suite pass.
    //
    // The added content streams are new, uncompressed bytes appended by the
    // incremental save, so searching the saved buffer for the operator is
    // enough — nothing else in this fixture emits one.
    let template = small_template().with_alignment(BlockAlignment::Justified);
    let mut s = session();
    let report = s
        .place_text(&words(400), &template, InsertPosition::End)
        .expect("import succeeds");
    assert!(report.pages_created > 1);

    let bytes = saved(&s);
    assert!(
        bytes.windows(4).any(|w| w == b"] TJ"),
        "a justified import emitted no `[ … ] TJ` slack line — every wrapped line reached the \
         emitter as its own paragraph, so nothing was stretched"
    );
}

#[test]
fn place_text_covers_everything_the_boxed_add_would_say() {
    // `place_text` drops the per-page `"boxed add: …"` disclosures, because
    // their numbers are per-page and read as contradicting the import's own
    // totals. That is only safe while this report says each of those things
    // ITSELF — so this drives the conditions that produce every member of the
    // family and checks the import discloses each one.
    let template = small_template();
    let mut s = session();
    // `supercalifragilistic…` at 10 pt is far wider than the 200 pt column:
    // the overlong-word condition, which `addtext` would report per page.
    let report = s
        .place_text(
            &format!("{} {}", "x".repeat(120), words(40)),
            &template,
            InsertPosition::End,
        )
        .expect("import succeeds");

    assert!(
        report
            .disclosures
            .iter()
            .all(|d| !d.starts_with("boxed add: ")),
        "the per-page recap must not reach the operator: {:?}",
        report.disclosures
    );
    // …and the thing it would have said is said here instead, once.
    assert_eq!(report.overlong_words, 1);
    assert_eq!(
        report
            .disclosures
            .iter()
            .filter(|d| d.contains("does not hyphenate"))
            .count(),
        1,
        "the overlong word must be disclosed exactly once: {:?}",
        report.disclosures
    );
    // The remaining members of the family are overflow, and the import's own
    // fields carry those — asserted to be absent, which is the whole point of
    // paginating.
    assert_eq!(report.box_overflow_lines, 0);
}

#[test]
fn a_column_too_short_for_one_line_is_refused_by_name() {
    let template = PageTemplate::new()
        .with_media_box(page_tree::Rect::from_corners(0.0, 0.0, 300.0, 120.0))
        .with_margins(50.0, 50.0, 55.0, 55.0)
        .with_size(24.0);
    let mut s = session();
    assert!(matches!(
        s.place_text("hello", &template, InsertPosition::End),
        Err(PlaceTextError::PageTooShort { .. })
    ));
}

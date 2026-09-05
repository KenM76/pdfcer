//! # `Pass 139.0` / `139.1` — the writing direction, and segmentation in the
//! line's own frame
//!
//! ## What was wrong
//!
//! [`ExtractedGlyph`](pdfcer_core::text_extract::ExtractedGlyph) published the
//! advance and the effective font size as **magnitudes** — the lengths of the
//! two transformed basis vectors of §9.4.4's text rendering matrix — and
//! discarded their directions. Everything downstream then had no choice but
//! to assume the text ran along the page's `+x` axis.
//!
//! That assumption is true of virtually every word-processor page and
//! **false of every CAD title block**, which stamps its source path with
//! `Tm = [0 1 -1 0 e f]`. Every derived-layout threshold in
//! `text_extract::layout` was stated in page axes, so a rotated line failed
//! them in two independent ways:
//!
//! | text | what broke | which clause |
//! |---|---|---|
//! | 90° / 270° | one advance lands entirely in `Δy`, which exceeds `line_gap_ratio × size` for any glyph wider than a third of an em | the baseline clause |
//! | 180° | the step is in `−x` while `advance` is a positive magnitude, so `Δx − advance ≈ −2·advance` | the backward-jump clause |
//!
//! Either way the verdict was a derived line break **between every letter**.
//!
//! ## The measurement this file pins
//!
//! On `text/rotated-text.pdf`: **22 derived line breaks before, 3 after**,
//! for four lines of text. The three rotated strings pasted into a text
//! editor as one character per line; Acrobat gives one line each.
//!
//! On the real file that prompted the work — a SOLIDWORKS drawing set whose
//! title block carries the source path stamped vertically — it was **82
//! glyphs in 72 runs separated by 71 derived line breaks**.
//!
//! ## Why a separate fixture and not an existing one
//!
//! `tools/gen-rotated-text-fixtures.py` byte-authors four `BT`…`ET` blocks
//! at 0°, 90°, 180° and 270°, **in capitals**. The capitals are the point:
//! at 12 pt Helvetica every capital's advance exceeds
//! `line_gap_ratio × size = 3.6 pt`, so under the old rule *no run held two
//! glyphs at all*. A fixture of narrow lowercase letters hides that — `i`,
//! `n`, `c` and the space are narrow enough for consecutive pairs to
//! survive, and the output then looks merely imperfect rather than totally
//! fragmented.
//!
//! The **horizontal block is a control, not decoration.** A change that
//! "fixed" rotation by loosening a threshold, or by inheriting the previous
//! glyph's direction, would pass every rotated assertion here and break
//! that one.
//!
//! ## What is NOT tested here
//!
//! §9.7.4.3 **vertical writing mode** (`/WMode 1`) is a different feature
//! with different metrics and is not implemented. Everything below is
//! ordinary horizontal-mode text placed by a rotated matrix.

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::text_extract::{self, ExtractOptions, ExtractedText};

/// A fixture path under `fixtures/synthetic/text/`.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name)
}

/// Load and extract with default options.
///
/// Deliberately the **defaults**: the direction must be published without
/// the caller opting into anything. `capture_provenance` also carries the
/// text matrix and the CTM, from which the direction is derivable — but it
/// is off by default and costs an owned font name and an `Arc` clone per
/// glyph, which is the wrong price for a reader that only wants to know
/// which way a line runs.
fn extract(name: &str) -> ExtractedText {
    let doc = Document::load(&fixture(name)).expect("fixture loads");
    text_extract::extract_document(&doc, &ExtractOptions::default()).expect("extraction runs")
}

/// A page-space point INSIDE one glyph's ink, deliberately off its
/// along-midpoint.
///
/// A quarter of an advance along the writing direction and a quarter of an em
/// up from the baseline. The offset from the midpoint is not fussiness: the
/// cell's geometric CENTRE projects to exactly `advance / 2`, which is the
/// knife edge where `hit_in_line` decides leading-versus-trailing, and float
/// noise then flips individual probes. That would make the test measure the
/// probe rather than the code.
fn probe(g: &pdfcer_core::text_extract::ExtractedGlyph) -> (f64, f64) {
    let (dx, dy) = g.direction;
    let (ux, uy) = g.up();
    let along = g.advance * 0.25;
    let across = g.size * 0.25;
    (
        f64::from(g.x + along * dx + across * ux),
        f64::from(g.y + along * dy + across * uy),
    )
}

/// The headline: four rotated blocks come out as four lines, not as 32.
#[test]
fn rotated_text_is_four_lines_not_one_line_per_letter() {
    let text = extract("rotated-text.pdf");
    assert_eq!(
        text.plain_text(),
        "HORIZONTAL\nUPWARD\nINVERTED\nDOWNWARD",
        "each block is ONE line; the breaks between them are the direction \
         changes, not the letters"
    );
    assert_eq!(
        text.diagnostics.lines_derived, 3,
        "three block boundaries. This was 22 before Pass 139.1 — the extra 19 \
         were one break between every letter of the three rotated blocks"
    );
    assert_eq!(
        text.diagnostics.spaces_derived, 0,
        "no gap in this fixture is wide enough for a derived space, in any frame"
    );
}

/// Each block publishes its own writing direction, exactly.
///
/// The four are exact in floating point — the `Tm` entries are literal `0`,
/// `1` and `-1` — so this asserts equality rather than a tolerance. A
/// normalisation that introduced an error would show up here rather than as
/// a slow drift in some consumer's caret.
#[test]
fn each_block_publishes_its_writing_direction() {
    let text = extract("rotated-text.pdf");
    let sourced: Vec<_> = text.pages[0]
        .runs
        .iter()
        .filter(|r| r.is_sourced())
        .collect();
    assert_eq!(sourced.len(), 4, "one run per block");
    let seen: Vec<(&str, (f32, f32))> = sourced
        .iter()
        .map(|r| (r.text.as_str(), r.direction()))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("HORIZONTAL", (1.0, 0.0)),
            ("UPWARD", (0.0, 1.0)),
            ("INVERTED", (-1.0, 0.0)),
            ("DOWNWARD", (0.0, -1.0)),
        ]
    );
}

/// Ordinary horizontal text still reports the page x axis — **the control**.
///
/// Without this, a change that defaulted the direction to whatever the
/// previous glyph had, or that loosened a threshold instead of rotating the
/// frame, would satisfy every rotated assertion in this file.
#[test]
fn horizontal_text_still_reports_the_page_x_axis() {
    let text = extract("simple-winansi.pdf");
    let mut checked = 0_u32;
    for page in &text.pages {
        for run in page.runs.iter().filter(|r| r.is_sourced()) {
            assert_eq!(
                run.direction(),
                (1.0, 0.0),
                "run {:?} is ordinary horizontal text",
                run.text
            );
            for g in &run.glyphs {
                assert_eq!(g.direction, (1.0, 0.0));
                checked += 1;
            }
        }
    }
    assert!(
        checked > 0,
        "the control must actually have examined glyphs"
    );
}

/// Every glyph in one run shares that run's direction.
///
/// This is the guarantee that makes
/// [`TextRun::direction`](pdfcer_core::text_extract::TextRun::direction)
/// answerable from the first glyph alone rather than by a
/// scan-and-reconcile. It holds because `layout::classify` treats a
/// direction change as a line break — so a regression that removes that
/// rule surfaces here, and not as a subtly wrong caret three Passes later.
#[test]
fn a_run_never_mixes_two_directions() {
    let text = extract("rotated-text.pdf");
    for page in &text.pages {
        for run in page.runs.iter().filter(|r| r.is_sourced()) {
            let want = run.direction();
            for g in &run.glyphs {
                assert_eq!(g.direction, want, "run {:?} mixes directions", run.text);
            }
        }
    }
}

/// A rotated glyph's cell is taken in the glyph's own frame, not the page's.
///
/// The 90° case is the one that was visibly and measurably wrong. The old
/// page-axis expression put the cell to the *right* of the origin and one em
/// *above* it, where the ink is *above* the origin and offset to the *left*.
/// It overlapped its own glyph by roughly a third and was hung off the wrong
/// corner — which is exactly why a click in the middle of a rotated letter
/// landed outside every line box and the nearest-line fallback fired.
#[test]
fn a_rotated_glyph_cell_encloses_its_own_ink() {
    let text = extract("rotated-text.pdf");
    let upward = text.pages[0]
        .runs
        .iter()
        .find(|r| r.text == "UPWARD")
        .expect("the 90 degree block");
    let g = &upward.glyphs[0];
    assert_eq!(g.direction, (0.0, 1.0));
    // The `U` is placed at (100, 300) and advances upward.
    assert!((g.x - 100.0).abs() < 1e-3, "x = {}", g.x);
    assert!((g.y - 300.0).abs() < 1e-3, "y = {}", g.y);

    let cell = g.cell();
    // Along the writing direction (+y): from the baseline origin up to one
    // advance further on.
    assert!(cell.lly <= 300.0 + 1e-6, "lly = {}", cell.lly);
    assert!(
        cell.ury >= 300.0 + f64::from(g.advance) - 1e-6,
        "ury = {} must reach the next glyph's origin",
        cell.ury
    );
    // Across it: a quarter turn counter-clockwise from +y is −x, so the
    // ascender extends to the LEFT of the origin and the descender to the
    // right. The old expression had both on the wrong axis entirely.
    assert!(
        cell.llx < 100.0 - f64::from(g.size) * 0.7,
        "the ascender must extend to the left; llx = {}",
        cell.llx
    );
    assert!(
        cell.urx > 100.0,
        "the descender must extend to the right; urx = {}",
        cell.urx
    );

    // And the run's own bbox must contain every glyph cell it was built
    // from — the property the axis-aligned version silently violated.
    let bbox = upward.bbox.expect("a glyph run has geometry");
    for g in &upward.glyphs {
        let c = g.cell();
        assert!(
            bbox.llx <= c.llx + 1e-6
                && bbox.lly <= c.lly + 1e-6
                && bbox.urx >= c.urx - 1e-6
                && bbox.ury >= c.ury - 1e-6,
            "run bbox {bbox:?} does not contain glyph cell {c:?}"
        );
    }
}

/// A horizontal glyph's cell is byte-for-byte what the old expression gave.
///
/// `glyph_cell` replaced four hand-written copies of
/// `min(x, x + advance) … y − 0.25·size … y + 0.75·size`. The generalisation
/// must **reduce** to that for `direction = (1, 0)`, not merely approximate
/// it — otherwise every existing bbox in every existing document shifts, and
/// the change stops being safe.
#[test]
fn a_horizontal_glyph_cell_reduces_to_the_old_expression() {
    let text = extract("simple-winansi.pdf");
    let mut checked = 0_u32;
    for page in &text.pages {
        for run in page.runs.iter().filter(|r| r.is_sourced()) {
            for g in &run.glyphs {
                let c = g.cell();
                assert_eq!(c.llx, f64::from(g.x.min(g.x + g.advance)));
                assert_eq!(c.urx, f64::from(g.x.max(g.x + g.advance)));
                assert_eq!(c.lly, f64::from(g.y - g.size * 0.25));
                assert_eq!(c.ury, f64::from(g.y + g.size * 0.75));
                checked += 1;
            }
        }
    }
    assert!(checked > 0);
}

/// `advance_end` is the next glyph's origin, whatever the direction.
///
/// Pins the replacement for `x + advance`, which is correct only for a
/// horizontal run. Checked on the 180° block as well as the 90° one, because
/// those two failed the old code through *different* clauses and a fix for
/// one need not have fixed the other.
#[test]
fn advance_end_lands_on_the_next_glyph_origin() {
    let text = extract("rotated-text.pdf");
    for name in ["HORIZONTAL", "UPWARD", "INVERTED", "DOWNWARD"] {
        let run = text.pages[0]
            .runs
            .iter()
            .find(|r| r.text == name)
            .unwrap_or_else(|| panic!("block {name}"));
        assert!(
            run.glyphs.len() >= 2,
            "{name} must have glyphs to step over"
        );
        for pair in run.glyphs.windows(2) {
            let (end_x, end_y) = pair[0].advance_end();
            assert!(
                (end_x - pair[1].x).abs() < 1e-3 && (end_y - pair[1].y).abs() < 1e-3,
                "{name}: advance_end ({end_x}, {end_y}) != next origin ({}, {})",
                pair[1].x,
                pair[1].y
            );
        }
    }
}

/// `up` is a quarter turn **counter-clockwise** from `direction`, always.
///
/// One line of arithmetic, published rather than left to each caller,
/// because handedness is the one thing a consumer drawing a caret or a
/// `/QuadPoints` array gets backwards — and gets backwards *silently* on any
/// page whose text happens to be symmetric about its baseline.
#[test]
fn up_is_a_quarter_turn_counter_clockwise_from_the_direction() {
    let text = extract("rotated-text.pdf");
    let mut checked = 0_u32;
    for page in &text.pages {
        for run in page.runs.iter().filter(|r| r.is_sourced()) {
            for g in &run.glyphs {
                let (dx, dy) = g.direction;
                let (ux, uy) = g.up();
                assert!(
                    (dx * ux + dy * uy).abs() < 1e-6,
                    "up must be perpendicular to the direction"
                );
                assert!(
                    (dx * uy - dy * ux - 1.0).abs() < 1e-6,
                    "and counter-clockwise, not clockwise"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0);
}

/// **Two abutting runs of different directions do not merge.**
///
/// # ★ Why this fixture exists at all, and what it cost to find out
///
/// `rotated-text.pdf` does **not cover** the direction-change rule, and a
/// sabotage run is what proved it: deleting that rule from
/// `layout::classify` outright left all nine of this file's other tests
/// green. Its four blocks sit far apart on the page, so the
/// perpendicular-displacement clause separates them regardless of what the
/// direction rule does. Those tests *exercised* the rule; none of them
/// *covered* it — the failure mode this project has now caught repeatedly,
/// and which only ever shows up under deliberate sabotage.
///
/// `rotated-text-abutting.pdf` removes every other signal. `AB` is set at
/// 12 pt Helvetica from `(200, 500)`; both capitals are 667/1000 em, so the
/// pen finishes at `216.008`, and the vertical run begins at exactly that
/// point. Between the `B` and the `C` the perpendicular displacement is
/// zero and the forward gap is zero. Every geometric clause says "adjacent".
///
/// Only the direction rule separates them. Without it the two merge into
/// one run holding glyphs that do not share a direction — which is exactly
/// the guarantee
/// [`TextRun::direction`](pdfcer_core::text_extract::TextRun::direction) is
/// published on, silently broken.
#[test]
fn abutting_runs_of_different_directions_do_not_merge() {
    let text = extract("rotated-text-abutting.pdf");
    let sourced: Vec<_> = text.pages[0]
        .runs
        .iter()
        .filter(|r| r.is_sourced())
        .collect();
    assert_eq!(
        sourced.len(),
        2,
        "the horizontal and the vertical run must stay separate even though \
         nothing but their direction distinguishes them; got {:?}",
        sourced.iter().map(|r| &r.text).collect::<Vec<_>>()
    );
    assert_eq!(sourced[0].text, "AB");
    assert_eq!(sourced[0].direction(), (1.0, 0.0));
    assert_eq!(sourced[1].text, "CD");
    assert_eq!(sourced[1].direction(), (0.0, 1.0));
    assert_eq!(
        text.diagnostics.lines_derived, 1,
        "the direction change is the one and only break on this page"
    );

    // The abutment really is exact — if it were not, the perpendicular or
    // gap clause would be doing the separating and this test would be
    // covering the wrong thing. Asserting it here means a future edit to
    // the fixture generator cannot quietly turn this back into a
    // geometric test.
    let (end_x, end_y) = sourced[0]
        .glyphs
        .last()
        .expect("AB has glyphs")
        .advance_end();
    let next = &sourced[1].glyphs[0];
    assert!(
        (end_x - next.x).abs() < 1e-2 && (end_y - next.y).abs() < 1e-2,
        "the two runs must abut to within a hundredth of a point, or this \
         test is not testing the direction rule: end ({end_x}, {end_y}) vs \
         start ({}, {})",
        next.x,
        next.y
    );
}

/// **A vertical word gap and a vertical column break, both of which a
/// page-axis reader gets quietly wrong.**
///
/// # ★ Why a third fixture, and what sabotage found
///
/// The other two fixtures cover the frame-aware **cursor** and the
/// **direction-change** rule. Neither covers the two dot products in
/// `classify`, because in both of them the glyphs abut exactly — `dy` is
/// zero within a run, so the correct `perp = d × dir` and the old page-axis
/// `−Δy` both come out zero and agree. Reverting `perp`/`gap` to page axes
/// while leaving the cursor alone left every test green.
///
/// They only disagree where there is a real gap. Both cases are in
/// `rotated-text-columns.pdf`, at 12 pt Helvetica (word threshold 2.4 pt,
/// line threshold 3.6 pt):
///
/// | what | correct | page-axis reader |
/// |---|---|---|
/// | 3.332 pt gap **along** a vertical baseline | derived word space | `gap = Δx = 0` — **nothing**, the two words run together |
/// | a second column 30 pt across, aligned so `Δy = 0` | derived line break (`perp = 30`) | `perp = 0`, then `gap = 30` — a **word space**, joining two columns |
///
/// ★ **Both wrong answers are quiet.** Neither produces the
/// one-letter-per-line fragmentation that made the original defect
/// visible — a reader would have to know what the file said to notice. That
/// is what makes this fixture worth authoring rather than trusting the two
/// loud ones.
#[test]
fn a_vertical_word_gap_and_a_second_column_are_read_in_the_frame() {
    let text = extract("rotated-text-columns.pdf");
    assert_eq!(
        text.plain_text(),
        "UP ON\nSIDE",
        "a page-axis reader gives \"UPON SIDE\": the vertical word gap \
         vanishes and the second column joins the first"
    );
    assert_eq!(
        text.diagnostics.spaces_derived, 1,
        "the 3.332 pt gap ALONG the vertical baseline is a word space"
    );
    assert_eq!(
        text.diagnostics.lines_derived, 1,
        "the 30 pt displacement ACROSS it is a line break, not a second space"
    );

    // Every glyph on this page runs the same way, so nothing here is being
    // separated by the direction rule — the two clauses under test are the
    // only thing doing the work.
    for run in text.pages[0].runs.iter().filter(|r| r.is_sourced()) {
        assert_eq!(
            run.direction(),
            (0.0, 1.0),
            "run {:?} — this fixture is single-direction on purpose",
            run.text
        );
    }
}

/// **A sweep down a rotated string selects every letter, not five of six.**
///
/// # `Pass 139.2` — the consequence the request actually measured
///
/// [`EditableTextModel::hit_test`](pdfcer_core::text_edit::EditableTextModel::hit_test)
/// resolves a click to a caret by finding the containing line box and then
/// the containing glyph within it. Both halves assumed the page x axis:
///
/// * the line box was the union of glyph cells computed as
///   `min(x, x + advance)` — for a 90° glyph, a box overlapping its own ink
///   by about a third and hung off the wrong corner, so **a press on the
///   middle of a rotated letter was outside every line box** and the
///   nearest-line fallback decided;
/// * within a line, each glyph's extent was `g.x .. g.x + g.advance` — and
///   every glyph of a 90° line has the **same** `x`, so the first one
///   "contained" every click and the caret never moved.
///
/// The consuming shell drove both before the fix: a sweep down a
/// six-letter 90° string selected **five**, and a sweep along an
/// eight-letter 180° string selected **nothing at all**, because both ends
/// collapsed onto one caret slot.
///
/// This test walks the caret slot by slot down `UPWARD` and asserts a
/// distinct, monotonically advancing offset at each — which is the
/// property both failures broke.
#[test]
fn a_sweep_down_a_rotated_line_visits_every_letter() {
    use pdfcer_core::text_edit::{BlockRecognitionOptions, EditableTextModel};

    let text = extract("rotated-text.pdf");
    let page = &text.pages[0];
    let model = EditableTextModel::recognize(page, &BlockRecognitionOptions::default());

    let run_index = page
        .runs
        .iter()
        .position(|r| r.text == "UPWARD")
        .expect("the 90 degree block");
    let glyphs = &page.runs[run_index].glyphs;
    assert_eq!(glyphs.len(), 6, "U P W A R D");

    // Probe INSIDE each glyph's ink, deliberately off the along-midpoint.
    // Under the old page-axis boxes these points were outside every line
    // box; the quarter-advance offset avoids the knife edge where
    // leading-vs-trailing is decided by float noise, which is a property of
    // the probe and not of the code under test.
    let mut offsets = Vec::new();
    for g in glyphs {
        let (px, py) = probe(g);
        let pos = model
            .hit_test(px, py)
            .expect("a click on a glyph must resolve to a caret");
        assert_eq!(
            pos.run, run_index,
            "the caret must land in the rotated run, not on some other line"
        );
        offsets.push(pos.byte_offset);
    }

    // Every probe lands on a different slot, and they advance in reading
    // order down the string. A degenerate hit test — the old one — returns
    // the same offset six times.
    assert_eq!(
        offsets.len(),
        offsets
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        "six probes must give six distinct caret slots, got {offsets:?}"
    );
    assert!(
        offsets.windows(2).all(|w| w[0] < w[1]),
        "and they must advance along the writing direction, got {offsets:?}"
    );
}

/// The same sweep on the 180° line, which failed through the other clause.
///
/// Kept separate from the 90° case rather than folded into a loop, because
/// the two broke through *different* code paths — the baseline clause and
/// the backward-jump clause — and a fix for one need not have fixed the
/// other. The shell measured this one selecting **nothing**.
#[test]
fn a_sweep_along_an_inverted_line_visits_every_letter() {
    use pdfcer_core::text_edit::{BlockRecognitionOptions, EditableTextModel};

    let text = extract("rotated-text.pdf");
    let page = &text.pages[0];
    let model = EditableTextModel::recognize(page, &BlockRecognitionOptions::default());

    let run_index = page
        .runs
        .iter()
        .position(|r| r.text == "INVERTED")
        .expect("the 180 degree block");
    let glyphs = &page.runs[run_index].glyphs;
    assert_eq!(glyphs.len(), 8);

    let mut offsets = Vec::new();
    for g in glyphs {
        let (px, py) = probe(g);
        let pos = model
            .hit_test(px, py)
            .expect("a click on a glyph must resolve to a caret");
        assert_eq!(pos.run, run_index);
        offsets.push(pos.byte_offset);
    }
    assert_eq!(
        offsets.len(),
        offsets
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        "eight probes must give eight distinct caret slots, got {offsets:?}"
    );
    assert!(
        offsets.windows(2).all(|w| w[0] < w[1]),
        "reading order runs right-to-left in PAGE space here, and the caret \
         must follow the TEXT's order, not the page's: {offsets:?}"
    );
}

/// `caret_point` names a real point on a rotated baseline; `caret_x` cannot.
///
/// The two are asserted together on purpose. `caret_x` returning the same
/// number for every slot on a vertical line is **not a bug** — it is the
/// honest answer a scalar can give — and pinning it here stops a later
/// session from "fixing" it into something that is neither an x nor a
/// point. The fix was to publish the pair, not to redefine the scalar.
#[test]
fn caret_point_moves_along_a_rotated_line_where_caret_x_cannot() {
    use pdfcer_core::text_edit::{BlockRecognitionOptions, EditableTextModel, TextPosition};

    let text = extract("rotated-text.pdf");
    let page = &text.pages[0];
    let model = EditableTextModel::recognize(page, &BlockRecognitionOptions::default());
    let run_index = page
        .runs
        .iter()
        .position(|r| r.text == "UPWARD")
        .expect("the 90 degree block");

    let mut points = Vec::new();
    let mut xs = Vec::new();
    for g in &page.runs[run_index].glyphs {
        let pos = TextPosition::new(run_index, g.text_start as usize);
        points.push(model.caret_point(pos).expect("a caret point"));
        xs.push(model.caret_x(pos).expect("a caret x"));
    }

    assert!(
        points.windows(2).all(|w| w[1].1 > w[0].1),
        "the caret must climb the page as the caret advances: {points:?}"
    );
    assert!(
        xs.windows(2).all(|w| (w[1] - w[0]).abs() < 1e-6),
        "and caret_x must be CONSTANT — a vertical line has one x, which is \
         exactly why the scalar cannot express this caret: {xs:?}"
    );
}

/// **A rotated line is ONE line in the editable model, not one per letter.**
///
/// # ★ The second copy of the same page-axis assumption
///
/// `Pass 139.1` fixed the *extraction*, so `PageText::runs` held one clean
/// run per rotated block. `EditableTextModel`'s Stage-1 clustering then
/// **re-fragmented it immediately**: its defensive within-line baseline
/// jump was also written in page axes, and on a 90° line every glyph
/// advances a whole advance in `y`. Measured on this fixture before
/// `Pass 139.2`: **16 lines for four lines of text.**
///
/// ★ It was found by **sabotage, not by a failing test.** With the runs
/// already correct upstream, the hit-test assertions below passed for the
/// wrong reason — each glyph had become its own line, so every probe
/// trivially found "its" line and resolved to the only slot in it. Reverting
/// the glyph-cell fix changed nothing, which is what gave it away.
///
/// This test asserts the line COUNT and the glyph counts within them, which
/// is the property neither the extraction tests nor the hit-test tests
/// constrain.
#[test]
fn the_editable_model_clusters_a_rotated_line_as_one_line() {
    use pdfcer_core::text_edit::{BlockRecognitionOptions, EditableTextModel};

    let text = extract("rotated-text.pdf");
    let model = EditableTextModel::recognize(&text.pages[0], &BlockRecognitionOptions::default());
    let shape: Vec<(usize, (f32, f32))> = model
        .lines()
        .iter()
        .map(|l| (l.glyphs.len(), l.direction))
        .collect();
    assert_eq!(
        shape,
        vec![
            (10, (1.0, 0.0)),
            (6, (0.0, 1.0)),
            (8, (-1.0, 0.0)),
            (8, (0.0, -1.0)),
        ],
        "four lines, one per block. Before Pass 139.2 this was SIXTEEN — the          90 and 270 degree blocks were split into one line per letter"
    );

    // ★ And every line's box must CONTAIN every glyph cell in it.
    //
    // Asserted here rather than left to `hit_test`, because sabotage showed
    // `hit_test` does not depend on it: when the box fails to contain the
    // point, the nearest-line-by-baseline fallback still finds the right
    // line — provided the box is wrong by less than one line-height, the
    // reach `Pass 14.5` gave that fallback (2026-09-05) — so a modestly
    // wrong box is invisible through that door. It is not
    // invisible to a shell that draws a selection highlight from
    // `Line::bbox`, which is what the box is for.
    for line in model.lines() {
        for &gref in &line.glyphs {
            let g = model.glyph(gref).expect("a clustered glyph resolves");
            let c = g.cell();
            assert!(
                line.bbox.llx <= c.llx + 1e-6
                    && line.bbox.lly <= c.lly + 1e-6
                    && line.bbox.urx >= c.urx - 1e-6
                    && line.bbox.ury >= c.ury - 1e-6,
                "line box {:?} (dir {:?}) does not contain glyph cell {c:?}",
                line.bbox,
                line.direction
            );
        }
    }
}

/// A derived-whitespace run answers `(1, 0)` rather than panicking.
///
/// It carries no glyphs, so there is no direction to report. The default is
/// the page x axis for the same reason the degenerate-matrix default is: a
/// caller iterating every run and orienting something must get a usable
/// vector, not a `NaN` and not a panic.
#[test]
fn a_run_with_no_glyphs_answers_the_page_x_axis() {
    let text = extract("rotated-text.pdf");
    let breaks: Vec<_> = text.pages[0]
        .runs
        .iter()
        .filter(|r| !r.is_sourced())
        .collect();
    assert_eq!(breaks.len(), 3, "the three derived line breaks");
    for r in breaks {
        assert!(r.glyphs.is_empty());
        assert_eq!(r.direction(), (1.0, 0.0));
    }
}

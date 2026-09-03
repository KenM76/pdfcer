//! # `pdfcer object-list` integration tests
//!
//! Black-box tests over the **real binary** for `object-list` — the
//! paint-order object inventory that is the discovery path for the
//! `--object` index `object-move` / `object-delete` / `node-move` consume,
//! and for the `--hit` headless hit-test query.
//!
//! ## What these tests are actually protecting
//!
//! Three distinct contracts, none of which is cosmetic:
//!
//! 1. **The index correspondence.** `object-list`'s `index=` and
//!    `object-move --object` must name the same object. They do because
//!    both come from one `pdfcer_core::vector::decompose_page` walk, but
//!    "because the implementation currently shares a function" is not a
//!    guarantee — [`listed_indices_are_the_ones_object_move_consumes`]
//!    pins it observably, by listing an object and then editing *that*
//!    index and checking exactly one object changed. If a future change
//!    ever inserted a filter on one side and not the other, this fails.
//!
//! 2. **The hit-test oracle.** `--hit` calls the same
//!    `pdfcer_core::vector::hit_test_point` the GUI's
//!    `ObjectModelProvider::hit_test` calls, which makes this subcommand
//!    the headless authority on GUI selection behaviour.
//!    [`a_click_on_a_stroked_line_selects_it`] is the regression for the
//!    concrete diagnosis this was built to settle: on
//!    `fixtures/synthetic/dimension/linear-base.pdf` — a page whose only
//!    content is one 1 pt horizontal stroked line, a *zero-height* bounding
//!    box — a click on the line hits at every sane tolerance, INCLUDING
//!    `--tolerance 0` (the stroke's own half-width carries it). A
//!    degenerate-bbox path being unhittable would be a real core bug and
//!    this is what would catch it.
//!
//! 3. **Clean refusals.** An out-of-range page, a `--page 0`, and a
//!    malformed `--hit` operand must each produce a named error and a
//!    non-zero exit, never a panic and never a confident wrong answer
//!    (rule 4: fuzzy, never sneaky).
//!
//! Fixtures used (provenance in each directory's `PROVENANCE.md`):
//! `fixtures/synthetic/vector/edit.pdf` (line / filled rectangle /
//! stroked triangle — three index-predictable path objects),
//! `fixtures/synthetic/text/simple-winansi.pdf` (one text object), and
//! `fixtures/synthetic/dimension/linear-base.pdf` (one stroked line).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
/// `exit::RUNTIME_ERROR` in the CLI's stable exit-code contract — what a
/// bad `--page` / `--hit` operand yields.
const RUNTIME_ERROR: i32 = 1;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_objlist_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn run(sub: &str, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg(sub)
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The `hit …` line's fields, for the tests that only care about the query
/// answer. Panics if the run did not emit one.
fn hit_line(out: &Output) -> String {
    stdout(out)
        .lines()
        .find(|l| l.starts_with("hit "))
        .unwrap_or_else(|| panic!("no `hit` line in output:\n{}", stdout(out)))
        .to_owned()
}

#[test]
fn lists_path_objects_in_paint_order_with_geometry() {
    let f = fixture("vector/edit.pdf");
    let out = run("object-list", &[f.to_str().unwrap(), "--page", "1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    let rows: Vec<&str> = text.lines().filter(|l| l.starts_with("object ")).collect();
    assert_eq!(rows.len(), 3, "three path objects: {text}");

    // Paint order: index 0 painted first, so indices ascend down the
    // listing and the LAST row is topmost.
    for (i, row) in rows.iter().enumerate() {
        assert!(row.contains(&format!("index={i}")), "row {i}: {row}");
        assert!(row.contains("kind=path"), "row {i}: {row}");
    }

    // Object 0 is the open stroked line: a stroke-only path with two
    // anchors and no closed subpath.
    assert!(
        rows[0].contains("bbox=50,50,150,150")
            && rows[0].contains("subpaths=1")
            && rows[0].contains("anchors=2")
            && rows[0].contains("closed=0")
            && rows[0].contains("paint=stroke"),
        "line: {}",
        rows[0]
    );
    // Object 1 is the `re` rectangle: filled (nonzero), four anchors, one
    // closed subpath. The `paint=` token is what explains a hit-test
    // result — a filled path is hit by its interior, a stroke-only path
    // only near its outline.
    assert!(
        rows[1].contains("bbox=200,50,280,110")
            && rows[1].contains("anchors=4")
            && rows[1].contains("closed=1")
            && rows[1].contains("paint=fill-nonzero"),
        "rectangle: {}",
        rows[1]
    );
    // Object 2 is the closed stroked triangle.
    assert!(
        rows[2].contains("anchors=3") && rows[2].contains("closed=1"),
        "triangle: {}",
        rows[2]
    );

    // The summary line tallies what was listed, and discloses whether the
    // decomposition dropped anything (a `MAX_OBJECTS`/`MAX_NODES` cap would
    // otherwise silently shift every index past the drop).
    assert!(
        text.contains("objects=3 paths=3 text=0 images=0 forms=0"),
        "summary: {text}"
    );
    assert!(
        text.contains("dropped_objects=0 dropped_nodes=0"),
        "summary: {text}"
    );
}

/// The whole reason this subcommand exists: the `index=` it prints IS the
/// `--object` the editing subcommands take.
///
/// Proven observably rather than by inspection — list the objects, then
/// move the LAST listed index and require exactly one object to change and
/// the edit to undo byte-identically. One past that index must be refused,
/// which pins the count as well as the base.
#[test]
fn listed_indices_are_the_ones_object_move_consumes() {
    let f = fixture("vector/edit.pdf");
    let listed = run("object-list", &[f.to_str().unwrap()]);
    assert!(listed.status.success(), "{}", stderr(&listed));
    let count = stdout(&listed)
        .lines()
        .filter(|l| l.starts_with("object "))
        .count();
    assert_eq!(count, 3);

    // The highest listed index is editable...
    let out_path = temp_path("corr");
    let moved = run(
        "object-move",
        &[
            f.to_str().unwrap(),
            "--object",
            &(count - 1).to_string(),
            "--dx=5",
            "--dy=5",
            "-o",
            out_path.to_str().unwrap(),
            "--verify-undo",
        ],
    );
    assert!(moved.status.success(), "{}", stderr(&moved));
    let text = stdout(&moved);
    assert!(text.contains("changed=1"), "{text}");
    assert!(text.contains("undo_identical=1"), "{text}");
    let _ = std::fs::remove_file(&out_path);

    // ...and one past it is refused, so the listing's count is the real
    // addressable range, not a prefix of it.
    let past = temp_path("past");
    let refused = run(
        "object-move",
        &[
            f.to_str().unwrap(),
            "--object",
            &count.to_string(),
            "--dx=5",
            "--dy=5",
            "-o",
            past.to_str().unwrap(),
        ],
    );
    assert!(
        !refused.status.success(),
        "index == count must be refused: {}",
        stdout(&refused)
    );
    assert!(!past.exists());
}

#[test]
fn lists_a_text_object_with_its_bbox() {
    let f = fixture("text/simple-winansi.pdf");
    let out = run("object-list", &[f.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("kind=text"), "{text}");
    // A text object is bbox-only (it carries no editable node geometry), so
    // the row is bbox + the two disclosure fields. `approximate=1` says the
    // box is not measured glyph ink — true of every text object — and
    // `bounds=` says which of the four constructions produced it.
    //
    // The fixture shows "Hello world" then "Second line" in Helvetica 24 at
    // x=72, second line one 24 pt leading below. Helvetica is a standard-14
    // face, so both its advance widths and its /Ascent+/Descent are real
    // metrics: the box's LEFT edge is the pen start (72) and its RIGHT edge
    // is 72 plus the accumulated advances of the longer line — NOT
    // `72 + one em`, which is what the pre-metrics box gave.
    assert!(text.contains("bounds=font-metrics"), "{text}");
    assert!(text.contains("bbox=72,665.032,232.008,717.232"), "{text}");
    assert!(text.contains("approximate=1"), "{text}");
    assert!(text.contains("objects=1 paths=0 text=1"), "{text}");
}

/// **The geometry regression.** The box must START at the pen and END past
/// the last glyph — the two properties the pre-metrics em box got wrong in
/// opposite directions, and the reason clicking visible letters could miss.
///
/// `mixed.pdf` shows `(Vector)` in Helvetica 14 at `30 150 Td`. Summing the
/// AFM advances for V-e-c-t-o-r gives 2890/1000 em ⇒ 40.46 pt, and
/// Helvetica's descriptor gives ascent 718 / descent −207 ⇒ +10.05/−2.90 pt.
/// So the box is `30,147.10 → 70.46,160.05`.
///
/// The old box was `16,136 → 44,164`: a 28 × 28 pt square centred on
/// (30,150). Note what that means concretely and why this test asserts BOTH
/// edges — the old box's left edge (16) was 14 pt of blank paper before the
/// first glyph, and its right edge (44) stopped 26 pt short of the last one.
#[test]
fn a_text_bbox_starts_at_the_pen_and_covers_the_whole_run() {
    let f = fixture("vector/mixed.pdf");
    let out = run("object-list", &[f.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    let row = stdout(&out)
        .lines()
        .find(|l| l.contains("kind=text"))
        .expect("the fixture has a text object")
        .to_owned();
    assert!(row.contains("bbox=30,147.102,70.46,160.052"), "{row}");
    assert!(row.contains("bounds=font-metrics"), "{row}");

    // And the hit test agrees, which is the point of the whole exercise: a
    // click on the LAST glyph of "Vector" (x≈68, comfortably past the old
    // box's right edge of 44) selects the text...
    let on_glyphs = run(
        "object-list",
        &[f.to_str().unwrap(), "--hit", "68,152", "--tolerance", "0"],
    );
    assert!(
        hit_line(&on_glyphs).contains("index=1 kind=text"),
        "a click on the last letters must select the text: {}",
        hit_line(&on_glyphs)
    );

    // ...and a click on the blank paper to the LEFT of the text (x=20, which
    // the old 28 pt square covered) selects nothing.
    let on_paper = run(
        "object-list",
        &[f.to_str().unwrap(), "--hit", "20,152", "--tolerance", "0"],
    );
    assert!(
        hit_line(&on_paper).contains("index=none"),
        "blank paper before the text must not select it: {}",
        hit_line(&on_paper)
    );
}

/// A **composite** font's advances come from `/W` over `/DW` (§9.7.4.3), a
/// dictionary array — not from the font program — so the metrics path
/// reaches the `Identity-H`-subsetted case that is the dominant modern
/// producer output, not only simple fonts.
///
/// The fixture's text cannot be DECODED (no `/ToUnicode`, §9.10.2's dead
/// end) and that is orthogonal: reading a CID's width and knowing which
/// character it is are two different lookups, and this row proves pdfcer
/// still gets the first one right while honestly failing the second.
///
/// It also pins the THIRD basis. The fixture's descendant CIDFont carries
/// `/DW` + `/W` but no `/FontDescriptor` at all, so the advances are real
/// and the vertical extent is pdfcer's nominal em — reported as
/// `bounds=metric-advances-nominal-height`, not as `font-metrics`. Claiming
/// the better basis here would be exactly the "sentence that no longer
/// matches the box" failure ui-spec §E.3 forbids.
#[test]
fn a_composite_font_run_is_measured_from_its_w_array() {
    let f = fixture("text/identity-h-no-tounicode.pdf");
    let out = run("object-list", &[f.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    let row = stdout(&out)
        .lines()
        .find(|l| l.contains("kind=text"))
        .expect("the fixture has a text object")
        .to_owned();
    // Undecodable text, measurable box — the two facts are independent.
    assert!(row.contains("text=undecodable"), "{row}");
    assert!(
        row.contains("bounds=metric-advances-nominal-height"),
        "{row}"
    );

    // The box must be WIDER than one em: an em box would be the pre-metrics
    // failure reappearing under a composite font. `size=18`, and the run's
    // `/W`-declared advances carry it to 81 pt.
    assert!(row.contains("size=18"), "{row}");
    let bbox = row
        .split_whitespace()
        .find_map(|t| t.strip_prefix("bbox="))
        .expect("a bbox field");
    let n: Vec<f64> = bbox.split(',').map(|v| v.parse().unwrap()).collect();
    let (width, size) = (n[2] - n[0], 18.0_f64);
    assert!(
        width > size,
        "a multi-glyph run must be wider than one em: {row}"
    );
    // And the nominal height is exactly the 1.0/0.25 em pair, so a reader of
    // this test can tell the guess apart from a measurement at a glance.
    let height = n[3] - n[1];
    assert!(
        (height - size * 1.25).abs() < 1e-6,
        "nominal height should be 1.25 em: {row}"
    );
}

/// A page past the end, and `--page 0`, are both clean named refusals — a
/// non-zero exit and a message naming the valid range, never a panic and
/// never an empty "success" listing that would read as "this page has no
/// objects".
#[test]
fn an_out_of_range_page_is_refused_cleanly() {
    let f = fixture("dimension/linear-base.pdf");
    for bad in ["7", "0"] {
        let out = run("object-list", &[f.to_str().unwrap(), "--page", bad]);
        assert_eq!(
            out.status.code(),
            Some(RUNTIME_ERROR),
            "--page {bad}: {}",
            stderr(&out)
        );
        let err = stderr(&out);
        assert!(err.contains("out of range"), "--page {bad}: {err}");
        assert!(err.contains("1 page(s)"), "--page {bad}: {err}");
        // A refusal lists nothing — no partial output to misread.
        assert!(
            !stdout(&out).contains("object page="),
            "--page {bad} printed rows"
        );
        // Not a panic.
        assert!(!err.contains("panicked"), "--page {bad}: {err}");
    }
}

/// The diagnosis regression (module docs §2). A page whose only content is
/// one stroked horizontal line — a path with a **zero-height** bounding box
/// — is hit by a click on it at every sane tolerance, and at `--tolerance 0`
/// too, because the stroke's own half-width (1 pt line ⇒ 0.5 pt each side)
/// is the hittable band. If this ever fails, hit-testing genuinely broke
/// for degenerate-bbox stroked geometry.
#[test]
fn a_click_on_a_stroked_line_selects_it() {
    let f = fixture("dimension/linear-base.pdf");
    // The bbox this same subcommand reports is 100,200..300,200; (200,200)
    // is its midpoint, dead on the line.
    for tol in ["0", "0.5", "3", "6"] {
        let out = run(
            "object-list",
            &[f.to_str().unwrap(), "--hit", "200,200", "--tolerance", tol],
        );
        assert!(out.status.success(), "tol {tol}: {}", stderr(&out));
        assert!(
            hit_line(&out).contains("index=0 kind=path"),
            "tol {tol}: a click ON the line must select it: {}",
            hit_line(&out)
        );
    }

    // 3 pt above the line: outside the 0.5 pt stroke band at a tight
    // tolerance, inside it at a forgiving one. This is the tolerance
    // actually doing something, not a constant that happens to pass.
    let tight = run(
        "object-list",
        &[
            f.to_str().unwrap(),
            "--hit",
            "200,203",
            "--tolerance",
            "0.5",
        ],
    );
    assert!(
        hit_line(&tight).contains("index=none"),
        "{}",
        hit_line(&tight)
    );
    let loose = run(
        "object-list",
        &[f.to_str().unwrap(), "--hit", "200,203", "--tolerance", "6"],
    );
    assert!(hit_line(&loose).contains("index=0"), "{}", hit_line(&loose));
}

/// A miss is an ANSWER, not an error: exit 0, with `index=none` as the
/// machine-readable field. A script asking "what is under this point?"
/// must be able to distinguish "nothing is there" from "the query failed",
/// and only the former is a success.
#[test]
fn a_hit_that_misses_reports_none_and_still_succeeds() {
    let f = fixture("dimension/linear-base.pdf");
    let out = run(
        "object-list",
        &[f.to_str().unwrap(), "--hit", "200,300", "--tolerance", "6"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let line = hit_line(&out);
    assert!(line.contains("index=none"), "{line}");
    assert!(line.contains("kind=none"), "{line}");
    // The inventory is still printed — a miss does not suppress the listing
    // that would tell the operator what IS on the page.
    assert!(
        stdout(&out).contains("object page=1 index=0"),
        "{}",
        stdout(&out)
    );
}

/// A nonsense `--tolerance` is refused rather than silently turning every
/// query into a miss.
///
/// This matters more than it looks: clap parses `--tolerance` as a bare
/// `f64`, so `nan` and negatives both arrive intact, and both would make
/// `hit_test_point` reject everything. An operator would read the resulting
/// `index=none` as "nothing is there" — a confident wrong answer, which is
/// exactly what rule 4 forbids. The refusal must also come BEFORE any
/// `object` row is printed, so a failed run never leaves half an answer on
/// stdout for a script to parse.
#[test]
fn a_nonsense_tolerance_is_refused_before_anything_is_printed() {
    let f = fixture("dimension/linear-base.pdf");
    for bad in ["-1", "nan"] {
        let out = run(
            "object-list",
            &[f.to_str().unwrap(), "--hit", "200,200", "--tolerance", bad],
        );
        assert_eq!(
            out.status.code(),
            Some(RUNTIME_ERROR),
            "--tolerance {bad}: {}",
            stdout(&out)
        );
        assert!(
            stderr(&out).contains("--tolerance must be"),
            "--tolerance {bad}: {}",
            stderr(&out)
        );
        assert!(
            stdout(&out).is_empty(),
            "--tolerance {bad} printed a partial answer: {}",
            stdout(&out)
        );
    }

    // Without `--hit` the tolerance is unused, so it is not policed — a
    // stray value must not break a plain inventory.
    let listing = run("object-list", &[f.to_str().unwrap(), "--tolerance", "-1"]);
    assert!(listing.status.success(), "{}", stderr(&listing));
}

/// Topmost wins: where two objects overlap, `--hit` reports the
/// LAST-painted one, matching the selection convention the GUI applies.
#[test]
fn a_hit_reports_the_topmost_object_only() {
    let f = fixture("vector/edit.pdf");
    // Inside the filled rectangle (bbox 200,50..280,110), which nothing
    // else covers — the single unambiguous case; the ordering guarantee
    // itself is unit-tested in core's `hit.rs`.
    let out = run("object-list", &[f.to_str().unwrap(), "--hit", "240,80"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(hit_line(&out).contains("index=1"), "{}", hit_line(&out));
}

/// A malformed coordinate is refused by name rather than silently parsed
/// into a confident wrong answer about what a click selects.
#[test]
fn a_malformed_hit_operand_is_refused() {
    let f = fixture("dimension/linear-base.pdf");
    for bad in ["oops", "200", "200,", "200,abc", "nan,0"] {
        let out = run("object-list", &[f.to_str().unwrap(), "--hit", bad]);
        assert_eq!(
            out.status.code(),
            Some(RUNTIME_ERROR),
            "--hit {bad} should be refused: {}",
            stdout(&out)
        );
        assert!(
            stderr(&out).contains("malformed --hit"),
            "--hit {bad}: {}",
            stderr(&out)
        );
    }
}

// ---------------------------------------------------------------------------
// `--all-hits` — the click-through-cycling oracle (ui-spec §C.3, rule 11)
// ---------------------------------------------------------------------------

/// Every `hit-candidate …` line, in output order.
fn candidate_lines(out: &Output) -> Vec<String> {
    stdout(out)
        .lines()
        .filter(|l| l.starts_with("hit-candidate "))
        .map(str::to_owned)
        .collect()
}

/// **The headline behaviour.** On `overlap.pdf`'s three concentric squares,
/// a click at the centre is inside all three — and only the innermost is
/// reachable through the topmost query. `--all-hits` reports the whole
/// stack, front-most first, which is exactly the order the GUI's repeated
/// Alt+clicks visit.
///
/// This is the CLI's half of rule 11 for the all-hits query: without it the
/// CLI could not reproduce, and therefore could not diagnose, the GUI's
/// cycling.
#[test]
fn all_hits_lists_every_object_under_the_point_front_most_first() {
    let f = fixture("vector/overlap.pdf");
    let out = run(
        "object-list",
        &[f.to_str().unwrap(), "--hit", "150,150", "--all-hits"],
    );
    assert!(out.status.success(), "{}", stderr(&out));

    let candidates = candidate_lines(&out);
    assert_eq!(candidates.len(), 3, "{}", stdout(&out));
    // Front-most first: object 2 (innermost, painted last), then 1, then 0.
    for (ordinal, index) in [(0, 2), (1, 1), (2, 0)] {
        assert!(
            candidates[ordinal].contains(&format!("ordinal={ordinal} index={index}")),
            "candidate {ordinal}: {}",
            candidates[ordinal]
        );
    }

    // The `hit` line still names the topmost — unchanged behaviour — and
    // agrees with `ordinal=0` by construction (one query answers both).
    let hit = hit_line(&out);
    assert!(hit.contains("index=2"), "{hit}");
    assert!(hit.contains("candidates=3"), "{hit}");
}

/// The list is about what is UNDER THE POINT, not about what is on the page:
/// a click nearer the edge sees a shorter stack, and a click outside every
/// square sees none.
#[test]
fn the_candidate_list_shrinks_as_the_point_leaves_each_square() {
    let f = fixture("vector/overlap.pdf");
    // (85,85): inside objects 0 and 1, outside 2.
    let two = run(
        "object-list",
        &[f.to_str().unwrap(), "--hit", "85,85", "--all-hits"],
    );
    let candidates = candidate_lines(&two);
    assert_eq!(candidates.len(), 2, "{}", stdout(&two));
    assert!(candidates[0].contains("index=1"), "{}", candidates[0]);
    assert!(candidates[1].contains("index=0"), "{}", candidates[1]);
    assert!(hit_line(&two).contains("candidates=2"));

    // (35,35): inside the outermost only.
    let one = run(
        "object-list",
        &[f.to_str().unwrap(), "--hit", "35,35", "--all-hits"],
    );
    assert_eq!(candidate_lines(&one).len(), 1, "{}", stdout(&one));
    assert!(hit_line(&one).contains("index=0"), "{}", hit_line(&one));

    // Off every square: a miss is a valid answer, exit 0, no candidate
    // lines, and `index=none` on the `hit` line.
    let none = run(
        "object-list",
        &[f.to_str().unwrap(), "--hit", "295,295", "--all-hits"],
    );
    assert!(none.status.success(), "{}", stderr(&none));
    assert!(candidate_lines(&none).is_empty(), "{}", stdout(&none));
    assert!(
        hit_line(&none).contains("index=none"),
        "{}",
        hit_line(&none)
    );
    assert!(
        hit_line(&none).contains("candidates=0"),
        "{}",
        hit_line(&none)
    );
}

/// `--all-hits` without `--hit` is a no-op, and the `hit-candidate` prefix
/// never collides with the `hit ` line a script may already be matching.
#[test]
fn all_hits_without_a_hit_query_prints_nothing_extra() {
    let f = fixture("vector/overlap.pdf");
    let out = run("object-list", &[f.to_str().unwrap(), "--all-hits"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(candidate_lines(&out).is_empty(), "{}", stdout(&out));
    assert!(
        !stdout(&out).lines().any(|l| l.starts_with("hit ")),
        "{}",
        stdout(&out)
    );
}

// ---------------------------------------------------------------------------
// Richer object rows (ui-spec §B.4, rule 11)
// ---------------------------------------------------------------------------

/// §B.4 #1 in the CLI: a text row now carries the decoded string and the
/// typeface, so a script and the GUI's Objects panel read the same facts
/// about the same object.
///
/// The string is `HelloworldSecond line` and NOT `Hello world` on two lines:
/// the fixture opens its word gap with a `TJ` kerning offset and no space
/// glyph, and starts its second line with a bare `Td` — §14.8.2.5 S3/S5,
/// neither of which the file states. A preview reports the SOURCED
/// characters, the way `ExtractedText::sourced_text` does; deriving spacing
/// here would present a reader's guess as the document's content.
#[test]
fn a_text_row_carries_its_decoded_string_and_font() {
    let f = fixture("text/simple-winansi.pdf");
    let out = run("object-list", &[f.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains(r#"font="Helvetica""#), "{text}");
    assert!(text.contains(r#"resource="F1""#), "{text}");
    assert!(text.contains("size=24"), "{text}");
    assert!(text.contains(r#"text="HelloworldSecond line""#), "{text}");
    assert!(text.contains("truncated=0 lossy=0"), "{text}");
}

/// **The honesty case.** A font whose encoding defeats §9.10.2's ladder must
/// report `text=undecodable` — a bare token, distinguishable from any quoted
/// string a document could contain — and never a row of U+FFFD dressed up as
/// extracted text. A test that ever sees real characters out of this fixture
/// has found a fabrication.
#[test]
fn text_that_cannot_be_decoded_says_so_rather_than_emitting_mojibake() {
    let f = fixture("text/identity-h-no-tounicode.pdf");
    let out = run("object-list", &[f.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("text=undecodable"), "{text}");
    assert!(text.contains("lossy=1"), "{text}");
    assert!(
        !text.contains('\u{fffd}'),
        "no replacement characters may reach the output: {text}"
    );
    // The FONT is still named — knowing which font cannot be read is most
    // of the value of the disclosure.
    assert!(text.contains(r#"font="ABCDEF+TestCID""#), "{text}");
}

/// §B.4 #2 in the CLI: an image row carries its `/Width`×`/Height` sample
/// count (§8.9.5 Table 89), distinct from the `bbox=` size on the same line.
#[test]
fn an_image_row_carries_its_pixel_dimensions_distinct_from_its_bbox() {
    let f = fixture("vector/mixed.pdf");
    let out = run("object-list", &[f.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    let image = stdout(&out)
        .lines()
        .find(|l| l.contains("kind=image"))
        .expect("the fixture has an image object")
        .to_owned();
    // 2x2 DeviceGray samples, placed into a 60x40 pt box by the CTM. The
    // two numbers are different things and both are on the line.
    assert!(image.contains("pixels=2x2"), "{image}");
    assert!(image.contains("bbox=30,250,90,290"), "{image}");
}

/// A path row gains nothing it does not have: no `text=`, no `pixels=`. The
/// per-kind fields stay per-kind, so a script can key on `kind=` and know
/// exactly which fields to expect.
#[test]
fn a_path_row_gains_no_text_or_pixel_fields() {
    let f = fixture("vector/edit.pdf");
    let out = run("object-list", &[f.to_str().unwrap()]);
    let text = stdout(&out);
    for line in text.lines().filter(|l| l.contains("kind=path")) {
        assert!(!line.contains("text="), "{line}");
        assert!(!line.contains("pixels="), "{line}");
        assert!(!line.contains("font="), "{line}");
    }
}

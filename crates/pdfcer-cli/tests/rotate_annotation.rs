//! # `pdfcer rotate-annotation` — black-box over the real binary
//!
//! `Pass 155.0` shipped annotation rotation in core **and** CLI. It shipped
//! with **no CLI test at all**, and the gap was carried in the handoff for
//! several Passes. This closes it.
//!
//! ## Why the shell needs its own tests here
//!
//! The core suite (`crates/pdfcer-core/tests/annot_rotate.rs`) covers the
//! geometry and the refusals against `EditSession`. None of that reaches:
//!
//! * **`--page` / `--index` resolution.** The CLI numbers annotations from
//!   **0** in `list-annotations` order and takes a **1-based** page. That
//!   mapping lives only in the shell, and an off-by-one rotates the *wrong
//!   annotation* — which no core test can see.
//! * **Flag wiring.** `--degrees`, `--anchor-x`, `--anchor-y` all accept
//!   negatives (`allow_negative_numbers`). A flag can be parsed, documented,
//!   and never reach the core call; unit tests hit core directly and pass
//!   regardless.
//! * **Exit codes**, which are the CLI's contract with a script.
//! * **The report line**, which is what a script parses and an operator reads.
//!
//! ## ★★ The assertion that actually discriminates: WHICH WAY IS POSITIVE
//!
//! Rotation here is **anticlockwise**. PDF user space has its origin at the
//! bottom-left (§8.3.2.3), so a positive angle turns the way a mathematician
//! expects and **not** the way a screen does — the single most likely thing
//! for a shell to get backwards, and it is invisible at 180° and at 0°.
//!
//! The fixture square is `[20 20 90 90]` and the anchor is `(100, 100)`.
//! Rotating `+90°` anticlockwise about that point maps a corner `(x, y)` to
//! `(100 − (y−100), 100 + (x−100))`:
//!
//! ```text
//!   (20,20) -> (180, 20)        (90,90) -> (110, 90)
//!   => /Rect [110 20 180 90]
//! ```
//!
//! **Clockwise would give `[20 110 90 180]`** — a different rectangle, so the
//! test fails loudly rather than passing on a mirrored convention.
//!
//! Fixture provenance: `fixtures/synthetic/annot/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_rotannot_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().expect("binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn demo() -> PathBuf {
    fixture("annot/demo-annotated.pdf")
}

/// `rect=` for annotation `index` on page 1, read back out of the SAVED file
/// with `list-annotations` — never from the report the rotate command printed
/// about itself.
///
/// A command that prints the right rectangle and writes the wrong one passes
/// any report-only assertion, and that is the failure mode worth guarding: the
/// report is computed from the plan, the file is what a viewer opens.
fn rect_of(path: &Path, index: usize) -> String {
    let out = run(&["list-annotations", path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "list-annotations failed: {}",
        stderr(&out)
    );
    stdout(&out)
        .lines()
        .find(|l| l.starts_with(&format!("annot page=1 index={index} ")))
        .and_then(|l| l.split("rect=").nth(1))
        .and_then(|s| s.split_whitespace().next())
        .unwrap_or_else(|| panic!("no annotation at index {index}"))
        .to_string()
}

fn rotate(input: &Path, index: usize, degrees: &str, out_path: &Path) -> Output {
    run(&[
        "rotate-annotation",
        input.to_str().unwrap(),
        "--page",
        "1",
        "--index",
        &index.to_string(),
        "--degrees",
        degrees,
        "--anchor-x",
        "100",
        "--anchor-y",
        "100",
        "-o",
        out_path.to_str().unwrap(),
    ])
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// ★★ +90° is ANTICLOCKWISE. See the module docs for the arithmetic — a
/// clockwise implementation lands on `20,110,90,180` and fails here.
#[test]
fn a_positive_angle_turns_anticlockwise_about_the_anchor() {
    let out_path = temp_path("ccw");
    assert_eq!(
        rect_of(&demo(), 0),
        "20,20,90,90",
        "fixture precondition: the stamp is a 70pt square at the origin corner"
    );

    let out = rotate(&demo(), 0, "90", &out_path);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        rect_of(&out_path, 0),
        "110,20,180,90",
        "+90 about (100,100) must turn ANTICLOCKWISE; clockwise gives \
         20,110,90,180"
    );
}

/// A negative angle is accepted (`allow_negative_numbers`) and turns the other
/// way. Both halves matter: clap would otherwise read `-90` as a flag, and the
/// result must be the mirror of the test above.
#[test]
fn a_negative_angle_is_accepted_and_turns_the_other_way() {
    let out_path = temp_path("cw");
    let out = rotate(&demo(), 0, "-90", &out_path);
    assert!(
        out.status.success(),
        "a negative angle must parse, not be read as a flag: {}",
        stderr(&out)
    );
    assert_eq!(
        rect_of(&out_path, 0),
        "20,110,90,180",
        "-90 is the mirror of +90 about the same anchor"
    );
}

/// 360° returns the rectangle to where it started — **to within floating-point
/// noise, which is asserted with a tolerance and not with string equality.**
///
/// This is the round-trip check on the anchor arithmetic: an anchor applied
/// inconsistently between the two halves of the transform drifts, and drift is
/// invisible at any single angle.
///
/// ★ **The identity is NOT exact, and that is stated rather than hidden.**
/// Measured 2026-08-29: `[20 20 90 90]` about `(100,100)` comes back as
/// `[19.999999999999975, 20.000000000000007, 90, 90.00000000000003]` — the
/// `sin`/`cos` of 2π in `f64`. The residual is ~2.5e-14 pt, which is about
/// 10⁻¹⁶ of a millimetre; no renderer, no viewer and no measurement can
/// resolve it.
///
/// pdfcer deliberately does **not** snap a near-integer result back: a snap
/// threshold is a made-up number that would silently move geometry an operator
/// *did* intend to place off-grid, and 360° is a legitimate edit request
/// rather than a no-op to be optimised away.
///
/// The tolerance is **0.001 pt** — four orders of magnitude above the observed
/// noise and four below anything an anchor bug would produce, so this still
/// fails loudly on the defect it exists for.
#[test]
fn a_full_turn_returns_the_rectangle_to_within_floating_point_noise() {
    fn corners(s: &str) -> Vec<f64> {
        s.split(',')
            .map(|v| v.parse::<f64>().expect("rect= is four numbers"))
            .collect()
    }

    let out_path = temp_path("full");
    let before = corners(&rect_of(&demo(), 0));
    let out = rotate(&demo(), 0, "360", &out_path);
    assert!(out.status.success(), "{}", stderr(&out));
    let after = corners(&rect_of(&out_path, 0));

    assert_eq!(before.len(), 4);
    for (i, (a, b)) in before.iter().zip(after.iter()).enumerate() {
        assert!(
            (a - b).abs() < 0.001,
            "corner {i} drifted {} pt over a full turn ({a} -> {b}); \
             floating-point noise is ~1e-14, so this is an anchor bug",
            (a - b).abs()
        );
    }
}

/// The report names both rectangles, so a script can diff them without
/// re-reading the file — and an operator can see what moved.
#[test]
fn the_report_names_the_before_and_after_rectangles() {
    let out_path = temp_path("report");
    let out = rotate(&demo(), 0, "90", &out_path);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains("[20.00 20.00 90.00 90.00]->[110.00 20.00 180.00 90.00]"),
        "the report must show the transition: {text}"
    );
    assert!(
        text.contains("degrees=90.0000") && text.contains("anchor=(100.00 100.00)"),
        "and echo the parameters it acted on: {text}"
    );
}

/// ★ `--index` selects, and selecting one annotation must not move another.
///
/// The core cannot see this: the index→id mapping is the shell's. An
/// off-by-one here rotates the wrong annotation and every core test still
/// passes.
#[test]
fn index_selects_one_annotation_and_leaves_its_neighbours_alone() {
    let out_path = temp_path("index");
    let neighbour_before = rect_of(&demo(), 2);
    let out = rotate(&demo(), 0, "90", &out_path);
    assert!(out.status.success(), "{}", stderr(&out));

    assert_ne!(rect_of(&out_path, 0), "20,20,90,90", "index 0 moved");
    assert_eq!(
        rect_of(&out_path, 2),
        neighbour_before,
        "index 2 must be untouched — a shifted index would move it instead"
    );
    assert!(
        stdout(&out).contains("index=0"),
        "and the report must name the index it acted on: {}",
        stdout(&out)
    );
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// ★★ A form widget is refused, and — since `Pass 163.0` — the message says
/// the verb it names is **not built yet**.
///
/// # ★ THIS TEST'S ASSERTION WAS INVERTED BY `Pass 177.0`, DELIBERATELY
///
/// It used to require the message to contain **"NOT BUILT YET"**. That was
/// right when written: the sentence said *"use `rotate_widget` instead"* and
/// `rotate_widget` did not exist — a runtime message, read at the moment the
/// operator is blocked, naming a way out that was not there
/// (`tools/check-cited-verbs-exist.py` is the structural guard for that
/// class; this pinned the operator-visible half).
///
/// `Pass 177.0` built the verb, so the old assertion became a test **pinning a
/// temporary state as though it were a contract** — and it failed, correctly,
/// the moment the state changed. That is the right failure mode and it is why
/// the assertion is rewritten rather than deleted: the message must now name a
/// verb that RESOLVES, and must not drift back into apologising for one that
/// does not exist.
#[test]
fn a_form_widget_is_refused_and_the_message_names_a_verb_that_exists() {
    let out_path = temp_path("widget");
    let out = run(&[
        "rotate-annotation",
        fixture("annot/as-state-checkbox.pdf").to_str().unwrap(),
        "--page",
        "1",
        "--index",
        "0",
        "--degrees",
        "45",
        "--anchor-x",
        "0",
        "--anchor-y",
        "0",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_ne!(out.status.code(), Some(0), "a widget must be refused");
    let err = stderr(&out);
    assert!(
        err.contains("/MK /R"),
        "the refusal must say WHY -- a widget's rotation is a quarter-turn \
         declaration plus a redrawn appearance, not a free-angle transform \
         of a rectangle: {err}"
    );
    assert!(
        err.contains("rotate_widget(fqn, index, degrees)"),
        "★ and must name the verb WITH ITS SIGNATURE, so the sentence is a \
         usable instruction rather than a search term: {err}"
    );
    assert!(
        !err.to_uppercase().contains("NOT BUILT"),
        "★★ and must NOT still apologise for a verb that now exists -- \
         `Pass 177.0` built it: {err}"
    );
    assert!(
        err.to_uppercase().contains("COUNTERCLOCKWISE"),
        "the direction is the trap worth naming here: /MK /R is \
         counterclockwise and the page's /Rotate is clockwise: {err}"
    );
    assert!(!out_path.exists(), "a refusal must write no output file");
}

/// An index past the end is refused with the count that would have worked, not
/// a panic and not a silent no-op.
#[test]
fn an_index_past_the_end_is_refused_with_the_real_count() {
    let out_path = temp_path("oor");
    let out = rotate(&demo(), 9, "45", &out_path);
    assert_ne!(out.status.code(), Some(0));
    let err = stderr(&out);
    assert!(
        err.contains("no annotation at index 9") && err.contains("it has 4"),
        "the refusal must name the range that works: {err}"
    );
    assert!(!out_path.exists());
}

/// A page past the end is refused too — the page number is 1-based and a `0`
/// or an over-count must not be read as "the first page".
#[test]
fn a_page_past_the_end_is_refused() {
    let out_path = temp_path("page");
    let out = run(&[
        "rotate-annotation",
        demo().to_str().unwrap(),
        "--page",
        "7",
        "--index",
        "0",
        "--degrees",
        "45",
        "--anchor-x",
        "0",
        "--anchor-y",
        "0",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_ne!(out.status.code(), Some(0), "page 7 does not exist");
    assert!(!out_path.exists());
}

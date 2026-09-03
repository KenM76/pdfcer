//! # `pdfcer` vector-edit integration tests (Pass 9c-min)
//!
//! Black-box tests over the **real binary** for the three basic vector edits
//! (decision 011 §2.5): `object-move`, `object-delete`, `node-move`. They
//! assert the process contract a batch script depends on — exit codes, the
//! stable report line, the `verify_undo` byte-identity flag, and the named
//! refusals — over the committed `fixtures/synthetic/vector/edit.pdf`
//! (three isolated, index-predictable objects: line / rectangle / triangle;
//! provenance in that directory's `PROVENANCE.md`).
//!
//! The strong invariant checked here (the R46/§5.7 named exception) is the
//! `--verify-undo` flag reporting `undo_identical=1`: undoing the edit
//! reproduces the input byte for byte, and the output is a byte-prefix of the
//! input plus one appended revision that names exactly one object.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
/// `exit::EDIT_REFUSED` in the CLI's stable exit-code contract.
const EDIT_REFUSED: i32 = 9;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/vector/edit.pdf")
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pdfcer_vedit_{tag}_{}_{n}.pdf", std::process::id()))
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

#[test]
fn object_move_succeeds_and_undoes_byte_identically() {
    let out_path = temp_path("move");
    let out = run(
        "object-move",
        &[
            fixture().to_str().unwrap(),
            "--page",
            "1",
            "--object",
            "0",
            "--dx=30",
            "--dy=-20",
            "-o",
            out_path.to_str().unwrap(),
            "--verify-undo",
        ],
    );
    assert!(out.status.success(), "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("object-move"), "{text}");
    assert!(
        text.contains("changed=1"),
        "exactly one object changed: {text}"
    );
    assert!(
        text.contains("undo_identical=1"),
        "undo byte-identical: {text}"
    );

    // The output is a byte-prefix of the input (incremental append) — the
    // R46/§5.7 content-identity property, observable from the bytes.
    let base = std::fs::read(fixture()).unwrap();
    let produced = std::fs::read(&out_path).unwrap();
    assert!(produced.starts_with(&base), "incremental prefix");
    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn object_delete_removes_one_object() {
    let out_path = temp_path("del");
    let out = run(
        "object-delete",
        &[
            fixture().to_str().unwrap(),
            "--object",
            "2",
            "-o",
            out_path.to_str().unwrap(),
            "--verify-undo",
        ],
    );
    assert!(out.status.success(), "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("object-delete"), "{text}");
    assert!(text.contains("changed=1"), "{text}");
    assert!(text.contains("undo_identical=1"), "{text}");
    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn node_move_relocates_an_anchor() {
    let out_path = temp_path("node");
    let out = run(
        "node-move",
        &[
            fixture().to_str().unwrap(),
            "--object",
            "0",
            "--node",
            "1",
            "--x",
            "200",
            "--y",
            "100",
            "-o",
            out_path.to_str().unwrap(),
            "--verify-undo",
        ],
    );
    assert!(out.status.success(), "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("node-move"), "{text}");
    assert!(text.contains("undo_identical=1"), "{text}");
    let _ = std::fs::remove_file(&out_path);
}

/// A rectangle corner drags (Pass 30.0), and the operator is TOLD that the
/// rectangle had to be rewritten as four lines to express it.
///
/// This test previously asserted the refusal. The disclosure half is the part
/// that needed a CLI-level test rather than a core one: the core produces the
/// string, and the only thing that can go wrong from here is the front end
/// dropping it — which is precisely what every caller of these methods did
/// before, because the return type let them.
///
/// Also pins WHICH stream it lands on. The stdout line is a fixed-shape record
/// that scripts parse; a prose block spliced into it would break them, and
/// nothing but a test stops a later edit from moving it there.
#[test]
fn node_move_on_a_rectangle_corner_expands_it_and_says_so() {
    let out_path = temp_path("rect");
    // Object 1 is the `re` rectangle; node 0 is its lower-left corner.
    let out = run(
        "node-move",
        &[
            fixture().to_str().unwrap(),
            "--object",
            "1",
            "--node",
            "0",
            "--x",
            "0",
            "--y",
            "0",
            "-o",
            out_path.to_str().unwrap(),
            "--verify-undo",
        ],
    );
    assert!(out.status.success(), "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("node-move"), "{text}");
    // Undo restores the single `re` from five operators — a shrink, which a
    // same-length rewrite would never exercise.
    assert!(text.contains("undo_identical=1"), "{text}");

    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        err.contains("rectangle"),
        "the operator must be told the rectangle was rewritten: {err:?}"
    );
    assert!(
        !text.contains("rectangle"),
        "the disclosure belongs on stderr; stdout is the machine-readable record: {text}"
    );
    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn an_out_of_range_object_is_refused() {
    let out_path = temp_path("oor");
    let out = run(
        "object-move",
        &[
            fixture().to_str().unwrap(),
            "--object",
            "999",
            "--dx=1",
            "--dy=1",
            "-o",
            out_path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    assert!(!out_path.exists());
}

// ---------------------------------------------------------------------------
// `nodes-move` — the batch form (`Pass 23.3`)
// ---------------------------------------------------------------------------

/// **The case a loop of `node-move` cannot express.** Both bottom corners of
/// the rectangle move in ONE command, and the result undoes byte-identically.
///
/// Object 1 of the fixture is a closed 4-anchor rectangle at
/// `bbox=200,50,280,110`. Corners 0 and 1 are its two bottom anchors; sending
/// both to `y = 80` lifts that edge without touching the top one. All four
/// corners of a rectangle are the same four operands of a single `re`
/// operator, so this is precisely the shape that needs one surgery rather
/// than two.
#[test]
fn nodes_move_lifts_two_corners_of_one_rectangle_in_one_command() {
    let out_path = temp_path("nodesmove");
    let out = run(
        "nodes-move",
        &[
            fixture().to_str().unwrap(),
            "--object",
            "1",
            "--move",
            "0,200,80",
            "--move",
            "1,280,80",
            "-o",
            out_path.to_str().unwrap(),
            "--verify-undo",
        ],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let line = stdout(&out);
    assert!(line.contains("nodes-move "), "report line missing: {line}");
    assert!(
        line.contains("object=1 nodes=2"),
        "the line must say how many anchors moved: {line}",
    );
    assert!(
        line.contains("undo_identical=1"),
        "one command must undo byte-identically: {line}",
    );
    // ONE object written — the whole batch is a single content-stream surgery,
    // not two.
    assert!(
        line.contains("objects=1"),
        "expected one object written: {line}"
    );

    // The rectangle-expansion disclosure goes to STDERR, so the stdout record
    // stays a fixed shape, and it is said ONCE for the pair.
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        err.matches("stored as a rectangle").count(),
        1,
        "two corners of ONE rectangle owe the disclosure once, not twice: {err}",
    );
    assert!(
        !err.contains("one corner"),
        "the disclosure must not claim a corner COUNT — two moved here: {err}",
    );

    // And the geometry actually changed: the bottom edge lifted to y=80.
    let listed = run("object-list", &[out_path.to_str().unwrap()]);
    let listing = stdout(&listed);
    assert!(
        listing.contains("index=1 kind=path bbox=200,80,280,110"),
        "bottom edge did not lift to y=80: {listing}",
    );
    assert!(
        listing.contains("anchors=4 closed=1"),
        "the shape must stay a closed 4-anchor path after the expansion: {listing}",
    );
    let _ = std::fs::remove_file(&out_path);
}

/// The same anchor twice is refused by name, before anything is written.
#[test]
fn nodes_move_refuses_a_duplicated_anchor() {
    let out_path = temp_path("nodesdup");
    let out = run(
        "nodes-move",
        &[
            fixture().to_str().unwrap(),
            "--object",
            "1",
            "--move",
            "0,1,2",
            "--move",
            "0,3,4",
            "-o",
            out_path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("named more than once"),
        "the refusal must say WHICH problem it is: {err}",
    );
    assert!(!out_path.exists(), "a refused batch must write nothing");
}

/// An out-of-range anchor refuses the WHOLE batch — never a partial apply.
#[test]
fn nodes_move_refuses_the_whole_batch_for_one_bad_index() {
    let out_path = temp_path("nodesrange");
    let out = run(
        "nodes-move",
        &[
            fixture().to_str().unwrap(),
            "--object",
            "1",
            "--move",
            "0,210,60",
            "--move",
            "99,1,2",
            "-o",
            out_path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("out of range"), "{err}");
    assert!(
        !out_path.exists(),
        "anchor 0's move must NOT have been applied — the batch is all or nothing",
    );
}

/// A malformed `--move` token is caught before the document is even opened,
/// and the message names the token and what was wrong with it.
#[test]
fn nodes_move_rejects_a_malformed_move_token_by_name() {
    for (token, needle) in [
        ("0,1", "expected NODE,X,Y"),
        ("a,1,2", "is not a 0-based anchor index"),
        ("0,x,2", "is not a number"),
    ] {
        let out_path = temp_path("nodesbad");
        let out = run(
            "nodes-move",
            &[
                fixture().to_str().unwrap(),
                "--object",
                "1",
                "--move",
                token,
                "-o",
                out_path.to_str().unwrap(),
            ],
        );
        assert!(!out.status.success(), "{token:?} must not succeed");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(needle), "for {token:?}, got: {err}");
        assert!(
            err.contains(token),
            "the message must quote the token: {err}"
        );
        assert!(!out_path.exists());
    }
}

/// **`--move` must not be greedy.** `--move 0,1,2 -o out.pdf` has to read
/// `-o` as the output flag, not as another move token.
///
/// This is a regression test for a real defect: the flag was first declared
/// `num_args = 1..`, which swallowed `-o` and its value and made every
/// invocation die reporting `--output` missing. The failure looked like a
/// user error rather than an argument-definition bug.
#[test]
fn the_move_flag_does_not_swallow_the_output_flag() {
    let out_path = temp_path("nodesgreedy");
    let out = run(
        "nodes-move",
        &[
            fixture().to_str().unwrap(),
            "--object",
            "1",
            "--move",
            "0,205,55",
            "-o",
            out_path.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "a single --move followed by -o must parse: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(out_path.exists());
    let _ = std::fs::remove_file(&out_path);
}

/// Negative page-space coordinates are legal and must survive the parser —
/// `allow_hyphen_values` is what makes `0,-5,-5` a value rather than a flag.
#[test]
fn nodes_move_accepts_negative_coordinates() {
    let out_path = temp_path("nodesneg");
    let out = run(
        "nodes-move",
        &[
            fixture().to_str().unwrap(),
            "--object",
            "1",
            "--move",
            "0,-5,-5",
            "-o",
            out_path.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "a negative coordinate must parse: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    let _ = std::fs::remove_file(&out_path);
}

// ---------------------------------------------------------------------------
// `text-run-delete` — one label, not all 237 (`Pass 32.0`)
// ---------------------------------------------------------------------------

fn text_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name)
}

/// **The Pass, end to end.** Four labels in one text object; deleting one
/// leaves three, where `object-delete` would have removed all four.
///
/// Asserted through `object-list`'s own `runs=` and decoded `text=`, so the
/// check is on what a reader sees rather than on the bytes the edit wrote.
#[test]
fn text_run_delete_removes_one_label_and_leaves_the_rest() {
    let src = text_fixture("runs-inherited.pdf");
    let before = stdout(&run("object-list", &[src.to_str().unwrap()]));
    assert!(
        before.contains("runs=4") && before.contains(r#"text="ALPHABETAGAMMADELTA""#),
        "the fixture must start with four runs: {before}",
    );

    let out_path = temp_path("textrun");
    let out = run(
        "text-run-delete",
        &[
            src.to_str().unwrap(),
            "--object",
            "0",
            "--run",
            "3",
            "-o",
            out_path.to_str().unwrap(),
            "--verify-undo",
        ],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    let line = stdout(&out);
    assert!(line.contains("object=0 run=3"), "{line}");
    assert!(
        line.contains("undo_identical=1"),
        "one run removal must undo byte-identically: {line}",
    );

    let after = stdout(&run("object-list", &[out_path.to_str().unwrap()]));
    assert!(
        after.contains("runs=3"),
        "one run must be gone, not the object: {after}",
    );
    assert!(
        after.contains(r#"text="ALPHABETAGAMMA""#),
        "the other three labels must survive intact: {after}",
    );
    let _ = std::fs::remove_file(&out_path);
}

/// The §9.4.2 guard reaches the CLI, and its message carries the remedy.
#[test]
fn text_run_delete_refuses_when_the_next_run_would_move() {
    let out_path = temp_path("textrunrefuse");
    let out = run(
        "text-run-delete",
        &[
            text_fixture("runs-inherited.pdf").to_str().unwrap(),
            "--object",
            "0",
            "--run",
            "2",
            "-o",
            out_path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("would move the run after it"),
        "the refusal must say WHY: {err}",
    );
    assert!(
        err.contains("delete the later run first"),
        "and must name the remedy, which always works: {err}",
    );
    assert!(!out_path.exists(), "a refused edit must write nothing");
}

/// Deleting the only run removes the text object and leaves the rest of the
/// page — asserted via `object-list`'s own object census.
#[test]
fn text_run_delete_on_the_last_run_removes_the_text_object() {
    let out_path = temp_path("textrunlast");
    let out = run(
        "text-run-delete",
        &[
            text_fixture("runs-single.pdf").to_str().unwrap(),
            "--object",
            "1",
            "--run",
            "0",
            "-o",
            out_path.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    let after = stdout(&run("object-list", &[out_path.to_str().unwrap()]));
    assert!(
        after.contains("text=0"),
        "the text object must be gone: {after}",
    );
    assert!(
        after.contains("paths=1"),
        "and the unrelated path must remain: {after}",
    );
    let _ = std::fs::remove_file(&out_path);
}

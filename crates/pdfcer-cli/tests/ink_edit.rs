//! # `pdfcer ink-edit` (`Pass 278.0`)
//!
//! Black-box over the **real binary**, and every claim is checked against the
//! *output file* or the binary's own output, never against a core-level call.
//!
//! ## ★ Why the core suite is not enough here
//!
//! `crates/pdfcer-core/tests/ink_reshape.rs` calls `reshape_ink` directly, so
//! it exercises every guard **whatever the CLI does with the flags**. A flag
//! that clap parses and the dispatch then drops on the floor passes all 23 of
//! those tests. This project has shipped exactly that defect before — a
//! half-wired flag is invisible to a core-only suite by construction — and the
//! only thing that finds it is running the binary and reading the saved bytes.
//!
//! So `--stroke` and `--point` are each asserted by running the same edit
//! twice with the index changed and requiring the outputs to **differ**. An
//! index that is parsed and ignored makes the two runs identical.
//!
//! The fixture is authored by `pdfcer annotate --type ink` in the same run, so
//! this suite depends on no checked-in ink file and the geometry under test is
//! written down here rather than inferred from bytes somebody else made.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
/// `exit::EDIT_REFUSED` in the CLI's stable exit-code contract.
const EDIT_REFUSED: i32 = 9;

fn base_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/annot/demo-annotated.pdf")
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_inkedit_{tag}_{}_{n}.pdf",
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

/// A file carrying one `/Ink` with two strokes: three points then two.
///
/// Authored through the binary's own `annotate`, so the whole chain an
/// operator would actually use is exercised — including that
/// `list-annotations` can find the thing `ink-edit` then addresses.
fn ink_file(tag: &str) -> PathBuf {
    let out = temp_path(tag);
    let r = run(&[
        "annotate",
        base_fixture().to_str().unwrap(),
        "--type",
        "ink",
        "--page",
        "1",
        "--strokes",
        "10,10 20,30 25,35 | 40,40 50,60",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "annotate failed: {}{}",
        stdout(&r),
        stderr(&r)
    );
    out
}

/// The index `list-annotations` gives the authored `/Ink`.
///
/// Looked up rather than assumed: the base fixture already carries four
/// annotations, and hard-coding `4` would silently address the wrong one the
/// day that fixture changes.
fn ink_index(file: &Path) -> usize {
    let r = run(&["list-annotations", file.to_str().unwrap()]);
    stdout(&r)
        .lines()
        .find(|l| l.contains("subtype=Ink"))
        .and_then(|l| {
            l.split_whitespace()
                .find_map(|t| t.strip_prefix("index="))
                .and_then(|n| n.parse().ok())
        })
        .expect("the authored /Ink is listed")
}

/// One `ink-edit` run over a freshly authored ink file, returning the saved
/// bytes.
fn edit(tag: &str, extra: &[&str]) -> Vec<u8> {
    let src = ink_file(tag);
    let idx = ink_index(&src).to_string();
    let out = temp_path(&format!("{tag}_out"));
    let mut args: Vec<String> = vec![
        "ink-edit".to_owned(),
        src.to_str().unwrap().to_owned(),
        "--annot".to_owned(),
        idx,
        "--output".to_owned(),
        out.to_str().unwrap().to_owned(),
    ];
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let r = run(&refs);
    assert_eq!(
        r.status.code(),
        Some(0),
        "ink-edit failed: {}{}",
        stdout(&r),
        stderr(&r)
    );
    std::fs::read(&out).expect("output file")
}

/// The LAST `/InkList` in the file — an incremental update appends the new
/// annotation dictionary, so the last occurrence is the effective one. Read
/// from the bytes rather than by re-parsing, which keeps this black-box.
fn last_ink_list(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let at = text.rfind("/InkList").expect("an /InkList in the output");
    let rest = &text[at + "/InkList".len()..];
    let end = rest.find("]]").map_or(rest.len(), |i| i + 2);
    rest[..end].trim().to_owned()
}

// --------------------------------------------------------------- the flags

/// ★★★ `--stroke` reaches the engine.
///
/// The same point index in two different strokes must produce two different
/// files. A `--stroke` that clap parses and the dispatch drops makes these
/// identical, and every core test still passes.
#[test]
fn the_stroke_flag_is_wired() {
    let a = edit(
        "stroke0",
        &[
            "--op",
            "move-point",
            "--stroke",
            "0",
            "--point",
            "0",
            "--dx",
            "5",
        ],
    );
    let b = edit(
        "stroke1",
        &[
            "--op",
            "move-point",
            "--stroke",
            "1",
            "--point",
            "0",
            "--dx",
            "5",
        ],
    );
    assert_ne!(
        last_ink_list(&a),
        last_ink_list(&b),
        "moving stroke 0's first point and stroke 1's first point cannot produce \
         the same /InkList"
    );
}

/// ★★ `--point` reaches the engine, for the same reason.
#[test]
fn the_point_flag_is_wired() {
    let a = edit(
        "point0",
        &[
            "--op",
            "move-point",
            "--stroke",
            "0",
            "--point",
            "0",
            "--dy",
            "9",
        ],
    );
    let b = edit(
        "point2",
        &[
            "--op",
            "move-point",
            "--stroke",
            "0",
            "--point",
            "2",
            "--dy",
            "9",
        ],
    );
    assert_ne!(last_ink_list(&a), last_ink_list(&b));
}

/// `--dx`/`--dy` land as page-space points, with the sign the help promises
/// (positive `--dy` is UP).
#[test]
fn a_point_moves_by_the_requested_displacement() {
    let bytes = edit(
        "move",
        &[
            "--op",
            "move-point",
            "--stroke",
            "0",
            "--point",
            "0",
            "--dx",
            "5",
            "--dy",
            "-3",
        ],
    );
    assert_eq!(
        last_ink_list(&bytes),
        "[[15.0 7.0 20.0 30.0 25.0 35.0] [40.0 40.0 50.0 60.0]]",
        "(10,10) + (5,-3) = (15,7), and NOTHING else moves -- asserted whole rather than by `contains`, which a stray digit anywhere would satisfy"
    );
}

#[test]
fn a_whole_stroke_is_replaced_through_the_binary() {
    let bytes = edit(
        "replace",
        &[
            "--op",
            "replace-stroke",
            "--stroke",
            "1",
            "--points",
            "100,100;110,120;130,140",
        ],
    );
    assert_eq!(
        last_ink_list(&bytes),
        "[[10.0 10.0 20.0 30.0 25.0 35.0] [100.0 100.0 110.0 120.0 130.0 140.0]]",
        "stroke 1 is replaced and stroke 0 is untouched, both in one assertion"
    );
}

#[test]
fn a_stroke_is_removed_through_the_binary() {
    let bytes = edit("rmstroke", &["--op", "remove-stroke", "--stroke", "1"]);
    assert_eq!(
        last_ink_list(&bytes),
        "[[10.0 10.0 20.0 30.0 25.0 35.0]]",
        "one stroke left, and it is the one that was not named"
    );
}

// ------------------------------------------------------------ the refusals

/// The two index spaces are reported as two different questions, all the way
/// out to stderr.
#[test]
fn a_bad_stroke_index_and_a_bad_point_index_say_different_things() {
    let src = ink_file("badidx");
    let idx = ink_index(&src).to_string();
    let out = temp_path("badidx_out");

    let bad_stroke = run(&[
        "ink-edit",
        src.to_str().unwrap(),
        "--annot",
        &idx,
        "--op",
        "move-point",
        "--stroke",
        "9",
        "--point",
        "0",
        "--dx",
        "1",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(bad_stroke.status.code(), Some(EDIT_REFUSED));
    let msg = stderr(&bad_stroke);
    assert!(
        msg.contains("ink stroke(s)"),
        "the STROKE space must be named: {msg}"
    );

    let bad_point = run(&[
        "ink-edit",
        src.to_str().unwrap(),
        "--annot",
        &idx,
        "--op",
        "move-point",
        "--stroke",
        "1",
        "--point",
        "9",
        "--dx",
        "1",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(bad_point.status.code(), Some(EDIT_REFUSED));
    let msg = stderr(&bad_point);
    assert!(
        msg.contains("point(s)") && msg.contains("stroke 1"),
        "the POINT space must be named, and so must the stroke it is inside: {msg}"
    );
}

/// `--op insert-point` without `--at` is told about the ARGUMENT, not about
/// the document — the same discipline `edit-text --span-from-pin` follows.
#[test]
fn insert_without_at_is_told_about_the_argument() {
    let src = ink_file("noat");
    let idx = ink_index(&src).to_string();
    let out = temp_path("noat_out");
    let r = run(&[
        "ink-edit",
        src.to_str().unwrap(),
        "--annot",
        &idx,
        "--op",
        "insert-point",
        "--stroke",
        "0",
        "--point",
        "0",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(EDIT_REFUSED));
    assert!(
        stderr(&r).contains("--at"),
        "name the missing flag: {}",
        stderr(&r)
    );
}

/// The point floor, end to end, with the remedy named in the operator's own
/// vocabulary — `--op remove-stroke` is the CLI spelling of the verb the
/// engine's message names.
#[test]
fn the_point_floor_is_refused_through_the_binary() {
    let src = ink_file("floor");
    let idx = ink_index(&src).to_string();
    let out = temp_path("floor_out");
    let r = run(&[
        "ink-edit",
        src.to_str().unwrap(),
        "--annot",
        &idx,
        "--op",
        "remove-point",
        "--stroke",
        "1",
        "--point",
        "0",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(EDIT_REFUSED));
    assert!(
        stderr(&r).contains("RemoveStroke"),
        "the refusal must name the remedy: {}",
        stderr(&r)
    );
}

// -------------------------------------------------------------- the report

/// `--dry-run` writes nothing and answers the same counts the real run does.
#[test]
fn dry_run_reports_and_writes_nothing() {
    let src = ink_file("dry");
    let idx = ink_index(&src).to_string();
    let out = temp_path("dry_out");
    let r = run(&[
        "ink-edit",
        src.to_str().unwrap(),
        "--annot",
        &idx,
        "--op",
        "remove-point",
        "--stroke",
        "0",
        "--point",
        "1",
        "--dry-run",
    ]);
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));
    let line = stdout(&r);
    assert!(line.contains("dry_run=1"), "{line}");
    assert!(line.contains("points_before=5"), "{line}");
    assert!(line.contains("points_after=4"), "{line}");
    assert!(
        !out.exists(),
        "a dry run must not write the output it was not given"
    );
}

/// ★ The provenance disclosure: this ink is pdfcer's own, so the report says
/// so and stderr stays quiet about replaced artwork.
///
/// The complement — a foreign `/AP` producing the warning — is covered at core
/// level; what this pins is that the CLI does not cry wolf on the common case,
/// which is the failure mode that gets a disclosure ignored.
#[test]
fn the_report_says_the_artwork_was_ours_and_does_not_warn() {
    let src = ink_file("ours");
    let idx = ink_index(&src).to_string();
    let out = temp_path("ours_out");
    let r = run(&[
        "ink-edit",
        src.to_str().unwrap(),
        "--annot",
        &idx,
        "--op",
        "move-stroke",
        "--stroke",
        "0",
        "--dx",
        "3",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));
    assert!(
        stdout(&r).contains("appearance_was_ours=1"),
        "{}",
        stdout(&r)
    );
    assert!(
        !stderr(&r).contains("did NOT draw"),
        "pdfcer authored this ink two commands ago: {}",
        stderr(&r)
    );
}

/// `annotation-vertex` sends an `/Ink` here by name, so an operator who tries
/// the obvious command is not told the capability is missing.
#[test]
fn annotation_vertex_names_this_command_for_ink() {
    let src = ink_file("signpost");
    let idx = ink_index(&src).to_string();
    let out = temp_path("signpost_out");
    let r = run(&[
        "annotation-vertex",
        src.to_str().unwrap(),
        "--annot",
        &idx,
        "--op",
        "move",
        "--index",
        "0",
        "--dx",
        "1",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(EDIT_REFUSED));
    let msg = stderr(&r);
    assert!(
        msg.contains("reshape_ink") || msg.contains("move_ink_point"),
        "the refusal must name where the capability lives: {msg}"
    );
}

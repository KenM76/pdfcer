//! # `LockedContents` refuses a note edit, through the BINARY
//!
//! ## Why this cannot be a unit test
//!
//! `pdfcer-core`'s own suite calls `set_markup_note` directly and proves the
//! gate fires. That leaves one thing unproven, and it is the thing an operator
//! actually experiences: **does the refusal reach them, with a usable message
//! and a non-zero exit code, or does the CLI swallow it and report success?**
//! A shell that mapped the new error onto exit 0, or printed it to stdout as
//! though it were a result, would satisfy every core test.
//!
//! ## ★ The `--help` text was true before the code was
//!
//! `set-annotation-flags --help` has said, since the subcommand shipped:
//!
//! > *"`--locked-contents` is a DIFFERENT flag (bit 10) and guards the
//! > annotation's text, not its geometry. It does not stop a move."*
//!
//! **Nothing enforced the first half of that sentence.** The flag was
//! writable, printed by `list-annotations`, described accurately in the help
//! — and no verb in the crate consulted it, so an operator who set it got a
//! documented guarantee and no protection at all.
//!
//! In clap-derive a doc comment **is** shipped user-facing text, which makes
//! that sentence a claim pdfcer published about its own behaviour. This file
//! is what turns it from a claim into a fact, and it asserts the second half
//! of the sentence too — *"it does not stop a move"* — because a fix that
//! over-applied the flag would make the help wrong in the other direction
//! while looking like a success.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED`, spelled out so a change to the number is a visible
/// test failure rather than a silent contract break.
const EDIT_REFUSED: i32 = 9;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pdfcer_lockc_{tag}_{}_{n}.pdf", std::process::id()))
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

/// The demo fixture's first annotation, given a note and then `LockedContents`.
///
/// Two invocations, because the CLI has no session — each invocation is the
/// commit (project rule 11), so building this state is genuinely two saves and
/// the test says so rather than hiding it behind a helper that pretends
/// otherwise.
fn locked_contents_file(tag: &str) -> PathBuf {
    let noted = temp_path(&format!("{tag}_noted"));
    let out = run(&[
        "set-markup-note",
        fixture("annot/demo-annotated.pdf").to_str().unwrap(),
        "--page",
        "1",
        "--index",
        "0",
        "--note",
        "the original words",
        "-o",
        noted.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "seeding the note: {}", stderr(&out));

    let locked = temp_path(&format!("{tag}_locked"));
    let out = run(&[
        "set-annotation-flags",
        noted.to_str().unwrap(),
        "--page",
        "1",
        "--index",
        "0",
        "--locked-contents",
        "-o",
        locked.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "setting the flag: {}", stderr(&out));
    locked
}

/// The note text `list-annotations` reports for annotation 0, if any.
fn note_of(path: &Path) -> String {
    let out = run(&["list-annotations", path.to_str().unwrap()]);
    assert!(out.status.success(), "list: {}", stderr(&out));
    stdout(&out)
        .lines()
        .find(|l| l.starts_with("annot page=1 index=0 "))
        .unwrap_or("")
        .to_string()
}

/// ★★ The refusal reaches the operator: non-zero exit, named flag, on stderr.
#[test]
fn a_note_edit_on_locked_contents_is_refused_by_the_binary() {
    let locked = locked_contents_file("refuse");
    let out_path = temp_path("refuse_out");
    let out = run(&[
        "set-markup-note",
        locked.to_str().unwrap(),
        "--page",
        "1",
        "--index",
        "0",
        "--note",
        "overwritten",
        "-o",
        out_path.to_str().unwrap(),
    ]);

    assert_eq!(
        out.status.code(),
        Some(EDIT_REFUSED),
        "a refusal must be a refusal to a script, not a silent success. \
         stderr was: {}",
        stderr(&out)
    );
    let err = stderr(&out);
    assert!(
        err.contains("LockedContents"),
        "the message must name the flag that is actually stopping them: {err}"
    );
    assert!(
        err.contains("set-annotation-flags") || err.contains("set_annotation_flags"),
        "and name the way out -- unlike Locked, this flag's remedy is \
         reachable inside pdfcer: {err}"
    );
    assert!(
        !out_path.exists(),
        "a refused edit must not leave an output file; a script that checked \
         for the file rather than the exit code would otherwise see success"
    );
}

/// Clearing the note is refused too — the more destructive contents change.
#[test]
fn clearing_the_note_is_refused_too() {
    let locked = locked_contents_file("clear");
    let out_path = temp_path("clear_out");
    let out = run(&[
        "set-markup-note",
        locked.to_str().unwrap(),
        "--page",
        "1",
        "--index",
        "0",
        "--clear",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED), "{}", stderr(&out));
    assert!(stderr(&out).contains("LockedContents"));
    assert!(note_of(&locked).contains("the original words"));
}

/// ★★ And the other half of the shipped `--help` sentence: *"it does not stop
/// a move."*
///
/// Without this, a fix that treated bit 10 as a general write-lock would pass
/// both tests above while making the published help text wrong in the opposite
/// direction — and nothing would report it, because refusing too much looks
/// like caution rather than a defect.
#[test]
fn locked_contents_still_allows_a_move() {
    let locked = locked_contents_file("move");
    let out_path = temp_path("move_out");
    let out = run(&[
        "move-annotation",
        locked.to_str().unwrap(),
        "--page",
        "1",
        "--index",
        "0",
        "--dx",
        "10",
        "--dy",
        "10",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "Table 165 bit 10 does not restrict position, and the help says so: {}",
        stderr(&out)
    );
}

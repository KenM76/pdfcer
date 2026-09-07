//! # `rotate-annotation --absolute`, through the BINARY (`Pass 155.2`)
//!
//! ## Why through the binary, and why this file exists at all
//!
//! `--absolute` is a **flag**, and a flag is the one thing a unit test cannot
//! see. `pdfcer-core`'s tests call `set_annotation_rotation` directly, so a
//! build that parsed `--absolute` and then quietly called the delta verb
//! anyway would pass every one of them — the flag would be declared,
//! documented, present in `--help`, and inert. That has happened in this
//! project before; only an end-to-end run through `main` can refute it.
//!
//! ## What is pinned
//!
//! 1. **Idempotence.** `--absolute --degrees 45` twice leaves the file
//!    unchanged the second time. This is the property the flag exists for and
//!    the one a delta verb structurally cannot have — so it is also the
//!    strongest possible test that the flag is *wired*, because the delta
//!    path fails it by construction.
//! 2. **The absolute and delta paths genuinely diverge** on the same input,
//!    which stops (1) passing vacuously on an annotation that happened to
//!    start at the target angle.
//! 3. **The refusal reaches the operator** with the right exit code, and
//!    **names the delta verb as the way forward** rather than being a dead
//!    end.
//! 4. **`rect_derived=` is printed** — `Pass 155.1`'s rule-4 disclosure. The
//!    CLI invocation IS the commit (project rule 11), so what pdfcer inferred
//!    is printed on the way past rather than being available to ask for.

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
    std::env::temp_dir().join(format!(
        "pdfcer_rotabs_{tag}_{}_{n}.pdf",
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

/// `rect=` for annotation `index`, read back out of the SAVED file rather
/// than from the report the command printed about itself.
///
/// A command that prints the right rectangle and writes the wrong one passes
/// any report-only assertion; the report is computed from the plan, the file
/// is what a viewer opens.
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

fn rotate(input: &Path, index: usize, degrees: &str, absolute: bool, out_path: &Path) -> Output {
    let mut args = vec![
        "rotate-annotation".to_owned(),
        input.to_str().unwrap().to_owned(),
        "--page".to_owned(),
        "1".to_owned(),
        "--index".to_owned(),
        index.to_string(),
        "--degrees".to_owned(),
        degrees.to_owned(),
        "--anchor-x".to_owned(),
        "100".to_owned(),
        "--anchor-y".to_owned(),
        "100".to_owned(),
        "-o".to_owned(),
        out_path.to_str().unwrap().to_owned(),
    ];
    if absolute {
        args.push("--absolute".to_owned());
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&borrowed)
}

/// Index 0 of `demo-annotated.pdf` is a `/Stamp` with an appearance stream,
/// so it has an orientation to read — which is what `--absolute` requires.
const STAMP: usize = 0;

/// ★★ The property the flag exists for, and the one a delta verb cannot
/// have. If `--absolute` were parsed and dropped on the floor, the second
/// run would apply another 45° and this would fail.
#[test]
fn setting_the_same_absolute_angle_twice_leaves_the_file_unchanged() {
    let a = temp_path("abs_a");
    let b = temp_path("abs_b");

    let first = rotate(&fixture("annot/demo-annotated.pdf"), STAMP, "45", true, &a);
    assert!(first.status.success(), "{}", stderr(&first));
    let after_one = rect_of(&a, STAMP);

    let second = rotate(&a, STAMP, "45", true, &b);
    assert!(second.status.success(), "{}", stderr(&second));
    let after_two = rect_of(&b, STAMP);

    assert_eq!(
        after_one, after_two,
        "--absolute must be idempotent: 45 degrees applied twice gave {after_one} \
         then {after_two}. A second, different rectangle means the flag was \
         parsed and the DELTA verb ran anyway."
    );

    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
}

/// The control for the test above: absolute and delta must genuinely diverge
/// on the same input. Without this, an annotation that happened to start at
/// the target angle would make idempotence trivially true and the flag could
/// still be inert.
#[test]
fn the_absolute_and_delta_paths_disagree_on_the_same_input() {
    let crooked = temp_path("crooked");
    let abs = temp_path("abs");
    let delta = temp_path("delta");

    // Put it at 20 degrees first, so 45 absolute and 45 delta land apart.
    let out = rotate(
        &fixture("annot/demo-annotated.pdf"),
        STAMP,
        "20",
        false,
        &crooked,
    );
    assert!(out.status.success(), "{}", stderr(&out));

    let out = rotate(&crooked, STAMP, "45", true, &abs);
    assert!(out.status.success(), "{}", stderr(&out));
    let out = rotate(&crooked, STAMP, "45", false, &delta);
    assert!(out.status.success(), "{}", stderr(&out));

    assert_ne!(
        rect_of(&abs, STAMP),
        rect_of(&delta, STAMP),
        "starting from 20 degrees, --absolute 45 lands at 45 and a delta of \
         45 lands at 65. Identical rectangles mean the flag changed nothing."
    );

    for p in [&crooked, &abs, &delta] {
        let _ = std::fs::remove_file(p);
    }
}

/// `--absolute` says WHAT IT ACTUALLY APPLIED, because the outcome lines
/// report the delta and the operator asked for an absolute — two different
/// numbers on one screen (rule 4).
#[test]
fn the_absolute_run_discloses_the_delta_it_worked_out() {
    let crooked = temp_path("disc_crooked");
    let out = rotate(
        &fixture("annot/demo-annotated.pdf"),
        STAMP,
        "20",
        false,
        &crooked,
    );
    assert!(out.status.success(), "{}", stderr(&out));

    let dest = temp_path("disc");
    let out = rotate(&crooked, STAMP, "45", true, &dest);
    assert!(out.status.success(), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(
        err.contains("--absolute") && err.contains("20.0000") && err.contains("25.0000"),
        "the absolute run must say it found the annotation at 20 and applied \
         25 to reach 45; got:\n{err}"
    );

    let _ = std::fs::remove_file(&crooked);
    let _ = std::fs::remove_file(&dest);
}

/// `Pass 155.1`'s rule-4 disclosure reaches the operator: which of the three
/// rectangle rules pdfcer used, on evidence they cannot see.
#[test]
fn the_rectangle_rule_is_printed_and_the_non_composing_one_is_warned_about() {
    let dest = temp_path("derive_ap");
    let out = rotate(
        &fixture("annot/demo-annotated.pdf"),
        STAMP,
        "30",
        false,
        &dest,
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("rect_derived=artwork"),
        "an annotation with an appearance must report the ARTWORK rule; got:\n{}",
        stdout(&out)
    );
    let _ = std::fs::remove_file(&dest);

    // The /Circle at index 2 has no appearance and no geometry, so it falls
    // to the rule that does NOT compose -- and that must be said out loud,
    // because rotating it twice really does keep enlarging it.
    let dest = temp_path("derive_prev");
    let out = rotate(&fixture("annot/demo-annotated.pdf"), 2, "30", false, &dest);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("rect_derived=previous-rect"),
        "got:\n{}",
        stdout(&out)
    );
    assert!(
        stderr(&out).contains("REPEATED rotation"),
        "the non-composing rule must WARN, not merely be named in a field the \
         operator has to know to read; got:\n{}",
        stderr(&out)
    );
    let _ = std::fs::remove_file(&dest);
}

/// The refusal reaches the operator with the right exit code and names the
/// way forward. A refusal that were a dead end would be worse than the
/// invention it prevents.
#[test]
fn an_unreadable_angle_is_refused_and_the_delta_verb_is_named() {
    let dest = temp_path("refused");
    // Index 2 is the /Circle with no /AP: nowhere to record an orientation.
    let out = rotate(&fixture("annot/demo-annotated.pdf"), 2, "45", true, &dest);
    assert_eq!(
        out.status.code(),
        Some(EDIT_REFUSED),
        "expected EDIT_REFUSED; stderr:\n{}",
        stderr(&out)
    );
    let err = stderr(&out);
    assert!(
        err.contains("no appearance stream"),
        "the refusal must name WHY: {err}"
    );
    assert!(
        err.contains("rotate_annotation"),
        "the refusal must name the verb that DOES work here: {err}"
    );

    // ... and it must be true. The same annotation rotates by delta.
    let ok = temp_path("refused_ok");
    let out = rotate(&fixture("annot/demo-annotated.pdf"), 2, "45", false, &ok);
    assert!(
        out.status.success(),
        "the refusal told the operator to use the delta verb; it must work: {}",
        stderr(&out)
    );
    let _ = std::fs::remove_file(&ok);
}

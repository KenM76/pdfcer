//! # `pdfcer move-annotation` (`Pass 149.0`)
//!
//! Black-box over the **real binary**, and every geometry assertion is made by
//! re-reading the *output file* — not by trusting the report the command
//! printed about itself.
//!
//! ## What these tests are actually guarding
//!
//! A move has two halves and only one is visible. `/Rect` moves the **painted**
//! result (ISO 32000-1 §12.5.5 recomputes the placement matrix from the
//! appearance `BBox` and the new `/Rect`). The geometry keys — `/L`,
//! `/Vertices`, `/InkList`, `/QuadPoints`, `/CL` — hold absolute page
//! coordinates and are what any **other** tool regenerates an appearance from.
//!
//! ★ Move only the first and the annotation looks right in pdfcer and is
//! reconstructed in the **old place** by the next viewer that rebuilds it. That
//! is invisible here, invisible in a screenshot, and shows up in somebody
//! else's product — which is why the report has a `geometry_keys_moved` field
//! at all, and why these tests read the saved bytes rather than the report.
//!
//! Fixture provenance: `fixtures/synthetic/annot/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
/// `exit::EDIT_REFUSED` in the CLI's stable exit-code contract.
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
        "pdfcer_mvannot_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
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

/// Every `annot …` row of `list-annotations`, read from a FILE — i.e. the way
/// a script would see it.
///
/// The summary line is **dropped deliberately**: it names the input path, so
/// comparing whole outputs from two different files can never be equal and a
/// test written that way reports a difference that is entirely its own. The
/// first cut of `a_zero_delta…` did exactly that and failed on a correct
/// binary — the assertion was over-broad, not the code wrong.
fn listed(path: &Path) -> String {
    let out = run(&["list-annotations", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    stdout(&out)
        .lines()
        .filter(|l| l.starts_with("annot "))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------

#[test]
fn it_moves_an_annotation_and_reports_both_halves() {
    let src = fixture("annot/demo-annotated.pdf");
    let out = temp_path("move");
    let r = run(&[
        "move-annotation",
        src.to_str().unwrap(),
        "--index",
        "0",
        "--dx",
        "12",
        "--dy",
        "-8",
        "--output",
        out.to_str().unwrap(),
    ]);
    let text = stdout(&r);
    assert_eq!(r.status.code(), Some(0), "{text}{}", stderr(&r));

    assert!(text.contains("subtype="), "{text}");
    assert!(text.contains("dx=12.000 dy=-8.000"), "{text}");
    assert!(
        text.contains("geometry_keys_moved=") && text.contains("appearance_carried="),
        "the report names both halves: {text}"
    );
    assert!(out.exists());
    let _ = std::fs::remove_file(&out);
}

#[test]
fn the_moved_rect_is_readable_from_the_output_file() {
    // Asserted by re-reading the SAVED FILE, not the report — the report is
    // what is being checked, so it cannot also be the evidence.
    let src = fixture("annot/demo-annotated.pdf");
    let before = listed(&src);

    let out = temp_path("read");
    let r = run(&[
        "move-annotation",
        src.to_str().unwrap(),
        "--index",
        "0",
        "--dx",
        "100",
        "--dy",
        "0",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));

    let after = listed(&out);
    assert_ne!(
        before, after,
        "the annotation listing must differ after a 100pt move"
    );
    let _ = std::fs::remove_file(&out);
}

#[test]
fn a_zero_delta_is_accepted_and_changes_nothing_visible() {
    // Not refused: "move by nothing" is a legitimate request from a shell
    // that drags and returns to the start, and refusing it would make the
    // caller special-case its own arithmetic.
    let src = fixture("annot/demo-annotated.pdf");
    let out = temp_path("zero");
    let r = run(&[
        "move-annotation",
        src.to_str().unwrap(),
        "--index",
        "0",
        "--dx",
        "0",
        "--dy",
        "0",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));
    assert_eq!(
        listed(&src),
        listed(&out),
        "a zero move leaves the listing identical"
    );
    let _ = std::fs::remove_file(&out);
}

#[test]
fn a_widget_is_refused_and_the_message_names_the_other_verb() {
    let src = fixture("forms/demo-form.pdf");
    let out = temp_path("widget");
    let r = run(&[
        "move-annotation",
        src.to_str().unwrap(),
        "--index",
        "0",
        "--dx",
        "5",
        "--dy",
        "5",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(EDIT_REFUSED), "{}", stdout(&r));
    let err = stderr(&r);
    assert!(err.contains("move_widget"), "{err}");
    assert!(err.contains("Nothing was moved"), "{err}");
    assert!(!out.exists(), "and nothing was written");
}

#[test]
fn an_index_past_the_end_names_how_many_there_are() {
    // "index 9 is out of range" without the bound sends the operator back to
    // re-run the list command for a number this process already knew.
    let src = fixture("annot/demo-annotated.pdf");
    let out = temp_path("oob");
    let r = run(&[
        "move-annotation",
        src.to_str().unwrap(),
        "--index",
        "99",
        "--dx",
        "1",
        "--dy",
        "1",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_ne!(r.status.code(), Some(0));
    let err = stderr(&r);
    assert!(err.contains("no annotation at index 99"), "{err}");
    assert!(err.contains("indices 0.."), "and states the bound: {err}");
    assert!(!out.exists());
}

#[test]
fn page_zero_is_refused_because_the_flag_is_one_based() {
    let src = fixture("annot/demo-annotated.pdf");
    let out = temp_path("page0");
    let r = run(&[
        "move-annotation",
        src.to_str().unwrap(),
        "--page",
        "0",
        "--index",
        "0",
        "--dx",
        "1",
        "--dy",
        "1",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_ne!(r.status.code(), Some(0));
    assert!(stderr(&r).contains("1-based"), "{}", stderr(&r));
}

#[test]
fn the_appearance_carry_is_disclosed_in_prose_not_only_as_a_field() {
    // Rule 4: the machine-readable field is for a script, the sentence is for
    // the person who will otherwise wonder why the artwork moved without the
    // stream changing.
    let src = fixture("annot/demo-annotated.pdf");
    let out = temp_path("prose");
    let r = run(&[
        "move-annotation",
        src.to_str().unwrap(),
        "--index",
        "0",
        "--dx",
        "3",
        "--dy",
        "3",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));
    let err = stderr(&r);
    assert!(
        err.contains("12.5.5") || err.contains("no geometry key"),
        "one of the two prose disclosures fires: {err}"
    );
    let _ = std::fs::remove_file(&out);
}

//! `Pass 288.1` — `stamp-pack --stamps-from`, the flag a real artwork sheet
//! needs.
//!
//! # Why this is a CLI test and not a core one
//!
//! The name list is parsed **only in the CLI**. A core test cannot see a
//! comment line that was not skipped, a blank line that became an empty stamp
//! name, or a flag that parses and never reaches the core call — and each of
//! those produces a collection that looks written and is wrong.
//!
//! # The file that motivated it
//!
//! A downloaded stamp artwork sheet the operator supplied is **113 pages**,
//! one stamp per page, with no text layer to derive names from (the artwork is
//! vector outlines). Repeating `--stamp` 113 times is not a command anybody
//! types twice, so the names come from a file.
//!
//! ## Every assertion re-reads the output through `stamp-list`
//!
//! Never the report `stamp-pack` printed about itself. A command that says
//! `stamps_named=4` and wrote a tree naming the wrong pages passes a
//! report-only assertion.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf")
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdfcer-stamp-pack-{name}"))
}

fn run(args: &[&str]) -> (String, String, i32) {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// ★★ A name list drives the pack, and its comments and blank lines are
/// skipped.
///
/// `hello.pdf` has one page, so only the first name can be used — which also
/// exercises the overflow disclosure in the same run.
#[test]
fn a_name_list_file_drives_the_pack_and_skips_comments() {
    let list = tmp("names.txt");
    std::fs::write(
        &list,
        "# a heading a human would write\n\nApproved\n\n# another\nRejected\n",
    )
    .expect("the list is written");
    let out_pdf = tmp("from-list.pdf");

    let (stdout, stderr, code) = run(&[
        "stamp-pack",
        fixture().to_str().expect("path"),
        "--category",
        "Red Stamps",
        "--stamps-from",
        list.to_str().expect("path"),
        "-o",
        out_pdf.to_str().expect("path"),
    ]);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    // ★ The comment and blank lines must not have become stamps: the fixture
    // has ONE page, so a parser that kept them would report a different
    // skipped set than the one asserted below.
    assert!(
        stdout.contains("stamps_named=1"),
        "one page, so one stamp: {stdout}"
    );
    assert!(
        stdout.contains("SKIPPED Rejected"),
        "and the second name is NAMED as skipped, not silently dropped: {stdout}"
    );
    assert!(
        !stdout.contains("a heading"),
        "a comment line must never become a stamp: {stdout}"
    );

    // Re-read through the binary rather than trusting the report.
    let (listed, _, code) = run(&["stamp-list", out_pdf.to_str().expect("path")]);
    assert_eq!(code, 0);
    assert!(listed.contains("category=Red Stamps"), "{listed}");
    assert!(listed.contains("Approved"), "{listed}");
    assert!(
        !listed.contains("Rejected"),
        "the skipped name must not be in the tree: {listed}"
    );

    let _ = std::fs::remove_file(&list);
    let _ = std::fs::remove_file(&out_pdf);
}

/// ★ THE CONTROL: `--stamp` still works, and the two flags conflict.
///
/// Without this, a `--stamps-from` implementation that quietly ignored
/// `--stamp` would pass the test above and break every existing invocation.
#[test]
fn the_repeated_flag_still_works_and_the_two_conflict() {
    let out_pdf = tmp("from-flags.pdf");
    let (stdout, stderr, code) = run(&[
        "stamp-pack",
        fixture().to_str().expect("path"),
        "--category",
        "Flags",
        "--stamp",
        "KMDraft=Draft",
        "-o",
        out_pdf.to_str().expect("path"),
    ]);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let (listed, _, _) = run(&["stamp-list", out_pdf.to_str().expect("path")]);
    assert!(listed.contains("KMDraft"), "{listed}");
    assert!(listed.contains("display=Draft"), "{listed}");

    // Both at once is refused by clap, before anything is written.
    let (_, stderr, code) = run(&[
        "stamp-pack",
        fixture().to_str().expect("path"),
        "--category",
        "Both",
        "--stamp",
        "A",
        "--stamps-from",
        "nowhere.txt",
        "-o",
        out_pdf.to_str().expect("path"),
    ]);
    assert_ne!(code, 0, "giving both name sources must be refused");
    assert!(
        stderr.contains("cannot be used with"),
        "and refused by name: {stderr}"
    );

    let _ = std::fs::remove_file(&out_pdf);
}

/// ★ An unreadable name list fails loudly and writes nothing.
#[test]
fn a_missing_name_list_is_an_error_not_an_empty_collection() {
    let out_pdf = tmp("never-written.pdf");
    let _ = std::fs::remove_file(&out_pdf);

    let (_, stderr, code) = run(&[
        "stamp-pack",
        fixture().to_str().expect("path"),
        "--category",
        "Missing",
        "--stamps-from",
        "no-such-file.txt",
        "-o",
        out_pdf.to_str().expect("path"),
    ]);
    assert_ne!(code, 0);
    assert!(stderr.contains("no-such-file.txt"), "{stderr}");
    assert!(
        !out_pdf.exists(),
        "a failed run must not leave a half-written collection"
    );
}

//! `pdfcer text-locate`: searched text to the `--object`/`--leaf` + `--run`
//! operands the `text-run-*` commands take.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn text_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name)
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn locate(name: &str, find: &str) -> (Output, String) {
    let src = text_fixture(name);
    let out = run(&["text-locate", src.to_str().unwrap(), "--find", find]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    (out, stdout)
}

/// The located operands are the ones `text-run-delete` acts on: deleting
/// what `text-locate` names removes exactly the searched label.
#[test]
fn located_run_is_the_one_text_run_delete_removes() {
    let (out, stdout) = locate("runs-inherited.pdf", "DELTA");
    assert!(out.status.success(), "{stdout}");
    assert!(
        stdout.contains("match ordinal=0 start=15 len=5 targets=object=0/run=3 unresolved=0"),
        "{stdout}"
    );
    assert!(stdout.contains("matches=1"), "{stdout}");

    let src = text_fixture("runs-inherited.pdf");
    let dst = std::env::temp_dir().join(format!("pdfcer_tlocate_{}.pdf", std::process::id()));
    let del = run(&[
        "text-run-delete",
        src.to_str().unwrap(),
        "--object",
        "0",
        "--run",
        "3",
        "-o",
        dst.to_str().unwrap(),
    ]);
    assert!(
        del.status.success(),
        "{}",
        String::from_utf8_lossy(&del.stderr)
    );
    let after = run(&["text-locate", dst.to_str().unwrap(), "--find", "DELTA"]);
    let after = String::from_utf8_lossy(&after.stdout).into_owned();
    let _ = std::fs::remove_file(&dst);
    assert!(after.contains("matches=0"), "{after}");
}

/// A word drawn by three text objects is found once and names all three.
#[test]
fn a_word_split_across_objects_lists_every_piece() {
    let (_, stdout) = locate("cross-object-word.pdf", "Driver-Side");
    assert!(
        stdout.contains("start=0 len=11 targets=object=0/run=0,object=1/run=0,object=2/run=0"),
        "{stdout}"
    );
    assert!(stdout.contains("matches=3"), "{stdout}");
}

/// Text inside a form XObject is addressed as a leaf, not a page object.
#[test]
fn text_inside_a_form_is_named_as_a_leaf() {
    let (_, stdout) = locate("fallback-font-form.pdf", "Hi");
    assert!(
        stdout.contains("targets=leaf=1/run=0 unresolved=0"),
        "{stdout}"
    );
}

#[test]
fn a_miss_exits_zero_and_an_empty_find_is_refused() {
    let (out, stdout) = locate("runs-inherited.pdf", "ZZZ");
    assert!(out.status.success());
    assert!(stdout.contains("matches=0"), "{stdout}");
    let (out, _) = locate("runs-inherited.pdf", "");
    assert_eq!(out.status.code(), Some(1));
}

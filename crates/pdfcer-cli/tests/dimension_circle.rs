//! `dimension-circle` and the `--circle-*` style flags over the real binary
//! (pdfcer-gui request G329): what is saved, and what `dimension-list` reads
//! back from the file.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED`, spelled out so a change to the number is visible.
const EDIT_REFUSED: i32 = 9;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf")
}

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-circle-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN).args(args).output().expect("pdfcer runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn s(p: &Path) -> &str {
    p.to_str().expect("utf-8 path")
}

/// One ce dimension of `kind` in a fresh copy of the fixture; it is id 0.
fn with(kind: &str, points: &str, name: &str) -> PathBuf {
    let out = temp_out(name);
    let (code, o, e) = run(&[
        "dimension-add",
        s(&fixture()),
        "--kind",
        kind,
        "--points",
        points,
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{o}\n{e}");
    out
}

fn radius(name: &str) -> PathBuf {
    with("radius", "250,200 200,250 150,200", name)
}

fn listed(path: &Path, extra: &[&str]) -> String {
    let mut args = vec!["dimension-list", s(path)];
    args.extend_from_slice(extra);
    run(&args).1
}

#[test]
fn a_hidden_circle_lists_as_hidden_and_shows_again() {
    let src = radius("src.pdf");
    let hidden = temp_out("hidden.pdf");
    let (code, out, err) = run(&[
        "dimension-circle",
        s(&src),
        "--dimension",
        "0",
        "--circle",
        "hidden",
        "--verify-undo",
        "-o",
        s(&hidden),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("circle=hidden") && out.contains("undo_identical=1"),
        "{out}"
    );
    assert!(listed(&hidden, &[]).contains("circle=hidden"));
    assert!(!listed(&src, &[]).contains("circle=hidden"));

    let shown = temp_out("shown.pdf");
    let (code, _, err) = run(&[
        "dimension-circle",
        s(&hidden),
        "--dimension",
        "0",
        "--circle",
        "shown",
        "-o",
        s(&shown),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(!listed(&shown, &[]).contains("circle=hidden"));
}

#[test]
fn a_linear_ce_dimension_is_refused_and_nothing_is_written() {
    let src = with("linear", "100,100 300,100", "linear.pdf");
    let out = temp_out("refused.pdf");
    let (code, _, err) = run(&[
        "dimension-circle",
        s(&src),
        "--dimension",
        "0",
        "--circle",
        "hidden",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, EDIT_REFUSED, "{err}");
    assert!(!out.exists());
}

#[test]
fn circle_style_flags_save_list_and_clear_by_tier() {
    let src = radius("styled-src.pdf");
    let styled = temp_out("styled.pdf");
    let (code, _, err) = run(&[
        "dimension-style",
        s(&src),
        "--dimension",
        "0",
        "--circle-dash",
        "6,2",
        "--circle-width",
        "0.35",
        "-o",
        s(&styled),
    ]);
    assert_eq!(code, 0, "{err}");
    let grouped = temp_out("grouped.pdf");
    let (code, _, err) = run(&[
        "group-style",
        s(&styled),
        "--group",
        "0",
        "--circle-color",
        "0,0,1",
        "-o",
        s(&grouped),
    ]);
    assert_eq!(code, 0, "{err}");
    let list = listed(&grouped, &["--style"]);
    assert!(list.contains("style circle-dash=6,2 (dimension)"), "{list}");
    assert!(
        list.contains("style circle-width=0.35 (dimension)"),
        "{list}"
    );
    assert!(list.contains("style circle-color=0,0,1 (group)"), "{list}");

    let cleared = temp_out("cleared.pdf");
    let (code, _, err) = run(&[
        "dimension-style",
        s(&grouped),
        "--dimension",
        "0",
        "--clear",
        "circle-width",
        "-o",
        s(&cleared),
    ]);
    assert_eq!(code, 0, "{err}");
    let list = listed(&cleared, &["--style"]);
    assert!(list.contains("style circle-width=0.75 (factory)"), "{list}");
    assert!(list.contains("style circle-dash=6,2 (dimension)"), "{list}");
}

#[test]
fn a_non_positive_circle_width_is_refused_by_name() {
    let src = radius("bad-width.pdf");
    let out = temp_out("bad-width-out.pdf");
    let (code, _, err) = run(&[
        "dimension-style",
        s(&src),
        "--dimension",
        "0",
        "--circle-width",
        "0",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, EDIT_REFUSED);
    assert!(err.contains("--circle-width"), "{err}");
}

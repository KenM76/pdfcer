//! The `--centre-mark-gap`, `--centre-mark-extension` and
//! `--centre-mark-line-font` style flags and the `extension_lines_drawn=`
//! disclosure over the real binary (pdfcer-gui request G333).

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf")
}

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-mark-reach-tests-{}", std::process::id()));
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

/// A standalone centre mark with extension lines on a 50 pt circle: id 0.
fn mark(name: &str) -> PathBuf {
    let out = temp_out(name);
    let (code, o, e) = run(&[
        "centre-mark",
        s(&fixture()),
        "--circle",
        "200,200,50",
        "--extension-lines",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{o}\n{e}");
    out
}

fn style(src: &Path, name: &str, args: &[&str]) -> (PathBuf, String) {
    let out = temp_out(name);
    let mut a = vec!["dimension-style", s(src), "--dimension", "0"];
    a.extend_from_slice(args);
    a.extend_from_slice(&["-o", s(&out)]);
    let (code, o, e) = run(&a);
    assert_eq!(code, 0, "{o}\n{e}");
    (out, o)
}

fn style_line<'a>(listing: &'a str, prop: &str) -> &'a str {
    let key = format!("style {prop}=");
    listing
        .lines()
        .map(str::trim_start)
        .find(|l| l.starts_with(&key))
        .unwrap_or_else(|| panic!("`{prop}` not listed:\n{listing}"))
}

#[test]
fn reach_flags_set_list_and_clear() {
    let src = mark("src.pdf");
    let (_, list, _) = run(&["dimension-list", s(&src), "--style"]);
    assert!(list.contains("extension_lines_drawn=true"), "{list}");
    assert_eq!(
        style_line(&list, "centre-mark-gap"),
        "style centre-mark-gap=5 (factory)"
    );
    let (styled, _) = style(
        &src,
        "styled.pdf",
        &[
            "--centre-mark-gap",
            "fixed:3",
            "--centre-mark-extension",
            "text:0.2",
            "--centre-mark-line-font",
            "true",
        ],
    );
    let (_, list, _) = run(&["dimension-list", s(&styled), "--style"]);
    assert_eq!(
        style_line(&list, "centre-mark-gap"),
        "style centre-mark-gap=3 (dimension)"
    );
    assert_eq!(
        style_line(&list, "centre-mark-extension"),
        "style centre-mark-extension=2 (dimension)"
    );
    assert_eq!(
        style_line(&list, "centre-mark-line-font"),
        "style centre-mark-line-font=true (dimension)"
    );
    let (cleared, _) = style(
        &styled,
        "cleared.pdf",
        &["--clear", "centre-mark-gap,centre-mark-line-font"],
    );
    let (_, list, _) = run(&["dimension-list", s(&cleared), "--style"]);
    assert!(style_line(&list, "centre-mark-gap").ends_with("(factory)"));
    assert!(style_line(&list, "centre-mark-line-font").ends_with("=false (factory)"));
    assert!(style_line(&list, "centre-mark-extension").ends_with("(dimension)"));
}

/// Would catch: the extension lines silently vanishing when the gap reaches
/// past the rim.
#[test]
fn a_gap_past_the_rim_is_listed_as_not_drawn() {
    let src = mark("far.pdf");
    let (far, _) = style(&src, "far-out.pdf", &["--centre-mark-gap", "fixed:80"]);
    let (_, list, _) = run(&["dimension-list", s(&far)]);
    assert!(
        list.contains("extension_lines=on") && list.contains("extension_lines_drawn=false"),
        "{list}"
    );
}

#[test]
fn a_bad_gap_is_refused_by_name() {
    let src = mark("bad.pdf");
    let out = temp_out("bad-out.pdf");
    let (code, _, err) = run(&[
        "dimension-style",
        s(&src),
        "--dimension",
        "0",
        "--centre-mark-extension",
        "fixed:-1",
        "-o",
        s(&out),
    ]);
    assert_ne!(code, 0);
    assert!(err.contains("--centre-mark-extension"), "{err}");
    assert!(!out.exists());
}

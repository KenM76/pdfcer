//! The `--centre-line-*` / `--centre-mark-*` style flags and the centre
//! line's `dash_fit=` disclosure over the real binary (pdfcer-gui request
//! G334).

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf")
}

fn temp_out(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("pdfcer-centre-stroke-tests-{}", std::process::id()));
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

/// A centre line between two parallel lines 168 pt long: id 0.
fn centre_line(name: &str) -> (PathBuf, String) {
    let out = temp_out(name);
    let (code, o, e) = run(&[
        "centre-line",
        s(&fixture()),
        "--points",
        "100,100 300,100  150,140 350,140",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{o}\n{e}");
    (out, o)
}

fn style(src: &Path, name: &str, args: &[&str]) -> PathBuf {
    let out = temp_out(name);
    let mut a = vec!["dimension-style", s(src), "--dimension", "0"];
    a.extend_from_slice(args);
    a.extend_from_slice(&["-o", s(&out)]);
    let (code, o, e) = run(&a);
    assert_eq!(code, 0, "{o}\n{e}");
    out
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
fn centre_line_prints_and_lists_its_dash_fit() {
    let (out, printed) = centre_line("fit.pdf");
    assert!(
        printed.contains("centre-line dash_fit=stretched"),
        "{printed}"
    );
    let (_, list, _) = run(&["dimension-list", s(&out)]);
    assert!(list.contains("dash_fit=stretched"), "{list}");
}

#[test]
fn centre_line_flags_set_list_and_clear() {
    let (src, _) = centre_line("src.pdf");
    let styled = style(
        &src,
        "styled.pdf",
        &[
            "--centre-line-dash",
            "12,3",
            "--centre-line-width",
            "0.25",
            "--centre-mark-color",
            "0,0,1",
        ],
    );
    let (_, list, _) = run(&["dimension-list", s(&styled), "--style"]);
    assert_eq!(
        style_line(&list, "centre-line-dash"),
        "style centre-line-dash=12,3 (dimension)"
    );
    assert_eq!(
        style_line(&list, "centre-line-width"),
        "style centre-line-width=0.25 (dimension)"
    );
    assert!(style_line(&list, "centre-mark-color").ends_with("(dimension)"));
    assert!(style_line(&list, "centre-mark-dash").ends_with("(factory)"));
    let cleared = style(
        &styled,
        "cleared.pdf",
        &["--clear", "centre-line-dash,centre-line-width"],
    );
    let (_, list, _) = run(&["dimension-list", s(&cleared), "--style"]);
    assert!(style_line(&list, "centre-line-dash").ends_with("(factory)"));
    assert!(style_line(&list, "centre-line-width").ends_with("(factory)"));
    assert!(style_line(&list, "centre-mark-color").ends_with("(dimension)"));
}

/// Would catch: `--dash` reaching a centre line (the doc's old sentence).
#[test]
fn a_main_dash_does_not_change_the_centre_line() {
    let (src, _) = centre_line("plain.pdf");
    let (_, before, _) = run(&["dimension-list", s(&src), "--style"]);
    let dashed = style(&src, "dashed.pdf", &["--dash", "6,2"]);
    let (_, after, _) = run(&["dimension-list", s(&dashed), "--style"]);
    assert_eq!(
        style_line(&before, "centre-line-dash"),
        style_line(&after, "centre-line-dash")
    );
    assert!(style_line(&after, "dash").contains("6,2"));
}

#[test]
fn a_non_positive_centre_width_is_refused_by_name() {
    let (src, _) = centre_line("bad.pdf");
    let out = temp_out("bad-out.pdf");
    let (code, _, err) = run(&[
        "dimension-style",
        s(&src),
        "--dimension",
        "0",
        "--centre-mark-width",
        "0",
        "-o",
        s(&out),
    ]);
    assert_ne!(code, 0);
    assert!(err.contains("centre-mark-width"), "{err}");
    assert!(!out.exists());
}

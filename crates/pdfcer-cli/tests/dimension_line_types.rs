//! `--dash <line type>`, `--line-type-scale` and their listing over the real
//! binary (pdfcer-gui request G331).

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf")
}

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-line-type-tests-{}", std::process::id()));
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

fn linear(name: &str) -> PathBuf {
    let out = temp_out(name);
    let (code, o, e) = run(&[
        "dimension-add",
        s(&fixture()),
        "--kind",
        "linear",
        "--points",
        "100,100 300,100",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{o}\n{e}");
    out
}

fn style(src: &Path, name: &str, args: &[&str]) -> (i32, PathBuf, String) {
    let out = temp_out(name);
    let mut a = vec!["dimension-style", s(src), "--dimension", "0"];
    a.extend_from_slice(args);
    a.extend_from_slice(&["-o", s(&out)]);
    let (code, _, e) = run(&a);
    (code, out, e)
}

fn listing(p: &Path) -> String {
    let (code, o, e) = run(&["dimension-list", s(p), "--style"]);
    assert_eq!(code, 0, "{e}");
    o
}

#[test]
fn a_named_dash_and_scale_are_listed_and_cleared() {
    let src = linear("plain.pdf");
    let before = listing(&src);
    assert!(
        before.contains("style line-type-scale=1 (factory)"),
        "{before}"
    );
    assert!(
        before.contains("style centre-line-dash=center (factory)"),
        "{before}"
    );
    let (code, styled, e) = style(
        &src,
        "hidden.pdf",
        &["--dash", "Hidden", "--line-type-scale", "2"],
    );
    assert_eq!(code, 0, "{e}");
    let list = listing(&styled);
    assert!(list.contains("style dash=hidden (dimension)"), "{list}");
    assert!(
        list.contains("style line-type-scale=2 (dimension)"),
        "{list}"
    );
    let (code, cleared, e) = style(&styled, "cleared.pdf", &["--clear", "line-type-scale"]);
    assert_eq!(code, 0, "{e}");
    let list = listing(&cleared);
    assert!(list.contains("style line-type-scale=1 (factory)"), "{list}");
    assert!(list.contains("style dash=hidden (dimension)"), "{list}");
}

#[test]
fn a_group_line_type_reaches_the_member() {
    let src = linear("g.pdf");
    let out = temp_out("grouped.pdf");
    let (code, _, e) = run(&[
        "group-style",
        s(&src),
        "--group",
        "0",
        "--dash",
        "phantom",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{e}");
    assert!(listing(&out).contains("style dash=phantom (group)"));
}

/// Would catch: an unknown word parsed as an empty run list, or a zero scale
/// written to the file.
#[test]
fn a_bad_name_or_scale_is_refused() {
    let src = linear("bad.pdf");
    let (code, out, e) = style(&src, "badname.pdf", &["--dash", "dashdot"]);
    assert_ne!(code, 0);
    assert!(
        e.contains("not a line type") && e.contains("phantom"),
        "{e}"
    );
    assert!(!out.exists());
    for bad in ["0", "-2"] {
        let (code, out, e) = style(&src, "badscale.pdf", &[&format!("--line-type-scale={bad}")]);
        assert_ne!(code, 0, "{bad}");
        assert!(e.contains("line-type-scale"), "{e}");
        assert!(!out.exists());
    }
}

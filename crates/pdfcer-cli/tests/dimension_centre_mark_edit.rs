//! `dimension-centre-mark` over the real binary (pdfcer-gui request G330):
//! what is saved, what `dimension-list` reads back, and what is refused.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED`, spelled out so a change to the number is visible.
const EDIT_REFUSED: i32 = 9;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf")
}

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-mark-edit-tests-{}", std::process::id()));
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

fn mark(src: &Path, out: &Path, extra: &[&str]) -> (i32, String, String) {
    let mut args = vec!["dimension-centre-mark", s(src), "--dimension", "0"];
    args.extend_from_slice(extra);
    args.extend_from_slice(&["-o", s(out)]);
    run(&args)
}

fn listed(path: &Path) -> String {
    run(&["dimension-list", s(path)]).1
}

#[test]
fn a_radius_gains_a_mark_discloses_the_moved_label_and_loses_it_again() {
    let src = with("radius", "250,200 200,250 150,200", "src.pdf");
    let marked = temp_out("marked.pdf");
    let (code, out, err) = mark(
        &src,
        &marked,
        &["--extension-lines", "--angle", "-15", "--verify-undo"],
    );
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("label_moved=1") && out.contains("undo_identical=1"),
        "{out}"
    );
    assert!(out.contains("label moved outside the rim"), "{out}");
    let list = listed(&marked);
    assert!(
        list.contains("centre_mark=on extension_lines=on mark_angle=-15"),
        "{list}"
    );

    let cleared = temp_out("cleared.pdf");
    let (code, out, err) = mark(&marked, &cleared, &["--clear"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("centre_mark=none label_moved=1"), "{out}");
    assert!(!listed(&cleared).contains("centre_mark=on"));
}

#[test]
fn a_linear_is_refused_and_nothing_is_written() {
    let src = with("linear", "100,100 300,100", "linear.pdf");
    let out = temp_out("refused.pdf");
    let (code, _, err) = mark(&src, &out, &[]);
    assert_eq!(code, EDIT_REFUSED, "{err}");
    assert!(err.contains("no centre to mark"), "{err}");
    assert!(!out.exists());
}

#[test]
fn clear_conflicts_with_a_spec() {
    let src = with("radius", "250,200 200,250 150,200", "conflict.pdf");
    let out = temp_out("conflict-out.pdf");
    let (code, _, _) = mark(&src, &out, &["--clear", "--extension-lines"]);
    assert_ne!(code, 0);
    assert!(!out.exists());
}

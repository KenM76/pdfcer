//! `dimension-extension-gap` (`Pass 369.0`): set, refuse and clear one end's
//! extension-line gap on a linear ce dimension, read back from the saved file.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("pdfcer-dimension-extension-gap-tests");
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

fn dim_line(path: &Path) -> String {
    let (code, stdout, stderr) = run(&["dimension-list", s(path)]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    stdout
        .lines()
        .find(|l| l.trim_start().starts_with("dim 0 "))
        .expect("dim 0 listed")
        .to_owned()
}

#[test]
fn a_gap_is_set_refused_and_cleared_through_the_saved_file() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf");
    let a = temp_out("a.pdf");
    let (code, out, err) = run(&[
        "dimension-add",
        s(&fixture),
        "--kind",
        "linear",
        "--points",
        "100,100 300,100",
        "--offset",
        "50",
        "-o",
        s(&a),
    ]);
    assert_eq!(code, 0, "{out}\n{err}");

    let b = temp_out("b.pdf");
    let (code, out, err) = run(&[
        "dimension-extension-gap",
        s(&a),
        "--dimension",
        "0",
        "--end",
        "b",
        "--gap",
        "20",
        "--verify-undo",
        "-o",
        s(&b),
    ]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("end=b gap=20 "), "{out}");
    assert!(out.contains("undo_identical=1"), "{out}");
    let listed = dim_line(&b);
    assert!(
        listed.contains(" gap_b=20") && !listed.contains("gap_a"),
        "{listed}"
    );

    // ANSI reach at a 50pt standoff: 50 - 3 overshoot.
    let (code, _, err) = run(&[
        "dimension-extension-gap",
        s(&b),
        "--dimension",
        "0",
        "--end",
        "a",
        "--gap",
        "47",
        "-o",
        s(&temp_out("refused.pdf")),
    ]);
    assert_eq!(code, EDIT_REFUSED, "{err}");
    assert!(err.contains("less than 47"), "{err}");

    let c = temp_out("c.pdf");
    let (code, out, err) = run(&[
        "dimension-extension-gap",
        s(&b),
        "--dimension",
        "0",
        "--end",
        "b",
        "--clear",
        "-o",
        s(&c),
    ]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("gap=standard"), "{out}");
    assert!(!dim_line(&c).contains("gap_"), "{}", dim_line(&c));
}

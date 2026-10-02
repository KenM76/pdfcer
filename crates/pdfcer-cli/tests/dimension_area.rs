//! Area ce dimensions from the CLI: `dimension-add --kind area`, the
//! three-vertex refusal, and `dimension-area` switching a closed perimeter
//! between its two readings, read back through `dimension-list`.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;
const SQUARE: &str = "100,100 172,100 172,172 100,172";

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pdfcer-dimension-area-tests-{}",
        std::process::id()
    ));
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

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf")
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

fn add(kind: &str, points: &str, out: &Path) -> (i32, String, String) {
    run(&[
        "dimension-add",
        s(&fixture()),
        "--kind",
        kind,
        "--points",
        points,
        "-o",
        s(out),
    ])
}

#[test]
fn an_area_is_added_and_listed_in_square_points() {
    let a = temp_out("area.pdf");
    let (code, out, err) = add("area", SQUARE, &a);
    assert_eq!(code, 0, "{out}\n{err}");
    let listed = dim_line(&a);
    assert!(listed.contains(" kind=area "), "{listed}");
    assert!(listed.contains("value=\"5184.00 pt\u{b2}\""), "{listed}");
}

#[test]
fn two_points_are_refused_by_name_and_nothing_is_written() {
    let out_path = temp_out("refused.pdf");
    let (code, _, err) = add("area", "100,100 172,100", &out_path);
    assert_eq!(code, EDIT_REFUSED, "{err}");
    assert!(err.contains("at least 3 vertices; 2 given"), "{err}");
    assert!(!out_path.exists(), "a refusal writes nothing");
}

#[test]
fn dimension_area_switches_a_closed_perimeter_and_back() {
    let p = temp_out("perimeter.pdf");
    let (code, out, err) = add("perimeter", SQUARE, &p);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(dim_line(&p).contains(" kind=perimeter "));

    let a = temp_out("switched.pdf");
    let (code, out, err) = run(&[
        "dimension-area",
        s(&p),
        "--dimension",
        "0",
        "--show",
        "area",
        "--verify-undo",
        "-o",
        s(&a),
    ]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains(" show=area "), "{out}");
    assert!(out.contains("undo_identical=1"), "{out}");
    assert!(dim_line(&a).contains(" kind=area "), "{}", dim_line(&a));

    let back = temp_out("back.pdf");
    let (code, out, err) = run(&[
        "dimension-area",
        s(&a),
        "--dimension",
        "0",
        "--show",
        "perimeter",
        "-o",
        s(&back),
    ]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(dim_line(&back).contains("value=\"288.00 pt\""));
}

#[test]
fn dimension_area_refuses_an_open_path() {
    let p = temp_out("path.pdf");
    let (code, out, err) = add("path", SQUARE, &p);
    assert_eq!(code, 0, "{out}\n{err}");
    let (code, _, err) = run(&[
        "dimension-area",
        s(&p),
        "--dimension",
        "0",
        "--show",
        "area",
        "-o",
        s(&temp_out("open-refused.pdf")),
    ]);
    assert_eq!(code, EDIT_REFUSED, "{err}");
    assert!(err.contains("needs a closed outline"), "{err}");
}

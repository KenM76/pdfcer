//! `centre-line` and `centre-mark` over the real binary: what the command
//! prints, and what `dimension-list` then finds in the file.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED`, spelled out so a change to the number is visible.
const EDIT_REFUSED: i32 = 9;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf")
}

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-centre-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

/// Run `pdfcer <verb> <fixture> <args> -o <out>`: `(exit, stdout, stderr, out)`.
fn run(verb: &str, name: &str, args: &[&str]) -> (i32, String, String, PathBuf) {
    let out = temp_out(name);
    let r = Command::new(BIN)
        .arg(verb)
        .arg(fixture())
        .args(args)
        .arg("-o")
        .arg(&out)
        .output()
        .expect("pdfcer runs");
    (
        r.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&r.stdout).into_owned(),
        String::from_utf8_lossy(&r.stderr).into_owned(),
        out,
    )
}

fn listed(path: &Path) -> String {
    let r = Command::new(BIN)
        .arg("dimension-list")
        .arg(path)
        .output()
        .expect("pdfcer runs");
    String::from_utf8_lossy(&r.stdout).into_owned()
}

#[test]
fn centre_line_discloses_its_reading_and_lists_as_a_centre_line() {
    let (code, out, err, path) = run(
        "centre-line",
        "parallel.pdf",
        &["--points", "100,100 300,100  150,140 350,140"],
    );
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("centre_line relation=parallel distance=40.0000"),
        "{out}"
    );
    assert!(out.contains("forced=0"), "{out}");
    let list = listed(&path);
    assert!(
        list.contains("kind=centre-line") && list.contains("from=141,120 to=309,120"),
        "{list}"
    );
}

#[test]
fn centre_line_refuses_collinear_lines_and_writes_nothing() {
    let (code, _, err, path) = run(
        "centre-line",
        "collinear.pdf",
        &["--points", "0,0 100,0  150,0 250,0"],
    );
    assert_eq!(code, EDIT_REFUSED);
    assert!(
        err.contains("collinear") && err.contains("Nothing was authored"),
        "{err}"
    );
    assert!(!path.exists());
}

#[test]
fn centre_mark_with_a_diameter_is_one_listed_ce_dimension() {
    let (code, out, err, path) = run(
        "centre-mark",
        "diameter.pdf",
        &[
            "--circle",
            "200,200,50",
            "--measure",
            "diameter",
            "--extension-lines",
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("centre_mark measure=diameter extension_lines=on angle=0"),
        "{out}"
    );
    let list = listed(&path);
    assert!(list.contains("dimensions=1"), "{list}");
    assert!(
        list.contains("kind=diameter") && list.contains("centre_mark=on extension_lines=on"),
        "{list}"
    );
}

#[test]
fn a_standalone_centre_mark_from_rim_points_discloses_the_fit() {
    let (code, out, err, path) = run(
        "centre-mark",
        "fitted.pdf",
        &["--points", "250,200 200,250 150,200 200,150"],
    );
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("centre_mark fitted centre=200.0000,200.0000 radius=50.0000"),
        "{out}"
    );
    let list = listed(&path);
    assert!(
        list.contains("kind=centre-mark") && list.contains(" centre=200,200 radius=50 "),
        "{list}"
    );
}

#[test]
fn a_bad_centre_mark_size_is_refused_by_name() {
    let out = temp_out("size.pdf");
    let r = Command::new(BIN)
        .arg("group-style")
        .arg(fixture())
        .args(["--group", "0", "--centre-mark-size", "huge", "-o"])
        .arg(&out)
        .output()
        .expect("pdfcer runs");
    assert_ne!(r.status.code(), Some(0));
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.contains("text:<multiple of the text height>"), "{err}");
}

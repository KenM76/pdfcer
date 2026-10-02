//! `dimension-offset` on a radius ce dimension (`Pass 370.0`): the pair is a
//! text distance past the rim and a leader angle, read back via
//! `dimension-list` from the saved file.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pdfcer-dimension-circular-placement-tests-{}",
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
fn a_radius_is_placed_by_angle_and_distance_through_the_saved_file() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf");
    let a = temp_out("a.pdf");
    // Three points on the circle of radius 50 about (200, 200).
    let (code, out, err) = run(&[
        "dimension-add",
        s(&fixture),
        "--kind",
        "radius",
        "--points",
        "250,200 200,250 150,200",
        "-o",
        s(&a),
    ]);
    assert_eq!(code, 0, "{out}\n{err}");
    // Unplaced, the text sits half-way along the radius: -r/2 past the rim.
    let before = dim_line(&a);
    assert!(
        before.contains(" leader_angle=0 text_distance=-2"),
        "{before}"
    );

    let b = temp_out("b.pdf");
    let (code, out, err) = run(&[
        "dimension-offset",
        s(&a),
        "--dimension",
        "0",
        "--offset",
        "20",
        "--text-along",
        "90",
        "--verify-undo",
        "-o",
        s(&b),
    ]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("undo_identical=1"), "{out}");
    let after = dim_line(&b);
    assert!(
        after.contains(" leader_angle=90") && after.contains(" text_distance=20"),
        "{after}"
    );
    // The printed value is the measurement, which placement does not touch.
    let value = |l: &str| l.split('"').nth(1).map(str::to_owned);
    assert_eq!(value(&before), value(&after), "{before}\n{after}");
}

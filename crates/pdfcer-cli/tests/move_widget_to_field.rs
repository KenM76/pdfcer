//! `move-widget-to-field`: the moved radio button's fresh state and the
//! source's removal reach the parseable line; refusals exit non-zero.

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms/radio-regroup-form.pdf")
}

fn run(tag: &str, name: &str, index: &str, into: &str) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("pdfcer-regroup-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let out = Command::new(BIN)
        .arg("move-widget-to-field")
        .arg(fixture())
        .args([
            "--name",
            name,
            "--index",
            index,
            "--into",
            into,
            "--verify-undo",
        ])
        .arg("-o")
        .arg(dir.join("out.pdf"))
        .output()
        .expect("run pdfcer");
    let _ = std::fs::remove_dir_all(&dir);
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_colliding_member_moves_with_a_fresh_state() {
    let (ok, out, err) = run("fresh", "Left", "0", "Right");
    assert!(ok, "{err}");
    assert!(
        out.contains("widget=11 on_state=Red1 source_removed=0"),
        "{out}"
    );
    assert!(out.contains("undo_identical=1"), "{out}");
    assert!(err.contains("\"Red1\""), "{err}");
}

#[test]
fn a_last_member_takes_its_group_with_it() {
    let (ok, out, err) = run("last", "Solo", "0", "Right");
    assert!(ok, "{err}");
    assert!(out.contains("on_state=Yes source_removed=1"), "{out}");
    assert!(err.contains("removed from the form"), "{err}");
}

#[test]
fn a_check_box_is_refused() {
    let (ok, out, err) = run("refuse", "Agree", "0", "Right");
    assert!(!ok);
    assert!(out.is_empty(), "{out}");
    assert!(err.contains("not a radio-button group"), "{err}");
}

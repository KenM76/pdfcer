//! `paste-field --as-widget-of` over a radio group: the new member's fresh
//! on-state reaches the parseable line, and the rename is disclosed.

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-paste-radio-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn run(args: &[&std::ffi::OsStr]) -> (bool, String, String) {
    let out = Command::new(BIN).args(args).output().expect("run pdfcer");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn pasting_a_radio_member_reports_its_fresh_state() {
    let dir = scratch("fresh");
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms/radio-group-form.pdf");
    let clip = dir.join("priority.clip");
    let output = dir.join("out.pdf");

    let (ok, _, err) = run(&[
        "copy-field".as_ref(),
        input.as_os_str(),
        "--name".as_ref(),
        "Priority".as_ref(),
        "-o".as_ref(),
        clip.as_os_str(),
    ]);
    assert!(ok, "copy-field failed: {err}");
    let (ok, out, err) = run(&[
        "paste-field".as_ref(),
        input.as_os_str(),
        "--clip".as_ref(),
        clip.as_os_str(),
        "--page".as_ref(),
        "1".as_ref(),
        "--rect".as_ref(),
        "200,40,212,52".as_ref(),
        "--as-widget-of".as_ref(),
        "Priority".as_ref(),
        "-o".as_ref(),
        output.as_os_str(),
    ]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(ok, "paste-field failed: {err}");
    assert!(out.contains("radio_state=Red1"), "stdout: {out}");
    assert!(err.contains("\"Red1\""), "the rename is disclosed: {err}");
}

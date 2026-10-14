//! `copy-fields` / `paste-fields`: the names the verb chose reach stdout, the
//! renames reach stderr, and a paste with no policy is refused.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-field-set-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn run(args: &[&OsStr]) -> (bool, String, String) {
    let out = Command::new(BIN).args(args).output().expect("run pdfcer");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms/field-set-form.pdf")
}

/// Copy `Qty`, `Total` and `Pick` onto `clip`.
fn copy_three(clip: &Path) {
    let input = fixture();
    let (ok, out, err) = run(&[
        "copy-fields".as_ref(),
        input.as_os_str(),
        "--name".as_ref(),
        "Qty".as_ref(),
        "--name".as_ref(),
        "Total".as_ref(),
        "--name".as_ref(),
        "Pick".as_ref(),
        "-o".as_ref(),
        clip.as_os_str(),
    ]);
    assert!(ok, "copy-fields failed: {err}");
    assert!(out.contains("fields=3 widgets=4"), "stdout: {out}");
    assert!(
        out.contains(r#"name="Qty" name="Total" name="Pick""#),
        "stdout: {out}"
    );
}

#[test]
fn pasting_as_new_fields_reports_the_chosen_names() {
    let dir = scratch("new");
    let clip = dir.join("set.clip");
    let output = dir.join("out.pdf");
    copy_three(&clip);
    let input = fixture();
    let (ok, out, err) = run(&[
        "paste-fields".as_ref(),
        input.as_os_str(),
        "--clip".as_ref(),
        clip.as_os_str(),
        "--page".as_ref(),
        "1".as_ref(),
        "--at".as_ref(),
        "200,100".as_ref(),
        "--as-new".as_ref(),
        "--no-tooltip".as_ref(),
        "--copy-actions".as_ref(),
        "--verify-undo".as_ref(),
        "-o".as_ref(),
        output.as_os_str(),
    ]);
    let listed = run(&["list-fields".as_ref(), output.as_os_str()]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(ok, "paste-fields failed: {err}");
    assert!(
        out.contains(r#"fields=3 widgets=4"#) && out.contains("undo_identical=1"),
        "stdout: {out}"
    );
    assert!(
        out.contains(r#"name="Qty_2" name="Total_2" name="Pick_2""#),
        "stdout: {out}"
    );
    assert!(err.contains("\"Qty_2\""), "the rename is disclosed: {err}");
    assert!(
        err.contains("calculation"),
        "the repoint is disclosed: {err}"
    );
    assert!(listed.1.contains("Total_2"), "list-fields: {}", listed.1);
}

#[test]
fn pasting_as_widgets_adds_to_the_same_named_fields() {
    let dir = scratch("widgets");
    let clip = dir.join("set.clip");
    let output = dir.join("out.pdf");
    copy_three(&clip);
    let input = fixture();
    let (ok, out, err) = run(&[
        "paste-fields".as_ref(),
        input.as_os_str(),
        "--clip".as_ref(),
        clip.as_os_str(),
        "--page".as_ref(),
        "1".as_ref(),
        "--at".as_ref(),
        "300,100".as_ref(),
        "--as-widgets".as_ref(),
        "-o".as_ref(),
        output.as_os_str(),
    ]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(ok, "paste-fields failed: {err}");
    assert!(out.contains("policy=additional-widgets"), "stdout: {out}");
    assert!(
        out.contains(r#"name="Qty" name="Total" name="Pick""#),
        "stdout: {out}"
    );
}

#[test]
fn a_paste_without_a_policy_or_tooltip_answer_is_refused() {
    let dir = scratch("refused");
    let clip = dir.join("set.clip");
    let output = dir.join("out.pdf");
    copy_three(&clip);
    let input = fixture();
    let base: Vec<&OsStr> = vec![
        "paste-fields".as_ref(),
        input.as_os_str(),
        "--clip".as_ref(),
        clip.as_os_str(),
        "--page".as_ref(),
        "1".as_ref(),
        "--at".as_ref(),
        "200,100".as_ref(),
        "-o".as_ref(),
        output.as_os_str(),
    ];
    let (ok_none, _, err_none) = run(&base);
    let mut as_new = base.clone();
    as_new.push("--as-new".as_ref());
    let (ok_tip, _, err_tip) = run(&as_new);
    let mut widgets_value = base.clone();
    widgets_value.push("--as-widgets".as_ref());
    widgets_value.push("--copy-value".as_ref());
    let (ok_mixed, _, _) = run(&widgets_value);
    let written = output.exists();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!ok_none && err_none.contains("--as-new"), "{err_none}");
    assert!(!ok_tip && err_tip.contains("--no-tooltip"), "{err_tip}");
    assert!(!ok_mixed, "--copy-value needs --as-new");
    assert!(!written, "nothing written");
}

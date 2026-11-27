//! `copy-field` names every carried `/DR` font, and `paste-field` reports a
//! font renamed because the destination binds its name to a different font.

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn forms(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms")
        .join(name)
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
fn a_renamed_font_reaches_the_parseable_line() {
    let dir = std::env::temp_dir().join(format!("pdfcer-font-renames-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let clip = dir.join("rev.clip");
    let output = dir.join("out.pdf");
    let (ok, out, err) = run(&[
        "copy-field".as_ref(),
        forms("rich-field-form.pdf").as_os_str(),
        "--name".as_ref(),
        "TitleBlock.Revision".as_ref(),
        "-o".as_ref(),
        clip.as_os_str(),
    ]);
    assert!(ok, "copy-field failed: {err}");
    assert!(out.contains(" fonts=TB "), "stdout: {out}");
    let (ok, out, err) = run(&[
        "paste-field".as_ref(),
        forms("rival-font-form.pdf").as_os_str(),
        "--clip".as_ref(),
        clip.as_os_str(),
        "--page".as_ref(),
        "1".as_ref(),
        "--rect".as_ref(),
        "100,100,300,130".as_ref(),
        "--as-new".as_ref(),
        "Imported".as_ref(),
        "--no-tooltip".as_ref(),
        "-o".as_ref(),
        output.as_os_str(),
    ]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(ok, "paste-field failed: {err}");
    assert!(
        out.trim_end().ends_with(" font_renames=TB>TB_1"),
        "stdout: {out}"
    );
    assert!(err.contains("already uses the font resource /TB"), "{err}");
}

//! `add-caret` (`Pass 261.1`): a caret, and with `--strike` a Replace Text
//! pair, written to the output and reported on stdout.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-add-caret-tests-{}", std::process::id()));
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

#[test]
fn a_replace_text_pair_is_written_and_reported() {
    let out = temp_out("replace.pdf");
    let (code, stdout, stderr) = run(&[
        "add-caret",
        s(&fixture()),
        "--page",
        "1",
        "--rect",
        "100,700,108,712",
        "--text",
        "new words",
        "--author",
        "Ken",
        "--strike",
        "110,700,180,712",
        "--paragraph",
        "--apply",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(
        stdout.contains("kind=replace") && stdout.contains("strikeout=") && stdout.contains("sy=P"),
        "{stdout}"
    );
    let bytes = std::fs::read(&out).expect("output written");
    let text = String::from_utf8_lossy(&bytes);
    for needle in [
        "/Caret",
        "/StrikeOut",
        "/RT /Group",
        "/IT /Replace",
        "/IT /StrikeOutTextEdit",
    ] {
        assert!(text.contains(needle), "missing {needle}");
    }
}

#[test]
fn a_dry_run_writes_nothing_and_bad_input_is_refused() {
    let out = temp_out("dry.pdf");
    let (code, stdout, stderr) = run(&[
        "add-caret",
        s(&fixture()),
        "--page",
        "1",
        "--rect",
        "100,700,108,712",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(
        stdout.contains("kind=insert")
            && stdout.contains("sy=None")
            && stdout.contains("applied=0"),
        "{stdout}"
    );
    assert!(!out.exists());

    for bad in [
        &["--page", "0", "--rect", "100,700,108,712"][..],
        &["--page", "1", "--rect", "100,700,108,712", "--opacity", "2"][..],
        &[
            "--page",
            "1",
            "--rect",
            "100,700,108,712",
            "--strike",
            "1,2,3",
        ][..],
    ] {
        let input = fixture();
        let mut args = vec!["add-caret", s(&input)];
        args.extend_from_slice(bad);
        let (code, _, stderr) = run(&args);
        assert_eq!(code, 9, "{bad:?}: {stderr}");
    }
}

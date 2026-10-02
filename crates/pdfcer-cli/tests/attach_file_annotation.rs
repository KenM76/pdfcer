//! `attach-file-annotation` (`Pass 261.0`): the file lands behind an icon on
//! the named page, and `list-attachments` reads it back from the saved file.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pdfcer-attach-file-annotation-tests-{}",
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

#[test]
fn the_file_is_listed_as_a_page_attachment_with_its_description() {
    let src = temp_out("data.txt");
    std::fs::write(&src, b"hello attachment").expect("write source");
    let out = temp_out("out.pdf");
    let (code, stdout, stderr) = run(&[
        "attach-file-annotation",
        s(&fixture()),
        "--file",
        s(&src),
        "--page",
        "1",
        "--rect",
        "72,700,92,724",
        "--icon",
        "tag",
        "--desc",
        "source data",
        "--author",
        "Ken",
        "--apply",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(
        stdout.contains("name=\"data.txt\" bytes=16 icon=Tag"),
        "{stdout}"
    );

    let (code, stdout, stderr) = run(&["list-attachments", s(&out)]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(
        stdout.contains("attachment name=\"data.txt\" kind=page:1 desc=\"source data\""),
        "{stdout}"
    );
}

#[test]
fn a_dry_run_writes_nothing_and_page_zero_is_refused() {
    let src = temp_out("dry.txt");
    std::fs::write(&src, b"x").expect("write source");
    let out = temp_out("dry.pdf");
    let (code, stdout, stderr) = run(&[
        "attach-file-annotation",
        s(&fixture()),
        "--file",
        s(&src),
        "--page",
        "1",
        "--rect",
        "72,700,92,724",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(
        stdout.contains("icon=PushPin") && stdout.contains("applied=0"),
        "{stdout}"
    );
    assert!(!out.exists());

    let (code, _, stderr) = run(&[
        "attach-file-annotation",
        s(&fixture()),
        "--file",
        s(&src),
        "--page",
        "0",
        "--rect",
        "72,700,92,724",
    ]);
    assert_eq!(code, 9, "{stderr}");
}

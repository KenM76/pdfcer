//! `pdfcer xfa-materialize` writes a dynamic XFA form's computed pages into
//! the file, removes the form, and discloses the inference on stderr.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

use super::xfa_layout::{flowed, pdf_with_data};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn run(name: &str, bytes: &[u8], extra: &[&str]) -> (u8, String, String, Vec<u8>) {
    let dir = std::env::temp_dir().join(format!(
        "pdfcer-xfa-materialize-{}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("in.pdf");
    let output = dir.join("out.pdf");
    std::fs::write(&input, bytes).unwrap();
    let out = Command::new(BIN)
        .arg("xfa-materialize")
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .args(extra)
        .output()
        .unwrap();
    let written = std::fs::read(&output).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    (
        u8::try_from(out.status.code().unwrap()).unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        written,
    )
}

fn kind(bytes: &[u8]) -> String {
    let dir = std::env::temp_dir().join(format!("pdfcer-xfa-mat-kind-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("k.pdf");
    std::fs::write(&path, bytes).unwrap();
    let out = Command::new(BIN)
        .arg("list-fields")
        .arg(&path)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn writes_the_computed_pages_and_discloses_them() {
    let input = pdf_with_data(Some(&flowed()), "<f><a>Ken</a></f>");
    let (code, stdout, stderr, out) = run("ok", &input, &["--verify-undo"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains(" pages=2 replaced=1 images=0 fields_dropped=0 usage_rights_removed=0 "),
        "{stdout}"
    );
    assert!(stdout.contains("undo_identical=1"), "{stdout}");
    assert!(stderr.contains("pdfcer's layout"), "{stderr}");
    assert!(stderr.contains("not fillable"), "{stderr}");
    assert!(out.starts_with(&input), "an incremental save appends");
    let k = kind(&out);
    assert!(!k.contains("xfa_kind=dynamic"), "{k}");
    assert!(kind(&input).contains("xfa_kind=dynamic"));
}

#[test]
fn a_file_without_a_dynamic_form_exits_one() {
    let (code, _, stderr, out) = run("plain", &pdf_with_data(None, ""), &[]);
    assert_eq!(code, 1, "{stderr}");
    assert!(out.is_empty());
}

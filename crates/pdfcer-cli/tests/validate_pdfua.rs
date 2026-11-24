//! `pdfcer validate-pdfua` prints findings in the preflight line format,
//! lists the checks no machine can make, and exits 18 on a violation.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn hello() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf")
}

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(BIN)
        .arg("validate-pdfua")
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

#[test]
fn an_untagged_file_fails_part_1_with_clause_level_findings() {
    let (code, out) = run(&[hello().to_str().unwrap()]);
    assert_eq!(code, 18, "{out}");
    assert!(out.starts_with("validate-pdfua "), "{out}");
    assert!(out.lines().next().unwrap().contains(" part=1 "), "{out}");
    assert!(
        out.lines()
            .any(|l| l.starts_with("error violation rule=PUI-V-7.1-11 page=- target=document")),
        "{out}"
    );
    assert!(
        out.lines()
            .any(|l| l.starts_with("info needs-person rule=PUI-")),
        "{out}"
    );
    assert!(
        out.lines().any(|l| l.starts_with("not-checked rule=PUI-")),
        "{out}"
    );
}

#[test]
fn json_names_the_clause_and_matterhorn_checkpoint() {
    let (code, out) = run(&[hello().to_str().unwrap(), "--part", "2", "--json"]);
    assert_eq!(code, 18, "{out}");
    assert!(out.contains("\"part\": 2"), "{out}");
    assert!(out.contains("\"rule\": \"PUI2-8.2.1-1\""), "{out}");
    assert!(out.contains("\"clause\": \"ISO 14289-2"), "{out}");
    assert!(out.contains("\"not_checked\": ["), "{out}");
}

#[test]
fn a_part_other_than_1_or_2_is_refused() {
    let out = Command::new(BIN)
        .args(["validate-pdfua", hello().to_str().unwrap(), "--part", "3"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn the_pdfua_profiles_run_through_preflight() {
    let out = Command::new(BIN)
        .args([
            "preflight",
            hello().to_str().unwrap(),
            "--profile",
            "pdfua-1",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(18), "{text}");
    assert!(text.contains("rule=pdfua-1/PUI-V-7.1-11"), "{text}");
}

//! `pdfcer validate-pdfa` names each finding's clause, exits 18 on a failing
//! file, says which rows it did not check, and the same rows run through
//! `preflight --profile pdfa-2b`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn hello() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf")
}

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(BIN).args(args).output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

#[test]
fn a_file_with_no_metadata_fails_and_names_the_clause() {
    let (code, out) = run(&["validate-pdfa", hello().to_str().unwrap(), "--level", "2b"]);
    assert_eq!(code, 18, "{out}");
    let header = out.lines().next().unwrap();
    assert!(header.starts_with("validate-pdfa "), "{out}");
    assert!(
        header.contains(" level=2b claim=\"-\" verdict=fails "),
        "{out}"
    );
    assert!(
        out.lines().any(
            |l| l.starts_with("error violation rule=PAI-MD-1 page=- target=document")
                && l.contains("clause=\"ISO 19005-2:2011 §6.6.2.1\"")
        ),
        "{out}"
    );
    assert!(
        out.lines()
            .any(|l| l.starts_with("info incomplete ") && l.contains("Not checked")),
        "the rows not checked are disclosed: {out}"
    );
}

#[test]
fn level_one_cites_part_one() {
    let (code, out) = run(&["validate-pdfa", hello().to_str().unwrap(), "--level", "1b"]);
    assert_eq!(code, 18, "{out}");
    assert!(
        out.lines().any(
            |l| l.contains("rule=PAI-MD-1") && l.contains("clause=\"ISO 19005-1:2005 §6.7.2\"")
        ),
        "{out}"
    );
}

#[test]
fn json_carries_the_same_findings() {
    let (code, out) = run(&[
        "validate-pdfa",
        hello().to_str().unwrap(),
        "--level",
        "3b",
        "--json",
    ]);
    assert_eq!(code, 18, "{out}");
    assert!(
        out.starts_with('{') && out.trim_end().ends_with('}'),
        "{out}"
    );
    assert!(out.contains("\"level\": \"3b\""), "{out}");
    assert!(out.contains("\"claim\": null"), "{out}");
    assert!(out.contains("\"verdict\": \"fails\""), "{out}");
    assert!(out.contains("\"rule\": \"PAI-MD-1\""), "{out}");
}

#[test]
fn an_unknown_level_is_a_usage_error() {
    let out = Command::new(BIN)
        .args(["validate-pdfa", hello().to_str().unwrap(), "--level", "2a"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a PDF/A level"));
}

#[test]
fn the_levels_are_preflight_profiles_too() {
    let (code, out) = run(&[
        "preflight",
        hello().to_str().unwrap(),
        "--profile",
        "pdfa-2b",
    ]);
    assert_eq!(code, 18, "{out}");
    assert!(
        out.starts_with("preflight ") && out.contains("profile=\"PDF/A-2b\""),
        "{out}"
    );
    assert!(out.lines().any(|l| l.contains("rule=PAI-MD-1")), "{out}");
    let (code, json) = run(&["preflight", "--profile", "pdfa-1b", "--dump-profile"]);
    assert_eq!(code, 0);
    assert!(json.contains("\"check\": \"pdfa-requirement\""), "{json}");
}

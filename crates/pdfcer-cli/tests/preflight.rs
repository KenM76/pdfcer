//! `pdfcer preflight` prints one line per finding, exits 18 on an
//! error-severity finding, and reads a profile back from its own JSON.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn hello() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf")
}

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(BIN)
        .arg("preflight")
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

#[test]
fn an_unembedded_font_fails_the_default_profile() {
    let (code, out) = run(&[hello().to_str().unwrap()]);
    assert_eq!(code, 18, "{out}");
    assert!(out.starts_with("preflight "), "{out}");
    assert!(
        out.lines()
            .any(|l| l.starts_with("error violation rule=font-not-embedded page=1 target=font:")),
        "{out}"
    );
}

#[test]
fn a_dumped_profile_edited_to_info_passes() {
    let (code, json) = run(&["--dump-profile"]);
    assert_eq!(code, 0);
    let edited = json.replace("\"severity\": \"error\"", "\"severity\": \"info\"");
    let file = std::env::temp_dir().join(format!("pdfcer_preflight_{}.json", std::process::id()));
    std::fs::write(&file, edited).unwrap();
    let (code, out) = run(&[
        hello().to_str().unwrap(),
        "--profile",
        file.to_str().unwrap(),
    ]);
    std::fs::remove_file(&file).ok();
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("info violation rule=font-not-embedded"),
        "{out}"
    );
}

#[test]
fn the_cap_reports_what_it_dropped_and_fails() {
    let (code, out) = run(&[hello().to_str().unwrap(), "--max-findings", "0"]);
    assert_eq!(code, 18);
    assert!(out.contains("findings=0 dropped=3"), "{out}");
}

#[test]
fn an_unknown_profile_is_refused() {
    let (code, _) = run(&[hello().to_str().unwrap(), "--profile", "no-such-profile"]);
    assert_eq!(code, 9);
}

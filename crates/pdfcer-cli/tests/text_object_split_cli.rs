//! `pdfcer text-object-split --dry-run`: a plan the real run would refuse is
//! refused by the dry run too, with the same reason and a failing exit code.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name)
}

fn dry_run(name: &str, extra: &[&str]) -> Output {
    let input = fixture(name);
    let mut args = vec![
        "text-object-split",
        input.to_str().unwrap(),
        "--object",
        "0",
        "--dry-run",
    ];
    args.extend_from_slice(extra);
    Command::new(BIN)
        .args(&args)
        .output()
        .expect("the binary runs")
}

#[test]
fn a_dry_run_of_a_refused_split_fails_with_the_reason() {
    // The second and third lines are shown by `'`, which moves before it shows.
    let out = dry_run("runs-quote-show.pdf", &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(
        stderr.contains("run 1 is shown by `'`"),
        "names the refusal: {stderr}"
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("text-object-split-plan ") && stdout.contains(" runs_before=[1, 2]"),
        "the plan is still printed: {stdout}"
    );
}

#[test]
fn a_dry_run_of_a_performable_split_succeeds() {
    let out = dry_run("runs-td-relative.pdf", &["--before", "1", "--before", "2"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains(" granularity=explicit cuts=2 runs_before=[1, 2]"),
        "{stdout}"
    );
}

//! `pdfcer sign --certify` / `--mdp-level` (`Pass 10.12`) as the SHIPPED
//! BINARY reports it: the level and its plain meaning reach stdout
//! (criterion 7), the two refusals reach stderr with exit 9, and
//! `verify-signatures` reports the certification back.

#![cfg(feature = "signing")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic")
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_certify_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn sign(input: &Path, output: &Path, extra: &[&str]) -> Output {
    let pfx = fixtures().join("signing/rsa2048-modern.pfx");
    let mut cmd = Command::new(BIN);
    cmd.arg("sign")
        .arg(input)
        .args(["--cert", pfx.to_str().unwrap(), "--password", "pdfcer"])
        .args(["--signing-time", "D:20260906000000Z"])
        .args(extra)
        .arg("--output")
        .arg(output);
    cmd.output().expect("the binary runs")
}

fn text(out: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn certify_without_a_level_takes_the_default_and_says_so() {
    let out_path = temp_path("default");
    let out = sign(&fixtures().join("hello.pdf"), &out_path, &["--certify"]);
    let (stdout, stderr) = text(&out);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stdout.contains("certification: DocMDP P=2 (form fill-in and signing) -- --mdp-level not given; this is Table 254's default"),
        "{stdout}"
    );
    // verify-signatures reports it back.
    let v = Command::new(BIN)
        .arg("verify-signatures")
        .arg(&out_path)
        .output()
        .unwrap();
    let (vs, _) = text(&v);
    assert!(
        vs.contains("certification: DocMDP P=2 (form fill-in and signing)"),
        "{vs}"
    );
    assert!(vs.contains("1 verified, 0 failed"), "{vs}");
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn mdp_level_implies_certify_and_prints_the_words() {
    let out_path = temp_path("annotate");
    let out = sign(
        &fixtures().join("hello.pdf"),
        &out_path,
        &["--mdp-level", "annotate"],
    );
    let (stdout, stderr) = text(&out);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stdout.contains("certification: DocMDP P=3 (form fill-in, signing and annotations)"),
        "{stdout}"
    );
    assert!(
        !stdout.contains("Table 254's default"),
        "a given level is not defaulted: {stdout}"
    );
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn a_second_certification_and_a_late_certification_are_refused_with_exit_9() {
    let first = temp_path("first");
    assert!(
        sign(&fixtures().join("hello.pdf"), &first, &["--certify"])
            .status
            .success()
    );
    let again = temp_path("again");
    let out = sign(&first, &again, &["--mdp-level", "none"]);
    let (_, stderr) = text(&out);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED), "{stderr}");
    assert!(
        stderr.contains("already certified (/DocMDP, P=2)"),
        "{stderr}"
    );
    assert!(!again.exists());

    let approval = temp_path("approval");
    assert!(
        sign(&fixtures().join("hello.pdf"), &approval, &[])
            .status
            .success()
    );
    let late = temp_path("late");
    let out = sign(&approval, &late, &["--certify"]);
    let (_, stderr) = text(&out);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED), "{stderr}");
    assert!(
        stderr.contains("a certification must be the FIRST signature"),
        "{stderr}"
    );
    assert!(!late.exists());
    for p in [first, approval] {
        let _ = std::fs::remove_file(p);
    }
}

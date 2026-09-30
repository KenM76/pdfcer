//! `pdfcer verify-signatures --crl` checks the signer's chain against the
//! given CRLs and prints a `revocation:` line (RFC 5280 §5); the exit code
//! stays integrity's.

#![cfg(feature = "signing")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn crl_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/crl")
}

fn signed(tag: &str) -> PathBuf {
    let out = std::env::temp_dir().join(format!("pdfcer_crl_{tag}_{}.pdf", std::process::id()));
    let run = Command::new(BIN)
        .arg("sign")
        .arg(crl_dir().join("../hello.pdf"))
        .arg("--cert")
        .arg(crl_dir().join("leaf.pfx"))
        .args([
            "--password",
            "pdfcer",
            "--signing-time",
            "D:20260930000000Z",
        ])
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    out
}

fn verify(path: &Path, crls: &[&str]) -> Output {
    let mut cmd = Command::new(BIN);
    cmd.arg("verify-signatures").arg(path);
    for c in crls {
        cmd.arg("--crl").arg(crl_dir().join(c));
    }
    cmd.output().unwrap()
}

fn revocation_line(out: &Output) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .find(|l| l.starts_with("  revocation: "))
        .unwrap_or_else(|| panic!("no revocation line: {stdout}"))
        .to_owned()
}

#[test]
fn verify_signatures_reports_what_the_crls_say() {
    let path = signed("each");
    let none = verify(&path, &[]);
    assert_eq!(none.status.code(), Some(0));
    assert_eq!(
        revocation_line(&none),
        "  revocation: not checked (no CRL in the document or given with --crl)"
    );

    let good = verify(&path, &["crl-good.crl"]);
    assert_eq!(good.status.code(), Some(0));
    assert_eq!(
        revocation_line(&good),
        "  revocation: good -- not revoked at the signing time: \"CN=pdfcer CRL test signer (test fixture, trust nothing)\" by supplied CRL of 2026-09-20T00:00:00Z (next update 2099-01-01T00:00:00Z)"
    );
    assert!(
        String::from_utf8_lossy(&good.stdout).contains(
            "  note: clock: no CMS signingTime, so certificate validity and revocation are checked at the /M time the signature dictionary states (2026-09-30T00:00:00Z)"
        ),
        "the clock used is disclosed"
    );

    let revoked = verify(&path, &["crl-good.crl", "crl-revoked.crl"]);
    assert_eq!(
        revoked.status.code(),
        Some(0),
        "revocation does not change the exit code"
    );
    assert_eq!(
        revocation_line(&revoked),
        "  revocation: REVOKED -- \"CN=pdfcer CRL test signer (test fixture, trust nothing)\" on 2026-09-15T00:00:00Z (reason keyCompromise; supplied CRL), BEFORE the signing time"
    );
    assert!(
        revocation_line(&verify(&path, &["crl-revoked-after.crl"]))
            .ends_with("(reason superseded; supplied CRL), after the signing time")
    );
    assert!(
        revocation_line(&verify(&path, &["crl-forged.crl"])).starts_with(
            "  revocation: undetermined -- the CRL for CN=pdfcer CRL test signer (test fixture, trust nothing) cannot be used: its signature does not verify"
        )
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn an_unreadable_crl_file_is_an_io_error() {
    let path = signed("missing");
    let out = verify(&path, &["no-such.crl"]);
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no-such.crl"));
    let _ = std::fs::remove_file(path);
}

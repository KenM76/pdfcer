//! `pdfcer create-digital-id` as the shipped binary runs it: the ID it
//! writes must be accepted by `pdfcer sign --cert`, and a refused spec must
//! exit 9 with nothing written.

#![cfg(feature = "signing")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn temp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdfcer_newid_{tag}_{}", std::process::id()))
}

fn create(pfx: &Path, extra: &[&str]) -> Output {
    Command::new(BIN)
        .args([
            "create-digital-id",
            "--name",
            "Test Signer",
            "--password",
            "secret",
        ])
        .args(["--iterations", "1000"])
        .args(extra)
        .arg("--output")
        .arg(pfx)
        .output()
        .expect("the binary runs")
}

#[test]
fn created_id_signs_a_document() {
    let pfx = temp_path("ok").with_extension("pfx");
    let cer = temp_path("ok").with_extension("cer");
    let out = create(
        &pfx,
        &[
            "--key",
            "p256",
            "--country",
            "ca",
            "--cert-out",
            cer.to_str().unwrap(),
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("key=EC P-256"), "{stdout}");
    assert!(stdout.contains("name=\"Test Signer\""), "{stdout}");
    assert!(stdout.contains("sha256="), "{stdout}");
    assert!(std::fs::metadata(&cer).unwrap().len() > 300);

    let signed = temp_path("signed").with_extension("pdf");
    let hello = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    let sign = Command::new(BIN)
        .arg("sign")
        .arg(&hello)
        .args(["--cert", pfx.to_str().unwrap(), "--password", "secret"])
        .args(["--signing-time", "D:20260930000000Z", "--output"])
        .arg(&signed)
        .output()
        .expect("the binary runs");
    assert_eq!(
        sign.status.code(),
        Some(0),
        "{}{}",
        String::from_utf8_lossy(&sign.stdout),
        String::from_utf8_lossy(&sign.stderr)
    );
    for p in [&pfx, &cer, &signed] {
        let _ = std::fs::remove_file(p);
    }
}

#[test]
fn refused_spec_writes_nothing() {
    let pfx = temp_path("refused").with_extension("pfx");
    let out = create(&pfx, &["--key", "p256", "--encryption"]);
    assert_eq!(out.status.code(), Some(9));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot be used for encryption"));
    assert!(!pfx.exists());
}

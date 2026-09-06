//! `pdfcer sign --field-name <existing empty /Sig field>` (`Pass 10.13`) as
//! the shipped binary reports it: `field=… (existing)`, the `/FieldMDP`
//! lock line, seed-value notes on stdout; the refusals on stderr with exit 9.

#![cfg(feature = "signing")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/signing")
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_intofield_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn sign(input: &Path, output: &Path, extra: &[&str]) -> Output {
    let pfx = fixtures().join("rsa2048-modern.pfx");
    Command::new(BIN)
        .arg("sign")
        .arg(input)
        .args(["--cert", pfx.to_str().unwrap(), "--password", "pdfcer"])
        .args(["--signing-time", "D:20260906000000Z"])
        .args(extra)
        .arg("--output")
        .arg(output)
        .output()
        .expect("the binary runs")
}

fn text(out: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn signing_into_the_placeholder_says_existing_and_verifies() {
    let out_path = temp_path("empty");
    let out = sign(
        &fixtures().join("sig-field-empty.pdf"),
        &out_path,
        &["--field-name", "SignHere"],
    );
    let (stdout, stderr) = text(&out);
    assert!(out.status.success(), "{stderr}");
    assert!(stdout.contains("field=\"SignHere\" (existing)"), "{stdout}");
    assert!(
        stdout.contains("appearance: \"Digitally signed by"),
        "the field's own rectangle placed a visible appearance: {stdout}"
    );
    let v = Command::new(BIN)
        .arg("list-signatures")
        .arg(&out_path)
        .output()
        .unwrap();
    let (ls, _) = text(&v);
    assert!(ls.contains("signature field=\"SignHere\""), "{ls}");
    let v = Command::new(BIN)
        .arg("verify-signatures")
        .arg(&out_path)
        .output()
        .unwrap();
    assert!(text(&v).0.contains("1 verified, 0 failed"));
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn a_lock_is_reported_and_a_created_field_says_created() {
    let out_path = temp_path("lock");
    let out = sign(
        &fixtures().join("sig-field-lock-include.pdf"),
        &out_path,
        &["--field-name", "SignHere"],
    );
    let (stdout, stderr) = text(&out);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stdout.contains(
            "field_lock: /FieldMDP Include: Name (copied from the field's /Lock, Table 233)"
        ),
        "{stdout}"
    );
    let _ = std::fs::remove_file(&out_path);
    let out = sign(&fixtures().join("sig-field-empty.pdf"), &out_path, &[]);
    let (stdout, _) = text(&out);
    assert!(
        stdout.contains("field=\"Signature1\" (created)"),
        "{stdout}"
    );
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn seed_value_refusals_exit_9_and_name_the_constraint() {
    let out_path = temp_path("sv");
    let out = sign(
        &fixtures().join("sig-field-sv-strict.pdf"),
        &out_path,
        &["--field-name", "SignHere"],
    );
    let (_, stderr) = text(&out);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED), "{stderr}");
    assert!(
        stderr.contains("requires a digest among [SHA1] (DigestMethod, required)"),
        "{stderr}"
    );
    assert!(!out_path.exists());
    let out = sign(
        &fixtures().join("sig-field-sv-cert.pdf"),
        &out_path,
        &["--field-name", "SignHere"],
    );
    let (_, stderr) = text(&out);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED), "{stderr}");
    assert!(stderr.contains("cannot honour (/Cert"), "{stderr}");
    // The recommended-only seed value signs and prints its note.
    let out = sign(
        &fixtures().join("sig-field-sv-ok.pdf"),
        &out_path,
        &["--field-name", "SignHere"],
    );
    let (stdout, stderr) = text(&out);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stdout.contains(
            "note: seed value: the form author suggests a reason among [Approved, Reviewed]"
        ),
        "{stdout}"
    );
    let _ = std::fs::remove_file(out_path);
}

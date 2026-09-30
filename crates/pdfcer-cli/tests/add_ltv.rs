//! `pdfcer add-ltv` writes validation material into `/DSS` (PAdES B-LT) and
//! reads each signature's revocation verdict back from the output.

#![cfg(feature = "signing")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn ocsp_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/ocsp")
}

fn temp(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdfcer_add_ltv_{tag}_{}.pdf", std::process::id()))
}

fn signed(tag: &str) -> PathBuf {
    let out = temp(tag);
    let run = Command::new(BIN)
        .arg("sign")
        .arg(ocsp_dir().join("../hello.pdf"))
        .arg("--cert")
        .arg(ocsp_dir().join("leaf.pfx"))
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

fn add_ltv(input: &Path, output: &Path, args: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(BIN);
    cmd.arg("add-ltv").arg(input).arg("-o").arg(output);
    for (flag, file) in args {
        cmd.arg(flag).arg(ocsp_dir().join(file));
    }
    cmd.output().unwrap()
}

#[test]
fn add_ltv_embeds_evidence_and_reports_the_verdict_from_it() {
    let input = signed("ok");
    let output = temp("ok_out");
    let run = add_ltv(
        &input,
        &output,
        &[
            ("--ocsp", "ocsp-basic-only.der"),
            ("--crl", "crl-clear.crl"),
        ],
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        run.status.code(),
        Some(0),
        "{stdout}{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(stdout.contains("crls_added=1 ocsps_added=1"), "{stdout}");
    assert!(stdout.contains("ocsps_wrapped=1"), "{stdout}");
    assert!(stdout.contains("revocation: good"), "{stdout}");
    assert!(stdout.contains(" by DSS "), "{stdout}");
    let before = std::fs::read(&input).unwrap();
    let after = std::fs::read(&output).unwrap();
    assert_eq!(&after[..before.len()], before.as_slice(), "incremental");

    // Again: everything is a duplicate, nothing is appended.
    let again = temp("again_out");
    let run = add_ltv(&output, &again, &[("--crl", "crl-clear.crl")]);
    assert_eq!(run.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&run.stdout).contains("crls_added=0"));
    assert_eq!(std::fs::read(&again).unwrap(), after);
    for p in [input, output, again] {
        let _ = std::fs::remove_file(p);
    }
}

#[test]
fn add_ltv_refuses_unusable_material_and_unsigned_documents() {
    let input = signed("refuse");
    let output = temp("refuse_out");
    let run = add_ltv(&input, &output, &[("--ocsp", "ocsp-trylater.der")]);
    assert_eq!(run.status.code(), Some(9));
    assert!(String::from_utf8_lossy(&run.stderr).contains("tryLater"));
    assert!(!output.exists(), "nothing written");
    let run = add_ltv(&ocsp_dir().join("../hello.pdf"), &output, &[]);
    assert_eq!(run.status.code(), Some(9));
    assert!(String::from_utf8_lossy(&run.stderr).contains("no signature"));
    let _ = std::fs::remove_file(input);
}

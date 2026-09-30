//! `pdfcer verify-signatures` prints the revocation locations the signer's
//! certificate states, marked as unfetched (RFC 5280 §4.2.1.13, §4.2.2.1).

#![cfg(feature = "signing")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic")
}

fn signed_with(pfx: &str, tag: &str) -> PathBuf {
    let out = std::env::temp_dir().join(format!("pdfcer_revsrc_{tag}_{}.pdf", std::process::id()));
    let run = Command::new(BIN)
        .arg("sign")
        .arg(fixtures().join("hello.pdf"))
        .arg("--cert")
        .arg(fixtures().join("signing").join(pfx))
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

fn verify(path: &Path) -> String {
    let v = Command::new(BIN)
        .arg("verify-signatures")
        .arg(path)
        .output()
        .unwrap();
    assert!(v.status.success(), "{}", String::from_utf8_lossy(&v.stderr));
    String::from_utf8_lossy(&v.stdout).into_owned()
}

#[test]
fn verify_signatures_prints_the_stated_revocation_locations() {
    let path = signed_with("revocation-ecp256-modern.pfx", "located");
    let stdout = verify(&path);
    assert!(
        stdout.contains(
            "  revocation-source: subject=\"CN=pdfcer revocation signer (test fixture, trust nothing), O=pdfcer fixtures, C=CA\" \
             ocsp=http://ocsp.example.invalid/ \
             crl=http://crl.example.invalid/a.crl,ldap://ldap.example.invalid/cn=CA?certificateRevocationList \
             ca_issuers=http://ca.example.invalid/issuer.cer unreadable=1 (stated by the certificate, NOT fetched)"
        ),
        "{stdout}"
    );
    let _ = std::fs::remove_file(path);

    let path = signed_with("rsa2048-modern.pfx", "plain");
    let stdout = verify(&path);
    assert!(!stdout.contains("revocation-source:"), "{stdout}");
    let _ = std::fs::remove_file(path);
}

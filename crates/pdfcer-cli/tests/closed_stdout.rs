//! A pipeline that stops reading stdout early ends `pdfcer` quietly, with
//! status 0 and no panic on stderr.

use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

#[test]
fn a_closed_stdout_is_not_reported_as_a_panic() {
    let pdf = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/hello.pdf"
    );
    let mut child = Command::new(BIN)
        .args(["dimension-list", pdf])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pdfcer");
    drop(child.stdout.take());
    let out = child.wait_with_output().expect("wait");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(out.status.success(), "{:?} {stderr}", out.status);
}

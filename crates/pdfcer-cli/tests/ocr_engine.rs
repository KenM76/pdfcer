//! CLI tests for `ocr --ocr-engine` (`Pass 327.0`): the selection is honoured
//! in both builds, and a missing engine or model is refused by name.
//!
//! Recognition itself is not run here: it needs model files the repository
//! does not carry for OCRcer, and `ocrs` inference in a debug test binary
//! costs tens of seconds per page.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn scan() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/ocr/scan.pdf")
}

fn out(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("pdfcer-cli-ocr-engine-tests");
    std::fs::create_dir_all(&d).expect("temp dir");
    d.join(name)
}

/// Without the `ocrcer` feature the choice still parses and is refused by
/// name — "not in this build", not "unknown value" and not zero words.
#[cfg(not(feature = "ocrcer"))]
#[test]
fn ocrcer_is_refused_by_name_when_not_compiled_in() {
    let o = Command::new(BIN)
        .arg("ocr")
        .arg(scan())
        .args(["--ocr-engine", "ocrcer", "-o"])
        .arg(out("refused.pdf"))
        .output()
        .expect("run pdfcer");
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(64), "stderr: {err}");
    assert!(
        err.contains("without the `ocrcer` feature"),
        "stderr: {err}"
    );
}

/// With the feature, a `--model-dir` lacking `ocrcer.ocrw` is reported with
/// the path and the file name, never replaced by another model.
#[cfg(feature = "ocrcer")]
#[test]
fn a_model_dir_without_the_ocrcer_model_is_reported() {
    let empty = std::env::temp_dir().join("pdfcer-cli-ocr-engine-tests-empty");
    std::fs::create_dir_all(&empty).expect("temp dir");
    let o = Command::new(BIN)
        .arg("ocr")
        .arg(scan())
        .args(["--ocr-engine", "ocrcer", "--model-dir"])
        .arg(&empty)
        .arg("-o")
        .arg(out("missing-model.pdf"))
        .output()
        .expect("run pdfcer");
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "stderr: {err}");
    assert!(err.contains("no OCR models for `ocrcer`"), "stderr: {err}");
    assert!(err.contains("ocrcer.ocrw"), "stderr: {err}");
}

/// A file that is not an `.ocrw` container is refused as a model, not read
/// as one.
#[cfg(feature = "ocrcer")]
#[test]
fn a_file_that_is_not_an_ocrw_model_is_refused() {
    let dir = std::env::temp_dir().join("pdfcer-cli-ocr-engine-tests-bogus");
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("ocrcer.ocrw"), b"not a model").expect("write");
    let o = Command::new(BIN)
        .arg("ocr")
        .arg(scan())
        .args(["--ocr-engine", "ocrcer", "--model-dir"])
        .arg(&dir)
        .arg("-o")
        .arg(out("bogus-model.pdf"))
        .output()
        .expect("run pdfcer");
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "stderr: {err}");
    assert!(err.contains("not a usable OCRcer model"), "stderr: {err}");
}

//! CLI tests for `ocr --ocr-engine` (`Pass 327.0`): the selection is honoured
//! in both builds, and a missing engine or model is refused by name.
//!
//! Recognition is run only for Tesseract, and only when a build is present
//! (it is not committed). OCRcer's model is not in the repository either,
//! and `ocrs` inference in a debug test binary costs tens of seconds per page.

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

fn run_tesseract(model_dir: &Path, lang: &str, name: &str) -> (Option<i32>, String) {
    let o = Command::new(BIN)
        .arg("ocr")
        .arg(scan())
        .args([
            "--ocr-engine",
            "tesseract",
            "--ocr-lang",
            lang,
            "--model-dir",
        ])
        .arg(model_dir)
        .arg("-o")
        .arg(out(name))
        .output()
        .expect("run pdfcer");
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn tess_dir(name: &str, with_exe: bool, langs: &[&str]) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pdfcer-cli-ocr-tess-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("tessdata")).expect("temp dir");
    if with_exe {
        let exe = if cfg!(windows) {
            "tesseract.exe"
        } else {
            "tesseract"
        };
        std::fs::write(d.join(exe), b"placeholder").expect("write");
    }
    for l in langs {
        std::fs::write(d.join("tessdata").join(format!("{l}.traineddata")), b"x").expect("write");
    }
    d
}

/// A folder without the program is reported with the path searched, never
/// replaced by another engine.
#[test]
fn tesseract_folder_without_the_program_is_reported() {
    let d = tess_dir("noexe", false, &["eng"]);
    let (code, err) = run_tesseract(&d, "eng", "tess-noexe.pdf");
    assert_eq!(code, Some(1), "stderr: {err}");
    assert!(
        err.contains("no OCR models for `tesseract`"),
        "stderr: {err}"
    );
    assert!(err.contains("--model-dir"), "stderr: {err}");
}

/// Every language in `--ocr-lang` must have its `.traineddata`; the missing
/// file is named, and the message says where to get it.
#[test]
fn tesseract_missing_language_data_is_named() {
    let d = tess_dir("nolang", true, &["eng"]);
    let (code, err) = run_tesseract(&d, "eng+deu", "tess-nolang.pdf");
    assert_eq!(code, Some(1), "stderr: {err}");
    assert!(err.contains("deu.traineddata"), "stderr: {err}");
    assert!(!err.contains("eng.traineddata"), "stderr: {err}");
    assert!(err.contains("tessdata_fast"), "stderr: {err}");
}

/// `--ocr-lang` is passed to a program, so anything but language codes
/// joined by `+` is refused before it gets there.
#[test]
fn tesseract_malformed_language_list_is_refused() {
    let d = tess_dir("badlang", true, &["eng"]);
    for bad in ["eng+", "eng --psm 0", "../eng", ""] {
        let (code, err) = run_tesseract(&d, bad, "tess-badlang.pdf");
        assert_eq!(code, Some(1), "{bad:?} stderr: {err}");
        assert!(
            err.contains("expected language codes"),
            "{bad:?} stderr: {err}"
        );
    }
}

/// End to end against a real build, when one is present (the bundle made by
/// `tools/tesseract/build-tesseract.py`, or `PDFCER_TEST_TESSERACT_DIR`).
/// Skipped, and says so, otherwise: the repository does not carry the exe.
#[test]
fn tesseract_bundle_reads_the_scan_when_present() {
    let dir = std::env::var_os("PDFCER_TEST_TESSERACT_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tesseract-bundle"),
        PathBuf::from,
    );
    let exe = if cfg!(windows) {
        "tesseract.exe"
    } else {
        "tesseract"
    };
    if !dir.join(exe).is_file() || !dir.join("tessdata/eng.traineddata").is_file() {
        eprintln!("skipped: no Tesseract build at {}", dir.display());
        return;
    }
    let (code, err) = run_tesseract(&dir, "eng", "tess-real.pdf");
    assert_eq!(code, Some(0), "stderr: {err}");
    let o = Command::new(BIN)
        .arg("find-text")
        .arg(out("tess-real.pdf"))
        .args(["--needle", "sleeping"])
        .output()
        .expect("run pdfcer");
    let found = String::from_utf8_lossy(&o.stdout);
    assert!(
        o.status.success() && found.contains("sleeping"),
        "find-text: {found}"
    );
}

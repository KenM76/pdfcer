//! `pdfcer edit-text --augment-subset` (decision 173) over the real binary.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

fn augment_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/text/augment")
}

/// A fresh folder holding only `face`, so `--font-dir` offers exactly one
/// face named `pdfcerAugFace`.
fn font_dir(tag: &str, face: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer_aug_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(augment_dir().join(face), dir.join(face)).unwrap();
    dir
}

fn run(tag: &str, extra: &[&str]) -> (Output, PathBuf) {
    run_on("subset-in.pdf", tag, extra)
}

fn run_on(input: &str, tag: &str, extra: &[&str]) -> (Output, PathBuf) {
    let out = std::env::temp_dir().join(format!("pdfcer_aug_{tag}_{}.pdf", std::process::id()));
    let input = augment_dir().join(input);
    let o = Command::new(BIN)
        .arg("edit-text")
        .arg(&input)
        .args(["--page", "1", "--find", "ABC", "--replace", "ABD", "-o"])
        .arg(&out)
        .args(extra)
        .output()
        .unwrap();
    (o, out)
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn the_flag_appends_the_glyph_and_prints_the_inference() {
    let dir = font_dir("ok", "face.ttf");
    let (o, out) = run(
        "ok",
        &["--font-dir", dir.to_str().unwrap(), "--augment-subset"],
    );
    let all = text(&o);
    assert_eq!(o.status.code(), Some(0), "{all}");
    assert!(
        all.contains("inference") && all.contains("pdfcerAugFace"),
        "{all}"
    );
    let saved = std::fs::read(&out).unwrap();
    let base = std::fs::read(augment_dir().join("subset-in.pdf")).unwrap();
    assert!(saved.starts_with(&base), "incremental");
    assert!(String::from_utf8_lossy(&saved).matches("/Length1").count() == 2);
}

#[test]
fn without_the_flag_the_missing_glyph_is_refused() {
    let dir = font_dir("off", "face.ttf");
    let (o, _) = run("off", &["--font-dir", dir.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(EDIT_REFUSED), "{}", text(&o));
}

#[test]
fn a_face_that_draws_a_shown_glyph_differently_is_refused_with_why() {
    let dir = font_dir("differs", "face-b-differs.ttf");
    let args = [
        "--font-dir",
        dir.to_str().unwrap(),
        "--augment-subset",
        "--augment-check",
        "shown-only",
    ];
    let (all, _) = run("differs", &args);
    assert_eq!(all.status.code(), Some(EDIT_REFUSED));
    assert!(text(&all).contains("differently"), "{}", text(&all));
}

#[test]
fn a_composite_subset_gains_the_glyph_too() {
    let dir = font_dir("cid", "face.ttf");
    let args = ["--font-dir", dir.to_str().unwrap(), "--augment-subset"];
    let (o, out) = run_on("cid-subset-in.pdf", "cid", &args);
    let all = text(&o);
    assert_eq!(o.status.code(), Some(0), "{all}");
    assert!(all.contains("inference") && all.contains("CID"), "{all}");
    let saved = std::fs::read(&out).unwrap();
    let base = std::fs::read(augment_dir().join("cid-subset-in.pdf")).unwrap();
    assert!(saved.starts_with(&base), "incremental");
    assert_eq!(
        String::from_utf8_lossy(&saved).matches("/Length1").count(),
        2
    );
}

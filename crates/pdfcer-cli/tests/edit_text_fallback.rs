//! `pdfcer edit-text --fallback-font` / `--fallback-font-file` over the real
//! binary.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

fn text_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/text")
}

fn run(tag: &str, replace: &str, extra: &[&str]) -> (Output, PathBuf) {
    let out = std::env::temp_dir().join(format!("pdfcer_fb_{tag}_{}.pdf", std::process::id()));
    let o = Command::new(BIN)
        .arg("edit-text")
        .arg(text_dir().join("fallback-font.pdf"))
        .args(["--page", "1", "--find", "Qu5", "--replace", replace, "-o"])
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
fn a_named_fallback_sets_the_euro_in_helvetica_and_prints_it() {
    let (o, out) = run("named", "Qu\u{20AC}5", &["--fallback-font", "Helvetica"]);
    let all = text(&o);
    assert_eq!(o.status.code(), Some(0), "{all}");
    assert!(
        all.contains("fallback=U+20AC face=Helvetica resource=/F1 source=page-resource"),
        "{all}"
    );
    let saved = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(saved.contains("/F1 24 Tf"), "{saved}");
    let _ = std::fs::remove_file(out);
}

#[test]
fn a_fallback_file_embeds_a_subset_and_prints_it() {
    let donor = text_dir().join("fallback-donor.ttf");
    let donor = donor.to_str().unwrap();
    let (o, out) = run(
        "file",
        "Qu\u{20AC} \u{2265} 5",
        &["--fallback-font-file", donor],
    );
    let all = text(&o);
    assert_eq!(o.status.code(), Some(0), "{all}");
    assert!(
        all.contains("fallback=U+20AC,U+0020,U+2265 face=")
            && all.contains("+fallback-donor resource=/pdfceF1 source=embedded-subset"),
        "{all}"
    );
    let saved = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(saved.contains("/Identity-H"));
    let _ = std::fs::remove_file(out);
}

#[test]
fn a_character_neither_font_has_is_refused_by_name() {
    let donor = text_dir().join("fallback-donor.ttf");
    let donor = donor.to_str().unwrap();
    let (o, out) = run(
        "gap",
        "Qu\u{20AC}\u{3A9}5",
        &["--fallback-font-file", donor],
    );
    let all = text(&o);
    assert_eq!(o.status.code(), Some(EDIT_REFUSED), "{all}");
    assert!(
        all.contains("U+03A9") && !all.contains("is missing"),
        "{all}"
    );
    assert!(!out.exists());
}

#[test]
fn without_a_fallback_the_edit_refuses() {
    let (o, out) = run("off", "Qu\u{20AC}5", &[]);
    assert_eq!(o.status.code(), Some(EDIT_REFUSED), "{}", text(&o));
    assert!(!out.exists());
}

#[test]
fn run_repertoire_counts_what_the_fallback_accepts() {
    let repertoire = |extra: &[&str]| {
        let o = Command::new(BIN)
            .arg("run-repertoire")
            .arg(text_dir().join("fallback-font.pdf"))
            .args(["--page", "1", "--find", "Qu5", "--list"])
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", text(&o));
        text(&o)
    };
    let with = repertoire(&["--fallback-font", "Helvetica"]);
    assert!(
        with.contains(" fallback_chars=") && with.contains("U+20AC"),
        "{with}"
    );
    let without = repertoire(&[]);
    assert!(
        !without.contains(" fallback=") && !without.contains("U+20AC"),
        "{without}"
    );
}

#[test]
fn the_two_flags_conflict() {
    let (o, out) = run(
        "both",
        "Qu\u{20AC}5",
        &["--fallback-font", "F1", "--fallback-font-file", "x.ttf"],
    );
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(!out.exists());
}

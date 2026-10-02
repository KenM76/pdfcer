//! `pdfcer edit-text --sibling-fonts` (decision 174) over the real binary.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

fn run(tag: &str, extra: &[&str]) -> (Output, PathBuf) {
    let out = std::env::temp_dir().join(format!("pdfcer_sib_{tag}_{}.pdf", std::process::id()));
    let input = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text/sibling-font.pdf");
    let o = Command::new(BIN)
        .arg("edit-text")
        .arg(&input)
        .args(["--page", "1", "--find", "AA", "--replace", "AB", "-o"])
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
fn the_flag_sets_the_replacement_in_the_sibling_and_prints_it() {
    let (o, out) = run("ok", &["--sibling-fonts"]);
    let all = text(&o);
    assert_eq!(o.status.code(), Some(0), "{all}");
    assert!(
        all.contains("SIBBBB+pdfceSib") && all.contains("/F1"),
        "the sibling is disclosed: {all}"
    );
    let saved = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(saved.contains("/F1 24 Tf"), "{saved}");
    let _ = std::fs::remove_file(out);
}

#[test]
fn without_the_flag_the_edit_refuses() {
    let (o, out) = run("off", &[]);
    assert_eq!(o.status.code(), Some(EDIT_REFUSED), "{}", text(&o));
    assert!(!out.exists());
}

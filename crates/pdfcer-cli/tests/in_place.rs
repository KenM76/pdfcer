//! `--in-place` over the real binary: a successful edit replaces the input,
//! and a refused one leaves it byte-identical.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn scratch_copy(tag: &str) -> PathBuf {
    let src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/addtext/plain.pdf");
    let dst = std::env::temp_dir().join(format!("pdfcer_inplace_{tag}_{}.pdf", std::process::id()));
    std::fs::copy(src, &dst).unwrap();
    dst
}

#[test]
fn in_place_edit_replaces_the_input() {
    let pdf = scratch_copy("ok");
    let before = std::fs::read(&pdf).unwrap();
    let out = Command::new(BIN)
        .args(["set-info", "--title", "Replaced", "--in-place"])
        .arg(&pdf)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let after = std::fs::read(&pdf).unwrap();
    // Incremental: the original bytes are a prefix and a revision follows.
    assert!(after.len() > before.len() && after.starts_with(&before));
    assert!(after.windows(8).any(|w| w == b"Replaced"));
    std::fs::remove_file(&pdf).unwrap();
}

#[test]
fn refused_in_place_edit_leaves_the_input_untouched() {
    let pdf = scratch_copy("refused");
    let before = std::fs::read(&pdf).unwrap();
    // Deleting every page of a document is refused.
    let out = Command::new(BIN)
        .args(["delete-pages", "--pages", "1", "--in-place"])
        .arg(&pdf)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(9), "EDIT_REFUSED");
    assert_eq!(std::fs::read(&pdf).unwrap(), before);
    std::fs::remove_file(&pdf).unwrap();
}

//! `render-page --soft-proof`: the separation preview through a CMYK output
//! profile.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture(rel: &str) -> String {
    format!(
        "{}/../../fixtures/synthetic/{rel}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn render(tag: &str, extra: &[&str]) -> (Option<i32>, String, Vec<u8>) {
    let png = std::env::temp_dir().join(format!(
        "pdfcer_soft_proof_{tag}_{}.png",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&png);
    let out = Command::new(BIN)
        .args([
            "render-page",
            &fixture("overprint/cmyk_k_op_over_spot.pdf"),
            "--scale",
            "0.25",
            "-o",
        ])
        .arg(&png)
        .args(extra)
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        std::fs::read(&png).unwrap_or_default(),
    )
}

#[test]
fn a_cmyk_output_profile_changes_the_preview() {
    let icc = fixture("icc-rgb/v4-cmyk-mab-lab.icc");
    let (plain_code, plain_err, plain) = render("plain", &["--separation-preview"]);
    assert_eq!(plain_code, Some(0), "{plain_err}");
    let (code, err, proofed) = render("proofed", &["--soft-proof", &icc]);
    assert_eq!(code, Some(0), "{err}");
    assert!(proofed.starts_with(b"\x89PNG"));
    assert_ne!(plain, proofed);
    assert!(
        err.contains("black point compensation cannot be applied"),
        "{err}"
    );
}

#[test]
fn a_profile_that_is_not_cmyk_is_refused() {
    let (code, err, _) = render("rgb", &["--soft-proof", &fixture("icc-rgb/swap-rg.icc")]);
    assert_eq!(code, Some(9), "{err}");
    assert!(err.contains("not CMYK"), "{err}");
}

#[test]
fn simulate_paper_needs_a_profile() {
    let (code, _, _) = render("needs", &["--simulate-paper"]);
    assert_eq!(code, Some(2));
}

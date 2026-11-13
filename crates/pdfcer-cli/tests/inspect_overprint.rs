//! `pdfcer inspect --overprint`: whether each page uses overprint, set and
//! in effect.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn inspect(fixture: &str) -> (Option<i32>, String) {
    let path = format!(
        "{}/../../fixtures/synthetic/overprint/{fixture}",
        env!("CARGO_MANIFEST_DIR")
    );
    let out = Command::new(BIN)
        .args(["inspect", &path, "--overprint"])
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn black_overprinting_a_spot_is_set_and_in_effect() {
    let (code, out) = inspect("cmyk_k_op_over_spot.pdf");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("page=1 overprint=set effect=1"), "{out}");
    assert!(
        out.contains("pages=1 overprint_set=1 overprint_effect=1 unknown=0"),
        "{out}"
    );
}

#[test]
fn devicen_naming_every_process_ink_is_set_without_effect() {
    let (code, out) = inspect("devicen_all4_op_cmyk.pdf");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("page=1 overprint=set effect=0"), "{out}");
}

#[test]
fn a_page_without_overprint_says_none() {
    let (code, out) = inspect("spot_only_noop_cmyk.pdf");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("page=1 overprint=none effect=0"), "{out}");
}

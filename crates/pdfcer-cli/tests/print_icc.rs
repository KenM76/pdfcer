//! `print --printer-profile` argument handling and `list-icc-profiles`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const PDF: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/synthetic/hello.pdf"
);
const CMYK: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/synthetic/icc-rgb/dest-cmyk.icc"
);

#[test]
fn list_icc_profiles_ends_with_a_count_line() {
    for args in [&["list-icc-profiles"][..], &["list-icc-profiles", "--all"]] {
        let out = Command::new(BIN).args(args).output().unwrap();
        assert!(out.status.success(), "{out:?}");
        let text = String::from_utf8(out.stdout).unwrap();
        let last = text.lines().last().unwrap();
        assert!(last.starts_with("list-icc-profiles count="), "{text}");
        let n: usize = last.rsplit('=').next().unwrap().parse().unwrap();
        assert_eq!(
            text.lines()
                .filter(|l| l.starts_with("icc-profile "))
                .count(),
            n
        );
        if args.len() == 1 {
            assert!(!text.contains("printable=0"), "{text}");
        }
    }
}

#[test]
fn a_cmyk_printer_profile_is_refused_before_anything_prints() {
    let out = Command::new(BIN)
        .args(["print", PDF, "--printer-profile", CMYK])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(
        err.contains("--printer-profile") && err.contains("CMYK"),
        "{err}"
    );
}

#[test]
fn printer_intent_needs_a_printer_profile() {
    let out = Command::new(BIN)
        .args(["print", PDF, "--printer-intent", "absolute"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
}

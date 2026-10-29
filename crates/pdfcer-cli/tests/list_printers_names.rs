//! `pdfcer list-printers --names-only` names the same printers, in the same
//! order and with the same default, as the full listing, without driver or
//! port.

#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn run(args: &[&str]) -> String {
    let out = Command::new(BIN).args(args).output().expect("run pdfcer");
    assert!(out.status.success(), "{args:?}: {out:?}");
    String::from_utf8(out.stdout).expect("utf-8")
}

#[test]
fn names_only_lists_the_full_listings_names_and_default() {
    let full = run(&["list-printers"]);
    let names = run(&["list-printers", "--names-only"]);
    // `printer name="X" driver=".." port=".." default=N` reduced to the
    // `--names-only` shape: `printer name="X" default=N`.
    let expected: Vec<String> = full
        .lines()
        .map(|l| match (l.find(" driver="), l.rfind(" default=")) {
            (Some(d), Some(f)) => format!("{}{}", &l[..d], &l[f..]),
            _ => l.to_owned(),
        })
        .collect();
    assert_eq!(names.lines().collect::<Vec<_>>(), expected);
    assert!(!names.contains("driver=") && !names.contains("port="));
}

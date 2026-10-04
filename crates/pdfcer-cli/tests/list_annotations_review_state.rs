//! `pdfcer list-annotations` prints a review-status reply's `/IRT`,
//! `/State` and `/StateModel` (ISO 32000-1 §12.5.6.3, Table 171) as
//! `in_reply_to=`, `state=` and `state_model=`, appended to the stable line.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn run(args: &[&str]) -> String {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn a_status_reply_names_its_target_state_and_model() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/annot/ap-cascade-single-stream.pdf");
    let out = std::env::temp_dir().join(format!("pdfcer_review_state_{}.pdf", std::process::id()));
    run(&[
        "set-review-state",
        src.to_str().unwrap(),
        "--page",
        "1",
        "--index",
        "0",
        "--state",
        "accepted",
        "--author",
        "Ken",
        "-o",
        out.to_str().unwrap(),
    ]);
    let listing = run(&["list-annotations", out.to_str().unwrap()]);
    let lines: Vec<&str> = listing
        .lines()
        .filter(|l| l.starts_with("annot "))
        .collect();
    let target = lines
        .iter()
        .find(|l| l.contains(" state=none "))
        .expect("the target keeps no state");
    assert!(target.contains(" in_reply_to=none "), "{target}");
    let reply = lines
        .iter()
        .find(|l| l.contains(" state=\"Accepted\""))
        .unwrap_or_else(|| panic!("a status reply: {listing}"));
    assert!(
        reply.ends_with(" in_reply_to=4 state=\"Accepted\" state_model=\"Review\""),
        "{reply}"
    );
    let _ = std::fs::remove_file(&out);
}

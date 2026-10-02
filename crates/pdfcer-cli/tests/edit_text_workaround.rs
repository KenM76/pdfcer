//! `pdfcer edit-text --workaround` over the real binary (decision 175).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

fn run(tag: &str, extra: &[&str]) -> (Output, PathBuf) {
    let out = std::env::temp_dir().join(format!("pdfcer_wa_{tag}_{}.pdf", std::process::id()));
    let o = Command::new(BIN)
        .arg("edit-text")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/synthetic/text/workaround-seam.pdf"),
        )
        .args(["--page", "1", "--find", "Hello", "--replace", "Howdy", "-o"])
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
fn without_the_flag_the_refusal_names_the_workaround_and_the_flag() {
    let (o, out) = run("off", &[]);
    let all = text(&o);
    assert_eq!(o.status.code(), Some(EDIT_REFUSED), "{all}");
    assert!(all.contains("a workaround is on offer"), "{all}");
    assert!(all.contains("re-run with --workaround"), "{all}");
    assert!(!out.exists(), "a refusal writes nothing");
}

#[test]
fn with_the_flag_the_run_is_retyped_and_the_disclosure_printed() {
    let (o, out) = run("on", &["--workaround"]);
    let all = text(&o);
    assert_eq!(o.status.code(), Some(0), "{all}");
    assert!(all.contains("  workaround=retype"), "{all}");
    assert!(all.contains("workaround (retype, approximate)"), "{all}");
    let saved = std::fs::read(&out).unwrap();
    assert!(saved.windows(7).any(|w| w == b"(Howdy)"), "the new run");
    let _ = std::fs::remove_file(out);
}

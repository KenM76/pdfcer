//! `pdfcer edit-text --cid-font-program` (decision 187) over the real binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

fn run(mode: &str) -> (Output, PathBuf, String) {
    let out = std::env::temp_dir().join(format!("pdfcer_cidp_{mode}_{}.pdf", std::process::id()));
    let input = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text/word-shaped-subset-shared-tounicode.pdf");
    let o = Command::new(BIN)
        .arg("edit-text")
        .arg(&input)
        .args(["--page", "1", "--find", "ABC", "--replace", "AB\u{394}"])
        .args(["--cid-font-program", mode, "-o"])
        .arg(&out)
        .output()
        .unwrap();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    (o, out, all)
}

#[test]
fn each_mode_reaches_the_engine_and_is_printed() {
    for (mode, expect) in [("strip", "stripped-copy"), ("share", "shared-with-cmap")] {
        let (o, out, all) = run(mode);
        assert_eq!(o.status.code(), Some(0), "{all}");
        assert!(all.contains("source=same-program"), "{all}");
        assert!(all.contains(&format!("cid_font_program={expect}")), "{all}");
        let _ = std::fs::remove_file(out);
    }
}

#[test]
fn off_refuses() {
    let (o, out, all) = run("off");
    assert_eq!(o.status.code(), Some(EDIT_REFUSED), "{all}");
    assert!(!out.exists());
}

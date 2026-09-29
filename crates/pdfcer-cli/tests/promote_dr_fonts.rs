//! `promote-dr-fonts` over the real binary.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// The inline `/Helv` becomes a reference, reported as `promoted=1`; a second
/// run promotes nothing and leaves the file byte-identical.
#[test]
fn promote_dr_fonts_makes_an_inline_font_indirect_once() {
    let src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/forms/demo-form.pdf");
    let pdf = std::env::temp_dir().join(format!("pdfcer_promote_dr_{}.pdf", std::process::id()));
    std::fs::copy(src, &pdf).unwrap();
    let run = || {
        Command::new(BIN)
            .args(["promote-dr-fonts", "--in-place"])
            .arg(&pdf)
            .output()
            .unwrap()
    };

    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(String::from_utf8_lossy(&first.stdout).ends_with("promoted=1\n"));
    let bytes = std::fs::read(&pdf).unwrap();
    let tail = String::from_utf8_lossy(&bytes[bytes.len() - 600..]).into_owned();
    assert!(tail.contains("/Font <</Helv 8 0 R>>"), "{tail}");

    let second = run();
    assert!(second.status.success());
    assert!(String::from_utf8_lossy(&second.stdout).ends_with("promoted=0\n"));
    assert_eq!(std::fs::read(&pdf).unwrap(), bytes);
    std::fs::remove_file(&pdf).unwrap();
}

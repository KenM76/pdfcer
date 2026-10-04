//! CLI tests for `--allow-rc4-append` (decision 190): an edit to an RC4
//! document is refused by default and names the flag; with the flag it
//! appends under the document's RC4 key and prints the keystream warning.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/encryption")
        .join(name)
}

fn out_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "pdfcer-rc4-append-{tag}-{}.pdf",
        std::process::id()
    ))
}

fn rotate(input: &Path, output: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["--open-password", "ownerpw"];
    args.extend_from_slice(extra);
    args.extend_from_slice(&[
        "rotate-page",
        input.to_str().unwrap(),
        "--page",
        "1",
        "--degrees",
        "90",
        "-o",
        output.to_str().unwrap(),
    ]);
    Command::new(BIN).args(&args).output().expect("pdfcer runs")
}

#[test]
fn rc4_edit_is_refused_by_default_and_names_the_flag() {
    let output = out_path("refused");
    let out = rotate(&fixture("enc-rc4-128.pdf"), &output, &[]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_ne!(out.status.code(), Some(0), "{err}");
    assert!(err.contains("--allow-rc4-append"), "{err}");
    assert!(!output.exists());
}

#[test]
fn allow_rc4_append_appends_and_warns() {
    for name in ["enc-rc4-40.pdf", "enc-rc4-128-v4.pdf"] {
        let input = fixture(name);
        let output = out_path(name);
        let out = rotate(&input, &output, &["--allow-rc4-append"]);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(0), "{name}: {err}");
        assert!(
            err.contains("kept the document's RC4 encryption"),
            "{name}: {err}"
        );
        assert!(
            err.contains("existing object(s) were re-encrypted"),
            "{name}: {err}"
        );
        let original = std::fs::read(&input).unwrap();
        let edited = std::fs::read(&output).unwrap();
        let _ = std::fs::remove_file(&output);
        assert_eq!(&edited[..original.len()], &original[..], "{name}: appends");
        assert!(edited.len() > original.len(), "{name}");
    }
}

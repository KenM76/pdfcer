//! `pdfcer set-markup-style --interior` on a `/Line` writes `/IC`, which
//! fills its closed arrowhead (pdfcer-gui request G153).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// One `/Line` with a closed arrowhead at its end and no `/IC`.
fn fixture(tag: &str) -> PathBuf {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Annots [4 0 R] >>",
        "<< /Type /Annot /Subtype /Line /Rect [0 0 100 60] /L [10 10 80 40] /C [1 0 0] \
         /LE [/None /ClosedArrow] >>",
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    let path = std::env::temp_dir().join(format!("pdfcer_line_{tag}_{}.pdf", std::process::id()));
    std::fs::write(&path, buf).unwrap();
    path
}

#[test]
fn interior_on_a_line_writes_ic() {
    let input = fixture("interior");
    let output = input.with_extension("out.pdf");
    let out = Command::new(BIN)
        .arg("set-markup-style")
        .arg(&input)
        .args(["--page", "1", "--index", "0", "--interior", "0000FF", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let saved = std::fs::read(&output).unwrap();
    let appended = &saved[std::fs::metadata(&input).unwrap().len() as usize..];
    let text = String::from_utf8_lossy(appended);
    assert!(text.contains("/IC [0.0 0.0 1.0]"), "{text}");
    for f in [input, output] {
        let _ = std::fs::remove_file(f);
    }
}

fn restyle_endings(input: &PathBuf, value: &str) -> (std::process::Output, PathBuf) {
    let output = input.with_extension(format!("{}.pdf", value.replace(',', "_")));
    let out = Command::new(BIN)
        .arg("set-markup-style")
        .arg(input)
        .args(["--page", "1", "--index", "0", "--line-endings", value, "-o"])
        .arg(&output)
        .output()
        .unwrap();
    (out, output)
}

/// `--line-endings` takes every ISO 32000-2 Table 179 style by name, in any
/// case, and writes it as `/LE`.
#[test]
fn line_endings_take_every_table_179_style() {
    let input = fixture("endings");
    for style in pdfcer_core::annot_author::LineEnding::ALL {
        let name = String::from_utf8_lossy(style.name()).into_owned();
        let value = format!("{},{name}", name.to_ascii_lowercase());
        let (out, output) = restyle_endings(&input, &value);
        assert!(out.status.success(), "{value}: {out:?}");
        let saved = std::fs::read(&output).unwrap();
        let appended = &saved[std::fs::metadata(&input).unwrap().len() as usize..];
        let text = String::from_utf8_lossy(appended);
        assert!(
            text.contains(&format!("/LE [/{name} /{name}]")),
            "{value}: {text}"
        );
        let _ = std::fs::remove_file(output);
    }
    let (out, _) = restyle_endings(&input, "arrow,none");
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("`arrow` is not one of"), "{err}");
    let _ = std::fs::remove_file(input);
}

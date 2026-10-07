//! `pdfcer set-text-annot-style` on a text box another program drew: refused
//! (exit 9) unless `--redraw-as-plain` (pdfcer-gui request G148).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// One `/FreeText` whose `/AP` is a blue rectangle no pdfcer layout draws.
fn fixture(tag: &str) -> PathBuf {
    let ap = "0 0 1 rg 0 0 100 70 re f\n";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Annots [4 0 R] >>".to_owned(),
        "<< /Type /Annot /Subtype /FreeText /Rect [20 20 120 90] /Contents (alpha beta) \
         /DA (/Helv 12 Tf 0 g) /AP << /N 5 0 R >> >>"
            .to_owned(),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 100 70] /Length {} >>\nstream\n{ap}endstream",
            ap.len()
        ),
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
    let path = std::env::temp_dir().join(format!(
        "pdfcer_text_style_redraw_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(input: &PathBuf, output: &PathBuf, extra: &[&str]) -> Output {
    Command::new(BIN)
        .arg("set-text-annot-style")
        .arg(input)
        .args(["--page", "1", "--index", "0", "--color", "FF0000"])
        .args(extra)
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

#[test]
fn a_foreign_text_box_is_refused_then_redrawn_on_request() {
    let input = fixture("foreign");
    let output = input.with_extension("out.pdf");
    let out = run(&input, &output, &[]);
    assert_eq!(out.status.code(), Some(9), "{out:?}");
    assert!(!output.exists());

    let out = run(&input, &output, &["--redraw-as-plain"]);
    assert!(out.status.success(), "{out:?}");
    let line = String::from_utf8_lossy(&out.stdout);
    assert!(line.contains("was_foreign=1"), "{line}");
    for f in [input, output] {
        let _ = std::fs::remove_file(f);
    }
}

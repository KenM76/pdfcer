//! `pdfcer set-annot-opacity` and `set-marker-color` (pdfcer-gui request
//! G150).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A page with one pdfcer-style caret whose appearance is foreign.
fn fixture(tag: &str) -> PathBuf {
    let content = "0 0 1 rg 0 0 10 10 re f";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Annots [4 0 R] >>".to_owned(),
        "<< /Type /Annot /Subtype /Caret /Rect [100 100 110 110] /C [0 0 1] \
         /AP << /N 5 0 R >> >>"
            .to_owned(),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length {} >>\nstream\n{content}\nendstream",
            content.len()
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
        "pdfcer_annot_restyle_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&str], input: &PathBuf, output: &PathBuf) -> std::process::Output {
    Command::new(BIN)
        .arg(args[0])
        .arg(input)
        .args(&args[1..])
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

fn appended(input: &PathBuf, output: &PathBuf) -> String {
    let saved = std::fs::read(output).unwrap();
    String::from_utf8_lossy(&saved[std::fs::metadata(input).unwrap().len() as usize..]).into_owned()
}

#[test]
fn opacity_writes_ca() {
    let input = fixture("ca");
    let output = input.with_extension("out.pdf");
    let out = run(
        &[
            "set-annot-opacity",
            "--page",
            "1",
            "--index",
            "0",
            "--opacity",
            "0.25",
        ],
        &input,
        &output,
    );
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("current=0.25"));
    assert!(appended(&input, &output).contains("/CA 0.25"));
    for f in [input, output] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn marker_color_refuses_a_foreign_icon_then_redraws_it() {
    let input = fixture("mc");
    let output = input.with_extension("out.pdf");
    let args = [
        "set-marker-color",
        "--page",
        "1",
        "--index",
        "0",
        "--color",
        "00FF00",
    ];
    let out = run(&args, &input, &output);
    assert_eq!(out.status.code(), Some(9), "{out:?}");

    let mut redraw = args.to_vec();
    redraw.push("--redraw-as-plain");
    let out = run(&redraw, &input, &output);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("was_foreign=1"));
    let text = appended(&input, &output);
    assert!(text.contains("/C [0.0 1.0 0.0]"), "{text}");
    assert!(text.contains("0 1 0 rg"), "{text}");
    for f in [input, output] {
        let _ = std::fs::remove_file(f);
    }
}

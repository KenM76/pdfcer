//! `pdfcer respan-markup` replaces a text markup's quads (pdfcer-gui
//! request G152).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn blank(tag: &str) -> PathBuf {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >>",
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    let path = std::env::temp_dir().join(format!(
        "pdfcer_respan_markup_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

fn pdfcer(args: &[&str], input: &Path, output: &Path) -> Output {
    Command::new(BIN)
        .arg(args[0])
        .arg(input)
        .args(&args[1..])
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

fn highlighted(tag: &str) -> (PathBuf, PathBuf) {
    let input = blank(tag);
    let marked = input.with_extension("marked.pdf");
    let out = pdfcer(
        &[
            "annotate",
            "--type",
            "highlight",
            "--page",
            "1",
            "--rect",
            "20,100,120,112",
            "--note",
            "check this",
        ],
        &input,
        &marked,
    );
    assert!(out.status.success(), "{out:?}");
    (input, marked)
}

fn respan(input: &Path, output: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["respan-markup", "--page", "1", "--index", "0"];
    args.extend_from_slice(extra);
    pdfcer(&args, input, output)
}

#[test]
fn quads_replace_the_span_and_keep_the_comment() {
    let (input, marked) = highlighted("quads");
    let output = input.with_extension("out.pdf");
    let out = respan(
        &marked,
        &output,
        &[
            "--quads",
            "20,112,250,112,20,100,250,100 ; 20,98,60,98,20,86,60,86",
        ],
    );
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("quads_before=1 quads_after=2"), "{stdout}");
    let saved = String::from_utf8_lossy(&std::fs::read(&output).unwrap()).into_owned();
    let tail = &saved[saved.rfind("/Highlight").unwrap()..];
    assert!(tail.contains("(check this)"), "comment kept: {tail}");
    for f in [input, marked, output] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn rect_gives_one_quad() {
    let (input, marked) = highlighted("rect");
    let output = input.with_extension("out.pdf");
    let out = respan(&marked, &output, &["--rect", "20,100,60,112"]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("quads_before=1 quads_after=1"));
    for f in [input, marked, output] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn no_geometry_or_a_shape_is_refused() {
    let (input, marked) = highlighted("refuse");
    let output = input.with_extension("never.pdf");
    let out = respan(&marked, &output, &[]);
    assert_eq!(out.status.code(), Some(9), "{out:?}");
    let square = input.with_extension("square.pdf");
    let out = pdfcer(
        &[
            "annotate",
            "--type",
            "square",
            "--page",
            "1",
            "--rect",
            "10,10,50,50",
        ],
        &input,
        &square,
    );
    assert!(out.status.success(), "{out:?}");
    let out = respan(&square, &output, &["--rect", "0,0,10,10"]);
    assert_eq!(out.status.code(), Some(9), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a text markup"));
    assert!(!output.exists());
    for f in [input, marked, square] {
        let _ = std::fs::remove_file(f);
    }
}

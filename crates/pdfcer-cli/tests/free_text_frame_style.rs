//! `pdfcer annotate --type freetext --background/--dash` and
//! `pdfcer set-text-annot-style` frame and text flags (pdfcer-gui request
//! G149).

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
    let path =
        std::env::temp_dir().join(format!("pdfcer_ft_frame_{tag}_{}.pdf", std::process::id()));
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

fn placed(tag: &str, extra: &[&str]) -> (PathBuf, PathBuf) {
    let input = blank(tag);
    let boxed = input.with_extension("boxed.pdf");
    let mut args = vec![
        "annotate",
        "--type",
        "freetext",
        "--page",
        "1",
        "--rect",
        "20,20,220,90",
        "--text",
        "frame me",
        "--fill",
        "FF0000",
    ];
    args.extend_from_slice(extra);
    let out = pdfcer(&args, &input, &boxed);
    assert!(out.status.success(), "{out:?}");
    (input, boxed)
}

#[test]
fn annotate_draws_the_background_and_dash() {
    let (input, boxed) = placed("annotate", &["--background", "FFFF00", "--dash", "4,2"]);
    let bytes = String::from_utf8_lossy(&std::fs::read(&boxed).unwrap()).into_owned();
    assert!(bytes.contains("/IC [1.0 1.0 0.0]"), "fill recorded");
    assert!(bytes.contains("/D [4.0 2.0]"), "dash recorded");
    assert!(bytes.contains("[4 2] 0 d"), "dash painted");
    for f in [input, boxed] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn background_is_refused_on_a_square() {
    let input = blank("square");
    let output = input.with_extension("out.pdf");
    let out = pdfcer(
        &[
            "annotate",
            "--type",
            "square",
            "--page",
            "1",
            "--rect",
            "20,20,120,90",
            "--background",
            "FFFF00",
        ],
        &input,
        &output,
    );
    assert_eq!(out.status.code(), Some(9), "{out:?}");
    assert!(!output.exists());
    let _ = std::fs::remove_file(input);
}

#[test]
fn set_text_annot_style_restyles_frame_and_text() {
    let (input, boxed) = placed("restyle", &["--background", "FFFF00"]);
    let output = input.with_extension("styled.pdf");
    let out = pdfcer(
        &[
            "set-text-annot-style",
            "--page",
            "1",
            "--index",
            "0",
            "--fill",
            "none",
            "--border-width",
            "3",
            "--dash",
            "2,1",
            "--opacity",
            "0.5",
            "--text-color",
            "00FF00",
            "--font",
            "Times-Bold",
        ],
        &boxed,
        &output,
    );
    assert!(out.status.success(), "{out:?}");
    let line = String::from_utf8_lossy(&out.stdout);
    assert!(
        line.contains("opacity_written=1 frame_written=1 text_style_written=1"),
        "{line}"
    );
    let bytes = String::from_utf8_lossy(&std::fs::read(&output).unwrap()).into_owned();
    let tail = &bytes[bytes.rfind("/FreeText").unwrap()..];
    assert!(tail.contains("/CA 0.5"), "{tail}");
    assert!(tail.contains("/W 3"), "{tail}");
    assert!(
        !tail[..tail.find("endobj").unwrap()].contains("/IC"),
        "{tail}"
    );
    assert!(bytes.contains("Times-Bold"));
    for f in [input, boxed, output] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn a_bad_flag_or_no_flag_is_refused_before_the_file_is_touched() {
    let (input, boxed) = placed("refuse", &[]);
    let output = input.with_extension("never.pdf");
    let base = ["set-text-annot-style", "--page", "1", "--index", "0"];
    for extra in [
        &[][..],
        &["--opacity", "1.5"][..],
        &["--dash", "0,0"][..],
        &["--font", "Wingdings"][..],
        &["--border-width=-1"][..],
    ] {
        let mut args = base.to_vec();
        args.extend_from_slice(extra);
        let out = pdfcer(&args, &boxed, &output);
        assert_eq!(out.status.code(), Some(9), "{extra:?}: {out:?}");
        assert!(!output.exists());
    }
    for f in [input, boxed] {
        let _ = std::fs::remove_file(f);
    }
}

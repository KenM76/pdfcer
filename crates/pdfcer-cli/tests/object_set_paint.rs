//! `pdfcer set-object-paint` — recolours device-colour paths, refuses a
//! spot ink by name, and refuses a bad index before changing anything.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::{Rgb, VectorObject};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// Object 0: a blue `DeviceRGB` stroke. Object 1: a stroke in the
/// `/Separation` ink `/CS0` named `Cutline`.
fn two_paths(tag: &str) -> PathBuf {
    let content = "0 0 1 RG 0 0 10 10 re S /CS0 CS 1 SC 20 20 10 10 re S";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
         /Resources << /ColorSpace << /CS0 [/Separation /Cutline /DeviceRGB 5 0 R] >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [1 0 0] /N 1 >>".to_owned(),
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
    let path =
        std::env::temp_dir().join(format!("pdfcer_set_paint_{tag}_{}.pdf", std::process::id()));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(input: &Path, output: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("set-object-paint")
        .arg(input)
        .args(args)
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

fn stroke(path: &Path, index: usize) -> Option<Rgb> {
    let mut s = EditSession::new(Document::from_bytes(std::fs::read(path).unwrap()).unwrap());
    let model = s.page_objects(0).unwrap();
    let VectorObject::Path(p) = &model.objects[index] else {
        panic!("object {index} is not a path");
    };
    p.stroke_paint.rgb()
}

#[test]
fn recolours_a_device_path_and_refuses_a_spot_ink_by_name() {
    let input = two_paths("mixed");
    let output = input.with_extension("out.pdf");
    let out = run(
        &input,
        &output,
        &["--objects", "0,1", "--stroke", "#ff0000"],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("changed=0 refused=1"), "{stdout}");
    assert!(
        stderr.contains("object 1 not recoloured") && stderr.contains("/CS0"),
        "{stderr}"
    );
    assert_eq!(
        stroke(&output, 0),
        Some(Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0
        })
    );
}

#[test]
fn an_out_of_range_index_refuses_the_whole_call() {
    let input = two_paths("range");
    let output = input.with_extension("out.pdf");
    let out = run(&input, &output, &["--objects", "0,7", "--fill", "0,1,0"]);
    assert_eq!(
        out.status.code(),
        Some(9),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!output.exists());
}

#[test]
fn a_colour_is_required() {
    let input = two_paths("none");
    let output = input.with_extension("out.pdf");
    let out = run(&input, &output, &["--objects", "0"]);
    assert!(!out.status.success());
}

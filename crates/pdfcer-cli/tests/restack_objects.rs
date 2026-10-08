//! `pdfcer restack-objects` — moves an object in paint order keeping its
//! state, reports a clipped object as limited, and refuses a bad index.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::VectorObject;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// 0 a blue square; 1 a red square under `/GS1` (`/ca 0.5`); 2 the clip
/// path (it paints nothing); 3 a square under that clip.
const CONTENT: &str = "0 0 1 rg 0 0 10 10 re f\n/GS1 gs 1 0 0 rg 5 5 10 10 re f\n\
                       q 0 0 50 50 re W n 0 1 0 rg 0 0 40 40 re f Q\n";

fn fixture(tag: &str) -> PathBuf {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
         /Resources << /ExtGState << /GS1 << /ca 0.5 >> >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{CONTENT}endstream",
            CONTENT.len()
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
    let path =
        std::env::temp_dir().join(format!("pdfcer_restack_{tag}_{}.pdf", std::process::id()));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(input: &Path, output: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("restack-objects")
        .arg(input)
        .args(args)
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

/// Each object's fill colour and fill alpha, in paint order.
fn fills(file: &Path) -> Vec<String> {
    let mut s = EditSession::new(Document::from_bytes(std::fs::read(file).unwrap()).unwrap());
    let model = s.page_objects(0).unwrap();
    model
        .objects
        .iter()
        .map(|o| match o {
            VectorObject::Path(p) => format!("{:?} {}", p.fill_color, p.fill_alpha),
            _ => panic!("not a path"),
        })
        .collect()
}

#[test]
fn back_is_a_no_op_and_forward_keeps_opacity() {
    let input = fixture("front");
    let output = input.with_extension("out.pdf");
    let before = fills(&input);
    let out = run(
        &input,
        &output,
        &["--objects", "0", "--to", "back", "--verify-undo"],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(stdout.contains("moved=none"), "{stdout}");
    let out = run(
        &input,
        &output,
        &["--objects", "0", "--to", "forward", "--verify-undo"],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("indices=1 moved=0 limited=0"), "{stdout}");
    let after = fills(&output);
    assert_eq!(
        after,
        vec![
            before[1].clone(),
            before[0].clone(),
            before[2].clone(),
            before[3].clone()
        ]
    );
    assert!(after[1].ends_with(" 1"), "{after:?}");
}

#[test]
fn an_object_under_a_clip_is_limited_and_says_so() {
    let input = fixture("clip");
    let output = input.with_extension("out.pdf");
    let out = run(&input, &output, &["--objects", "3", "--to", "back"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("limited=1"), "{stdout}");
    assert!(stderr.contains("object 3 limited"), "{stderr}");
}

#[test]
fn an_out_of_range_index_exits_9_and_writes_nothing() {
    let input = fixture("range");
    let output = input.with_extension("out.pdf");
    let _ = std::fs::remove_file(&output);
    let out = run(&input, &output, &["--objects", "9", "--to", "front"]);
    assert_eq!(out.status.code(), Some(9));
    assert!(!output.exists());
}

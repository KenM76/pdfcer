//! `pdfcer group-length` reads typed scaled lengths back to page user units
//! and shows page lengths in the group's scale, honouring `/UserUnit`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdfcer_glen_{tag}_{}.pdf", std::process::id()))
}

/// One page with `/UserUnit 2`.
fn fixture() -> PathBuf {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /UserUnit 2 /Resources << >> >>",
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
    let p = path("in");
    std::fs::write(&p, buf).unwrap();
    p
}

fn pdfcer(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

/// A metre group at 1:50.
fn scaled() -> PathBuf {
    let input = fixture();
    let (grouped, out) = (path("g"), path("s"));
    let s = |p: &Path| p.to_str().unwrap().to_owned();
    ok(&pdfcer(&[
        "group-add",
        &s(&input),
        "--name",
        "site",
        "--unit",
        "m",
        "-o",
        &s(&grouped),
    ]));
    ok(&pdfcer(&[
        "group-set-scale",
        &s(&grouped),
        "--group",
        "1",
        "--unit",
        "m",
        "--ratio",
        "1:50",
        "-o",
        &s(&out),
    ]));
    out
}

fn value(line: &str, key: &str) -> f64 {
    let at = line.find(&format!("{key}=")).unwrap() + key.len() + 1;
    line[at..].split(' ').next().unwrap().parse().unwrap()
}

#[test]
fn typed_lengths_read_back_through_the_scale_and_user_unit() {
    let file = scaled();
    let f = file.to_str().unwrap();
    let out = ok(&pdfcer(&[
        "group-length",
        f,
        "--group",
        "1",
        "--typed",
        "2.4 m",
        "--typed",
        "2400mm",
        "--typed",
        "-1.5",
        "--user-units",
        "36",
    ]));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 5, "{out}");
    // 2.4 m at 1:50 is 48 mm printed, 136.06 pt, 68.03 user units at /UserUnit 2.
    let expect = 2.4 / 50.0 / 0.0254 * 72.0 / 2.0;
    for line in &lines[1..3] {
        assert!((value(line, "user_units") - expect).abs() < 1e-9, "{line}");
        assert!(line.ends_with("readout=\"2.400 m\""), "{line}");
    }
    assert!((value(lines[3], "user_units") + 1.5 / 2.4 * expect).abs() < 1e-9);
    // 36 user units are 72 pt: one inch printed, 1.27 m at 1:50.
    assert_eq!(lines[4], "user_units=36 readout=\"1.270 m\"");
}

#[test]
fn bad_text_an_unknown_group_and_an_unknown_page_are_refused() {
    let file = scaled();
    let f = file.to_str().unwrap();
    let bad = pdfcer(&[
        "group-length",
        f,
        "--group",
        "1",
        "--typed",
        "abc",
        "--typed",
        "1",
    ]);
    assert_eq!(bad.status.code(), Some(9));
    assert_eq!(String::from_utf8(bad.stdout).unwrap().lines().count(), 2);
    for args in [
        ["--group", "7", "--page", "1"],
        ["--group", "1", "--page", "2"],
    ] {
        let out = pdfcer(&[
            "group-length",
            f,
            args[0],
            args[1],
            args[2],
            args[3],
            "--typed",
            "1",
        ]);
        assert_eq!(out.status.code(), Some(9), "{args:?}");
        assert!(out.stdout.is_empty());
    }
}

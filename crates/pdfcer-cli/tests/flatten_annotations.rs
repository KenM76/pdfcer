//! `pdfcer flatten-annotations`: burn annotations into one page.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_flatten_annots_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

/// Index 0: a Square with a pop-up (index 1) and a reply without an
/// appearance (index 2); index 3: a Link.
fn source(tag: &str) -> PathBuf {
    let stream = |extra: &str, content: &str| {
        format!(
            "<< {extra} /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
    };
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Annots [5 0 R 6 0 R 7 0 R 8 0 R] >>".to_owned(),
        stream("", "0 0 1 rg 0 0 200 10 re f"),
        "<< /Type /Annot /Subtype /Square /Rect [20 20 60 60] /F 4 /AP << /N 9 0 R >> /Popup 6 0 R >>".to_owned(),
        "<< /Type /Annot /Subtype /Popup /Rect [100 150 180 190] /Parent 5 0 R >>".to_owned(),
        "<< /Type /Annot /Subtype /Text /Rect [150 20 170 40] /F 4 /IRT 5 0 R /RT /R >>".to_owned(),
        "<< /Type /Annot /Subtype /Link /Rect [20 100 60 140] /F 4 >>".to_owned(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10]",
            "1 0 0 rg 0 0 10 10 re f",
        ),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
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
    let path = temp_path(&format!("{tag}_src"));
    std::fs::write(&path, buf).unwrap();
    path
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn every_burnable_annotation_is_burned_and_the_rest_listed() {
    let src = source("all");
    let out = temp_path("all");
    let o = run(&[
        "flatten-annotations",
        src.to_str().unwrap(),
        "--page",
        "1",
        "--mode",
        "full",
        "--verify-undo",
        "--output",
        out.to_str().unwrap(),
    ]);
    let text = stdout(&o);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{text}{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(
        text.contains("flattened=1 grouped=0 layered=0 popups=1 replies=1 skipped=2 changed=true"),
        "{text}"
    );
    assert!(
        text.contains("skipped: id=7 subtype=Text it has no appearance"),
        "{text}"
    );
    assert!(text.contains("skipped: id=8 subtype=Link"), "{text}");
    assert!(
        text.contains("disclosure: burned 1 annotation(s) into page 1"),
        "{text}"
    );
    let listed = stdout(&run(&["list-annotations", out.to_str().unwrap()]));
    assert!(
        !listed.contains("Square") && !listed.contains("Popup"),
        "{listed}"
    );
    assert!(
        listed.contains("Text") && listed.contains("Link"),
        "{listed}"
    );
}

#[test]
fn a_named_refused_annotation_exits_9_and_writes_nothing() {
    let src = source("refused");
    let out = temp_path("refused");
    let o = run(&[
        "flatten-annotations",
        src.to_str().unwrap(),
        "--page",
        "1",
        "--index",
        "0",
        "--index",
        "3",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(EDIT_REFUSED));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("cannot be flattened: it is a link"), "{err}");
    assert!(!out.exists());
}

#[test]
fn dry_run_writes_nothing_and_page_zero_is_refused() {
    let src = source("dry");
    let o = run(&[
        "flatten-annotations",
        src.to_str().unwrap(),
        "--page",
        "1",
        "--index",
        "0",
        "--dry-run",
    ]);
    assert_eq!(o.status.code(), Some(0));
    assert!(
        stdout(&o).contains("page=1 dry-run; flattened=1"),
        "{}",
        stdout(&o)
    );
    let o = run(&[
        "flatten-annotations",
        src.to_str().unwrap(),
        "--page",
        "0",
        "--dry-run",
    ]);
    assert_eq!(o.status.code(), Some(EDIT_REFUSED));
}

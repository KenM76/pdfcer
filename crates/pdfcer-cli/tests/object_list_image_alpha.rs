//! `object-list --hit` honours image transparency as the GUI's click does,
//! and the summary line counts the ways its rows can differ from the render.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut s = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    s.extend_from_slice(data);
    s.extend_from_slice(b"\nendstream");
    s
}

/// A 100 x 100 page: a line across y = 50, then a 2 x 2 image over 30..70
/// whose left column a soft mask makes transparent, then `extra` content.
fn write_page(tag: &str, extra: &str) -> PathBuf {
    let content = format!("0 50 m 100 50 l S q 40 0 0 40 30 30 cm /Im0 Do Q {extra}");
    let grey = "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
                /ColorSpace /DeviceGray /BitsPerComponent 8";
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
          /Resources << /XObject << /Im0 5 0 R >> /ExtGState << /G0 << /CA 0 >> >> >> >>"
            .to_vec(),
        stream("", content.as_bytes()),
        stream(&format!("{grey} /SMask 6 0 R"), &[90, 90, 90, 90]),
        stream(grey, &[0, 255, 0, 255]),
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(o);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    let n = objects.len() + 1;
    pdf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    let path = std::env::temp_dir().join(format!(
        "pdfcer_objlist_alpha_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, pdf).unwrap();
    path
}

fn run(path: &PathBuf, args: &[&str]) -> Output {
    let out = Command::new(BIN)
        .arg("object-list")
        .arg(path)
        .args(args)
        .output()
        .expect("the binary runs");
    assert!(out.status.success(), "{out:?}");
    out
}

fn line<'a>(text: &'a str, prefix: &str) -> &'a str {
    text.lines()
        .find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no `{prefix}` line in:\n{text}"))
}

#[test]
fn a_hit_on_a_transparent_image_sample_falls_through_by_default() {
    let path = write_page("default", "");
    let clear = run(&path, &["--hit", "40,50"]);
    let clear = String::from_utf8_lossy(&clear.stdout).into_owned();
    let hit = line(&clear, "hit ");
    assert!(hit.contains(" index=0 kind=path"), "{hit}");
    assert!(hit.ends_with(" image_alpha=honour"), "{hit}");

    let opaque = run(&path, &["--hit", "60,50"]);
    let opaque = String::from_utf8_lossy(&opaque.stdout).into_owned();
    assert!(line(&opaque, "hit ").contains(" index=1 kind=image"));
    std::fs::remove_file(path).ok();
}

#[test]
fn image_alpha_ignore_hits_the_whole_outline() {
    let path = write_page("ignore", "");
    let out = run(
        &path,
        &["--hit", "40,50", "--image-alpha", "ignore", "--all-hits"],
    );
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let hit = line(&text, "hit ");
    assert!(hit.contains(" index=1 kind=image"), "{hit}");
    assert!(hit.contains(" candidates=2 image_alpha=ignore"), "{hit}");
    std::fs::remove_file(path).ok();
}

#[test]
fn the_summary_counts_and_names_render_divergences() {
    let path = write_page(
        "diag",
        "/Sh0 sh /OC /L1 BDC 0 0 m 5 5 l S EMC /G0 gs 0 0 m 9 9 l S",
    );
    let out = run(&path, &[]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let summary = line(&text, "object-list ");
    assert!(
        summary.ends_with(
            " undecoded_colour=0 invisible_by_alpha=1 shadings_unmodelled=1 oc_sections=1"
        ),
        "{summary}"
    );
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        err.contains("1 path(s) are listed but painted invisible"),
        "{err}"
    );
    assert!(err.contains("1 `sh` shading fill(s)"), "{err}");
    assert!(err.contains("1 optional-content section(s)"), "{err}");
    assert!(!err.contains("colour space"), "{err}");
    std::fs::remove_file(path).ok();
}

#[test]
fn a_clean_page_prints_no_divergence_note() {
    let path = write_page("clean", "");
    let out = run(&path, &[]);
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::remove_file(path).ok();
}

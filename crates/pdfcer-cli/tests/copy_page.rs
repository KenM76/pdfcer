//! # `pdfcer copy-page` integration tests (`Pass 248.2`)
//!
//! The refusals are tested everywhere; the placement itself is tested only
//! where a clipboard exists and only when asked, because a test that
//! silently overwrites the developer's clipboard on every `cargo test` is
//! the kind of side effect people stop running tests over. Set
//! `PDFCER_CLIPBOARD_TESTS=1` to run the round trip; CI's Windows job does.
//!
//! What the round trip checks is the CONTRACT other applications read:
//! the registered format names, the trailing NUL on the SVG payload, and
//! a PNG whose unpainted corner is transparent. Whether Word then inserts
//! an SVG graphic was measured by hand through combridge on 2026-09-03
//! (`docs/core-api/03-capabilities.md` §7.9) — a Word automation
//! dependency has no place in a test suite.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn build_pdf(objects: &[(u32, String)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.4\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let size = objects.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for num in 1..=objects.len() as u32 {
        let (_, off) = offsets.iter().find(|(n, _)| *n == num).unwrap();
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn one_page() -> Vec<u8> {
    let content = "1 0 0 rg 20 20 20 20 re f";
    build_pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] /Resources << >> >>"
                .into(),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ),
    ])
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("pdfcer-test-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().expect("spawn")
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn refusals_fire_before_the_clipboard_is_touched() {
    let dir = TempDir::new("copy-page-refusals");
    let input = dir.write("one.pdf", &one_page());
    let i = input.to_str().unwrap();

    let o = run(&["copy-page", i, "--no-svg", "--no-raster", "--no-pdf"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("nothing to copy"), "{}", stderr(&o));

    let o = run(&["copy-page", i, "--dpi", "0"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("--dpi"), "{}", stderr(&o));

    let o = run(&["copy-page", i, "--page", "2"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("out of range"), "{}", stderr(&o));

    let o = run(&["copy-page", i, "--background", "blue"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("--background"), "{}", stderr(&o));
}

#[cfg(not(windows))]
#[test]
fn a_non_windows_build_refuses_by_name_and_points_at_export_image() {
    let dir = TempDir::new("copy-page-unsupported");
    let input = dir.write("one.pdf", &one_page());
    let o = run(&["copy-page", input.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("Windows only"), "{}", stderr(&o));
    assert!(stderr(&o).contains("export-image"), "{}", stderr(&o));
}

#[cfg(windows)]
#[test]
fn the_clipboard_round_trip_when_asked_for() {
    if std::env::var_os("PDFCER_CLIPBOARD_TESTS").is_none() {
        println!("copy_page: PDFCER_CLIPBOARD_TESTS not set; the clipboard round trip did not run");
        return;
    }
    let dir = TempDir::new("copy-page-roundtrip");
    let input = dir.write("one.pdf", &one_page());
    let o = run(&["copy-page", input.to_str().unwrap(), "--dpi", "72"]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(
        out.contains(
            "formats=image/svg+xml,PNG,CF_DIBV5,application/pdf 60x60 dpi=72 background=none;"
        ),
        "{out}"
    );
    assert!(
        out.lines().nth(1).unwrap().starts_with("svg: ops=1 "),
        "{out}"
    );

    // Read back exactly what another application would.
    let _guard = clipboard_win::Clipboard::new_attempts(10).expect("open clipboard");
    let svg_id = clipboard_win::raw::register_format("image/svg+xml").unwrap();
    let mut svg = Vec::new();
    clipboard_win::raw::get_vec(svg_id.get(), &mut svg).expect("svg present");
    assert!(
        svg.starts_with(b"<svg xmlns="),
        "{}",
        String::from_utf8_lossy(&svg[..40])
    );
    assert_eq!(svg.last(), Some(&0u8), "Chromium's trailing NUL");

    let png_id = clipboard_win::raw::register_format("PNG").unwrap();
    let mut png = Vec::new();
    clipboard_win::raw::get_vec(png_id.get(), &mut png).expect("png present");
    let decoder = png::Decoder::new(png.as_slice());
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(buf[3], 0, "unpainted corner is transparent");

    let mut dib = Vec::new();
    clipboard_win::raw::get_vec(clipboard_win::formats::CF_DIBV5, &mut dib).expect("dibv5 present");
    assert_eq!(u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]), 124);

    let pdf_id = clipboard_win::raw::register_format("application/pdf").unwrap();
    let mut pdf = Vec::new();
    clipboard_win::raw::get_vec(pdf_id.get(), &mut pdf).expect("pdf present");
    assert!(pdf.starts_with(b"%PDF-"));
}

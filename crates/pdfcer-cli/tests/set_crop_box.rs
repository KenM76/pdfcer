//! `pdfcer set-crop-box` and `set-page-size --crop` (`G056`): the page
//! grows visibly when its crop box equalled its sheet.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Rect};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn build_pdf(bodies: &[&str]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// One page whose crop box equals its sheet — the Word/Acrobat shape.
fn word_shaped(dir: &Path) -> PathBuf {
    let path = dir.join("in.pdf");
    std::fs::write(
        &path,
        build_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] \
             /CropBox [0 0 300 300] /Resources << >> >>",
        ]),
    )
    .unwrap();
    path
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-set-crop-box-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn crop_of(path: &Path) -> Rect {
    let doc = Document::from_bytes(std::fs::read(path).unwrap()).unwrap();
    page_tree::pages(&doc).unwrap()[0].crop_box
}

#[test]
fn set_page_size_grows_the_visible_page_by_default() {
    let dir = tmp("grow");
    let input = word_shaped(&dir);
    let out = dir.join("out.pdf");
    let o = run(&[
        "set-page-size",
        input.to_str().unwrap(),
        "--width",
        "600",
        "--height",
        "600",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(stdout.trim_end().ends_with("crop_followed=1"), "{stdout}");
    assert_eq!(crop_of(&out), Rect::from_corners(0.0, 0.0, 600.0, 600.0));
}

#[test]
fn set_page_size_crop_keep_is_the_old_behaviour() {
    let dir = tmp("keep");
    let input = word_shaped(&dir);
    let out = dir.join("out.pdf");
    let o = run(&[
        "set-page-size",
        input.to_str().unwrap(),
        "--width",
        "600",
        "--height",
        "600",
        "--crop",
        "keep",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(crop_of(&out), Rect::from_corners(0.0, 0.0, 300.0, 300.0));
}

#[test]
fn set_crop_box_sets_and_resets() {
    let dir = tmp("set-reset");
    let input = word_shaped(&dir);
    let cropped = dir.join("cropped.pdf");
    let o = run(&[
        "set-crop-box",
        input.to_str().unwrap(),
        "--rect",
        "10,20,110,220",
        "-o",
        cropped.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("explicit=1"));
    assert_eq!(
        crop_of(&cropped),
        Rect::from_corners(10.0, 20.0, 110.0, 220.0)
    );

    let reset = dir.join("reset.pdf");
    let o = run(&[
        "set-crop-box",
        cropped.to_str().unwrap(),
        "--reset",
        "-o",
        reset.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("removed=1"));
    assert_eq!(crop_of(&reset), Rect::from_corners(0.0, 0.0, 300.0, 300.0));
}

#[test]
fn set_crop_box_refuses_a_rectangle_off_the_sheet() {
    let dir = tmp("refuse");
    let input = word_shaped(&dir);
    let out = dir.join("out.pdf");
    let o = run(&[
        "set-crop-box",
        input.to_str().unwrap(),
        "--rect",
        "400,400,500,500",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(!o.status.success());
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("leaves nothing visible"),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

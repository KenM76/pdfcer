//! `pdfcer add-image` with a GIF: the first frame is placed and what was left
//! behind is printed, on stdout as a key and on stderr in prose.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn synthetic(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn add_image(gif: &str, tag: &str) -> (Output, PathBuf) {
    let out = std::env::temp_dir().join(format!(
        "pdfcer_add_image_gif_{tag}_{}.pdf",
        std::process::id()
    ));
    let src = synthetic("dimension/plain-base.pdf");
    let image = synthetic(&format!("gif/{gif}"));
    let o = Command::new(BIN)
        .args([
            "add-image",
            src.to_str().unwrap(),
            "--image",
            image.to_str().unwrap(),
            "--page",
            "1",
            "--rect",
            "10,10,110,110",
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("the binary runs");
    (o, out)
}

#[test]
fn an_animated_gif_reports_the_frames_it_left_behind() {
    let (o, out) = add_image("animated-3-frames.gif", "anim");
    let stdout = String::from_utf8_lossy(&o.stdout);
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{stderr}");
    assert!(stdout.contains("gif_frames_ignored=2"), "{stdout}");
    assert!(stderr.contains("2 further frame(s)"), "{stderr}");
    std::fs::remove_file(out).ok();
}

#[test]
fn a_transparent_gif_writes_its_soft_mask() {
    let (o, out) = add_image("two-colour-transparent.gif", "smask");
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(stdout.contains("gif_frames_ignored=0"), "{stdout}");
    assert!(stdout.contains("smask_written=1"), "{stdout}");
    assert!(
        std::fs::read(&out)
            .unwrap()
            .windows(6)
            .any(|w| w == b"/SMask")
    );
    std::fs::remove_file(out).ok();
}

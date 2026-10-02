//! `add-image-stamp`, and `edit-widget --button-icon / --caption-position /
//! --clear-button-icon`, end to end through the binary.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use pdfcer_core::document::Document;
use pdfcer_core::forms;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_imgstamp_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/images")
        .join(name)
        .to_str()
        .unwrap()
        .to_owned()
}

fn blank() -> PathBuf {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
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
    let path = temp_path("src");
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&str]) -> (Output, String, String) {
    let r = Command::new(BIN).args(args).output().unwrap();
    let so = String::from_utf8_lossy(&r.stdout).into_owned();
    let se = String::from_utf8_lossy(&r.stderr).into_owned();
    (r, so, se)
}

#[test]
fn add_image_stamp_writes_a_stamp_and_reports_its_mask() {
    let src = blank();
    let out = temp_path("stamp");
    let (r, so, se) = run(&[
        "add-image-stamp",
        src.to_str().unwrap(),
        "--image",
        &fixture("rgba-half-clear.png"),
        "--page",
        "1",
        "--rect",
        "20,20,68,44",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert!(r.status.success(), "{so}{se}");
    assert!(
        so.starts_with("add-image-stamp ") && so.contains("smask=1"),
        "{so}"
    );
    assert!(so.contains("pixels=64x32"), "{so}");
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        bytes.windows(13).any(|w| w == b"/Subtype /Sta"),
        "a stamp was written"
    );
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&out);
}

/// `/MK` of the one widget of field `Go` in `path`.
fn widget(path: &Path) -> forms::Widget {
    let doc = Document::from_bytes(std::fs::read(path).unwrap()).unwrap();
    forms::parse_acroform(&doc.view())
        .unwrap()
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == "Go")
        .unwrap()
        .widgets
        .remove(0)
}

#[test]
fn edit_widget_sets_positions_and_clears_a_button_icon() {
    let src = blank();
    let button = temp_path("button");
    let (r, so, se) = run(&[
        "add-push-button",
        src.to_str().unwrap(),
        "--name",
        "Go",
        "--page",
        "1",
        "--rect",
        "40,80,160,104",
        "--caption",
        "Go",
        "--no-tooltip",
        "--output",
        button.to_str().unwrap(),
    ]);
    assert!(r.status.success(), "{so}{se}");

    let icon = temp_path("icon");
    let (r, so, se) = run(&[
        "edit-widget",
        button.to_str().unwrap(),
        "--name",
        "Go",
        "--button-icon",
        &fixture("icon32.png"),
        "--caption-position",
        "icon-only",
        "--output",
        icon.to_str().unwrap(),
    ]);
    assert!(r.status.success(), "{so}{se}");
    assert!(so.contains("regenerated=1"), "{so}");
    let w = widget(&icon);
    assert!(w.icon.is_some());
    assert_eq!(
        w.caption_position,
        Some(pdfcer_core::annot_author::CaptionPosition::IconOnly)
    );

    let cleared = temp_path("cleared");
    let (r, so, se) = run(&[
        "edit-widget",
        icon.to_str().unwrap(),
        "--name",
        "Go",
        "--clear-button-icon",
        "--output",
        cleared.to_str().unwrap(),
    ]);
    assert!(r.status.success(), "{so}{se}");
    let w = widget(&cleared);
    assert_eq!((w.icon, w.caption_position), (None, None));
    for p in [&src, &button, &icon, &cleared] {
        let _ = std::fs::remove_file(p);
    }
}

#[test]
fn a_button_icon_on_a_check_box_is_refused() {
    let src = blank();
    let boxed = temp_path("box");
    let (r, so, se) = run(&[
        "add-check-box",
        src.to_str().unwrap(),
        "--name",
        "Go",
        "--page",
        "1",
        "--rect",
        "40,80,60,100",
        "--no-tooltip",
        "--output",
        boxed.to_str().unwrap(),
    ]);
    assert!(r.status.success(), "{so}{se}");
    let out = temp_path("refused");
    let (r, _, se) = run(&[
        "edit-widget",
        boxed.to_str().unwrap(),
        "--name",
        "Go",
        "--caption-position",
        "below",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert!(!r.status.success());
    assert!(se.contains("not a push button"), "{se}");
    assert!(!out.exists(), "nothing written");
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&boxed);
}

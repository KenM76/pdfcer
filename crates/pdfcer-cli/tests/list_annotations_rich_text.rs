//! `pdfcer list-annotations` prints `/RC` as `rich_note=` and `/DS` as
//! `default_style=`, appended to the stable line.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn annot_line(fixture: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/annot")
        .join(fixture);
    let out = Command::new(BIN)
        .args(["list-annotations", path.to_str().unwrap()])
        .output()
        .expect("the binary runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .find(|l| l.starts_with("annot "))
        .expect("an annot line")
        .to_owned()
}

#[test]
fn inline_rc_and_stray_ds_are_printed_last() {
    let line = annot_line("rich-text-square-stray-ds.pdf");
    assert!(
        line.ends_with(
            " rich_note=\"<?xml version=\\\"1.0\\\"?><body><p>THE RICH WORDS</p></body>\" \
default_style=\"font: 12pt Helvetica\""
        ),
        "{line}"
    );
}

#[test]
fn a_stream_rc_is_printed_decoded() {
    let line = annot_line("rich-text-stream.pdf");
    assert!(line.contains(" rich_note=\"<?xml version=\\\"1.0\\\"?><body><p>STREAMED RICH WORDS \u{e9}</p></body>\""), "{line}");
    assert!(line.ends_with(" default_style=none"), "{line}");
}

#[test]
fn absent_keys_print_none() {
    let line = annot_line("no-ap-circle.pdf");
    assert!(
        line.ends_with(" rich_note=none default_style=none"),
        "{line}"
    );
}

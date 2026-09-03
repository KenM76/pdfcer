//! # `list-fields --widgets` — the border and visibility READERS (`Pass 146.0`)
//!
//! **Why these are round-trip tests rather than fixture tests.** `WidgetEdit`
//! could *write* four widget properties and `forms::Widget` could *read* two,
//! so a shell building a properties pane had two controls with no honest way
//! to show their current value. The defect was an **asymmetry between a writer
//! and a reader**, and the only test that can pin an asymmetry is one that
//! drives both halves: write a border with `edit-widget`, read it back with
//! `list-fields --widgets`, assert the same values come out.
//!
//! A hand-authored fixture would test the reader against bytes *I* chose. This
//! tests it against bytes *pdfcer* chose, which is the pair that has to agree.
//!
//! ## The one thing that must never regress
//!
//! **`border=-` means the file states no border. It does NOT mean "solid,
//! 1 pt".** `BorderSpec::default()` is solid/1pt because that reproduces the
//! bytes pdfcer has always *authored*; returning it from a *reader* would make
//! a properties control display a border the document does not contain, and
//! the operator's first press would write that invention in. `pdfcer-gui`
//! refused to ship the control rather than do that, citing pdfcer's own
//! precedent — the text colour swatch shows a *sentence* rather than a
//! nearest-RGB approximation for DeviceCMYK ink, because the approximation
//! would be written back on the next press.
//!
//! Fixture provenance: `fixtures/synthetic/forms/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms")
        .join(name)
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_widget_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The `widget 0` line of the first field in a `list-fields --widgets` listing.
fn first_widget_line(path: &Path) -> String {
    let text = stdout(&run(&["list-fields", path.to_str().unwrap(), "--widgets"]));
    text.lines()
        .find(|l| l.trim_start().starts_with("widget 0"))
        .unwrap_or_else(|| panic!("no widget line in:\n{text}"))
        .to_owned()
}

/// Write one widget property set with `edit-widget`, returning the output path.
fn write_widget(extra: &[&str], tag: &str) -> PathBuf {
    let out = temp_path(tag);
    let src = fixture("demo-form.pdf");
    let mut args = vec!["edit-widget", src.to_str().unwrap(), "--name", "FullName"];
    args.extend_from_slice(extra);
    args.extend(["--output", out.to_str().unwrap()]);
    let r = run(&args);
    assert_eq!(r.status.code(), Some(0), "{}", stdout(&r));
    out
}

// ---------------------------------------------------------------------------

#[test]
fn a_widget_whose_file_states_no_border_reads_a_dash_not_a_default() {
    // ★ THE ONE THAT MATTERS. `demo-form.pdf` carries neither `/BS` nor
    // `/Border` on either widget. If this ever prints `S/1.00`, a properties
    // control is about to write a border nobody authored.
    let line = first_widget_line(&fixture("demo-form.pdf"));
    assert!(line.contains("border=-"), "{line}");
}

#[test]
fn a_border_written_by_edit_widget_reads_back_with_the_same_style_and_width() {
    let out = write_widget(
        &["--border-style", "dashed", "--border-width", "3"],
        "dashed",
    );
    let line = first_widget_line(&out);
    assert!(
        line.contains("border=D/3.00"),
        "the writer and the reader must agree: {line}"
    );
    let _ = std::fs::remove_file(&out);
}

#[test]
fn every_border_style_the_writer_can_set_reads_back_as_itself() {
    // Read against what the WRITER produces for each style, so the two cannot
    // drift. A style that stopped round-tripping would show the operator the
    // wrong line style over a widget they are about to modify.
    for (flag, expect) in [
        ("solid", "border=S/"),
        ("dashed", "border=D/"),
        ("beveled", "border=B/"),
        ("inset", "border=I/"),
        ("underline", "border=U/"),
    ] {
        let out = write_widget(&["--border-style", flag, "--border-width", "2"], flag);
        let line = first_widget_line(&out);
        assert!(line.contains(expect), "for --border-style {flag}: {line}");
        let _ = std::fs::remove_file(&out);
    }
}

#[test]
fn a_zero_width_border_reads_as_a_value_not_as_an_absence() {
    // Table 166 states zero explicitly: "no border". It is a value the file
    // asserts, and a reader that collapsed it to `-` would tell a control the
    // file is silent when it has said something definite.
    let out = write_widget(&["--border-style", "solid", "--border-width", "0"], "zero");
    let line = first_widget_line(&out);
    assert!(line.contains("border=S/0.00"), "{line}");
    let _ = std::fs::remove_file(&out);
}

#[test]
fn every_visibility_the_writer_can_set_reads_back_as_itself() {
    for (flag, expect) in [
        ("screen-and-print", "visibility=visible+print"),
        ("screen-only", "visibility=screen-only"),
        ("print-only", "visibility=print-only"),
        ("hidden", "visibility=hidden"),
    ] {
        let out = write_widget(&["--visibility", flag], flag);
        let line = first_widget_line(&out);
        assert!(line.contains(expect), "for --visibility {flag}: {line}");
        let _ = std::fs::remove_file(&out);
    }
}

#[test]
fn the_raw_flag_word_is_printed_beside_the_mapping() {
    // So a control can say "these flags are not one of the four pdfcer can
    // set" instead of showing nothing or showing a lie. `print-only` is
    // Print|NoView = 0x24.
    let out = write_widget(&["--visibility", "print-only"], "flags");
    let line = first_widget_line(&out);
    assert!(line.contains("visibility=print-only"), "{line}");
    assert!(line.contains("flags=0x24"), "{line}");
    let _ = std::fs::remove_file(&out);
}

#[test]
fn the_widget_lines_are_off_by_default() {
    // A per-widget block under every field would double the length of a
    // listing that scripts already parse. Opt-in, and the field rows are
    // byte-for-byte what they were.
    let text = stdout(&run(&[
        "list-fields",
        fixture("demo-form.pdf").to_str().unwrap(),
    ]));
    assert!(text.contains("field name=\"FullName\""));
    assert!(
        !text.contains("widget 0"),
        "no widget lines without --widgets:\n{text}"
    );
}

#[test]
fn a_multi_widget_field_gets_one_line_per_widget() {
    // The reason these are per-widget and not a field column: a field with two
    // widgets can carry two different borders, and a single field-level
    // `border=` would be a lie for one of them.
    let text = stdout(&run(&[
        "list-fields",
        fixture("multi-widget-form.pdf").to_str().unwrap(),
        "--widgets",
    ]));
    let lines = text
        .lines()
        .filter(|l| l.trim_start().starts_with("widget "))
        .count();
    assert!(
        lines >= 2,
        "the multi-widget fixture must produce more than one widget line:\n{text}"
    );
}

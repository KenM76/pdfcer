//! `pdfcer text-locate`: searched text to the `--object`/`--leaf` + `--run`
//! operands the `text-run-*` commands take.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn text_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name)
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn locate(name: &str, find: &str) -> (Output, String) {
    let src = text_fixture(name);
    let out = run(&["text-locate", src.to_str().unwrap(), "--find", find]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    (out, stdout)
}

/// The located operands are the ones `text-run-delete` acts on: deleting
/// what `text-locate` names removes exactly the searched label.
#[test]
fn located_run_is_the_one_text_run_delete_removes() {
    let (out, stdout) = locate("runs-inherited.pdf", "DELTA");
    assert!(out.status.success(), "{stdout}");
    assert!(
        stdout.contains("match ordinal=0 start=15 len=5 targets=object=0/run=3 unresolved=0"),
        "{stdout}"
    );
    assert!(stdout.contains("matches=1"), "{stdout}");

    let src = text_fixture("runs-inherited.pdf");
    let dst = std::env::temp_dir().join(format!("pdfcer_tlocate_{}.pdf", std::process::id()));
    let del = run(&[
        "text-run-delete",
        src.to_str().unwrap(),
        "--object",
        "0",
        "--run",
        "3",
        "-o",
        dst.to_str().unwrap(),
    ]);
    assert!(
        del.status.success(),
        "{}",
        String::from_utf8_lossy(&del.stderr)
    );
    let after = run(&["text-locate", dst.to_str().unwrap(), "--find", "DELTA"]);
    let after = String::from_utf8_lossy(&after.stdout).into_owned();
    let _ = std::fs::remove_file(&dst);
    assert!(after.contains("matches=0"), "{after}");
}

/// A word drawn by three text objects is found once and names all three.
#[test]
fn a_word_split_across_objects_lists_every_piece() {
    let (_, stdout) = locate("cross-object-word.pdf", "Driver-Side");
    assert!(
        stdout.contains("start=0 len=11 targets=object=0/run=0,object=1/run=0,object=2/run=0"),
        "{stdout}"
    );
    assert!(stdout.contains("matches=3"), "{stdout}");
}

/// Text inside a form XObject is addressed as a leaf, not a page object.
#[test]
fn text_inside_a_form_is_named_as_a_leaf() {
    let (_, stdout) = locate("fallback-font-form.pdf", "Hi");
    assert!(
        stdout.contains("targets=leaf=1/run=0 unresolved=0"),
        "{stdout}"
    );
}

#[test]
fn a_miss_exits_zero_and_an_empty_find_is_refused() {
    let (out, stdout) = locate("runs-inherited.pdf", "ZZZ");
    assert!(out.status.success());
    assert!(stdout.contains("matches=0"), "{stdout}");
    let (out, _) = locate("runs-inherited.pdf", "");
    assert_eq!(out.status.code(), Some(1));
}

/// `format-text` output, then `text-locate` over it: the `target` lines.
fn locate_after_format(name: &str, find: &str, format_args: &[&str], tag: &str) -> String {
    let src = text_fixture(name);
    let dst = std::env::temp_dir().join(format!("pdfcer_tlocate_{tag}_{}.pdf", std::process::id()));
    let mut args = vec![
        "format-text",
        src.to_str().unwrap(),
        "--page",
        "1",
        "--find",
        find,
    ];
    args.extend_from_slice(format_args);
    args.extend_from_slice(&["-o", dst.to_str().unwrap()]);
    let fmt = run(&args);
    assert!(
        fmt.status.success(),
        "{}",
        String::from_utf8_lossy(&fmt.stderr)
    );
    let out = run(&["text-locate", dst.to_str().unwrap(), "--find", find]);
    let _ = std::fs::remove_file(&dst);
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A faux bold (mode 2 plus a stroke) and a faux italic (a shear) are told
/// apart from the face's own weight and slant; untouched text is `none`.
#[test]
fn target_lines_report_synthesized_style() {
    let (_, plain) = locate("runs-single.pdf", "ONLY");
    assert!(
        plain.contains(
            "target match=0 ref=object=1/run=0 font=\"Helvetica\" render_mode=0 line_width=1 synthetic=none"
        ),
        "{plain}"
    );
    let bold = locate_after_format("runs-single.pdf", "ONLY", &["--bold-synthetic"], "b");
    assert!(
        bold.contains("render_mode=2 line_width=0.22 synthetic=bold"),
        "{bold}"
    );
    let italic = locate_after_format("runs-single.pdf", "ONLY", &["--italic-synthetic"], "i");
    assert!(
        italic.contains("render_mode=0 line_width=1 synthetic=italic"),
        "{italic}"
    );
}

/// The font named is the one the run was SHOWN in, not the text object's
/// first: a `Tf` switch mid-object gives the later run its own face.
#[test]
fn target_font_follows_a_mid_object_font_switch() {
    let out = locate_after_format(
        "runs-two-explicit.pdf",
        "BETA",
        &["--set-font", "Courier"],
        "f",
    );
    assert!(out.contains("ref=object=0/run=1 font=\"Courier\""), "{out}");
}

/// Inside a form XObject the font is looked up in the form's own
/// `/Resources` — this page's resources carry no `/Font` at all.
#[test]
fn target_font_resolves_through_the_form_resources() {
    let (_, out) = locate("fallback-font-form.pdf", "Hi");
    assert!(
        out.contains("target match=0 ref=leaf=1/run=0 font=\"Helvetica\""),
        "{out}"
    );
}

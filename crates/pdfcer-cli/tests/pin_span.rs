//! # `--pin-span` and `extract-text --spans` (`Pass 145.0`)
//!
//! Black-box over the **real binary**, and the first test walks the whole loop
//! a caller actually has to walk: get a span out of `extract-text --json
//! --spans`, hand it back to `format-text --pin-span`, and check the edit
//! landed. That round trip is the feature; testing either half alone would
//! pass with the two ends disagreeing about what a span is.
//!
//! ## Why the loop matters more than either half
//!
//! Before this Pass there was **no way to obtain a show-operator span from
//! outside the library** — `extract-text --json` emitted `start`/`len` into the
//! run's *text*, never the operator's byte span. So a consuming project
//! reconstructed a `find` string from extracted text instead, and got it wrong
//! three times running; the operator-facing symptom was *"eleven pieces of text
//! went bold and the twelfth refused"*. The two halves shipped here only remove
//! that if they fit together.
//!
//! Fixture provenance: `fixtures/synthetic/textedit/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
/// `exit::EDIT_REFUSED` in the CLI's stable exit-code contract.
const EDIT_REFUSED: i32 = 9;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/textedit")
        .join(name)
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pdfcer_pin_{tag}_{}_{n}.pdf", std::process::id()))
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

/// The first `op_start`/`op_len` in `extract-text --json --spans` output.
///
/// Parsed by string scan rather than with a JSON crate on purpose: this test
/// is about the **shape a script sees**, and a script on this machine has
/// `grep`, not `serde`.
fn first_span(json: &str) -> (usize, usize) {
    let i = json.find("\"op_start\": ").expect("--spans emits op_start");
    let rest = &json[i + "\"op_start\": ".len()..];
    let start: usize = rest[..rest.find(',').unwrap()].trim().parse().unwrap();
    let j = json.find("\"op_len\": ").expect("--spans emits op_len");
    let rest = &json[j + "\"op_len\": ".len()..];
    let len: usize = rest[..rest.find(',').unwrap()].trim().parse().unwrap();
    (start, len)
}

// ---------------------------------------------------------------------------

#[test]
fn a_span_from_extract_text_drives_format_text() {
    let path = fixture("format_family.pdf");
    let src = path.to_str().unwrap();

    let extracted = run(&["extract-text", src, "--json", "--spans"]);
    assert_eq!(extracted.status.code(), Some(0));
    let (start, len) = first_span(&stdout(&extracted));

    let out = temp_path("loop");
    let formatted = run(&[
        "format-text",
        src,
        "--pin-span",
        &format!("{start}:{len}"),
        "--set-size",
        "24",
        "--output",
        out.to_str().unwrap(),
    ]);
    let text = stdout(&formatted);
    assert_eq!(formatted.status.code(), Some(0), "{text}");
    assert!(text.contains("set_size=12->24"), "{text}");
    assert!(
        text.contains("whole operator:"),
        "the extent pdfcer chose is disclosed: {text}"
    );
    assert!(out.exists());
    let _ = std::fs::remove_file(&out);
}

#[test]
fn the_span_fields_appear_only_with_the_flag() {
    let src = fixture("format_family.pdf");
    let src = src.to_str().unwrap();

    let with = stdout(&run(&["extract-text", src, "--json", "--spans"]));
    assert!(with.contains("\"op_start\""));
    assert!(with.contains("\"op_len\""));
    assert!(
        with.contains("\"stream\": \"page\""),
        "a span is meaningless without the buffer it indexes: {}",
        &with[..with.len().min(400)]
    );

    let without = stdout(&run(&["extract-text", src, "--json"]));
    assert!(
        !without.contains("op_start"),
        "ABSENT, not zero — a consumer must be able to tell 'not captured' \
         from 'offset 0'"
    );
}

#[test]
fn an_empty_find_with_no_pin_is_refused_by_both_editing_verbs() {
    let src = fixture("format_family.pdf");
    let src = src.to_str().unwrap();

    for (verb, extra) in [
        ("format-text", vec!["--set-size", "24"]),
        ("edit-text", vec!["--replace", "x"]),
    ] {
        let out = temp_path(verb);
        let mut args = vec![verb, src];
        args.extend(extra);
        args.extend(["--output", out.to_str().unwrap()]);
        let r = run(&args);
        assert_eq!(
            r.status.code(),
            Some(EDIT_REFUSED),
            "{verb} must refuse an empty --find with no pin"
        );
        let err = String::from_utf8_lossy(&r.stderr).into_owned();
        assert!(
            err.contains("--pin-span START:LEN"),
            "and the refusal names the flag that would fix it, which core \
             cannot know about: {err}"
        );
        assert!(!out.exists(), "nothing is written on a refusal");
    }
}

#[test]
fn a_malformed_span_fails_before_the_file_is_opened() {
    // "before the file is opened" is asserted by pointing at a path that does
    // not exist: if the span were parsed second, this would report an I/O
    // error instead of the span error.
    let out = temp_path("bad");
    let r = run(&[
        "format-text",
        "no-such-file-anywhere.pdf",
        "--pin-span",
        "37",
        "--set-size",
        "24",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(EDIT_REFUSED));
    let err = String::from_utf8_lossy(&r.stderr).into_owned();
    assert!(err.contains("is not START:LEN"), "{err}");
    assert!(
        err.contains("extract-text --json --spans"),
        "and it says where to get a valid one: {err}"
    );
}

#[test]
fn a_zero_length_span_is_refused_by_name() {
    let out = temp_path("zero");
    let r = run(&[
        "format-text",
        fixture("format_family.pdf").to_str().unwrap(),
        "--pin-span",
        "37:0",
        "--set-size",
        "24",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(EDIT_REFUSED));
    let err = String::from_utf8_lossy(&r.stderr).into_owned();
    assert!(err.contains("names no operator"), "{err}");
}

#[test]
fn edit_text_replaces_the_whole_pinned_operator() {
    let path = fixture("format_family.pdf");
    let src = path.to_str().unwrap();
    let (start, len) = first_span(&stdout(&run(&["extract-text", src, "--json", "--spans"])));

    let out = temp_path("replace");
    let r = run(&[
        "edit-text",
        src,
        "--pin-span",
        &format!("{start}:{len}"),
        "--replace",
        "goodbye",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(0), "{}", stdout(&r));

    // Verified by re-extracting the OUTPUT, not by reading the report: the
    // claim is about the saved bytes.
    let after = stdout(&run(&["extract-text", out.to_str().unwrap()]));
    assert!(
        after.contains("goodbye") && !after.contains("hello"),
        "the whole operator was replaced: {after:?}"
    );
    let _ = std::fs::remove_file(&out);
}

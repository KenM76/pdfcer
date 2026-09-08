//! # `edit-text --span-from-pin`, through the BINARY
//!
//! ## Why this cannot be a core test
//!
//! `--span-from-pin` is a **flag**, and a flag is the one thing a unit test
//! cannot see. `pdfcer-core`'s suite builds `EditRequest::spanning_from`
//! directly, so a build that parsed the flag and then quietly dropped it on
//! the floor would pass every one of those — the flag would be declared,
//! documented, present in `--help`, and inert.
//!
//! ★ That has happened in this project before, which is why the rule exists at
//! all. Only an end-to-end run through `main` can refute it.
//!
//! ## What is pinned
//!
//! 1. **The flag changes the outcome.** The same command with and without it,
//!    on the same file and the same pin, produces a refusal and an edit
//!    respectively. That is the strongest available evidence the flag is
//!    *wired*, because it is a difference the argument parser alone cannot
//!    produce.
//! 2. **It selects WHICH occurrence.** Two pins, one `find`, two different
//!    lines edited.
//! 3. **It is refused without a pin**, by clap's `requires`, because it has
//!    nothing to start from — and the refusal names the missing flag rather
//!    than failing later with something about text.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED`, spelled out so a change to the number is a visible
/// test failure rather than a silent contract break.
const EDIT_REFUSED: i32 = 9;

/// Byte spans the fixture generator prints. Line 1 and line 3 both span two
/// operators; line 2 holds the same text in one.
const SPANNING_LINE1: &str = "23:7";
const SPANNING_LINE3: &str = "100:7";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/text/span-from-pin.pdf")
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pdfcer_sfp_{tag}_{}_{n}.pdf", std::process::id()))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().expect("binary runs")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The text of one line of the newest revision, named by its `Td`.
fn line_at(path: &Path, td: &str) -> String {
    let bytes = std::fs::read(path).expect("output exists");
    let s = String::from_utf8_lossy(&bytes).into_owned();
    let Some(start) = s.rfind(td) else {
        return String::new();
    };
    let rest = &s[start..];
    let end = rest.find("ET").unwrap_or(rest.len());
    rest[..end].to_owned()
}

fn edit(pin: &str, with_flag: bool, replace: &str, out_path: &Path) -> Output {
    let mut args = vec![
        "edit-text".to_owned(),
        fixture().to_str().unwrap().to_owned(),
        "--page".to_owned(),
        "1".to_owned(),
        "--pin-span".to_owned(),
        pin.to_owned(),
        "--find".to_owned(),
        "ABCD".to_owned(),
        "--replace".to_owned(),
        replace.to_owned(),
        "-o".to_owned(),
        out_path.to_str().unwrap().to_owned(),
    ];
    if with_flag {
        args.push("--span-from-pin".to_owned());
    }
    run(&args.iter().map(String::as_str).collect::<Vec<_>>())
}

/// ★★★ The flag is what makes the difference. Same command otherwise.
#[test]
fn the_flag_turns_a_refusal_into_an_edit() {
    let without = temp_path("without");
    let out = edit(SPANNING_LINE1, false, "WXYZ", &without);
    assert_eq!(
        out.status.code(),
        Some(EDIT_REFUSED),
        "without the flag a pinned spanning request must still be refused -- \
         that is the behaviour the consuming shell asked to keep. stderr: {}",
        stderr(&out)
    );
    assert!(
        !without.exists(),
        "a refused edit must not leave an output file"
    );

    let with = temp_path("with");
    let out = edit(SPANNING_LINE1, true, "WXYZ", &with);
    assert!(
        out.status.success(),
        "with the flag the same request must succeed: {}",
        stderr(&out)
    );
    assert!(
        line_at(&with, "20 140 Td").contains("(WXYZ)"),
        "and it must edit the pinned line; it reads {:?}",
        line_at(&with, "20 140 Td")
    );
}

/// ★★★ It selects WHICH occurrence — the disambiguation the BOM case needs.
#[test]
fn two_pins_one_find_two_different_lines() {
    let a = temp_path("line1");
    assert!(edit(SPANNING_LINE1, true, "WXYZ", &a).status.success());
    let b = temp_path("line3");
    assert!(edit(SPANNING_LINE3, true, "MNOP", &b).status.success());

    assert!(
        line_at(&a, "20 140 Td").contains("(WXYZ)"),
        "pinning line 1 must edit line 1"
    );
    assert!(
        !line_at(&a, "20 60 Td").contains("(WXYZ)"),
        "and must NOT edit line 3"
    );

    assert!(
        line_at(&b, "20 60 Td").contains("(MNOP)"),
        "pinning line 3 must edit line 3; it reads {:?}",
        line_at(&b, "20 60 Td")
    );
    assert!(
        !line_at(&b, "20 140 Td").contains("(MNOP)"),
        "and must NOT edit line 1 -- the occurrence a page scan reaches first"
    );
}

/// The flag alone is refused, and the refusal names the missing pin.
///
/// It has nothing to start from without one. Caught by clap's `requires` so
/// the message is about the argument rather than about the document — a
/// caller who forgot the pin should not be told its text was not found.
#[test]
fn the_flag_without_a_pin_is_refused_by_name() {
    let out = run(&[
        "edit-text",
        fixture().to_str().unwrap(),
        "--page",
        "1",
        "--span-from-pin",
        "--find",
        "ABCD",
        "--replace",
        "WXYZ",
        "-o",
        temp_path("nopin").to_str().unwrap(),
    ]);
    assert!(!out.status.success(), "the flag needs a pin");
    let err = stderr(&out);
    assert!(
        err.contains("--pin-span"),
        "the refusal must name the flag that is missing, not the text: {err}"
    );
}

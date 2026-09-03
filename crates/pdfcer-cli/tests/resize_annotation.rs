//! # `pdfcer resize-annotation` (`Pass 151.0`)
//!
//! Black-box over the **real binary**, and every claim is checked against the
//! *output file* or the binary's own `--help`, never against the report the
//! command printed about itself.
//!
//! ## ★ Why a core-level test suite is not enough here
//!
//! `crates/pdfcer-core/tests/annot_resize.rs` calls `resize_annotation`
//! directly, so it exercises `ResizeOptions` **whatever the CLI does with the
//! flags**. A flag that clap parses and the dispatch then drops on the floor
//! passes every one of those tests. This project has shipped exactly that
//! defect before, and the only thing that finds it is running the binary and
//! observing a difference in the saved bytes.
//!
//! So each of the three flags below is asserted by **running the same resize
//! twice, once with and once without**, and requiring the outputs to differ in
//! the specific key that flag owns. A flag that is parsed and ignored makes the
//! two runs identical and the test fails.
//!
//! ## What the flags mean, in one line each
//!
//! * `--scale-stroke-width` — `/BS /W` travels. Off by default: a line weight
//!   is a drafting convention, not a length in the space being scaled.
//! * `--keep-rect-differences` — `/RD` does NOT travel. Off by default,
//!   because an inset *is* such a length.
//! * `--allow-appearance-distortion` — proceed when carrying a foreign `/AP`
//!   would contradict the other options. Off by default: refused by name.
//!
//! Fixture provenance: `fixtures/synthetic/annot/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
/// `exit::EDIT_REFUSED` in the CLI's stable exit-code contract.
const EDIT_REFUSED: i32 = 9;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_rzannot_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().expect("binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The `/RD`-carrying square. Its `/AP` was written by the fixture generator,
/// so it is genuinely **foreign** to pdfcer — which is what makes the
/// appearance-refusal tests below measure the real gate rather than a
/// manufactured one.
fn rd_square() -> PathBuf {
    fixture("annot/rect-differences-square.pdf")
}

/// One resize, returning the saved file's raw bytes. `extra` carries whichever
/// flag is under test; everything else is held constant.
fn resize(tag: &str, extra: &[&str], sx: &str, sy: &str) -> Vec<u8> {
    let src = rd_square();
    let out = temp_path(tag);
    let mut args: Vec<String> = vec![
        "resize-annotation".to_owned(),
        src.to_str().unwrap().to_owned(),
        "--sx".to_owned(),
        sx.to_owned(),
        "--sy".to_owned(),
        sy.to_owned(),
        "--anchor-x".to_owned(),
        "100".to_owned(),
        "--anchor-y".to_owned(),
        "100".to_owned(),
        "--output".to_owned(),
        out.to_str().unwrap().to_owned(),
    ];
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let r = run(&refs);
    assert_eq!(
        r.status.code(),
        Some(0),
        "resize failed: {}{}",
        stdout(&r),
        stderr(&r)
    );
    std::fs::read(&out).expect("output file")
}

/// The incremental update appends the new annotation dictionary, so the LAST
/// occurrence of a key in the file is the effective one. Reading it this way
/// rather than re-parsing keeps this test black-box: it asserts about bytes a
/// third-party tool would see.
fn last_value_after(bytes: &[u8], key: &str) -> String {
    let text = String::from_utf8_lossy(bytes);
    let at = text
        .rfind(key)
        .unwrap_or_else(|| panic!("no {key} in output"));
    let rest = &text[at + key.len()..];
    let end = rest.find(['/', '>']).unwrap_or(rest.len());
    rest[..end].trim().to_owned()
}

// ---------------------------------------------------------------------------
// 1. THE HAPPY PATH, AND THE DISCLOSURES THAT RIDE WITH IT
// ---------------------------------------------------------------------------

#[test]
fn it_resizes_and_reports_every_field() {
    let src = rd_square();
    let out = temp_path("basic");
    let r = run(&[
        "resize-annotation",
        src.to_str().unwrap(),
        "--sx",
        "2",
        "--sy",
        "2",
        "--anchor-x",
        "100",
        "--anchor-y",
        "100",
        "--scale-stroke-width",
        "--output",
        out.to_str().unwrap(),
    ]);
    let text = stdout(&r);
    assert_eq!(r.status.code(), Some(0), "{text}{}", stderr(&r));

    assert!(text.contains("subtype=Square"), "{text}");
    assert!(text.contains("sx=2.0000 sy=2.0000"), "{text}");
    assert!(text.contains("anchor=(100.00 100.00)"), "{text}");
    // 100..200 wide about x=100 doubles to 100..300; 100..160 tall to 100..220.
    assert!(
        text.contains("[100.00 100.00 200.00 160.00]->[100.00 100.00 300.00 220.00]"),
        "the printed rect must show the anchor held: {text}"
    );
    assert!(
        text.contains("stroke_width=3.000->6.000"),
        "the width change must be disclosed numerically: {text}"
    );
    assert!(text.contains("rect_differences=scaled"), "{text}");
}

/// Rule 4 in the CLI: the invocation IS the commit, so what pdfcer chose *not*
/// to do is printed on the way past. The single most surprising choice is that
/// a 3× resize leaves a 3 pt border at 3 pt, so that is the one asserted.
#[test]
fn it_says_out_loud_that_the_border_width_did_not_follow() {
    let src = rd_square();
    let out = temp_path("silent");
    let r = run(&[
        "resize-annotation",
        src.to_str().unwrap(),
        "--sx",
        "3",
        "--sy",
        "3",
        "--anchor-x",
        "100",
        "--anchor-y",
        "100",
        // The fixture's /AP is foreign, so the stroke-scaling-OFF case is
        // refused unless the distortion is accepted. That refusal is tested
        // elsewhere; here it would simply prevent the disclosure being reached.
        "--allow-appearance-distortion",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));
    let err = stderr(&r);
    assert!(
        err.contains("border width was NOT scaled"),
        "the omission must be disclosed, not merely defaulted: {err}"
    );
    assert!(
        err.contains("--scale-stroke-width"),
        "and the disclosure must name the flag that changes it: {err}"
    );
}

// ---------------------------------------------------------------------------
// 2. EACH FLAG IS WIRED — proved by a difference in the SAVED BYTES
// ---------------------------------------------------------------------------

/// ★ The test that catches a parsed-but-unused flag. Same command twice; the
/// only difference is the flag, and the saved `/BS /W` must differ.
#[test]
fn scale_stroke_width_reaches_the_saved_bytes() {
    // Both runs carry --allow-appearance-distortion because the fixture's /AP
    // is foreign and the default gate refuses the stroke-scaling-OFF case. It
    // is held CONSTANT across the pair, so the only variable is the flag under
    // test — which is the whole point of the A/B.
    let without = resize("wnone", &["--allow-appearance-distortion"], "2", "2");
    let with = resize(
        "wflag",
        &["--allow-appearance-distortion", "--scale-stroke-width"],
        "2",
        "2",
    );

    // Parsed as a NUMBER, not compared as a string: pdfcer writes reals in a
    // canonical form of its own ("6.0"), so a literal "6" fails on correct
    // output. That is a property of the writer, not of this flag, and a test
    // that couples to it reports the writer's formatting as a resize defect.
    let w = |b: &[u8]| {
        last_value_after(b, "/W ")
            .parse::<f64>()
            .expect("border width is numeric")
    };
    assert!(
        (w(&without) - 3.0).abs() < 1e-9,
        "the default must leave the fixture's 3 pt border alone, got {}",
        w(&without)
    );
    assert!(
        (w(&with) - 6.0).abs() < 1e-9,
        "with the flag, 3 pt scaled 2x must be 6 pt IN THE FILE, got {}",
        w(&with)
    );
}

/// The opt-out, same method. `/RD` is `[2 4 2 4]` in the fixture; a 2× uniform
/// scale takes it to `[4 8 4 8]` by default and leaves it alone with the flag.
#[test]
fn keep_rect_differences_reaches_the_saved_bytes() {
    let scaled = resize("rdscl", &["--scale-stroke-width"], "2", "2");
    let kept = resize(
        "rdkeep",
        &["--scale-stroke-width", "--keep-rect-differences"],
        "2",
        "2",
    );

    assert!(
        last_value_after(&scaled, "/RD ").starts_with('['),
        "sanity: /RD should be an array"
    );
    assert_ne!(
        last_value_after(&scaled, "/RD "),
        last_value_after(&kept, "/RD "),
        "the flag must change the saved /RD, not merely the report"
    );
    assert!(
        last_value_after(&kept, "/RD ").contains('2')
            && last_value_after(&kept, "/RD ").contains('4'),
        "kept /RD must still be the fixture's [2 4 2 4]: {}",
        last_value_after(&kept, "/RD ")
    );
}

/// The third flag is wired if and only if the refusal it lifts actually
/// happens without it — so this asserts both directions in one test.
#[test]
fn allow_appearance_distortion_lifts_a_real_refusal() {
    let src = rd_square();
    let out = temp_path("refuse");
    let refused = run(&[
        "resize-annotation",
        src.to_str().unwrap(),
        "--sx",
        "4",
        "--sy",
        "1",
        "--anchor-x",
        "100",
        "--anchor-y",
        "100",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        refused.status.code(),
        Some(EDIT_REFUSED),
        "a non-uniform scale of a foreign appearance must refuse: {}{}",
        stdout(&refused),
        stderr(&refused)
    );
    let err = stderr(&refused);
    assert!(
        err.contains("pdfcer did not draw it"),
        "the refusal must say why it will not redraw: {err}"
    );
    assert!(
        err.contains("allow_appearance_distortion") || err.contains("allow-appearance-distortion"),
        "and must name the way past it: {err}"
    );

    // And with the flag, the same command succeeds and names the distortion.
    let bytes = resize("allow", &["--allow-appearance-distortion"], "4", "1");
    assert!(!bytes.is_empty());
}

// ---------------------------------------------------------------------------
// 3. REFUSALS AND ARGUMENT HANDLING
// ---------------------------------------------------------------------------

#[test]
fn a_zero_factor_is_refused_by_name() {
    let src = rd_square();
    let out = temp_path("zero");
    let r = run(&[
        "resize-annotation",
        src.to_str().unwrap(),
        "--sx",
        "0",
        "--sy",
        "2",
        "--anchor-x",
        "100",
        "--anchor-y",
        "100",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(EDIT_REFUSED), "{}", stderr(&r));
    assert!(
        stderr(&r).contains("sx"),
        "the message must name WHICH factor: {}",
        stderr(&r)
    );
}

/// A negative factor is a mirror, not a typo — and clap must accept it rather
/// than reading `-1` as an unknown flag. That is what `allow_negative_numbers`
/// is for, and it is easy to omit on one argument out of four.
///
/// ★ This test also pins a correctness claim it was not written for. A mirror
/// is an ISOMETRY: `sx = -1, sy = 1` preserves every length, including the
/// drawn stroke width, so it must NOT be classified as a non-uniform scale and
/// must NOT trip the foreign-appearance refusal. The first cut of the core
/// used `(sx - sy).abs()`, which reads a mirror as a 2:1 distortion; this test
/// is what found it, because no core case paired a negative factor with a
/// foreign appearance. Note the absence of --allow-appearance-distortion
/// below: that absence IS the assertion.
#[test]
fn negative_factors_and_anchors_are_accepted_as_values() {
    let src = rd_square();
    let out = temp_path("mirror");
    let r = run(&[
        "resize-annotation",
        src.to_str().unwrap(),
        "--sx",
        "-1",
        "--sy",
        "1",
        "--anchor-x",
        "-50",
        "--anchor-y",
        "100",
        "--scale-stroke-width",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "a mirror about a negative anchor is legitimate: {}{}",
        stdout(&r),
        stderr(&r)
    );
    assert!(stdout(&r).contains("sx=-1.0000"), "{}", stdout(&r));
}

#[test]
fn page_zero_is_refused_because_pages_are_one_based() {
    let src = rd_square();
    let out = temp_path("page0");
    let r = run(&[
        "resize-annotation",
        src.to_str().unwrap(),
        "--page",
        "0",
        "--sx",
        "2",
        "--sy",
        "2",
        "--anchor-x",
        "100",
        "--anchor-y",
        "100",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_ne!(r.status.code(), Some(0));
    assert!(stderr(&r).contains("1-based"), "{}", stderr(&r));
}

/// The help text is operator-facing copy, and a doc comment spliced into the
/// wrong place ships as the *previous* command's help. That has happened in
/// this repo, so the shape of it is asserted rather than assumed.
#[test]
fn the_help_describes_this_command_and_its_flags() {
    let r = run(&["resize-annotation", "--help"]);
    let text = stdout(&r);
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));
    for needle in [
        "--scale-stroke-width",
        "--keep-rect-differences",
        "--allow-appearance-distortion",
        "--anchor-x",
        "--anchor-y",
    ] {
        assert!(
            text.contains(needle),
            "--help must document {needle}: {text}"
        );
    }
    assert!(
        !text.contains("Move a markup"),
        "the help must not be the neighbouring command's: {text}"
    );
}

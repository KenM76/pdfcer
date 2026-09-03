//! # `pdfcer font-preflight` integration tests (`Pass 142.1`)
//!
//! Black-box over the **real binary**. The subcommand answers *"which strings
//! would `format-text --set-font` accept for this run, on this page"*, which
//! is a different question from `list-fonts` and gives different answers for a
//! reason a script has to be able to see.
//!
//! The three properties pinned here are the three a caller would otherwise
//! learn by pressing a button and reading a refusal:
//!
//! 1. **The verdict is per resource and matches the real thing.** Every
//!    `REFUSE` line here is reproduced by running `format-text --set-font` for
//!    the same resource in the same test, so the pre-flight cannot quietly
//!    disagree with the command it predicts.
//! 2. **A name claim is not an acceptance.** `/F3` says `Times-Bold` and
//!    refuses `"hello world"`; the summary line must therefore say synthesis
//!    is the route, not point at `/F3`.
//! 3. **A `/BaseFont` is not always a selector.** On `format_twins.pdf` two
//!    resources share `/Times-Bold`; the selector column falls back to the
//!    resource key and says so.
//!
//! Read-only: the subcommand takes no `--output` and writes no file, which the
//! last test asserts by giving it a read-only-shaped invocation and checking
//! the fixture directory is untouched.
//!
//! Fixture provenance: `fixtures/synthetic/textedit/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
/// `exit::EDIT_REFUSED` in the CLI's stable exit-code contract.
const EDIT_REFUSED: i32 = 9;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/textedit")
        .join(name)
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("font-preflight")
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The one line of the listing describing resource `key`.
fn row<'a>(text: &'a str, key: &str) -> &'a str {
    let needle = format!("  /{key}  ");
    text.lines()
        .find(|l| l.starts_with(&needle))
        .unwrap_or_else(|| panic!("no row for /{key} in:\n{text}"))
}

// ---------------------------------------------------------------------------

#[test]
fn it_reports_a_verdict_for_every_font_resource_on_the_page() {
    let out = run(&[
        fixture("format_family.pdf").to_str().unwrap(),
        "--find",
        "hello world",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    let text = stdout(&out);

    assert!(text.contains(r#"run: /F1 "Times-Roman" text="hello world""#));
    assert!(row(&text, "F1").contains("ACCEPT"));
    assert!(row(&text, "F2").contains("ACCEPT"));
    assert!(row(&text, "F3").contains("REFUSE"));
    assert!(
        text.contains("3 font resource(s) on page 1; 2 would be accepted for this run"),
        "{text}"
    );
}

#[test]
fn a_refusal_is_printed_verbatim_and_the_command_it_predicts_agrees() {
    let path = fixture("format_family.pdf");
    let out = run(&[path.to_str().unwrap(), "--find", "hello world"]);
    let text = stdout(&out);

    // The pre-flight's own words…
    assert!(
        text.contains("R-INV-7") && text.contains("U+006F"),
        "the refusal is printed, not merely counted:\n{text}"
    );

    // …and the command it is predicting, run for real. If these two ever
    // diverge the pre-flight has become a parallel description of `set_font`,
    // which is the exact defect `Pass 144.0` was filed for.
    let real = Command::new(BIN)
        .args([
            "format-text",
            path.to_str().unwrap(),
            "--find",
            "hello world",
            "--set-font",
            "F3",
            "--output",
            std::env::temp_dir()
                .join("pdfcer_preflight_never_written.pdf")
                .to_str()
                .unwrap(),
        ])
        .output()
        .expect("the binary runs");
    assert_eq!(real.status.code(), Some(EDIT_REFUSED));
    let refusal = String::from_utf8_lossy(&real.stderr).into_owned();
    assert!(
        refusal.contains("R-INV-7") && refusal.contains("U+006F"),
        "the real command refuses for the reason the pre-flight predicted:\n{refusal}"
    );
}

#[test]
fn a_bold_claim_that_cannot_cover_the_run_routes_to_synthesis_not_to_that_face() {
    let out = run(&[
        fixture("format_family.pdf").to_str().unwrap(),
        "--find",
        "hello world",
    ]);
    let text = stdout(&out);

    // /F3's NAME says Bold, and the listing says so — the claim is not hidden.
    assert!(row(&text, "F3").contains("claims=bold"));
    // …but the verdict that decides a button says otherwise.
    assert!(
        text.contains("no real bold face of this run's family is a resource ON THIS PAGE"),
        "{text}"
    );
    // ★ The old assertion here required "--bold-synthetic is the route".
    //
    // That sentence was FALSE and had been since `Pass 162.0`: on this same
    // page `--set-font Helvetica-Bold` succeeds and embeds nothing, so
    // synthesis was never "the" route. The line understated what pdfcer can
    // do, which sends an operator to the worse remedy and fails nothing.
    //
    // The message now offers both and says which one this survey does not
    // look for, so the test pins BOTH halves — a correction that named only
    // the standard-14 route without saying it is unsurveyed would be true and
    // still misleading.
    assert!(text.contains("--bold-synthetic is one route"), "{text}");
    assert!(
        text.contains("Helvetica-Bold") && text.contains("NOT surveyed by this check"),
        "the other route, and the scope of the check, must both be stated: {text}"
    );
}

#[test]
fn the_same_face_is_accepted_for_a_run_that_omits_the_uncovered_character() {
    let out = run(&[
        fixture("format_family.pdf").to_str().unwrap(),
        "--find",
        "hell",
    ]);
    let text = stdout(&out);
    assert!(
        row(&text, "F3").contains("ACCEPT"),
        "acceptance is per RUN, not per page:\n{text}"
    );
    assert!(
        text.contains(
            r#"bold: a REAL bold face of this run's family is accepted — --set-font "Times-Bold""#
        ),
        "{text}"
    );
}

#[test]
fn two_resources_sharing_a_base_font_are_selected_by_resource_key() {
    let out = run(&[
        fixture("format_twins.pdf").to_str().unwrap(),
        "--find",
        "hello world",
    ]);
    let text = stdout(&out);

    for key in ["FB1", "FB2"] {
        let r = row(&text, key);
        assert!(
            r.contains(&format!("selector=\"{key}\"")),
            "/{key} must be selected by its resource key:\n{r}"
        );
        assert!(r.contains("ambiguous /BaseFont"), "{r}");
    }
    // The run's own face is unambiguous and keeps the readable selector.
    assert!(row(&text, "F1").contains(r#"selector="Times-Roman""#));
    // And the offered bold is the twin that WORKS, by resource key.
    assert!(
        text.contains(
            r#"bold: a REAL bold face of this run's family is accepted — --set-font "FB2""#
        ),
        "{text}"
    );
}

#[test]
fn json_output_carries_every_field_the_listing_does() {
    let out = run(&[
        fixture("format_twins.pdf").to_str().unwrap(),
        "--find",
        "hello world",
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(0));
    let text = stdout(&out);

    for needle in [
        r#""run_resource": "F1""#,
        r#""run_font": "Times-Roman""#,
        r#""resource": "FB1""#,
        r#""base_font_ambiguous": true"#,
        r#""accepted": false"#,
        r#""refused_character": "U+006F""#,
        r#""selector": "FB2""#,
    ] {
        assert!(text.contains(needle), "missing {needle} in:\n{text}");
    }
}

#[test]
fn a_run_that_is_not_on_the_page_is_refused_by_name() {
    let out = run(&[
        fixture("format_family.pdf").to_str().unwrap(),
        "--find",
        "not on this page",
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(err.contains("font-preflight refused"), "{err}");
}

#[test]
fn page_zero_is_refused_because_the_flag_is_one_based() {
    let out = run(&[
        fixture("format_family.pdf").to_str().unwrap(),
        "--page",
        "0",
        "--find",
        "hello world",
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
}

#[test]
fn the_subcommand_writes_nothing() {
    let path = fixture("format_family.pdf");
    let before = std::fs::read(&path).unwrap();
    let listing = std::fs::read_dir(path.parent().unwrap()).unwrap().count();

    let out = run(&[path.to_str().unwrap(), "--find", "hello world"]);
    assert_eq!(out.status.code(), Some(0));

    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the input is not touched"
    );
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        listing,
        "and no sidecar appears beside it"
    );
}

// ---------------------------------------------------------------------------
// `Pass 147.0` — the pin, and the empty `--find` that used to say yes to
// everything
// ---------------------------------------------------------------------------

/// The first `op_start`/`op_len` from `extract-text --json --spans`.
fn first_span(path: &Path) -> (usize, usize) {
    let json = String::from_utf8_lossy(
        &Command::new(BIN)
            .args(["extract-text", path.to_str().unwrap(), "--json", "--spans"])
            .output()
            .expect("the binary runs")
            .stdout,
    )
    .into_owned();
    let i = json.find("\"op_start\": ").expect("--spans emits op_start");
    let rest = &json[i + "\"op_start\": ".len()..];
    let start: usize = rest[..rest.find(',').unwrap()].trim().parse().unwrap();
    let j = json.find("\"op_len\": ").expect("--spans emits op_len");
    let rest = &json[j + "\"op_len\": ".len()..];
    let len: usize = rest[..rest.find(',').unwrap()].trim().parse().unwrap();
    (start, len)
}

#[test]
fn a_pinned_empty_find_surveys_the_operator_and_does_not_say_yes_to_everything() {
    // The reported defect, end to end. Before `Pass 147.0` this printed every
    // face as ACCEPT — including /F3, which cannot show the run — because zero
    // characters were tested. The failure looked like a RICHER list.
    let path = fixture("format_family.pdf");
    let (start, len) = first_span(&path);
    let out = run(&[
        path.to_str().unwrap(),
        "--pin-span",
        &format!("{start}:{len}"),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    let text = stdout(&out);

    assert!(
        text.contains(r#"text="hello world""#),
        "the RESOLVED text is printed, so a caller sees what was tested: {text}"
    );
    assert!(row(&text, "F3").contains("REFUSE"), "{text}");
    assert!(text.contains("2 would be accepted for this run"), "{text}");
}

#[test]
fn a_pinned_empty_find_agrees_with_an_explicit_find_for_the_same_operator() {
    // The two spellings must describe the same characters, or a shell's
    // preview and its commit disagree.
    let path = fixture("format_family.pdf");
    let (start, len) = first_span(&path);
    let pinned = stdout(&run(&[
        path.to_str().unwrap(),
        "--pin-span",
        &format!("{start}:{len}"),
        "--json",
    ]));
    let explicit = stdout(&run(&[
        path.to_str().unwrap(),
        "--find",
        "hello world",
        "--json",
    ]));
    assert_eq!(pinned, explicit);
}

#[test]
fn an_empty_find_with_no_pin_is_refused_and_names_the_flag() {
    let out = run(&[fixture("format_family.pdf").to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(err.contains("--pin-span START:LEN"), "{err}");
}

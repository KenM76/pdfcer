//! # `pdfcer unembed-font` integration tests
//!
//! Black-box tests over the **real binary** for the first destructive font
//! operation (Pass 67.0 phase B). `pdfcer_core::font_unembed`'s own unit
//! tests pin the planning and the object surgery; these pin the *contract
//! the CLI publishes* — the stable stdout shape a batch sweep parses, the
//! exit codes a script branches on, and the disclosures that make an
//! irreversible operation something an operator can consent to.
//!
//! ## What these tests are protecting
//!
//! 1. **The dry run is the default.** `--apply` is the only thing that
//!    writes a file. A regression that made the default write would be
//!    silent — the command would look like it was working — and would take
//!    a file's fonts away from someone who was only looking.
//!
//! 2. **★ Every refusal is printed by name, with its reason.** The one
//!    thing this command does that Acrobat does not: Acrobat refuses the
//!    same fonts by omitting them from a list, with no reason shown
//!    anywhere (`Acrobat_Features/optimize__font_unembedding.md`, sourced
//!    to a former Adobe Principal Scientist). If the reason stopped being
//!    printed the output would still look correct and the feature would
//!    have lost its point, so the *sentence* is asserted, not the verdict
//!    token alone.
//!
//! 3. **The two byte figures.** `reclaim_on_full` is what a full rewrite
//!    drops; `reclaim_now` is what this save drops, which is **zero** under
//!    the default incremental mode. An operator whose goal is a smaller
//!    file is the one most likely to read one number and stop, so a
//!    regression that collapsed them into one would mislead exactly the
//!    audience the command has.
//!
//! 4. **The PDF/A gate.** Unembedding breaks a conformance claim the file
//!    makes about itself, so `--apply` on a PDF/A document refuses until
//!    `--acknowledge-pdfa` is given, and refuses **before** writing.
//!
//! 5. **The input is never touched**, in any mode. Asserted by byte
//!    comparison, because "nothing currently writes to the input" is not a
//!    contract.
//!
//! Fixtures (provenance in each directory's `PROVENANCE.md`):
//! `fixtures/synthetic/text/*`, `fixtures/synthetic/unembed/*`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED`. Spelled out rather than imported: these tests are
/// black-box over the binary, and hard-coding the number is what makes a
/// change to it a test failure rather than an invisible contract break.
const EDIT_REFUSED: i32 = 9;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

/// Run `unembed-font` and return `(exit code, stdout, stderr)`.
fn run(rel: &str, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN)
        .arg("unembed-font")
        .arg(fixture(rel))
        .args(args)
        .output()
        .expect("pdfcer runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("pdfcer-unembed-tests");
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join(name)
}

/// The default is a DRY RUN: the full report is printed, the exit is clean,
/// and no output file exists because none was asked for.
#[test]
fn the_default_is_a_dry_run() {
    let (code, stdout, stderr) = run("text/subset-simple-embedded.pdf", &["--all-removable"]);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("applied=0"), "stdout:\n{stdout}");
    assert!(
        stderr.contains("DRY RUN"),
        "the dry run says so, in words:\n{stderr}"
    );
    assert!(stdout.contains("unembed name=\"SUBSET+pdfceSubsetDemo\""));
    assert!(stdout.contains("rename=\"pdfceSubsetDemo\""));
}

/// `--apply` without `--output` is refused before the document is even
/// opened — the operator asked for a write and named no destination.
#[test]
fn apply_without_output_is_refused() {
    let (code, _, stderr) = run(
        "text/subset-simple-embedded.pdf",
        &["--all-removable", "--apply"],
    );
    assert_eq!(code, EDIT_REFUSED);
    assert!(stderr.contains("--apply needs --output"), "{stderr}");
}

/// ★ The two byte figures, and the sentence that explains the difference.
/// Under the default incremental mode nothing is reclaimed, and the command
/// says so rather than letting `reclaim_on_full` be read as a saving.
#[test]
fn an_incremental_save_reports_reclaiming_nothing() {
    let (code, stdout, stderr) = run("text/subset-simple-embedded.pdf", &["--all-removable"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("reclaim_on_full=720"), "stdout:\n{stdout}");
    assert!(stdout.contains("reclaim_now=0"), "stdout:\n{stdout}");
    assert!(
        stderr.contains("RECLAIMS NOTHING") && stderr.contains("--mode full"),
        "the difference is explained, and the fix named:\n{stderr}"
    );
}

/// A full rewrite is where the bytes go, and the output really is smaller.
#[test]
fn a_full_rewrite_actually_shrinks_the_file() {
    let out = temp_out("full-rewrite.pdf");
    let _ = std::fs::remove_file(&out);
    let (code, stdout, stderr) = run(
        "text/subset-simple-embedded.pdf",
        &[
            "--all-removable",
            "--apply",
            "--mode",
            "full",
            "--verify-undo",
            "-o",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("reclaim_now=720"), "stdout:\n{stdout}");
    assert!(stdout.contains("undo_identical=1"), "stdout:\n{stdout}");
    let source = std::fs::read(fixture("text/subset-simple-embedded.pdf")).unwrap();
    let written = std::fs::read(&out).unwrap();
    assert!(
        written.len() < source.len(),
        "wrote {} bytes from a {}-byte input",
        written.len(),
        source.len()
    );
}

/// The appearance change is stated as a fact, on every run that has a
/// target — dry run included. It is the consequence an operator is least
/// likely to have anticipated and the one a byte report cannot show.
#[test]
fn the_appearance_change_is_disclosed_as_a_certainty() {
    let (_, _, stderr) = run("text/subset-simple-embedded.pdf", &["--all-removable"]);
    assert!(
        stderr.contains("WILL LOOK DIFFERENT"),
        "not 'may' — this is what the operation does:\n{stderr}"
    );
    assert!(
        stderr.contains("/Widths is preserved"),
        "and the mechanism is named, so the claim is checkable:\n{stderr}"
    );
}

/// ★ The divergence from Acrobat. A font that cannot go is named, with its
/// object number, its size, its verdict token AND the sentence explaining
/// the mechanism.
#[test]
fn a_blocked_font_is_printed_by_name_with_its_reason() {
    let (code, stdout, _) = run(
        "text/cidfonttype2-nocmap-embedded.pdf",
        &["--all-removable"],
    );
    assert_eq!(code, EDIT_REFUSED, "nothing could go");
    assert!(
        stdout.contains("verdict=blocked-identity"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("reason: This font's text is stored as glyph indices"),
        "the reason names the mechanism:\n{stdout}"
    );
    assert!(stdout.contains("fonts=0"), "stdout:\n{stdout}");
}

/// A dry run that would do nothing exits non-zero: "nothing to do" and
/// "done" must not share an exit code, or a script cannot tell them apart.
#[test]
fn a_dry_run_with_no_target_exits_non_zero() {
    let (code, _, stderr) = run(
        "text/cidfonttype2-nocmap-embedded.pdf",
        &["--all-removable"],
    );
    assert_eq!(code, EDIT_REFUSED);
    assert!(stderr.contains("nothing would be unembedded"), "{stderr}");
}

/// A `--font` name that matches nothing is reported and exits non-zero —
/// never a silent success over a typo.
#[test]
fn an_unmatched_font_name_is_reported() {
    let (code, stdout, stderr) = run("text/subset-simple-embedded.pdf", &["--font", "Helvetica"]);
    assert_eq!(code, EDIT_REFUSED);
    assert!(
        stdout.contains("unmatched \"Helvetica\""),
        "stdout:\n{stdout}"
    );
    assert!(stderr.contains("matched no font"), "stderr:\n{stderr}");
}

/// `--font` takes either spelling — the name the file uses and the name the
/// operator reads.
#[test]
fn a_font_can_be_named_with_or_without_its_subset_tag() {
    for spelling in ["SUBSET+pdfceSubsetDemo", "pdfceSubsetDemo"] {
        let (code, stdout, stderr) = run("text/subset-simple-embedded.pdf", &["--font", spelling]);
        assert_eq!(code, 0, "{spelling}\nstderr:\n{stderr}");
        assert!(stdout.contains("fonts=1"), "{spelling}\nstdout:\n{stdout}");
    }
}

/// `--keep-subset-tag` leaves both names exactly as the file spells them,
/// and the rename disclosure does not fire.
#[test]
fn keep_subset_tag_leaves_the_name_alone() {
    let (code, stdout, stderr) = run(
        "text/subset-simple-embedded.pdf",
        &["--all-removable", "--keep-subset-tag"],
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("rename=unchanged"), "stdout:\n{stdout}");
    assert!(
        !stderr.contains("subset tag is being removed"),
        "no rename, no rename disclosure:\n{stderr}"
    );
}

/// ★ A PDF/A document is refused BEFORE anything is written, and the
/// refusal names the flag that would permit it.
#[test]
fn a_pdfa_document_is_refused_until_acknowledged() {
    let out = temp_out("pdfa-refused.pdf");
    let _ = std::fs::remove_file(&out);
    let (code, _, stderr) = run(
        "unembed/unembed-pdfa.pdf",
        &["--all-removable", "--apply", "-o", out.to_str().unwrap()],
    );
    assert_eq!(code, EDIT_REFUSED);
    assert!(stderr.contains("PDF/A-2B"), "the level is named:\n{stderr}");
    assert!(stderr.contains("--acknowledge-pdfa"), "{stderr}");
    assert!(
        !out.exists(),
        "the refusal fires BEFORE the write, so no file exists"
    );

    let (code, stdout, stderr) = run(
        "unembed/unembed-pdfa.pdf",
        &[
            "--all-removable",
            "--apply",
            "--acknowledge-pdfa",
            "-o",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("pdfa=pdfa-identified"), "stdout:\n{stdout}");
    assert!(out.exists());
    // The warning is still printed on the acknowledged run — acknowledging
    // a consequence does not make it stop being true.
    assert!(stderr.contains("breaks that conformance"), "{stderr}");
}

/// ★ A shared font program is reported as staying in the file, and no
/// saving is claimed for it. The naive implementation would free it and
/// silently blank the font that still needs it.
#[test]
fn a_shared_program_is_disclosed_as_not_reclaimed() {
    let (code, stdout, stderr) = run("unembed/unembed-shared-program.pdf", &["--all-removable"]);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("freed=0"), "stdout:\n{stdout}");
    assert!(stdout.contains("program_shared_with="), "stdout:\n{stdout}");
    assert!(stdout.contains("reclaim_on_full=0"), "stdout:\n{stdout}");
    assert!(
        stderr.contains("stays in the file"),
        "the operator is told the bytes do not go:\n{stderr}"
    );
}

/// ★ A shared descriptor blocks the removable font outright, by name.
#[test]
fn a_shared_descriptor_is_refused_by_name() {
    let (code, stdout, _) = run(
        "unembed/unembed-shared-descriptor.pdf",
        &["--all-removable"],
    );
    assert_eq!(code, EDIT_REFUSED);
    assert!(
        stdout.contains("verdict=descriptor-shared"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("would remove it from that font as well"),
        "stdout:\n{stdout}"
    );
}

/// The input file is never modified, in any mode.
#[test]
fn the_input_is_never_touched() {
    let path = fixture("text/subset-simple-embedded.pdf");
    let before = std::fs::read(&path).unwrap();
    let out = temp_out("input-untouched.pdf");
    let _ = std::fs::remove_file(&out);
    for args in [
        vec!["--all-removable"],
        vec!["--all-removable", "--apply", "-o", out.to_str().unwrap()],
    ] {
        let (_, _, _) = run("text/subset-simple-embedded.pdf", &args);
    }
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the input is read-only to this command"
    );
}

/// The output of an unembed still opens, and `list-fonts` now reports the
/// font as not embedded under its de-prefixed name.
///
/// The failure mode that matters most: a file that unembeds and then will
/// not load. Asserted through the binary's own reader rather than by
/// inspection, so the round trip is real.
#[test]
fn the_result_still_opens_and_reports_the_new_name() {
    let out = temp_out("reopens.pdf");
    let _ = std::fs::remove_file(&out);
    let (code, _, stderr) = run(
        "text/subset-simple-embedded.pdf",
        &[
            "--all-removable",
            "--apply",
            "--mode",
            "full",
            "-o",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");

    let listed = Command::new(BIN)
        .arg("list-fonts")
        .arg(&out)
        .output()
        .expect("pdfcer runs");
    assert_eq!(listed.status.code(), Some(0), "the result opens");
    let stdout = String::from_utf8_lossy(&listed.stdout);
    assert!(
        stdout.contains("name=\"pdfceSubsetDemo\""),
        "the tag is gone from the stored name:\n{stdout}"
    );
    assert!(stdout.contains("embedded=no"), "stdout:\n{stdout}");
    assert!(stdout.contains("verdict=not-embedded"), "stdout:\n{stdout}");
    assert!(stdout.contains("embedded=0 bytes=0"), "stdout:\n{stdout}");
}

/// ★ An invocation that names NO fonts is REFUSED, not answered with a
/// report of zeros.
///
/// The same defect `embed-font` carried, in its sibling and shipped in the
/// same Pass: `--font` and `--all-removable` shared a `clap` group that was
/// not `required`, so `unembed-font <file>` parsed cleanly, selected nothing,
/// and printed `fonts=0 refused=0 unmatched=0` followed by "Every refusal is
/// printed above with its reason" — with no refusals printed. Found by
/// checking whether the bug reported against `embed-font` was a one-off; it
/// was not.
///
/// See `embed_font.rs`'s `naming_no_fonts_is_refused_rather_than_reported_as_zero`
/// for the full reasoning; this pins the identical contract here so the two
/// commands cannot drift apart on it.
#[test]
fn naming_no_fonts_is_refused_rather_than_reported_as_zero() {
    let out = Command::new(BIN)
        .arg("unembed-font")
        .arg(fixture("unembed/unembed-many-pages.pdf"))
        .output()
        .expect("pdfcer runs");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(
        out.status.code().unwrap_or(-1),
        2,
        "clap usage error.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("fonts=0"),
        "the old bug: a zero-report instead of a refusal.\nstdout:\n{stdout}"
    );
    assert!(
        stderr.contains("--all-removable") && stderr.contains("--font"),
        "the refusal names both ways to satisfy it:\n{stderr}"
    );
}

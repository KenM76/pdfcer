//! # The bold/italic **fallback posture** (`Pass 179.x`, decision 106)
//!
//! Two operator rulings, on the same day, that have to hold together:
//!
//! 1. *"bold font should be automatically used if available, but otherwise
//!    synthetic should be supported, and the user shouldn't have to
//!    intervene."*
//! 2. *"let's still make the current method of warning or forcing it manually
//!    or refusing available as well as the automatic silent one."*
//!
//! The second is what this file covers: the postures pdfcer already had are
//! **kept**, not replaced, and the automatic one is added as the default.
//!
//! ## What is being pinned, and why each case is not redundant
//!
//! - **`refuse` still refuses**, with the message it always had. That was
//!   pdfcer's unconditional behaviour and the operator asked for it to remain
//!   reachable; a Pass that quietly dropped it would satisfy ruling 1 and
//!   break ruling 2.
//! - **`auto` and `warn` proceed** — the edit happens, which is the change.
//! - **★ Both of them still DISCLOSE.** *"Shouldn't have to intervene"*
//!   removed the **gate**, not the disclosure. `CLAUDE.md` rule 4's CLI half
//!   is that the invocation is the commit, so the command prints what it did
//!   on the way past. A posture that proceeded silently and said nothing
//!   would be the rule-4 failure this project keeps finding in the
//!   *understating* direction.
//! - **Nothing is emitted when nothing was passed over.** Without this the
//!   disclosure could be unconditional noise and every other assertion here
//!   would still pass.
//!
//! ## Why the fixture matters more than usual
//!
//! `format_family.pdf` carries `Times-Roman` **and** a real `Calibri-Bold`,
//! so a synthesis request on the `Times-Roman` run genuinely *could* have been
//! satisfied by a real face. On `format_other.pdf` — one `Helvetica` resource
//! — there is nothing to pass over, so every posture proceeds and says
//! nothing. Both files are needed: the first makes the postures
//! distinguishable, the second proves they are not just always-on.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/textedit")
        .join(name)
}

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("pdfcer-style-policy-tests");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

/// `(exit, stdout, stderr)` for one `format-text --bold-synthetic` run under
/// `policy`.
fn bold(fixture_name: &str, policy: &str, out_name: &str) -> (i32, String, String) {
    let out = temp_out(out_name);
    let res = Command::new(BIN)
        .args([
            "format-text",
            fixture(fixture_name).to_str().unwrap(),
            "--page",
            "1",
            "--find",
            "hello",
            "--bold-synthetic",
            "--style-policy",
            policy,
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("pdfcer runs");
    (
        res.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&res.stdout).into_owned(),
        String::from_utf8_lossy(&res.stderr).into_owned(),
    )
}

/// **`refuse` keeps pdfcer's prior behaviour, verbatim.**
///
/// The operator asked for it to stay available. It names the face, the
/// resource, that the family differs, and the exact `--set-font` to retry
/// with — none of which this test re-words, because the point is that the
/// sentence did not change.
#[test]
fn refuse_still_refuses_and_names_the_face() {
    let (_code, _out, err) = bold("format_family.pdf", "refuse", "refuse.pdf");
    assert!(
        err.contains("REAL bold face is available"),
        "the refusal must still fire under `refuse`: {err}"
    );
    assert!(
        err.contains("Calibri-Bold") && err.contains("/F2"),
        "and still name the face and its resource: {err}"
    );
    assert!(
        err.contains("Nothing was applied"),
        "and still apply nothing: {err}"
    );
}

/// **`auto` proceeds — and says what it passed over.**
///
/// The edit happening is ruling 1. The `note:` is rule 4: removing the gate
/// did not remove the obligation to disclose, and a posture that proceeded in
/// silence would understate what pdfcer had chosen on the operator's behalf.
#[test]
fn auto_proceeds_and_still_discloses_on_stdout() {
    let (code, out, _err) = bold("format_family.pdf", "auto", "auto.pdf");
    assert_eq!(code, 0, "auto does not refuse");
    assert!(
        out.contains("synthesis=synthetic bold"),
        "the synthesis actually happened: {out}"
    );
    assert!(
        out.contains("note:") && out.contains("REAL bold face is available"),
        "★ and auto is not SILENT about it -- rule 4 survives the ruling: {out}"
    );
}

/// **`warn` proceeds too, but on STDERR.**
///
/// The stream is the difference, and it is deliberate: a script that reads
/// only stdout still surfaces the warning, and a human sees it separated from
/// the result. Asserted on both streams, because "it warned" and "it warned
/// in the right place" are different claims.
#[test]
fn warn_proceeds_but_warns_on_stderr() {
    let (code, out, err) = bold("format_family.pdf", "warn", "warn.pdf");
    assert_eq!(code, 0, "warn does not refuse");
    assert!(
        out.contains("synthesis=synthetic bold"),
        "the edit still happened: {out}"
    );
    assert!(
        err.contains("warning:") && err.contains("REAL bold face is available"),
        "the warning is on stderr: {err}"
    );
    assert!(
        !out.contains("note:"),
        "and NOT also duplicated as a stdout note -- one disclosure, one place: {out}"
    );
}

/// **Nothing is said when nothing was passed over.**
///
/// The contrast case, and it is load-bearing: without it the disclosure could
/// be printed unconditionally and every assertion above would still pass.
/// `format_other.pdf` has a single `Helvetica` resource, so the synthesis is
/// genuinely the only route the current survey can see and there is nothing
/// to report.
#[test]
fn a_page_with_nothing_to_pass_over_is_quiet_under_every_posture() {
    for policy in ["auto", "warn", "refuse"] {
        let (code, out, err) = bold("format_other.pdf", policy, &format!("quiet-{policy}.pdf"));
        assert_eq!(
            code, 0,
            "{policy} proceeds when there is no real face: {err}"
        );
        assert!(
            out.contains("synthesis=synthetic bold"),
            "{policy}: the synthesis happened: {out}"
        );
        assert!(
            !out.contains("note:") && !err.contains("warning:"),
            "{policy}: nothing was passed over, so nothing is said: {out}{err}"
        );
    }
}

/// **The three postures differ ONLY in what they do about the fallback — the
/// edit they produce is the same.**
///
/// `auto` and `warn` must write the same bytes. If a posture changed which
/// face was chosen it would be a second resolution path, and two paths drift;
/// this is the assertion that would catch that.
#[test]
fn auto_and_warn_produce_identical_output() {
    let a = temp_out("same-auto.pdf");
    let w = temp_out("same-warn.pdf");
    for (policy, path) in [("auto", &a), ("warn", &w)] {
        let res = Command::new(BIN)
            .args([
                "format-text",
                fixture("format_family.pdf").to_str().unwrap(),
                "--page",
                "1",
                "--find",
                "hello",
                "--bold-synthetic",
                "--style-policy",
                policy,
                "-o",
                path.to_str().unwrap(),
            ])
            .output()
            .expect("pdfcer runs");
        assert_eq!(res.status.code(), Some(0), "{policy} succeeds");
    }
    assert_eq!(
        std::fs::read(&a).expect("auto output"),
        std::fs::read(&w).expect("warn output"),
        "a posture decides what to SAY about a fallback, never which face to use"
    );
}

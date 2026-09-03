//! # `dimension-add --kind two-lines` — pdfcer decides which dimension
//!
//! Black-box tests over the real binary for the operator's request
//! (2026-08-12), quoted so the acceptance criteria stay faithful:
//!
//! > "dimensioning tool should allow the selection of two lines. if those
//! > lines are parallel it makes a linear dimension between them like
//! > SolidWorks would, if they are at an angle it makes an angle dimension."
//!
//! and, mid-build:
//!
//! > "We should have an option in our settings and allow the user to set the
//! > tolerance for nearly parallel lines. When making or editing a dimension
//! > of this type, there should be a checkbox option to treat the two lines
//! > as parallel."
//!
//! ## What these tests protect
//!
//! The DECISION, not the drawing. `pdfcer-core`'s own unit tests pin the
//! classification maths; these pin that the CLI acts on it — that a parallel
//! pair really produces a `linear` ce dimension and an angled pair really
//! produces an `angular` one, read back through `dimension-list` rather than
//! from the authoring command's own output.
//!
//! Reading it back matters. The authoring command reports what it decided;
//! `dimension-list` reports what is actually in the file. A build that
//! decided correctly and wrote the wrong kind would pass the first and fail
//! the second, and only the second is what the operator ends up with.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED`, spelled out so a change to the number is a visible
/// test failure rather than a silent contract break.
const EDIT_REFUSED: i32 = 9;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf")
}

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("pdfcer-two-line-tests");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

/// Author from four points; return `(exit, stdout, stderr, output path)`.
fn author(name: &str, points: &str, extra: &[&str]) -> (i32, String, String, PathBuf) {
    let out = temp_out(name);
    let run = Command::new(BIN)
        .arg("dimension-add")
        .arg(fixture())
        .args(["--kind", "two-lines", "--points", points, "-o"])
        .arg(&out)
        .args(extra)
        .output()
        .expect("pdfcer runs");
    (
        run.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
        out,
    )
}

/// What `dimension-list` says is actually in the file.
fn listed_kind(path: &Path) -> String {
    let out = Command::new(BIN)
        .arg("dimension-list")
        .arg(path)
        .output()
        .expect("pdfcer runs");
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .find(|l| l.trim_start().starts_with("dim "))
        .map(std::string::ToString::to_string)
        .unwrap_or_default()
}

/// Two parallel lines produce a LINEAR ce dimension of the distance between
/// them — the operator's "like SolidWorks would".
#[test]
fn two_parallel_lines_produce_a_linear_dimension() {
    let (code, stdout, stderr, out) =
        author("parallel.pdf", "100,100 300,100 100,140 300,140", &[]);
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        stdout.contains("authored=linear"),
        "the decision must be reported:\n{stdout}"
    );
    assert!(
        stdout.contains("distance=40.0"),
        "and must measure the real 40pt gap:\n{stdout}"
    );
    let listed = listed_kind(&out);
    assert!(
        listed.contains("kind=linear"),
        "the FILE must actually contain a linear ce dimension, got {listed:?}"
    );
}

/// Two lines at an angle produce an ANGULAR ce dimension.
#[test]
fn two_angled_lines_produce_an_angular_dimension() {
    let (code, stdout, stderr, out) = author("angled.pdf", "100,100 300,100 100,100 273,200", &[]);
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        stdout.contains("authored=angular"),
        "the decision must be reported:\n{stdout}"
    );
    let listed = listed_kind(&out);
    assert!(
        listed.contains("kind=angular"),
        "the FILE must contain an angular ce dimension, got {listed:?}"
    );
    // ★ And the VALUE must read as an angle, not as a length. An angle routed
    // through the length formatter would be multiplied by the group scale and
    // come back as a plausible, wrong number with no unit.
    assert!(
        listed.contains('\u{b0}'),
        "the value must carry a degree sign, got {listed:?}"
    );
}

/// ★ The operator's checkbox: force the parallel reading, and still be told
/// what was overridden.
///
/// A control that hid the number it overrides would be asking for a decision
/// while withholding the fact it turns on.
#[test]
fn treat_as_parallel_overrides_the_measurement_and_discloses_it() {
    // ~5 degrees apart — unambiguously angled by the default 0.5 threshold.
    const NEARLY: &str = "100,100 300,100 100,140 300,157.5";

    let (_, plain, _, _) = author("near-auto.pdf", NEARLY, &[]);
    assert!(
        plain.contains("authored=angular"),
        "without the override this pair must read as angled:\n{plain}"
    );

    let (code, forced, stderr, out) = author("near-forced.pdf", NEARLY, &["--treat-as-parallel"]);
    assert_eq!(code, 0, "stdout:\n{forced}\nstderr:\n{stderr}");
    assert!(
        forced.contains("authored=linear"),
        "the override must change the DECISION:\n{forced}"
    );
    assert!(
        forced.contains("measured_angle=5."),
        "and must still report the real angle it overrode — a checkbox that \
         hid the number it overrides asks for a decision while withholding \
         the fact it turns on:\n{forced}"
    );
    assert!(
        listed_kind(&out).contains("kind=linear"),
        "and the file must contain what was decided"
    );
}

/// Collinear lines are refused by name rather than authored as a zero.
///
/// A zero-length dimension is not a drawing anyone wanted, and a tool that
/// silently produced one would have the operator hunting for a mark that is
/// there and invisible.
#[test]
fn collinear_lines_are_refused_by_name() {
    let (code, _stdout, stderr, out) = author("collinear.pdf", "0,0 100,0 200,0 300,0", &[]);
    assert_eq!(code, EDIT_REFUSED, "stderr:\n{stderr}");
    assert!(
        stderr.to_lowercase().contains("collinear"),
        "the refusal must name the actual condition:\n{stderr}"
    );
    assert!(
        !out.exists(),
        "a refused authoring must not leave an output file"
    );
}

/// Fewer than four points is a usage error, not a panic or a silent guess.
#[test]
fn fewer_than_four_points_is_refused() {
    let (code, _stdout, stderr, _) = author("short.pdf", "0,0 100,0 200,0", &[]);
    assert_eq!(code, EDIT_REFUSED);
    assert!(
        stderr.contains("FOUR points"),
        "the message must say how many are needed:\n{stderr}"
    );
}

/// Lines that would only meet if extended still dimension, and the virtual
/// apex is DISCLOSED.
///
/// Refusing would be wrong — CAD drawings dimension a virtual apex routinely
/// — but so would staying quiet, because the operator may not have realised
/// the two edges never actually touch.
#[test]
fn a_virtual_apex_is_dimensioned_and_disclosed() {
    let (code, stdout, stderr, _) = author("virtual.pdf", "0,0 50,0 200,100 250,150", &[]);
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        stdout.contains("apex_is_real=0"),
        "the report must mark the apex virtual:\n{stdout}"
    );
    assert!(
        stderr.contains("do not actually meet"),
        "and must say so in words the operator will read:\n{stderr}"
    );
}

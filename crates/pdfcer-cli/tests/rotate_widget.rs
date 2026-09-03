//! # `Pass 177.0` — `/MK /R` widget rotation, through the CLI
//!
//! The last unbuilt carrier of pdfcer's transform trio. Every other
//! `/Rect`-carrying annotation already rotates; `rotate_annotation` refused
//! widgets by name and cited a verb that did not exist.
//!
//! ## What these tests pin, and why each is not redundant
//!
//! 1. **The geometry is UNDISTORTED.** The interesting failure is not "it did
//!    not rotate" — it is "it rotated *and stretched*". §12.5.5 step (b) fits
//!    the transformed appearance box onto `/Rect` **anisotropically**, so
//!    spinning an already-drawn `w x h` appearance presents an `h x w` box to
//!    be squashed back into `w x h`. Asserted on the `/BBox` swap and the
//!    `/Matrix`, because those are what make the fit a 1:1 identity.
//! 2. **`/Rect` does not move.** A rotated field turns its CONTENT inside the
//!    box the operator placed. A version that rotated the rectangle would look
//!    plausible in isolation and move every field on the page.
//! 3. **The direction is COUNTERCLOCKWISE** (ISO 32000-1 §12.5.6.19
//!    Table 189), which is the OPPOSITE of the page's `/Rotate`. Asserted on
//!    the emitted matrix, because a sign flip is the single most likely
//!    "correction" a future session would make.
//! 4. **A silent file is not the same as `/R 0`**, and rotating back to
//!    upright removes the key rather than writing zero.
//! 5. **Where pdfcer cannot redraw, it says so** — the disclosure is not
//!    optional decoration, it is the difference between a working command and
//!    one an operator reports as broken.
//!
//! ## Why through the BINARY
//!
//! `--degrees` is normalised and refused in the engine, but the `was=-`
//! versus `was=0` distinction and the stale-appearance note are things only
//! the printed line carries. A unit test on `WidgetRotation` would pass on a
//! build whose CLI printed `0` for a silent file.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED`, spelled out so a change to the number is a visible
/// test failure rather than a silent contract break.
const EDIT_REFUSED: i32 = 9;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/forms/demo-form.pdf")
}

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("pdfcer-rotate-widget-tests");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN).args(args).output().expect("pdfcer runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The demo form with its text field filled, so the appearance has something
/// in it to be rotated.
fn filled(name: &str) -> PathBuf {
    let p = temp_out(&format!("{name}-filled.pdf"));
    let (code, _, err) = run(&[
        "fill-field",
        fixture().to_str().unwrap(),
        "--set",
        "FullName=ROTATE ME",
        "-o",
        p.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "fill-field: {err}");
    p
}

/// Every `/BBox`, `/Matrix` and `/Rect` array **anywhere in the file's bytes**,
/// as raw text.
///
/// A byte scan rather than a parse, deliberately: this asserts what pdfcer
/// WROTE, and a reader that normalised on the way back could hide a wrong
/// array behind a right model.
///
/// ★ **It sees EVERY REVISION of an incrementally-saved file, not the current
/// one.** So it answers *"did pdfcer ever write this array"* and cannot answer
/// *"is this array in force"*. Use it for presence (`.any(...)`); never for
/// absence, and never `.last()` — an earlier revision's `/Matrix` survives in
/// the bytes forever, which is the whole point of an incremental save. The
/// absence case is asserted by RENDERING instead, below.
fn arrays(path: &Path, key: &str) -> Vec<String> {
    let bytes = std::fs::read(path).expect("read output");
    let text = String::from_utf8_lossy(&bytes);
    let needle = format!("/{key}");
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(i) = text[from..].find(&needle) {
        let start = from + i + needle.len();
        if let Some(open) = text[start..].find('[') {
            let open = start + open + 1;
            if let Some(close) = text[open..].find(']') {
                out.push(text[open..open + close].trim().to_owned());
                from = open + close;
                continue;
            }
        }
        from = start;
    }
    out
}

// ---------------------------------------------------------------------------
// The geometry
// ---------------------------------------------------------------------------

/// **A quarter turn swaps the authored `/BBox`, writes the counterclockwise
/// `/Matrix`, and leaves `/Rect` alone.**
///
/// The three facts together are what make the rotation undistorted, and any
/// one of them alone is not enough:
///
/// - `/Matrix` without the swap ⇒ rotated AND stretched by `w/h`.
/// - the swap without `/Matrix` ⇒ the appearance is simply the wrong shape.
/// - moving `/Rect` ⇒ the field leaves the place the operator put it.
#[test]
fn a_quarter_turn_swaps_the_bbox_and_leaves_the_rect_alone() {
    let src = filled("geom");
    let out = temp_out("geom-90.pdf");
    let (code, stdout, err) = run(&[
        "rotate-widget",
        src.to_str().unwrap(),
        "--name",
        "FullName",
        "--degrees",
        "90",
        "-o",
        out.to_str().unwrap(),
        "--verify-undo",
    ]);
    assert_eq!(code, 0, "rotate-widget 90: {err}");
    assert!(stdout.contains("now=90"), "{stdout}");
    assert!(
        stdout.contains("regenerated=1"),
        "a text field's appearance IS redrawn: {stdout}"
    );

    // ★ COUNTERCLOCKWISE. §8.3.4's matrix `[cos, sin, -sin, cos, 0, 0]` at
    // 90 degrees, UNNEGATED. The page's `/Rotate` is the clockwise outlier and
    // is the reason a future session might "fix" this sign.
    let matrices = arrays(&out, "Matrix");
    assert!(
        matrices.iter().any(|m| m.starts_with("0.0 1.0 -1.0 0.0")),
        "the CCW quarter-turn matrix must be written: {matrices:?}"
    );

    // The field's own widget is 230 x 22 (`/Rect [20 150 250 172]`), so the
    // authored box must come back 22 x 230.
    let bboxes = arrays(&out, "BBox");
    assert!(
        bboxes.iter().any(|b| b.starts_with("0.0 0.0 22.0 230.0")),
        "the authored /BBox must be SWAPPED, or step (b) squashes it: {bboxes:?}"
    );

    // ★ And `/Rect` is untouched. Asserted against the literal the fixture
    // carries rather than against a re-derived number.
    let rects = arrays(&out, "Rect");
    assert!(
        rects
            .iter()
            .any(|r| r.replace(".0", "") == "20 150 250 172"),
        "/Rect must not move -- a rotated field turns its CONTENT: {rects:?}"
    );
}

/// **A half turn does NOT swap the box** — only the matrix changes.
///
/// The contrast case, and it is load-bearing: a swap keyed on "is there a
/// rotation" rather than on "is it a quarter turn" would pass every assertion
/// in the test above and silently transpose every 180-degree field.
#[test]
fn a_half_turn_keeps_the_box_and_only_flips_the_matrix() {
    let src = filled("half");
    let out = temp_out("half-180.pdf");
    let (code, stdout, err) = run(&[
        "rotate-widget",
        src.to_str().unwrap(),
        "--name",
        "FullName",
        "--degrees",
        "180",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "rotate-widget 180: {err}");
    assert!(stdout.contains("now=180"), "{stdout}");

    let matrices = arrays(&out, "Matrix");
    assert!(
        matrices.iter().any(|m| m.starts_with("-1.0 0.0 0.0 -1.0")),
        "the half-turn matrix: {matrices:?}"
    );
    let bboxes = arrays(&out, "BBox");
    assert!(
        bboxes.iter().any(|b| b.starts_with("0.0 0.0 230.0 22.0")),
        "a half turn must NOT swap the authored box: {bboxes:?}"
    );
}

/// **Rotating back to upright removes `/R` and writes no `/Matrix`.**
///
/// `0` is Table 189's default, so writing it explicitly would change the saved
/// bytes for no visible change — an R34 minimal-diff violation invisible until
/// somebody diffs two saves. The absent `/Matrix` is the same argument at the
/// appearance level: the identity matrix is the default, and emitting it would
/// rewrite every appearance in every form pdfcer touches, for nothing.
#[test]
fn rotating_back_to_upright_removes_the_key_rather_than_writing_zero() {
    let src = filled("zero");
    let rotated = temp_out("zero-90.pdf");
    let (code, _, err) = run(&[
        "rotate-widget",
        src.to_str().unwrap(),
        "--name",
        "FullName",
        "--degrees",
        "90",
        "-o",
        rotated.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");

    let back = temp_out("zero-back.pdf");
    let (code, stdout, err) = run(&[
        "rotate-widget",
        rotated.to_str().unwrap(),
        "--name",
        "FullName",
        "--degrees",
        "0",
        "-o",
        back.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "rotate back: {err}");
    assert!(
        stdout.contains("was=90"),
        "the previous rotation is reported: {stdout}"
    );
    assert!(
        stdout.contains("now=-"),
        "and the new state is SILENT, not zero: {stdout}"
    );

    // ★ Absence is asserted by RENDERING, not by scanning for a missing
    // `/Matrix`.
    //
    // The first draft of this test scanned the bytes and took `.last()`, and
    // it failed for a reason worth keeping: an incremental save KEEPS every
    // earlier revision, so the rotate-90 step's `/Matrix` is still in the
    // file and always will be. The bytes cannot answer "is this in force".
    //
    // Rendering can, and it answers the operator's question rather than the
    // file format's: a field rotated and rotated back must look exactly like
    // one that was never rotated.
    let a = temp_out("zero-orig.png");
    let b = temp_out("zero-back.png");
    for (pdf, png) in [(&src, &a), (&back, &b)] {
        let (code, _, err) = run(&[
            "render-page",
            pdf.to_str().unwrap(),
            "--page",
            "1",
            "--scale",
            "2",
            "-o",
            png.to_str().unwrap(),
        ]);
        assert_eq!(code, 0, "render-page: {err}");
    }
    assert_eq!(
        std::fs::read(&a).expect("orig png"),
        std::fs::read(&b).expect("restored png"),
        "a widget rotated and rotated back must RENDER identically to one never rotated"
    );
}

// ---------------------------------------------------------------------------
// The reported facts
// ---------------------------------------------------------------------------

/// **`was=-` means the file was SILENT, which is not `was=0`.**
///
/// Table 189 defaults `/R` to `0`, so a silent file renders upright — but a
/// shell seeded from `Some(0)` would write that invention into the document on
/// the operator's first press. The same distinction `forms::Widget::border`
/// makes, and `pdfcer-gui` refused to ship a border control rather than blur it.
#[test]
fn a_silent_file_reports_a_dash_and_not_a_zero() {
    let src = filled("silent");
    let out = temp_out("silent-out.pdf");
    let (code, stdout, err) = run(&[
        "rotate-widget",
        src.to_str().unwrap(),
        "--name",
        "FullName",
        "--degrees",
        "90",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(
        stdout.contains("was=-"),
        "the fixture's widget has no /MK /R, and that is reported as silence: {stdout}"
    );
    assert!(!stdout.contains("was=0"), "and NOT as zero: {stdout}");
}

/// **A negative or over-turn angle is accepted and reduced, and the reduction
/// is REPORTED.**
///
/// The standard's entire constraint is "a multiple of 90" — unbounded — so
/// `-90` and `450` conform and mean what they say. Normalising into `[0, 360)`
/// is pdfcer's product rule, so a caller who passed `-90` and reads back `270`
/// is told that pdfcer did that, rather than being left to wonder whether the
/// file had been wrong.
#[test]
fn a_negative_angle_is_reduced_and_the_reduction_is_reported() {
    let src = filled("neg");
    for (asked, want) in [("-90", "270"), ("450", "90"), ("-270", "90")] {
        let out = temp_out(&format!("neg{asked}.pdf"));
        let (code, stdout, err) = run(&[
            "rotate-widget",
            src.to_str().unwrap(),
            "--name",
            "FullName",
            "--degrees",
            asked,
            "-o",
            out.to_str().unwrap(),
        ]);
        assert_eq!(code, 0, "rotate-widget {asked}: {err}");
        assert!(
            stdout.contains(&format!("now={want}")),
            "{asked} must store {want}: {stdout}"
        );
        assert!(
            stdout.contains("normalised=1"),
            "and say that it reduced it: {stdout}"
        );
    }

    // A value already in range reports no normalisation -- the contrast case,
    // without which `normalised=1` could be hard-coded.
    let out = temp_out("neg-plain.pdf");
    let (code, stdout, err) = run(&[
        "rotate-widget",
        src.to_str().unwrap(),
        "--name",
        "FullName",
        "--degrees",
        "270",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(stdout.contains("normalised=0"), "{stdout}");
}

/// **A non-multiple of 90 is refused by name, and nothing is written.**
///
/// Refused rather than rounded: a widget declared at 45 degrees has no
/// conforming meaning, and snapping it to 90 would put a rotation on an
/// operator's form that they did not ask for and could not see was
/// substituted.
#[test]
fn a_non_quarter_turn_is_refused_and_writes_nothing() {
    let src = filled("refuse");
    let out = temp_out("refuse-out.pdf");
    let (code, _, err) = run(&[
        "rotate-widget",
        src.to_str().unwrap(),
        "--name",
        "FullName",
        "--degrees",
        "45",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, EDIT_REFUSED, "45 degrees is refused: {err}");
    assert!(
        err.contains("multiple of 90"),
        "and the message says what the rule is: {err}"
    );
    assert!(!out.exists(), "a refusal writes no output file");
}

/// ★★ **Where pdfcer cannot redraw the appearance, it SAYS SO.**
///
/// A check box's `/AP` is state-keyed artwork pdfcer's text generator does not
/// produce, so `/MK /R` is written and the pixels do not move. That is the
/// outcome most likely to be reported as a defect, because PDF Association
/// erratum #56 (`ISO approved`) puts `MK` in §12.5.2's ignore-list — a
/// conforming PDF 2.0 reader shows the field upright however `/R` reads.
///
/// The disclosure is the whole difference between a command that worked in a
/// way the operator did not expect and one that appears broken.
#[test]
fn a_widget_pdfcer_cannot_redraw_is_rotated_and_disclosed() {
    let src = filled("stale");
    let out = temp_out("stale-out.pdf");
    let (code, stdout, err) = run(&[
        "rotate-widget",
        src.to_str().unwrap(),
        "--name",
        "Subscribe",
        "--degrees",
        "90",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "rotating a check box succeeds: {err}");
    assert!(
        stdout.contains("now=90"),
        "the declaration IS written: {stdout}"
    );
    assert!(
        stdout.contains("regenerated=0"),
        "and the appearance is NOT redrawn: {stdout}"
    );
    assert!(
        stdout.contains("note:") && stdout.contains("did NOT redraw"),
        "and that is disclosed in as many words: {stdout}"
    );
    assert!(
        stdout.contains("erratum #56") || stdout.contains("PDF 2.0 reader ignores /MK"),
        "including WHY it will still look upright: {stdout}"
    );
}

/// **An unknown field and an out-of-range widget index are both refused.**
#[test]
fn an_unknown_target_is_refused() {
    let src = filled("unknown");
    for args in [
        vec!["--name", "NoSuchField", "--degrees", "90"],
        vec!["--name", "FullName", "--index", "7", "--degrees", "90"],
    ] {
        let out = temp_out("unknown-out.pdf");
        let mut argv = vec!["rotate-widget", src.to_str().unwrap()];
        argv.extend(args.iter().copied());
        argv.extend(["-o", out.to_str().unwrap()]);
        let (code, _, err) = run(&argv);
        assert_eq!(code, EDIT_REFUSED, "refused by name: {err}");
        assert!(!out.exists(), "a refusal writes no output file");
    }
}

/// **The rotation READS BACK**, so a shell can seed a control from the file
/// rather than from a guess.
///
/// A property pdfcer can write and cannot read is exactly the asymmetry
/// `pdfcer-gui` refused to ship a border control for. `list-fields --widgets`
/// is where a shell would look.
#[test]
fn the_rotation_reads_back_from_the_saved_file() {
    let src = filled("readback");
    let out = temp_out("readback-out.pdf");
    let (code, _, err) = run(&[
        "rotate-widget",
        src.to_str().unwrap(),
        "--name",
        "FullName",
        "--degrees",
        "270",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");

    let (code, listing, err) = run(&["list-fields", out.to_str().unwrap(), "--widgets"]);
    assert_eq!(code, 0, "list-fields: {err}");
    assert!(
        listing.contains("rotation=270"),
        "the widget listing must report the rotation it carries: {listing}"
    );
    // And the un-rotated check box in the same file reports silence, not 0.
    assert!(
        listing.contains("rotation=-"),
        "a widget whose file is silent reports a dash: {listing}"
    );
}

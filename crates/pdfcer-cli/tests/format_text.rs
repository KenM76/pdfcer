//! # `pdfcer format-text` integration tests (Pass 14.2)
//!
//! Black-box tests over the **real binary** for in-place text FORMATTING —
//! size, fill colour, and font-family/style change. They assert the process
//! contract a script depends on (exit codes, the stable report lines, the
//! verbatim disclosures) across the three Pass-14.2 fixtures in
//! `fixtures/synthetic/textedit/` (provenance: that directory's
//! `PROVENANCE.md`), plus the shared `tagged.pdf` for the anti-corruption
//! case. Each acceptance clause of decision 014 §5.2's "13.2" slice has a
//! test here:
//!
//! - a size change succeeds, is minimal-diff (the fill-colour operator is
//!   untouched), and the incremental output keeps every untouched object
//!   byte-identical (the original file is a byte-prefix of the output);
//! - a colour change STORES the chosen device space (`cmyk:` -> `k`), never
//!   force-converting to DeviceRGB;
//! - a colour change on a non-device (`Other`) original DISCLOSES the
//!   narrowing and restores the tail verbatim;
//! - a family change to a fully-covering target succeeds and re-encodes;
//! - a family change whose target cannot cover the run is REFUSED by name
//!   (exit `EDIT_REFUSED`) with nothing applied;
//! - a `--font-dir`-supplied face lifts a non-embedded target to `Supplied`;
//! - a colour change on a tagged run keeps its `/MCID` wrapper and discloses
//!   staleness (the anti-Acrobat-tag-corruption property).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
/// `exit::EDIT_REFUSED` in the CLI's stable exit-code contract.
const EDIT_REFUSED: i32 = 9;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/textedit")
}

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(name)
}

/// A unique temp path so parallel tests never collide.
fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pdfcer_fmt_{tag}_{}_{n}.pdf", std::process::id()))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("format-text")
        .args(args)
        .output()
        .expect("the binary runs")
}

/// Run the binary with NO subcommand prepended — [`run`] hard-codes
/// `format-text`, and these assertions have to re-read the OUTPUT FILE with a
/// different verb to check what actually landed in it.
fn run_raw(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn size_change_succeeds_and_is_minimal_diff() {
    let out_path = temp_path("size");
    let out = run(&[
        fixture("format_color.pdf").to_str().unwrap(),
        "--find",
        "hello",
        "--set-size",
        "24",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("set_size=12->24"), "{text}");
    assert!(text.contains("set_color=none"), "colour untouched: {text}");
    // Incremental append: the original is a byte-prefix of the output.
    let orig = std::fs::read(fixture("format_color.pdf")).unwrap();
    let edited = std::fs::read(&out_path).unwrap();
    assert!(edited.starts_with(&orig), "must be an incremental append");
    // The new size is emitted; the original blue survives byte-verbatim.
    let s = String::from_utf8_lossy(&edited);
    assert!(s.contains("/F1 24 Tf"), "new size emitted");
    assert!(s.contains("0 0 1 rg"), "original colour untouched");
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn cmyk_color_change_stores_k_not_devicergb() {
    let out_path = temp_path("cmyk");
    let out = run(&[
        fixture("format_color.pdf").to_str().unwrap(),
        "--find",
        "hello",
        "--set-color",
        "cmyk:0,1,1,0",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    assert!(stdout(&out).contains("set_color=k"), "{}", stdout(&out));
    let edited = std::fs::read(&out_path).unwrap();
    assert!(
        String::from_utf8_lossy(&edited).contains("0 1 1 0 k"),
        "CMYK stored as the k operator, not converted to rg"
    );
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn other_space_color_change_discloses_narrowing_and_restores_tail() {
    let out_path = temp_path("other");
    let out = run(&[
        fixture("format_other.pdf").to_str().unwrap(),
        "--find",
        "hello",
        "--set-color",
        "rgb:1,0,0",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("fill_narrowed=1"), "{text}");
    assert!(text.contains("NARROWING"), "narrowing disclosed: {text}");
    // The original non-device scn sequence is restored verbatim.
    let edited = std::fs::read(&out_path).unwrap();
    assert!(
        String::from_utf8_lossy(&edited).contains("/CS0 cs 0.7 scn"),
        "the Other-space colour is restored byte-verbatim for the tail"
    );
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn family_change_to_a_covering_target_succeeds_and_reencodes() {
    let out_path = temp_path("fam");
    let out = run(&[
        fixture("format_family.pdf").to_str().unwrap(),
        "--find",
        "hello world",
        "--set-font",
        "F2",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    assert!(
        stdout(&out).contains("set_font=Times-Roman->Calibri-Bold"),
        "{}",
        stdout(&out)
    );
    let edited = std::fs::read(&out_path).unwrap();
    assert!(
        String::from_utf8_lossy(&edited).contains("/F2 12 Tf"),
        "the Tf resource is swapped to the target"
    );
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn family_change_by_base_font_resolves_the_resource() {
    let out_path = temp_path("fambf");
    let out = run(&[
        fixture("format_family.pdf").to_str().unwrap(),
        "--find",
        "hello world",
        "--set-font",
        "Calibri-Bold",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let edited = std::fs::read(&out_path).unwrap();
    assert!(String::from_utf8_lossy(&edited).contains("/F2 12 Tf"));
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn family_coverage_failure_is_refused_with_nothing_applied() {
    let out_path = temp_path("cov");
    let out = run(&[
        fixture("format_family.pdf").to_str().unwrap(),
        "--find",
        "hello world",
        "--set-font",
        "F3",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED), "refusal is exit 9");
    let err = stderr(&out);
    // The uncovered character 'o' (U+006F) is named.
    assert!(
        err.contains("U+006F"),
        "coverage refusal names the char: {err}"
    );
    // Nothing was written.
    assert!(!out_path.exists(), "a refused format writes nothing");
}

#[test]
fn supplied_font_dir_lifts_the_family_target_to_supplied() {
    // Copy the in-repo Foxit CFF into a temp dir as `Calibri-Bold.cff` so
    // --font-dir registers a `Calibri-Bold` face (decision 012).
    // Per-call-unique temp dir (pid = binary-safe, atomic `N` = thread-safe) so
    // no two concurrent tests share this scratch dir. See `temp_path` above.
    let font_dir = {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("pdfcer_fmt_fd_{}_{n}", std::process::id()))
    };
    std::fs::create_dir_all(&font_dir).unwrap();
    let cff =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../pdfcer-render/assets/fonts/FoxitSans.cff");
    std::fs::copy(&cff, font_dir.join("Calibri-Bold.cff")).unwrap();

    let out_path = temp_path("sup");
    let out = run(&[
        fixture("format_family.pdf").to_str().unwrap(),
        "--find",
        "hello world",
        "--set-font",
        "F2",
        "--font-dir",
        font_dir.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    assert!(
        stdout(&out).contains("glyph_source=Supplied"),
        "{}",
        stdout(&out)
    );
    let _ = std::fs::remove_file(out_path);
    let _ = std::fs::remove_dir_all(font_dir);
}

#[test]
fn tagged_run_color_change_keeps_mcid_and_discloses() {
    let out_path = temp_path("tag");
    let out = run(&[
        fixture("tagged.pdf").to_str().unwrap(),
        "--find",
        "teh",
        "--set-color",
        "rgb:1,0,0",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("tagged_mcid=0"), "{text}");
    assert!(text.contains("R72"), "tagged staleness disclosed: {text}");
    // The MCID wrapper survives (the anti-corruption property).
    let edited = std::fs::read(&out_path).unwrap();
    assert!(
        String::from_utf8_lossy(&edited).contains("/MCID 0"),
        "the MCID wrapper must survive a formatting change"
    );
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn missing_target_font_is_refused_by_name() {
    let out_path = temp_path("miss");
    let out = run(&[
        fixture("format_family.pdf").to_str().unwrap(),
        "--find",
        "hello world",
        "--set-font",
        "Nonexistent",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    assert!(
        stderr(&out).contains("not an existing font resource"),
        "{}",
        stderr(&out)
    );
    assert!(!out_path.exists());
}

#[test]
fn no_op_request_is_refused() {
    let out_path = temp_path("noop");
    let out = run(&[
        fixture("format_color.pdf").to_str().unwrap(),
        "--find",
        "hello",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    assert!(
        stderr(&out).contains("no formatting operation"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn bad_color_spec_is_refused_before_io() {
    let out_path = temp_path("badc");
    let out = run(&[
        fixture("format_color.pdf").to_str().unwrap(),
        "--find",
        "hello",
        "--set-color",
        "rgb:1,0", // too few components
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    assert!(!out_path.exists());
}

// ---------------------------------------------------------------------------
// `Pass 162.0` — a face the page does NOT carry
// ---------------------------------------------------------------------------

/// ★★ The CLI uses `text_edit::set_format`, a save path the `EditSession`
/// tests never touch.
///
/// This Pass first wired the resource-creation into the two session paths
/// only. Every core unit test passed. The binary printed the disclosure saying
/// a font resource had been added and wrote a file in which it had not — a
/// content stream naming `/pdfceF1` with no `/pdfceF1` anywhere. That is the
/// defect this test exists for, and it is asserted on the OUTPUT FILE
/// (`list-fonts` re-reads it) rather than on the report the command printed
/// about itself.
#[test]
fn set_font_to_a_face_the_page_lacks_adds_the_resource_to_the_file() {
    let out_path = temp_path("newface");
    let out = run(&[
        fixture("format_family.pdf").to_str().unwrap(),
        "--find",
        "hello",
        "--set-font",
        "Helvetica",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("was NOT a font resource here")
            || stdout(&out).contains("was NOT a font resource here"),
        "adding a resource the operator did not ask for must be disclosed — \
         the invocation IS the commit here, so this is the only chance to say \
         so: {}{}",
        stdout(&out),
        stderr(&out)
    );

    // The file, not the report.
    let listed = run_raw(&["list-fonts", out_path.to_str().unwrap()]);
    assert!(listed.status.success());
    let text = stdout(&listed);
    assert!(
        text.contains("name=\"Helvetica\""),
        "the saved file must actually carry the font: {text}"
    );
    assert!(
        text.contains("resources=pdfceF1"),
        "bound under the non-colliding key the disclosure named: {text}"
    );

    // And the words survive the re-encode.
    let extracted = run_raw(&["extract-text", out_path.to_str().unwrap()]);
    assert!(
        stdout(&extracted).contains("hello world"),
        "the restyled run must still read: {}",
        stdout(&extracted)
    );
}

/// A face outside the standard 14 still needs a font program, so it is still
/// refused — and the refusal still names both the face and the deferral, so it
/// does not read as a permanent no.
#[test]
fn set_font_to_a_non_standard_14_face_is_still_refused() {
    let out_path = temp_path("arial");
    let out = run(&[
        fixture("format_family.pdf").to_str().unwrap(),
        "--find",
        "hello",
        "--set-font",
        "Arial",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_ne!(out.status.code(), Some(0), "Arial cannot be authored");
    let err = stderr(&out);
    assert!(err.contains("Arial"), "{err}");
    assert!(err.contains("FF-C"), "{err}");
    assert!(!out_path.exists(), "a refusal must write no output file");
}

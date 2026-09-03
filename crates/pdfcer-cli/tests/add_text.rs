//! # `pdfcer add-text` integration tests (Pass 16.0 / FF-D)
//!
//! Black-box tests over the **real binary** for add-new-page-text. They assert
//! the process contract a script depends on — exit codes, the stable report
//! lines, the verbatim disclosures — across the three committed
//! `fixtures/synthetic/addtext/` fixtures (provenance: that directory's
//! `PROVENANCE.md`). Each acceptance clause of decision 016 §6's 16.0 slice has
//! a test here:
//!
//! - a plain add succeeds, the incremental output keeps the original bytes as a
//!   prefix, the report discloses `provenance=Bundled`, and the output RENDERS
//!   (R59) via `render-page`;
//! - a glyph the face lacks is REFUSED by name (exit `EDIT_REFUSED`), no output;
//! - a tagged page emits the R73 untagged disclosure;
//! - an inherited-`/Resources` page discloses the inheritance-safe add;
//! - `--font Times-Roman` writes that exact `/BaseFont`;
//! - a `--font-dir` face lifts the disclosed provenance to `Supplied`;
//! - an enforced-DocMDP certified document REFUSES add-text — point and box —
//!   by name (exit `EDIT_REFUSED`, no output), the §12.8.4 add-markup mirror.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
/// `exit::EDIT_REFUSED` in the CLI's stable exit-code contract.
const EDIT_REFUSED: i32 = 9;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/addtext")
}

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(name)
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pdfcer_add_{tag}_{}_{n}.pdf", std::process::id()))
}

fn run_add(args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("add-text")
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
fn plain_add_succeeds_incremental_bundled_and_renders() {
    let out_path = temp_path("plain");
    let out = run_add(&[
        fixture("plain.pdf").to_str().unwrap(),
        "--at",
        "100,650",
        "--text",
        "Hello world",
        "--size",
        "14",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("provenance=Bundled"), "{text}");
    assert!(text.contains("base_font=Helvetica"), "{text}");
    assert!(text.contains("font_resource=/pdfceF1"), "{text}");

    // The incremental save keeps every untouched object byte-identical: the
    // original file is a byte-prefix of the output.
    let orig = std::fs::read(fixture("plain.pdf")).unwrap();
    let edited = std::fs::read(&out_path).unwrap();
    assert!(
        edited.starts_with(&orig),
        "output must be an incremental append"
    );
    assert!(edited.len() > orig.len());

    // R59: the output renders without error (and the added run rasterizes).
    let png = temp_path("plain").with_extension("png");
    let render = Command::new(BIN)
        .arg("render-page")
        .args([
            out_path.to_str().unwrap(),
            "--page",
            "1",
            "-o",
            png.to_str().unwrap(),
        ])
        .output()
        .expect("render runs");
    assert!(
        render.status.success(),
        "render exit 0: {}",
        stderr(&render)
    );
    assert!(png.exists(), "a PNG was written");

    let _ = std::fs::remove_file(out_path);
    let _ = std::fs::remove_file(png);
}

#[test]
fn missing_glyph_is_refused_by_name() {
    let out_path = temp_path("bad");
    let out = run_add(&[
        fixture("plain.pdf").to_str().unwrap(),
        "--at",
        "100,650",
        "--text",
        "hi \u{4E2D}", // a CJK char outside WinAnsi's single-byte repertoire
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED), "refusal is exit 9");
    let err = stderr(&out);
    assert!(err.contains("R-INV-1"), "named refusal: {err}");
    assert!(!out_path.exists(), "a refused add writes nothing");
}

#[test]
fn tagged_page_add_discloses_untagged() {
    let out_path = temp_path("tag");
    let out = run_add(&[
        fixture("tagged.pdf").to_str().unwrap(),
        "--at",
        "120,600",
        "--text",
        "Added tagged",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("tagged_untagged=true"), "{text}");
    assert!(
        text.contains("untagged page content") && text.contains("R73"),
        "the R73 disclosure must be surfaced: {text}"
    );
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn inherited_resources_add_discloses_the_inheritance_safe_recipe() {
    let out_path = temp_path("inh");
    let out = run_add(&[
        fixture("inherited-resources.pdf").to_str().unwrap(),
        "--at",
        "120,600",
        "--text",
        "Added here",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("gave_page_own_resources=true"), "{text}");
    assert!(
        text.contains("INHERITED") && text.contains("NOT modified"),
        "the inheritance-safe disclosure must be surfaced: {text}"
    );
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn chosen_standard_14_face_is_written() {
    let out_path = temp_path("times");
    let out = run_add(&[
        fixture("plain.pdf").to_str().unwrap(),
        "--at",
        "100,650",
        "--text",
        "Times please",
        "--font",
        "Times-Roman",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    assert!(
        stdout(&out).contains("base_font=Times-Roman"),
        "{}",
        stdout(&out)
    );
    let bytes = std::fs::read(&out_path).unwrap();
    assert!(
        String::from_utf8_lossy(&bytes).contains("/BaseFont /Times-Roman"),
        "the chosen face must be written into the font dict"
    );
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn bad_font_name_is_a_clean_refusal() {
    let out_path = temp_path("badfont");
    let out = run_add(&[
        fixture("plain.pdf").to_str().unwrap(),
        "--at",
        "100,650",
        "--text",
        "x",
        "--font",
        "Arial", // not a Standard-14 spelling
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    assert!(stderr(&out).contains("not a Standard-14 BaseFont name"));
    assert!(!out_path.exists());
}

#[test]
fn bad_at_coordinate_is_a_clean_refusal() {
    let out_path = temp_path("badat");
    let out = run_add(&[
        fixture("plain.pdf").to_str().unwrap(),
        "--at",
        "100",
        "--text",
        "x",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    assert!(stderr(&out).contains("--at expects"));
    assert!(!out_path.exists());
}

#[test]
fn boxed_add_wraps_renders_and_is_incremental() {
    // BOXED mode (16.1): wrap to a 40pt-wide box, then R59-render the output.
    let out_path = temp_path("box");
    let out = run_add(&[
        fixture("plain.pdf").to_str().unwrap(),
        "--box",
        "72,600,40,120",
        "--align",
        "left",
        "--font",
        "Courier",
        "--size",
        "10",
        "--text",
        "wrap this text into the box",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("mode=boxed"), "{text}");
    assert!(text.contains("align=left"), "{text}");
    assert!(text.contains("wrapped_lines="), "{text}");

    // Incremental append: the original is a byte-prefix of the output.
    let orig = std::fs::read(fixture("plain.pdf")).unwrap();
    let edited = std::fs::read(&out_path).unwrap();
    assert!(edited.starts_with(&orig), "boxed add must be incremental");

    // R59: the wrapped output renders.
    let png = temp_path("box").with_extension("png");
    let render = Command::new(BIN)
        .arg("render-page")
        .args([
            out_path.to_str().unwrap(),
            "--page",
            "1",
            "-o",
            png.to_str().unwrap(),
        ])
        .output()
        .expect("render runs");
    assert!(
        render.status.success(),
        "render exit 0: {}",
        stderr(&render)
    );
    assert!(png.exists());

    let _ = std::fs::remove_file(out_path);
    let _ = std::fs::remove_file(png);
}

#[test]
fn boxed_justified_surfaces_the_alignment() {
    let out_path = temp_path("boxj");
    let out = run_add(&[
        fixture("plain.pdf").to_str().unwrap(),
        "--box",
        "72,600,60,120",
        "--align",
        "justify",
        "--font",
        "Courier",
        "--size",
        "10",
        "--text",
        "aa aa aa aa",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    assert!(stdout(&out).contains("align=justified"), "{}", stdout(&out));
    // The justified line's TJ slack is in the appended stream bytes.
    let bytes = std::fs::read(&out_path).unwrap();
    assert!(
        String::from_utf8_lossy(&bytes).contains("] TJ"),
        "a justified line is a TJ array"
    );
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn at_and_box_are_mutually_exclusive() {
    let out_path = temp_path("both");
    let out = run_add(&[
        fixture("plain.pdf").to_str().unwrap(),
        "--at",
        "72,700",
        "--box",
        "72,600,40,120",
        "--text",
        "x",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    assert!(stderr(&out).contains("mutually exclusive"));
    assert!(!out_path.exists());
}

#[test]
fn neither_at_nor_box_is_refused() {
    let out_path = temp_path("none");
    let out = run_add(&[
        fixture("plain.pdf").to_str().unwrap(),
        "--text",
        "x",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    assert!(stderr(&out).contains("needs a placement"));
    assert!(!out_path.exists());
}

#[test]
fn bad_box_geometry_is_a_clean_refusal() {
    let out_path = temp_path("badbox");
    let out = run_add(&[
        fixture("plain.pdf").to_str().unwrap(),
        "--box",
        "72,600,40", // only three components
        "--text",
        "x",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(EDIT_REFUSED));
    assert!(stderr(&out).contains("--box expects"));
    assert!(!out_path.exists());
}

#[test]
fn supplied_font_dir_lifts_provenance_to_supplied() {
    // Copy the in-repo Foxit CFF into a temp dir as `Helvetica.cff` so
    // --font-dir registers a `Helvetica` face (decision 012).
    // Per-call-unique temp dir (pid = binary-safe, atomic `N` = thread-safe) so
    // no two concurrent tests share this scratch dir. See `temp_path` above.
    let font_dir = {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("pdfcer_afd_{}_{n}", std::process::id()))
    };
    std::fs::create_dir_all(&font_dir).unwrap();
    let cff =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../pdfcer-render/assets/fonts/FoxitSans.cff");
    std::fs::copy(&cff, font_dir.join("Helvetica.cff")).unwrap();

    let out_path = temp_path("sup");
    let out = run_add(&[
        fixture("plain.pdf").to_str().unwrap(),
        "--at",
        "100,650",
        "--text",
        "Supplied face",
        "--font-dir",
        font_dir.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "exit 0: {}", stderr(&out));
    assert!(
        stdout(&out).contains("provenance=Supplied"),
        "{}",
        stdout(&out)
    );

    let _ = std::fs::remove_file(out_path);
    let _ = std::fs::remove_dir_all(font_dir);
}

#[test]
fn certified_document_refuses_point_add_by_name() {
    // FF-D follow-up: adding page content to a PDF whose enforced DocMDP
    // certification forbids structural changes is REFUSED (§12.8.4), the same
    // as `EditSession::add_markup` — a clean named non-zero exit, no output.
    let out_path = temp_path("cert_point");
    let out = run_add(&[
        fixture("certified-locked.pdf").to_str().unwrap(),
        "--at",
        "100,650",
        "--text",
        "should be blocked",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(EDIT_REFUSED),
        "an enforced-certified doc refuses add-text with EDIT_REFUSED: {}",
        stderr(&out)
    );
    let err = stderr(&out);
    assert!(
        err.contains("certification signature") && err.contains("12.8.4"),
        "the refusal cites the certification and §12.8.4: {err}"
    );
    assert!(!out_path.exists(), "a refused add writes nothing");
}

#[test]
fn certified_document_refuses_box_add_by_name() {
    // The boxed placement path shares the same guard — likewise refused.
    let out_path = temp_path("cert_box");
    let out = run_add(&[
        fixture("certified-locked.pdf").to_str().unwrap(),
        "--box",
        "72,600,180,120",
        "--text",
        "should be blocked",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(EDIT_REFUSED),
        "a boxed add to an enforced-certified doc is refused: {}",
        stderr(&out)
    );
    assert!(stderr(&out).contains("certification signature"));
    assert!(!out_path.exists());
}

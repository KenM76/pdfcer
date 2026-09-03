//! # `pdfcer export-dxf` integration tests — the MULTI-PAGE mode and
//! the scale gate (Pass 52.2)
//!
//! Black-box: these spawn the **real binary** (`CARGO_BIN_EXE_pdfcer`)
//! and assert on its process contract — exit code, the stable stdout
//! result line, the stderr prose, and the files it wrote. Same posture and
//! same reasoning as `render_page.rs`: the unit tests cover the pure
//! functions, and what *this* file protects is the part a batch script
//! depends on and a refactor cannot see.
//!
//! ## What is actually at stake here, stated once
//!
//! A DXF at the wrong scale **opens without complaint and is wrong**.
//! Nothing in the file says it is five times real size; the operator finds
//! out at the cutting table. So the assertions below are weighted toward
//! what must NOT happen — no file written when the scale is unresolved, no
//! page silently inheriting another page's calibration, no disclosure
//! repeated until it is scrolled past.
//!
//! ## Why the fixtures are built inline
//!
//! Every PDF here is assembled byte-by-byte below and written to a temp
//! file, for the two reasons `render_page.rs` records: `docs/LEGAL.md` §5
//! (synthetic or rights-cleared only — generating the bytes makes
//! provenance a non-question), and legibility (the structure under test is
//! visible at the call site rather than hidden in a binary blob).
//!
//! ## What these tests deliberately do NOT cover
//!
//! The *derived* scale paths — Calibrated and Conflicting — need ce
//! dimensions in a `/PieceInfo` sidecar, which is authored through
//! `dimension-add` / `group-set-scale` rather than expressible in
//! hand-written bytes. Those paths are covered where the authoring already
//! exists: `crates/pdfcer-core/tests/dxf_scale.rs` pins the inference
//! itself (including the page-scoping defect), and the checked-in
//! `fixtures/synthetic/dimension/` files exercise the CLI end to end. What
//! is left for here is everything that does NOT need a calibrated
//! document: the page-selection surface, the file naming, the
//! all-or-nothing posture, and the uncalibrated disclosure's gating.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

// ---------------------------------------------------------------------------
// Fixture construction
// ---------------------------------------------------------------------------

/// A document of `pages` pages, each drawing one horizontal stroked line
/// at a different height so the pages are distinguishable in the output.
///
/// One line per page means exactly one `LWPOLYLINE`, so `entities=1` in
/// the result line is an assertion about the writer having run on THAT
/// page rather than a coincidence of an empty page producing nothing.
fn multipage_pdf(pages: usize) -> Vec<u8> {
    let mut bodies: Vec<String> = vec!["<< /Type /Catalog /Pages 2 0 R >>".to_owned(), {
        let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 3 + i)).collect();
        format!(
            "<< /Type /Pages /Kids [{}] /Count {pages} >>",
            kids.join(" ")
        )
    }];
    for i in 0..pages {
        bodies.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> \
             /Contents {} 0 R >>",
            3 + pages + i
        ));
    }
    for i in 0..pages {
        let content = format!("1 w 50 {} m 350 {} l S", 50 + i * 10, 50 + i * 10);
        bodies.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }

    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

// ---------------------------------------------------------------------------
// Temp-directory scaffolding (same shape as `render_page.rs`, same reason:
// ~30 lines beats a dependency that costs a licence classification and an
// attribution entry — `docs/LEGAL.md` §6)
// ---------------------------------------------------------------------------

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let path = std::env::temp_dir().join(format!(
            "pdfcer-dxf-{tag}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("could not create temp dir");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.join(name);
        std::fs::write(&p, bytes).expect("could not write fixture");
        p
    }

    /// Every entry's file name, sorted — what the operator sees in a file
    /// manager, which is the thing the zero-padding exists to make right.
    fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.0)
            .expect("could not read temp dir")
            .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned()))
            .filter(|n| n.ends_with(".dxf"))
            .collect();
        v.sort();
        v
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("could not spawn pdfcer")
}

fn code(out: &Output) -> i32 {
    out.status
        .code()
        .expect("pdfcer terminated without an exit code (signal?)")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout must be valid UTF-8")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("stderr must be valid UTF-8")
}

// ---------------------------------------------------------------------------
// Multi-page mode
// ---------------------------------------------------------------------------

/// **`--pages all` writes one DXF per page, zero-padded to sort.**
///
/// Twelve pages, because eleven is where naive naming breaks: `_p1`,
/// `_p10`, `_p11`, `_p12`, `_p2` … is what a file manager shows without
/// padding, and an operator feeding a folder to a CAM post-processor in
/// name order would cut them in that order.
#[test]
fn every_selected_page_becomes_its_own_zero_padded_file() {
    let dir = TempDir::new("multi");
    let pdf = dir.write("sheet.pdf", &multipage_pdf(12));
    let out_dir = dir.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let out = run(&[
        "export-dxf",
        pdf.to_str().unwrap(),
        "--pages",
        "all",
        "--output-dir",
        out_dir.to_str().unwrap(),
        "--scale",
        "1",
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));

    let mut names: Vec<String> = std::fs::read_dir(&out_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        (1..=12)
            .map(|n| format!("sheet_p{n:02}.dxf"))
            .collect::<Vec<_>>(),
        "twelve files, padded to two digits so name order IS page order"
    );

    // One machine-readable result line per page, each naming its own page.
    let printed = stdout(&out);
    let lines: Vec<&str> = printed.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 12, "one result line per page");
    assert!(
        lines[0].contains(" page 1 ->") && lines[11].contains(" page 12 ->"),
        "each line names its own page: {lines:?}"
    );
    assert!(
        lines.iter().all(|l| l.contains("entities=1")),
        "every page drew its one line: {lines:?}"
    );
}

/// **The padding width comes from the RUN, not from the document.**
///
/// Exporting pages 8-10 of a twelve-page file pads to two digits, because
/// 10 is the widest number in the run. Padding to the document's page
/// count would give `_p08` for a three-file run, which sorts fine and
/// misnames the page.
#[test]
fn the_padding_width_is_the_widest_page_in_the_run() {
    let dir = TempDir::new("width");
    let pdf = dir.write("sheet.pdf", &multipage_pdf(12));
    let out_dir = dir.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let out = run(&[
        "export-dxf",
        pdf.to_str().unwrap(),
        "--pages",
        "8-10",
        "--output-dir",
        out_dir.to_str().unwrap(),
        "--scale",
        "1",
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));

    let mut names: Vec<String> = std::fs::read_dir(&out_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "sheet_p08.dxf".to_owned(),
            "sheet_p09.dxf".to_owned(),
            "sheet_p10.dxf".to_owned()
        ]
    );
}

/// **A page past the end of the document refuses the WHOLE run.**
///
/// `parse_pages`'s own contract, reaching this command: a batch script
/// that asks for pages 1-50 of a 30-page file has made a mistake, and
/// silently handing back 30 files is how that mistake ships to a thousand
/// documents. Nothing is written — not even the pages that did exist.
#[test]
fn an_out_of_range_page_refuses_the_run_and_writes_nothing() {
    let dir = TempDir::new("range");
    let pdf = dir.write("sheet.pdf", &multipage_pdf(3));
    let out_dir = dir.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let out = run(&[
        "export-dxf",
        pdf.to_str().unwrap(),
        "--pages",
        "1-5",
        "--output-dir",
        out_dir.to_str().unwrap(),
        "--scale",
        "1",
    ]);
    assert_ne!(code(&out), 0, "an impossible page list must fail");
    assert_eq!(
        std::fs::read_dir(&out_dir).unwrap().count(),
        0,
        "all-or-nothing: pages 1-3 exist and must NOT have been written"
    );
}

// ---------------------------------------------------------------------------
// The destination flags
// ---------------------------------------------------------------------------

/// **Neither destination flag is an error with a message naming both.**
///
/// Clap cannot express "one of these two, depending on the other flag", so
/// this is checked in the command — and the message has to name the flag
/// the operator actually wants rather than say "required argument
/// missing".
#[test]
fn no_destination_is_refused_with_a_message_naming_both_flags() {
    let dir = TempDir::new("nodest");
    let pdf = dir.write("sheet.pdf", &multipage_pdf(1));

    let out = run(&["export-dxf", pdf.to_str().unwrap()]);
    assert_ne!(code(&out), 0);
    let err = stderr(&out);
    assert!(err.contains("--output"), "must name --output: {err}");
    assert!(
        err.contains("--output-dir"),
        "must name --output-dir: {err}"
    );
}

/// **`--pages` with a single-file `--output` is refused rather than
/// overwriting one file N times.**
///
/// The silently-plausible failure this rules out: N pages exported into
/// one path leaves the LAST page's geometry in a file the operator
/// believes holds the first.
#[test]
fn pages_with_a_single_output_file_is_refused() {
    let dir = TempDir::new("pagesout");
    let pdf = dir.write("sheet.pdf", &multipage_pdf(3));
    let target = dir.join("one.dxf");

    let out = run(&[
        "export-dxf",
        pdf.to_str().unwrap(),
        "--pages",
        "all",
        "-o",
        target.to_str().unwrap(),
    ]);
    assert_ne!(code(&out), 0);
    assert!(!target.exists(), "nothing may be written");
    assert!(
        stderr(&out).contains("--output-dir"),
        "the message must point at the flag that works: {}",
        stderr(&out)
    );
}

/// **`--page` and `--pages` are mutually exclusive, at the clap layer.**
///
/// Exit 2, clap's usage-error code, distinct from the command's own
/// runtime refusals — which is the distinction a script's error handling
/// depends on.
#[test]
fn page_and_pages_together_is_a_usage_error() {
    let dir = TempDir::new("both");
    let pdf = dir.write("sheet.pdf", &multipage_pdf(2));
    let out_dir = dir.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let out = run(&[
        "export-dxf",
        pdf.to_str().unwrap(),
        "--page",
        "1",
        "--pages",
        "all",
        "--output-dir",
        out_dir.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 2, "clap usage error, not a runtime refusal");
}

/// **Single-page mode is untouched by all of the above.**
///
/// The regression guard on the flag surface that already shipped: adding
/// two flags must not have changed what `--page N -o file.dxf` does.
#[test]
fn single_page_mode_still_writes_one_named_file() {
    let dir = TempDir::new("single");
    let pdf = dir.write("sheet.pdf", &multipage_pdf(3));
    let target = dir.join("just-page-two.dxf");

    let out = run(&[
        "export-dxf",
        pdf.to_str().unwrap(),
        "--page",
        "2",
        "-o",
        target.to_str().unwrap(),
        "--scale",
        "1",
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(target.exists(), "the named file, at the name given");
    assert_eq!(dir.names(), vec!["just-page-two.dxf".to_owned()]);
    assert!(
        stdout(&out).contains(" page 2 ->"),
        "the result line names the page asked for: {}",
        stdout(&out)
    );
}

// ---------------------------------------------------------------------------
// The uncalibrated disclosure, and its two gates
// ---------------------------------------------------------------------------

/// **The paper-scale warning fires ONCE for a run, not once per page.**
///
/// Twelve near-identical paragraphs is a disclosure an operator scrolls
/// past, which is the same learned-past failure the wording was chosen to
/// avoid, arriving through volume instead. The per-page machine-readable
/// stdout line already carries each page's own counts.
#[test]
fn the_paper_scale_warning_is_emitted_once_for_the_whole_run() {
    let dir = TempDir::new("once");
    let pdf = dir.write("sheet.pdf", &multipage_pdf(12));
    let out_dir = dir.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let out = run(&[
        "export-dxf",
        pdf.to_str().unwrap(),
        "--pages",
        "all",
        "--output-dir",
        out_dir.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert_eq!(
        err.matches("PAPER scale").count(),
        1,
        "exactly one paper-scale disclosure for twelve pages: {err}"
    );
    assert!(
        err.contains("these pages"),
        "and it says pages, plural, because twelve were exported: {err}"
    );
}

/// **★ An EXPLICIT `--scale 1` is not lectured at.**
///
/// The defect this test was written after. The warning was gated on the
/// pages being uncalibrated and the scale being 1, but not on who chose
/// the 1 — so `--scale 1` printed *"pdfcer does not know what scale the
/// drawing is at … pass --scale 2 for 1:2, and so on"*, instructing the
/// operator to do the thing they had just done.
///
/// It is the same objection as the uncalibrated gate itself, from the
/// other side: an explicit `--scale 1` is the operator answering, exactly
/// as an explicit 1:1 calibration is.
#[test]
fn an_explicit_scale_of_one_is_not_warned_about() {
    let dir = TempDir::new("explicit");
    let pdf = dir.write("sheet.pdf", &multipage_pdf(2));
    let out_dir = dir.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let out = run(&[
        "export-dxf",
        pdf.to_str().unwrap(),
        "--pages",
        "all",
        "--output-dir",
        out_dir.to_str().unwrap(),
        "--scale",
        "1",
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(
        !stderr(&out).contains("PAPER scale"),
        "the operator said 1; telling them pdfcer does not know the scale is absurd: {}",
        stderr(&out)
    );
}

/// **A rejected scale writes nothing, in either mode.**
///
/// Zero collapses the drawing to a point and a negative mirrors it, and
/// both produce a DXF that opens successfully and is wrong.
#[test]
fn a_non_positive_scale_is_refused_before_anything_is_written() {
    let dir = TempDir::new("badscale");
    let pdf = dir.write("sheet.pdf", &multipage_pdf(2));
    let out_dir = dir.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    for bad in ["0", "-2"] {
        let out = run(&[
            "export-dxf",
            pdf.to_str().unwrap(),
            "--pages",
            "all",
            "--output-dir",
            out_dir.to_str().unwrap(),
            "--scale",
            bad,
        ]);
        assert_ne!(code(&out), 0, "--scale {bad} must be refused");
        assert_eq!(
            std::fs::read_dir(&out_dir).unwrap().count(),
            0,
            "--scale {bad} wrote a file"
        );
    }
}

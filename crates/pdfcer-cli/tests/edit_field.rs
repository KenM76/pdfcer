//! # `pdfcer edit-field` / `edit-widget` integration tests (`Pass 134.0`)
//!
//! Black-box: they spawn the real binary and assert on its process contract.
//! Every fixture is assembled inline (docs/LEGAL.md §5).
//!
//! What these defend, beyond "the flag is wired":
//!
//! - the standard's producer gates are checked against the RESULTING field,
//!   so an edit that never mentions `comb` can still be refused for breaking
//!   comb's precondition;
//! - a stored value that no longer fits is DISCLOSED and not repaired —
//!   Acrobat does the same edits silently;
//! - and a `--rect` that changes the extent rebuilds the appearance while one
//!   that merely moves does not, which is the difference between a field that
//!   gains room for text and one whose text is stretched.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

/// Path to the freshly built `pdfcer` binary. Cargo sets this for
/// integration tests, so the test always exercises the binary produced by
/// the same build — never a stale one on `PATH`.
const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

// ---------------------------------------------------------------------------
// Fixture construction
// ---------------------------------------------------------------------------

/// Assemble a syntactically complete single-generation PDF from a list of
/// `(object number, body)` pairs, appending a classic cross-reference
/// table and a trailer that names `1 0 R` as the catalog.
///
/// The layout follows §7.5: header, body, `xref` section with one
/// subsection covering objects `0..=n`, `trailer`, `startxref`, `%%EOF`.
/// Offsets are recorded as each object is emitted, so the table is
/// correct by construction rather than by hand-counting — which matters,
/// because `pdfcer-core` is strict: a wrong offset is a load failure, not
/// a warning.
///
/// Free entry `0` is emitted as the spec's mandatory
/// `0000000000 65535 f` head-of-free-list. Entries are exactly 20 bytes
/// each including the `\r\n` terminator, as §7.5.4 requires.
fn build_pdf(objects: &[(u32, String)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.4\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let size = objects.len() + 1; // +1 for the free object 0
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for num in 1..=objects.len() as u32 {
        let (_, off) = offsets
            .iter()
            .find(|(n, _)| *n == num)
            .expect("object numbers must be 1..=n and contiguous");
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// A document with `contents.len()` pages, page *i* drawing
/// `contents[i]`, all sharing a 200x100 MediaBox.
///
/// The non-square box is on purpose: it makes the `WxH` half of the
/// stdout line assert something real. A square page would pass even if
/// width and height were transposed somewhere in the geometry chain.
fn multipage_pdf(contents: &[&str]) -> Vec<u8> {
    // Object numbering: 1 = catalog, 2 = page-tree root,
    // then per page i: page dict at 3+2i, content stream at 4+2i.
    let kids: Vec<String> = (0..contents.len())
        .map(|i| format!("{} 0 R", 3 + 2 * i))
        .collect();
    let mut objects: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [{}] /Count {} /MediaBox [0 0 200 100] \
                 /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 \
                 /BaseFont /Helvetica >> >> >> >>",
                kids.join(" "),
                contents.len()
            ),
        ),
    ];
    for (i, content) in contents.iter().enumerate() {
        let page_num = 3 + 2 * i as u32;
        let stream_num = page_num + 1;
        objects.push((
            page_num,
            format!("<< /Type /Page /Parent 2 0 R /Contents {stream_num} 0 R >>"),
        ));
        objects.push((
            stream_num,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ));
    }
    build_pdf(&objects)
}

// ---------------------------------------------------------------------------
// Temp-directory scaffolding
// ---------------------------------------------------------------------------

/// A uniquely named directory under the system temp dir, removed on drop.
///
/// Uniqueness is process id + nanosecond clock + a per-process counter:
/// the pid separates concurrent `cargo test` invocations, the counter
/// separates tests within one process (Rust runs them on parallel
/// threads, so two could otherwise read the same clock tick), and the
/// clock separates sequential runs that reuse a pid.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let path = std::env::temp_dir().join(format!(
            "pdfcer-test-{tag}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("could not create temp dir");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Write `bytes` to `name` inside this directory and return the path.
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.join(name);
        std::fs::write(&p, bytes).expect("could not write fixture");
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Best-effort: a failure here must not mask the test's own
        // failure, so the result is deliberately discarded.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---------------------------------------------------------------------------
// Process helpers
// ---------------------------------------------------------------------------

/// Run `pdfcer` with `args` and capture the whole process outcome.
fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("could not spawn pdfcer")
}

/// Exit code as `u8`, matching the [`exit`] table's own type. A process
/// killed by a signal (no code) fails the test loudly rather than
/// silently comparing against a default.
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
// Tests
// ---------------------------------------------------------------------------

/// A field with a value, ready to be edited. Returns the PDF path.
fn field_to_edit(dir: &TempDir, value: &str) -> PathBuf {
    let src = dir.write("blank.pdf", &multipage_pdf(&["0 0 0 rg 0 0 10 10 re f"]));
    let out = dir.join("withfield.pdf");
    let r = run(&[
        "add-text-field",
        src.to_str().unwrap(),
        "--name",
        "Customer",
        "--page",
        "1",
        "--rect",
        "10,10,110,34",
        "--no-tooltip",
        "--value",
        value,
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code(&r), 0, "stderr: {}", stderr(&r));
    out
}

/// ★ The headline of `Pass 134.0`: a property set at placing time can be
/// changed afterwards, without deleting and re-placing the field.
#[test]
fn a_fields_properties_can_be_changed_after_it_is_placed() {
    let dir = TempDir::new("edit-field");
    let pdf = field_to_edit(&dir, "Hello");
    let out = dir.join("edited.pdf");

    let r = run(&[
        "edit-field",
        pdf.to_str().unwrap(),
        "--name",
        "Customer",
        "--required",
        "true",
        "--read-only",
        "true",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code(&r), 0, "stderr: {}", stderr(&r));
    // Bits 1 and 2 of Table 221: ReadOnly | Required.
    assert!(
        stdout(&r).contains("flags=0x0->0x3"),
        "the flag word before and after is the machine-readable half: {}",
        stdout(&r)
    );

    let listed = run(&["list-fields", out.to_str().unwrap()]);
    assert!(
        stdout(&listed).contains("readonly=1"),
        "the change must survive the save and read back: {}",
        stdout(&listed)
    );
}

/// ★ Rule 4, and the case Acrobat performs SILENTLY: shortening the length
/// limit below the stored value leaves the field over its own limit. pdfcer
/// does not truncate the operator's data, and says so.
#[test]
fn shortening_the_length_limit_discloses_and_does_not_truncate() {
    let dir = TempDir::new("edit-maxlen");
    let pdf = field_to_edit(&dir, "Hello");
    let out = dir.join("shorter.pdf");

    let r = run(&[
        "edit-field",
        pdf.to_str().unwrap(),
        "--name",
        "Customer",
        "--max-len",
        "3",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code(&r), 0, "a legitimate authoring act, not a failure");
    assert!(
        stdout(&r).contains("value_fits=0"),
        "machine-readable: {}",
        stdout(&r)
    );
    assert!(
        stderr(&r).contains("did NOT truncate"),
        "the operator must be told, in words: {}",
        stderr(&r)
    );
    let listed = run(&["list-fields", out.to_str().unwrap()]);
    assert!(
        stdout(&listed).contains("\"Hello\""),
        "the value must still be there: {}",
        stdout(&listed)
    );
}

/// The gate is checked against the RESULT. This request never mentions comb;
/// the FILE says the field is comb, and clearing `/MaxLen` breaks Table 228.
#[test]
fn clearing_the_length_limit_on_a_comb_field_is_refused() {
    let dir = TempDir::new("edit-comb");
    let src = dir.write("blank.pdf", &multipage_pdf(&["0 0 0 rg 0 0 10 10 re f"]));
    let combed = dir.join("comb.pdf");
    let r = run(&[
        "add-text-field",
        src.to_str().unwrap(),
        "--name",
        "Serial",
        "--page",
        "1",
        "--rect",
        "10,10,110,34",
        "--no-tooltip",
        "--comb",
        "--max-len",
        "10",
        "-o",
        combed.to_str().unwrap(),
    ]);
    assert_eq!(code(&r), 0, "stderr: {}", stderr(&r));

    let r = run(&[
        "edit-field",
        combed.to_str().unwrap(),
        "--name",
        "Serial",
        "--max-len",
        "0",
        "-o",
        dir.join("broken.pdf").to_str().unwrap(),
    ]);
    assert_ne!(
        code(&r),
        0,
        "a comb field with no /MaxLen has no defined rendering"
    );
    assert!(
        stderr(&r).to_lowercase().contains("maxlen"),
        "the refusal must name the precondition: {}",
        stderr(&r)
    );
}

/// A property that belongs to another field type is refused BY NAME, not
/// ignored: `/Ff` is one shared word whose bits mean different things per
/// type, so a mis-typed edit does something else rather than nothing.
#[test]
fn a_text_property_on_a_check_box_is_refused_by_name() {
    let dir = TempDir::new("edit-mismatch");
    let src = dir.write("blank.pdf", &multipage_pdf(&["0 0 0 rg 0 0 10 10 re f"]));
    let boxed = dir.join("box.pdf");
    let r = run(&[
        "add-check-box",
        src.to_str().unwrap(),
        "--name",
        "Agree",
        "--page",
        "1",
        "--rect",
        "10,10,30,30",
        "--no-tooltip",
        "-o",
        boxed.to_str().unwrap(),
    ]);
    assert_eq!(code(&r), 0, "stderr: {}", stderr(&r));

    let r = run(&[
        "edit-field",
        boxed.to_str().unwrap(),
        "--name",
        "Agree",
        "--multiline",
        "true",
        "-o",
        dir.join("no.pdf").to_str().unwrap(),
    ]);
    assert_ne!(code(&r), 0);
    assert!(
        stderr(&r).contains("check box"),
        "the error names the type in the OPERATOR's words: {}",
        stderr(&r)
    );
}

/// ★ The geometry distinction the whole widget path turns on: replacing the
/// rectangle with the same extent is a MOVE (no rebuild), and with a
/// different extent is a RESIZE (rebuild, because §12.5.5 would otherwise
/// scale the old artwork into the new box).
#[test]
fn a_resize_rebuilds_the_appearance_and_a_move_does_not() {
    let dir = TempDir::new("edit-widget");
    let pdf = field_to_edit(&dir, "Hello");

    let resized = dir.join("resized.pdf");
    let r = run(&[
        "edit-widget",
        pdf.to_str().unwrap(),
        "--name",
        "Customer",
        "--rect",
        "10,10,310,58",
        "-o",
        resized.to_str().unwrap(),
    ]);
    assert_eq!(code(&r), 0, "stderr: {}", stderr(&r));
    assert!(
        stdout(&r).contains("resized=1") && stdout(&r).contains("regenerated=1"),
        "a changed extent must rebuild: {}",
        stdout(&r)
    );

    let moved = dir.join("moved.pdf");
    let r = run(&[
        "edit-widget",
        resized.to_str().unwrap(),
        "--name",
        "Customer",
        "--rect",
        "50,50,350,98",
        "-o",
        moved.to_str().unwrap(),
    ]);
    assert_eq!(code(&r), 0, "stderr: {}", stderr(&r));
    assert!(
        stdout(&r).contains("resized=0") && stdout(&r).contains("regenerated=0"),
        "same width and height is a translation, and §12.5.5 makes that exact \
         for free — rebuilding would rewrite a stream for nothing: {}",
        stdout(&r)
    );
}

/// A widget resized to nothing is refused: it would exist, accept a value,
/// and never be visible or clickable.
#[test]
fn a_widget_resized_to_no_area_is_refused() {
    let dir = TempDir::new("edit-degenerate");
    let pdf = field_to_edit(&dir, "Hello");
    let r = run(&[
        "edit-widget",
        pdf.to_str().unwrap(),
        "--name",
        "Customer",
        "--rect",
        "10,10,10,10",
        "-o",
        dir.join("no.pdf").to_str().unwrap(),
    ]);
    assert_ne!(code(&r), 0);
    assert!(stderr(&r).contains("no area"), "{}", stderr(&r));
}

/// A malformed `--rect` is a clear refusal rather than a silently ignored
/// flag — the failure mode that would otherwise save an unchanged file and
/// report success.
#[test]
fn a_malformed_rect_is_refused_rather_than_ignored() {
    let dir = TempDir::new("edit-badrect");
    let pdf = field_to_edit(&dir, "Hello");
    let r = run(&[
        "edit-widget",
        pdf.to_str().unwrap(),
        "--name",
        "Customer",
        "--rect",
        "10,10,110",
        "-o",
        dir.join("no.pdf").to_str().unwrap(),
    ]);
    assert_ne!(code(&r), 0);
    assert!(
        stderr(&r).contains("four comma-separated"),
        "{}",
        stderr(&r)
    );
}

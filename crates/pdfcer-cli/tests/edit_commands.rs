//! # `pdfcer set-info` / `rotate-page` integration tests
//!
//! Black-box tests against the **real binary**: exit code, the stable
//! stdout result line, the stderr disclosures, and the bytes of the file
//! it wrote. The unit tests in `main.rs` cover the pure mapping
//! functions and `pdfcer-core`'s own tests cover the command log; what
//! *this* file protects is the part a script depends on and a refactor
//! cannot see.
//!
//! ## Why the same key test appears here as well as in pdfcer-core
//!
//! `edit → undo → save is byte-identical` is verified in
//! `pdfcer-core/tests/edit_undo.rs` at the API level. It is verified
//! **again** here, through `--verify-undo`, because the two prove
//! different things: the core test proves the mechanism is correct, and
//! this one proves the mechanism is what the CLI actually invokes. A
//! front end that bypassed `EditSession` and mutated a `Document`
//! directly would pass every core test and fail here — which is exactly
//! the shape of regression the single-mutation-path rule exists to
//! prevent.
//!
//! Fixtures are synthesized inline, per `docs/LEGAL.md` §5 (no
//! checked-in real-world PDFs) and for legibility: the exact structure
//! under test is visible at the call site.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A classic §7.5.4 file with three pages, optionally an `/Info`
/// dictionary, optionally an `/ID`.
///
/// Three pages so the "only the edited page changed" assertion has
/// something to be false about.
fn pdf(info: bool, id: bool) -> Vec<u8> {
    let mut objects: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>".to_owned(),
        ),
    ];
    for num in [3u32, 4, 5] {
        objects.push((
            num,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_owned(),
        ));
    }
    if info {
        objects.push((
            6,
            "<< /Producer (Original) /Title (Old title) >>".to_owned(),
        ));
    }

    let mut buf = b"%PDF-1.4\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let max = objects.iter().map(|(n, _)| *n).max().unwrap();
    let xref_at = buf.len();
    buf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max + 1).as_bytes());
    for num in 1..=max {
        let (_, off) = offsets.iter().find(|(n, _)| *n == num).unwrap();
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    let info_key = if info { " /Info 6 0 R" } else { "" };
    let id_key = if id { " /ID [<0102> <0304>]" } else { "" };
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R{info_key}{id_key} >>\nstartxref\n{xref_at}\n%%EOF\n",
            max + 1
        )
        .as_bytes(),
    );
    buf
}

// ---------------------------------------------------------------------------
// Scaffolding (see render_page.rs for why there is no `tempfile` dep)
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
            "pdfcer-edit-{tag}-{}-{}-{nanos}",
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

fn code(out: &Output) -> u8 {
    u8::try_from(out.status.code().expect("process was killed by a signal")).unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout must be valid UTF-8")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Parse the metrics half of a result line into `(key, value)` pairs,
/// exactly the way the documented contract says a script should.
fn metrics(line: &str) -> Vec<(String, u64)> {
    line.split("; ")
        .nth(1)
        .expect("result line has no metrics half")
        .trim_end()
        .split(' ')
        .map(|pair| {
            let (k, v) = pair.split_once('=').expect("metric is not key=value");
            (
                k.to_owned(),
                v.parse().expect("metric value is not an integer"),
            )
        })
        .collect()
}

fn metric(line: &str, key: &str) -> u64 {
    metrics(line)
        .into_iter()
        .find(|(k, _)| k == key)
        .unwrap_or_else(|| panic!("no metric named {key} in: {line}"))
        .1
}

// ---------------------------------------------------------------------------
// rotate-page
// ---------------------------------------------------------------------------

#[test]
fn rotate_page_appends_a_revision_touching_exactly_one_object() {
    let dir = TempDir::new("rot");
    let input = dir.write("in.pdf", &pdf(true, true));
    let output = dir.join("out.pdf");

    let out = run(&[
        "rotate-page",
        input.to_str().unwrap(),
        "--page",
        "2",
        "--degrees",
        "90",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    let line = stdout(&out);
    assert!(line.starts_with("rotate-page "), "{line}");
    assert!(line.contains("mode=incremental"), "{line}");
    assert_eq!(metric(&line, "rotate"), 90);
    assert_eq!(metric(&line, "changed"), 1, "only the page object");
    assert_eq!(metric(&line, "objects"), 1);
    assert_eq!(metric(&line, "verbatim"), 0);
    assert_eq!(metric(&line, "promoted"), 0);
    assert!(metric(&line, "appended") > 0);

    // The minimal-diff property, checked on the bytes: the whole input
    // is a prefix of the output.
    let base = std::fs::read(&input).unwrap();
    let saved = std::fs::read(&output).unwrap();
    assert!(saved.starts_with(&base), "prior bytes were perturbed");

    // ...and only the named page moved.
    let doc = pdfcer_core::document::Document::from_bytes(saved).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    assert_eq!(pages[0].rotate, 0);
    assert_eq!(pages[1].rotate, 90);
    assert_eq!(pages[2].rotate, 0);
}

#[test]
fn rotate_page_relative_accumulates() {
    let dir = TempDir::new("rel");
    let input = dir.write("in.pdf", &pdf(true, true));
    let once = dir.join("once.pdf");
    let twice = dir.join("twice.pdf");

    for (src, dst) in [(&input, &once), (&once, &twice)] {
        let out = run(&[
            "rotate-page",
            src.to_str().unwrap(),
            "--page",
            "1",
            "--degrees",
            "90",
            "--relative",
            "-o",
            dst.to_str().unwrap(),
        ]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
    }
    let line = stdout(&run(&[
        "round-trip",
        twice.to_str().unwrap(),
        "--no-raster",
    ]));
    assert!(line.contains("identical=1"), "{line}");

    let doc = pdfcer_core::document::Document::from_bytes(std::fs::read(&twice).unwrap()).unwrap();
    assert_eq!(pdfcer_core::page_tree::pages(&doc).unwrap()[0].rotate, 180);
}

#[test]
fn rotate_page_verify_undo_reports_byte_identity() {
    // The CLI-level form of the Pass's key test. `undo_identical=1`
    // means: had the operator undone this edit, saving would have
    // reproduced their input exactly.
    let dir = TempDir::new("undo");
    let input = dir.write("in.pdf", &pdf(true, true));
    let output = dir.join("out.pdf");

    let out = run(&[
        "rotate-page",
        input.to_str().unwrap(),
        "--page",
        "1",
        "--degrees",
        "270",
        "-o",
        output.to_str().unwrap(),
        "--verify-undo",
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let line = stdout(&out);
    assert_eq!(metric(&line, "undo_verified"), 1);
    assert_eq!(metric(&line, "undo_identical"), 1);
}

#[test]
fn a_rotation_that_changes_nothing_writes_a_byte_copy() {
    // "Zero edits means zero bytes" survives contact with the CLI: the
    // output is the input, `appended=0`, and the operator is told why
    // rather than being left to wonder.
    let dir = TempDir::new("noop");
    let input = dir.write("in.pdf", &pdf(true, true));
    let output = dir.join("out.pdf");

    let out = run(&[
        "rotate-page",
        input.to_str().unwrap(),
        "--page",
        "1",
        "--degrees",
        "0",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let line = stdout(&out);
    assert_eq!(metric(&line, "changed"), 0);
    assert_eq!(metric(&line, "appended"), 0);
    assert!(stderr(&out).contains("nothing changed"), "{}", stderr(&out));
    assert_eq!(
        std::fs::read(&input).unwrap(),
        std::fs::read(&output).unwrap()
    );
}

#[test]
fn a_rotation_that_is_not_a_multiple_of_ninety_is_refused_by_name() {
    let dir = TempDir::new("bad-deg");
    let input = dir.write("in.pdf", &pdf(true, true));
    let out = run(&[
        "rotate-page",
        input.to_str().unwrap(),
        "--page",
        "1",
        "--degrees",
        "45",
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 9, "edit refusals get their own exit code");
    assert!(stderr(&out).contains("multiple of 90"), "{}", stderr(&out));
    assert!(
        !dir.join("out.pdf").exists(),
        "a refused edit writes nothing"
    );
}

#[test]
fn an_out_of_range_page_is_refused_with_the_real_page_count() {
    let dir = TempDir::new("range");
    let input = dir.write("in.pdf", &pdf(true, true));
    for page in ["0", "9"] {
        let out = run(&[
            "rotate-page",
            input.to_str().unwrap(),
            "--page",
            page,
            "--degrees",
            "90",
            "-o",
            dir.join("out.pdf").to_str().unwrap(),
        ]);
        assert_eq!(code(&out), 9, "page {page}: {}", stderr(&out));
    }
    let out = run(&[
        "rotate-page",
        input.to_str().unwrap(),
        "--page",
        "9",
        "--degrees",
        "90",
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert!(stderr(&out).contains("3 page(s)"), "{}", stderr(&out));
}

#[test]
fn negative_degrees_are_accepted_and_normalized() {
    // Table 30 constrains "a multiple of 90" and nothing else, so -90 is
    // a conforming request. It must reach the engine rather than being
    // eaten by the argument parser as an unknown flag.
    let dir = TempDir::new("neg");
    let input = dir.write("in.pdf", &pdf(true, true));
    let output = dir.join("out.pdf");
    let out = run(&[
        "rotate-page",
        input.to_str().unwrap(),
        "--page",
        "1",
        "--degrees",
        "-90",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(metric(&stdout(&out), "rotate"), 270);
}

// ---------------------------------------------------------------------------
// set-info
// ---------------------------------------------------------------------------

#[test]
fn set_info_edits_the_information_dictionary_only() {
    let dir = TempDir::new("info");
    let input = dir.write("in.pdf", &pdf(true, true));
    let output = dir.join("out.pdf");

    let out = run(&[
        "set-info",
        input.to_str().unwrap(),
        "--title",
        "Quarterly Report",
        "--author",
        "Ada Lovelace",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let line = stdout(&out);
    assert_eq!(metric(&line, "changed"), 1, "both fields, one object");
    assert_eq!(metric(&line, "info_created"), 0);

    let saved = std::fs::read(&output).unwrap();
    assert!(saved.starts_with(&std::fs::read(&input).unwrap()));
    let doc = pdfcer_core::document::Document::from_bytes(saved).unwrap();
    let info = doc.resolve(doc.trailer().get(b"Info").unwrap());
    let dict = info.as_dict().unwrap();
    let Some(pdfcer_core::object::Object::String(title)) = dict.get(b"Title") else {
        panic!("title missing");
    };
    assert_eq!(title, b"Quarterly Report");
    // R41: editing metadata does not stamp pdfcer's own producer.
    let Some(pdfcer_core::object::Object::String(producer)) = dict.get(b"Producer") else {
        panic!("producer missing");
    };
    assert_eq!(producer, b"Original");
}

#[test]
fn set_info_creates_a_dictionary_when_the_file_has_none_and_says_so() {
    let dir = TempDir::new("mkinfo");
    let input = dir.write("in.pdf", &pdf(false, true));
    let output = dir.join("out.pdf");

    let out = run(&[
        "set-info",
        input.to_str().unwrap(),
        "--subject",
        "Structural test",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(metric(&stdout(&out), "info_created"), 1);
    assert!(
        stderr(&out).contains("had no document information dictionary"),
        "creating an object must be disclosed: {}",
        stderr(&out)
    );

    let doc = pdfcer_core::document::Document::from_bytes(std::fs::read(&output).unwrap()).unwrap();
    let info = doc.trailer().get(b"Info").unwrap();
    assert!(info.as_reference().is_some(), "Table 15: shall be indirect");
    assert!(
        doc.resolve(info)
            .as_dict()
            .unwrap()
            .contains_key(b"Subject")
    );
}

#[test]
fn set_info_clear_removes_the_entry() {
    let dir = TempDir::new("clear");
    let input = dir.write("in.pdf", &pdf(true, true));
    let output = dir.join("out.pdf");

    let out = run(&[
        "set-info",
        input.to_str().unwrap(),
        "--clear",
        "title",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    let doc = pdfcer_core::document::Document::from_bytes(std::fs::read(&output).unwrap()).unwrap();
    let dict = doc
        .resolve(doc.trailer().get(b"Info").unwrap())
        .as_dict()
        .unwrap()
        .clone();
    assert!(!dict.contains_key(b"Title"));
    assert!(dict.contains_key(b"Producer"), "siblings survive");
}

#[test]
fn clear_wins_over_a_conflicting_set_for_the_same_field() {
    // Documented resolution, pinned: argument order is not something a
    // script author can see, so the rule must be in the contract.
    let dir = TempDir::new("conflict");
    let input = dir.write("in.pdf", &pdf(true, true));
    let output = dir.join("out.pdf");
    let out = run(&[
        "set-info",
        input.to_str().unwrap(),
        "--title",
        "Ignored",
        "--clear",
        "title",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let doc = pdfcer_core::document::Document::from_bytes(std::fs::read(&output).unwrap()).unwrap();
    assert!(
        !doc.resolve(doc.trailer().get(b"Info").unwrap())
            .as_dict()
            .unwrap()
            .contains_key(b"Title")
    );
}

#[test]
fn set_info_with_no_fields_is_refused_rather_than_silently_copying() {
    let dir = TempDir::new("nofields");
    let input = dir.write("in.pdf", &pdf(true, true));
    let out = run(&[
        "set-info",
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 9);
    assert!(stderr(&out).contains("no fields given"), "{}", stderr(&out));
}

#[test]
fn set_info_verify_undo_reports_byte_identity() {
    let dir = TempDir::new("info-undo");
    let input = dir.write("in.pdf", &pdf(true, true));
    let out = run(&[
        "set-info",
        input.to_str().unwrap(),
        "--keywords",
        "pdf, parity, writer",
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
        "--verify-undo",
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(metric(&stdout(&out), "undo_identical"), 1);
}

#[test]
fn non_ascii_metadata_survives_the_command_line() {
    // The UTF-16BE + BOM path (§7.9.2), driven end to end from an
    // argv string through the writer and back through the loader.
    let dir = TempDir::new("utf16");
    let input = dir.write("in.pdf", &pdf(true, true));
    let output = dir.join("out.pdf");
    let out = run(&[
        "set-info",
        input.to_str().unwrap(),
        "--title",
        "Été — 日本語",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    let doc = pdfcer_core::document::Document::from_bytes(std::fs::read(&output).unwrap()).unwrap();
    let dict = doc
        .resolve(doc.trailer().get(b"Info").unwrap())
        .as_dict()
        .unwrap()
        .clone();
    let Some(pdfcer_core::object::Object::String(bytes)) = dict.get(b"Title") else {
        panic!("title missing");
    };
    assert_eq!(
        pdfcer_core::edit::decode_text_string(bytes).text,
        "Été — 日本語"
    );
}

#[test]
fn a_full_rewrite_edit_produces_a_single_revision() {
    let dir = TempDir::new("full");
    let input = dir.write("in.pdf", &pdf(true, true));
    let output = dir.join("out.pdf");
    let out = run(&[
        "set-info",
        input.to_str().unwrap(),
        "--title",
        "Rewritten",
        "-o",
        output.to_str().unwrap(),
        "--mode",
        "full",
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains("mode=full"), "{}", stdout(&out));

    let saved = std::fs::read(&output).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&saved).matches("%%EOF").count(),
        1,
        "a full rewrite is one revision"
    );
    let doc = pdfcer_core::document::Document::from_bytes(saved).unwrap();
    let dict = doc
        .resolve(doc.trailer().get(b"Info").unwrap())
        .as_dict()
        .unwrap()
        .clone();
    let Some(pdfcer_core::object::Object::String(title)) = dict.get(b"Title") else {
        panic!("title missing");
    };
    assert_eq!(title, b"Rewritten");
}

// ---------------------------------------------------------------------------
// Contract-level checks
// ---------------------------------------------------------------------------

#[test]
fn creating_metadata_is_refused_when_size_hides_entries() {
    // The bug the `writer_roundtrip` fuzz target found: this file's
    // `/Size` suppresses real cross-reference entries, so creating an
    // `/Info` object would raise `/Size` and resurrect them. The
    // operator gets a named refusal and no output file — never a
    // plausible-looking document with objects they never touched.
    let dir = TempDir::new("hidden");
    // A bespoke fixture: one page reachable from the tree, plus an
    // object 4 that nothing references. Dropping `/Size` to 4 hides
    // object 4 and nothing else, so the document still opens and
    // renders — which is the whole point. A file whose *page* was
    // hidden would simply fail to load and would prove nothing about
    // the writer.
    let mut bytes = {
        let objects: [(u32, &str); 4] = [
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>",
            ),
            (4, "<< /Unreferenced (hidden by Size) >>"),
        ];
        let mut buf = b"%PDF-1.4\n".to_vec();
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        for (num, body) in objects {
            offsets.push((num, buf.len()));
            buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        let xref_at = buf.len();
        buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for num in 1..=4u32 {
            let (_, off) = offsets.iter().find(|(n, _)| *n == num).unwrap();
            buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        buf.extend_from_slice(
            format!(
                "trailer\n<< /Size 5 /Root 1 0 R /ID [<0102> <0304>] >>\n\
startxref\n{xref_at}\n%%EOF\n"
            )
            .as_bytes(),
        );
        buf
    };
    // Rewrite the trailer's `/Size` downward, leaving the table alone —
    // exactly the damaged shape real files exhibit.
    let needle = b"/Size ";
    let at = bytes
        .windows(needle.len())
        .rposition(|w| w == needle)
        .unwrap();
    let start = at + needle.len();
    let digits = bytes[start..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    bytes.splice(start..start + digits, b"4".to_vec());

    let input = dir.write("in.pdf", &bytes);
    let output = dir.join("out.pdf");
    let out = run(&[
        "set-info",
        input.to_str().unwrap(),
        "--title",
        "Nope",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 9, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("/Size"),
        "the refusal must name the cause: {}",
        stderr(&out)
    );
    assert!(!output.exists(), "a refused edit writes nothing");

    // ...but editing an object that already exists is still fine,
    // because it does not raise /Size.
    let out = run(&[
        "rotate-page",
        input.to_str().unwrap(),
        "--page",
        "1",
        "--degrees",
        "90",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(metric(&stdout(&out), "changed"), 1);
}

#[test]
fn a_missing_input_maps_to_the_io_exit_code() {
    let dir = TempDir::new("missing");
    let out = run(&[
        "rotate-page",
        dir.join("nope.pdf").to_str().unwrap(),
        "--page",
        "1",
        "--degrees",
        "90",
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 3);
}

#[test]
fn a_non_pdf_input_maps_to_the_not_a_pdf_exit_code() {
    let dir = TempDir::new("notpdf");
    let input = dir.write("in.pdf", b"this is not a PDF at all");
    let out = run(&[
        "set-info",
        input.to_str().unwrap(),
        "--title",
        "x",
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 4);
}

#[test]
fn both_result_lines_are_pure_ascii_and_single_line() {
    // The stdout contract: one LF-terminated, pure-ASCII, locale-
    // invariant line per successful invocation. A path with a space in
    // it must not break the metrics half, which is why the split is on
    // "; " rather than on whitespace.
    let dir = TempDir::new("contract");
    let input = dir.write("has space.pdf", &pdf(true, true));
    for args in [
        vec![
            "rotate-page",
            input.to_str().unwrap(),
            "--page",
            "1",
            "--degrees",
            "90",
        ],
        vec!["set-info", input.to_str().unwrap(), "--title", "T"],
    ] {
        let output = dir.join("out.pdf");
        let mut full = args.clone();
        full.push("-o");
        full.push(output.to_str().unwrap());
        let out = run(&full);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        let line = stdout(&out);
        assert!(line.is_ascii(), "result line is not ASCII: {line}");
        assert_eq!(line.lines().count(), 1, "{line}");
        assert!(line.ends_with('\n'));
        // Every metric parses as key=<non-negative integer>.
        assert!(!metrics(&line).is_empty());
    }
}

// ---------------------------------------------------------------------------
// Structural page operations (Pass 3.2)
// ---------------------------------------------------------------------------

/// The three-page fixture, written into a fresh directory.
fn three_pages(dir: &TempDir) -> PathBuf {
    dir.write("in.pdf", &pdf(true, true))
}

#[test]
fn rotate_turns_every_page_by_default() {
    // `rotate` was a stub through Pass 3.1, which redirected to
    // `rotate-page`. It is the real thing now: all pages unless `--pages`
    // says otherwise.
    let dir = TempDir::new("rotate-all");
    let input = three_pages(&dir);
    let output = dir.join("out.pdf");
    let out = run(&[
        "rotate",
        input.to_str().unwrap(),
        "--degrees",
        "90",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let line = stdout(&out);
    assert!(line.starts_with("rotate "), "{line}");
    assert!(line.contains("rotated=3"), "{line}");
    // The narrative half carries `signature=`, which is a NAME and
    // therefore must not be in the integer-only metrics half.
    assert!(line.contains("signature=none"), "{line}");
    for (key, _) in metrics(&line) {
        assert_ne!(key, "signature", "a name leaked into the metrics half");
    }
}

#[test]
fn rotate_honours_a_page_selection() {
    let dir = TempDir::new("rotate-some");
    let input = three_pages(&dir);
    let out = run(&[
        "rotate",
        input.to_str().unwrap(),
        "--degrees",
        "-90",
        "--pages",
        "1,3",
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains("rotated=2"), "{}", stdout(&out));
}

#[test]
fn delete_pages_reports_the_page_count_and_the_freed_objects() {
    let dir = TempDir::new("delete");
    let input = three_pages(&dir);
    let output = dir.join("out.pdf");
    let out = run(&[
        "delete-pages",
        input.to_str().unwrap(),
        "--pages",
        "2",
        "-o",
        output.to_str().unwrap(),
        "--verify-undo",
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let line = stdout(&out);
    assert!(line.contains("pages_removed=1"), "{line}");
    assert!(line.contains("deleted=1"), "{line}");
    assert!(line.contains("undo_identical=1"), "{line}");
    // The "delete is not redaction" disclosure is mandatory, not
    // conditional — an operator who deletes a page for confidentiality
    // reasons is wrong, and must be told so every time.
    assert!(stderr(&out).contains("not redaction"), "{}", stderr(&out));
}

#[test]
fn deleting_every_page_is_refused_with_the_edit_refused_code() {
    let dir = TempDir::new("delete-all");
    let input = three_pages(&dir);
    let out = run(&[
        "delete-pages",
        input.to_str().unwrap(),
        "--pages",
        "1-3",
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 9, "{}", stderr(&out));
    assert!(!dir.join("out.pdf").exists(), "a refusal writes nothing");
}

#[test]
fn a_page_number_past_the_end_is_refused_rather_than_clamped() {
    // A batch script asking for pages 1-50 of a 3-page file has made a
    // mistake; silently handing back 3 pages is how that mistake ships.
    let dir = TempDir::new("range");
    let input = three_pages(&dir);
    let out = run(&[
        "extract-pages",
        input.to_str().unwrap(),
        "--pages",
        "1-50",
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 9, "{}", stderr(&out));
    assert!(stderr(&out).contains("past the end"), "{}", stderr(&out));
}

#[test]
fn reorder_requires_a_complete_permutation() {
    let dir = TempDir::new("reorder-bad");
    let input = three_pages(&dir);
    let out = run(&[
        "reorder-pages",
        input.to_str().unwrap(),
        "--order",
        "2,1",
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 9, "{}", stderr(&out));
}

#[test]
fn reorder_accepts_a_descending_range_as_a_reversal() {
    let dir = TempDir::new("reorder");
    let input = three_pages(&dir);
    let out = run(&[
        "reorder-pages",
        input.to_str().unwrap(),
        "--order",
        "3-1",
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
        "--verify-undo",
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stdout(&out).contains("undo_identical=1"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn extract_merge_and_split_produce_loadable_files() {
    let dir = TempDir::new("producers");
    let input = three_pages(&dir);
    let extracted = dir.join("extract.pdf");
    let out = run(&[
        "extract-pages",
        input.to_str().unwrap(),
        "--pages",
        "3,1",
        "-o",
        extracted.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains("pages=2"), "{}", stdout(&out));

    let merged = dir.join("merged.pdf");
    let out = run(&[
        "merge",
        input.to_str().unwrap(),
        extracted.to_str().unwrap(),
        "-o",
        merged.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains("pages=5"), "{}", stdout(&out));

    let parts = dir.join("parts");
    let out = run(&[
        "split",
        merged.to_str().unwrap(),
        "--out-dir",
        parts.to_str().unwrap(),
        "--every",
        "2",
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains("parts=3"), "{}", stdout(&out));

    // Every produced file must load through `inspect`, which is the
    // cheapest end-to-end "is this a real PDF" the CLI has.
    for path in [&extracted, &merged] {
        let out = run(&["inspect", path.to_str().unwrap()]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
    }
}

#[test]
fn merge_needs_at_least_two_inputs() {
    let dir = TempDir::new("merge-one");
    let input = three_pages(&dir);
    let out = run(&[
        "merge",
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 9, "{}", stderr(&out));
}

#[test]
fn split_refuses_to_overwrite_without_force() {
    let dir = TempDir::new("split-clobber");
    let input = three_pages(&dir);
    let parts = dir.join("parts");
    let args = [
        "split",
        input.to_str().unwrap(),
        "--out-dir",
        parts.to_str().unwrap(),
        "--every",
        "1",
    ];
    assert_eq!(code(&run(&args)), 0);
    // A split that overwrites half a folder and then fails is worse than
    // one that refuses.
    let second = run(&args);
    assert_eq!(code(&second), 9, "{}", stderr(&second));
    assert!(stderr(&second).contains("--force"), "{}", stderr(&second));
}

#[test]
fn insert_pages_splices_a_second_document_in() {
    let dir = TempDir::new("insert");
    let input = three_pages(&dir);
    let source = dir.write("src.pdf", &pdf(false, true));
    let out = run(&[
        "insert-pages",
        input.to_str().unwrap(),
        "--source",
        source.to_str().unwrap(),
        "--source-pages",
        "1",
        "--after",
        "1",
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains("pages=4"), "{}", stdout(&out));
}

#[test]
fn every_page_op_result_line_is_one_ascii_line_of_integer_metrics() {
    // The stdout contract, applied to the new subcommands: one
    // LF-terminated pure-ASCII line, and every metrics-half pair is
    // `key=<non-negative integer>`.
    let dir = TempDir::new("contract");
    let input = three_pages(&dir);
    let cases: Vec<Vec<String>> = vec![
        vec![
            "delete-pages".into(),
            input.display().to_string(),
            "--pages".into(),
            "2".into(),
            "-o".into(),
            dir.join("a.pdf").display().to_string(),
        ],
        vec![
            "reorder-pages".into(),
            input.display().to_string(),
            "--order".into(),
            "3-1".into(),
            "-o".into(),
            dir.join("b.pdf").display().to_string(),
        ],
        vec![
            "extract-pages".into(),
            input.display().to_string(),
            "--pages".into(),
            "1".into(),
            "-o".into(),
            dir.join("c.pdf").display().to_string(),
        ],
    ];
    for case in cases {
        let args: Vec<&str> = case.iter().map(String::as_str).collect();
        let out = run(&args);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        let line = stdout(&out);
        assert!(line.is_ascii(), "result line is not ASCII: {line}");
        assert_eq!(line.lines().count(), 1, "{line}");
        assert!(line.ends_with('\n'));
        assert!(!metrics(&line).is_empty(), "{line}");
    }
}

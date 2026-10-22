//! `add-text-field` / `add-choice-field` `--font`, `--font-resource`,
//! `--font-size`, `--font-color`: the `/DA` stated at creation.
//!
//! Black-box against the built binary; the fixture is assembled inline
//! (docs/LEGAL.md §5). The `/DA` layout itself is pinned by the core tests;
//! these pin the wiring and the refusals.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED` — the file was readable and pdfcer declined the edit.
const EDIT_REFUSED: u8 = 9;

/// A minimal one-page classic-xref PDF with no form of any kind.
fn formless_pdf() -> Vec<u8> {
    let objects: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_owned(),
        ),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0usize; objects.len() + 1];
    for (num, body) in &objects {
        offsets[*num as usize] = out.len();
        out.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let startxref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for off in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{startxref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A self-deleting scratch directory (see `render_page.rs` for why this
/// project carries no `tempfile` dependency).
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let path = std::env::temp_dir().join(format!(
            "pdfcer-field-style-{tag}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("could not create temp dir");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// The scratch directory seeded with the formless fixture as `in.pdf`.
    fn seeded(tag: &str) -> (Self, PathBuf) {
        let dir = Self::new(tag);
        let input = dir.join("in.pdf");
        std::fs::write(&input, formless_pdf()).expect("could not write fixture");
        (dir, input)
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

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// `/DA` as the writer emits it: a hex string, since it holds newlines.
fn da(text: &str) -> String {
    let hex: String = text.bytes().map(|b| format!("{b:02X}")).collect();
    format!("/DA <{hex}>")
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// `add-text-field` on the formless fixture with `extra` flags.
fn add_text(
    dir: &TempDir,
    input: &Path,
    name: &str,
    rect: &str,
    extra: &[&str],
) -> (Output, PathBuf) {
    let out = dir.join(&format!("{name}-{}.pdf", extra.len()));
    let mut args = vec![
        "add-text-field",
        input.to_str().unwrap(),
        "--name",
        name,
        "--page",
        "1",
        "--rect",
        rect,
        "--no-tooltip",
        "--value",
        "Hello",
        "-o",
        out.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    (run(&args), out)
}

#[test]
fn add_text_field_writes_the_stated_da_and_its_font_resource() {
    let (dir, input) = TempDir::seeded("text");
    let (r, out) = add_text(
        &dir,
        &input,
        "t",
        "20,20,180,40",
        &[
            "--font",
            "courier",
            "--font-size",
            "18",
            "--font-color",
            "1,0,0",
        ],
    );
    assert_eq!(code(&r), 0, "stderr: {}", stderr(&r));
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        contains(
            &bytes,
            &da("/Cour 18 Tf
1 0 0 rg
")
        ),
        "no Courier /DA"
    );
    assert!(
        contains(&bytes, "/BaseFont /Courier"),
        "no Courier /DR font"
    );
}

#[test]
fn add_choice_field_writes_the_stated_da() {
    let (dir, input) = TempDir::seeded("choice");
    let out = dir.join("c.pdf");
    let r = run(&[
        "add-choice-field",
        input.to_str().unwrap(),
        "--name",
        "c",
        "--page",
        "1",
        "--rect",
        "20,20,180,40",
        "--option",
        "Apple",
        "--no-tooltip",
        "--font",
        "times-bold",
        "--font-size",
        "12",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code(&r), 0, "stderr: {}", stderr(&r));
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        contains(
            &bytes,
            &da("/TiBo 12 Tf
0 g
")
        ),
        "no Times-Bold /DA"
    );
    assert!(
        contains(&bytes, "/BaseFont /Times-Bold"),
        "no Times-Bold /DR font"
    );
}

#[test]
fn a_font_without_a_size_or_a_size_without_a_font_writes_nothing() {
    let (dir, input) = TempDir::seeded("partial");
    for extra in [&["--font", "courier"][..], &["--font-size", "12"][..]] {
        let (r, out) = add_text(&dir, &input, "t", "20,20,180,40", extra);
        assert_ne!(code(&r), 0, "{extra:?} was accepted");
        assert!(!out.exists(), "{extra:?} wrote a file");
    }
}

#[test]
fn an_absent_font_resource_is_refused_naming_what_is_there() {
    let (dir, input) = TempDir::seeded("absent");
    let (r, out) = add_text(
        &dir,
        &input,
        "t",
        "20,20,180,40",
        &["--font-resource", "F9", "--font-size", "10"],
    );
    assert_eq!(code(&r), EDIT_REFUSED, "stderr: {}", stderr(&r));
    assert!(stderr(&r).contains("F9"), "{}", stderr(&r));
    assert!(!out.exists());
}

#[test]
fn a_font_on_a_name_that_would_merge_is_refused() {
    let (dir, input) = TempDir::seeded("merge");
    let (r, first) = add_text(&dir, &input, "t", "20,20,180,40", &[]);
    assert_eq!(code(&r), 0, "stderr: {}", stderr(&r));
    let (r, out) = add_text(
        &dir,
        &first,
        "t",
        "20,60,180,80",
        &["--font", "courier", "--font-size", "9"],
    );
    assert_eq!(code(&r), EDIT_REFUSED, "stderr: {}", stderr(&r));
    assert!(stderr(&r).contains("edit-field"), "{}", stderr(&r));
    assert!(!out.exists());
}

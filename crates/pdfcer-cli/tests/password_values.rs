//! CLI tests for `password-values` and the `fill-field` note that a password
//! field's earlier value survives an incremental save (ISO 32000-1 §12.7.4.3
//! Table 228 bit 14; §7.5.6). The value itself must never reach stdout or
//! stderr.

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const SECRET: &str = "hunter2";

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "pdfcer-pwvalues-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("could not create temp dir");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A one-page form whose Password text field `pin` stores `/V (hunter2)`.
fn form_storing_password() -> Vec<u8> {
    form(false)
}

/// [`form_storing_password`]; `signed` adds a signature field whose `/V` is a
/// signature dictionary (ISO 32000-1 Table 252).
fn form(signed: bool) -> Vec<u8> {
    let fields = if signed { "[4 0 R 5 0 R]" } else { "[4 0 R]" };
    let mut objs = vec![
        format!("<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields {fields} >> >>"),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>".to_owned(),
        format!(
            "<< /Type /Annot /Subtype /Widget /FT /Tx /Ff 8192 /T (pin) \
             /Rect [10 10 100 30] /P 3 0 R /V ({SECRET}) >>"
        ),
    ];
    if signed {
        objs.push(
            "<< /FT /Sig /T (sig) /V << /Type /Sig /Filter /Adobe.PPKLite \
             /ByteRange [0 0 0 0] /Contents <00> >> >>"
                .to_owned(),
        );
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = out.len();
    let size = objs.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in offs {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    out
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("could not spawn pdfcer")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Runs `password-values` and returns its summary line.
fn summary(path: &std::path::Path) -> String {
    let out = run(&["password-values", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    assert!(!text(&out.stdout).contains(SECRET), "the value was printed");
    assert!(!text(&out.stderr).contains(SECRET), "the value was printed");
    text(&out.stdout)
        .lines()
        .find(|l| l.starts_with("password-values "))
        .expect("no summary line")
        .to_owned()
}

fn fill(input: &std::path::Path, output: &std::path::Path, extra: &[&str]) -> Output {
    let mut args = vec![
        "fill-field",
        input.to_str().unwrap(),
        "--set",
        "pin=new-pin",
        "-o",
        output.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    let out = run(&args);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    out
}

#[test]
fn a_stored_value_in_the_opened_revision_is_reported_as_latest() {
    let dir = TempDir::new("latest");
    let input = dir.join("in.pdf");
    std::fs::write(&input, form_storing_password()).unwrap();
    let out = run(&["password-values", input.to_str().unwrap()]);
    assert!(text(&out.stdout).contains("stored field=pin revision=0 latest=1"));
    assert!(summary(&input).ends_with("revisions=1 unreadable_revisions=0 latest=1 superseded=0"));
}

#[test]
fn an_incremental_fill_withholds_the_value_and_names_the_one_left_behind() {
    let dir = TempDir::new("incremental");
    let input = dir.join("in.pdf");
    let output = dir.join("out.pdf");
    std::fs::write(&input, form_storing_password()).unwrap();
    let out = fill(&input, &output, &[]);
    let err = text(&out.stderr);
    assert!(
        err.contains("earlier value of this password field is still in the file")
            && err.contains("--mode full"),
        "stderr: {err}"
    );
    assert!(!err.contains(SECRET));
    assert!(
        summary(&output).ends_with("revisions=2 unreadable_revisions=0 latest=0 superseded=1"),
        "{}",
        summary(&output)
    );
}

#[test]
fn a_full_rewrite_fill_leaves_no_revision_holding_a_value_and_says_nothing() {
    let dir = TempDir::new("full");
    let input = dir.join("in.pdf");
    let output = dir.join("out.pdf");
    std::fs::write(&input, form_storing_password()).unwrap();
    let out = fill(&input, &output, &["--mode", "full"]);
    assert!(!text(&out.stderr).contains("earlier value of this password field"));
    assert!(
        summary(&output).ends_with("revisions=1 unreadable_revisions=0 latest=0 superseded=0"),
        "{}",
        summary(&output)
    );
}

fn purge(input: &std::path::Path, output: &std::path::Path, extra: &[&str]) -> Output {
    let mut args = vec![
        "purge-password-values",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    let out = run(&args);
    assert!(!text(&out.stdout).contains(SECRET), "the value was printed");
    assert!(!text(&out.stderr).contains(SECRET), "the value was printed");
    out
}

#[test]
fn purging_a_file_with_history_leaves_no_value_in_any_revision() {
    let dir = TempDir::new("purge");
    let input = dir.join("in.pdf");
    let filled = dir.join("filled.pdf");
    let output = dir.join("out.pdf");
    std::fs::write(&input, form_storing_password()).unwrap();
    // An incremental fill leaves the old value in revision 0.
    fill(&input, &filled, &[]);
    let out = purge(&filled, &output, &[]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    let stdout = text(&out.stdout);
    // The opened revision already withholds the value; the rewrite alone
    // drops the revision that stored it.
    assert!(
        stdout.contains(" purged=0 ") && stdout.contains(" remaining=0 "),
        "{stdout}"
    );
    let bytes = std::fs::read(&output).unwrap();
    assert!(!bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()));
    assert!(!bytes.windows(7).any(|w| w == b"new-pin"));
    assert!(
        summary(&output).ends_with("revisions=1 unreadable_revisions=0 latest=0 superseded=0"),
        "{}",
        summary(&output)
    );
}

#[test]
fn a_signed_file_is_refused_unless_the_operator_accepts_invalidation() {
    let dir = TempDir::new("signed");
    let input = dir.join("in.pdf");
    let output = dir.join("out.pdf");
    std::fs::write(&input, form(true)).unwrap();
    let out = purge(&input, &output, &[]);
    assert_eq!(out.status.code(), Some(9), "stderr: {}", text(&out.stderr));
    assert!(text(&out.stderr).contains("--invalidate-signatures"));
    assert!(!output.exists(), "a refused purge wrote an output");

    let out = purge(&input, &output, &["--invalidate-signatures"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    assert!(text(&out.stdout).contains("signature=invalidated"));
    assert!(text(&out.stderr).contains("INVALIDATES"));
}

#[test]
fn purging_the_opened_revision_names_the_field_and_removes_the_value() {
    let dir = TempDir::new("purge-latest");
    let input = dir.join("in.pdf");
    let output = dir.join("out.pdf");
    std::fs::write(&input, form_storing_password()).unwrap();
    let out = purge(&input, &output, &[]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("purged field=pin read_only=0"), "{stdout}");
    assert!(
        stdout.contains(" purged=1 ") && stdout.contains(" remaining=0 "),
        "{stdout}"
    );
    let bytes = std::fs::read(&output).unwrap();
    assert!(!bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()));
}

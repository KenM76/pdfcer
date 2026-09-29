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
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>".to_owned(),
        format!(
            "<< /Type /Annot /Subtype /Widget /FT /Tx /Ff 8192 /T (pin) \
             /Rect [10 10 100 30] /P 3 0 R /V ({SECRET}) >>"
        ),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = out.len();
    out.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in offs {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
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

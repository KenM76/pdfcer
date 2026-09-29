//! CLI tests for `bates-stamp`: numbering carries across files, a refusal
//! anywhere writes nothing, and an output never replaces an input.

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "pdfcer-bates-{tag}-{}-{}",
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

/// A PDF of `pages` blank 200 x 200 pages.
fn blank(pages: usize) -> Vec<u8> {
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", i + 3)).collect();
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {pages} >>",
            kids.join(" ")
        ),
    ];
    for _ in 0..pages {
        objs.push("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_owned());
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offs = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
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
    Command::new(BIN).args(args).output().expect("run pdfcer")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn has(bytes: &[u8], needle: &str) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn numbering_carries_across_files_in_argument_order() {
    let dir = TempDir::new("batch");
    let (a, b, out) = (dir.join("a.pdf"), dir.join("b.pdf"), dir.join("out"));
    std::fs::write(&a, blank(2)).unwrap();
    std::fs::write(&b, blank(3)).unwrap();
    let o = run(&[
        "bates-stamp",
        b.to_str().unwrap(),
        a.to_str().unwrap(),
        "--out-dir",
        out.to_str().unwrap(),
        "--start",
        "5",
        "--prefix",
        "ACME",
        "--digits",
        "4",
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", text(&o.stderr));
    let stdout = text(&o.stdout);
    assert!(
        stdout.contains("pages=3 first=ACME0005 last=ACME0007"),
        "{stdout}"
    );
    assert!(
        stdout.contains("pages=2 first=ACME0008 last=ACME0009"),
        "{stdout}"
    );
    assert!(
        stdout.contains("bates-stamp files=2 pages=5 first=ACME0005 last=ACME0009 next=10"),
        "{stdout}"
    );
    let stamped_b = std::fs::read(out.join("b.pdf")).unwrap();
    let stamped_a = std::fs::read(out.join("a.pdf")).unwrap();
    assert!(
        stamped_b.starts_with(&blank(3)),
        "the original bytes were not kept"
    );
    for label in ["(ACME0005)", "(ACME0006)", "(ACME0007)"] {
        assert!(has(&stamped_b, label), "{label} missing");
    }
    for label in ["(ACME0008)", "(ACME0009)"] {
        assert!(has(&stamped_a, label), "{label} missing");
    }
}

#[test]
fn a_refusal_in_a_later_file_writes_nothing() {
    let dir = TempDir::new("overflow");
    let (a, b, out) = (dir.join("a.pdf"), dir.join("b.pdf"), dir.join("out"));
    std::fs::write(&a, blank(2)).unwrap();
    std::fs::write(&b, blank(2)).unwrap();
    // 98, 99 fit two digits; 100 does not.
    let o = run(&[
        "bates-stamp",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "--out-dir",
        out.to_str().unwrap(),
        "--start",
        "98",
        "--digits",
        "2",
    ]);
    assert_eq!(o.status.code(), Some(9), "stderr: {}", text(&o.stderr));
    assert!(
        text(&o.stderr).contains("does not fit in 2 digits"),
        "{}",
        text(&o.stderr)
    );
    assert!(!out.exists(), "a refused batch wrote output");
}

#[test]
fn an_output_never_replaces_an_input() {
    let dir = TempDir::new("inplace");
    let a = dir.join("a.pdf");
    std::fs::write(&a, blank(1)).unwrap();
    let o = run(&[
        "bates-stamp",
        a.to_str().unwrap(),
        "--out-dir",
        dir.0.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(9), "stderr: {}", text(&o.stderr));
    assert_eq!(
        std::fs::read(&a).unwrap(),
        blank(1),
        "the input was changed"
    );
}

#[test]
fn names_and_page_selection_follow_the_flags() {
    let dir = TempDir::new("names");
    let (a, out) = (dir.join("brief.pdf"), dir.join("out"));
    std::fs::write(&a, blank(3)).unwrap();
    let o = run(&[
        "bates-stamp",
        a.to_str().unwrap(),
        "--out-dir",
        out.to_str().unwrap(),
        "--pages",
        "2-3",
        "--name",
        "keep-range",
        "--suffix",
        "-C",
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", text(&o.stderr));
    let written = std::fs::read(out.join("brief_000001-C-000002-C.pdf")).expect("named output");
    assert!(has(&written, "(000002-C)") && !has(&written, "(000003-C)"));
    assert!(text(&o.stdout).contains("next=3"));
}

#[test]
fn replace_and_remove_take_off_only_the_earlier_labels() {
    let dir = TempDir::new("replace");
    let a = dir.join("a.pdf");
    std::fs::write(&a, blank(2)).unwrap();
    let (one, two, three, four) = (dir.join("1"), dir.join("2"), dir.join("3"), dir.join("4"));
    let stamp = |input: &PathBuf, out: &PathBuf, extra: &[&str]| {
        let mut args = vec![
            "bates-stamp",
            input.to_str().unwrap(),
            "--out-dir",
            out.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        run(&args)
    };
    let remove = |input: &PathBuf, out: &PathBuf| {
        run(&[
            "bates-remove",
            input.to_str().unwrap(),
            "--out-dir",
            out.to_str().unwrap(),
        ])
    };
    let o = stamp(&a, &one, &[]);
    assert!(text(&o.stdout).contains("removed=0"), "{}", text(&o.stdout));

    let o = stamp(&one.join("a.pdf"), &two, &["--replace", "--start", "100"]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", text(&o.stderr));
    assert!(text(&o.stdout).contains("removed=2"), "{}", text(&o.stdout));

    let o = remove(&two.join("a.pdf"), &three);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", text(&o.stderr));
    let stdout = text(&o.stdout);
    assert!(stdout.contains("pages=2 labels=2"), "{stdout}");
    assert!(stdout.contains("bates-remove files=1 labels=2"), "{stdout}");

    // The replaced set is gone too: nothing is left to remove.
    let o = remove(&three.join("a.pdf"), &four);
    assert!(text(&o.stdout).contains("labels=0"), "{}", text(&o.stdout));
    // An output never replaces its input.
    let o = remove(&four.join("a.pdf"), &four);
    assert_eq!(o.status.code(), Some(9));
}

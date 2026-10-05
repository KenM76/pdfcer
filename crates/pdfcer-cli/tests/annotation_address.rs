//! Every verb that addresses one annotation by `--page`/`--index` refuses a
//! bad address with `EDIT_REFUSED` (9), never the generic failure (1): the
//! document is readable and the address is wrong, and a script branches on
//! that difference.
//!
//! Fixture provenance: `fixtures/synthetic/annot/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

/// Each verb with the extra arguments it needs to reach the address check.
const VERBS: &[(&str, &[&str])] = &[
    ("delete-annotation", &[]),
    ("set-annotation-flags", &["--print"]),
    ("set-markup-style", &["--width", "2"]),
    (
        "rotate-annotation",
        &["--degrees", "90", "--anchor-x", "0", "--anchor-y", "0"],
    ),
    (
        "resize-annotation",
        &[
            "--sx",
            "2",
            "--sy",
            "2",
            "--anchor-x",
            "0",
            "--anchor-y",
            "0",
        ],
    ),
    ("set-markup-note", &["--note", "hi"]),
    ("set-annotation-open", &["--open", "true"]),
    ("move-annotation", &["--dx", "1", "--dy", "1"]),
];

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/annot/demo-annotated.pdf")
}

/// Runs `verb` at `page`/`index`; returns the exit code and stderr, and
/// asserts no output file was written.
fn run(verb: &str, extra: &[&str], page: &str, index: &str) -> (Option<i32>, String) {
    let out = std::env::temp_dir().join(format!(
        "pdfcer_annotaddr_{verb}_{page}_{index}_{}.pdf",
        std::process::id()
    ));
    let r = Command::new(BIN)
        .arg(verb)
        .arg(fixture())
        .args(extra)
        .args(["--page", page, "--index", index, "--output"])
        .arg(&out)
        .output()
        .expect("the binary runs");
    assert!(
        !out.exists(),
        "{verb}: a refused edit wrote {}",
        out.display()
    );
    (
        r.status.code(),
        String::from_utf8_lossy(&r.stderr).into_owned(),
    )
}

#[test]
fn an_index_past_the_end_is_refused_with_the_bound() {
    for (verb, extra) in VERBS {
        let (code, err) = run(verb, extra, "1", "99");
        assert_eq!(code, Some(EDIT_REFUSED), "{verb}: {err}");
        assert!(
            err.contains("no annotation at index 99 — it has 4 (indices 0..3)"),
            "{verb}: {err}"
        );
    }
}

#[test]
fn page_zero_is_refused() {
    for (verb, extra) in VERBS {
        let (code, err) = run(verb, extra, "0", "0");
        assert_eq!(code, Some(EDIT_REFUSED), "{verb}: {err}");
        assert!(err.contains("1-based"), "{verb}: {err}");
    }
}

#[test]
fn a_page_past_the_end_is_refused_with_the_page_count() {
    for (verb, extra) in VERBS {
        let (code, err) = run(verb, extra, "9", "0");
        assert_eq!(code, Some(EDIT_REFUSED), "{verb}: {err}");
        assert!(
            err.contains("no page 9 — the document has 1 page(s)"),
            "{verb}: {err}"
        );
    }
}

#[test]
fn reorder_refuses_a_bad_page_the_same_way() {
    for page in ["0", "9"] {
        let r = Command::new(BIN)
            .arg("reorder-annotations")
            .arg(fixture())
            .args(["--page", page, "--order", "0", "--output"])
            .arg(std::env::temp_dir().join(format!(
                "pdfcer_annotaddr_reorder_{page}_{}.pdf",
                std::process::id()
            )))
            .output()
            .expect("the binary runs");
        assert_eq!(r.status.code(), Some(EDIT_REFUSED), "page {page}");
    }
}

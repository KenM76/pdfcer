//! `place-text`: the CLI's own logic around `EditSession::place_text` — the
//! created-document scaffold, `--position`, flag refusals and the report.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn base() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/dimension/plain-base.pdf")
}

fn temp_path(tag: &str, ext: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_place_text_{tag}_{}_{n}.{ext}",
        std::process::id()
    ))
}

fn text_file(tag: &str, body: &[u8]) -> PathBuf {
    let p = temp_path(tag, "txt");
    std::fs::write(&p, body).unwrap();
    p
}

fn run(args: &[&str]) -> Output {
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

fn page_count(path: &Path) -> usize {
    let doc = pdfcer_core::document::Document::load(path).expect("output loads");
    pdfcer_core::page_tree::pages(&doc).unwrap().len()
}

#[test]
fn a_created_document_holds_only_the_pages_the_text_needed() {
    let txt = text_file("create", b"page one\x0cpage two\n");
    let out_pdf = temp_path("create", "pdf");
    let out = run(&[
        "place-text",
        txt.to_str().unwrap(),
        "-o",
        out_pdf.to_str().unwrap(),
        "--mode",
        "full",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("pages_created=2 first_page=1 "), "{text}");
    assert_eq!(page_count(&out_pdf), 2, "the scaffold page was removed");
    assert!(
        !stderr(&out).contains("scaffold"),
        "full mode needs no note"
    );
}

#[test]
fn an_incremental_created_document_discloses_the_scaffold_revision() {
    let txt = text_file("incr", b"hello\n");
    let out_pdf = temp_path("incr", "pdf");
    let out = run(&[
        "place-text",
        txt.to_str().unwrap(),
        "-o",
        out_pdf.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("blank scaffold page"),
        "{}",
        stderr(&out)
    );
    assert_eq!(page_count(&out_pdf), 1);
}

#[test]
fn an_insert_reports_the_one_based_page_it_landed_on() {
    let txt = text_file("insert", b"inserted\n");
    let out_pdf = temp_path("insert", "pdf");
    let src = base();
    let before = page_count(&src);
    let out = run(&[
        "place-text",
        txt.to_str().unwrap(),
        "-o",
        out_pdf.to_str().unwrap(),
        "--input",
        src.to_str().unwrap(),
        "--position",
        "end",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let expected = format!("first_page={} ", before + 1);
    assert!(stdout(&out).contains(&expected), "{}", stdout(&out));
    assert_eq!(page_count(&out_pdf), before + 1);
}

#[test]
fn each_malformed_flag_is_refused_by_name() {
    let txt = text_file("flags", b"x\n");
    let out_pdf = temp_path("flags", "pdf");
    let src = base();
    let cases: [(&[&str], &str); 6] = [
        (&["--page-size", "0,4"], "--page-size"),
        (&["--paper", "nope"], "--paper"),
        (&["--font", "Arial"], "--font"),
        (&["--align", "middle"], "--align"),
        (&["--color", "2,0"], "--color"),
        (
            &["--input", src.to_str().unwrap(), "--position", "zz"],
            "--position",
        ),
    ];
    for (extra, flag) in cases {
        let mut args = vec![
            "place-text",
            txt.to_str().unwrap(),
            "-o",
            out_pdf.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        let out = run(&args);
        assert_eq!(out.status.code(), Some(9), "{flag}: {}", stderr(&out));
        assert!(stderr(&out).contains(flag), "{flag}: {}", stderr(&out));
        assert!(!out_pdf.exists(), "{flag}: nothing written");
    }
}

#[test]
fn a_missing_text_file_is_an_io_error() {
    let out_pdf = temp_path("missing", "pdf");
    let missing = temp_path("missing", "txt");
    let out = run(&[
        "place-text",
        missing.to_str().unwrap(),
        "-o",
        out_pdf.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
}

#[test]
fn an_unencodable_character_refuses_or_is_named_when_dropped() {
    let txt = text_file("snow", "a \u{2603} b\n".as_bytes());
    let out_pdf = temp_path("snow", "pdf");
    let refused = run(&[
        "place-text",
        txt.to_str().unwrap(),
        "-o",
        out_pdf.to_str().unwrap(),
    ]);
    assert_eq!(refused.status.code(), Some(9), "{}", stderr(&refused));
    assert!(stderr(&refused).contains("place-text refused"));

    let dropped = run(&[
        "place-text",
        txt.to_str().unwrap(),
        "-o",
        out_pdf.to_str().unwrap(),
        "--drop-unmappable",
    ]);
    assert_eq!(dropped.status.code(), Some(0), "{}", stderr(&dropped));
    assert!(
        stdout(&dropped).contains("dropped_characters: U+2603 x1"),
        "{}",
        stdout(&dropped)
    );
}

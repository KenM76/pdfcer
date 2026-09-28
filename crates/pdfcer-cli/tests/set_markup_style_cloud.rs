//! `pdfcer set-markup-style --cloud 0-2|none` (Pass 264.3), black-box over
//! the real binary.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
/// `exit::EDIT_REFUSED` in the CLI's stable exit-code contract.
const EDIT_REFUSED: i32 = 9;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/annot/demo-annotated.pdf")
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pdfcer_cloud_{tag}_{}_{n}.pdf", std::process::id()))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A straight square appended at page 1, index 4.
fn straight_square() -> PathBuf {
    let out = temp_path("src");
    let src = fixture();
    let o = run(&[
        "annotate",
        src.to_str().unwrap(),
        "--type",
        "square",
        "--page",
        "1",
        "--rect",
        "10,10,60,40",
        "--color",
        "000000",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    out
}

fn restyle(input: &Path, cloud: &str, tag: &str) -> (Output, PathBuf) {
    let out = temp_path(tag);
    let o = run(&[
        "set-markup-style",
        input.to_str().unwrap(),
        "--page",
        "1",
        "--index",
        "4",
        "--cloud",
        cloud,
        "-o",
        out.to_str().unwrap(),
    ]);
    (o, out)
}

/// `/Rect` of annotation index 4 on page 1, as `list-annotations` prints it.
fn rect(path: &Path) -> String {
    let o = run(&["list-annotations", path.to_str().unwrap()]);
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .find(|l| l.contains("page=1 index=4 "))
        .and_then(|l| l.split(' ').find_map(|w| w.strip_prefix("rect=")))
        .expect("annotation 4 listed")
        .to_owned()
}

#[test]
fn cloud_then_none_round_trips_the_square() {
    let src = straight_square();
    let (o, cloudy) = restyle(&src, "1.5", "cloudy");
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(rect(&cloudy), "2.5,2.5,67.5,47.5", "the bulge widens /Rect");

    let (o, straight) = restyle(&cloudy, "none", "straight");
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(
        !stderr(&o).contains("dropped"),
        "clearing pdfcer's own cloud loses nothing: {}",
        stderr(&o)
    );
    assert_eq!(rect(&straight), "10,10,60,40");
}

#[test]
fn an_out_of_range_or_unparsable_cloud_is_refused() {
    let src = straight_square();
    for bad in ["3", "abc"] {
        let (o, out) = restyle(&src, bad, "bad");
        assert_eq!(o.status.code(), Some(EDIT_REFUSED), "{bad}: {}", stderr(&o));
        assert!(!out.exists(), "{bad}: a refusal writes nothing");
    }
}

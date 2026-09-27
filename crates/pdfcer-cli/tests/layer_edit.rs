//! `pdfcer layer-edit`: a layer's name, visibility, lock and print state.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/layers")
        .join(name)
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pdfcer_layer_edit_{tag}_{}_{n}.pdf",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

fn edit(src: &Path, extra: &[&str], tag: &str) -> (Output, PathBuf) {
    let out = temp_path(tag);
    let mut args = vec!["layer-edit", src.to_str().unwrap()];
    args.extend_from_slice(extra);
    args.extend(["--output", out.to_str().unwrap()]);
    (run(&args), out)
}

fn listing(path: &Path) -> String {
    String::from_utf8_lossy(&run(&["list-layers", path.to_str().unwrap()]).stdout).into_owned()
}

/// Rename, hide and lock by name; `list-layers` reads all three back.
#[test]
fn rename_hide_and_lock_by_name() {
    let (o, out) = edit(
        &fixture("basic-layers.pdf"),
        &[
            "--layer",
            "Dimensions",
            "--rename",
            "Welds",
            "--visible",
            "off",
            "--locked",
            "on",
        ],
        "rename",
    );
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(String::from_utf8_lossy(&o.stdout).contains("changed=true"));
    let listed = listing(&out);
    let line = listed
        .lines()
        .find(|l| l.contains("name=\"Welds\""))
        .unwrap_or_else(|| panic!("no renamed layer in {listed}"));
    assert!(line.contains("visible=0"), "{line}");
    assert!(line.contains("locked"), "{line}");
    assert!(line.contains(" id=4"), "{line}");
    std::fs::remove_file(out).ok();
}

/// `--print never` by id writes a Print usage entry the file keeps.
#[test]
fn print_never_by_id() {
    let (o, out) = edit(
        &fixture("basic-layers.pdf"),
        &["--id", "6", "--print", "never"],
        "print",
    );
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let bytes = std::fs::read(&out).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/PrintState /OFF"), "no usage written");
    assert!(text.contains("/Event /Print"), "no /AS entry written");
    std::fs::remove_file(out).ok();
}

/// An unknown name is refused and nothing is written.
#[test]
fn an_unknown_layer_is_refused() {
    let (o, out) = edit(
        &fixture("basic-layers.pdf"),
        &["--layer", "Nope", "--locked", "on"],
        "unknown",
    );
    assert_eq!(o.status.code(), Some(EDIT_REFUSED));
    assert!(String::from_utf8_lossy(&o.stderr).contains("list-layers"));
    assert!(!out.exists());
}

/// An empty new name is refused.
#[test]
fn an_empty_name_is_refused() {
    let (o, out) = edit(
        &fixture("basic-layers.pdf"),
        &["--layer", "Dimensions", "--rename", ""],
        "empty",
    );
    assert_eq!(o.status.code(), Some(EDIT_REFUSED));
    assert!(!out.exists());
}

/// `layer-add` creates a layer `list-layers` then shows, hidden and locked
/// as asked.
#[test]
fn layer_add_creates_a_listed_layer() {
    let out = temp_path("add");
    let o = run(&[
        "layer-add",
        fixture("basic-layers.pdf").to_str().unwrap(),
        "--name",
        "Welds",
        "--visible",
        "off",
        "--locked",
        "on",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
    let id = stdout
        .split_whitespace()
        .find_map(|w| w.strip_prefix("id="))
        .unwrap_or_else(|| panic!("no id in {stdout}"))
        .to_owned();
    let listed = listing(&out);
    let line = listed
        .lines()
        .find(|l| l.contains("name=\"Welds\""))
        .unwrap_or_else(|| panic!("no new layer in {listed}"));
    assert!(line.contains("visible=0"), "{line}");
    assert!(line.contains("locked"), "{line}");
    assert!(line.contains(&format!(" id={id}")), "{line}");
    std::fs::remove_file(out).ok();
}

/// An empty name is refused and nothing is written.
#[test]
fn layer_add_refuses_an_empty_name() {
    let out = temp_path("add_empty");
    let o = run(&[
        "layer-add",
        fixture("basic-layers.pdf").to_str().unwrap(),
        "--name",
        "",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(EDIT_REFUSED));
    assert!(!out.exists());
}

fn delete(src: &Path, extra: &[&str], tag: &str) -> (Output, PathBuf) {
    let out = temp_path(tag);
    let mut args = vec!["layer-delete", src.to_str().unwrap()];
    args.extend_from_slice(extra);
    args.extend(["--output", out.to_str().unwrap()]);
    (run(&args), out)
}

/// `layer-delete` removes the layer from `list-layers`, keeps what it drew,
/// and reports the unwrapped section.
#[test]
fn layer_delete_keeps_the_content() {
    let (o, out) = delete(
        &fixture("painted-layers.pdf"),
        &["--layer", "Hidden Box", "--mode", "full", "--verify-undo"],
        "delete",
    );
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(stdout.contains("sections=1 streams=1"), "{stdout}");
    let listed = listing(&out);
    assert!(!listed.contains("Hidden Box"), "{listed}");
    assert!(listed.contains("Nested Inner"), "{listed}");
    let bytes = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(!bytes.contains("/OC /L2 BDC"));
    assert!(bytes.contains("400 60 120 120 re f"));
    std::fs::remove_file(out).ok();
}

/// A layer a membership dictionary names is refused and nothing is written.
#[test]
fn layer_delete_refuses_a_membership_member() {
    let (o, out) = delete(&fixture("ocmd-membership.pdf"), &["--id", "4"], "ocmd");
    assert_eq!(o.status.code(), Some(EDIT_REFUSED));
    assert!(String::from_utf8_lossy(&o.stderr).contains("membership"));
    assert!(!out.exists());
}

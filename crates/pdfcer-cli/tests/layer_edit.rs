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

/// `--content remove` stops the layer painting and says how much it removed.
#[test]
fn layer_delete_can_remove_the_content() {
    let (o, out) = delete(
        &fixture("painted-layers.pdf"),
        &[
            "--layer",
            "Hidden Box",
            "--content",
            "remove",
            "--mode",
            "full",
            "--verify-undo",
        ],
        "delete_remove",
    );
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(stdout.contains("content=remove"), "{stdout}");
    assert!(stdout.contains("paints=2 xobject_calls=0"), "{stdout}");
    let bytes = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(bytes.contains("400 60 120 120 re n"));
    assert!(!bytes.contains("400 60 120 120 re f"));
    std::fs::remove_file(out).ok();
}

fn order_edit(verb: &str, src: &Path, extra: &[&str], tag: &str) -> (Output, PathBuf) {
    let out = temp_path(tag);
    let mut args = vec![verb, src.to_str().unwrap()];
    args.extend_from_slice(extra);
    args.extend(["--output", out.to_str().unwrap(), "--verify-undo"]);
    (run(&args), out)
}

fn tree(path: &Path) -> String {
    String::from_utf8_lossy(&run(&["list-layers", path.to_str().unwrap(), "--tree"]).stdout)
        .into_owned()
}

/// Add a folder at the end, then move a layer and its sublayer into it; the
/// tree prints each entry's `at=`.
#[test]
fn layer_folder_add_then_move_a_layer_into_it() {
    let (o, added) = order_edit(
        "layer-folder-add",
        &fixture("nested-order.pdf"),
        &["--label", "Parts"],
        "fadd",
    );
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(
        stdout.contains("at=2 changed=true follows_layer=0"),
        "{stdout}"
    );
    assert!(tree(&added).contains("folder label=\"Parts\" at=2"));

    let (o, moved) = order_edit(
        "layer-move",
        &added,
        &["--from", "1", "--parent", "1"],
        "fmove",
    );
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(stdout.starts_with("layer-move "), "{stdout}");
    assert!(stdout.contains("at=1.0 changed=true"), "{stdout}");
    let t = tree(&moved);
    assert!(t.contains("folder label=\"Parts\" at=1\n"), "{t}");
    assert!(
        t.contains("  layer name=\"WHISKEY\" visible=1 id=7 at=1.0\n"),
        "{t}"
    );
    assert!(
        t.contains("    layer name=\"VICTOR\" visible=1 id=8 at=1.0.0\n"),
        "{t}"
    );
    for p in [added, moved] {
        std::fs::remove_file(p).ok();
    }
}

/// Rename a folder, then remove it: its layer takes its place.
#[test]
fn layer_folder_rename_then_delete() {
    let (o, renamed) = order_edit(
        "layer-folder-rename",
        &fixture("nested-order.pdf"),
        &["--at", "0", "--label", "Sheets"],
        "fren",
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(tree(&renamed).contains("folder label=\"Sheets\" at=0\n"));

    let (o, deleted) = order_edit("layer-folder-delete", &renamed, &["--at", "0"], "fdel");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let t = tree(&deleted);
    assert!(!t.contains("folder label"), "{t}");
    assert!(
        t.starts_with("layer name=\"ZULU\" visible=1 id=4 at=0\n"),
        "{t}"
    );
    for p in [renamed, deleted] {
        std::fs::remove_file(p).ok();
    }
}

/// A layer is not a folder; a malformed position is a usage error.
#[test]
fn layer_folder_refusals() {
    let (o, out) = order_edit(
        "layer-folder-rename",
        &fixture("nested-order.pdf"),
        &["--at", "1", "--label", "X"],
        "fref",
    );
    assert_eq!(o.status.code(), Some(EDIT_REFUSED));
    assert!(String::from_utf8_lossy(&o.stderr).contains("is not a folder"));
    assert!(!out.exists());
    let (o, _) = order_edit(
        "layer-folder-delete",
        &fixture("nested-order.pdf"),
        &["--at", "x.1"],
        "fbad",
    );
    assert_eq!(o.status.code(), Some(2));
}

fn annotations(path: &Path) -> String {
    String::from_utf8_lossy(&run(&["list-annotations", path.to_str().unwrap()]).stdout).into_owned()
}

/// Replace an annotation's visibility expression with a layer, then take it
/// off every layer; `list-annotations` shows `oc=` each time.
#[test]
fn set_annotation_layer_puts_and_clears() {
    let src = fixture("ocmd-membership.pdf");
    let (o, put) = order_edit(
        "set-annotation-layer",
        &src,
        &["--page", "1", "--index", "0", "--layer", "Registered B"],
        "alayer",
    );
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(stdout.contains("subtype=Square oc="), "{stdout}");
    assert!(stdout.contains("->5 popup=0 changed=true"), "{stdout}");
    let listed = annotations(&put);
    assert!(
        listed.lines().next().is_some_and(|l| l.ends_with(" oc=5")),
        "{listed}"
    );

    let (o, cleared) = order_edit(
        "set-annotation-layer",
        &put,
        &["--page", "1", "--index", "0", "--none"],
        "anone",
    );
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(stdout.contains("oc=5->none"), "{stdout}");
    assert!(
        annotations(&cleared)
            .lines()
            .next()
            .is_some_and(|l| l.ends_with(" oc=none"))
    );
    for p in [put, cleared] {
        std::fs::remove_file(p).ok();
    }
}

/// A group missing from `/OCGs` is refused; nothing is written.
#[test]
fn set_annotation_layer_refuses_an_unregistered_group() {
    let (o, out) = order_edit(
        "set-annotation-layer",
        &fixture("ocmd-membership.pdf"),
        &["--page", "1", "--index", "0", "--id", "7"],
        "aunreg",
    );
    assert_eq!(o.status.code(), Some(EDIT_REFUSED));
    assert!(!out.exists());
}

fn object_layers(path: &Path) -> Vec<String> {
    let o = run(&["object-list", path.to_str().expect("utf-8 path")]);
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter(|l| l.starts_with("object "))
        .filter_map(|l| {
            l.split(' ')
                .find(|t| t.starts_with("oc="))
                .map(str::to_owned)
        })
        .collect()
}

/// Move two objects onto a layer, then take a nested one off every layer;
/// `object-list` shows `oc=` each time and the neighbours keep theirs.
#[test]
fn set_object_layer_moves_and_clears() {
    let src = fixture("painted-layers.pdf");
    assert_eq!(
        object_layers(&src),
        ["oc=4", "oc=5", "oc=7", "oc=6", "oc=none"]
    );
    let (o, moved) = order_edit(
        "set-object-layer",
        &src,
        &["--objects", "4,0", "--layer", "Hidden Box"],
        "olayer",
    );
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(
        stdout.contains("moved=2 unchanged=0 name=L2 binding_added=0"),
        "{stdout}"
    );
    assert_eq!(
        object_layers(&moved),
        ["oc=5", "oc=5", "oc=7", "oc=6", "oc=5"]
    );

    let (o, cleared) = order_edit(
        "set-object-layer",
        &moved,
        &["--objects", "2", "--none"],
        "onone",
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        object_layers(&cleared),
        ["oc=5", "oc=5", "oc=none", "oc=6", "oc=5"]
    );
    for p in [moved, cleared] {
        std::fs::remove_file(p).ok();
    }
}

/// An out-of-range index refuses the whole call with exit 9.
#[test]
fn set_object_layer_refuses_an_out_of_range_index() {
    let src = fixture("painted-layers.pdf");
    let (o, out) = order_edit(
        "set-object-layer",
        &src,
        &["--objects", "0,9", "--id", "5"],
        "orange",
    );
    assert_eq!(
        o.status.code(),
        Some(9),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(!out.exists());
}

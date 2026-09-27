//! `pdfcer list-layers --tree`: the layer panel's folders and nesting.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::Command;

fn list(file: &str, tree: bool) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/layers")
        .join(file);
    let mut args = vec!["list-layers".to_owned(), path.to_str().unwrap().to_owned()];
    if tree {
        args.push("--tree".to_owned());
    }
    let o = Command::new(env!("CARGO_BIN_EXE_pdfcer"))
        .args(&args)
        .output()
        .expect("the binary runs");
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap()
}

/// `/Order [(Sheet metal) [4 0 R [5 0 R 6 0 R]] 7 0 R [8 0 R]]`: a named
/// folder holding a layer with two sublayers, then a layer with one.
#[test]
fn a_folder_and_sublayers_print_as_an_indented_tree() {
    let out = list("nested-order.pdf", true);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[..6],
        [
            "folder label=\"Sheet metal\"",
            "  layer name=\"ZULU\" visible=1 id=4",
            "    layer name=\"YANKEE\" visible=1 id=5",
            "    layer name=\"XRAY\" visible=1 id=6",
            "layer name=\"WHISKEY\" visible=1 id=7",
            "  layer name=\"VICTOR\" visible=1 id=8",
        ]
    );
    assert!(lines[6].starts_with("list-layers "), "{out}");
}

/// A layer the tree does not reach is still listed, after it, flagged.
#[test]
fn a_layer_outside_the_tree_is_still_listed() {
    let out = list("basic-layers.pdf", true);
    assert!(
        out.lines()
            .any(|l| l.starts_with("layer name=\"€5 tier\"") && l.contains("not-in-order")),
        "{out}"
    );
    assert_eq!(out.matches("layer name=").count(), 4, "{out}");
}

/// Without the flag the flat list is unchanged: no folder rows.
#[test]
fn the_flat_list_is_the_default() {
    let out = list("nested-order.pdf", false);
    assert!(!out.contains("folder"), "{out}");
    assert!(out.lines().all(|l| !l.starts_with(' ')), "{out}");
}

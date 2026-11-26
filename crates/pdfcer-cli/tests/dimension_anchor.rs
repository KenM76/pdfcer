//! `dimension-anchor` and `dimension-add --anchor` over the real binary
//! (pdfcer-gui request G332): what is saved, what `dimension-list` reads
//! back, the followed and detached lines, and what is refused.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED`, spelled out so a change to the number is visible.
const EDIT_REFUSED: i32 = 9;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf")
}

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-anchor-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN).args(args).output().expect("pdfcer runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn s(p: &Path) -> &str {
    p.to_str().expect("utf-8 path")
}

fn add(src: &Path, out: &Path, kind: &str, points: &str, extra: &[&str]) -> (i32, String, String) {
    let mut args = vec!["dimension-add", s(src), "--kind", kind, "--points", points];
    args.extend_from_slice(extra);
    args.extend_from_slice(&["-o", s(out)]);
    run(&args)
}

/// A radius ce dimension 0 centred on (200, 200), radius 50, and a linear
/// ce dimension 1 whose second point follows its centre.
fn anchored(name: &str) -> PathBuf {
    let circle = temp_out(&format!("{name}-circle.pdf"));
    let (code, o, e) = add(
        &fixture(),
        &circle,
        "radius",
        "250,200 200,250 150,200",
        &[],
    );
    assert_eq!(code, 0, "{o}\n{e}");
    let out = temp_out(&format!("{name}.pdf"));
    let extra = ["--anchor", "1:0:mark-centre", "--verify-undo"];
    let (code, o, e) = add(&circle, &out, "linear", "100,200 190,200", &extra);
    assert_eq!(code, 0, "{o}\n{e}");
    assert!(o.contains("undo_identical=1"), "{o}");
    out
}

fn listed(path: &Path) -> String {
    run(&["dimension-list", s(path)]).1
}

#[test]
fn an_anchored_add_is_listed_solved_and_follows_a_rotation() {
    let path = anchored("rot");
    let list = listed(&path);
    assert!(
        list.contains("    anchor point=1 target=0 role=mark-centre state=solved"),
        "{list}"
    );
    let out = temp_out("rot-out.pdf");
    let (code, o, e) = run(&[
        "dimension-rotate",
        s(&path),
        "--dimension",
        "0",
        "--degrees",
        "90",
        "--pivot-x",
        "100",
        "--pivot-y",
        "200",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{e}");
    assert!(o.contains("  followed dimension=1 before="), "{o}");
}

#[test]
fn dimension_anchor_rebinds_and_clears() {
    let path = anchored("rebind");
    let out = temp_out("rebind-max.pdf");
    let (code, o, e) = run(&[
        "dimension-anchor",
        s(&path),
        "--dimension",
        "1",
        "--point",
        "1",
        "--target",
        "0",
        "--role",
        "rim-max",
        "--verify-undo",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{e}");
    assert!(
        o.contains("target=0 role=rim-max state=solved") && o.contains("undo_identical=1"),
        "{o}"
    );
    assert!(listed(&out).contains("role=rim-max state=solved"));
    let cleared = temp_out("rebind-clear.pdf");
    let args = [
        "dimension-anchor",
        s(&out),
        "--dimension",
        "1",
        "--point",
        "1",
        "--clear",
    ];
    let (code, o, e) = run(&[&args[..], &["-o", s(&cleared)]].concat());
    assert_eq!(code, 0, "{e}");
    assert!(o.contains("point=1 none state=none"), "{o}");
    assert!(!listed(&cleared).contains("anchor point="));
}

#[test]
fn deleting_the_target_prints_the_detached_dependent() {
    let path = anchored("delete");
    let out = temp_out("delete-out.pdf");
    let args = [
        "dimension-delete",
        s(&path),
        "--dimension",
        "0",
        "-o",
        s(&out),
    ];
    let (code, o, e) = run(&args);
    assert_eq!(code, 0, "{e}");
    assert!(o.contains("  detached dimension=1 value="), "{o}");
    assert!(!listed(&out).contains("anchor point="));
}

#[test]
fn refusals_exit_edit_refused_and_write_nothing() {
    let path = anchored("refuse");
    let out = temp_out("refuse-out.pdf");
    let base = ["dimension-anchor", s(&path), "--dimension", "1"];
    for extra in [
        &["--point", "2", "--target", "0", "--role", "mark-centre"][..],
        &["--point", "1", "--target", "7", "--role", "mark-centre"][..],
        &["--point", "1", "--target", "0", "--role", "line-midpoint"][..],
    ] {
        let (code, _, e) = run(&[&base[..], extra, &["-o", s(&out)]].concat());
        assert_eq!(code, EDIT_REFUSED, "{extra:?}: {e}");
        assert!(!out.exists(), "{extra:?}");
    }
    let (code, _, e) = add(
        &path,
        &out,
        "linear",
        "1,1 2,2",
        &["--anchor", "1:0:sideways"],
    );
    assert_eq!(code, EDIT_REFUSED, "{e}");
    assert!(
        e.contains("POINT:TARGET:ROLE") && e.contains("rim-max"),
        "{e}"
    );
    assert!(!out.exists());
}

/// Copy annotations `which` of `path` and paste them 100 pt up.
fn copy_paste(path: &Path, which: &str, name: &str) -> (i32, String, String) {
    let clip = temp_out(&format!("{name}.clip"));
    let copy = [
        "object-copy",
        s(path),
        "--page",
        "1",
        "--annotations",
        which,
        "--clip",
        s(&clip),
    ];
    let (code, o, e) = run(&copy);
    assert_eq!(code, 0, "{o}\n{e}");
    let out = temp_out(&format!("{name}.pdf"));
    let paste = [
        "object-paste",
        s(path),
        "--page",
        "1",
        "--clip",
        s(&clip),
        "--translate",
        "0,100",
        "--verify-undo",
        "-o",
        s(&out),
    ];
    run(&paste)
}

#[test]
fn pasting_target_and_dependent_prints_the_followed_copy() {
    let path = anchored("paste-both");
    let (code, o, e) = copy_paste(&path, "0,1", "paste-both-out");
    assert_eq!(code, 0, "{e}");
    assert!(o.contains("  followed dimension=3 "), "{o}");
    assert!(o.contains("undo_identical=1"), "{o}");
}

#[test]
fn pasting_only_the_dependent_prints_and_discloses_the_detach() {
    let path = anchored("paste-one");
    let (code, o, e) = copy_paste(&path, "1", "paste-one-out");
    assert_eq!(code, 0, "{e}");
    assert!(o.contains("  detached dimension=2 value="), "{o}");
    assert!(e.contains("anchor(s) were dropped"), "{e}");
}

#[test]
fn clearing_the_target_centre_mark_prints_the_followed_dependent() {
    let path = anchored("mark");
    let out = temp_out("mark-out.pdf");
    let args = [
        "dimension-centre-mark",
        s(&path),
        "--dimension",
        "0",
        "--clear",
        "-o",
        s(&out),
    ];
    let (code, o, e) = run(&args);
    assert_eq!(code, 0, "{e}");
    assert!(o.contains("  followed dimension=1 "), "{o}");
    assert!(listed(&out).contains("anchor point="), "{}", listed(&out));
}

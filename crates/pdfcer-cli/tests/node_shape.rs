//! `node-insert`, `node-convert` and `segment-convert` (pdfcer-gui request
//! G154), on the page and with `--leaf`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::decompose::Segment;
use pdfcer_core::vector::{Point, VectorObject};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// Object 0 on the page: `0 0 m 40 0 l 40 40 l S`. `Fm0`, drawn at
/// `2 0 0 2 100 100 cm`, holds one leaf: `0 0 m 10 0 l 10 10 l S`.
fn input(tag: &str) -> PathBuf {
    input_with(tag, "0 0 m 40 0 l 40 40 l S")
}

/// As [`input`], with `first` as object 0 on the page.
fn input_with(tag: &str, first: &str) -> PathBuf {
    let page = &format!("{first}\nq 2 0 0 2 100 100 cm /Fm0 Do Q\n");
    let fm0 = "0 0 m 10 0 l 10 10 l S\n";
    let stream = |dict: &str, body: &str| {
        format!(
            "<< {dict} /Length {} >>\nstream\n{body}endstream",
            body.len()
        )
    };
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject \
         << /Fm0 5 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        stream("", page),
        stream("/Type /XObject /Subtype /Form /BBox [0 0 20 20]", fm0),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    let path = std::env::temp_dir().join(format!(
        "pdfcer_node_shape_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&str], input: &Path, output: &Path) -> Output {
    Command::new(BIN)
        .arg(args[0])
        .arg(input)
        .args(&args[1..])
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

fn out_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "pdfcer_node_shape_{tag}_out_{}.pdf",
        std::process::id()
    ))
}

/// Page-space subpaths of page object `object`, or of form leaf `leaf`.
fn path_segments(path: &Path, leaf: bool) -> (Point, Vec<Segment>) {
    let doc = Document::from_bytes(std::fs::read(path).unwrap()).unwrap();
    let mut s = EditSession::new(doc);
    let model = s.page_objects(0).unwrap();
    let obj = if leaf {
        &model.leaves[0].object
    } else {
        &model.objects[0]
    };
    let VectorObject::Path(p) = obj else {
        panic!("not a path: {obj:?}");
    };
    let sp = p.page_subpaths().remove(0);
    (sp.start, sp.segments)
}

fn near(p: Point, x: f64, y: f64) -> bool {
    (p.x - x).abs() < 1e-6 && (p.y - y).abs() < 1e-6
}

#[test]
fn node_insert_adds_a_point_on_the_page() {
    let (i, o) = (input("ins"), out_path("ins"));
    let out = run(
        &[
            "node-insert",
            "--object",
            "0",
            "--node",
            "0",
            "--at",
            "0.25",
        ],
        &i,
        &o,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("node-insert "), "{stdout}");
    assert!(stdout.contains("object=0 node=0 at=0.25"), "{stdout}");
    let (_, segs) = path_segments(&o, false);
    assert_eq!(segs.len(), 3);
    assert!(near(segs[0].end(), 10.0, 0.0));
}

/// `--leaf` must reach the form, in page space: the leaf's first edge runs
/// from page (100, 100) to (120, 100).
#[test]
fn node_insert_with_leaf_edits_the_form() {
    let (i, o) = (input("leaf"), out_path("leaf"));
    let out = run(
        &["node-insert", "--leaf", "0", "--node", "0", "--at", "0.5"],
        &i,
        &o,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (_, segs) = path_segments(&o, true);
    assert_eq!(segs.len(), 3);
    assert!(near(segs[0].end(), 110.0, 100.0), "{:?}", segs[0].end());
    let (_, page) = path_segments(&o, false);
    assert_eq!(page.len(), 2, "the page object is untouched");
}

#[test]
fn node_convert_smooth_makes_curves_and_says_so() {
    let (i, o) = (input("conv"), out_path("conv"));
    let out = run(
        &[
            "node-convert",
            "--object",
            "0",
            "--node",
            "1",
            "--kind",
            "smooth",
        ],
        &i,
        &o,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("kind=smooth"));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("turned into a curve"),
        "the line-to-curve promotion is disclosed"
    );
    let (_, segs) = path_segments(&o, false);
    assert!(segs.iter().all(|s| matches!(s, Segment::Cubic { .. })));
}

#[test]
fn segment_convert_with_leaf_curves_the_form_segment() {
    let (i, o) = (input("seg"), out_path("seg"));
    let out = run(
        &[
            "segment-convert",
            "--leaf",
            "0",
            "--node",
            "1",
            "--to",
            "curve",
        ],
        &i,
        &o,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (_, segs) = path_segments(&o, true);
    assert!(matches!(segs[0], Segment::Line { .. }));
    assert!(matches!(segs[1], Segment::Cubic { .. }));
}

#[test]
fn a_refusal_names_the_problem_and_writes_nothing() {
    let (i, o) = (input("ref"), out_path("ref"));
    let _ = std::fs::remove_file(&o);
    let out = run(
        &["node-insert", "--object", "0", "--node", "2", "--at", "0.5"],
        &i,
        &o,
    );
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no segment after it"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!o.exists());
}

fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn path_extend_continues_from_the_last_point_on_the_page() {
    let (i, o) = (input("ext"), out_path("ext"));
    let args = [
        "path-extend",
        "--object",
        "0",
        "--node",
        "2",
        "--to",
        "0,40",
        "--to",
        "-5,45,-5,55,0,60",
        "--verify-undo",
    ];
    let stdout = ok(&run(&args, &i, &o));
    assert!(stdout.starts_with("path-extend "), "{stdout}");
    assert!(stdout.contains("object=0 node=2 segments=2"), "{stdout}");
    assert!(stdout.contains("undo_identical=1"), "{stdout}");
    let (start, segs) = path_segments(&o, false);
    assert!(near(start, 0.0, 0.0));
    assert_eq!(segs.len(), 4);
    assert!(near(segs[2].end(), 0.0, 40.0));
    assert!(matches!(segs[3], Segment::Cubic { c1, to, .. }
        if near(c1, -5.0, 45.0) && near(to, 0.0, 60.0)));
}

/// The form is drawn at `2 0 0 2 100 100 cm`; `--to` is page space.
#[test]
fn path_extend_continues_from_the_first_point_inside_a_form() {
    let (i, o) = (input("extleaf"), out_path("extleaf"));
    let args = [
        "path-extend",
        "--leaf",
        "0",
        "--node",
        "0",
        "--to",
        "90,110",
    ];
    let stdout = ok(&run(&args, &i, &o));
    assert!(stdout.contains("leaf=0 node=0 segments=1"), "{stdout}");
    let (start, segs) = path_segments(&o, true);
    assert!(near(start, 90.0, 110.0), "{start:?}");
    assert_eq!(segs.len(), 3);
    assert!(near(segs[0].end(), 100.0, 100.0));
}

#[test]
fn path_open_opens_a_closed_path_and_the_result_can_be_continued() {
    let i = input_with("open", "0 0 m 40 0 l 40 40 l h S");
    let (o, o2) = (out_path("open"), out_path("open2"));
    let stdout = ok(&run(&["path-open", "--object", "0", "--node", "1"], &i, &o));
    assert!(stdout.starts_with("path-open "), "{stdout}");
    let (start, segs) = path_segments(&o, false);
    assert!(near(start, 40.0, 0.0));
    assert_eq!(segs.len(), 3);
    assert!(near(segs[2].end(), 40.0, 0.0));

    let args = [
        "path-extend",
        "--object",
        "0",
        "--node",
        "3",
        "--to",
        "60,0",
    ];
    ok(&run(&args, &o, &o2));
    assert_eq!(path_segments(&o2, false).1.len(), 4);
}

#[test]
fn the_path_commands_refuse_by_name_and_write_nothing() {
    let closed = input_with("refuse", "0 0 m 40 0 l 40 40 l h S");
    let o = out_path("refuse");
    let _ = std::fs::remove_file(&o);
    let args = ["path-extend", "--object", "0", "--node", "0", "--to", "1,1"];
    let out = run(&args, &closed, &o);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("closed path"), "{stderr}");
    assert!(!o.exists());

    let out = run(
        &["path-open", "--object", "0", "--node", "0"],
        &input("refuse2"),
        &o,
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("already open"), "{stderr}");
    assert!(!o.exists());

    let args = [
        "path-extend",
        "--object",
        "0",
        "--node",
        "2",
        "--to",
        "1,2,3",
    ];
    let out = run(&args, &input("refuse3"), &o);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("got 3 numbers"), "{stderr}");
}

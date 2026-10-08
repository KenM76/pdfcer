//! `pdfcer add-link`, `set-link-target` and `set-link-border` (pdfcer-gui
//! request G161).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pdfcer_core::annot::page_link_destinations;
use pdfcer_core::document::Document;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::outline::{DestView, Destination, DestinationReader};
use pdfcer_core::page_tree::pages;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// Two blank pages.
fn blank(tag: &str) -> PathBuf {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >>",
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    let path = std::env::temp_dir().join(format!(
        "pdfcer_link_author_{tag}_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, buf).unwrap();
    path
}

fn pdfcer(args: &[&str], input: &Path, output: &Path) -> Output {
    Command::new(BIN)
        .arg(args[0])
        .arg(input)
        .args(&args[1..])
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Page 1's link destinations, and its first annotation dictionary.
fn first_link(path: &Path) -> (Vec<Destination>, pdfcer_core::object::Dict) {
    let doc = Document::from_bytes(std::fs::read(path).unwrap()).unwrap();
    let page = pages(&doc).unwrap()[0].id;
    let dests = page_link_destinations(&doc, page, &DestinationReader::new(&doc))
        .links
        .into_iter()
        .map(|l| l.destination)
        .collect();
    let Some(Object::Dict(p)) = doc.value(page).cloned() else {
        panic!("page")
    };
    let Some(Object::Array(annots)) = p.get(b"Annots").cloned() else {
        panic!("no /Annots")
    };
    let Some(Object::Dict(a)) = annots.first().map(|r| doc.resolve(r).clone()) else {
        panic!("annot")
    };
    (dests, a)
}

fn linked(tag: &str, extra: &[&str]) -> (PathBuf, PathBuf) {
    let input = blank(tag);
    let output = input.with_extension("linked.pdf");
    let mut args = vec!["add-link", "--page", "1", "--rect", "20,20,120,40"];
    args.extend_from_slice(extra);
    let out = pdfcer(&args, &input, &output);
    assert!(out.status.success(), "{out:?}");
    (input, output)
}

#[test]
fn add_link_to_a_page_is_invisible_by_default() {
    let (_, output) = linked("page", &["--to-page", "2", "--top", "150"]);
    let (dests, annot) = first_link(&output);
    assert!(
        dests.iter().any(|d| matches!(
            d,
            Destination::Page {
                page_index: 1,
                view: DestView::Xyz { top: Some(t), .. }
            } if (*t - 150.0).abs() < 1e-9
        )),
        "{dests:?}"
    );
    assert!(annot.get(b"Border").is_some());
    assert!(annot.get(b"AP").is_none());
}

#[test]
fn a_bordered_uri_link_then_retarget_and_unborder() {
    let (input, output) = linked(
        "uri",
        &[
            "--uri",
            "https://example.com/x",
            "--border-width",
            "2",
            "--border-color",
            "0000FF",
            "--dash",
            "3,2",
        ],
    );
    let (_, annot) = first_link(&output);
    assert!(annot.get(b"AP").is_some());
    assert!(annot.get(b"BS").is_some());

    let retargeted = input.with_extension("retarget.pdf");
    let out = pdfcer(
        &[
            "set-link-target",
            "--page",
            "1",
            "--index",
            "0",
            "--to-page",
            "2",
        ],
        &output,
        &retargeted,
    );
    assert!(out.status.success(), "{out:?}");
    assert!(
        stdout(&out).contains("replaced a /URI action"),
        "{}",
        stdout(&out)
    );
    let (dests, annot) = first_link(&retargeted);
    assert!(annot.get(b"A").is_none());
    assert!(
        dests
            .iter()
            .any(|d| matches!(d, Destination::Page { page_index: 1, .. }))
    );

    let plain = input.with_extension("plain.pdf");
    let out = pdfcer(
        &["set-link-border", "--page", "1", "--index", "0", "--none"],
        &retargeted,
        &plain,
    );
    assert!(out.status.success(), "{out:?}");
    assert!(
        stdout(&out).contains("appearance_replaced=1"),
        "{}",
        stdout(&out)
    );
    let (_, annot) = first_link(&plain);
    assert!(annot.get(b"AP").is_none());
    assert!(annot.get(b"Dest").is_some(), "target kept");
}

#[test]
fn bad_flags_are_refused_before_opening() {
    let input = blank("bad");
    let output = input.with_extension("bad_out.pdf");
    for args in [
        &["add-link", "--page", "1", "--rect", "1,1,9,9"][..],
        &[
            "add-link",
            "--page",
            "1",
            "--rect",
            "1,1,9,9",
            "--to-page",
            "2",
            "--uri",
            "a:b",
        ],
        &[
            "add-link", "--page", "1", "--rect", "1,1,9,9", "--uri", "a:b", "--dash", "3",
        ],
        &[
            "add-link",
            "--page",
            "1",
            "--rect",
            "1,1,9,9",
            "--dest-name",
            "x",
            "--top",
            "5",
        ],
        &[
            "add-link",
            "--page",
            "1",
            "--rect",
            "1,1,9,9",
            "--uri",
            "caf\u{e9}",
        ],
        &["set-link-border", "--page", "1", "--index", "0"],
    ] {
        let out = pdfcer(args, &input, &output);
        assert!(!out.status.success(), "{args:?} succeeded");
        assert!(!output.exists(), "{args:?} wrote output");
    }
}

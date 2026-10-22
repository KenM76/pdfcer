//! Separation preview (`SeparationPreview`): the page as a press prints it
//! with chosen plates left off, overprint included.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, SeparationPreview, render_page_with};

/// Build an offset-consistent classic PDF from `(number, body)` pairs.
pub(super) fn build(objects: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..=max_num {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            max_num + 1
        )
        .as_bytes(),
    );
    buf
}

/// A 100 pt page painting `content`, with a `Spot1` separation (`/CS0`) and
/// an overprinting graphics state (`/OP`). `cmyk_group` gives the page a
/// `DeviceCMYK` transparency group, so it composites in ink.
fn page(content: &str, cmyk_group: bool) -> Vec<u8> {
    let group = if cmyk_group {
        "/Group << /S /Transparency /CS /DeviceCMYK >>"
    } else {
        ""
    };
    build(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
        ),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /Contents 4 0 R {group} /Resources << \
                 /ColorSpace << /CS0 [/Separation /Spot1 /DeviceCMYK 5 0 R] >> \
                 /ExtGState << /OP << /OP true /op true /OPM 1 >> >> >> >>"
            )
            .into_bytes(),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len() + 1
            )
            .into_bytes(),
        ),
        (
            5,
            b"<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0.8 0.2 0] /N 1 >>".to_vec(),
        ),
    ])
}

fn render(bytes: Vec<u8>, preview: Option<SeparationPreview>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let page = page_tree::pages(&doc).expect("page tree walks").remove(0);
    let options = RenderOptions::default().with_separation_preview(preview);
    render_page_with(&doc, &page, 1.0, &options).expect("render succeeds")
}

fn hiding(names: &[&str]) -> Option<SeparationPreview> {
    Some(SeparationPreview::new().with_hidden(names.iter().copied()))
}

const CYAN: &str = "0.6 0 0 0 k 10 10 50 50 re f";
const SPOT: &str = "/CS0 cs 1 scn 30 30 50 50 re f";

#[test]
fn hiding_black_equals_painting_without_it() {
    let with_k = page("0.2 0.3 0.4 0.5 k 20 20 60 60 re f", true);
    let without_k = page("0.2 0.3 0.4 0 k 20 20 60 60 re f", true);
    let hidden = render(with_k.clone(), hiding(&["Black"]));
    assert_eq!(hidden.diagnostics.inks_hidden, 1);
    assert_eq!(
        hidden.pixmap.data(),
        render(without_k.clone(), None).pixmap.data()
    );
    assert_ne!(
        render(with_k, None).pixmap.data(),
        render(without_k, None).pixmap.data()
    );
}

#[test]
fn hiding_an_overprinting_spot_leaves_the_process_ink_under_it() {
    let both = page(&format!("{CYAN} /OP gs {SPOT}"), true);
    let cyan_only = render(page(CYAN, true), None);
    let hidden = render(both.clone(), hiding(&["Spot1"]));
    let d = &hidden.diagnostics;
    assert_eq!(d.inks, ["Cyan", "Magenta", "Yellow", "Black", "Spot1"]);
    assert_eq!((d.inks_hidden, d.inks_forced_subtractive), (1, 0));
    assert!(d.inks_unmatched.is_empty());
    assert_eq!(hidden.pixmap.data(), cyan_only.pixmap.data());
    assert_ne!(
        render(both, None).pixmap.data(),
        cyan_only.pixmap.data(),
        "the spot must show when not hidden"
    );
    // A knocking-out spot erases the cyan under it, so hiding it leaves paper.
    let knockout = render(page(&format!("{CYAN} {SPOT}"), true), hiding(&["Spot1"]));
    assert_ne!(knockout.pixmap.data(), cyan_only.pixmap.data());
}

#[test]
fn an_additive_page_is_composited_in_ink_and_says_so() {
    let r = render(
        page(&format!("{CYAN} 0 0 0 1 k 70 70 20 20 re f"), false),
        hiding(&["Black", "PANTONE 999 C"]),
    );
    let d = &r.diagnostics;
    assert_eq!(d.inks_forced_subtractive, 1);
    assert!(d.cmyk_buffer_engaged);
    assert_eq!(d.inks, ["Cyan", "Magenta", "Yellow", "Black"]);
    assert_eq!(d.inks_hidden, 1);
    assert_eq!(d.inks_unmatched, ["PANTONE 999 C"]);
    // The black patch is gone: paper white at its centre.
    let px = r.pixmap.pixel(80, 20).unwrap();
    assert_eq!((px.red(), px.green(), px.blue()), (255, 255, 255));
}

#[test]
fn without_a_preview_nothing_is_listed_or_forced() {
    let r = render(page(CYAN, false), None);
    let d = &r.diagnostics;
    assert!(d.inks.is_empty() && d.inks_unmatched.is_empty());
    assert_eq!((d.inks_hidden, d.inks_forced_subtractive), (0, 0));
    assert!(!d.cmyk_buffer_engaged);
}

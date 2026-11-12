//! Type 1 (function-based) shadings: a 2-in `/Function` over `/Domain`
//! `[xmin xmax ymin ymax]`, mapped into target space by `/Matrix`, and
//! nothing painted outside the transformed domain (ISO 32000-1 §8.7.4.5.2,
//! Table 79; spec RAG SH15, SH19-SH21).
//!
//! The fixtures use an asymmetric domain and a non-diagonal `/Matrix`, so a
//! transposed domain order or a forward-instead-of-inverse matrix lands the
//! colour somewhere else rather than agreeing by symmetry.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

/// Assemble a classic single-page PDF with a correct xref table.
fn build(objects: &[(u32, String)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let size = objects.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    offsets.sort_unstable();
    for (_, off) in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// A 100x100 page whose `/Sh0` and `/P1` are `shading`, painted by
/// `content`; object 5 is the 2-in type 4 function [`RGB_PROGRAM`], and
/// `extra` holds any further objects from 6 up.
fn page(shading: &str, content: &str, extra: Vec<(u32, String)>) -> Vec<u8> {
    let mut objects = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".into(),
        ),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
                 /Resources << /Shading << /Sh0 {shading} >> \
                 /Pattern << /P1 << /PatternType 2 /Shading {shading} >> >> >> >>"
            ),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len() + 1
            ),
        ),
        (5, function(RGB_PROGRAM, "[0 1 0 1 0 1]")),
    ];
    objects.extend(extra);
    build(&objects)
}

/// A type 4 function over `[0 2 0 1]` running `program`.
fn function(program: &str, range: &str) -> String {
    format!(
        "<< /FunctionType 4 /Domain [0 2 0 1] /Range {range} /Length {} >>\n\
         stream\n{program}\nendstream",
        program.len()
    )
}

/// Domain `[0 2 0 1]`, rotated 90 degrees and placed at (80, 10): domain
/// `(x, y)` lands at user `(80 - 40y, 10 + 40x)`, so the domain covers user
/// `x` 40..80, `y` 10..90.
const ROTATED: &str = "/ShadingType 1 /ColorSpace /DeviceRGB /Domain [0 2 0 1] \
     /Matrix [0 40 -40 0 80 10]";

/// Colour `(x / 2, y, 0)` at domain `(x, y)`.
const RGB_PROGRAM: &str = "{ exch 2 div exch 0 }";

fn render(bytes: Vec<u8>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    render_page_with(&doc, &p, 1.0, &RenderOptions::default()).expect("render")
}

/// The demultiplied pixel at user `(x, y)` on the 100pt page at 1.0 scale.
fn at(r: &RenderedPage, x: u32, y: u32) -> [u8; 3] {
    let p = r.pixmap.pixel(x, 100 - y).expect("in bounds").demultiply();
    [p.red(), p.green(), p.blue()]
}

fn near(got: [u8; 3], want: [u8; 3]) -> bool {
    got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 6)
}

fn shading(extra: &str, function: &str) -> String {
    format!("<< {ROTATED} {extra} /Function {function} >>")
}

#[test]
fn a_type_1_shading_paints_its_function_through_the_inverse_matrix() {
    let r = render(page(&shading("", "5 0 R"), "/Sh0 sh", Vec::new()));
    // User (60, 50) is domain (1, 0.5): colour (0.5, 0.5, 0).
    assert!(near(at(&r, 60, 50), [128, 128, 0]), "{:?}", at(&r, 60, 50));
    // User (75, 80) is domain (1.75, 0.125): colour (0.875, 0.125, 0).
    assert!(near(at(&r, 75, 80), [223, 32, 0]), "{:?}", at(&r, 75, 80));
    // User (45, 20) is domain (0.25, 0.875): colour (0.125, 0.875, 0).
    assert!(near(at(&r, 45, 20), [32, 223, 0]), "{:?}", at(&r, 45, 20));
    assert_eq!(r.diagnostics.shading.painted, 1);
    assert_eq!(r.diagnostics.color.patterns_unpainted, 0);
}

/// SH21: under `sh` there is no `/Background`, so outside the transformed
/// domain rectangle nothing is painted at all.
#[test]
fn outside_the_transformed_domain_nothing_is_painted() {
    let r = render(page(
        &shading("/Background [0 0 1]", "5 0 R"),
        "/Sh0 sh",
        Vec::new(),
    ));
    for (x, y) in [(20, 50), (90, 50), (60, 5), (60, 95)] {
        assert_eq!(at(&r, x, y), [255, 255, 255], "({x}, {y})");
    }
    // Just inside each edge, so the test cannot pass by painting nothing.
    for (x, y) in [(42, 50), (78, 50), (60, 12), (60, 88)] {
        assert_ne!(at(&r, x, y), [255, 255, 255], "({x}, {y})");
    }
}

/// SH15: an array of n 2-in, 1-out functions means the same as one 2-in,
/// n-out function.
#[test]
fn an_array_of_one_output_functions_is_the_same_colour() {
    let per = |p: &str| function(p, "[0 1]");
    let extra = vec![
        (6, per("{ pop 2 div }")),
        (7, per("{ exch pop }")),
        (8, per("{ pop pop 0 }")),
    ];
    let r = render(page(&shading("", "[6 0 R 7 0 R 8 0 R]"), "/Sh0 sh", extra));
    assert!(near(at(&r, 60, 50), [128, 128, 0]), "{:?}", at(&r, 60, 50));
    assert!(near(at(&r, 75, 80), [223, 32, 0]), "{:?}", at(&r, 75, 80));
}

/// A shading pattern fill takes the same painter.
#[test]
fn a_type_1_shading_pattern_fill_is_painted() {
    let r = render(page(
        &shading("", "5 0 R"),
        "/Pattern cs /P1 scn 0 0 100 100 re f",
        Vec::new(),
    ));
    assert!(near(at(&r, 60, 50), [128, 128, 0]), "{:?}", at(&r, 60, 50));
    assert_eq!(r.diagnostics.color.patterns_unpainted, 0);
}

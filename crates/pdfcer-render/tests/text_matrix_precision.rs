//! A text line reached by a long chain of relative `Td` steps renders
//! exactly like the same line placed by one absolute `Tm` (§9.4.2).
//!
//! CAD exporters emit one absolute `Tm` and then hundreds of relative
//! `Td` steps. `split_text_object` replaces part of such a chain with the
//! absolute `Tm` it sums to, so the split renders identically only if the
//! renderer composes the chain, and reads its operands, in `f64`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, render_page_with};

const STEPS: usize = 240;
const START: (f64, f64) = (803.318_77, 1_163.742_91);

fn step(i: usize) -> (f64, f64) {
    #[allow(clippy::cast_precision_loss)] // i < 240
    let k = (i % 7) as f64;
    // Page-scale jumps, as a CAD sheet's labels make: an `f32` operand
    // of this size is already off by ~1e-5 before any composition.
    (97.377_13 * (k - 3.0) + 0.013_7, -4.173_19)
}

/// `cut = None`: the whole chain relative. `Some(n)`: before step `n` the
/// text object is closed and reopened at the chain's absolute position,
/// exactly what `split_text_object` writes.
fn content(cut: Option<usize>) -> String {
    let mut s = format!("BT /F1 3 Tf 1 0 0 1 {} {} Tm\n", START.0, START.1);
    let (mut x, mut y) = START;
    for i in 0..STEPS {
        let (dx, dy) = step(i);
        if cut == Some(i) {
            writeln!(s, "ET BT /F1 3 Tf 1 0 0 1 {x} {y} Tm").unwrap();
        }
        writeln!(s, "{dx} {dy} Td (Hg{i}) Tj").unwrap();
        x += dx;
        y += dy;
    }
    s.push_str("ET\n");
    s
}

fn pdf(content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 1600 1200] \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    buf
}

fn render(content: &str) -> Vec<u8> {
    let doc = Document::from_bytes(pdf(content)).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    render_page_with(&doc, &p, 2.0, &RenderOptions::default())
        .expect("render")
        .pixmap
        .data()
        .to_vec()
}

#[test]
fn a_relative_chain_renders_like_its_absolute_split() {
    let whole = render(&content(None));
    let differing: Vec<usize> = [40, 97, 150, 201, 233]
        .into_iter()
        .filter(|&cut| render(&content(Some(cut))) != whole)
        .collect();
    assert!(
        differing.is_empty(),
        "splitting the chain before steps {differing:?} changed the rendering"
    );
}

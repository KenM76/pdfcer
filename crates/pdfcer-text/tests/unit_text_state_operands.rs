//! A text-state operator with surplus operands is ignored, as the renderer
//! ignores it, so extracted glyphs sit where the page paints them (§9.3
//! Table 105, X099).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use pdfcer_model::document::Document;
use pdfcer_text::text_extract::{ExtractOptions, extract_document};

/// One page, `/F1` Helvetica, showing `content`.
fn pdf(content: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// Every glyph's origin, in page order.
fn origins(content: &str) -> Vec<(f32, f32)> {
    let doc = Document::from_bytes(pdf(content)).unwrap();
    let text = extract_document(&doc, &ExtractOptions::default()).unwrap();
    text.pages[0]
        .runs
        .iter()
        .flat_map(|r| r.glyphs.iter().map(|g| (g.x, g.y)))
        .collect()
}

fn assert_at(content: &str, glyph: usize, want: (f32, f32)) {
    let o = origins(content);
    let got = o[glyph];
    assert!(
        (got.0 - want.0).abs() < 0.01 && (got.1 - want.1).abs() < 0.01,
        "{content}: glyph {glyph} at {got:?}, want {want:?} ({o:?})"
    );
}

/// Helvetica A at 12: 667 / 1000 * 12.
const A: f32 = 8.004;
/// Helvetica space at 12: 278 / 1000 * 12.
const SPACE: f32 = 3.336;

/// Each operator with a surplus operand (ignored) and alone (today's
/// output). The surplus values differ from each other and from the default,
/// so reading either the first or the last operand fails.
struct Case {
    surplus: &'static str,
    single: &'static str,
    show: &'static str,
    glyph: usize,
    ignored: (f32, f32),
    set: (f32, f32),
}

const CASES: &[Case] = &[
    Case {
        surplus: "5 30 Ts",
        single: "30 Ts",
        show: "(A) Tj",
        glyph: 0,
        ignored: (100.0, 700.0),
        set: (100.0, 730.0),
    },
    Case {
        surplus: "4 9 Tc",
        single: "9 Tc",
        show: "(AA) Tj",
        glyph: 1,
        ignored: (100.0 + A, 700.0),
        set: (100.0 + A + 9.0, 700.0),
    },
    Case {
        surplus: "5 9 Tw",
        single: "9 Tw",
        show: "(A A) Tj",
        glyph: 2,
        ignored: (100.0 + A + SPACE, 700.0),
        set: (100.0 + A + SPACE + 9.0, 700.0),
    },
    Case {
        surplus: "50 200 Tz",
        single: "200 Tz",
        show: "(AA) Tj",
        glyph: 1,
        ignored: (100.0 + A, 700.0),
        set: (100.0 + 2.0 * A, 700.0),
    },
    Case {
        surplus: "12 TL 5 30 TL",
        single: "12 TL 30 TL",
        show: "(A) Tj T* (A) Tj",
        glyph: 1,
        ignored: (100.0, 688.0),
        set: (100.0, 670.0),
    },
];

fn content(op: &str, show: &str) -> String {
    format!("BT /F1 12 Tf {op} 100 700 Td {show} ET")
}

#[test]
fn a_surplus_operand_leaves_the_parameter_alone() {
    for c in CASES {
        assert_at(&content(c.surplus, c.show), c.glyph, c.ignored);
    }
}

#[test]
fn a_single_operand_sets_the_parameter() {
    for c in CASES {
        assert_at(&content(c.single, c.show), c.glyph, c.set);
    }
}

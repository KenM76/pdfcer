//! Fuzz target: mask-aware image hit-testing (`vector::DocumentImageAlpha`).
//!
//! Builds a one-page PDF placing one image whose dictionary, mask kind,
//! `/Decode`, sample bytes and CTM all come from the input, then hit-tests a
//! grid of points over and around it. Drives every mask route — `/SMask`,
//! stencil `/Mask`, colour-key `/Mask`, `/ImageMask true` — through the
//! sample unpacking at every legal and illegal bit depth, short data, and
//! degenerate or non-finite placements.
//!
//! Invariant (ARCHITECTURE.md §10): no panic and bounded work for any input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::vector::{DocumentImageAlpha, Matrix, Point, decompose_page, hit_test_point_with};

fuzz_target!(|data: &[u8]| {
    let [kind, w, h, bpc, flags, a, b, c, d, rest @ ..] = data else {
        return;
    };
    // Mostly small grids; a high bit asks for a huge declared size, which
    // must be refused before anything is allocated for it.
    let size = |v: u8| {
        if v & 0x80 == 0 {
            u32::from(v % 17)
        } else {
            u32::from(v) << 24
        }
    };
    let (w, h) = (size(*w), size(*h));
    let bpc = [1, 2, 4, 8, 16, 3][usize::from(*bpc % 6)];
    let decode = if flags & 1 == 1 { " /Decode [1 0]" } else { "" };
    let grey = format!("/ColorSpace /DeviceGray /BitsPerComponent {bpc}");
    let (image, mask) = match kind % 5 {
        0 => (
            format!("{grey} /SMask 6 0 R"),
            Some(format!("{grey}{decode}")),
        ),
        1 => (
            format!("{grey} /Mask 6 0 R"),
            Some(format!("/ImageMask true{decode}")),
        ),
        2 => (
            format!("{grey} /Mask [{} {}]", flags >> 4, flags & 15),
            None,
        ),
        3 => (format!("/ImageMask true{decode}"), None),
        _ => (format!("{grey} /SMaskInData 1"), None),
    };
    let content = format!(
        "q {} {} {} {} 10 10 cm /Im0 Do Q",
        f64::from(*a as i8),
        f64::from(*b as i8),
        f64::from(*c as i8),
        f64::from(*d as i8)
    );
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R \
          /Resources << /XObject << /Im0 5 0 R >> >> >>"
            .to_vec(),
        stream("", content.as_bytes()),
        stream(&format!("/Width {w} /Height {h} {image}"), rest),
    ];
    if let Some(mask) = mask {
        objects.push(stream(&format!("/Width {h} /Height {w} {mask}"), rest));
    }
    let Ok(doc) = Document::from_bytes(assemble(&objects)) else {
        return;
    };
    let Ok(pages) = page_tree::pages(&doc) else {
        return;
    };
    let Some(page) = pages.first() else {
        return;
    };
    let view = doc.view();
    let Ok(model) = decompose_page(&view, page, Matrix::IDENTITY) else {
        return;
    };
    let alpha = DocumentImageAlpha::new(&view);
    for i in 0..12 {
        for j in 0..12 {
            let p = Point::new(f64::from(i) * 25.0 - 50.0, f64::from(j) * 25.0 - 50.0);
            let _ = hit_test_point_with(&model, p, 1.0, &alpha);
        }
    }
});

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut s = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    s.extend_from_slice(data);
    s.extend_from_slice(b"\nendstream");
    s
}

fn assemble(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(o);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    let n = objects.len() + 1;
    pdf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

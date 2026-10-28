//! Fuzz target: the XDP reader behind `/AcroForm /XFA`
//! and template DOM (`pdfcer_core::xfa::read_xdp`, `read_template`,
//! decision 204, Pass 647.0).
//!
//! The fuzz bytes become the XFA stream of a fixed one-page document, raw
//! or split into a two-stream packet array. Reading must never panic, and
//! deep nesting must be refused rather than overflow roxmltree's
//! recursive-descent parser (ARCHITECTURE.md §10).

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;

fn wrap(data: &[u8]) -> Vec<u8> {
    let split = data.first().map_or(0, |b| usize::from(*b)) % (data.len() + 1);
    let as_array = data.len() % 2 == 1;
    let xfa = if as_array {
        "[(a) 4 0 R (b) 5 0 R]"
    } else {
        "4 0 R"
    };
    let mut bodies: Vec<Vec<u8>> = vec![
        format!("<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [] /XFA {xfa} >> >>")
            .into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>".to_vec(),
    ];
    let parts: Vec<&[u8]> = if as_array {
        vec![&data[..split], &data[split..]]
    } else {
        vec![data]
    };
    for p in parts {
        let mut s = format!("<< /Length {} >>\nstream\n", p.len()).into_bytes();
        s.extend_from_slice(p);
        s.extend_from_slice(b"\nendstream");
        bodies.push(s);
    }
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref = buf.len();
    let n = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f\r\n").as_bytes());
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn walk(n: &pdfcer_core::xfa::XfaNode) {
    for k in ["x", "y", "w", "h", "minW", "maxW", "minH", "maxH"] {
        let _ = n.measurement(k);
    }
    let _ = (n.layout(), n.anchor_type(), n.presence(), n.col_span(), n.occur());
    for c in &n.children {
        walk(c);
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = Document::from_bytes(wrap(data)) {
        if let Ok(Some(xdp)) = pdfcer_core::xfa::read_xdp(&doc.view()) {
            for p in xdp.packets() {
                let _ = xdp.packet_text(&p.name);
            }
            if let Ok(Some(t)) = pdfcer_core::xfa::read_template(&xdp) {
                walk(t.element());
            }
        }
    }
});

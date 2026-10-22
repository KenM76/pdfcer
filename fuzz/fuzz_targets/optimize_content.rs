//! Fuzz target: content-stream compaction in the optimizer (`content(...)`,
//! `pdfcer_core::edit::EditSession::plan_optimize` / `apply_optimize`).
//!
//! The first byte picks the settings (its high bit splits the rest into two
//! page content streams); the rest becomes the content of a one-page PDF,
//! so the tokenizer, the redundancy walk, the emitter and the re-read check
//! all see arbitrary operators and operands. Planning and applying must
//! never panic, and an applied plan must save.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::optimize::{ContentPatch, OptimizeSettings};

const SPECS: [&str; 8] = [
    "content()",
    "content(whitespace=keep)",
    "content(numbers=keep)",
    "content(redundant=off)",
    "content(whitespace=keep,numbers=keep)",
    "content(whitespace=minimal,redundant=on,numbers=shortest)",
    "content(round=0.01)",
    "content(round=5,whitespace=keep)",
];

/// A one-page PDF whose `/Contents` is one stream per entry of `streams`.
fn pdf(streams: &[&[u8]]) -> Vec<u8> {
    let refs: Vec<String> = (0..streams.len())
        .map(|k| format!("{} 0 R", k + 4))
        .collect();
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents [{}] >>",
        refs.join(" ")
    );
    let mut bodies: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        page.into_bytes(),
    ];
    for content in streams {
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend_from_slice(content);
        stream.extend_from_slice(b"\nendstream");
        bodies.push(stream);
    }
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fuzz_target!(|data: &[u8]| {
    let Some((&pick, content)) = data.split_first() else {
        return;
    };
    // The high bit splits the content in two page streams, so the second
    // starts under the matrix the first leaves.
    let streams: Vec<&[u8]> = if pick & 0x80 == 0 {
        vec![content]
    } else {
        let (a, b) = content.split_at(content.len() / 2);
        vec![a, b]
    };
    let Ok(doc) = Document::from_bytes(pdf(&streams)) else {
        return;
    };
    let Some(spec) = SPECS.get(usize::from(pick & 0x7f) % SPECS.len()) else {
        return;
    };
    let Ok(patch) = ContentPatch::parse(spec) else {
        return;
    };
    let mut settings = OptimizeSettings::new();
    settings.content = Some(patch);
    let mut session = EditSession::new(doc);
    let Ok(plan) = session.plan_optimize(&settings) else {
        return;
    };
    if session.apply_optimize(&plan).is_ok() {
        let _ = session.to_full_bytes(&Default::default());
    }
});

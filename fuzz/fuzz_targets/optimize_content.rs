//! Fuzz target: content-stream compaction in the optimizer (`content(...)`,
//! `pdfcer_core::edit::EditSession::plan_optimize` / `apply_optimize`).
//!
//! The first byte picks the settings; the rest becomes the content stream
//! of a one-page PDF, so the tokenizer, the redundancy walk, the emitter and
//! the re-read check all see arbitrary operators and operands. Planning and
//! applying must never panic, and an applied plan must save.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::optimize::{ContentPatch, OptimizeSettings};

const SPECS: [&str; 6] = [
    "content()",
    "content(whitespace=keep)",
    "content(numbers=keep)",
    "content(redundant=off)",
    "content(whitespace=keep,numbers=keep)",
    "content(whitespace=minimal,redundant=on,numbers=shortest)",
];

fn pdf(content: &[u8]) -> Vec<u8> {
    let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    stream.extend_from_slice(content);
    stream.extend_from_slice(b"\nendstream");
    let bodies: [&[u8]; 4] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>",
        &stream,
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f\r\n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

fuzz_target!(|data: &[u8]| {
    let Some((&pick, content)) = data.split_first() else {
        return;
    };
    let Ok(doc) = Document::from_bytes(pdf(content)) else {
        return;
    };
    let Some(spec) = SPECS.get(usize::from(pick) % SPECS.len()) else {
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

//! `EditSession::format_texts` (request G227): many restyles as one command.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::time::Instant;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditSession};
use pdfcer_core::span::ByteSpan;
use pdfcer_core::text_edit::{FormatBatchError, FormatOptions, FormatRequest};
use pdfcer_core::text_extract::{ExtractOptions, extract_page};
use pdfcer_core::writer::SaveOptions;

fn build_pdf(content: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f\r\n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

/// One line of three absolutely positioned pieces (so reflow shifts of
/// different requests land on the same `Tm`), then `rows` single-word lines.
fn content(rows: usize) -> String {
    let mut s = String::from(
        "BT /F1 12 Tf 1 0 0 1 72 760 Tm (Alpha) Tj 1 0 0 1 120 760 Tm (Beta) Tj \
         1 0 0 1 160 760 Tm (Gamma) Tj ET\n",
    );
    for i in 0..rows {
        let y = 740.0 - 3.0 * i as f64;
        s.push_str(&format!("BT /F1 10 Tf 1 0 0 1 72 {y} Tm (row{i}) Tj ET\n"));
    }
    s
}

fn spans(doc: &Document) -> Vec<ByteSpan> {
    let pages = pdfcer_core::page_tree::pages(doc).unwrap();
    let opts = ExtractOptions::default().with_provenance(true);
    let page = extract_page(doc, &pages[0], 0, &opts).unwrap();
    let mut out: Vec<ByteSpan> = Vec::new();
    for g in page.runs.iter().flat_map(|r| r.glyphs.iter()) {
        if let Some(p) = &g.provenance
            && !out.contains(&p.operator_span)
        {
            out.push(p.operator_span);
        }
    }
    out
}

fn session(rows: usize) -> EditSession {
    EditSession::new(Document::from_bytes(build_pdf(&content(rows))).unwrap())
}

fn saved(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

fn requests(rows: usize) -> Vec<FormatRequest> {
    spans(&Document::from_bytes(build_pdf(&content(rows))).unwrap())
        .into_iter()
        .map(|sp| FormatRequest::whole_operator(0, sp).drawn_size(6.0))
        .collect()
}

/// The result of `format_text` per request in descending span order.
fn sequential(rows: usize) -> (Vec<u8>, u128) {
    let mut s = session(rows);
    let mut reqs = requests(rows);
    reqs.sort_by_key(|r| std::cmp::Reverse(r.pinned_span.unwrap().start));
    let t = Instant::now();
    for (k, r) in reqs.iter().enumerate() {
        s.format_text(r, &FormatOptions::default()).unwrap();
        if k > 0 {
            assert!(s.coalesce_last(2, CommandKind::FormatText));
        }
    }
    (saved(&s), t.elapsed().as_micros())
}

#[test]
fn batch_equals_sequential_and_undoes_in_one_step() {
    let mut s = session(4);
    let base = saved(&s);
    let reqs = requests(4);
    assert_eq!(reqs.len(), 7);
    let batch = s.format_texts(&reqs, &FormatOptions::default()).unwrap();
    assert!(batch.planned_once);
    assert_eq!(batch.reports.len(), 7);
    assert_eq!(batch.reports[1].size_change.map(|c| c.1), Some(6.0));
    assert_eq!(saved(&s), sequential(4).0);
    assert_eq!(s.undo(), Some(CommandKind::FormatText));
    assert_eq!(saved(&s), base);
    assert_eq!(s.undo(), None);
}

#[test]
fn refusal_names_the_request_and_applies_nothing() {
    let mut s = session(3);
    let base = saved(&s);
    let mut reqs = requests(3);
    // A pin on the leading `BT`, which shows no text.
    reqs.insert(
        2,
        FormatRequest::whole_operator(0, ByteSpan { start: 0, len: 2 }).drawn_size(6.0),
    );
    let err = s
        .format_texts(&reqs, &FormatOptions::default())
        .unwrap_err();
    assert_eq!(err.index(), Some(2), "{err}");
    assert!(matches!(err, FormatBatchError::Request { index: 2, .. }));
    assert_eq!(saved(&s), base);
    assert_eq!(s.undo(), None);
}

#[test]
fn empty_and_no_op_requests() {
    let mut s = session(1);
    let batch = s.format_texts(&[], &FormatOptions::default()).unwrap();
    assert!(batch.reports.is_empty());
    assert_eq!(s.undo(), None);
    let mut reqs = requests(1);
    reqs.push(FormatRequest::whole_operator(
        0,
        ByteSpan { start: 0, len: 2 },
    ));
    let err = s
        .format_texts(&reqs, &FormatOptions::default())
        .unwrap_err();
    assert_eq!(err.index(), Some(reqs.len() - 1));
}

#[test]
fn a_resource_creating_request_falls_back_to_one_undo_entry() {
    let mut s = session(2);
    let base = saved(&s);
    let mut reqs = requests(2);
    reqs[0] = reqs[0].clone().fill_alpha(0.5);
    let batch = s.format_texts(&reqs, &FormatOptions::default()).unwrap();
    assert!(!batch.planned_once);
    assert_eq!(batch.reports.len(), reqs.len());
    assert_eq!(s.undo(), Some(CommandKind::FormatText));
    assert_eq!(saved(&s), base);
    assert_eq!(s.undo(), None);
}

#[test]
fn two_hundred_operators_cost_far_less_than_one_call_each() {
    let rows = 220;
    let (seq_bytes, seq_us) = sequential(rows);
    let mut s = session(rows);
    let reqs = requests(rows);
    let t = Instant::now();
    let batch = s.format_texts(&reqs, &FormatOptions::default()).unwrap();
    let batch_us = t.elapsed().as_micros();
    assert!(batch.planned_once);
    assert_eq!(saved(&s), seq_bytes);
    assert!(
        batch_us * 5 < seq_us,
        "batch {batch_us} us vs sequential {seq_us} us"
    );
}

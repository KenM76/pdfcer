//! `coverage_warning_masks` gives each rule the mask a single-rule call gives.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use pdfcer_core::document::Document;
use pdfcer_render::{
    CoverageWarning, RenderOptions, coverage_warning_mask, coverage_warning_masks,
};

/// A 60x60 page: the left half is 50/50/50/100 CMYK (250%, rich black),
/// the right half 100% K alone.
fn halves() -> Document {
    let content = "0.5 0.5 0.5 1 k 0 0 30 60 re f 0 0 0 1 k 30 0 30 60 re f";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60] /Contents 4 0 R >>".to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f\r\n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(buf).unwrap()
}

#[test]
fn each_rule_matches_its_single_rule_mask_across_chunks() {
    let doc = halves();
    let view = doc.view();
    let page = &pdfcer_core::page_tree::pages(&doc).unwrap()[0];
    let opts = RenderOptions::default();
    // Four rules: the fourth lands in a second render.
    let rules = [
        CoverageWarning::TotalAreaCoverage { limit_percent: 200 },
        CoverageWarning::RichBlack {
            black_start_percent: 95,
        },
        CoverageWarning::TotalAreaCoverage { limit_percent: 50 },
        CoverageWarning::TotalAreaCoverage { limit_percent: 300 },
    ];
    let masks = coverage_warning_masks(&view, page, 1.0, None, &opts, &rules).unwrap();
    assert_eq!(masks.len(), rules.len());
    for (mask, &rule) in masks.iter().zip(&rules) {
        let single = coverage_warning_mask(&view, page, 1.0, None, &opts, rule).unwrap();
        assert_eq!(*mask, single, "{rule:?}");
    }
    let half = u64::from(masks[0].width * masks[0].height) / 2;
    assert_eq!(masks[0].flagged, half, "250% over 200%: the left half");
    assert_eq!(masks[1].flagged, half, "rich black: the left half");
    assert_eq!(masks[2].flagged, half * 2, "both halves over 50%");
    assert_eq!(masks[3].flagged, 0, "nothing over 300%");
    assert!(masks[0].is_flagged(5, 5) && !masks[0].is_flagged(50, 5));
}

#[test]
fn no_rules_render_nothing() {
    let doc = halves();
    let page = &pdfcer_core::page_tree::pages(&doc).unwrap()[0];
    let masks =
        coverage_warning_masks(&doc.view(), page, 1.0, None, &RenderOptions::default(), &[])
            .unwrap();
    assert!(masks.is_empty());
}

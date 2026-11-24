//! `TacMeter` measures total area coverage per pixel and bounds the
//! over-limit areas in page space.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::preflight::{CoverageMeter, TacRequest};
use pdfcer_render::{RenderOptions, TacMeter, total_area_coverage};

/// A one-page 144x72 document drawing `content`, with `page_extra` in the
/// page dictionary and `resources` as its resources.
fn page(content: &str, page_extra: &str, resources: &str) -> Document {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 144 72] /Resources {resources} \
             /Contents 4 0 R {page_extra} >>"
        ),
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

fn measure(doc: &Document, limit: f64) -> pdfcer_core::preflight::TacMeasurement {
    let pages = pdfcer_core::page_tree::pages(doc).unwrap();
    TacMeter::new()
        .measure_tac(&doc.view(), &pages[0], &TacRequest::new(limit, 72.0))
        .unwrap()
}

fn close(a: Rect, b: Rect) -> bool {
    let d = [a.llx - b.llx, a.lly - b.lly, a.urx - b.urx, a.ury - b.ury];
    d.iter().all(|v| v.abs() < 1.01)
}

#[test]
fn rich_black_four_solid_plates_reads_400_percent() {
    let doc = page("1 1 1 1 k 10 20 30 20 re f", "", "<< >>");
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let tac = total_area_coverage(&doc.view(), &pages[0], 1.0, &RenderOptions::default()).unwrap();
    assert!(tac.in_ink);
    assert_eq!((tac.width, tac.height), (144, 72));
    assert_eq!(tac.max_tenths(), 4000);
    // Pixel (20, 40) from the top is inside; (100, 10) is bare paper.
    assert_eq!(tac.tenths[40 * 144 + 20], 4000);
    assert_eq!(tac.tenths[10 * 144 + 100], 0);
}

#[test]
fn an_area_over_the_limit_is_bounded_in_page_space() {
    let doc = page("1 1 1 1 k 10 20 30 20 re f", "", "<< >>");
    let m = measure(&doc, 300.0);
    assert!((m.max_percent - 400.0).abs() < 0.05);
    assert_eq!(m.over_pixels, 30 * 20);
    assert!((m.ppi - 72.0).abs() < f64::EPSILON);
    assert_eq!(m.regions.len(), 1);
    let r = m.regions[0];
    assert!(
        close(r.rect, Rect::from_corners(10.0, 20.0, 40.0, 40.0)),
        "{:?}",
        r.rect
    );
    assert_eq!((r.over_pixels, r.merged), (600, 1));
    assert!(m.caveats.is_empty() && m.not_measured.is_empty(), "{m:?}");
}

#[test]
fn exactly_at_the_limit_is_not_over() {
    let doc = page("1 1 1 0 k 0 0 144 72 re f", "", "<< >>");
    let m = measure(&doc, 300.0);
    assert!((m.max_percent - 300.0).abs() < 0.05);
    assert_eq!(m.over_pixels, 0);
    assert!(m.regions.is_empty());
}

#[test]
fn a_rotated_page_bounds_the_area_where_it_is_painted() {
    let doc = page("1 1 1 1 k 10 20 30 20 re f", "/Rotate 90", "<< >>");
    let m = measure(&doc, 300.0);
    assert_eq!(m.regions.len(), 1);
    assert!(
        close(
            m.regions[0].rect,
            Rect::from_corners(10.0, 20.0, 40.0, 40.0)
        ),
        "{:?}",
        m.regions[0].rect
    );
}

#[test]
fn separate_areas_are_separate_regions_largest_first() {
    let doc = page("1 1 1 1 k 4 4 20 20 re f 100 40 30 20 re f", "", "<< >>");
    let m = measure(&doc, 300.0);
    assert_eq!(m.regions.len(), 2);
    assert_eq!(m.regions[0].over_pixels, 600);
    assert_eq!(m.regions[1].over_pixels, 400);
}

#[test]
fn a_spot_plane_counts_toward_the_total() {
    let resources = "<< /ColorSpace << /CS0 [/Separation /Varnish /DeviceCMYK \
         << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0 0 0] /N 1 >>] >> \
         /ExtGState << /OP << /Type /ExtGState /OP true /op true /OPM 1 >> >> >>";
    let content = "1 1 1 0 k 0 0 144 72 re f /OP gs /CS0 cs 1 scn 0 0 144 72 re f";
    let doc = page(content, "", resources);
    let m = measure(&doc, 300.0);
    assert!((m.max_percent - 400.0).abs() < 0.05, "{m:?}");
    assert_eq!(m.over_pixels, 144 * 72);
}

#[test]
fn rgb_content_is_disclosed_by_preflight() {
    use pdfcer_core::preflight::{Check, FindingKind, Profile, RunOptions, run_with_meter};
    let doc = page("0 0 0 rg 0 0 144 72 re f", "", "<< >>");
    let profile = Profile::from_json(
        br#"{"name": "tac", "rules": [{"id": "tac", "severity": "error",
            "check": "total-area-coverage", "limit_percent": 300, "ppi": 72}]}"#,
    )
    .unwrap();
    assert!(matches!(
        profile.rules[0].check,
        Check::TotalAreaCoverage { .. }
    ));
    let report = run_with_meter(
        &doc.view(),
        &profile,
        &RunOptions::default(),
        Some(&TacMeter::new()),
    );
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.kind == FindingKind::NeedsPerson && f.message.contains("convert-colours")),
        "{:?}",
        report.findings
    );
}

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)] // tests

use super::*;
use crate::document::Document;
use crate::page_tree;
use crate::pageops::tests_support::build_pdf_bytes;
use crate::preflight::{Check, FindingKind, MeasureUnit, Profile, ProfileRule, Severity, run};

/// Page objects: 0 is 0.1 wide at scale 1; 1 is 0.4 wide under a 3-by-0.5
/// scale (0.2 on its thin axis, 0.49 by the geometric mean); 2 is 1 wide; 3 is
/// width 0. Leaf 0 is 0.05 wide in `Fm0`, drawn at scale 2. Object 6 is never
/// touched.
pub(crate) fn fixture() -> Vec<u8> {
    let page = "0.1 w 0 0 20 20 re S\n\
                q 3 0 0 0.5 30 0 cm 0.4 w 0 0 5 5 re S Q\n\
                1 w 50 50 10 10 re S\n\
                0 w 70 70 5 5 re S\n\
                q 2 0 0 2 10 50 cm /Fm0 Do Q\n";
    let fm0 = "0.05 w 0 0 5 5 re S\n";
    let stream = |dict: &str, body: &str| {
        format!(
            "<< {dict} /Length {} >>\nstream\n{body}endstream",
            body.len()
        )
    };
    let content = stream("", page);
    let form = stream("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", fm0);
    build_pdf_bytes(&[
        (1, "<< /Type /Catalog /Pages 2 0 R /Untouched 6 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject \
             << /Fm0 5 0 R >> >> /Contents 4 0 R >>",
        ),
        (4, &content),
        (5, &form),
        (6, "<< /Note (left alone) >>"),
    ])
}

fn found(min: f64) -> Vec<Hairline> {
    let doc = Document::from_bytes(fixture()).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    hairlines(&doc.view(), &pages[0], min).unwrap()
}

#[test]
fn measures_the_thinnest_axis_through_the_ctm() {
    let h = found(0.25);
    let targets: Vec<FindingTarget> = h.iter().map(|h| h.target).collect();
    assert_eq!(
        targets,
        vec![
            FindingTarget::PageObject(0),
            FindingTarget::PageObject(1),
            FindingTarget::PageObject(3),
            FindingTarget::FormLeaf(0),
        ]
    );
    assert!((h[0].width_pt - 0.1).abs() < 1e-9);
    assert!((h[1].axis_scale - 0.5).abs() < 1e-9 && (h[1].width_pt - 0.2).abs() < 1e-9);
    assert!(h[2].zero_width && h[2].width_pt == 0.0);
    assert!((h[3].width_pt - 0.1).abs() < 1e-9);
    assert!((h[1].width_for(0.25) - 0.5).abs() < 1e-9);
}

#[test]
fn a_zero_minimum_still_finds_width_zero() {
    let h = found(0.0);
    assert_eq!(h.len(), 1);
    assert!(h[0].zero_width);
}

#[test]
fn min_axis_scale_is_the_smaller_singular_value() {
    let m = |a, b, c, d| crate::vector::Matrix {
        a,
        b,
        c,
        d,
        e: 0.0,
        f: 0.0,
    };
    assert!((min_axis_scale(m(3.0, 0.0, 0.0, 0.5)) - 0.5).abs() < 1e-12);
    assert!((min_axis_scale(m(0.0, 2.0, -2.0, 0.0)) - 2.0).abs() < 1e-12);
    assert_eq!(min_axis_scale(m(1.0, 0.0, 0.0, 0.0)), 0.0);
}

#[test]
fn stroke_below_reports_points_against_the_object() {
    let doc = Document::from_bytes(fixture()).unwrap();
    let profile = Profile {
        name: "t".into(),
        description: String::new(),
        rules: vec![ProfileRule::new(
            "thin",
            Check::StrokeBelow { min_width_pt: 0.25 },
            Severity::Warning,
        )],
    };
    let report = run(&doc.view(), &profile);
    assert_eq!(report.findings.len(), 4);
    let f = &report.findings[1];
    assert_eq!(f.kind, FindingKind::Violation);
    assert_eq!(f.target, FindingTarget::PageObject(1));
    assert_eq!(f.page, Some(1));
    assert!(f.rect.is_some());
    let m = f.measured.unwrap();
    assert_eq!(m.unit, MeasureUnit::Points);
    assert!((m.value - 0.2).abs() < 1e-9 && (m.limit - 0.25).abs() < 1e-12);
    assert_eq!(report.findings[3].target, FindingTarget::FormLeaf(0));
}

#[test]
fn stroke_below_round_trips_and_refuses_a_negative_minimum() {
    let p = Profile {
        name: "t".into(),
        description: String::new(),
        rules: vec![ProfileRule::new(
            "thin",
            Check::StrokeBelow {
                min_width_pt: 0.125,
            },
            Severity::Error,
        )],
    };
    assert_eq!(Profile::from_json(p.to_json().as_bytes()).unwrap(), p);
    let bad = br#"{"name":"x","rules":[{"id":"r","severity":"info","check":"stroke-below","min_width_pt":-1}]}"#;
    assert!(Profile::from_json(bad).is_err());
    let missing = br#"{"name":"x","rules":[{"id":"r","severity":"info","check":"stroke-below"}]}"#;
    assert!(Profile::from_json(missing).is_err());
}

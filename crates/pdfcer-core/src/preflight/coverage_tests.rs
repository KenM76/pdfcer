#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)] // tests

use std::cell::Cell;

use super::*;
use crate::document::Document;
use crate::pageops::tests_support::{build_pdf, stream};
use crate::preflight::{PreflightReport, RunOptions, run_with, run_with_meter};

fn page_drawing(content: &str) -> Document {
    let content = stream("", content);
    build_pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>",
        ),
        (4, &content),
    ])
}

fn tac_profile() -> Profile {
    Profile {
        name: "tac".into(),
        description: String::new(),
        rules: vec![ProfileRule::new(
            "tac",
            Check::TotalAreaCoverage {
                limit_percent: 300.0,
                ppi: 72.0,
            },
            Severity::Error,
        )],
    }
}

/// Reports a fixed measurement and remembers the request.
struct Fixed(TacMeasurement, Cell<Option<TacRequest>>);

impl CoverageMeter for Fixed {
    fn measure_tac(
        &self,
        _: &DocumentView<'_>,
        _: &Page,
        request: &TacRequest,
    ) -> Result<TacMeasurement, TacFailure> {
        self.1.set(Some(*request));
        Ok(self.0.clone())
    }
}

struct Refuses;

impl CoverageMeter for Refuses {
    fn measure_tac(
        &self,
        _: &DocumentView<'_>,
        _: &Page,
        _: &TacRequest,
    ) -> Result<TacMeasurement, TacFailure> {
        Err(TacFailure::new("out of memory"))
    }
}

fn over() -> TacMeasurement {
    let rect = Rect::from_corners(10.0, 10.0, 20.0, 30.0);
    TacMeasurement {
        max_percent: 400.0,
        pixels: 10_000,
        over_pixels: 200,
        ppi: 72.0,
        regions: vec![TacRegion::new(rect, 400.0, 200)],
        ..TacMeasurement::default()
    }
}

fn measured(content: &str, m: TacMeasurement) -> (PreflightReport, Option<TacRequest>) {
    let doc = page_drawing(content);
    let meter = Fixed(m, Cell::new(None));
    let report = run_with_meter(
        &doc.view(),
        &tac_profile(),
        &RunOptions::default(),
        Some(&meter),
    );
    (report, meter.1.get())
}

#[test]
fn without_a_meter_the_rule_is_incomplete_not_a_pass() {
    let doc = page_drawing("1 1 1 1 k 0 0 10 10 re f");
    let report = run_with(&doc.view(), &tac_profile(), &RunOptions::default());
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].kind, FindingKind::Incomplete);
    assert!(report.fails(), "an error-severity incomplete fails the run");
}

#[test]
fn a_region_over_the_limit_is_a_violation_with_its_rect_and_measure() {
    let (report, asked) = measured("1 1 1 1 k 0 0 10 10 re f", over());
    let asked = asked.expect("the meter was asked");
    assert_eq!((asked.limit_percent, asked.ppi), (300.0, 72.0));
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    let f = &report.findings[0];
    assert_eq!(f.kind, FindingKind::Violation);
    assert_eq!(f.page, Some(1));
    assert_eq!(f.rect, Some(Rect::from_corners(10.0, 10.0, 20.0, 30.0)));
    let m = f.measured.expect("measured");
    assert_eq!(
        (m.value, m.limit, m.unit),
        (400.0, 300.0, MeasureUnit::Percent)
    );
    assert!(f.message.contains("72 ppi"), "{}", f.message);
    assert!(report.fails());
}

#[test]
fn a_folded_region_says_how_many_areas_it_bounds() {
    let mut m = over();
    m.regions[0] = m.regions[0].with_merged(3);
    let (report, _) = measured("1 1 1 1 k 0 0 10 10 re f", m);
    assert!(
        report.findings[0].message.contains("3 smaller areas"),
        "{}",
        report.findings[0].message
    );
}

#[test]
fn a_clean_measurement_is_a_pass() {
    let clean = TacMeasurement {
        max_percent: 100.0,
        ppi: 72.0,
        ..TacMeasurement::default()
    };
    let (report, _) = measured("0 0 0 1 k 0 0 10 10 re f", clean);
    assert!(report.is_clean(), "{:?}", report.findings);
}

#[test]
fn a_refusing_meter_is_incomplete() {
    let doc = page_drawing("1 1 1 1 k 0 0 10 10 re f");
    let report = run_with_meter(
        &doc.view(),
        &tac_profile(),
        &RunOptions::default(),
        Some(&Refuses),
    );
    assert_eq!(report.findings[0].kind, FindingKind::Incomplete);
    assert!(report.findings[0].message.contains("out of memory"));
}

#[test]
fn unmeasured_content_is_incomplete_beside_the_violation() {
    let mut partial = over();
    partial.not_measured.push("2 images".into());
    let (report, _) = measured("1 1 1 1 k 0 0 10 10 re f", partial);
    let kinds: Vec<_> = report.findings.iter().map(|f| f.kind).collect();
    assert_eq!(kinds, [FindingKind::Violation, FindingKind::Incomplete]);
}

#[test]
fn rgb_content_is_disclosed_as_an_estimate() {
    let clean = TacMeasurement {
        max_percent: 200.0,
        ppi: 72.0,
        ..TacMeasurement::default()
    };
    let (report, _) = measured("1 0 0 rg 0 0 10 10 re f", clean);
    assert_eq!(report.findings.len(), 1);
    let f = &report.findings[0];
    assert_eq!(
        (f.kind, f.severity),
        (FindingKind::NeedsPerson, Severity::Info)
    );
    assert!(f.message.contains("convert-colours"), "{}", f.message);
    assert!(!report.fails(), "a disclosure alone does not fail the run");
}

#[test]
fn the_rule_round_trips_through_json_and_validates_ppi() {
    let profile = tac_profile();
    let back = Profile::from_json(profile.to_json().as_bytes()).unwrap();
    assert_eq!(back, profile);
    let defaulted = br#"{"name": "x", "rules": [{"id": "t", "severity": "error",
        "check": "total-area-coverage", "limit_percent": 320}]}"#;
    let p = Profile::from_json(defaulted).unwrap();
    assert_eq!(
        p.rules[0].check,
        Check::TotalAreaCoverage {
            limit_percent: 320.0,
            ppi: TacRequest::DEFAULT_PPI
        }
    );
    let bad = br#"{"name": "x", "rules": [{"id": "t", "severity": "error",
        "check": "total-area-coverage", "limit_percent": 300, "ppi": 5000}]}"#;
    assert!(Profile::from_json(bad).is_err());
    let missing = br#"{"name": "x", "rules": [{"id": "t", "severity": "error",
        "check": "total-area-coverage"}]}"#;
    assert!(Profile::from_json(missing).is_err());
}

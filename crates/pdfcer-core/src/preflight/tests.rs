#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)] // tests

use std::sync::atomic::AtomicBool;

use super::*;
use crate::document::Document;
use crate::pageops::tests_support::{build_pdf, stream};

/// One page drawing `content` with `resources`; extra objects from 5.
fn one_page(page_extra: &str, resources: &str, content: &str, extra: &[(u32, String)]) -> Document {
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources {resources} /Contents 4 0 R {page_extra} >>"
    );
    let content = stream("", content);
    let mut objects: Vec<(u32, &str)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, &page),
        (4, &content),
    ];
    objects.extend(extra.iter().map(|(n, b)| (*n, b.as_str())));
    build_pdf(&objects)
}

fn rule(id: &str, check: Check) -> ProfileRule {
    ProfileRule::new(id, check, Severity::Error)
}

fn profile(rules: Vec<ProfileRule>) -> Profile {
    Profile {
        name: "test".into(),
        description: String::new(),
        rules,
    }
}

fn ids(report: &PreflightReport) -> Vec<&str> {
    report.findings.iter().map(|f| f.rule_id.as_str()).collect()
}

#[test]
fn digital_printing_finds_each_kind_of_defect() {
    let image = stream(
        "/Type /XObject /Subtype /Image /Width 100 /Height 100 /ColorSpace /DeviceGray /BitsPerComponent 8",
        "x",
    );
    let doc = one_page(
        "",
        "<< /Font << /F1 6 0 R >> /XObject << /Im 5 0 R >> /ExtGState << /G 7 0 R >> >>",
        "/G gs 1 0 0 rg 0 0 10 10 re f q 200 0 0 200 0 0 cm /Im Do Q BT /F1 12 Tf (a) Tj ET",
        &[
            (5, image),
            (
                6,
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
            ),
            (7, "<< /Type /ExtGState /OP true >>".into()),
        ],
    );
    let report = run(&doc.view(), &Profile::digital_printing());
    let found = ids(&report);
    for id in [
        "font-not-embedded",
        "image-low-ppi",
        "rgb-colour",
        "overprint",
        "no-trim-box",
    ] {
        assert!(found.contains(&id), "{id} missing from {found:?}");
    }
    assert!(!found.contains(&"encrypted"));
    assert!(report.fails());

    let ppi = report
        .findings
        .iter()
        .find(|f| f.rule_id == "image-low-ppi")
        .unwrap();
    let m = ppi.measured.unwrap();
    assert!((m.value - 36.0).abs() < 1e-9, "{m:?}");
    assert_eq!(m.limit, 150.0);
    assert!(matches!(ppi.target, FindingTarget::PageObject(_)));
    let r = ppi.rect.unwrap();
    assert!((r.urx - 200.0).abs() < 1e-9 && (r.ury - 200.0).abs() < 1e-9);

    let font = report
        .findings
        .iter()
        .find(|f| f.rule_id == "font-not-embedded")
        .unwrap();
    assert_eq!(
        font.target,
        FindingTarget::Font(crate::object::ObjId::new(6, 0))
    );
    assert_eq!(font.page, Some(1));
    let op = report
        .findings
        .iter()
        .find(|f| f.rule_id == "overprint")
        .unwrap();
    assert_eq!(
        op.target,
        FindingTarget::GraphicsState(crate::object::ObjId::new(7, 0))
    );
}

#[test]
fn a_sharp_image_and_a_trimmed_page_pass() {
    let image = stream(
        "/Type /XObject /Subtype /Image /Width 600 /Height 600 /ColorSpace /DeviceCMYK /BitsPerComponent 8",
        "x",
    );
    let doc = one_page(
        "/TrimBox [10 10 602 782]",
        "<< /XObject << /Im 5 0 R >> >>",
        "q 144 0 0 144 0 0 cm /Im Do Q",
        &[(5, image)],
    );
    let report = run(&doc.view(), &Profile::digital_printing());
    assert!(report.is_clean(), "{:?}", report.findings);
}

#[test]
fn type3_fonts_are_never_unembedded() {
    let doc = one_page(
        "/TrimBox [0 0 612 792]",
        "<< /Font << /F1 5 0 R >> >>",
        "BT /F1 1 Tf (a) Tj ET",
        &[(
            5,
            "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] /CharProcs << >> /Encoding << /Differences [] >> /FirstChar 0 /LastChar 0 /Widths [0] >>".into(),
        )],
    );
    let report = run(
        &doc.view(),
        &profile(vec![rule("f", Check::FontNotEmbedded)]),
    );
    assert!(report.is_clean(), "{:?}", report.findings);
}

#[test]
fn spots_skip_process_all_and_none_and_the_allowed_list() {
    let doc = one_page(
        "",
        "<< /ColorSpace << /S1 [/Separation /Gold /DeviceCMYK 5 0 R] /S2 [/DeviceN [/Cyan /Silver /None] /DeviceCMYK 5 0 R] /S3 [/Separation /All /DeviceCMYK 5 0 R] >> >>",
        "/S1 cs 1 scn /S2 CS 1 1 1 SCN /S3 cs 1 scn",
        &[(
            5,
            "<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 1 1 1] /N 1 >>".into(),
        )],
    );
    let check = Check::SpotNotAllowed {
        allowed: vec!["Gold".into()],
    };
    let report = run(&doc.view(), &profile(vec![rule("spot", check)]));
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    assert!(report.findings[0].message.contains("\"Silver\""));
}

#[test]
fn annotation_appearances_are_walked_and_targeted() {
    let ap = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 10 10]",
        "0 0 0 1 k 0 0 5 5 re f",
    );
    let doc = one_page(
        "/Annots [5 0 R]",
        "<< >>",
        "",
        &[
            (
                5,
                "<< /Type /Annot /Subtype /Square /Rect [0 0 10 10] /AP << /N 6 0 R >> >>".into(),
            ),
            (6, ap),
        ],
    );
    let check = Check::ColourSpaceUsed {
        families: vec![ColourFamily::DeviceCmyk],
    };
    let report = run(&doc.view(), &profile(vec![rule("cmyk", check)]));
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    assert_eq!(
        report.findings[0].target,
        FindingTarget::Annotation(crate::object::ObjId::new(5, 0))
    );
}

#[test]
fn a_form_that_draws_itself_terminates() {
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /XObject << /Me 5 0 R >> >>",
        "0 1 0 rg /Me Do",
    );
    let doc = one_page("", "<< /XObject << /Me 5 0 R >> >>", "/Me Do", &[(5, form)]);
    let check = Check::ColourSpaceUsed {
        families: vec![ColourFamily::DeviceRgb],
    };
    let report = run(&doc.view(), &profile(vec![rule("rgb", check)]));
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
}

#[test]
fn indexed_and_icc_report_their_base() {
    let icc = stream("/N 4", "x");
    let doc = one_page(
        "",
        "<< /ColorSpace << /I [/Indexed 5 0 R 0 <00000000>] >> >>",
        "/I cs 0 sc",
        &[(5, "[/ICCBased 6 0 R]".to_owned()), (6, icc)],
    );
    let check = Check::ColourSpaceUsed {
        families: vec![ColourFamily::IccCmyk],
    };
    let report = run(&doc.view(), &profile(vec![rule("icc", check)]));
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
}

#[test]
fn unreadable_content_is_incomplete_never_a_pass() {
    let content = stream("/Filter /FlateDecode", "not flate");
    let doc = build_pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> /Contents 4 0 R >>",
        ),
        (4, &content),
    ]);
    let check = Check::ColourSpaceUsed {
        families: vec![ColourFamily::DeviceRgb],
    };
    let report = run(&doc.view(), &profile(vec![rule("rgb", check)]));
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    assert_eq!(report.findings[0].kind, FindingKind::Incomplete);
    assert!(!report.is_clean());
}

#[test]
fn the_cap_counts_what_it_drops_and_cancel_stops() {
    let doc = one_page("", "<< >>", "", &[]);
    let p = profile(vec![
        rule("a", Check::NoTrimBox),
        rule("b", Check::NoOutputIntent),
        rule("c", Check::NoTrimBox),
    ]);
    let capped = run_with(
        &doc.view(),
        &p,
        &RunOptions {
            max_findings: 1,
            ..RunOptions::default()
        },
    );
    assert_eq!((capped.findings.len(), capped.dropped), (1, 2));

    let cancel = AtomicBool::new(true);
    let stopped = run_with(
        &doc.view(),
        &p,
        &RunOptions {
            cancel: Some(&cancel),
            ..RunOptions::default()
        },
    );
    assert!(stopped.cancelled);
    assert_eq!(stopped.pages_checked, 0);
    assert_eq!(ids(&stopped), ["b"]);
}

#[test]
fn version_above_measures_the_header() {
    let doc = one_page("", "<< >>", "", &[]);
    let p = profile(vec![rule(
        "v",
        Check::VersionAbove {
            max: crate::PdfVersion { major: 1, minor: 4 },
        },
    )]);
    let report = run(&doc.view(), &p);
    let m = report.findings[0].measured.unwrap();
    assert!((m.value - 1.7).abs() < 1e-9 && (m.limit - 1.4).abs() < 1e-9);
}

#[test]
fn profiles_round_trip_through_json() {
    let mut p = Profile::digital_printing();
    p.rules.push(rule(
        "spots",
        Check::SpotNotAllowed {
            allowed: vec!["PANTONE \"1\"".into()],
        },
    ));
    p.rules.push(rule(
        "v",
        Check::VersionAbove {
            max: crate::PdfVersion { major: 1, minor: 6 },
        },
    ));
    p.rules.push(rule("oi", Check::NoOutputIntent));
    let back = Profile::from_json(p.to_json().as_bytes()).expect("parses");
    assert_eq!(back, p);
}

#[test]
fn bad_profiles_are_refused_by_name() {
    let unknown = br#"{"name":"x","rules":[{"id":"r","severity":"error","check":"teleport"}]}"#;
    assert!(matches!(
        Profile::from_json(unknown),
        Err(ProfileError::UnknownCheck { .. })
    ));
    let dup = br#"{"name":"x","rules":[{"id":"r","severity":"info","check":"encrypted"},{"id":"r","severity":"info","check":"encrypted"}]}"#;
    assert_eq!(
        Profile::from_json(dup),
        Err(ProfileError::DuplicateId("r".into()))
    );
    let ppi = br#"{"name":"x","rules":[{"id":"r","severity":"info","check":"image-below-ppi","min_ppi":-1}]}"#;
    assert!(matches!(
        Profile::from_json(ppi),
        Err(ProfileError::BadValue { .. })
    ));
    assert!(matches!(
        Profile::from_json(b"{"),
        Err(ProfileError::Json(_))
    ));
    for name in Profile::BUILTIN_NAMES {
        assert!(Profile::builtin(name).is_some());
    }
}

/// Every `Error` finding as `rule: message`, for a readable assertion.
pub(crate) fn error_lines(report: &PreflightReport) -> Vec<String> {
    report
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .map(|f| format!("{}: {}", f.rule_id, f.message))
        .collect()
}

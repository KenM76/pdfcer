#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)] // tests

use super::*;
use crate::document::Document;
use crate::pageops::tests_support::{build_pdf, build_pdf_bytes, stream};
use crate::preflight::tests::error_lines;

const XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:pdfxid="http://www.npes.org/pdfx/ns/id/" pdfxid:GTS_PDFXVersion="PDF/X-4"/></rdf:RDF></x:xmpmeta>"#;

/// The parts of a one-page test file.
struct Parts<'a> {
    catalog: &'a str,
    page: &'a str,
    resources: &'a str,
    content: &'a str,
    info: &'a str,
    extra: Vec<(u32, String)>,
}

impl Default for Parts<'_> {
    fn default() -> Self {
        Self {
            catalog: "/OutputIntents [7 0 R] /Metadata 9 0 R",
            page: "/TrimBox [10 10 602 782]",
            resources: "<< >>",
            content: "0 0 0 1 k 0 0 10 10 re f",
            info: "/GTS_PDFXVersion (PDF/X-4) /Title (t) /CreationDate (D:20260101) \
                   /ModDate (D:20260101) /Trapped /False",
            extra: Vec::new(),
        }
    }
}

fn icc(class: &str) -> String {
    stream("", &format!("000000000000{class}0000"))
}

fn bytes(parts: &Parts<'_>) -> Vec<u8> {
    let catalog = format!("<< /Type /Catalog /Pages 2 0 R {} >>", parts.catalog);
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources {} /Contents 4 0 R {} >>",
        parts.resources, parts.page
    );
    let content = stream("", parts.content);
    let info = format!("<< {} >>", parts.info);
    let profile = icc("prtr");
    let xmp = stream("/Type /Metadata /Subtype /XML", XMP);
    let mut objects: Vec<(u32, &str)> = vec![
        (1, &catalog),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, &page),
        (4, &content),
        (6, &info),
        (
            7,
            "<< /Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier (FOGRA39) /DestOutputProfile 8 0 R >>",
        ),
        (8, &profile),
        (9, &xmp),
    ];
    objects.extend(parts.extra.iter().map(|(n, b)| (*n, b.as_str())));
    let raw = build_pdf_bytes(&objects);
    let text = String::from_utf8_lossy(&raw).replace("/Root 1 0 R", "/Root 1 0 R /Info 6 0 R");
    text.into_bytes()
}

fn doc(parts: &Parts<'_>) -> Document {
    Document::from_bytes(bytes(parts)).expect("fixture loads")
}

fn findings<'r>(report: &'r PdfxReport, row: &str) -> Vec<&'r Finding> {
    report
        .preflight
        .findings
        .iter()
        .filter(|f| f.rule_id == row)
        .collect()
}

#[test]
fn a_well_formed_x4_file_has_no_error_but_is_not_confirmed() {
    let report = validate_pdfx(&doc(&Parts::default()).view(), PdfxLevel::X4);
    assert_eq!(error_lines(&report.preflight), Vec::<String>::new());
    assert_eq!(report.verdict(), PdfxVerdict::Unconfirmed);
    assert!(report.claim.is_some());
}

#[test]
fn rgb_fails_x1a_with_its_clause_but_not_x4() {
    let parts = Parts {
        content: "1 0 0 rg 0 0 10 10 re f",
        ..Parts::default()
    };
    let d = doc(&parts);
    let report = validate_pdfx(&d.view(), PdfxLevel::X1a2001);
    let cs = findings(&report, "PXQ-CS-1");
    assert!(!cs.is_empty());
    assert_eq!(cs[0].severity, Severity::Error);
    assert_eq!(cs[0].page, Some(1));
    assert!(
        cs[0].message.contains("ISO 15930-1:2001 §6.3.1"),
        "{}",
        cs[0].message
    );
    assert_eq!(report.verdict(), PdfxVerdict::Fails);
    assert!(findings(&validate_pdfx(&d.view(), PdfxLevel::X4), "PXQ-CS-1").is_empty());
}

#[test]
fn lab_fails_x1a_2003() {
    let parts = Parts {
        resources: "<< /ColorSpace << /L [/Lab << /WhitePoint [0.95 1 1.09] >>] >> >>",
        content: "/L cs 50 0 0 sc 0 0 10 10 re f",
        ..Parts::default()
    };
    let report = validate_pdfx(&doc(&parts).view(), PdfxLevel::X1a2003);
    assert!(!findings(&report, "PXQ-CS-1").is_empty());
}

#[test]
fn transparency_fails_x1a_2003_and_x3_2002_but_not_x4() {
    let parts = Parts {
        resources: "<< /ExtGState << /G 10 0 R >> >>",
        content: "/G gs 0 0 0 1 k 0 0 10 10 re f",
        extra: vec![(10, "<< /Type /ExtGState /ca 0.5 >>".into())],
        ..Parts::default()
    };
    let d = doc(&parts);
    let x1a = validate_pdfx(&d.view(), PdfxLevel::X1a2003);
    assert!(!findings(&x1a, "PXQ-TR-1").is_empty());
    let x3 = validate_pdfx(&d.view(), PdfxLevel::X3_2002);
    let tr = findings(&x3, "PXQ-TR-2");
    assert!(!tr.is_empty());
    assert!(
        tr[0].message.contains("ISO 15930-3:2002 Annex D"),
        "{}",
        tr[0].message
    );
    let x4 = validate_pdfx(&d.view(), PdfxLevel::X4);
    assert!(findings(&x4, "PXQ-TR-1").is_empty() && findings(&x4, "PXQ-TR-2").is_empty());
}

#[test]
fn a_missing_output_intent_fails_and_a_wrong_kind_is_named() {
    let none = Parts {
        catalog: "/Metadata 9 0 R",
        ..Parts::default()
    };
    let report = validate_pdfx(&doc(&none).view(), PdfxLevel::X4);
    assert_eq!(findings(&report, "PXQ-OI-1").len(), 1);
    let wrong = Parts {
        catalog: "/OutputIntents [10 0 R] /Metadata 9 0 R",
        extra: vec![(10, "<< /Type /OutputIntent /S /GTS_PDFA1 >>".into())],
        ..Parts::default()
    };
    let report = validate_pdfx(&doc(&wrong).view(), PdfxLevel::X4);
    let oi = findings(&report, "PXQ-OI-1");
    assert_eq!(oi.len(), 1);
    assert!(oi[0].message.contains("GTS_PDFA1"), "{}", oi[0].message);
}

#[test]
fn a_display_profile_fails_x3_2002() {
    let parts = Parts {
        catalog: "/OutputIntents [10 0 R]",
        info: "/GTS_PDFXVersion (PDF/X-3:2002) /GTS_PDFXConformance (PDF/X-3:2002) /Title (t) \
               /CreationDate (D:20260101) /ModDate (D:20260101) /Trapped /False",
        extra: vec![
            (
                10,
                "<< /Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier (x) /DestOutputProfile 11 0 R >>"
                    .into(),
            ),
            (11, icc("mntr")),
        ],
        ..Parts::default()
    };
    let report = validate_pdfx(&doc(&parts).view(), PdfxLevel::X3_2002);
    assert_eq!(findings(&report, "PXQ-OI-8").len(), 1);
}

#[test]
fn an_unembedded_font_fails() {
    let parts = Parts {
        resources: "<< /Font << /F1 10 0 R >> >>",
        content: "BT /F1 12 Tf (a) Tj ET",
        extra: vec![(
            10,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        )],
        ..Parts::default()
    };
    let report = validate_pdfx(&doc(&parts).view(), PdfxLevel::X4);
    let ft = findings(&report, "PXQ-FT-1");
    assert!(!ft.is_empty());
    assert!(
        ft[0].message.contains("ISO 15930-7:2010 §6.5"),
        "{}",
        ft[0].message
    );
}

#[test]
fn a_page_without_trim_or_art_box_fails() {
    let parts = Parts {
        page: "",
        ..Parts::default()
    };
    let report = validate_pdfx(&doc(&parts).view(), PdfxLevel::X1a2003);
    assert_eq!(findings(&report, "PXQ-BX-2").len(), 1);
}

#[test]
fn encryption_fails() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/encryption/enc-emptyuser.pdf"
    );
    let d = Document::from_bytes(std::fs::read(path).unwrap()).unwrap();
    let report = validate_pdfx(&d.view(), PdfxLevel::X4);
    assert_eq!(findings(&report, "PXQ-EN-1").len(), 1);
}

#[test]
fn an_annotation_inside_the_trim_reports_whether_it_prints() {
    let parts = Parts {
        page: "/TrimBox [10 10 602 782] /BleedBox [0 0 612 792] /Annots [10 0 R 11 0 R]",
        extra: vec![
            (
                10,
                "<< /Type /Annot /Subtype /Square /Rect [50 50 100 100] /F 4 >>".into(),
            ),
            (
                11,
                "<< /Type /Annot /Subtype /Text /Rect [60 60 70 70] >>".into(),
            ),
        ],
        ..Parts::default()
    };
    let d = doc(&parts);
    let report = validate_pdfx(&d.view(), PdfxLevel::X1a2003);
    let an = findings(&report, "PXQ-AN-1");
    assert_eq!(an.len(), 2);
    assert!(an[0].message.contains("set to print"));
    assert!(an[1].message.contains("not set to print"));
    assert!(matches!(an[0].target, FindingTarget::Annotation(_)));
    assert!(an[0].rect.is_some());
    // PDF/X-4 grade K: the finding is a warning, its geometry disclosed.
    let x4 = validate_pdfx(&d.view(), PdfxLevel::X4);
    let an = findings(&x4, "PXQ-AN-1");
    assert_eq!(an.len(), 2);
    assert_eq!(an[0].severity, Severity::Warning);
    assert!(an[0].message.contains("NOT SOURCED"));
}

/// The ceiling is a recommendation in the inventory, so a warning.
#[test]
fn a_jpx_image_exceeds_the_pdf_1_3_ceiling_of_x1a_2001() {
    let parts = Parts {
        resources: "<< /XObject << /Im 10 0 R >> >>",
        content: "q 10 0 0 10 0 0 cm /Im Do Q",
        extra: vec![(
            10,
            stream(
                "/Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode",
                "x",
            ),
        )],
        ..Parts::default()
    };
    let d = doc(&parts);
    let report = validate_pdfx(&d.view(), PdfxLevel::X1a2001);
    let st = findings(&report, "PXQ-ST-8");
    assert!(st.iter().any(|f| f.message.contains("JPEG 2000")
        && f.severity == Severity::Warning
        && matches!(f.target, FindingTarget::Image(_))));
    let x4 = validate_pdfx(&d.view(), PdfxLevel::X4);
    assert!(
        findings(&x4, "PXQ-ST-8")
            .iter()
            .all(|f| f.severity == Severity::Info)
    );
}

#[test]
fn a_question_for_a_person_is_a_warning_never_a_pass_or_an_error() {
    let parts = Parts {
        resources: "<< /XObject << /Im 10 0 R >> >>",
        content: "q 10 0 0 10 0 0 cm /Im Do Q",
        extra: vec![(
            10,
            stream(
                "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
                 /BitsPerComponent 8 /Filter /DCTDecode",
                "x",
            ),
        )],
        ..Parts::default()
    };
    let report = validate_pdfx(&doc(&parts).view(), PdfxLevel::X1a2003);
    let fl = findings(&report, "PXQ-FL-4");
    assert_eq!(fl.len(), 1);
    assert_eq!(fl[0].kind, FindingKind::NeedsPerson);
    assert_eq!(fl[0].severity, Severity::Warning);
}

#[test]
fn an_unsourced_row_is_disclosed_at_info() {
    let report = validate_pdfx(&doc(&Parts::default()).view(), PdfxLevel::X4);
    let tr4 = findings(&report, "PXQ-TR-4");
    assert_eq!(tr4.len(), 1);
    assert_eq!(tr4[0].severity, Severity::Info);
    assert_eq!(tr4[0].kind, FindingKind::Incomplete);
    assert!(tr4[0].message.contains("NOT SOURCED"));
}

#[test]
fn hello_fails_every_level() {
    let d = build_pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>"),
    ]);
    for level in PdfxLevel::ALL {
        let report = validate_pdfx(&d.view(), level);
        assert_eq!(report.verdict(), PdfxVerdict::Fails, "{level:?}");
        assert!(report.claim.is_none());
    }
}

#[test]
fn level_names_round_trip() {
    for level in PdfxLevel::ALL {
        assert_eq!(PdfxLevel::from_name(level.as_str()), Some(level));
        assert!(Profile::BUILTIN_NAMES.contains(&level.profile_name()));
    }
    assert_eq!(PdfxLevel::from_name("X-1A"), Some(PdfxLevel::X1a2003));
    assert_eq!(PdfxLevel::from_name("x-3"), Some(PdfxLevel::X3_2003));
    assert_eq!(PdfxLevel::from_name("x-2"), None);
}

#[test]
fn the_builtin_profiles_round_trip_through_json() {
    for level in PdfxLevel::ALL {
        let profile = Profile::builtin(level.profile_name()).expect("builtin");
        assert_eq!(profile, Profile::pdfx(level));
        let back = Profile::from_json(profile.to_json().as_bytes()).expect("parses");
        assert_eq!(back, profile);
    }
    let bad = r#"{"name": "x", "rules": [{"id": "a", "check": "pdfx-requirement", "level": "x-4", "row": "PXQ-CS-1"}]}"#;
    assert!(Profile::from_json(bad.as_bytes()).is_err());
}

#[test]
fn the_report_matches_running_the_profile_through_preflight() {
    let d = doc(&Parts::default());
    let direct = validate_pdfx(&d.view(), PdfxLevel::X3_2003).preflight;
    let via = super::super::run(&d.view(), &Profile::pdfx(PdfxLevel::X3_2003));
    let rows = |r: &PreflightReport| {
        r.findings
            .iter()
            .map(|f| f.rule_id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(rows(&direct), rows(&via));
}

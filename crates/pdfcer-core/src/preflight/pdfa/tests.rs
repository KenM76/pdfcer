#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)] // tests

use super::*;
use crate::document::Document;
use crate::pageops::tests_support::{build_pdf_bytes, stream};
use crate::preflight::tests::error_lines;

/// The parts of a one-page test file.
struct Parts<'a> {
    catalog: &'a str,
    page: &'a str,
    resources: &'a str,
    content: &'a str,
    part: &'a str,
    extra: Vec<(u32, String)>,
}

impl Default for Parts<'_> {
    fn default() -> Self {
        Self {
            catalog: "/OutputIntents [7 0 R] /Metadata 9 0 R",
            page: "",
            resources: "<< >>",
            content: "0 0 1 rg 0 0 10 10 re f",
            part: "2",
            extra: Vec::new(),
        }
    }
}

fn xmp(part: &str) -> String {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
         xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description \
         rdf:about=\"\" xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\" \
         pdfaid:part=\"{part}\" pdfaid:conformance=\"B\"/></rdf:RDF></x:xmpmeta>\
         <?xpacket end=\"w\"?>"
    )
}

fn bytes(parts: &Parts<'_>) -> Vec<u8> {
    let catalog = format!("<< /Type /Catalog /Pages 2 0 R {} >>", parts.catalog);
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources {} /Contents 4 0 R {} >>",
        parts.resources, parts.page
    );
    let content = stream("", parts.content);
    let profile = stream("/N 3", "\0\0\0\0\0\0\0\0\u{2}\0\0\0mntrRGB XYZ ");
    let metadata = stream("/Type /Metadata /Subtype /XML", &xmp(parts.part));
    let mut objects: Vec<(u32, &str)> = vec![
        (1, &catalog),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, &page),
        (4, &content),
        (
            7,
            "<< /Type /OutputIntent /S /GTS_PDFA1 /OutputConditionIdentifier (sRGB) /DestOutputProfile 8 0 R >>",
        ),
        (8, &profile),
        (9, &metadata),
    ];
    objects.extend(parts.extra.iter().map(|(n, b)| (*n, b.as_str())));
    let raw = build_pdf_bytes(&objects);
    let text = String::from_utf8_lossy(&raw).replace(
        "/Root 1 0 R",
        "/Root 1 0 R /ID [<0123456789ABCDEF> <0123456789ABCDEF>]",
    );
    text.into_bytes()
}

impl Parts<'_> {
    fn load(&self) -> Document {
        Document::from_bytes(bytes(self)).expect("fixture loads")
    }
}

fn findings<'r>(report: &'r PdfaReport, row: &str) -> Vec<&'r Finding> {
    report
        .preflight
        .findings
        .iter()
        .filter(|f| f.rule_id == row && f.kind == FindingKind::Violation)
        .collect()
}

#[test]
fn a_well_formed_2b_file_has_no_error_but_is_never_confirmed() {
    let report = validate_pdfa(&Parts::default().load().view(), PdfaLevel::A2b);
    assert_eq!(error_lines(&report.preflight), Vec::<String>::new());
    assert_eq!(report.verdict(), PdfaVerdict::Unconfirmed);
    let not_yet = report
        .preflight
        .findings
        .iter()
        .any(|f| f.kind == FindingKind::Incomplete && f.message.contains("does not check"));
    assert!(not_yet, "uncovered rows are disclosed");
}

#[test]
fn jxl_fails_at_every_level() {
    for level in PdfaLevel::ALL {
        let part = level.part().to_string();
        let parts = Parts {
            part: &part,
            resources: "<< /XObject << /Im 10 0 R >> >>",
            content: "/Im Do",
            extra: vec![(
                10,
                stream(
                    "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB \
                     /BitsPerComponent 8 /Filter /JXLDecode",
                    "xxx",
                ),
            )],
            ..Parts::default()
        };
        let report = validate_pdfa(&parts.load().view(), level);
        let hits = findings(&report, "PAI-FS-15");
        assert!(!hits.is_empty(), "{level:?}");
        assert!(hits[0].message.contains("JXLDecode"), "{}", hits[0].message);
        assert_eq!(
            hits[0].target,
            FindingTarget::Image(crate::object::ObjId::new(10, 0))
        );
        assert_eq!(report.verdict(), PdfaVerdict::Fails);
    }
}

#[test]
fn lzw_fails_1b_with_its_clause() {
    let parts = Parts {
        part: "1",
        extra: vec![(10, stream("/Filter /LZWDecode", "x"))],
        catalog: "/OutputIntents [7 0 R] /Metadata 9 0 R /Extra 10 0 R",
        ..Parts::default()
    };
    let report = validate_pdfa(&parts.load().view(), PdfaLevel::A1b);
    let hits = findings(&report, "PAI-FS-15");
    assert!(!hits.is_empty());
    assert!(
        hits[0].message.contains("ISO 19005-1:2005 §6.1.10"),
        "{}",
        hits[0].message
    );
}

#[test]
fn missing_metadata_fails() {
    let parts = Parts {
        catalog: "/OutputIntents [7 0 R]",
        ..Parts::default()
    };
    let report = validate_pdfa(&parts.load().view(), PdfaLevel::A2b);
    assert!(!findings(&report, "PAI-MD-1").is_empty());
    assert_eq!(report.verdict(), PdfaVerdict::Fails);
}

#[test]
fn the_wrong_part_fails_md_16() {
    let report = validate_pdfa(&Parts::default().load().view(), PdfaLevel::A3b);
    let hits = findings(&report, "PAI-MD-16");
    assert!(!hits.is_empty());
    assert!(hits[0].message.contains("\"2\""), "{}", hits[0].message);
}

#[test]
fn rgb_without_an_intent_fails_on_its_page() {
    let parts = Parts {
        catalog: "/Metadata 9 0 R",
        ..Parts::default()
    };
    let report = validate_pdfa(&parts.load().view(), PdfaLevel::A2b);
    let hits = findings(&report, "PAI-CS-4");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page, Some(1));
}

#[test]
fn a_default_rgb_satisfies_2b_but_not_1b() {
    let parts = Parts {
        catalog: "/Metadata 9 0 R",
        resources: "<< /ColorSpace << /DefaultRGB [/ICCBased 8 0 R] >> >>",
        ..Parts::default()
    };
    let d = parts.load();
    assert!(findings(&validate_pdfa(&d.view(), PdfaLevel::A2b), "PAI-CS-4").is_empty());
    assert!(!findings(&validate_pdfa(&d.view(), PdfaLevel::A1b), "PAI-CS-4").is_empty());
}

#[test]
fn transparency_fails_1b_only() {
    let parts = Parts {
        resources: "<< /ExtGState << /G 10 0 R >> >>",
        content: "/G gs 0 0 1 rg 0 0 10 10 re f",
        extra: vec![(10, "<< /Type /ExtGState /ca 0.5 >>".into())],
        ..Parts::default()
    };
    let d = parts.load();
    assert!(!findings(&validate_pdfa(&d.view(), PdfaLevel::A1b), "PAI-TG-6").is_empty());
    assert!(findings(&validate_pdfa(&d.view(), PdfaLevel::A2b), "PAI-TG-6").is_empty());
}

#[test]
fn annotation_rows_name_the_annotation() {
    let parts = Parts {
        page: "/Annots [10 0 R]",
        extra: vec![(
            10,
            "<< /Type /Annot /Subtype /Movie /Rect [0 0 10 10] /F 2 >>".into(),
        )],
        ..Parts::default()
    };
    let report = validate_pdfa(&parts.load().view(), PdfaLevel::A2b);
    for row in ["PAI-IA-2", "PAI-IA-6", "PAI-IA-7"] {
        let hits = findings(&report, row);
        assert_eq!(hits.len(), 1, "{row}");
        let id = crate::object::ObjId::new(10, 0);
        assert_eq!(hits[0].target, FindingTarget::Annotation(id), "{row}");
    }
}

#[test]
fn a_too_narrow_crop_box_fails_fs_44() {
    let parts = Parts {
        page: "/CropBox [0 0 2 792]",
        ..Parts::default()
    };
    let report = validate_pdfa(&parts.load().view(), PdfaLevel::A2b);
    assert!(!findings(&report, "PAI-FS-44").is_empty());
}

#[test]
fn levels_parse_and_name_their_profiles() {
    assert_eq!(PdfaLevel::from_name("2B"), Some(PdfaLevel::A2b));
    assert_eq!(PdfaLevel::from_name("pdfa-3b"), Some(PdfaLevel::A3b));
    assert_eq!(PdfaLevel::from_name("a-1b"), Some(PdfaLevel::A1b));
    assert_eq!(PdfaLevel::from_name("1a"), None);
    for level in PdfaLevel::ALL {
        assert!(Profile::BUILTIN_NAMES.contains(&level.profile_name()));
        let p = Profile::builtin(level.profile_name()).expect("built in");
        assert_eq!(p, Profile::pdfa(level));
    }
}

#[test]
fn the_profile_round_trips_through_json() {
    for level in PdfaLevel::ALL {
        let p = Profile::pdfa(level);
        let back = Profile::from_json(p.to_json().as_bytes()).expect("parses");
        assert_eq!(back, p, "{level:?}");
    }
}

#[test]
fn a_row_of_another_level_is_refused() {
    let json = r#"{"name": "x", "rules": [{"id": "r", "severity": "error", "check": "pdfa-requirement", "level": "2b", "row": "PAI-TG-6"}]}"#;
    assert!(Profile::from_json(json.as_bytes()).is_err());
}

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)] // tests

use std::collections::BTreeMap;

use super::*;
use crate::document::Document;
use crate::object::ObjId;
use crate::pageops::tests_support::build_pdf;
use crate::pageops::tests_support::stream;
use crate::preflight::Profile;

fn xmp(part: u8) -> String {
    let rev = if part == 2 {
        "<pdfuaid:rev>2024</pdfuaid:rev>"
    } else {
        ""
    };
    let body = format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF><rdf:Description \
         xmlns:pdfuaid=\"http://www.aiim.org/pdfua/ns/id/\"><pdfuaid:part>{part}</pdfuaid:part>{rev}\
         <dc:title><rdf:Alt><rdf:li xml:lang=\"en\">A square</rdf:li></rdf:Alt></dc:title>\
         </rdf:Description></rdf:RDF></x:xmpmeta>"
    );
    stream("/Type /Metadata /Subtype /XML", &body)
}

/// A one-page tagged document conforming to `part`: a `<Figure>` with
/// `/Alt` over one filled square, and an artifact square. `over` replaces
/// or adds objects by number.
fn doc(part: u8, over: &[(u32, String)]) -> Document {
    let mut objects: BTreeMap<u32, String> = BTreeMap::new();
    objects.insert(
        1,
        "<< /Type /Catalog /Pages 2 0 R /MarkInfo << /Marked true >> /StructTreeRoot 5 0 R \
         /Lang (en-US) /ViewerPreferences << /DisplayDocTitle true >> /Metadata 8 0 R >>"
            .into(),
    );
    objects.insert(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into());
    objects.insert(
        3,
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> \
         /Contents 4 0 R /StructParents 0 /Tabs /S >>"
            .into(),
    );
    objects.insert(
        4,
        stream(
            "",
            "/Figure << /MCID 0 >> BDC 0 0 10 10 re f EMC /Artifact BMC 0 0 1 1 re f EMC",
        ),
    );
    objects.insert(
        5,
        "<< /Type /StructTreeRoot /K 6 0 R /ParentTree 9 0 R >>".into(),
    );
    objects.insert(
        6,
        "<< /Type /StructElem /S /Document /P 5 0 R /K 7 0 R >>".into(),
    );
    objects.insert(
        7,
        "<< /Type /StructElem /S /Figure /P 6 0 R /Pg 3 0 R /K 0 /Alt (A square) >>".into(),
    );
    objects.insert(8, xmp(part));
    objects.insert(9, "<< /Nums [0 [7 0 R]] >>".into());
    for (n, body) in over {
        objects.insert(*n, body.clone());
    }
    let list: Vec<(u32, &str)> = objects.iter().map(|(n, b)| (*n, b.as_str())).collect();
    build_pdf(&list)
}

fn part_of(n: u8) -> PdfuaPart {
    PdfuaPart::from_number(n).unwrap()
}

fn violations(d: &Document, n: u8) -> Vec<String> {
    let report = validate_pdfua(&d.view(), part_of(n));
    report.violations().map(|f| f.rule_id.clone()).collect()
}

fn assert_has(d: &Document, n: u8, id: &str) {
    let v = violations(d, n);
    assert!(v.iter().any(|x| x == id), "{id} not in {v:?}");
}

#[test]
fn a_conforming_document_has_no_violations_in_either_part() {
    for n in [1, 2] {
        let report = validate_pdfua(&doc(n, &[]).view(), part_of(n));
        let v: Vec<_> = report.violations().collect();
        assert!(v.is_empty(), "UA-{n}: {v:#?}");
        let incomplete: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.kind == FindingKind::Incomplete)
            .collect();
        assert!(incomplete.is_empty(), "UA-{n}: {incomplete:#?}");
        assert!(!report.fails());
    }
}

#[test]
fn checks_a_machine_cannot_decide_are_needs_a_person_findings_not_passes() {
    let report = validate_pdfua(&doc(1, &[]).view(), PdfuaPart::Ua1);
    let people: Vec<_> = report.needs_person().collect();
    assert!(!people.is_empty());
    assert!(people.iter().all(|f| f.severity == Severity::Info));
    let sourced_person = requirements(PdfuaPart::Ua1)
        .iter()
        .filter(|r| r.decided_by == DecidedBy::Person && r.sourced)
        .count();
    assert_eq!(people.len(), sourced_person);
}

#[test]
fn every_requirement_is_checked_asked_or_listed_as_not_checked() {
    for part in [PdfuaPart::Ua1, PdfuaPart::Ua2] {
        let report = validate_pdfua(&doc(part.number(), &[]).view(), part);
        let checked = match part {
            PdfuaPart::Ua1 => UA1_CHECKED,
            PdfuaPart::Ua2 => UA2_CHECKED,
        };
        for r in requirements(part) {
            let asked = r.decided_by == DecidedBy::Person && r.sourced;
            let listed = report.not_checked.iter().any(|n| n.id == r.id);
            let machine = checked.contains(&r.id);
            assert_eq!(
                u8::from(asked) + u8::from(listed) + u8::from(machine),
                1,
                "{} must be in exactly one bucket",
                r.id
            );
        }
        for id in checked {
            assert!(requirement(id).is_some(), "{id} is not an inventory row");
        }
    }
}

#[test]
fn an_untagged_document_fails_structure_and_content() {
    let d = doc(
        1,
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
            (4, stream("", "0 0 10 10 re f")),
        ],
    );
    let v = violations(&d, 1);
    for id in [
        "PUI-V-7.1-11",
        "PUI-V-6.2-1",
        "PUI-06-001",
        "PUI-07-001",
        "PUI-01-005",
    ] {
        assert!(v.iter().any(|x| x == id), "{id} not in {v:?}");
    }
    assert_has(&d, 2, "PUI2-8.2.1-1");
    assert_has(&d, 2, "PUI2-8.4.4-1");
}

#[test]
fn a_figure_without_alt_targets_its_struct_elem() {
    let d = doc(
        1,
        &[(
            7,
            "<< /Type /StructElem /S /Figure /P 6 0 R /Pg 3 0 R /K 0 >>".into(),
        )],
    );
    let report = validate_pdfua(&d.view(), PdfuaPart::Ua1);
    let f = report
        .violations()
        .find(|f| f.rule_id == "PUI-13-004")
        .expect("Figure without Alt");
    assert_eq!(f.target, FindingTarget::StructElem(ObjId::new(7, 0)));
    assert_has(&d, 2, "PUI2-8.2.5.28.2-1");
}

#[test]
fn title_display_and_identification_are_checked() {
    let bad_xmp = stream(
        "/Type /Metadata /Subtype /XML",
        "<x:xmpmeta><pdfuaid:part>2</pdfuaid:part><dc:title> </dc:title></x:xmpmeta>",
    );
    let d = doc(
        1,
        &[
            (8, bad_xmp),
            (
                1,
                "<< /Type /Catalog /Pages 2 0 R /MarkInfo << /Marked true >> /StructTreeRoot 5 0 R \
                 /Lang (en-US) /ViewerPreferences << /DisplayDocTitle false >> /Metadata 8 0 R >>"
                    .into(),
            ),
        ],
    );
    let v = violations(&d, 1);
    for id in ["PUI-06-002", "PUI-06-003", "PUI-07-002"] {
        assert!(v.iter().any(|x| x == id), "{id} not in {v:?}");
    }
    let v2 = violations(&d, 2);
    for id in ["PUI2-5-5", "PUI2-8.11.1-1", "PUI2-8.11.2-1"] {
        assert!(v2.iter().any(|x| x == id), "{id} not in {v2:?}");
    }
    assert!(
        !v2.iter().any(|x| x == "PUI2-5-2"),
        "part 2 is right for UA-2"
    );
}

#[test]
fn untagged_content_and_misnested_artifacts_are_found() {
    let d = doc(
        1,
        &[(
            4,
            stream(
                "",
                "/Figure << /MCID 0 >> BDC /Artifact BMC 0 0 1 1 re f EMC EMC 5 5 1 1 re f",
            ),
        )],
    );
    let v = violations(&d, 1);
    assert!(v.iter().any(|x| x == "PUI-01-003"), "{v:?}");
    assert!(v.iter().any(|x| x == "PUI-01-005"), "{v:?}");
}

/// A form past the depth cap is not read; the validator says so rather
/// than passing content it never saw.
#[test]
fn a_form_chain_too_deep_to_read_is_incomplete_not_passed() {
    const CHAIN: u32 = 33;
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                /Resources << /XObject << /F 10 0 R >> >> /Contents 4 0 R /StructParents 0 \
                /Tabs /S >>";
    let mut over = vec![
        (3, page.to_owned()),
        (
            4,
            stream("", "/Figure << /MCID 0 >> BDC 0 0 10 10 re f EMC /F Do"),
        ),
    ];
    for i in 0..CHAIN {
        let n = 10 + i;
        let (resources, body) = if i + 1 == CHAIN {
            (String::new(), "0 0 1 1 re f".to_owned())
        } else {
            (
                format!("/Resources << /XObject << /F {} 0 R >> >>", n + 1),
                "/F Do".to_owned(),
            )
        };
        let dict = format!("/Type /XObject /Subtype /Form /BBox [0 0 10 10] {resources}");
        over.push((n, stream(&dict, &body)));
    }
    let report = validate_pdfua(&doc(1, &over).view(), PdfuaPart::Ua1);
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.kind == FindingKind::Incomplete
                && f.rule_id == "PUI-01-005"
                && f.message.contains("deep")),
        "{:#?}",
        report.findings
    );
}

#[test]
fn tagged_text_without_a_language_is_found() {
    let font = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";
    let d = doc(
        1,
        &[
            (
                1,
                "<< /Type /Catalog /Pages 2 0 R /MarkInfo << /Marked true >> /StructTreeRoot 5 0 R \
                 /ViewerPreferences << /DisplayDocTitle true >> /Metadata 8 0 R >>"
                    .into(),
            ),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 10 0 R >> >> \
                 /Contents 4 0 R /StructParents 0 >>"
                    .into(),
            ),
            (
                4,
                stream("", "/Figure << /MCID 0 >> BDC BT /F1 12 Tf (a) Tj ET EMC"),
            ),
            (10, font.into()),
        ],
    );
    let v = violations(&d, 1);
    assert!(v.iter().any(|x| x == "PUI-11-001"), "{v:?}");
    // Helvetica is not embedded and shows visible text.
    let report = validate_pdfua(&d.view(), PdfuaPart::Ua1);
    let f = report
        .violations()
        .find(|f| f.rule_id == "PUI-31-009")
        .expect("unembedded font");
    assert_eq!(f.target, FindingTarget::Font(ObjId::new(10, 0)));
}

#[test]
fn annotations_need_tabs_a_struct_parent_and_contents() {
    let page = |tabs: &str| {
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> \
             /Contents 4 0 R /StructParents 0 /Annots [10 0 R] {tabs} >>"
        )
    };
    let link = "<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] >>".to_owned();
    let d = doc(1, &[(3, page("")), (10, link.clone())]);
    let report = validate_pdfua(&d.view(), PdfuaPart::Ua1);
    let ids: Vec<_> = report.violations().map(|f| f.rule_id.as_str()).collect();
    for id in ["PUI-28-008", "PUI-28-011", "PUI-28-012"] {
        assert!(ids.contains(&id), "{id} not in {ids:?}");
    }
    let f = report
        .violations()
        .find(|f| f.rule_id == "PUI-28-011")
        .unwrap();
    assert_eq!(f.target, FindingTarget::Annotation(ObjId::new(10, 0)));
    assert_eq!(f.page, Some(1));
    assert_has(
        &doc(1, &[(3, page("/Tabs /R")), (10, link.clone())]),
        1,
        "PUI-28-009",
    );
    // A hidden annotation is exempt from the UA-1 annotation rules.
    let hidden = "<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /F 2 >>".to_owned();
    let v = violations(&doc(1, &[(3, page("/Tabs /S")), (10, hidden)]), 1);
    assert!(!v.iter().any(|x| x.starts_with("PUI-28-01")), "{v:?}");
    // UA-2 accepts /Tabs /A.
    let v2 = violations(&doc(2, &[(3, page("/Tabs /A")), (10, link)]), 2);
    assert!(!v2.iter().any(|x| x == "PUI2-8.9.3.3-1"), "{v2:?}");
}

#[test]
fn a_tagged_link_annotation_passes_ua1() {
    let d = doc(
        1,
        &[
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> \
                 /Contents 4 0 R /StructParents 0 /Annots [10 0 R] /Tabs /S >>"
                    .into(),
            ),
            (6, "<< /Type /StructElem /S /Document /P 5 0 R /K [7 0 R 11 0 R] >>".into()),
            (
                10,
                "<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /Contents (Home) /StructParent 1 >>"
                    .into(),
            ),
            (
                11,
                "<< /Type /StructElem /S /Link /P 6 0 R /Pg 3 0 R /K << /Type /OBJR /Obj 10 0 R >> >>"
                    .into(),
            ),
            (9, "<< /Nums [0 [7 0 R] 1 11 0 R] >>".into()),
        ],
    );
    let v = violations(&d, 1);
    assert!(!v.iter().any(|x| x.starts_with("PUI-28")), "{v:?}");
}

#[test]
fn deprecated_and_undescribed_annotations_fail_ua2() {
    let d = doc(
        2,
        &[
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> \
                 /Contents 4 0 R /StructParents 0 /Annots [10 0 R 11 0 R] /Tabs /S >>"
                    .into(),
            ),
            (
                10,
                "<< /Type /Annot /Subtype /Sound /Rect [0 0 1 1] >>".into(),
            ),
            (
                11,
                "<< /Type /Annot /Subtype /Ink /Rect [0 0 1 1] >>".into(),
            ),
        ],
    );
    let v = violations(&d, 2);
    for id in ["PUI2-8.9.2.4.11-1", "PUI2-8.9.2.4.8-1"] {
        assert!(v.iter().any(|x| x == id), "{id} not in {v:?}");
    }
}

#[test]
fn a_cyclic_structure_tree_terminates() {
    let d = doc(
        1,
        &[
            (
                6,
                "<< /Type /StructElem /S /Document /P 5 0 R /K [7 0 R 6 0 R] >>".into(),
            ),
            (
                7,
                "<< /Type /StructElem /S /Figure /P 6 0 R /Pg 3 0 R /K [0 6 0 R] /Alt (x) >>"
                    .into(),
            ),
        ],
    );
    let _ = validate_pdfua(&d.view(), PdfuaPart::Ua1);
    let _ = validate_pdfua(&d.view(), PdfuaPart::Ua2);
}

#[test]
fn a_self_drawing_form_terminates() {
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Resources << /XObject << /X 10 0 R >> >>",
        "/X Do",
    );
    let d = doc(
        1,
        &[
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /XObject << /X 10 0 R >> >> \
                 /Contents 4 0 R /StructParents 0 >>"
                    .into(),
            ),
            (4, stream("", "/Artifact BMC /X Do EMC")),
            (10, form),
        ],
    );
    let _ = validate_pdfua(&d.view(), PdfuaPart::Ua1);
}

#[test]
fn the_pdfua_profile_runs_through_preflight() {
    let d = doc(
        1,
        &[(
            7,
            "<< /Type /StructElem /S /Figure /P 6 0 R /Pg 3 0 R /K 0 >>".into(),
        )],
    );
    let profile = Profile::builtin("pdfua-1").unwrap();
    let report = crate::preflight::run(&d.view(), &profile);
    assert!(report.fails());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule_id == "pdfua-1/PUI-13-004")
    );
    assert!(
        report
            .findings
            .iter()
            .filter(|f| f.kind == FindingKind::NeedsPerson)
            .all(|f| f.severity == Severity::Info)
    );
    let json = profile.to_json();
    assert_eq!(Profile::from_json(json.as_bytes()).unwrap(), profile);
    assert!(Profile::from_json(json.replace("\"part\": 1", "\"part\": 3").as_bytes()).is_err());
}

#[test]
fn part_numbers_round_trip() {
    for p in [PdfuaPart::Ua1, PdfuaPart::Ua2] {
        assert_eq!(PdfuaPart::from_number(p.number()), Some(p));
    }
    assert_eq!(PdfuaPart::from_number(3), None);
    assert_eq!(requirements(PdfuaPart::Ua1).len(), 143);
    assert_eq!(requirements(PdfuaPart::Ua2).len(), 139);
}

#[test]
fn a_violation_fails_the_verdict() {
    let d = doc(1, &[(1, "<< /Type /Catalog /Pages 2 0 R >>".into())]);
    let report = validate_pdfua(&d.view(), PdfuaPart::Ua1);
    assert_eq!(report.verdict(), PdfuaVerdict::Fails);
    assert_eq!(report.verdict().as_str(), "fails");
}

#[test]
fn needs_a_person_or_not_checked_leaves_the_verdict_unconfirmed() {
    for n in [1, 2] {
        let report = validate_pdfua(&doc(n, &[]).view(), part_of(n));
        assert!(!report.fails());
        assert_eq!(report.verdict(), PdfuaVerdict::Unconfirmed, "UA-{n}");

        let mut people_only = report.clone();
        people_only.not_checked.clear();
        assert!(people_only.needs_person().count() > 0);
        assert_eq!(people_only.verdict(), PdfuaVerdict::Unconfirmed);

        let mut unlisted_only = report.clone();
        unlisted_only.findings.clear();
        assert!(!unlisted_only.not_checked.is_empty());
        assert_eq!(unlisted_only.verdict(), PdfuaVerdict::Unconfirmed);
    }
}

#[test]
fn nothing_left_to_decide_conforms_unless_pages_went_unexamined() {
    let mut report = validate_pdfua(&doc(1, &[]).view(), PdfuaPart::Ua1);
    report.findings.clear();
    report.not_checked.clear();
    assert_eq!(report.verdict(), PdfuaVerdict::Conforms);
    assert_eq!(report.verdict().as_str(), "conforms");
    report.pages_checked = 0;
    assert_eq!(report.verdict(), PdfuaVerdict::Unconfirmed);
}

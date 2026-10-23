//! Structure-tree replacement text over redacted content (§14.7.2).

use super::form_tests::{form, has, tagged};
use super::*;
use crate::writer::SaveOptions;

const ROOT: &str = "/StructTreeRoot 7 0 R /MarkInfo << /Marked true >>";
const FONT: &str = "<< /Font << /F1 5 0 R >> >>";
/// MCID 0 is under the mark, MCID 1 is not.
const TWO_SEQUENCES: &str = "/P << /MCID 0 >> BDC BT /F1 12 Tf 10 20 Td (SECRET) Tj ET EMC \
     /P << /MCID 1 >> BDC BT /F1 12 Tf 10 300 Td (KEEP) Tj ET EMC";
const MARK: [f64; 4] = [0.0, 0.0, 600.0, 100.0];

/// Apply, then return the saved file and the report.
fn apply(bytes: Vec<u8>) -> (Vec<u8>, RedactionReport) {
    let doc = Document::from_bytes(bytes).unwrap();
    apply_redactions(&doc, &SaveOptions::identity()).unwrap()
}

fn action(report: &RedactionReport) -> CarrierAction {
    report
        .carriers
        .iter()
        .find(|c| c.carrier == "struct_tree")
        .map(|c| c.action)
        .unwrap()
}

/// The redacted element and its ancestor lose their text; the sibling over
/// unredacted content keeps its.
#[test]
fn an_element_over_redacted_content_and_its_ancestors_lose_their_text() {
    let bytes = tagged(
        ROOT,
        FONT,
        TWO_SEQUENCES,
        MARK,
        &[
            "<< /Type /StructTreeRoot /K 8 0 R >>".to_owned(),
            "<< /Type /StructElem /S /Document /P 7 0 R /Alt (DOCALT) /K [9 0 R \
             << /Type /StructElem /S /P /P 8 0 R /Pg 3 0 R /ActualText (KEEPTEXT) /K 1 >>] >>"
                .to_owned(),
            "<< /Type /StructElem /S /P /P 8 0 R /Pg 3 0 R /ActualText (HIDDENWORDS) /E (EXPWORDS) /K 0 >>"
                .to_owned(),
        ],
    );
    let (out, report) = apply(bytes);
    assert!(
        !has(&out, b"HIDDENWORDS"),
        "the element kept its /ActualText"
    );
    assert!(!has(&out, b"EXPWORDS"), "the element kept its /E");
    assert!(!has(&out, b"DOCALT"), "the ancestor kept its /Alt");
    assert!(
        has(&out, b"KEEPTEXT"),
        "an unredacted sibling lost its text"
    );
    assert_eq!(action(&report), CarrierAction::Scrubbed, "{report:?}");
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("2 structure element(s)")),
        "{:?}",
        report.notes
    );
}

/// An MCR naming a form stream (`/Stm`, Table 324) matches the MCID inside
/// that form.
#[test]
fn an_mcr_into_a_redacted_form_loses_its_elements_text() {
    let bytes = tagged(
        ROOT,
        "<< /Font << /F1 5 0 R >> /XObject << /Fm0 10 0 R >> >>",
        "/Fm0 Do",
        MARK,
        &[
            "<< /Type /StructTreeRoot /K [9 0 R] >>".to_owned(),
            "null".to_owned(),
            "<< /Type /StructElem /S /P /P 7 0 R /Pg 3 0 R /ActualText (HIDDENWORDS) \
             /K << /Type /MCR /MCID 0 /Stm 10 0 R >> >>"
                .to_owned(),
            form(
                "/BBox [0 0 300 50] /Resources << /Font << /F1 5 0 R >> >>",
                "/P << /MCID 0 >> BDC BT /F1 12 Tf 10 20 Td (SECRET) Tj ET EMC",
            ),
        ],
    );
    let (out, report) = apply(bytes);
    assert!(
        !has(&out, b"HIDDENWORDS"),
        "the element kept its /ActualText"
    );
    assert_eq!(action(&report), CarrierAction::Scrubbed, "{report:?}");
}

/// The same MCID in the page's own content does not match an MCR into a
/// form.
#[test]
fn a_page_mcid_does_not_match_a_form_mcr() {
    let bytes = tagged(
        ROOT,
        FONT,
        TWO_SEQUENCES,
        MARK,
        &[
            "<< /Type /StructTreeRoot /K [9 0 R] >>".to_owned(),
            "null".to_owned(),
            "<< /Type /StructElem /S /P /P 7 0 R /Pg 3 0 R /ActualText (FORMTEXT) \
             /K << /Type /MCR /MCID 0 /Stm 10 0 R >> >>"
                .to_owned(),
            form("/BBox [0 0 300 50]", "/P << /MCID 0 >> BDC EMC"),
        ],
    );
    let (out, report) = apply(bytes);
    assert!(has(&out, b"FORMTEXT"), "{report:?}");
    assert_eq!(action(&report), CarrierAction::CheckedClean);
}

/// A tree whose content is untouched is reported checked, not scrubbed.
#[test]
fn a_tree_over_untouched_content_is_checked_clean() {
    let bytes = tagged(
        ROOT,
        FONT,
        TWO_SEQUENCES,
        [0.0, 0.0, 600.0, 10.0],
        &[
            "<< /Type /StructTreeRoot /K 8 0 R >>".to_owned(),
            "<< /Type /StructElem /S /P /P 7 0 R /Pg 3 0 R /ActualText (HIDDENWORDS) /K 0 >>"
                .to_owned(),
        ],
    );
    let (out, report) = apply(bytes);
    assert!(has(&out, b"HIDDENWORDS"));
    assert_eq!(action(&report), CarrierAction::CheckedClean);
}

/// A cyclic `/K` terminates.
#[test]
fn a_cyclic_tree_terminates() {
    let bytes = tagged(
        ROOT,
        FONT,
        TWO_SEQUENCES,
        MARK,
        &[
            "<< /Type /StructTreeRoot /K 8 0 R >>".to_owned(),
            "<< /Type /StructElem /S /Div /P 7 0 R /Pg 3 0 R /Alt (CYCLEALT) /K [8 0 R 0] >>"
                .to_owned(),
        ],
    );
    let (out, _report) = apply(bytes);
    assert!(!has(&out, b"CYCLEALT"));
}

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)] // tests

use std::collections::BTreeMap;

use crate::content::ContentStream;
use crate::document::Document;
use crate::edit::{
    CommandKind, DocTitleSource, EditError, EditSession, FixPdfuaOptions, PdfuaFixup,
    PdfuaFixupOutcome,
};
use crate::pageops::tests_support::{build_pdf_bytes, stream};
use crate::preflight::{PdfuaPart, validate_pdfua};
use crate::writer::SaveOptions;

const TAGGED: &str = "/Figure << /MCID 0 >> BDC 0 0 10 10 re f EMC";
const XMP_TITLE: &str = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
     xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description \
     xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title><rdf:Alt><rdf:li \
     xml:lang=\"x-default\">A square</rdf:li></rdf:Alt></dc:title></rdf:Description>\
     </rdf:RDF></x:xmpmeta>";

/// A one-page tagged document missing every PDF/UA fixup: no `/Lang`, no
/// `/ViewerPreferences`, no XMP, no `/Tabs`, and `content` on the page.
/// `over` replaces or adds objects by number.
fn bytes(content: &str, over: &[(u32, String)]) -> Vec<u8> {
    let mut objects: BTreeMap<u32, String> = BTreeMap::new();
    objects.insert(
        1,
        "<< /Type /Catalog /Pages 2 0 R /MarkInfo << /Marked true >> /StructTreeRoot 5 0 R >>"
            .into(),
    );
    objects.insert(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into());
    objects.insert(
        3,
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> \
         /Contents 4 0 R /StructParents 0 >>"
            .into(),
    );
    objects.insert(4, stream("", content));
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
    objects.insert(9, "<< /Nums [0 [7 0 R]] >>".into());
    for (n, body) in over {
        objects.insert(*n, body.clone());
    }
    let list: Vec<(u32, &str)> = objects.iter().map(|(n, b)| (*n, b.as_str())).collect();
    build_pdf_bytes(&list)
}

fn session(content: &str, over: &[(u32, String)]) -> EditSession {
    EditSession::new(Document::from_bytes(bytes(content, over)).unwrap())
}

/// [`session`] with a `%PDF-2.0` header, as PDF/UA-2 requires.
fn session_for(part: PdfuaPart, content: &str) -> EditSession {
    let mut b = bytes(content, &[]);
    if part == PdfuaPart::Ua2 {
        b[5..8].copy_from_slice(b"2.0");
    }
    EditSession::new(Document::from_bytes(b).unwrap())
}

fn saved(s: &EditSession) -> Document {
    Document::from_bytes(s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0).unwrap()
}

fn page_content(d: &Document) -> String {
    let view = d.view();
    let pages = crate::page_tree::pages(d).unwrap();
    let c = ContentStream::from_page(&view, &pages[0]).unwrap();
    String::from_utf8_lossy(&c.buf).into_owned()
}

fn violations(d: &Document, part: PdfuaPart) -> Vec<String> {
    validate_pdfua(&d.view(), part)
        .violations()
        .map(|f| f.rule_id.clone())
        .collect()
}

#[test]
fn fix_pdfua_makes_a_fixable_file_pass_and_claims_it_in_one_undo() {
    for part in [PdfuaPart::Ua1, PdfuaPart::Ua2] {
        let content = format!("0 0 1 1 re f {TAGGED} 2 2 1 1 re f");
        let mut s = session_for(part, &content);
        let options = FixPdfuaOptions::default()
            .with_lang("en-US")
            .with_title("A square");
        let report = s.fix_pdfua(part, &options).unwrap();
        assert!(
            !report.validation.fails(),
            "{:?}",
            report.validation.findings
        );
        assert!(report.identified(), "{:?}", report.results);
        for r in &report.results {
            let want = if r.fixup == PdfuaFixup::TabOrder {
                PdfuaFixupOutcome::AlreadyDone // no annotations
            } else {
                PdfuaFixupOutcome::Applied
            };
            assert_eq!(r.outcome, want, "{r:?}");
        }
        assert!(report.disclosures.iter().any(|d| d.contains("decorative")));
        assert_eq!(s.undo_depth(), 1);
        let reopened = saved(&s);
        assert!(violations(&reopened, part).is_empty());
        let rev = validate_pdfua(&reopened.view(), part);
        assert!(!rev.fails(), "{:?}", rev.findings);
        assert_eq!(s.undo(), Some(CommandKind::FixPdfua));
        assert!(validate_pdfua(&s.view(), part).fails());
    }
}

#[test]
fn a_second_run_changes_nothing() {
    let mut s = session(&format!("0 0 1 1 re f {TAGGED}"), &[]);
    let options = FixPdfuaOptions::default()
        .with_lang("en-US")
        .with_title("A square");
    s.fix_pdfua(PdfuaPart::Ua1, &options).unwrap();
    let again = s
        .fix_pdfua(PdfuaPart::Ua1, &FixPdfuaOptions::default())
        .unwrap();
    for r in &again.results {
        assert_eq!(r.outcome, PdfuaFixupOutcome::AlreadyDone, "{r:?}");
    }
    assert_eq!(s.undo_depth(), 1);
}

#[test]
fn without_a_language_or_title_those_fixups_refuse_and_the_claim_is_withheld() {
    let mut s = session(TAGGED, &[]);
    let report = s
        .fix_pdfua(PdfuaPart::Ua1, &FixPdfuaOptions::default())
        .unwrap();
    let outcome = |f: PdfuaFixup| report.results.iter().find(|r| r.fixup == f).unwrap();
    assert_eq!(
        outcome(PdfuaFixup::Language).outcome,
        PdfuaFixupOutcome::Skipped
    );
    assert!(
        outcome(PdfuaFixup::Language)
            .detail
            .contains("does not guess")
    );
    assert_eq!(
        outcome(PdfuaFixup::DisplayDocTitle).outcome,
        PdfuaFixupOutcome::Refused
    );
    assert_eq!(
        outcome(PdfuaFixup::Identification).outcome,
        PdfuaFixupOutcome::Refused
    );
    assert!(!report.identified());
    assert!(report.validation.fails());
    let xmp = crate::font_unembed::catalog_xmp(&s.view());
    assert!(xmp.is_none_or(|p| !String::from_utf8_lossy(&p).contains("pdfuaid")));
}

#[test]
fn a_malformed_language_tag_is_refused_and_writes_nothing() {
    let mut s = session(TAGGED, &[]);
    for bad in ["en_US", "", "englishlanguage", "en-"] {
        let err = s.set_document_language(bad).unwrap_err();
        assert!(
            matches!(err, EditError::LanguageTagMalformed { .. }),
            "{bad}: {err:?}"
        );
    }
    assert_eq!(s.undo_depth(), 0);
    assert!(s.set_document_language("de-CH").unwrap());
    assert_eq!(s.document_language().as_deref(), Some("de-CH"));
    assert!(!s.set_document_language("de-CH").unwrap());
    assert_eq!(s.undo(), Some(CommandKind::SetDocumentLanguage));
    assert_eq!(s.document_language(), None);
}

#[test]
fn display_doc_title_never_invents_a_title() {
    let mut s = session(TAGGED, &[]);
    assert!(matches!(
        s.set_display_doc_title(None),
        Err(EditError::DocumentTitleMissing)
    ));
    assert!(matches!(
        s.set_display_doc_title(Some("   ")),
        Err(EditError::DocumentTitleMissing)
    ));
    assert_eq!(s.undo_depth(), 0);
}

#[test]
fn display_doc_title_uses_an_existing_dc_title_and_edits_only_the_catalog() {
    let catalog = "<< /Type /Catalog /Pages 2 0 R /MarkInfo << /Marked true >> \
                   /StructTreeRoot 5 0 R /Metadata 8 0 R >>";
    let over = [
        (1, catalog.to_owned()),
        (8, stream("/Type /Metadata /Subtype /XML", XMP_TITLE)),
    ];
    let original = bytes(TAGGED, &over);
    let mut s = EditSession::new(Document::from_bytes(original.clone()).unwrap());
    let outcome = s.set_display_doc_title(None).unwrap();
    assert_eq!(outcome.source, DocTitleSource::Existing);
    assert!(outcome.changed && !outcome.packet_created);
    let saved = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let tail = String::from_utf8_lossy(&saved[original.len()..]).into_owned();
    assert!(
        tail.contains("1 0 obj") && !tail.contains("8 0 obj"),
        "{tail}"
    );
    let d = Document::from_bytes(saved).unwrap();
    let rows = violations(&d, PdfuaPart::Ua1);
    assert!(!rows.iter().any(|r| r.starts_with("PUI-07")), "{rows:?}");
    assert_eq!(s.undo(), Some(CommandKind::SetDisplayDocTitle));
}

#[test]
fn a_supplied_title_creates_the_packet_and_the_info_title() {
    let mut s = session(TAGGED, &[]);
    let outcome = s.set_display_doc_title(Some("Quarterly")).unwrap();
    assert_eq!(outcome.source, DocTitleSource::Supplied);
    assert!(outcome.packet_created);
    let packet = crate::font_unembed::catalog_xmp(&s.view()).unwrap();
    assert!(String::from_utf8_lossy(&packet).contains("Quarterly"));
    assert_eq!(
        s.info_text(crate::edit::InfoField::Title).unwrap().text,
        "Quarterly"
    );
    assert_eq!(s.undo_depth(), 1);
    let rows = violations(&saved(&s), PdfuaPart::Ua1);
    assert!(
        !rows.iter().any(|r| r == "PUI-06-001" || r == "PUI-06-003"),
        "{rows:?}"
    );
}

#[test]
fn identification_is_withheld_while_content_is_untagged() {
    let mut s = session(&format!("0 0 1 1 re f {TAGGED}"), &[]);
    s.set_document_language("en").unwrap();
    s.set_display_doc_title(Some("A square")).unwrap();
    let depth = s.undo_depth();
    match s.write_pdfua_identification(PdfuaPart::Ua1) {
        Err(EditError::PdfuaIdentificationWithheld { part: 1, reasons }) => {
            assert!(
                reasons.iter().any(|r| r.starts_with("PUI-01-005")),
                "{reasons:?}"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(s.undo_depth(), depth);
    s.mark_untagged_as_artifacts(&[]).unwrap();
    match s.write_pdfua_identification(PdfuaPart::Ua2) {
        Err(EditError::PdfuaIdentificationWithheld { reasons, .. }) => {
            assert!(reasons.iter().any(|r| r.contains("PDF 2.0")), "{reasons:?}");
        }
        other => panic!("a PDF 1.7 file must not claim PDF/UA-2: {other:?}"),
    }
    assert!(s.write_pdfua_identification(PdfuaPart::Ua1).unwrap());
    assert!(!s.write_pdfua_identification(PdfuaPart::Ua1).unwrap());
    let packet = crate::font_unembed::catalog_xmp(&s.view()).unwrap();
    let text = String::from_utf8_lossy(&packet).into_owned();
    assert!(text.contains(">1</pdfuaid:part>"), "{text}");
    assert!(!text.contains("pdfuaid:rev"));
}

#[test]
fn ua2_identification_carries_rev_2024() {
    let mut s = session_for(PdfuaPart::Ua2, TAGGED);
    s.set_document_language("en").unwrap();
    s.set_display_doc_title(Some("A square")).unwrap();
    assert!(s.write_pdfua_identification(PdfuaPart::Ua2).unwrap());
    let packet = crate::font_unembed::catalog_xmp(&s.view()).unwrap();
    let text = String::from_utf8_lossy(&packet).into_owned();
    assert!(text.contains(">2</pdfuaid:part>"), "{text}");
    assert!(text.contains(">2024</pdfuaid:rev>"), "{text}");
    assert!(!validate_pdfua(&saved(&s).view(), PdfuaPart::Ua2).fails());
}

#[test]
fn artifacts_wrap_around_tagged_content_and_whole_text_objects() {
    let content = format!("0 0 1 1 re f BT 1 0 0 1 5 5 Tm (x) Tj ET {TAGGED} q 2 2 1 1 re S Q");
    let mut s = session(&content, &[]);
    let report = s.mark_untagged_as_artifacts(&[]).unwrap();
    assert_eq!(report.sequences, 2);
    assert_eq!(report.items, 3);
    assert_eq!(report.marked, vec![(0, 3)]);
    let text = page_content(&saved(&s));
    let figure = text.find("/Figure").unwrap();
    let first_close = text.find("EMC").unwrap();
    assert!(text.starts_with("/Artifact BMC"), "{text}");
    assert!(
        first_close < figure,
        "the wrap must close before the tag: {text}"
    );
    let bt = text.find("BT").unwrap();
    let et = text.find("ET").unwrap();
    assert!(
        text[..bt].contains("/Artifact BMC") && text[et..figure].contains("EMC"),
        "{text}"
    );
    assert!(text.trim_end().ends_with("Q\nEMC"), "{text}");
    let rows = violations(&saved(&s), PdfuaPart::Ua1);
    assert!(!rows.iter().any(|r| r.starts_with("PUI-01")), "{rows:?}");
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(s.undo(), Some(CommandKind::MarkUntaggedAsArtifacts));
}

#[test]
fn artifacts_refuse_an_untagged_document_and_skip_untagged_pages() {
    let untagged = "<< /Type /Catalog /Pages 2 0 R >>";
    let mut s = session("0 0 1 1 re f", &[(1, untagged.to_owned())]);
    assert!(matches!(
        s.mark_untagged_as_artifacts(&[]),
        Err(EditError::DocumentNotTagged)
    ));
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> \
                /Contents 4 0 R >>";
    let mut s = session(
        "0 0 1 1 re f",
        &[(3, page.to_owned()), (7, "<< /S /Figure >>".into())],
    );
    let report = s.mark_untagged_as_artifacts(&[]).unwrap();
    assert_eq!(report.sequences, 0);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(s.undo_depth(), 0);
    assert!(matches!(
        s.mark_untagged_as_artifacts(&[4]),
        Err(EditError::PageOutOfRange { index: 4, count: 1 })
    ));
}

#[test]
fn artifacts_skip_an_unbalanced_page_and_leave_unclaimed_mcids() {
    let mut s = session("q 0 0 1 1 re f", &[]);
    let report = s.mark_untagged_as_artifacts(&[]).unwrap();
    assert_eq!(report.skipped.len(), 1, "{report:?}");
    assert_eq!(s.undo_depth(), 0);
    let mut s = session(
        &format!("/P << /MCID 7 >> BDC 0 0 1 1 re f EMC {TAGGED}"),
        &[],
    );
    let report = s.mark_untagged_as_artifacts(&[]).unwrap();
    assert_eq!(report.sequences, 0);
    assert!(
        report.disclosures.iter().any(|d| d.contains("/MCID")),
        "{report:?}"
    );
}

#[test]
fn tab_order_sets_structure_order_on_annotated_pages_only() {
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> \
                /Contents 4 0 R /StructParents 0 /Annots [10 0 R] >>";
    let annot = "<< /Type /Annot /Subtype /Square /Rect [0 0 5 5] >>";
    let mut s = session(TAGGED, &[(3, page.to_owned()), (10, annot.to_owned())]);
    assert_eq!(s.set_pdfua_tab_order(PdfuaPart::Ua1).unwrap(), vec![0]);
    assert_eq!(s.undo_depth(), 1);
    assert!(s.set_pdfua_tab_order(PdfuaPart::Ua1).unwrap().is_empty());
    assert_eq!(s.undo(), Some(CommandKind::SetPdfuaTabOrder));
    let mut plain = session(TAGGED, &[]);
    assert!(
        plain
            .set_pdfua_tab_order(PdfuaPart::Ua1)
            .unwrap()
            .is_empty()
    );
    assert_eq!(plain.undo_depth(), 0);
}

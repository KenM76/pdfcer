#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use crate::document::Document;
use crate::optimize::tests::build;
use crate::space_audit::tests::stream;

/// 1 catalog with `catalog_extra`, 2 pages, 3 page, 4 Info with `info`,
/// 5 an XMP packet when `xmp` is given.
fn doc(catalog_extra: &str, info: &str, xmp: Option<&str>) -> Document {
    let metadata = if xmp.is_some() { "/Metadata 5 0 R" } else { "" };
    let mut bodies = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {metadata} {catalog_extra} >>").into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] >>".to_vec(),
        format!("<< {info} >>").into_bytes(),
    ];
    if let Some(x) = xmp {
        bodies.push(stream("/Type /Metadata /Subtype /XML", x.as_bytes()));
    }
    let bytes = String::from_utf8_lossy(&build(&bodies))
        .replace("/Root 1 0 R", "/Root 1 0 R /Info 4 0 R")
        .into_bytes();
    Document::from_bytes(bytes).unwrap()
}

fn packet(description: &str) -> String {
    format!(
        "<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
         xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">{description}</rdf:RDF>\
         </x:xmpmeta><?xpacket end=\"w\"?>"
    )
}

fn claim(d: &Document) -> Option<PdfxClaim> {
    pdfx_claim(&d.view())
}

const INTENT: &str = "/OutputIntents [<< /Type /OutputIntent /S /GTS_PDFX \
    /OutputConditionIdentifier (FOGRA39) >> << /S /GTS_PDFA1 >>]";

#[test]
fn an_x4_claim_in_xmp_element_form_with_its_output_intent() {
    let xmp = packet(
        "<rdf:Description rdf:about=\"\" xmlns:pdfxid=\"http://www.npes.org/pdfx/ns/id/\">\
         <pdfxid:GTS_PDFXVersion>PDF/X-4</pdfxid:GTS_PDFXVersion></rdf:Description>",
    );
    let c = claim(&doc(INTENT, "", Some(&xmp))).unwrap();
    assert_eq!(c.level, "PDF/X-4");
    assert_eq!(c.output_intents, vec!["FOGRA39".to_owned()]);
    assert!(c.is_recognised() && c.carriers_agree() && c.has_output_intent());
    assert_eq!(c.info_version, None);
}

#[test]
fn the_attribute_form_under_any_prefix_bound_to_the_namespace() {
    let xmp = packet(
        "<rdf:Description rdf:about=\"\" xmlns:q='http://www.npes.org/pdfx/ns/id/' \
         q:GTS_PDFXVersion='PDF/X-5pg'/>",
    );
    assert_eq!(claim(&doc("", "", Some(&xmp))).unwrap().level, "PDF/X-5pg");
}

#[test]
fn the_adobe_pdfx_namespace_is_not_a_claim() {
    let xmp = packet(
        "<rdf:Description rdf:about=\"\" xmlns:pdfx=\"http://ns.adobe.com/pdfx/1.3/\">\
         <pdfx:GTS_PDFXVersion>PDF/X-4</pdfx:GTS_PDFXVersion></rdf:Description>",
    );
    assert_eq!(claim(&doc(INTENT, "", Some(&xmp))), None);
}

#[test]
fn an_output_intent_alone_is_not_a_claim() {
    let d = doc(INTENT, "", None);
    assert_eq!(claim(&d), None);
    assert_eq!(pdfx_output_intents(&d.view()), vec!["FOGRA39".to_owned()]);
    assert!(pdfx_output_intents(&doc("", "", None).view()).is_empty());
}

#[test]
fn the_2001_conformance_key_splits_x1_from_x1a() {
    let both = "/GTS_PDFXVersion (PDF/X-1:2001) /GTS_PDFXConformance (PDF/X-1a:2001)";
    let c = claim(&doc("", both, None)).unwrap();
    assert_eq!(c.level, "PDF/X-1a:2001");
    assert_eq!(c.info_conformance.as_deref(), Some("PDF/X-1a:2001"));
    let x1 = claim(&doc("", "/GTS_PDFXVersion (PDF/X-1:2001)", None)).unwrap();
    assert_eq!(x1.level, "PDF/X-1:2001");
    assert!(!x1.has_output_intent());
}

#[test]
fn a_utf16_info_value_decodes() {
    let c = claim(&doc(
        "",
        "/GTS_PDFXVersion <FEFF005000440046002F0058002D0033003A0032003000300033>",
        None,
    ));
    assert_eq!(c.unwrap().level, "PDF/X-3:2003");
}

#[test]
fn xmp_wins_and_a_mismatch_is_reported() {
    let xmp = packet(
        "<rdf:Description xmlns:pdfxid=\"http://www.npes.org/pdfx/ns/id/\" \
         pdfxid:GTS_PDFXVersion=\"PDF/X-4\"/>",
    );
    let c = claim(&doc("", "/GTS_PDFXVersion (PDF/X-3:2003)", Some(&xmp))).unwrap();
    assert_eq!(c.level, "PDF/X-4");
    assert_eq!(c.info_version.as_deref(), Some("PDF/X-3:2003"));
    assert!(!c.carriers_agree());
}

#[test]
fn an_unregistered_value_passes_through_verbatim() {
    let c = claim(&doc("", "/GTS_PDFXVersion (PDF/X-6:2027)", None)).unwrap();
    assert_eq!(c.level, "PDF/X-6:2027");
    assert!(!c.is_recognised());
}

#[test]
fn a_prefix_that_ends_another_name_does_not_match() {
    // `xid:` is bound to the namespace, but the property is under `pdfxid:`
    // bound elsewhere; `pdfxid:GTS…` must not be read as `xid:GTS…`.
    let xmp = packet(
        "<rdf:Description xmlns:xid=\"http://www.npes.org/pdfx/ns/id/\" \
         xmlns:pdfxid=\"urn:other\"><pdfxid:GTS_PDFXVersion>PDF/X-4</pdfxid:GTS_PDFXVersion>\
         </rdf:Description>",
    );
    assert_eq!(claim(&doc("", "", Some(&xmp))), None);
}

#[test]
fn a_default_namespace_element_is_read() {
    let xmp = packet(
        "<rdf:Description><GTS_PDFXVersion xmlns=\"http://www.npes.org/pdfx/ns/id/\">\
         PDF/X-6</GTS_PDFXVersion></rdf:Description>",
    );
    assert_eq!(claim(&doc("", "", Some(&xmp))).unwrap().level, "PDF/X-6");
}

#[test]
fn junk_values_are_ignored() {
    let long = format!("/GTS_PDFXVersion ({})", "X".repeat(65));
    assert_eq!(claim(&doc("", &long, None)), None);
    assert_eq!(claim(&doc("", "/GTS_PDFXVersion /PDFX4", None)), None);
}

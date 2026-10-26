//! The residual sweep (default [`ResidualScope::HiddenCarriers`]) reaches the
//! carriers an operator cannot see on the page: outline titles at any depth
//! (§12.3.3), XMP `dc:title` in both its `rdf:Alt` and plain forms (§14.3.2),
//! and the `/Contents` of an annotation the marked region does not overlap
//! (§12.5.2). Redacting the drawn word must leave none of them carrying it.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::redact::ResidualScope;
use pdfcer_core::writer::SaveOptions;

/// UTF-16BE with BOM, as a hex string (§7.9.2.2).
fn utf16_hex(s: &str) -> String {
    let mut out = String::from("<FEFF");
    for u in s.encode_utf16() {
        out.push_str(&format!("{u:04X}"));
    }
    out.push('>');
    out
}

fn fixture() -> Vec<u8> {
    let content = "BT /F1 24 Tf 40 200 Td (CONFIDENTIAL) Tj ET";
    let alt = "<?xpacket begin='' ?><x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF \
               xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description \
               xmlns:dc='http://purl.org/dc/elements/1.1/'><dc:title><rdf:Alt>\
               <rdf:li xml:lang='x-default'>CONFIDENTIAL alt title</rdf:li></rdf:Alt>\
               </dc:title></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end='w'?>";
    let plain = "<?xpacket begin='' ?><x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF \
                 xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description \
                 xmlns:dc='http://purl.org/dc/elements/1.1/'><dc:title>CONFIDENTIAL plain \
                 title</dc:title></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end='w'?>";
    let stream = |dict: &str, data: &str| {
        format!(
            "<< {dict}/Length {} >>\nstream\n{data}\nendstream",
            data.len()
        )
    };
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /Outlines 6 0 R /Metadata 9 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> /Annots [11 0 R] /Metadata 10 0 R >>"
            .to_string(),
        stream("", content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Type /Outlines /First 7 0 R /Last 7 0 R /Count 2 >>".to_string(),
        "<< /Title (CONFIDENTIAL top) /Parent 6 0 R /First 8 0 R /Last 8 0 R /Count 1 \
         /Dest [3 0 R /Fit] >>"
            .to_string(),
        format!(
            "<< /Title {} /Parent 7 0 R /Dest [3 0 R /Fit] >>",
            utf16_hex("Nested CONFIDENTIAL")
        ),
        stream("/Type /Metadata /Subtype /XML ", alt),
        stream("/Type /Metadata /Subtype /XML ", plain),
        "<< /Type /Annot /Subtype /Text /Rect [260 260 280 280] \
         /Contents (CONFIDENTIAL note) >>"
            .to_string(),
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = pdf.len();
    let n = bodies.len() + 1;
    pdf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

fn redacted(scope: ResidualScope) -> Vec<u8> {
    let mut session = EditSession::new(Document::from_bytes(fixture()).expect("fixture loads"));
    session.set_residual_scope(scope);
    let marks = session
        .mark_redactions_by_search("CONFIDENTIAL", false)
        .expect("search marks");
    assert_eq!(marks.len(), 1, "only the drawn word is a search hit");
    session.apply_redactions().expect("redaction applies");
    session
        .to_full_bytes(&SaveOptions::default())
        .expect("saves")
        .0
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Every carrier's survivor, in the spelling the fixture wrote it.
const CARRIERS: [(&str, &[u8]); 5] = [
    ("outline /Title", b"CONFIDENTIAL top"),
    ("XMP dc:title rdf:Alt", b"CONFIDENTIAL alt title"),
    ("XMP dc:title plain", b"CONFIDENTIAL plain"),
    ("annotation /Contents", b"CONFIDENTIAL note"),
    ("fixture text", b"CONFIDENTIAL"),
];

#[test]
fn the_fixture_carries_the_word_in_every_place() {
    let pdf = fixture();
    for (what, needle) in CARRIERS {
        assert!(contains(&pdf, needle), "{what}");
    }
    assert!(contains(&pdf, utf16_hex("Nested CONFIDENTIAL").as_bytes()));
}

#[test]
fn no_hidden_carrier_keeps_the_redacted_word() {
    let out = redacted(ResidualScope::default());
    for (what, needle) in CARRIERS {
        assert!(!contains(&out, needle), "{what} still carries it");
    }
}

#[test]
fn a_nested_utf16_outline_title_is_scrubbed() {
    let out = redacted(ResidualScope::default());
    let doc = Document::from_bytes(out).expect("redacted file loads");
    let outline = pdfcer_core::outline::read_outline(&doc);
    let titles: Vec<&str> = outline.flatten().iter().map(|i| i.title.as_str()).collect();
    assert_eq!(titles.len(), 2, "{titles:?}");
    assert!(
        titles.iter().all(|t| !t.contains("CONFIDENTIAL")),
        "{titles:?}"
    );
}

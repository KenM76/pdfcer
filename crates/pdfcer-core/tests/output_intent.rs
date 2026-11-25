//! `prepress::output_intent` and `PdfxOutputIntent::from_document`: the
//! first intent with a decodable `/N` profile is chosen, and its
//! identifying entries come with it. Fixtures are built inline (rule 7).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::prepress::{PdfxOutputIntent, output_intent};

/// A one-page document whose catalog lists `intents` (the array's body)
/// over the extra objects `extra`, numbered from 4.
fn doc(intents: &str, extra: &[&str]) -> Document {
    let mut bodies = vec![
        format!("<< /Type /Catalog /Pages 2 0 R /OutputIntents [{intents}] >>"),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R >>".to_owned(),
    ];
    bodies.extend(extra.iter().map(|s| (*s).to_owned()));
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    Document::from_bytes(buf).expect("fixture loads")
}

/// Object 4: a profile with no `/N`, passed over. Object 5: an
/// ASCIIHex-encoded four-component profile, `ABCD`.
const PROFILES: [&str; 2] = [
    "<< /Length 4 >>\nstream\nWXYZ\nendstream",
    "<< /N 4 /Filter /ASCIIHexDecode /Length 9 >>\nstream\n41424344>\nendstream",
];

#[test]
fn the_first_intent_with_a_decodable_n_profile_is_chosen() {
    let d = doc(
        "<< /Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier (Broken) \
         /DestOutputProfile 4 0 R >> \
         << /Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier (FOGRA39) \
         /OutputCondition (Coated) /RegistryName (http://www.color.org) /Info (Offset) \
         /DestOutputProfile 5 0 R >>",
        &PROFILES,
    );
    let i = output_intent(&d.view()).expect("an intent");
    assert_eq!(&*i.profile, b"ABCD", "decoded through its filter");
    assert_eq!(i.components, 4);
    assert_eq!(i.subtype.as_deref(), Some("GTS_PDFX"));
    assert_eq!(i.condition_identifier.as_deref(), Some("FOGRA39"));
    assert_eq!(i.condition.as_deref(), Some("Coated"));
    assert_eq!(i.registry_name.as_deref(), Some("http://www.color.org"));
    assert_eq!(i.info.as_deref(), Some("Offset"));

    let x = PdfxOutputIntent::from_document(&d.view()).expect("a PDF/X intent");
    assert_eq!(x.profile, b"ABCD");
    assert_eq!(x.condition.as_deref(), Some("FOGRA39"));
    assert_eq!(x.registry_name.as_deref(), Some("http://www.color.org"));
    assert_eq!(x.info.as_deref(), Some("Offset"));
}

#[test]
fn a_registered_condition_with_no_profile_gives_none() {
    let d = doc(
        "<< /Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier (FOGRA39) >> \
         << /S /GTS_PDFX /DestOutputProfile 4 0 R >>",
        &PROFILES,
    );
    assert!(output_intent(&d.view()).is_none());
    assert!(PdfxOutputIntent::from_document(&d.view()).is_none());
}

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::tests::{build, content, dict, id, session};
use super::*;
use crate::document::Document;
use crate::edit::EditSession;
use crate::object::{Dict, Object};
use crate::space_audit::tests::stream;
use crate::writer::SaveOptions;

const URL: &[u8] = b"https://pdfa.org/resource/extension-brotli";

/// 1 catalog, 2 pages, 3 page, 4 plain content; `extra` appended from 5.
fn doc(catalog_extra: &str, extra: &[Vec<u8>]) -> Vec<u8> {
    let mut bodies = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>").into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>".to_vec(),
        stream("", &content()),
    ];
    bodies.extend_from_slice(extra);
    build(&bodies)
}

fn brotli(quality: u8) -> OptimizeSettings {
    let mut s = OptimizeSettings::new();
    s.streams = StreamAction::Brotli { quality };
    s
}

/// Plan and apply `settings`, returning the session and its plan.
fn run(bytes: Vec<u8>, settings: &OptimizeSettings) -> (EditSession, OptimizePlan) {
    let mut s = session(bytes);
    let plan = s.plan_optimize(settings).unwrap();
    s.apply_optimize(&plan).unwrap();
    (s, plan)
}

fn saved(s: &EditSession, plan: &OptimizePlan) -> Document {
    let (file, _) = s
        .save_optimized(plan, &SaveOptions::default(), &RebuildOptions::none())
        .unwrap();
    let bytes = match file {
        OptimizedFile::Smaller(b) | OptimizedFile::NotSmaller { bytes: b, .. } => b,
        other => panic!("expected a written file, got {other:?}"),
    };
    Document::from_bytes(bytes).unwrap()
}

fn is_brotli(o: &Object) -> bool {
    o.as_dict()
        .and_then(|d| d.get(b"URL"))
        .is_some_and(|u| matches!(u, Object::String(s) if s == URL))
}

fn pdfa_entry(catalog: &Dict) -> Object {
    catalog
        .get(b"Extensions")
        .and_then(Object::as_dict)
        .and_then(|e| e.get(b"PDFa"))
        .cloned()
        .expect("an /Extensions /PDFa entry")
}

#[test]
fn brotli_spellings_parse_and_print() {
    assert_eq!(
        StreamAction::parse("brotli"),
        Ok(StreamAction::Brotli { quality: 11 })
    );
    let four = StreamAction::parse("brotli-4").unwrap();
    assert_eq!(four, StreamAction::Brotli { quality: 4 });
    assert_eq!(four.to_profile_string(), "brotli-4");
    assert!(StreamAction::parse("brotli-12").is_err());
    assert!(!four.is_lossy());
}

#[test]
fn a_brotli_plan_writes_brotli_streams_the_extension_and_version_2() {
    let (s, plan) = run(doc("", &[]), &brotli(11));
    assert!(plan.uses_brotli());
    assert_eq!(
        plan.version_raise(),
        Some(crate::PdfVersion { major: 2, minor: 0 })
    );
    assert!(plan.notes.iter().any(|n| n.contains("Brotli extension")));
    let after = saved(&s, &plan);
    let Some(Object::Stream(c)) = after.get(id(4)).map(|o| o.value.clone()) else {
        panic!("object 4 is not a stream");
    };
    assert_eq!(
        c.dict.get(b"Filter"),
        Some(&Object::Name(crate::object::Name::from(b"BrotliDecode")))
    );
    let raw = after.bytes()[c.data_span.start..c.data_span.start + c.data_span.len].to_vec();
    assert_eq!(
        crate::filters::decode_stream(&c.dict, &raw).unwrap(),
        content()
    );
    let catalog = after
        .get(id(1))
        .and_then(|o| o.value.as_dict().cloned())
        .unwrap();
    assert!(is_brotli(&pdfa_entry(&catalog)));
    assert_eq!(
        catalog.get(b"Version"),
        Some(&Object::Name(crate::object::Name::from(b"2.0")))
    );
}

#[test]
fn other_developers_extensions_and_an_earlier_pdfa_entry_are_kept() {
    let ext = "/Extensions << /ADBE << /BaseVersion /1.7 /ExtensionLevel 3 >> \
               /PDFa << /BaseVersion /2.0 /ExtensionLevel 9 /URL (https://example.org/x) >> >>";
    let (s, _) = run(doc(ext, &[]), &brotli(5));
    let catalog = dict(&s, 1);
    let ext = catalog
        .get(b"Extensions")
        .and_then(Object::as_dict)
        .unwrap();
    assert!(ext.contains_key(b"ADBE"));
    let Object::Array(items) = pdfa_entry(&catalog) else {
        panic!("the earlier /PDFa entry and the Brotli one become an array");
    };
    assert_eq!(items.len(), 2);
    assert!(!is_brotli(&items[0]) && is_brotli(&items[1]));
}

#[test]
fn an_indirect_extensions_dictionary_is_patched_where_it_lives() {
    let (s, _) = run(
        doc(
            "/Extensions 5 0 R",
            &[b"<< /ADBE << /ExtensionLevel 3 >> >>".to_vec()],
        ),
        &brotli(5),
    );
    assert!(matches!(
        dict(&s, 1).get(b"Extensions"),
        Some(Object::Reference(_))
    ));
    let ext = dict(&s, 5);
    assert!(ext.contains_key(b"ADBE"));
    assert!(is_brotli(ext.get(b"PDFa").unwrap()));
}

#[test]
fn an_existing_brotli_entry_is_not_added_twice() {
    let ext = "/Version /2.0 /Extensions << /PDFa [ << /URL (https://example.org/x) >> \
               << /URL (https://pdfa.org/resource/extension-brotli) >> ] >>";
    let original = doc(ext, &[]);
    let before = session(original.clone());
    let (s, plan) = run(original, &brotli(5));
    assert!(plan.uses_brotli());
    assert_eq!(dict(&s, 1), dict(&before, 1), "the catalog is untouched");
}

#[test]
fn a_pdfa_claim_skips_brotli_by_name_and_leaves_the_catalog() {
    let xmp = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" pdfaid:part="2" pdfaid:conformance="B"/></rdf:RDF></x:xmpmeta>"#;
    let bytes = doc(
        "/Metadata 5 0 R",
        &[stream("/Type /Metadata /Subtype /XML", xmp)],
    );
    let before = session(bytes.clone());
    let (s, plan) = run(bytes, &brotli(11));
    assert!(plan.changes.is_empty());
    assert!(
        plan.skipped
            .iter()
            .any(|k| k.id == id(4) && k.reason == SkipReason::BrotliUnderPdfa)
    );
    assert!(!plan.uses_brotli());
    assert_eq!(dict(&s, 1), dict(&before, 1));
}

#[test]
fn one_undo_removes_the_streams_and_the_catalog_change() {
    let original = doc("", &[]);
    let before = session(original.clone());
    let (mut s, _) = run(original, &brotli(11));
    assert_ne!(dict(&s, 1), dict(&before, 1));
    s.undo();
    assert!(!s.is_modified());
    assert_eq!(dict(&s, 1), dict(&before, 1));
}

#[test]
fn a_brotli_stream_is_decoded_by_recompress_rather_than_refused() {
    // Quality 0 on a padded run leaves room for Flate 9 to win.
    let mut plain = content();
    plain.extend(std::iter::repeat_n(b' ', 50_000));
    let packed = crate::filters::brotli::encode(&plain, 0).unwrap();
    let bytes = build(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>".to_vec(),
        stream("/Filter /BrotliDecode", &packed),
    ]);
    let mut settings = OptimizeSettings::new();
    settings.streams = StreamAction::Recompress { level: 9 };
    let plan = session(bytes).plan_optimize(&settings).unwrap();
    let skip = plan.skipped.iter().find(|k| k.id == id(4));
    assert!(
        !matches!(
            skip.map(|k| &k.reason),
            Some(SkipReason::UnsupportedFilter(_))
        ),
        "{skip:?}"
    );
    if let Some(c) = plan.changes.iter().find(|c| c.id == id(4)) {
        assert_eq!(c.filters_before, ["BrotliDecode"]);
    }
    assert!(!plan.uses_brotli());
}

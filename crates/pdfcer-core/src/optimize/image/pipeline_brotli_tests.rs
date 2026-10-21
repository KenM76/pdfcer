#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::pipeline_lzw_tests::ramp;
use super::pipeline_tests::{AT_100, doc, plan, skip};
use super::{ImageEncode, ImageSkip};
use crate::PdfVersion;
use crate::document::Document;
use crate::edit::EditSession;
use crate::filters;
use crate::object::{Name, ObjId, Object};
use crate::optimize::tests::build;
use crate::optimize::{OptimizedFile, RebuildOptions, SkipReason, StreamAction};
use crate::space_audit::tests::stream;
use crate::writer::SaveOptions;

const PDF_2_0: PdfVersion = PdfVersion { major: 2, minor: 0 };

#[test]
fn encode_brotli_parses_and_prints() {
    let a = StreamAction::parse("image(encode=brotli)").unwrap();
    assert_eq!(a.to_string(), "image(encode=brotli-11)");
    let b = StreamAction::parse("image(encode=brotli-4)").unwrap();
    assert_eq!(b.to_string(), "image(encode=brotli-4)");
    assert!(StreamAction::parse("image(encode=brotli-12)").is_err());
    assert!(!b.is_lossy());
}

#[test]
fn an_image_is_written_as_brotli_with_the_extension_and_version_2() {
    let (obj, px) = ramp();
    let (mut s, p) = plan(doc(AT_100, &[obj]), &[("image(encode=brotli)", None)]);
    let c = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    let img = c.image.as_ref().unwrap();
    assert_eq!(img.encoded_as, ImageEncode::Brotli { quality: 11 });
    assert!(!c.lossy);
    assert!(img.flate_bytes.is_some_and(|f| f > 0));
    assert_eq!(img.raises_version, Some(PDF_2_0));
    assert!(p.uses_brotli());
    assert!(p.notes.iter().any(|n| n.contains("Brotli extension")));
    s.apply_optimize(&p).unwrap();
    let OptimizedFile::Smaller(bytes) = s
        .save_optimized(&p, &SaveOptions::default(), &RebuildOptions::none())
        .unwrap()
        .0
    else {
        panic!("expected a smaller file")
    };
    let d = Document::from_bytes(bytes).unwrap();
    assert_eq!(d.version(), PDF_2_0);
    let catalog = d.catalog().unwrap();
    let ext = catalog
        .get(b"Extensions")
        .and_then(Object::as_dict)
        .unwrap();
    assert!(ext.contains_key(b"PDFa"));
    let Some(Object::Stream(st)) = d.get(ObjId::new(5, 0)).map(|o| o.value.clone()) else {
        panic!()
    };
    assert_eq!(
        st.dict.get(b"Filter"),
        Some(&Object::Name(Name::from(b"BrotliDecode")))
    );
    let s = EditSession::new(d);
    let raw = s.view().slice(st.data_span).unwrap();
    assert_eq!(filters::decode_stream(&st.dict, raw).unwrap(), px);
}

#[test]
fn a_document_claiming_pdfa_refuses_brotli_by_name() {
    let (obj, _) = ramp();
    let xmp = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" pdfaid:part="2" pdfaid:conformance="B"/></rdf:RDF></x:xmpmeta>"#;
    let bytes = build(&[
        b"<< /Type /Catalog /Pages 2 0 R /Metadata 6 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Contents 4 0 R \
           /Resources << /XObject << /Im5 5 0 R >> >> >>"
            .to_vec(),
        stream("", AT_100.as_bytes()),
        obj,
        stream("/Type /Metadata /Subtype /XML", xmp),
    ]);
    let (_, p) = plan(bytes, &[("image(encode=brotli)", None)]);
    assert!(matches!(
        skip(&p, 5),
        Some(SkipReason::ImageUnsupported(ImageSkip::BrotliUnderPdfa))
    ));
    assert!(!p.uses_brotli());
}

#[test]
fn version_raise_refuse_skips_brotli_below_pdf_2() {
    let (obj, _) = ramp();
    let (_, p) = plan(
        doc(AT_100, &[obj]),
        &[("image(encode=brotli,version-raise=refuse)", None)],
    );
    assert!(matches!(
        skip(&p, 5),
        Some(SkipReason::ImageUnsupported(ImageSkip::VersionTooLow {
            needs: PDF_2_0,
            ..
        }))
    ));
    assert!(p.version_raise().is_none());
}

#[test]
fn auto_never_tries_brotli() {
    let (obj, _) = ramp();
    let (_, p) = plan(doc(AT_100, &[obj]), &[("image(encode=auto)", None)]);
    assert!(!p.uses_brotli());
    for c in &p.changes {
        let img = c.image.as_ref().unwrap();
        assert!(
            img.candidates
                .iter()
                .all(|(e, _)| !matches!(e, ImageEncode::Brotli { .. }))
        );
    }
}

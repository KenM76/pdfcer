#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::pipeline_tests::{AT_100, N, doc, plan, skip};
use super::{ImageEncode, ImageSkip};
use crate::document::Document;
use crate::edit::EditSession;
use crate::filters::{self, FilterNotes, lzw};
use crate::object::{ObjId, Object};
use crate::optimize::tests::build;
use crate::optimize::{OptimizedFile, RebuildOptions, SkipReason};
use crate::space_audit::tests::stream;
use crate::writer::SaveOptions;

/// 300 x 300 grey, a horizontal ramp repeated on every row, stored
/// unfiltered: LZW and Flate both compress it well.
fn ramp() -> (Vec<u8>, Vec<u8>) {
    let px: Vec<u8> = (0..N * N).map(|i| ((i % N) * 255 / N) as u8).collect();
    let obj = stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {N} /Height {N} /ColorSpace /DeviceGray \
             /BitsPerComponent 8"
        ),
        &px,
    );
    (obj, px)
}

#[test]
fn encode_lzw_parses_and_prints() {
    let a = crate::optimize::StreamAction::parse("image(encode=lzw)").unwrap();
    assert_eq!(a.to_string(), "image(encode=lzw)");
}

#[test]
fn an_image_is_written_as_lzw_with_the_flate_size_beside_it() {
    let (obj, px) = ramp();
    let (mut s, p) = plan(doc(AT_100, &[obj]), &[("image(encode=lzw)", None)]);
    let c = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    let img = c.image.as_ref().unwrap();
    assert_eq!(img.encoded_as, ImageEncode::Lzw);
    assert!(!c.lossy);
    let flate = img.flate_bytes.unwrap();
    assert!(
        flate > 0 && flate < c.after_bytes,
        "{flate} vs {}",
        c.after_bytes
    );
    s.apply_optimize(&p).unwrap();
    let OptimizedFile::Smaller(bytes) = s
        .save_optimized(&p, &SaveOptions::default(), &RebuildOptions::none())
        .unwrap()
        .0
    else {
        panic!("expected a smaller file")
    };
    let d = Document::from_bytes(bytes).unwrap();
    let Some(Object::Stream(st)) = d.get(ObjId::new(5, 0)).map(|o| o.value.clone()) else {
        panic!()
    };
    assert_eq!(
        st.dict.get(b"Filter"),
        Some(&Object::Name(b"LZWDecode"[..].into()))
    );
    let s = EditSession::new(d);
    let raw = s.view().slice(st.data_span).unwrap();
    assert_eq!(filters::decode_stream(&st.dict, raw).unwrap(), px);
    if st.dict.get(b"DecodeParms").is_none() {
        let mut notes = FilterNotes::default();
        assert_eq!(lzw::decode(raw, None, &mut notes).unwrap(), px);
    }
}

#[test]
fn a_document_claiming_pdfa_refuses_lzw_by_name() {
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
    let (_, p) = plan(bytes, &[("image(encode=lzw)", None)]);
    assert!(matches!(
        skip(&p, 5),
        Some(SkipReason::ImageUnsupported(ImageSkip::LzwUnderPdfa))
    ));
    assert!(p.changes.iter().all(|c| c.id.num != 5));
}

#[test]
fn flate_output_carries_no_flate_size() {
    let (obj, _) = ramp();
    let (_, p) = plan(doc(AT_100, &[obj]), &[("image(encode=flate)", None)]);
    let c = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    assert_eq!(c.image.as_ref().unwrap().flate_bytes, None);
}

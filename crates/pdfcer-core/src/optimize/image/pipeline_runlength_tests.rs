#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::ImageEncode;
use super::pipeline_tests::{AT_100, N, doc, grey, plan, skip};
use crate::document::Document;
use crate::edit::EditSession;
use crate::filters::{self, runlength};
use crate::object::{ObjId, Object};
use crate::optimize::{OptimizedFile, RebuildOptions, SkipReason};
use crate::space_audit::tests::stream;
use crate::writer::SaveOptions;

/// 300 x 300 grey in ten flat horizontal bands, stored unfiltered.
fn bands() -> (Vec<u8>, Vec<u8>) {
    let px: Vec<u8> = (0..N * N).map(|i| ((i / N / 30) * 25) as u8).collect();
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
fn encode_runlength_parses_and_prints() {
    let a = crate::optimize::StreamAction::parse("image(encode=runlength)").unwrap();
    assert_eq!(a.to_string(), "image(encode=runlength)");
}

#[test]
fn flat_artwork_is_written_as_runlength_and_decodes_to_the_same_samples() {
    let (obj, px) = bands();
    let (mut s, p) = plan(doc(AT_100, &[obj]), &[("image(encode=runlength)", None)]);
    let c = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    assert_eq!(c.image.as_ref().unwrap().encoded_as, ImageEncode::RunLength);
    assert!(!c.lossy);
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
        Some(&Object::Name(b"RunLengthDecode"[..].into()))
    );
    let s = EditSession::new(d);
    let raw = s.view().slice(st.data_span).unwrap();
    assert_eq!(runlength::decode(raw).unwrap(), px);
    assert_eq!(filters::decode_stream(&st.dict, raw).unwrap(), px);
}

#[test]
fn noise_is_not_smaller_as_runlength_and_is_kept() {
    let (_, p) = plan(
        doc(AT_100, &[grey("")]),
        &[("image(encode=runlength)", None)],
    );
    assert!(matches!(skip(&p, 5), Some(SkipReason::NotSmaller { .. })));
}

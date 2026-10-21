#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::pipeline_ccitt_tests::{GREY_1, dashes, one_bit, saved};
use super::pipeline_tests::{AT_100, doc, grey, plan, skip};
use super::{ImageEncode, ImageSkip};
use crate::object::Object;
use crate::optimize::SkipReason;

#[test]
fn encode_jbig2_parses_and_prints() {
    let a = crate::optimize::StreamAction::parse("image(encode=jbig2)").unwrap();
    assert_eq!(a.to_string(), "image(encode=jbig2)");
}

/// Both polarities of the picture come back as the same samples, with
/// no `/DecodeParms` and no `/Decode` added.
#[test]
fn a_one_bit_image_is_written_as_jbig2_and_decodes_to_the_same_samples() {
    for invert in [false, true] {
        let px = dashes(invert);
        let (s, p) = plan(
            doc(AT_100, &[one_bit(GREY_1, &px)]),
            &[("image(encode=jbig2)", None)],
        );
        let c = p.changes.iter().find(|c| c.id.num == 5).unwrap();
        assert_eq!(c.image.as_ref().unwrap().encoded_as, ImageEncode::Jbig2);
        assert!(!c.lossy);
        let (dict, samples) = saved(s, &p);
        assert_eq!(
            dict.get(b"Filter"),
            Some(&Object::Name(b"JBIG2Decode"[..].into()))
        );
        assert_eq!(dict.get(b"DecodeParms"), None);
        assert_eq!(dict.get(b"Decode"), None);
        assert_eq!(samples, px, "invert={invert}");
    }
}

#[test]
fn an_image_mask_keeps_its_decode_array() {
    let px = dashes(true);
    let (s, p) = plan(
        doc(AT_100, &[one_bit("/ImageMask true /Decode [1 0]", &px)]),
        &[("image(encode=jbig2)", None)],
    );
    let (dict, samples) = saved(s, &p);
    assert_eq!(dict.get(b"ImageMask"), Some(&Object::Boolean(true)));
    assert_eq!(
        dict.get(b"Decode"),
        Some(&Object::Array(vec![Object::Integer(1), Object::Integer(0)]))
    );
    assert_eq!(samples, px);
}

#[test]
fn a_grey_image_is_not_bilevel_unless_converted() {
    let (_, p) = plan(doc(AT_100, &[grey("")]), &[("image(encode=jbig2)", None)]);
    assert!(matches!(
        skip(&p, 5),
        Some(SkipReason::ImageUnsupported(ImageSkip::NotBilevel(
            ImageEncode::Jbig2
        )))
    ));
    let (_, p) = plan(
        doc(AT_100, &[grey("")]),
        &[("image(encode=jbig2)", Some(5))],
    );
    assert!(matches!(
        skip(&p, 5),
        Some(SkipReason::ImageUnsupported(ImageSkip::NotBilevelOverride(
            ImageEncode::Jbig2
        )))
    ));
    let (s, p) = plan(
        doc(AT_100, &[grey("")]),
        &[("image(convert=bilevel,encode=jbig2)", Some(5))],
    );
    let (dict, _) = saved(s, &p);
    assert_eq!(
        dict.get(b"Filter"),
        Some(&Object::Name(b"JBIG2Decode"[..].into()))
    );
}

/// JBIG2Decode is PDF 1.4; a PDF 1.3 document is skipped by name.
#[test]
fn a_document_older_than_pdf_1_4_is_skipped_by_version() {
    let mut bytes = doc(AT_100, &[one_bit(GREY_1, &dashes(false))]);
    assert!(bytes.starts_with(b"%PDF-1.7"));
    bytes[7] = b'3';
    let (_, p) = plan(bytes, &[("image(encode=jbig2)", None)]);
    let why = skip(&p, 5).unwrap();
    assert!(matches!(
        why,
        SkipReason::ImageUnsupported(ImageSkip::VersionTooLow {
            encode: ImageEncode::Jbig2,
            ..
        })
    ));
    assert!(why.to_string().contains("needs PDF 1.4"), "{why}");
}

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::pipeline_ccitt_tests::{GREY_1, dashes, one_bit, saved};
use super::pipeline_tests::{AT_100, doc, grey, plan, skip};
use super::{ImageEncode, ImageSkip};
use crate::optimize::SkipReason;

const FLATE_9: ImageEncode = ImageEncode::Flate { level: 9 };

fn tried(c: &[(ImageEncode, u64)]) -> Vec<ImageEncode> {
    c.iter().map(|&(e, _)| e).collect()
}

#[test]
fn encode_auto_parses_and_prints() {
    let a = crate::optimize::StreamAction::parse("image(encode=auto)").unwrap();
    assert_eq!(a.to_string(), "image(encode=auto)");
}

/// A 1-bit image tries all four candidates in tie-break order, writes the
/// smallest, and decodes to the same samples.
#[test]
fn a_one_bit_image_is_written_as_the_smallest_of_four() {
    let px = dashes(false);
    let (s, p) = plan(
        doc(AT_100, &[one_bit(GREY_1, &px)]),
        &[("image(encode=auto)", None)],
    );
    let img = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    let img = img.image.as_ref().unwrap();
    let order = [
        FLATE_9,
        ImageEncode::Ccitt,
        ImageEncode::RunLength,
        ImageEncode::Jbig2,
    ];
    assert_eq!(tried(&img.candidates), order);
    let least = img.candidates.iter().map(|&(_, n)| n).min().unwrap();
    let first = img.candidates.iter().find(|&&(_, n)| n == least).unwrap();
    assert_eq!(img.encoded_as, first.0);
    let (_, samples) = saved(s, &p);
    assert_eq!(samples, px);
}

/// JBIG2 is not a candidate where the document's version predates it, and
/// `auto` never asks for a raise.
#[test]
fn a_pdf_1_3_document_drops_jbig2_from_the_candidates() {
    let mut bytes = doc(AT_100, &[one_bit(GREY_1, &dashes(false))]);
    bytes[7] = b'3';
    let (_, p) = plan(bytes, &[("image(encode=auto)", None)]);
    let img = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    let img = img.image.as_ref().unwrap();
    assert_eq!(
        tried(&img.candidates),
        [FLATE_9, ImageEncode::Ccitt, ImageEncode::RunLength]
    );
}

/// Noise compresses under no candidate: the image is kept and the reason
/// names the stored size and both grey candidates.
#[test]
fn an_image_no_candidate_shrinks_is_kept_with_every_size() {
    let (_, p) = plan(doc(AT_100, &[grey("")]), &[("image(encode=auto)", None)]);
    let why = skip(&p, 5).unwrap();
    let SkipReason::ImageUnsupported(ImageSkip::AutoNotSmaller { candidates, .. }) = &why else {
        panic!("{why:?}")
    };
    assert_eq!(tried(candidates), [FLATE_9, ImageEncode::RunLength]);
    let text = why.to_string();
    assert!(
        text.contains("flate-9 ") && text.contains("runlength "),
        "{text}"
    );
}

/// A source this pipeline does not decode is left as stored, silently,
/// as under `keep`.
#[test]
fn an_undecodable_source_is_left_as_stored() {
    let (_, p) = plan(
        doc(
            AT_100,
            &[one_bit(
                "/Filter /JBIG2Decode /ColorSpace /DeviceGray /BitsPerComponent 1",
                b"x",
            )],
        ),
        &[("image(encode=auto)", None)],
    );
    assert!(p.changes.iter().all(|c| c.id.num != 5));
    assert_eq!(skip(&p, 5), None);
}

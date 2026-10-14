//! A clipboard payload carrying NaN or an infinity is refused: pdfcer never
//! writes one, and NaN != NaN would leave a parsed clip unequal to its own
//! round trip (found by the `clipboard_payloads` fuzz target).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::formclip::FieldClip;
use pdfcer_core::vector::{ClipError, ObjectClip};
use std::path::Path;

/// The fuzzer's input: an empty object clip whose bounding box holds a NaN.
const NAN_BBOX: &[u8] = b"PDFCECLIP\x00\x00\x01\x00\x00\x00\x00\
\x00\x00\x94\x94\x94\x94\x94\x00\x00\x00\x00\x00\x00\x00\x00\x00\
\x00\x00\x00\x00\x00\xf7\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\
\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x94\x94\
\x94\x94q;P\x00\x00";

#[test]
fn an_object_clip_with_a_nan_coordinate_is_refused() {
    assert!(matches!(
        ObjectClip::from_bytes(NAN_BBOX),
        Err(ClipError::Content(_))
    ));
}

#[test]
fn a_field_clip_with_an_infinite_coordinate_is_refused() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms/field-set-form.pdf");
    let s = EditSession::new(Document::load(&path).unwrap());
    let mut bytes = s.copy_field("Total").unwrap().to_bytes();
    // Total's widget rect starts at x = 20.
    let twenty = 20.0f64.to_le_bytes();
    let at = bytes
        .windows(8)
        .position(|w| w == twenty)
        .expect("the rect is in the payload");
    bytes[at..at + 8].copy_from_slice(&f64::INFINITY.to_le_bytes());
    assert!(matches!(
        FieldClip::from_bytes(&bytes),
        Err(ClipError::Content(_))
    ));
}

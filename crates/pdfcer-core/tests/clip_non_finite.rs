//! Clipboard payloads pdfcer never writes are refused, since each would
//! leave a parsed clip unequal to its own round trip (both found by the
//! `clipboard_payloads` fuzz target).

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

/// The fuzzer's second input: a name object carrying stream data, which the
/// writer drops, so the clip differed from its own round trip.
const PAYLOAD_ON_A_NAME: &[u8] = b"PDFCECLIP\x00\x00\x01\x00\x00\x00\x00\x00\x00\xff\xff\xff\xef\
\xff\x94\x94\x95\x94vkPDFCECLIP\x00\x00\x01EFLD\x00\x00\x00\x00\
\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x0d\x00\x00\x00\
\x00/PDFCEFLD\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00\
\xc5\x04\x1e\x00\x00\x00888<\x008\x00\x008\x0a\x0a\x0a\x0a\x0a\
\x0a\x0aDFCEF\x00D\x0a\xe5\xdf\xdc\xdc\xdc\xc5\xdc\xdc\xdc\xdc\
\xdc\xdc\xdc\xdc\xdc\xdc\xdc\xdc\xdcLDEC\xdc\xdc\xdc\xdc\x01\
\x00\x00\x00\xff\xff\xff\xff\xff\x00\x00\xd5\x00\x80\xae\x00";

#[test]
fn stream_data_beside_a_non_dictionary_is_refused() {
    assert!(matches!(
        ObjectClip::from_bytes(PAYLOAD_ON_A_NAME),
        Err(ClipError::Content(_))
    ));
}

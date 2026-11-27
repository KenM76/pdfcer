//! Shared fuzz oracle for the evidence bounds (decision 227): a
//! `DeclaredGeometryExceedsData` refusal must name the bytes it was given
//! and a minimum above them, or it refused a stream its own bound admits.

use pdfcer_core::image_codec::ImageCodecError;

/// Panics when `result` is an evidence refusal inconsistent with `len`.
pub fn assert_evidence_holds<T>(result: &Result<T, ImageCodecError>, len: usize) {
    if let Err(ImageCodecError::DeclaredGeometryExceedsData {
        min_bytes,
        have_bytes,
        ..
    }) = result
    {
        assert_eq!(
            *have_bytes, len as u64,
            "refusal misreports the data length"
        );
        assert!(min_bytes > have_bytes, "refused a stream the bound admits");
    }
}

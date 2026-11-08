//! Lossless rewrite of a sequential JPEG whose components are coded in
//! separate scans into one interleaved scan (ITU-T T.81 A.2.2 → A.2.3).
//!
//! T.81 permits both layouts for baseline and extended-sequential Huffman
//! coding. Some decoders mis-read the non-interleaved one when chroma is
//! subsampled (`zune-jpeg` 0.5.15 does; `jpeg-encoder` writes the layout
//! whenever optimized Huffman tables are on). [`interleave`] decodes the
//! quantized coefficients and re-encodes them unchanged, so a correct
//! decoder produces identical pixels from the input and the output.
//!
//! ```
//! let jpeg: &[u8] = b"\xFF\xD8\xFF\xD9";
//! let fixed = pdfcer_jpeg_remux::interleave(jpeg, 1 << 20)?;
//! let to_decode = fixed.as_deref().unwrap_or(jpeg);
//! assert_eq!(to_decode, jpeg, "nothing to rewrite here");
//! # Ok::<(), pdfcer_jpeg_remux::RemuxError>(())
//! ```
#![forbid(unsafe_code)]

mod bits;
mod frame;
mod huffman;
mod scan;

/// Why a codestream that needed the rewrite could not be rewritten.
///
/// A caller that only wants a workaround can decode the original bytes on
/// any error: the rewrite never applies to a stream it cannot fully read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RemuxError {
    /// The codestream breaks a T.81 rule the rewrite depends on.
    #[error("malformed JPEG: {0}")]
    Malformed(&'static str),
    /// Holding the coefficients needs more 8×8 blocks than the ceiling.
    #[error("JPEG needs {needed} coefficient blocks, over the ceiling of {ceiling}")]
    TooLarge {
        /// Blocks the frame needs, padded to whole MCUs.
        needed: usize,
        /// The `max_blocks` the caller passed.
        ceiling: usize,
    },
}

/// Rewrites `data` as one interleaved scan when it is an 8-bit sequential
/// Huffman JPEG (`SOF0`/`SOF1`) with subsampled components coded in more
/// than one scan; returns `Ok(None)` for every other stream.
///
/// `max_blocks` bounds memory: the rewrite holds every component's
/// quantized coefficients at 128 bytes per 8×8 block, and refuses a frame
/// needing more blocks than this.
///
/// The output keeps every marker segment before the first scan except
/// `DHT` and `DRI`, byte-for-byte and in order (`APPn`/`COM` met between
/// scans are moved there too). It carries the T.81 Annex K.3 Huffman
/// tables (luminance tables for the first component, chrominance for the
/// rest) and no restart interval.
///
/// # Errors
///
/// [`RemuxError::Malformed`] when the stream needed the rewrite but breaks
/// a rule it depends on; [`RemuxError::TooLarge`] past `max_blocks`.
pub fn interleave(data: &[u8], max_blocks: usize) -> Result<Option<Vec<u8>>, RemuxError> {
    let Some(parsed) = frame::read(data, max_blocks)? else {
        return Ok(None);
    };
    scan::write(&parsed).map(Some)
}

#[cfg(test)]
mod tests;

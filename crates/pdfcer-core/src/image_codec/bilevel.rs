//! # The 1-bit sample sink shared by the two fax codecs
//!
//! `CCITTFaxDecode` (§7.4.6) and `JBIG2Decode` (§7.4.7) are the two PDF
//! filters that Table 89 says "shall always deliver 1-bit samples". Both
//! of pdfcer's chosen decoders — `hayro-ccitt` and `hayro-jbig2` — hand
//! their output to the caller through a **push sink trait** rather than
//! returning a buffer:
//!
//! ```text
//! trait Decoder {
//!     fn push_pixel(&mut self, colour: bool);
//!     fn push_pixel_chunk(&mut self, colour: bool, chunk_count: u32);
//!     fn next_line(&mut self);
//! }
//! ```
//!
//! That shape is the reason decision 005 §3.5 preferred these crates over
//! `fax`: the sink is where pdfcer's **own** row stride, byte padding and
//! resource ceiling live, so none of that has to be reconstructed from a
//! vendor-shaped buffer afterwards, and the ceiling can be enforced
//! *while* decoding rather than after (rule R25, and the same
//! "abort mid-decode, never inflate-then-check" discipline
//! [`crate::filters`] already promises for Flate).
//!
//! ## What this type produces
//!
//! Exactly §8.9.3's packing: one bit per sample, samples left to right,
//! **each row padded to a byte boundary**, rows top to bottom, no
//! padding between rows beyond that. §7.4.6's first bit-orientation rule
//! says the same thing from the filter's side — "unencoded data shall be
//! treated as complete scan lines, with unused bits inserted at the end
//! of each scan line to fill out the last byte. This approach is
//! compatible with the PDF convention for sampled image data."
//!
//! So the row stride is `ceil(columns / 8)` bytes, which is also what
//! Table 11's `Columns` rule ("if the value is not a multiple of 8, the
//! filter shall adjust the width of the unencoded image to the next
//! multiple of 8 so that each line starts on a byte boundary") describes.
//! pdfcer reports `width = columns`, *not* the rounded-up width: the
//! rounding is a property of the byte layout, not of the picture, and
//! `pdfcer-render` derives the identical stride from `columns` with its
//! own `row_stride` helper. Reporting the rounded width would paint the
//! padding.
//!
//! ## Polarity is the CALLER's decision, not this type's
//!
//! This sink speaks one word: **white**. `push_white(true)` writes a
//! `1` bit, `push_white(false)` writes a `0` bit, and that is the whole
//! contract — because "0 is black" is the normal PDF convention for
//! bilevel image data, stated in Table 11's own `BlackIs1` description
//! ("1 bits shall be interpreted as black pixels and 0 bits as white
//! pixels, **the reverse of the normal PDF convention for image data**").
//! With the DeviceGray default `Decode [0 1]`, sample 0 maps to grey 0.0
//! — black. So a white pixel is a `1` bit, always, and both adapters
//! translate their vendor's colour convention into this one:
//!
//! - [`super::ccitt`] sets `hayro-ccitt`'s `invert_black` from
//!   `/BlackIs1` and forwards the crate's own `white` flag unchanged.
//! - [`super::jbig2`] inverts: T.88 bitmaps use `1 = black`, so
//!   `push_white(!black)`.
//!
//! Keeping the inversion in the adapters rather than here means each
//! module documents the *one* polarity rule it is responsible for, and a
//! future third bilevel codec cannot silently inherit the wrong one.
//!
//! ## The ceiling (rule R25)
//!
//! `budget` is a hard byte cap computed by the caller from
//! [`MAX_IMAGE_PIXELS`](super::MAX_IMAGE_PIXELS) and
//! [`MAX_IMAGE_SAMPLE_BYTES`](super::MAX_IMAGE_SAMPLE_BYTES) — never
//! from a vendor default, because neither hayro crate has one: both
//! allocate strictly from the geometry the caller hands them, which puts
//! the entire responsibility on pdfcer. Once the budget is crossed the
//! sink stops appending and latches [`BilevelSink::overflowed`]; the
//! adapter turns that into [`ImageCodecError::TooLarge`]. It cannot be
//! an early `return Err` because the vendor traits are infallible by
//! signature — which is exactly why the latch exists.
//!
//! [`ImageCodecError::TooLarge`]: super::ImageCodecError::TooLarge

/// A push sink that packs 1-bit samples into §8.9.3's byte layout under
/// a fixed byte budget.
///
/// Constructed by [`BilevelSink::new`], driven by whichever vendor
/// `Decoder` impl the adapter installs, and consumed by
/// [`BilevelSink::finish`].
#[derive(Debug)]
pub(super) struct BilevelSink {
    /// Packed samples, `stride` bytes per completed row.
    data: Vec<u8>,
    /// Bytes per row, `ceil(columns / 8)`.
    stride: usize,
    /// Hard ceiling on `data.len()` (rule R25).
    budget: usize,
    /// Index in `data` where the row currently being filled starts.
    row_start: usize,
    /// Bits accumulated but not yet flushed to `data` (0..8).
    accum: u8,
    /// How many bits `accum` holds.
    accum_len: u8,
    /// Completed rows.
    rows: u32,
    /// Latched once an append was refused by the budget.
    overflowed: bool,
}

impl BilevelSink {
    /// Create a sink for `columns`-wide rows with room for at most
    /// `max_rows` of them.
    ///
    /// `columns` must be non-zero — the adapters reject a zero or
    /// negative `/Columns` before reaching here, because a zero stride
    /// makes every row-relative index meaningless. A zero is tolerated
    /// (stride 1) rather than panicking, since this type is on a fuzzed
    /// path and a panic would be a finding in itself.
    pub(super) fn new(columns: u32, max_rows: u32) -> Self {
        let stride = (columns.max(1) as usize).div_ceil(8);
        // Saturating, then clamped: `max_rows` is already derived from
        // MAX_IMAGE_PIXELS by the caller, so this is belt-and-braces
        // against an arithmetic surprise rather than the primary guard.
        let budget = stride
            .saturating_mul(max_rows as usize)
            .min(super::MAX_IMAGE_SAMPLE_BYTES);
        Self {
            // Not pre-allocated to `budget`: the budget is a CEILING,
            // not an expectation. A stream that decodes two rows must
            // not cost the memory of a stream that decodes thirty
            // thousand.
            data: Vec::new(),
            stride,
            budget,
            row_start: 0,
            accum: 0,
            accum_len: 0,
            rows: 0,
            overflowed: false,
        }
    }

    /// Append one sample: `true` writes a `1` bit (white), `false` a `0`
    /// bit (black). See the module docs for why that mapping is fixed.
    pub(super) fn push_white(&mut self, white: bool) {
        self.accum = (self.accum << 1) | u8::from(white);
        self.accum_len += 1;
        if self.accum_len == 8 {
            let byte = self.accum;
            self.emit(byte);
            self.accum = 0;
            self.accum_len = 0;
        }
    }

    /// Append `chunks × 8` samples of one colour.
    ///
    /// Both vendor traits document that this is only called when the
    /// row is already byte-aligned, which makes it a plain byte fill —
    /// the fast path that keeps a full-width white run from costing one
    /// shift per pixel. The alignment is *checked* rather than assumed:
    /// if a future version of either crate relaxes that contract, the
    /// fallback still produces correct bits instead of shearing the row.
    pub(super) fn push_white_chunk(&mut self, white: bool, chunks: u32) {
        if self.accum_len != 0 {
            for _ in 0..chunks.saturating_mul(8) {
                self.push_white(white);
            }
            return;
        }
        let fill = if white { 0xFF } else { 0x00 };
        for _ in 0..chunks {
            self.emit(fill);
        }
    }

    /// Close the current row: flush any partial byte, pad to `stride`,
    /// and start the next row.
    ///
    /// The padding bits are `0`. Their value is unobservable — §8.9.3
    /// leaves them unspecified and `pdfcer-render` reads exactly
    /// `Columns` samples per row — so the choice is made for
    /// determinism (a byte-diffable fixture) rather than for appearance.
    pub(super) fn end_row(&mut self) {
        if self.accum_len != 0 {
            // Left-justify the partial byte: sample 0 of a row is the
            // MOST significant bit (§8.9.3), so the unused bits are the
            // low ones.
            let byte = self.accum << (8 - self.accum_len);
            self.emit(byte);
            self.accum = 0;
            self.accum_len = 0;
        }
        let want = self.row_start.saturating_add(self.stride);
        while self.data.len() < want {
            if !self.emit(0) {
                break;
            }
        }
        // Defensive: a vendor that over-pushed a row must not shift
        // every subsequent row. Neither hayro crate does (both clamp to
        // the declared line width), so this never fires today.
        self.data.truncate(want.min(self.data.len()));
        self.row_start = self.data.len();
        self.rows = self.rows.saturating_add(1);
    }

    /// Did an append cross the byte budget? (rule R25)
    pub(super) fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Completed rows so far.
    pub(super) fn rows(&self) -> u32 {
        self.rows
    }

    /// Take the packed samples and the number of complete rows.
    ///
    /// Any bits of an unterminated final row are discarded: a row the
    /// decoder never finished is not a scan line, and emitting a
    /// half-filled one would be exactly the "plausible-looking garbage"
    /// the fail-clean contract forbids.
    pub(super) fn finish(mut self) -> (Vec<u8>, u32) {
        self.data.truncate(self.row_start);
        (self.data, self.rows)
    }

    /// Append one byte if the budget allows; latch and refuse otherwise.
    /// Returns whether the byte was written.
    fn emit(&mut self, byte: u8) -> bool {
        if self.data.len() >= self.budget {
            self.overflowed = true;
            return false;
        }
        self.data.push(byte);
        true
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn a_full_byte_row_packs_msb_first() {
        // §8.9.3: sample 0 is the most significant bit.
        let mut sink = BilevelSink::new(8, 1);
        for white in [true, false, false, false, false, false, false, false] {
            sink.push_white(white);
        }
        sink.end_row();
        let (data, rows) = sink.finish();
        assert_eq!(rows, 1);
        assert_eq!(data, vec![0b1000_0000]);
    }

    #[test]
    fn a_partial_row_is_left_justified_and_padded_to_the_stride() {
        // 12 columns -> stride 2, and the last 4 bits are padding.
        let mut sink = BilevelSink::new(12, 1);
        for _ in 0..8 {
            sink.push_white(true);
        }
        for _ in 0..4 {
            sink.push_white(false);
        }
        sink.end_row();
        let (data, _) = sink.finish();
        assert_eq!(data, vec![0xFF, 0x00], "4 sample bits + 4 padding bits");
    }

    #[test]
    fn chunks_and_singles_interleave_correctly() {
        // 24 columns: one chunk of 8 white, then 8 singles, then a chunk.
        let mut sink = BilevelSink::new(24, 1);
        sink.push_white_chunk(true, 1);
        for _ in 0..8 {
            sink.push_white(false);
        }
        sink.push_white_chunk(true, 1);
        sink.end_row();
        let (data, _) = sink.finish();
        assert_eq!(data, vec![0xFF, 0x00, 0xFF]);
    }

    #[test]
    fn a_chunk_that_arrives_unaligned_still_produces_the_right_bits() {
        // The documented contract says this cannot happen; the fallback
        // exists so a vendor change degrades to slow, not to wrong.
        let mut sink = BilevelSink::new(16, 1);
        sink.push_white(true);
        sink.push_white_chunk(false, 1);
        for _ in 0..7 {
            sink.push_white(true);
        }
        sink.end_row();
        let (data, _) = sink.finish();
        assert_eq!(data, vec![0b1000_0000, 0b0111_1111]);
    }

    #[test]
    fn rows_accumulate_at_the_declared_stride() {
        let mut sink = BilevelSink::new(4, 3);
        for row in 0..3 {
            for i in 0..4 {
                sink.push_white(i == row);
            }
            sink.end_row();
        }
        let (data, rows) = sink.finish();
        assert_eq!(rows, 3);
        assert_eq!(data, vec![0b1000_0000, 0b0100_0000, 0b0010_0000]);
    }

    #[test]
    fn the_budget_latches_instead_of_growing_without_bound() {
        // Two rows of room, four rows pushed. The sink must stop
        // appending and say so — the adapter turns this into TooLarge.
        let mut sink = BilevelSink::new(8, 2);
        for _ in 0..4 {
            sink.push_white_chunk(true, 1);
            sink.end_row();
        }
        assert!(sink.overflowed());
        let (data, _) = sink.finish();
        assert_eq!(data.len(), 2, "never grew past the budget");
    }

    #[test]
    fn an_unterminated_final_row_is_discarded_not_half_emitted() {
        let mut sink = BilevelSink::new(8, 4);
        sink.push_white_chunk(true, 1);
        sink.end_row();
        // A second row that never reaches `end_row`.
        sink.push_white(true);
        sink.push_white(false);
        let (data, rows) = sink.finish();
        assert_eq!(rows, 1);
        assert_eq!(data, vec![0xFF]);
    }

    #[test]
    fn zero_columns_does_not_panic() {
        // The adapters reject this first; the sink must still be total.
        let mut sink = BilevelSink::new(0, 4);
        sink.push_white(true);
        sink.end_row();
        let (_, rows) = sink.finish();
        assert_eq!(rows, 1);
    }
}

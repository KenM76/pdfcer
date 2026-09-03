//! # Byte-span provenance — the foundation of round-trip / minimal-diff
//!
//! This module exists because of the single most important architecture
//! decision in pdfcer: **provenance is a first-class field, not an
//! optimization** (`docs/decisions/001-oxidize-pdf-adopt-vs-build.md`
//! §6.1 item 1, enacting `docs/ARCHITECTURE.md` §5).
//!
//! ## The contract
//!
//! Every syntactic entity parsed out of a PDF buffer — every token, every
//! COS object, every content-stream operator — records the exact byte
//! range of the source buffer it was parsed from, as a [`ByteSpan`]. The
//! source buffer itself is retained for the lifetime of the loaded
//! document. At save time the rule is:
//!
//! > An entity that is span-backed and structurally unmodified since the
//! > base revision re-emits its **source bytes verbatim** (full rewrite)
//! > or is **omitted entirely** (incremental save — the default mode).
//!
//! This is what makes pdfcer's editing signature-safe: an incremental save
//! appends a new revision and leaves every prior byte untouched, so a
//! digital signature's `/ByteRange` over the earlier revision stays
//! valid (ISO 32000-1 §7.5.6; see `iso32000__s__7.5.6.md` in the spec
//! RAG).
//!
//! ## Why re-encoding is NOT equivalent — concrete spec cases
//!
//! PDF syntax is deliberately non-canonical; a decoded value does not
//! determine its source bytes. Cases the spec RAG documents explicitly:
//!
//! - **Names** (§7.3.5 NOTE 1): `/A#42` and `/AB` are the *same name*.
//!   Re-emitting the decoded value changes bytes.
//! - **Literal strings** (§7.3.4.2): a bare CRLF inside a literal string
//!   decodes to a single 0Ah byte — decode-then-re-encode is lossy by
//!   specification.
//! - **Numerics** (§7.3.3): `4.`, `+17`, `0.40` all carry formatting a
//!   parsed value can't reproduce.
//!
//! Hence every lossy-decoded entity keeps its span, and the span — not
//! the decoded value — is the unit of re-emission for untouched content.
//!
//! ## Offsets are absolute
//!
//! Spans are offsets into **the retained source buffer, from byte 0 of
//! that buffer** — not relative to the `%PDF-` header (which may not be
//! at byte 0; see the header-offset open question in `lib.rs`). One
//! buffer, one coordinate system, no ambiguity.

use std::fmt;
use std::ops::Range;

/// A half-open byte range `[start, start + len)` into a retained source
/// buffer.
///
/// See the module docs for why this type is load-bearing rather than a
/// convenience. It is deliberately a plain value type (`Copy`, ordered,
/// hashable) so it can be embedded in every token and object without
/// ceremony.
///
/// # Examples
///
/// ```
/// use pdfcer_core::span::ByteSpan;
///
/// let buf = b"12 0 obj";
/// let span = ByteSpan::new(0, 2); // the "12" token
/// assert_eq!(span.slice(buf), Some(&b"12"[..]));
/// assert_eq!(span.end(), 2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ByteSpan {
    /// Offset of the first byte, from byte 0 of the retained buffer.
    pub start: usize,
    /// Number of bytes covered. May be 0 (e.g. the empty name `/`
    /// has a 1-byte span for its solidus, but an empty *decoded* value —
    /// zero-length spans arise in synthesized entities).
    pub len: usize,
}

impl ByteSpan {
    /// Construct a span from a start offset and length.
    #[must_use]
    pub const fn new(start: usize, len: usize) -> Self {
        Self { start, len }
    }

    /// Construct a span covering `range` (`start..end`).
    ///
    /// Returns a zero-length span at `range.start` if the range is
    /// inverted (`end < start`) — inverted ranges are always a caller
    /// bug, but this type is used in paths that must not panic
    /// (`pdfcer-core`'s crate-level panic-free policy), so it degrades to
    /// the least-harmful value instead.
    #[must_use]
    pub const fn from_range(range: Range<usize>) -> Self {
        Self {
            start: range.start,
            len: range.end.saturating_sub(range.start),
        }
    }

    /// The offset one past the last byte (`start + len`), saturating.
    #[must_use]
    pub const fn end(self) -> usize {
        self.start.saturating_add(self.len)
    }

    /// This span as a `Range<usize>` for slicing.
    #[must_use]
    pub const fn range(self) -> Range<usize> {
        self.start..self.end()
    }

    /// The bytes this span covers in `buf`, or `None` if the span is out
    /// of bounds for `buf`.
    ///
    /// `None` here always indicates a logic error (a span applied to a
    /// buffer it wasn't produced from); it is surfaced as an `Option`
    /// rather than a panic per the crate's panic-free policy, so callers
    /// convert it into a proper structural error.
    #[must_use]
    pub fn slice(self, buf: &[u8]) -> Option<&[u8]> {
        buf.get(self.range())
    }

    /// Whether this span covers zero bytes.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }
}

impl fmt::Display for ByteSpan {
    /// Renders as `start..end` (the half-open range), the form used in
    /// diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end())
    }
}

impl From<Range<usize>> for ByteSpan {
    fn from(range: Range<usize>) -> Self {
        Self::from_range(range)
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
    fn slice_returns_covered_bytes() {
        let buf = b"hello world";
        assert_eq!(ByteSpan::new(6, 5).slice(buf), Some(&b"world"[..]));
    }

    #[test]
    fn slice_out_of_bounds_is_none_not_panic() {
        let buf = b"short";
        assert_eq!(ByteSpan::new(3, 10).slice(buf), None);
        assert_eq!(ByteSpan::new(99, 1).slice(buf), None);
    }

    #[test]
    fn from_inverted_range_degrades_to_empty() {
        #[allow(clippy::reversed_empty_ranges)]
        let s = ByteSpan::from_range(5..2);
        assert_eq!(s.start, 5);
        assert!(s.is_empty());
    }

    #[test]
    fn display_is_half_open() {
        assert_eq!(ByteSpan::new(3, 4).to_string(), "3..7");
    }
}

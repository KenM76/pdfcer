//! # The object-encoder seam (ISO 32000-1 §7.6, decision 007 R37)
//!
//! A one-trait indirection between "the serializer has produced the
//! bytes of a string or a stream" and "those bytes go into the file".
//! In Pass 3.0 the only implementation is [`IdentityEncoder`], which
//! returns its input untouched. **This module exists entirely so that
//! Pass 5 (encryption) is a plug-in rather than a rewrite.**
//!
//! ## Why a seam now, when nothing uses it
//!
//! Decision 007 W8, verbatim: *"The crypt stage touches EVERY string
//! and EVERY stream, and incremental save of an encrypted document must
//! encrypt newly-appended objects with the existing key. Bolting that
//! onto a completed serializer is a cross-cutting rewrite."* The two
//! call sites that would have to be found and changed later are
//! [`crate::writer::serialize`]'s string arm and its stream arm. Naming
//! them now, as a trait boundary, costs one indirection and removes an
//! entire class of Pass-5 archaeology.
//!
//! ## What the seam deliberately does NOT abstract
//!
//! Three categories of bytes are **never** routed through an encoder,
//! and the type system should not make it look as though they could be:
//!
//! 1. **Verbatim re-emission of a `Provenance::File` object.** Those
//!    bytes are copied from the retained source buffer without ever
//!    being decomposed into strings and streams — in an encrypted
//!    document they are *already* ciphertext, and re-encrypting them
//!    would be catastrophic. The writer's verbatim path does not call
//!    into this module at all, which is the correct structural answer.
//! 2. **Cross-reference streams.** §7.5.8.2: *"The cross-reference
//!    stream shall not be encrypted and strings appearing in the
//!    cross-reference stream dictionary shall not be encrypted. It
//!    shall not have a `Filter` entry that specifies a `Crypt`
//!    filter."* [`crate::writer::xref_out`] therefore serializes with
//!    [`IdentityEncoder`] unconditionally, in every Pass, forever.
//! 3. **The two `/ID` strings when `/Encrypt` is present.** Table 15:
//!    both *"shall be direct and unencrypted"* — they are an input to
//!    key derivation, so encrypting them is circular.
//!
//! Each of those is a `shall`/`shall not` that Pass 5 will otherwise
//! have to rediscover; they are recorded here because here is where a
//! Pass-5 engineer will look.

use std::borrow::Cow;

use crate::object::ObjId;

/// Transforms a serialized string or stream payload on its way into the
/// file — the §7.6 encryption hook, in identity form for Pass 3.0.
///
/// Implementations receive the **containing indirect object's**
/// identifier because §7.6.2's key derivation is per-object: the
/// encryption key for object `N G` mixes the document key with `N` and
/// `G`. A serializer that did not thread the id through would be
/// unable to host a conforming standard security handler at all, which
/// is the whole reason this parameter exists in Pass 3.0 where it is
/// unused.
///
/// # Examples
///
/// ```
/// use pdfcer_core::object::ObjId;
/// use pdfcer_core::writer::{IdentityEncoder, ObjectEncoder};
///
/// let enc = IdentityEncoder;
/// let id = ObjId::new(4, 0);
/// assert_eq!(&*enc.encode_string(id, b"hello"), b"hello");
/// assert_eq!(&*enc.encode_stream(id, b"data"), b"data");
/// ```
pub trait ObjectEncoder {
    /// Transform the **decoded** bytes of a string object (§7.3.4)
    /// belonging to indirect object `owner`.
    ///
    /// Called with the string's value, before any literal/hex escaping:
    /// §7.6.2 encrypts the string's *value*, and the escaping is
    /// syntax applied to the result.
    fn encode_string<'a>(&self, owner: ObjId, data: &'a [u8]) -> Cow<'a, [u8]>;

    /// Transform the **already-filter-encoded** bytes of a stream
    /// (§7.3.8) belonging to indirect object `owner`.
    ///
    /// Called with the bytes that would otherwise be written between
    /// `stream` and `endstream`. §7.6.2's ordering is load-bearing:
    /// encryption is applied *after* the `/Filter` chain on write (and
    /// therefore *before* it on read), so `/Length` must be computed
    /// from this function's **output**, never its input.
    fn encode_stream<'a>(&self, owner: ObjId, data: &'a [u8]) -> Cow<'a, [u8]>;
}

/// The Pass 3.0 encoder: returns every payload unchanged.
///
/// This is not a placeholder to be deleted — it stays as the encoder
/// for unencrypted documents (the overwhelming majority) and as the
/// mandatory encoder for cross-reference streams even in encrypted ones
/// (module docs, item 2).
///
/// Deliberately **not** `#[non_exhaustive]`: a unit struct marked so
/// cannot be constructed outside its defining crate at all, and this one
/// exists precisely to be constructed by `pdfcer`, `pdfcer-render`
/// and the round-trip harness.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct IdentityEncoder;

impl ObjectEncoder for IdentityEncoder {
    /// Borrows through: no allocation, no copy.
    fn encode_string<'a>(&self, _owner: ObjId, data: &'a [u8]) -> Cow<'a, [u8]> {
        Cow::Borrowed(data)
    }

    /// Borrows through: no allocation, no copy.
    fn encode_stream<'a>(&self, _owner: ObjId, data: &'a [u8]) -> Cow<'a, [u8]> {
        Cow::Borrowed(data)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn identity_encoder_borrows_rather_than_copies() {
        // The zero-cost property is part of the contract: an identity
        // save of a 200 MB document must not clone every stream.
        let enc = IdentityEncoder;
        let data = b"payload".to_vec();
        let out = enc.encode_stream(ObjId::new(1, 0), &data);
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(&*out, b"payload");
    }

    #[test]
    fn encoder_is_object_safe_and_usable_behind_a_reference() {
        // Pass 5 will hand the writer a `&dyn ObjectEncoder` chosen at
        // runtime from the document's /Encrypt dictionary; pin that
        // this compiles now rather than discovering it later.
        let boxed: &dyn ObjectEncoder = &IdentityEncoder;
        assert_eq!(&*boxed.encode_string(ObjId::new(9, 1), b"x"), b"x");
    }
}

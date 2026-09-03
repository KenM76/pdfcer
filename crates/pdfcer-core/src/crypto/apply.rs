//! Applying a file key to a loaded document — the never-encrypted list.
//!
//! [`crate::crypto::standard`] answers "what key?". This module answers
//! "**to which bytes?**", and that question is where the correctness surface
//! actually lives: ISO 32000-1's exception list is spread across four clauses,
//! §7.6.1's own bullet list stops at three items, and every omission is a
//! *silent* failure that presents as a corrupt document rather than as a
//! decryption error.
//!
//! # Why decryption happens in the retained buffer
//!
//! [`Stream`](crate::object::Stream) does not own its bytes — it holds a
//! [`ByteSpan`](crate::span::ByteSpan) into the buffer the document retains
//! for its lifetime. That would normally make in-place decryption awkward, and
//! for AES it will be: CBC output is 16 bytes of IV plus padding, so plaintext
//! is *shorter* than ciphertext and cannot be written back over it.
//!
//! **RC4 is a stream cipher and preserves length exactly.** So the decrypted
//! bytes fit precisely where the ciphertext was, and every span, offset,
//! `/Length` and provenance record in the document stays true without a single
//! change to the object model. That is not a lucky accident to lean on
//! silently — it is the reason this increment could be built without touching
//! `Stream`, and the reason the AES increment will have to.
//!
//! Strings are different: [`Object::String`] owns its bytes, so a string is
//! decrypted in the *parsed object* and the buffer keeps the ciphertext. The
//! two halves therefore disagree after load, which is exactly why
//! [`Document::save_full`](crate::document::Document::save_full) and
//! [`save_incremental`](crate::document::Document::save_incremental) **refuse**
//! a decrypted document in this increment (see their docs). Writing encrypted
//! documents is a separate piece of work; half-writing one is a corrupt file.
//!
//! # The never-encrypted list, and why it is not §7.6.1's bullet list
//!
//! §7.6.1 gives three bullets. The full set is **eight**, three of which live
//! in other clauses entirely, and one of which (**E9**) exists only in ISO
//! 32000-2. Applied here:
//!
//! | | Never decrypted | Where enforced |
//! |---|---|---|
//! | **E1** | trailer `/ID` strings | never reaches this module — the trailer is not an object |
//! | **E2** | strings inside `/Encrypt` | [`skip`] — by object number, since `/Encrypt` may be indirect |
//! | **E3** | the `/Encrypt` dictionary itself | same |
//! | **E4** | strings inside an object stream | **by construction** — see below |
//! | **E5** | cross-reference streams | [`skip`] — `/Type /XRef` |
//! | **E6** | external stream data (`/F`) | [`skip`] — no bytes in this file to decrypt |
//! | **E7** | non-string, non-stream types | by construction — the walk only touches strings and streams |
//! | **E8** | `/Metadata` when `/EncryptMetadata false`, **or** `/Crypt`+`/Identity` | [`skip`] — both spellings |
//!
//! **E4 is the one that would destroy every modern file, and it is handled by
//! ordering rather than by a check.** Strings inside an object stream are not
//! separately encrypted: the container's *data* was encrypted once, and the
//! objects inside it were serialized into that plaintext. So the walk runs
//! **after phase 1 and before phase 2** of the document load — file-level
//! objects are decrypted, which makes each object stream's data plaintext, and
//! phase 2 then parses objects out of it that need no decryption at all. A
//! design that decrypted after phase 2 would re-apply Algorithm 1 per contained
//! object and corrupt every string in the document (**T4**).
//!
//! # What a wrong answer looks like
//!
//! None of these fail loudly. Decrypting a cross-reference stream produces
//! garbage that fails to inflate, and the error surfaces as a broken xref two
//! layers away from the cause. Missing `/EncryptMetadata false` produces a
//! `/Metadata` stream of noise that only an XMP reader ever notices. That is
//! why each skip below cites the clause that grants it rather than describing
//! what it does.

use crate::crypto::standard::FileKey;
use crate::object::{Dict, IndirectObject, ObjId, Object};

/// Why an object's bytes are left alone.
///
/// Returned rather than a bare `bool` so the reason can be asserted in tests
/// and reported in diagnostics — "this stream was skipped" is not a useful
/// fact without "because it is a cross-reference stream".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// **E3/E2** — this *is* the `/Encrypt` dictionary. §7.6.1: the encryption
    /// dictionary "shall not" be encrypted, and neither are its strings; it has
    /// to be readable before any key exists.
    EncryptDict,
    /// **E5** — a cross-reference stream. §7.5.8.2 exempts it because the
    /// bootstrap order (xref → trailer → `/Encrypt` → authenticate → decrypt)
    /// means nothing on the path to the key may itself require the key
    /// (**T6**).
    XrefStream,
    /// **E6** — the data is in another file (`/F`), so there are no bytes here.
    /// Note the asymmetry: an *embedded* file stream **is** encrypted.
    ExternalStream,
    /// **E8** — the document metadata stream, left in the clear by
    /// `/EncryptMetadata false`. This changes the *key* as well (Algorithm 2
    /// step (f), **T11**), so the two must agree or nothing decrypts.
    ClearMetadata,
    /// **E8, second spelling** — a `/Crypt` filter naming `/Identity`. Unlike
    /// `/EncryptMetadata`, this does *not* change the file key (**T23**); both
    /// mechanisms exist and a document may use either.
    IdentityCryptFilter,
}

/// Should this object's bytes be left alone, and why?
///
/// `encrypt_dict_id` is the object number of an indirect `/Encrypt` dictionary,
/// if the document has one. The `/Encrypt` dictionary is *usually* direct in
/// the trailer, in which case it never appears as an object at all — but it may
/// be indirect, and then its `/O` and `/U` strings would be decrypted with a
/// key derived from themselves. That produces an authentication that succeeded
/// followed by a document of noise.
#[must_use]
pub fn skip(
    obj: &IndirectObject,
    encrypt_dict_id: Option<u32>,
    encrypt_metadata: bool,
) -> Option<Skip> {
    if Some(obj.id.num) == encrypt_dict_id {
        return Some(Skip::EncryptDict);
    }

    let Object::Stream(stream) = &obj.value else {
        // Non-stream objects can only carry strings, and the only string-level
        // exemptions are the trailer `/ID` (not an object) and `/Encrypt`
        // (handled above).
        return None;
    };
    let dict = &stream.dict;

    if matches!(dict.get(b"Type"), Some(Object::Name(n)) if n.as_bytes() == b"XRef") {
        return Some(Skip::XrefStream);
    }
    if dict.contains_key(b"F") {
        return Some(Skip::ExternalStream);
    }
    if !encrypt_metadata
        && matches!(dict.get(b"Type"), Some(Object::Name(n)) if n.as_bytes() == b"Metadata")
    {
        return Some(Skip::ClearMetadata);
    }
    if has_identity_crypt_filter(dict) {
        return Some(Skip::IdentityCryptFilter);
    }
    None
}

/// Does this stream's `/Filter` chain begin with a `/Crypt` filter naming
/// `/Identity`?
///
/// §7.4.10 requires `/Crypt` to be **first** in the `/Filter` array (**W13**)
/// and makes it a *routing annotation* rather than a decoder (**T22**) — it
/// says which crypt filter applies, and the filter pipeline strips it. The
/// only value pdfcer can act on without a handler-private algorithm is
/// `/Identity`, which means "do not decrypt this stream".
///
/// `/Name` defaults to `/Identity` when absent (Table 14), so a bare `/Crypt`
/// filter with no `/DecodeParms` is the identity case.
fn has_identity_crypt_filter(dict: &Dict) -> bool {
    let is_crypt = |o: &Object| matches!(o, Object::Name(n) if n.as_bytes() == b"Crypt");

    let first_is_crypt = match dict.get(b"Filter") {
        Some(o @ Object::Name(_)) => is_crypt(o),
        Some(Object::Array(items)) => items.first().is_some_and(is_crypt),
        _ => false,
    };
    if !first_is_crypt {
        return false;
    }

    // The parameter dictionary sits at the matching position in
    // /DecodeParms — index 0, since /Crypt shall be first.
    let parms = match dict.get(b"DecodeParms") {
        Some(Object::Dict(d)) => Some(d),
        Some(Object::Array(items)) => match items.first() {
            Some(Object::Dict(d)) => Some(d),
            _ => None,
        },
        _ => None,
    };
    match parms.and_then(|d| d.get(b"Name")) {
        Some(Object::Name(n)) => n.as_bytes() == b"Identity",
        // Table 14: /Name defaults to /Identity.
        _ => true,
    }
}

/// Decrypt every string in `value`, keyed on the containing object `id`.
///
/// **T3** — a string is keyed on the *containing indirect object's* identity
/// at **any nesting depth**. A string four dictionaries deep inside object
/// `12 0` uses object `12 0`'s key, not anything nearer. The recursion
/// therefore threads one `id` all the way down and never recomputes it.
///
/// References are not followed: an [`Object::Reference`] is a pointer, and the
/// object it names is decrypted on its own turn with its own key.
pub fn decrypt_strings(value: &mut Object, id: ObjId, key: &FileKey) {
    match value {
        Object::String(bytes) => {
            *bytes = key.decrypt_string(id, bytes);
        }
        Object::Array(items) => {
            for item in items {
                decrypt_strings(item, id, key);
            }
        }
        Object::Dict(dict) => {
            for (_, v) in &mut dict.0 {
                decrypt_strings(v, id, key);
            }
        }
        Object::Stream(stream) => {
            // A stream's *dictionary* carries ordinary encrypted strings —
            // §7.6.5's own EXAMPLE confirms it for `/Info`-style entries. The
            // stream *data* is handled separately, in the buffer.
            for (_, v) in &mut stream.dict.0 {
                decrypt_strings(v, id, key);
            }
        }
        // E7: every other type is unencrypted by nature.
        Object::Null
        | Object::Boolean(_)
        | Object::Integer(_)
        | Object::Real(_)
        | Object::Name(_)
        | Object::Reference(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Name, Provenance, Stream};
    use crate::span::ByteSpan;

    fn name(s: &str) -> Object {
        Object::Name(Name(s.as_bytes().to_vec()))
    }

    fn stream_obj(num: u32, entries: Vec<(&str, Object)>) -> IndirectObject {
        let mut d = Dict::new();
        for (k, v) in entries {
            d.insert(Name(k.as_bytes().to_vec()), v);
        }
        IndirectObject {
            id: ObjId::new(num, 0),
            value: Object::Stream(Stream {
                dict: d,
                data_span: ByteSpan::new(0, 0),
            }),
            provenance: Provenance::File(ByteSpan::new(0, 0)),
        }
    }

    /// E5 — a cross-reference stream is never encrypted. Decrypting one
    /// produces bytes that fail to inflate, and the error surfaces as a
    /// broken cross-reference table rather than as an encryption problem.
    #[test]
    fn xref_streams_are_skipped() {
        let o = stream_obj(4, vec![("Type", name("XRef"))]);
        assert_eq!(skip(&o, None, true), Some(Skip::XrefStream));
    }

    /// E6 — `/F` means the data lives in another file. There is nothing here
    /// to decrypt. The asymmetry is worth stating: an *embedded* file stream
    /// (`/EF`) has its data in this file and **is** encrypted.
    #[test]
    fn external_streams_are_skipped() {
        let o = stream_obj(5, vec![("F", Object::String(b"other.dat".to_vec()))]);
        assert_eq!(skip(&o, None, true), Some(Skip::ExternalStream));
    }

    /// E8 — `/Metadata` is skipped only when `/EncryptMetadata` is false.
    /// With the default `true` it is an ordinary encrypted stream, and
    /// skipping it would leave XMP as ciphertext.
    #[test]
    fn metadata_is_skipped_only_when_declared_clear() {
        let o = stream_obj(6, vec![("Type", name("Metadata"))]);
        assert_eq!(skip(&o, None, false), Some(Skip::ClearMetadata));
        assert_eq!(skip(&o, None, true), None);
    }

    /// E2/E3 — an indirect `/Encrypt` dictionary is matched by object number.
    /// Its `/O` and `/U` are the inputs to the key derivation; decrypting them
    /// with a key derived from themselves gives a successful authentication
    /// followed by a document of noise.
    #[test]
    fn indirect_encrypt_dict_is_skipped_by_number() {
        let mut d = Dict::new();
        d.insert(Name(b"O".to_vec()), Object::String(vec![0; 32]));
        let o = IndirectObject {
            id: ObjId::new(9, 0),
            value: Object::Dict(d),
            provenance: Provenance::File(ByteSpan::new(0, 0)),
        };
        assert_eq!(skip(&o, Some(9), true), Some(Skip::EncryptDict));
        // A different object number is not exempt.
        assert_eq!(skip(&o, Some(10), true), None);
    }

    /// T23 — `/Crypt` + `/Identity` is the *other* way to leave a stream in
    /// the clear, and it does not change the file key the way
    /// `/EncryptMetadata false` does. Both spellings exist; implementing only
    /// one leaves a class of files broken.
    #[test]
    fn identity_crypt_filter_is_skipped() {
        // Bare /Crypt with no parameters: /Name defaults to /Identity.
        let bare = stream_obj(7, vec![("Filter", name("Crypt"))]);
        assert_eq!(skip(&bare, None, true), Some(Skip::IdentityCryptFilter));

        // Explicit /Name /Identity in an array-form filter chain.
        let mut parms = Dict::new();
        parms.insert(Name(b"Name".to_vec()), name("Identity"));
        let explicit = stream_obj(
            8,
            vec![
                (
                    "Filter",
                    Object::Array(vec![name("Crypt"), name("FlateDecode")]),
                ),
                (
                    "DecodeParms",
                    Object::Array(vec![Object::Dict(parms), Object::Null]),
                ),
            ],
        );
        assert_eq!(skip(&explicit, None, true), Some(Skip::IdentityCryptFilter));
    }

    /// A `/Crypt` filter naming a *real* crypt filter is not the identity
    /// case and must still be decrypted — treating every `/Crypt` as
    /// "skip" would leave real content as ciphertext.
    #[test]
    fn named_crypt_filter_is_not_identity() {
        let mut parms = Dict::new();
        parms.insert(Name(b"Name".to_vec()), name("StdCF"));
        let o = stream_obj(
            11,
            vec![
                ("Filter", Object::Array(vec![name("Crypt")])),
                ("DecodeParms", Object::Array(vec![Object::Dict(parms)])),
            ],
        );
        assert_eq!(skip(&o, None, true), None);
    }

    /// W13 — `/Crypt` "shall be first". A `/Crypt` appearing later in the
    /// chain is malformed, and reading it as an identity exemption would let
    /// a malformed file suppress decryption of real content.
    #[test]
    fn crypt_filter_not_first_is_ignored() {
        let o = stream_obj(
            12,
            vec![(
                "Filter",
                Object::Array(vec![name("FlateDecode"), name("Crypt")]),
            )],
        );
        assert_eq!(skip(&o, None, true), None);
    }

    /// An ordinary content stream is decrypted — the negative case, without
    /// which every assertion above would also pass on a function that
    /// returned `Some` unconditionally.
    #[test]
    fn ordinary_streams_are_not_skipped() {
        let o = stream_obj(13, vec![("Length", Object::Integer(42))]);
        assert_eq!(skip(&o, None, true), None);
    }
}

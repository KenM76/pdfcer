//! # Object streams (ISO 32000-1 §7.5.7) — compressed indirect objects
//!
//! A PDF 1.5 **object stream** is an ordinary stream object whose
//! decoded data holds a sequence of *other* indirect objects, so that
//! non-stream objects can be compressed by a stream filter. A type-2
//! cross-reference entry (§7.5.8.3 Table 18; [`XrefEntry::InStream`])
//! names the container and the index within it; this module turns that
//! pair into a parsed [`Object`].
//!
//! Spec source: `iso32000__s__7.5.7.md` (Table 16, the decoded layout,
//! the `Extends` collection model) in the PDF-spec RAG, plus
//! `iso32000__s__7.3.10.md` for the generation rule. Clause numbers are
//! ISO 32000-1:2008.
//!
//! [`XrefEntry::InStream`]: crate::xref::XrefEntry::InStream
//!
//! ## The decoded layout
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ N pairs of integers: objnum offset …         │  ← the "pair table"
//! │                                              │
//! ├──────────────────────────────────────────────┤ ← byte `First`
//! │ object 0's value                             │
//! │ object 1's value                             │
//! │ …                                            │
//! └──────────────────────────────────────────────┘
//! ```
//!
//! Three rules from §7.5.7 that a naïve implementation gets wrong:
//!
//! 1. **Each pair's offset is relative to `First`**, not to the start
//!    of the decoded data. Absolute position = `First + offset`.
//! 2. **Offsets are increasing, object numbers are NOT.** NOTE 6:
//!    "there is no restriction on the order of objects in the object
//!    stream; in particular, the objects need not be stored in
//!    object-number order." So the pair table must never be
//!    binary-searched by object number — index into it positionally,
//!    which is exactly what the type-2 entry's field 3 gives.
//! 3. **No `obj`/`endobj` framing.** "Only the object values are
//!    stored in the stream; the `obj` and `endobj` keywords shall not
//!    be used." Each object is parsed as a bare direct value, and its
//!    identity comes from the pair table, not from the bytes.
//!
//! A conforming writer places the first object immediately after the
//! pair table, but "a conforming reader **shall rely on the `First`
//! entry**" — so `First` is authoritative here and the pair table's
//! extent is never used to locate object data.
//!
//! ## What cannot be in an object stream (and why that bounds us)
//!
//! §7.5.7 forbids: stream objects, objects with a non-zero generation,
//! the encryption dictionary, the object that is the value of an object
//! stream's own `/Length`, and (in linearized files) the catalog, the
//! linearization dictionary and page objects.
//!
//! Two of those are load-bearing for this module's guarantees:
//!
//! - **No streams inside** means a parsed compressed object can never
//!   carry a [`ByteSpan`](crate::span::ByteSpan) — [`Object`] holds
//!   spans only inside [`Object::Stream`]. So the decoded buffer can be
//!   dropped after parsing without leaving dangling coordinates, and
//!   the object's provenance is honestly
//!   [`Provenance::ObjectStream`](crate::object::Provenance::ObjectStream)
//!   with no file span at all.
//! - **The `/Length` object cannot itself be compressed**, which bounds
//!   the resolution recursion: fetching a container's `/Length` can
//!   never require fetching another container.
//!
//! ## `Extends` is not followed at read time
//!
//! Table 16's `Extends` links object streams into a collection ("a
//! directed acyclic graph") so an update can add objects to a
//! collection without rewriting the original stream. It is
//! **informational for a reader**: a type-2 entry names the specific
//! container and index directly, so resolution never walks the chain.
//! pdfcer therefore does not follow it — which also means the "damaged
//! file cycles through `Extends`" hazard cannot arise here at all,
//! rather than being defended against with a depth counter. A future
//! *writer* doing collection updates is where `Extends` becomes live,
//! and that is where its cycle guard belongs.
//!
//! ## Guards (ARCHITECTURE.md §10 — pdfcer policy, not spec)
//!
//! - [`MAX_OBJSTM_OBJECTS`] bounds `/N` before any allocation, so a
//!   hostile `/N 4000000000` cannot make the pair-table `Vec` reserve
//!   gigabytes.
//! - The pair table is read strictly **within** the first `First`
//!   bytes; a pair claiming to live past `First` is malformed rather
//!   than an excuse to read into object data.
//! - Filter decoding runs through `crate::filters`, which enforces
//!   `MAX_DECODED_LEN` incrementally (decompression-bomb ceiling).
//!
//! ## Failure posture
//!
//! Every failure is a structured [`ObjStmError`], never a panic and
//! never a partially-decoded object presented as complete — the same
//! fail-clean contract the filter and parser layers hold.

use crate::filters::{self, FilterError};
use crate::lexer::{Lexer, TokenKind};
use crate::object::{Dict, Name, Object};
use crate::parser::{ParseError, Parser};

/// Maximum number of objects (`/N`) accepted in a single object stream.
///
/// pdfcer policy (ARCHITECTURE.md §10.1): ISO 32000-1 sets no limit —
/// NOTE 4 merely *advises* writers to keep object streams small so a
/// reader need not decompress a large stream to reach one object. One
/// million objects in a single container is orders of magnitude beyond
/// any real producer while bounding the pair-table allocation.
pub const MAX_OBJSTM_OBJECTS: usize = 1_000_000;

/// Something went wrong reading an object stream (§7.5.7).
///
/// C-GOOD-ERR via `thiserror`; every variant is `Send + Sync +
/// 'static`. Variants name the specific spec rule violated so a
/// diagnostic can point at the producer's actual mistake.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum ObjStmError {
    /// The object a type-2 entry named as its container is not a stream
    /// object at all (§7.5.7: an object stream is a stream).
    #[error("the container object is not a stream")]
    NotAStream,
    /// The container's `/Type` is present but is not `/ObjStm`
    /// (Table 16, Required).
    #[error("container /Type is present but is not /ObjStm")]
    WrongType,
    /// `/N` (Table 16, Required) is missing or not a direct
    /// non-negative integer.
    ///
    /// Table 16 does not mark `/N` direct-only, but resolving an
    /// indirect `/N` would require a document-level resolve *inside*
    /// the loader that is building the document — pdfcer refuses rather
    /// than reordering the load for a case no real producer emits.
    #[error("/N missing or not a direct non-negative integer")]
    BadObjectCount,
    /// `/First` (Table 16, Required) is missing, not a direct
    /// non-negative integer, or points past the end of the decoded
    /// data.
    #[error("/First missing, invalid, or past the end of the decoded data")]
    BadFirst,
    /// `/N` exceeded [`MAX_OBJSTM_OBJECTS`] (pdfcer guard).
    #[error("object count {0} exceeds MAX_OBJSTM_OBJECTS ({MAX_OBJSTM_OBJECTS})")]
    TooManyObjects(usize),
    /// The `N` `objnum offset` integer pairs could not be read from the
    /// region before `/First` — too few numbers, a non-integer token, a
    /// negative value, or a pair extending past `/First`.
    #[error("malformed object-stream pair table")]
    BadPairTable,
    /// The container's raw data span did not lie inside the retained
    /// buffer (a loader-side coordinate bug, surfaced rather than
    /// panicked).
    #[error("container stream data lies outside the source buffer")]
    DataOutOfRange,
    /// A type-2 entry's index is past the last pair in the table.
    #[error("index {index} is out of range for an object stream holding {count} object(s)")]
    IndexOutOfRange {
        /// The requested 0-based index.
        index: usize,
        /// How many objects the pair table actually describes.
        count: usize,
    },
    /// `First + offset` for the requested object falls outside the
    /// decoded data.
    #[error("object offset is outside the decoded object-stream data")]
    OffsetOutOfRange,
    /// The container's `/Filter` chain failed to decode.
    #[error("object-stream data could not be decoded: {0}")]
    Decode(#[from] FilterError),
    /// The bytes at the object's position are not a well-formed COS
    /// object.
    #[error("compressed object does not parse: {0}")]
    Parse(#[from] ParseError),
    /// The stored object consists solely of an indirect reference —
    /// §7.5.7 states an object in an object stream "shall not consist
    /// solely of an object reference" (its EXAMPLE 2 of the forbidden
    /// form is `3 0 R`). The prohibition exists to prevent an aliasing
    /// ambiguity; the spec does not define reader behaviour, so pdfcer
    /// treats it as malformed.
    #[error("compressed object consists solely of an indirect reference")]
    SoleReference,
}

/// A decoded object stream, ready for positional object lookup.
///
/// Constructed once per container by [`ObjectStream::parse`] and cached
/// by the document loader — decoding is the expensive part and a
/// container typically holds dozens to hundreds of objects, each of
/// which would otherwise re-inflate the whole thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectStream {
    /// The fully decoded stream data (pair table + object values).
    data: Vec<u8>,
    /// `/First`: byte offset in `data` of the first object value.
    first: usize,
    /// The pair table, in stored order: `(object number, offset
    /// relative to `first`)`. Index into this positionally — see the
    /// module docs on why object-number search is wrong.
    pairs: Vec<(u32, usize)>,
}

impl ObjectStream {
    /// Decode `raw` through `dict`'s filter chain and read its Table 16
    /// header and pair table.
    ///
    /// `dict` is the container's stream dictionary and `raw` its
    /// still-encoded `data_span` bytes.
    ///
    /// # Errors
    ///
    /// [`ObjStmError`] — see the variants; each names the §7.5.7 rule
    /// that was violated.
    pub fn parse(dict: &Dict, raw: &[u8]) -> Result<Self, ObjStmError> {
        // Table 16: `/Type` shall be `/ObjStm`. As with cross-reference
        // streams, a *wrong* type is refused (the xref pointed at
        // something else entirely) while an *absent* type is tolerated:
        // `/N` and `/First` are what actually drive decoding.
        if let Some(ty) = dict.get(b"Type")
            && ty.as_name().map(Name::as_bytes) != Some(b"ObjStm")
        {
            return Err(ObjStmError::WrongType);
        }

        let count = dict
            .get(b"N")
            .and_then(Object::as_int)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or(ObjStmError::BadObjectCount)?;
        if count > MAX_OBJSTM_OBJECTS {
            return Err(ObjStmError::TooManyObjects(count));
        }
        let first = dict
            .get(b"First")
            .and_then(Object::as_int)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or(ObjStmError::BadFirst)?;

        let data = filters::decode_stream(dict, raw)?;
        if first > data.len() {
            return Err(ObjStmError::BadFirst);
        }

        let pairs = read_pair_table(&data, first, count)?;
        Ok(Self { data, first, pairs })
    }

    /// How many objects the pair table describes (`/N`, as actually
    /// read).
    #[must_use]
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// The object numbers this container holds, in stored (index) order.
    ///
    /// Cheap: it reads the already-parsed pair table without decoding any
    /// object *value* (unlike [`ObjectStream::object_at`]). It exists for
    /// the cross-reference **recovery** path (`crate::recover`), which
    /// rebuilds the type-2 (`InStream`) entries a lost xref would
    /// otherwise have named — recovery needs only *which numbers* live in
    /// *which container at what index*, exactly the type-2 entry's fields
    /// (§7.5.8.3 Table 18), and the pair table is the authority for that
    /// (module docs). The index of each yielded number is its position in
    /// the iterator, i.e. the type-2 entry's field 3.
    pub fn member_numbers(&self) -> impl Iterator<Item = u32> + '_ {
        self.pairs.iter().map(|&(num, _)| num)
    }

    /// Whether the container holds no objects.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// Parse the object stored at 0-based `index`, returning its
    /// declared object number (from the pair table) and its value.
    ///
    /// The object number is returned rather than assumed so the caller
    /// can cross-check it against the object number the type-2
    /// cross-reference entry promised — a mismatch means the xref and
    /// the container disagree, which the strict loader refuses instead
    /// of silently trusting one of them.
    ///
    /// # Errors
    ///
    /// [`ObjStmError::IndexOutOfRange`], [`ObjStmError::OffsetOutOfRange`],
    /// [`ObjStmError::Parse`], or [`ObjStmError::SoleReference`].
    pub fn object_at(&self, index: usize) -> Result<(u32, Object), ObjStmError> {
        let &(num, rel) = self.pairs.get(index).ok_or(ObjStmError::IndexOutOfRange {
            index,
            count: self.pairs.len(),
        })?;
        // Absolute position = First + offset (module docs rule 1).
        let start = self
            .first
            .checked_add(rel)
            .ok_or(ObjStmError::OffsetOutOfRange)?;
        if start > self.data.len() {
            return Err(ObjStmError::OffsetOutOfRange);
        }
        // No `obj`/`endobj` framing (module docs rule 3): a bare value.
        let value = Parser::at(&self.data, start).parse_object()?;
        if matches!(value, Object::Reference(_)) {
            return Err(ObjStmError::SoleReference);
        }
        Ok((num, value))
    }
}

/// Read `count` `objnum offset` integer pairs from the region of `data`
/// before `first`.
///
/// Tokenizing (rather than hand-scanning digits) is deliberate: §7.5.7
/// specifies only "N pairs of integers separated by white space", and
/// the lexer already implements §7.2's white-space and comment rules.
/// Every consumed token is bounds-checked against `first` so the pair
/// table can never run into object data.
fn read_pair_table(
    data: &[u8],
    first: usize,
    count: usize,
) -> Result<Vec<(u32, usize)>, ObjStmError> {
    let mut lexer = Lexer::at(data, 0);
    let mut pairs = Vec::with_capacity(count);
    for _ in 0..count {
        let num = next_pair_int(&mut lexer, first)?;
        let offset = next_pair_int(&mut lexer, first)?;
        let num = u32::try_from(num).map_err(|_| ObjStmError::BadPairTable)?;
        let offset = usize::try_from(offset).map_err(|_| ObjStmError::BadPairTable)?;
        pairs.push((num, offset));
    }
    Ok(pairs)
}

/// Read one non-negative integer token from the pair table, refusing
/// anything that ends at or past `first` (i.e. that would be reading
/// object data, not header).
fn next_pair_int(lexer: &mut Lexer<'_>, first: usize) -> Result<i64, ObjStmError> {
    let token = lexer
        .next_token()
        .map_err(|_| ObjStmError::BadPairTable)?
        .ok_or(ObjStmError::BadPairTable)?;
    if token.span.end() > first {
        return Err(ObjStmError::BadPairTable);
    }
    match token.kind {
        TokenKind::Integer(v) if v >= 0 => Ok(v),
        _ => Err(ObjStmError::BadPairTable),
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
    use crate::object::ObjId;

    /// Build an object-stream dictionary + uncompressed data for the
    /// given `(objnum, value-text)` list, laid out exactly per §7.5.7.
    fn build(objects: &[(u32, &str)]) -> (Dict, Vec<u8>) {
        let mut body = String::new();
        let mut header = String::new();
        for (num, text) in objects {
            header.push_str(&format!("{num} {} ", body.len()));
            body.push_str(text);
            body.push(' ');
        }
        let first = header.len();
        let data = format!("{header}{body}").into_bytes();

        let mut dict = Dict::new();
        dict.insert(Name::from(b"Type"), Object::Name(Name::from(b"ObjStm")));
        dict.insert(
            Name::from(b"N"),
            Object::Integer(i64::try_from(objects.len()).unwrap()),
        );
        dict.insert(
            Name::from(b"First"),
            Object::Integer(i64::try_from(first).unwrap()),
        );
        dict.insert(
            Name::from(b"Length"),
            Object::Integer(i64::try_from(data.len()).unwrap()),
        );
        (dict, data)
    }

    #[test]
    fn reads_pair_table_and_objects_in_stored_order() {
        // NOTE 6: object numbers need NOT ascend — offsets must.
        let (dict, data) = build(&[
            (7, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
            (9, "42"),
        ]);
        let objstm = ObjectStream::parse(&dict, &data).unwrap();
        assert_eq!(objstm.len(), 3);

        let (num, value) = objstm.object_at(0).unwrap();
        assert_eq!(num, 7);
        assert_eq!(
            value
                .as_dict()
                .unwrap()
                .get(b"Pages")
                .unwrap()
                .as_reference(),
            Some(ObjId::new(2, 0))
        );

        let (num, value) = objstm.object_at(2).unwrap();
        assert_eq!(num, 9);
        assert_eq!(value.as_int(), Some(42));
    }

    #[test]
    fn index_past_the_end_is_an_error_not_a_panic() {
        let (dict, data) = build(&[(1, "null")]);
        let objstm = ObjectStream::parse(&dict, &data).unwrap();
        assert_eq!(
            objstm.object_at(5).unwrap_err(),
            ObjStmError::IndexOutOfRange { index: 5, count: 1 }
        );
    }

    #[test]
    fn sole_reference_is_refused() {
        // §7.5.7: "An object in an object stream shall not consist
        // solely of an object reference." Spec EXAMPLE 2: `3 0 R`.
        let (dict, data) = build(&[(1, "3 0 R")]);
        let objstm = ObjectStream::parse(&dict, &data).unwrap();
        assert_eq!(objstm.object_at(0).unwrap_err(), ObjStmError::SoleReference);
    }

    #[test]
    fn wrong_type_is_refused_missing_type_is_tolerated() {
        let (mut dict, data) = build(&[(1, "null")]);
        dict.insert(Name::from(b"Type"), Object::Name(Name::from(b"XRef")));
        assert_eq!(
            ObjectStream::parse(&dict, &data).unwrap_err(),
            ObjStmError::WrongType
        );

        let (mut dict, data) = build(&[(1, "null")]);
        dict.0.retain(|(k, _)| k.as_bytes() != b"Type");
        assert!(ObjectStream::parse(&dict, &data).is_ok());
    }

    #[test]
    fn first_past_end_of_data_is_refused() {
        let (mut dict, data) = build(&[(1, "null")]);
        dict.insert(Name::from(b"First"), Object::Integer(9999));
        assert_eq!(
            ObjectStream::parse(&dict, &data).unwrap_err(),
            ObjStmError::BadFirst
        );
    }

    #[test]
    fn pair_table_running_past_first_is_refused() {
        // `/N` claims more pairs than fit before `/First`: the extra
        // reads would consume object data, so the table is malformed.
        let (mut dict, data) = build(&[(1, "null")]);
        dict.insert(Name::from(b"N"), Object::Integer(4));
        assert_eq!(
            ObjectStream::parse(&dict, &data).unwrap_err(),
            ObjStmError::BadPairTable
        );
    }

    #[test]
    fn hostile_object_count_is_bounded_before_allocation() {
        let (mut dict, data) = build(&[(1, "null")]);
        let hostile = i64::try_from(MAX_OBJSTM_OBJECTS).unwrap() + 1;
        dict.insert(Name::from(b"N"), Object::Integer(hostile));
        assert!(matches!(
            ObjectStream::parse(&dict, &data).unwrap_err(),
            ObjStmError::TooManyObjects(_)
        ));
    }

    #[test]
    fn flate_encoded_container_decodes() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write as _;

        let (mut dict, data) = build(&[(4, "(hello)")]);
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&data).unwrap();
        let compressed = enc.finish().unwrap();
        dict.insert(
            Name::from(b"Filter"),
            Object::Name(Name::from(b"FlateDecode")),
        );

        let objstm = ObjectStream::parse(&dict, &compressed).unwrap();
        let (num, value) = objstm.object_at(0).unwrap();
        assert_eq!(num, 4);
        assert_eq!(value, Object::String(b"hello".to_vec()));
    }
}

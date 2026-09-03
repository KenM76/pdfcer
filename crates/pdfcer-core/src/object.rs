//! # COS object model (ISO 32000-1 §7.3)
//!
//! The value types of the Carousel Object System — the tree every PDF
//! is made of. Spec sources: `iso32000__s__7.3.md` (the eight basic
//! types, dictionary semantics, null), `iso32000__s__7.3.10.md`
//! (indirect objects and references), `iso32000__s__7.3.8.md` (streams)
//! in the PDF-spec RAG. Clause numbers are ISO 32000-1:2008.
//!
//! ## ONE object model (named invariant)
//!
//! This is the **only** object representation in pdfcer-core: the parse
//! result and the write source are the same types
//! (docs/decisions/001-oxidize-pdf-adopt-vs-build.md §6.1 item 3,
//! recorded in ARCHITECTURE.md §12). The audited failure mode this
//! prevents: a read-only parser model plus a separate builder-only
//! generation model whose bridge never gets built, making round-trip
//! structurally impossible. Any future "builder" convenience API must
//! construct *these* types.
//!
//! ## Value semantics vs provenance
//!
//! `Object` is a plain value tree — no spans inside it. Provenance
//! lives one level up: an [`IndirectObject`] records *where its
//! definition physically lives* as a [`Provenance`], and re-emission of
//! logically-untouched objects works from that (`crate::span`;
//! ARCHITECTURE.md §5). Within an object the operator *did* modify,
//! re-serialization from values is correct — §5 demands byte-identity
//! only for what was NOT touched. (Content streams get a finer-grained
//! span-per-token model in their own module, per decision 001 §6.1
//! item 2 — that is deliberately NOT this tree.)
//!
//! Provenance is an **enum, not a span**, because PDF 1.5's object
//! streams (§7.5.7) create objects that have no byte range of their own
//! in the file — see [`Provenance`] for the full reasoning.
//!
//! ## Normative behaviors encoded here
//!
//! - **`null`-valued dictionary entry ≡ absent entry** (§7.3.7/§7.3.9):
//!   collapsed in [`Dict::get`] — one accessor, not a check at every
//!   call site.
//! - **Duplicate keys are malformed** (§7.3.7 "shall not") — the parser
//!   rejects them at parse time (fail-clean), so `Dict` never holds
//!   duplicates from a parsed file.
//! - **Integer vs real is preserved** (§7.3.3): two variants, never
//!   collapsed to f64 — `4` and `4.0` are different tokens with
//!   different re-serializations.
//! - **Generation numbers are part of object identity** (§7.3.10):
//!   [`ObjId`] carries both.

use std::fmt;

use crate::span::ByteSpan;

/// An indirect-object identifier: object number + generation number
/// (§7.3.10 — "together … shall uniquely identify an indirect object").
///
/// Object numbers are positive (0 is reserved for the free-list head,
/// §7.5.4); generations run 0–65,535 (65,535 marks a never-reusable
/// entry). `u32`/`u16` cover the spec ranges exactly (Annex C caps
/// object numbers well inside `u32`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjId {
    /// Object number (positive; 0 never identifies a real object).
    pub num: u32,
    /// Generation number (0–65,535).
    pub generation: u16,
}

impl ObjId {
    /// Construct an identifier.
    #[must_use]
    pub const fn new(num: u32, generation: u16) -> Self {
        Self { num, generation }
    }
}

impl fmt::Display for ObjId {
    /// Renders as `num gen` — the order used by both `obj` definitions
    /// and `R` references.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.num, self.generation)
    }
}

/// A PDF name (§7.3.5): the decoded byte sequence, `#`-escapes already
/// expanded, without the introducing `/`.
///
/// Stored decoded so `/Type` and `/Ty#70e` (same name, §7.3.5 NOTE 1)
/// compare and hash identically. Names are raw bytes, not guaranteed
/// UTF-8 (§7.3.5: interpretation as UTF-8 applies only where a name is
/// *used as text*).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(pub Vec<u8>);

impl Name {
    /// The decoded name bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Name {
    /// Debug form is `/Name` with non-ASCII bytes hex-escaped —
    /// mirrors how the name would be discussed, not its exact source
    /// encoding (which is non-unique anyway).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "/")?;
        for &b in &self.0 {
            if b.is_ascii_graphic() && b != b'#' {
                write!(f, "{}", b as char)?;
            } else {
                write!(f, "#{b:02X}")?;
            }
        }
        Ok(())
    }
}

impl From<&[u8]> for Name {
    fn from(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }
}

impl<const N: usize> From<&[u8; N]> for Name {
    fn from(bytes: &[u8; N]) -> Self {
        Self(bytes.to_vec())
    }
}

/// A dictionary (§7.3.7): name-keyed, unordered per spec — but pdfcer
/// preserves the parsed entry order so that re-serializing a *modified*
/// dictionary perturbs sibling entries as little as possible (smaller
/// diffs; the spec says written order "shall be ignored", so preserving
/// it is always safe).
///
/// Backed by an ordered `Vec` with linear-scan lookup: PDF dictionaries
/// are small (a handful to a few dozen entries), where a `Vec` beats a
/// hash map on both speed and memory, and it keeps deterministic order
/// for free.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dict(pub Vec<(Name, Object)>);

impl Dict {
    /// Empty dictionary.
    #[must_use]
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// Look up `key`, applying the §7.3.7/§7.3.9 rule: **an entry whose
    /// value is `null` is treated as if absent** — this returns `None`
    /// for it. Implemented here, once, so no call site ever needs a
    /// second "is it null?" check (see the spec RAG's API note).
    #[must_use]
    pub fn get(&self, key: &[u8]) -> Option<&Object> {
        self.0
            .iter()
            .find(|(k, _)| k.as_bytes() == key)
            .map(|(_, v)| v)
            .filter(|v| !matches!(v, Object::Null))
    }

    /// Whether `key` is present with a non-null value (same collapse
    /// rule as [`Dict::get`]).
    #[must_use]
    pub fn contains_key(&self, key: &[u8]) -> bool {
        self.get(key).is_some()
    }

    /// Number of stored entries (including any explicit-null entries —
    /// this is the physical count, used by serialization; semantic
    /// presence goes through [`Dict::get`]).
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the dictionary stores no entries at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterate entries in preserved (parsed/inserted) order.
    pub fn iter(&self) -> impl Iterator<Item = (&Name, &Object)> {
        self.0.iter().map(|(k, v)| (k, v))
    }

    /// Insert or replace `key` → `value`, preserving position on
    /// replace (append on fresh insert). The write-side primitive the
    /// editing Passes build on.
    pub fn insert(&mut self, key: Name, value: Object) {
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key, value)),
        }
    }

    /// Remove `key` entirely, returning its stored value if there was
    /// one.
    ///
    /// ## Why removal, and not "set it to `null`"
    ///
    /// §7.3.7 makes a `null`-valued entry semantically identical to an
    /// absent one, so `insert(key, Object::Null)` would be *correct*.
    /// It would also leave a physical entry behind, which means a
    /// re-serialized dictionary carries a visible `/Title null` the
    /// operator never asked for, and a later reader cannot tell "pdfcer
    /// cleared this" from "the producer wrote an explicit null". Under
    /// the minimal-diff discipline the honest form of *clear this field*
    /// is for the field not to be there.
    ///
    /// Removes **all** entries with this key, not just the first.
    /// Duplicate keys are malformed per §7.3.7 ("shall not") and the
    /// parser rejects them, so a parsed dictionary never has any — but a
    /// `Dict` built programmatically could, and leaving a shadowed
    /// duplicate behind would make removal look like it did nothing.
    pub fn remove(&mut self, key: &[u8]) -> Option<Object> {
        let mut removed = None;
        self.0.retain(|(k, v)| {
            if k.as_bytes() == key {
                if removed.is_none() {
                    removed = Some(v.clone());
                }
                false
            } else {
                true
            }
        });
        removed
    }
}

/// A stream object (§7.3.8): its dictionary plus the span of its
/// **encoded** data bytes in the retained source buffer.
///
/// The data is *not* copied out at parse time — the span points into
/// the retained buffer (`crate::span`'s model), and decoding through
/// the `/Filter` chain is a separate, on-demand step with its own
/// resource ceilings (ARCHITECTURE.md §10.1). `data_span` is the exact
/// `/Length`-byte region beginning after the `stream` keyword's EOL
/// (§7.3.8.1's framing rules, including the CR-alone prohibition).
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    /// The stream dictionary (`/Length`, `/Filter`, …). Always a
    /// direct dictionary per §7.3.8.1 (its *entries* may be indirect).
    pub dict: Dict,
    /// Exact span of the raw (still-encoded) stream data in the source
    /// buffer.
    pub data_span: ByteSpan,
}

/// A COS object (§7.3.1's eight basic types, plus the indirect
/// reference, which is a value position in its own right per §7.3.10's
/// substitutability rule).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Object {
    /// `null` (§7.3.9). Also the resolution of any dangling reference
    /// (§7.3.10 — "shall not be considered an error").
    Null,
    /// `true` / `false` (§7.3.2).
    Boolean(bool),
    /// Integer (§7.3.3). Kept distinct from `Real` — see module docs.
    Integer(i64),
    /// Real (§7.3.3).
    Real(f64),
    /// String (§7.3.4), decoded to raw bytes (escapes/hex applied).
    /// Interpretation as text (§7.9.2) is a later layer's concern.
    String(Vec<u8>),
    /// Name (§7.3.5), decoded.
    Name(Name),
    /// Array (§7.3.6) — heterogeneous, possibly empty.
    Array(Vec<Object>),
    /// Dictionary (§7.3.7).
    Dict(Dict),
    /// Stream (§7.3.8). Streams only occur as the body of an indirect
    /// object (§7.3.8.1 "all streams shall be indirect objects"); the
    /// parser enforces that.
    Stream(Stream),
    /// An indirect reference `N G R` (§7.3.10) — a *pointer*, resolved
    /// on demand by the document layer (dangling → `Null`, generation
    /// mismatch → `Null`, cycle-guarded).
    Reference(ObjId),
}

impl Object {
    /// This object as an integer, if it is one.
    #[must_use]
    pub const fn as_int(&self) -> Option<i64> {
        match self {
            Self::Integer(v) => Some(*v),
            _ => None,
        }
    }

    /// This object as a number, widening integer to f64 — the §7.3.3
    /// NOTE 2 rule ("wherever a real number is expected, an integer may
    /// be used instead"). The converse direction is deliberately NOT
    /// offered.
    #[must_use]
    pub const fn as_number(&self) -> Option<f64> {
        match self {
            Self::Integer(v) => Some(*v as f64),
            Self::Real(v) => Some(*v),
            _ => None,
        }
    }

    /// This object as a name, if it is one.
    #[must_use]
    pub fn as_name(&self) -> Option<&Name> {
        match self {
            Self::Name(n) => Some(n),
            _ => None,
        }
    }

    /// This object as a dictionary, if it is one — a plain dict OR a
    /// stream's dict (a stream is usable anywhere its dictionary
    /// content is what matters).
    #[must_use]
    pub fn as_dict(&self) -> Option<&Dict> {
        match self {
            Self::Dict(d) => Some(d),
            Self::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }

    /// This object as an array, if it is one.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Object]> {
        match self {
            Self::Array(a) => Some(a),
            _ => None,
        }
    }

    /// This object as an indirect reference, if it is one.
    #[must_use]
    pub const fn as_reference(&self) -> Option<ObjId> {
        match self {
            Self::Reference(id) => Some(*id),
            _ => None,
        }
    }
}

/// Compare two objects that were parsed from **different source
/// buffers**, ignoring where their stream data physically sits.
///
/// ## Why the derived `PartialEq` is the wrong tool here, and is a trap
///
/// [`Stream`] stores a [`ByteSpan`] rather than the bytes themselves
/// (that is the whole point — see the type's docs), and `#[derive(
/// PartialEq)]` therefore compares **offsets**. Two structurally
/// identical documents whose streams sit at different byte positions
/// compare *unequal*, and — worse — two *different* streams that happen
/// to occupy the same span in two unrelated buffers compare *equal*.
///
/// That makes derived equality actively misleading for exactly the
/// comparison a round-trip harness needs: "did this document survive a
/// save?" A save legitimately moves objects (a full rewrite renumbers
/// every offset; an incremental append puts a second copy at the end),
/// so a span-sensitive comparison reports a false failure on every
/// stream in the corpus. This function was added in Pass 3.0 after
/// exactly that false red appeared across ~1,600 corpus files.
///
/// Within a single buffer the derived `PartialEq` remains correct and
/// cheaper, so it is deliberately left in place; this is the
/// cross-buffer counterpart, not a replacement.
///
/// ## What "equivalent" means precisely
///
/// - Streams: their **dictionaries** are compared recursively, and
///   their **raw (still filter-encoded) data bytes** are compared for
///   equality. Filters are not run — this is a structural comparison,
///   and decoding would both cost far more and hide a real difference
///   between two streams that decode alike.
/// - A stream whose span lies outside its buffer is treated as empty,
///   consistent with `crate::writer::serialize`'s degradation rule.
/// - Everything else is ordinary structural equality, recursing through
///   arrays and dictionaries so a nested difference cannot hide.
///
/// # Examples
///
/// ```
/// use pdfcer_core::object::{equivalent_across_buffers, Dict, Name, Object, Stream};
/// use pdfcer_core::span::ByteSpan;
///
/// // The same stream content at two different offsets.
/// let a = Object::Stream(Stream { dict: Dict::new(), data_span: ByteSpan::new(0, 2) });
/// let b = Object::Stream(Stream { dict: Dict::new(), data_span: ByteSpan::new(3, 2) });
/// assert_ne!(a, b);                                        // derived: offsets differ
/// assert!(equivalent_across_buffers(&a, b"hi", &b, b"xxxhi")); // content matches
/// ```
#[must_use]
pub fn equivalent_across_buffers(a: &Object, a_source: &[u8], b: &Object, b_source: &[u8]) -> bool {
    match (a, b) {
        (Object::Stream(x), Object::Stream(y)) => {
            let xd = x.data_span.slice(a_source).unwrap_or(&[]);
            let yd = y.data_span.slice(b_source).unwrap_or(&[]);
            xd == yd && dicts_equivalent(&x.dict, a_source, &y.dict, b_source)
        }
        (Object::Dict(x), Object::Dict(y)) => dicts_equivalent(x, a_source, y, b_source),
        (Object::Array(x), Object::Array(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y.iter())
                    .all(|(i, j)| equivalent_across_buffers(i, a_source, j, b_source))
        }
        // No stream can hide inside any other variant (§7.3.8.1: "all
        // streams shall be indirect objects"), so plain equality is
        // exact for the rest.
        _ => a == b,
    }
}

/// Recursive helper for [`equivalent_across_buffers`]: dictionaries
/// compare by entry order and content.
///
/// Order-sensitive on purpose. §7.3.7 says written order *"shall be
/// ignored"* semantically, but `crate::object::Dict` deliberately
/// **preserves** parsed order so a re-serialized dictionary diffs
/// minimally — so a round-trip that reordered entries would be a
/// minimal-diff regression this comparison should catch, not excuse.
fn dicts_equivalent(a: &Dict, a_source: &[u8], b: &Dict, b_source: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|((ak, av), (bk, bv))| {
            ak == bk && equivalent_across_buffers(av, a_source, bv, b_source)
        })
}

/// Where an indirect object's definition physically lives in the loaded
/// file — the provenance discriminator the minimal-diff writer needs
/// (ARCHITECTURE.md §5; decision 001 §6.1 item 1).
///
/// ## Why this is an enum and not a `ByteSpan`
///
/// §5's contract — "an object pdfcer did not logically modify re-emits
/// its source bytes verbatim, or is omitted entirely" — is only
/// *expressible* for an object defined at file level as
/// `N G obj … endobj`. A PDF 1.5 object **compressed inside an object
/// stream** (§7.5.7) has no byte range of its own in the file at all:
/// its bytes exist only inside its container's *encoded* stream data,
/// interleaved with the other objects of that container, and are not
/// even recoverable without running the container's filter chain.
///
/// Modelling that as "a span that happens to be wrong/absent" would be
/// a lie the writer could not detect. Making it a type-level
/// distinction forces every future save path to decide **consciously**
/// what a compressed object means for it:
///
/// - *Incremental save, object untouched* — emit nothing. The
///   container object is itself span-backed and untouched, so the whole
///   prior revision stays byte-identical. No decision needed.
/// - *Incremental save, object modified* — the object cannot be patched
///   in place; the writer must either re-emit it as a new file-level
///   object in the update section (legal: an object may move out of an
///   object stream between revisions) or write a whole new container.
///   Both are correct; the choice is the writer's, and it must be made
///   knowingly.
/// - *Full rewrite* — re-serialize from values; there is nothing
///   verbatim to preserve.
///
/// The [`ObjectStream`](Provenance::ObjectStream) variant carries the
/// container id and the index within it precisely so the writer has the
/// facts to make that choice without re-deriving them from the xref.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Provenance {
    /// Defined at file level. The span covers the **complete
    /// definition** — from the first byte of the object number through
    /// the last byte of `endobj` — so an untouched object re-emits (or,
    /// in incremental save, retains) exactly its source bytes.
    File(ByteSpan),
    /// Defined at file level at this span, but the retained bytes are
    /// **internally inconsistent** and therefore must NOT be re-emitted
    /// verbatim — the parsed value is authoritative, not the source bytes.
    ///
    /// Set in exactly one situation: cross-reference recovery re-derived a
    /// stream's extent from its `endstream` keyword because the stored
    /// `/Length` was unusable
    /// ([`StreamLengthPolicy::RecoverFromEndstream`](crate::parser::StreamLengthPolicy::RecoverFromEndstream)).
    /// The span's bytes then say one length while the parsed
    /// [`Stream::data_span`] says another, and copying them through would
    /// make pdfcer *write out* the very inconsistency it just recovered
    /// from — producing a file pdfcer itself would refuse to reload. The
    /// writer re-serializes instead, which regenerates `/Length` from the
    /// actual data (`writer::serialize`'s "`/Length` is always recomputed,
    /// never trusted").
    ///
    /// This is a third state rather than a flag on
    /// [`File`](Provenance::File) for the same reason
    /// [`ObjectStream`](Provenance::ObjectStream) is: it forces every save
    /// path to decide **consciously** what it means, instead of leaving a
    /// "span that happens to be untrustworthy" the writer cannot detect.
    /// [`file_span`](Provenance::file_span) still returns the span — the
    /// bytes are genuinely there, and a consumer that wants to *show* the
    /// definition's location (the CLI's object listing) is still right to
    /// ask. Only *re-emission* is affected.
    RecoveredFile(ByteSpan),
    /// Compressed inside an object stream (§7.5.7); reached through a
    /// type-2 cross-reference entry (§7.5.8.3 Table 18). There is no
    /// file-level span — see the type docs.
    ObjectStream {
        /// The object stream object that holds this object. Its own
        /// generation is always 0 (§7.5.7).
        container: ObjId,
        /// 0-based index of this object within the container's
        /// pair table (the type-2 entry's field 3).
        index: u32,
    },
}

impl Provenance {
    /// The object's verbatim byte range in the retained source buffer,
    /// or `None` for a compressed object (which has none).
    ///
    /// This is the accessor the writer uses; returning `Option` rather
    /// than a sentinel is the whole point of the enum (type docs).
    #[must_use]
    pub const fn file_span(self) -> Option<ByteSpan> {
        match self {
            // `RecoveredFile` answers `Some` deliberately: its bytes DO
            // exist at that span. What is untrustworthy is re-emitting
            // them, which is [`Provenance::is_verbatim_safe`]'s question,
            // not this one.
            Self::File(span) | Self::RecoveredFile(span) => Some(span),
            Self::ObjectStream { .. } => None,
        }
    }

    /// Whether this object's retained bytes may be copied through to a
    /// saved file **verbatim**.
    ///
    /// True only for [`File`](Provenance::File). A
    /// [`RecoveredFile`](Provenance::RecoveredFile) object has bytes but
    /// they contradict the parsed value, and an
    /// [`ObjectStream`](Provenance::ObjectStream) object has no file-level
    /// bytes at all — both must be re-serialized from values.
    ///
    /// Exists so the writer asks the question it actually means. Testing
    /// `file_span().is_some()` would silently start copying
    /// self-contradictory bytes the day the third variant appeared, which
    /// is precisely the "span that happens to be wrong" failure mode this
    /// enum was designed to make impossible.
    #[must_use]
    pub const fn is_verbatim_safe(self) -> bool {
        matches!(self, Self::File(_))
    }

    /// The containing object stream, or `None` for a file-level object.
    #[must_use]
    pub const fn container(self) -> Option<ObjId> {
        match self {
            Self::File(_) | Self::RecoveredFile(_) => None,
            Self::ObjectStream { container, .. } => Some(container),
        }
    }
}

/// A parsed indirect object: identifier, value, and the provenance that
/// makes minimal-diff re-emission possible.
///
/// See [`Provenance`] for why the location is a discriminated union
/// rather than a plain span.
#[derive(Debug, Clone, PartialEq)]
pub struct IndirectObject {
    /// The `(num, gen)` identity.
    pub id: ObjId,
    /// The parsed value between `obj` and `endobj` (or, for a
    /// compressed object, the bare value stored in its container —
    /// §7.5.7: "the `obj` and `endobj` keywords shall not be used").
    pub value: Object,
    /// Where this object's definition physically lives.
    pub provenance: Provenance,
}

impl IndirectObject {
    /// Shorthand for `self.provenance.file_span()`: the object's
    /// verbatim `N G obj … endobj` bytes in the retained buffer, or
    /// `None` when it is compressed inside an object stream.
    #[must_use]
    pub const fn file_span(&self) -> Option<ByteSpan> {
        self.provenance.file_span()
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

    /// The three provenance states answer the two questions independently:
    /// "are there file-level bytes?" and "may they be copied verbatim?".
    ///
    /// `RecoveredFile` is the case that makes the distinction necessary —
    /// it is the only state that answers YES to the first and NO to the
    /// second. A consumer that conflated them (testing
    /// `file_span().is_some()` to decide re-emission) would copy
    /// self-contradictory bytes into a saved file.
    #[test]
    fn provenance_separates_having_bytes_from_trusting_them() {
        let span = ByteSpan::from_range(0..10);
        let file = Provenance::File(span);
        let recovered = Provenance::RecoveredFile(span);
        let compressed = Provenance::ObjectStream {
            container: ObjId::new(7, 0),
            index: 2,
        };

        assert_eq!(file.file_span(), Some(span));
        assert_eq!(recovered.file_span(), Some(span));
        assert_eq!(compressed.file_span(), None);

        assert!(file.is_verbatim_safe());
        assert!(!recovered.is_verbatim_safe());
        assert!(!compressed.is_verbatim_safe());

        // Only a compressed object names a container; a recovered object
        // is still file-level.
        assert_eq!(file.container(), None);
        assert_eq!(recovered.container(), None);
        assert_eq!(compressed.container(), Some(ObjId::new(7, 0)));
    }

    #[test]
    fn dict_null_value_collapses_to_absent() {
        // §7.3.7/§7.3.9: present-with-null ≡ absent.
        let mut d = Dict::new();
        d.insert(Name::from(b"Alive"), Object::Integer(1));
        d.insert(Name::from(b"Ghost"), Object::Null);
        assert!(d.get(b"Alive").is_some());
        assert!(d.get(b"Ghost").is_none());
        assert!(!d.contains_key(b"Ghost"));
        // ...but the physical entry still exists for serialization.
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn dict_insert_replaces_in_place_preserving_order() {
        let mut d = Dict::new();
        d.insert(Name::from(b"A"), Object::Integer(1));
        d.insert(Name::from(b"B"), Object::Integer(2));
        d.insert(Name::from(b"A"), Object::Integer(9));
        let keys: Vec<&[u8]> = d.iter().map(|(k, _)| k.as_bytes()).collect();
        assert_eq!(keys, vec![&b"A"[..], b"B"]);
        assert_eq!(d.get(b"A").unwrap().as_int(), Some(9));
    }

    #[test]
    fn as_number_widens_integer_only() {
        // §7.3.3 NOTE 2 is one-directional.
        assert_eq!(Object::Integer(4).as_number(), Some(4.0));
        assert_eq!(Object::Real(4.5).as_number(), Some(4.5));
        assert_eq!(Object::Real(4.5).as_int(), None);
    }

    #[test]
    fn stream_exposes_its_dict_via_as_dict() {
        let s = Object::Stream(Stream {
            dict: {
                let mut d = Dict::new();
                d.insert(Name::from(b"Length"), Object::Integer(0));
                d
            },
            data_span: ByteSpan::new(0, 0),
        });
        assert!(s.as_dict().unwrap().contains_key(b"Length"));
    }

    #[test]
    fn name_debug_form_is_readable() {
        assert_eq!(format!("{:?}", Name::from(b"Type")), "/Type");
        assert_eq!(format!("{:?}", Name::from(b"lime Green")), "/lime#20Green");
    }

    #[test]
    fn objid_display_matches_reference_order() {
        assert_eq!(ObjId::new(12, 3).to_string(), "12 3");
    }

    #[test]
    fn cross_buffer_equivalence_ignores_stream_position() {
        // The false red this function exists to prevent: a save moves a
        // stream, the derived PartialEq compares spans, and every
        // stream-bearing file in the corpus reports a phantom change.
        let mut dict = Dict::new();
        dict.insert(Name::from(b"Length"), Object::Integer(2));
        let a = Object::Stream(Stream {
            dict: dict.clone(),
            data_span: ByteSpan::new(0, 2),
        });
        let b = Object::Stream(Stream {
            dict,
            data_span: ByteSpan::new(5, 2),
        });
        assert_ne!(
            a, b,
            "derived equality is span-sensitive (that is the trap)"
        );
        assert!(equivalent_across_buffers(&a, b"hi", &b, b"xxxxxhi"));
    }

    #[test]
    fn cross_buffer_equivalence_still_catches_real_differences() {
        // The other half: it must not become a comparison that always
        // says yes. Different data, and different dictionaries, both
        // have to fail.
        let a = Object::Stream(Stream {
            dict: Dict::new(),
            data_span: ByteSpan::new(0, 2),
        });
        let b = Object::Stream(Stream {
            dict: Dict::new(),
            data_span: ByteSpan::new(0, 2),
        });
        assert!(!equivalent_across_buffers(&a, b"hi", &b, b"ho"));

        let mut d = Dict::new();
        d.insert(Name::from(b"K"), Object::Integer(1));
        let c = Object::Stream(Stream {
            dict: d,
            data_span: ByteSpan::new(0, 2),
        });
        assert!(!equivalent_across_buffers(&a, b"hi", &c, b"hi"));
    }

    #[test]
    fn cross_buffer_equivalence_recurses_and_is_order_sensitive() {
        let mut x = Dict::new();
        x.insert(Name::from(b"A"), Object::Integer(1));
        x.insert(Name::from(b"B"), Object::Integer(2));
        let mut y = Dict::new();
        y.insert(Name::from(b"B"), Object::Integer(2));
        y.insert(Name::from(b"A"), Object::Integer(1));
        // Dict order is preserved by design (minimal diff), so a
        // reordering IS a regression worth catching.
        assert!(!equivalent_across_buffers(
            &Object::Dict(x.clone()),
            b"",
            &Object::Dict(y),
            b""
        ));
        assert!(equivalent_across_buffers(
            &Object::Array(vec![Object::Dict(x.clone())]),
            b"",
            &Object::Array(vec![Object::Dict(x)]),
            b""
        ));
        // Length mismatch in an array is caught before indexing.
        assert!(!equivalent_across_buffers(
            &Object::Array(vec![Object::Null]),
            b"",
            &Object::Array(vec![]),
            b""
        ));
    }

    #[test]
    fn provenance_distinguishes_file_backed_from_compressed_objects() {
        // The §5 writer contract in one assertion: only a file-level
        // object can offer verbatim bytes, and a compressed one names
        // the container instead of pretending to have a span.
        let file = Provenance::File(ByteSpan::new(10, 20));
        assert_eq!(file.file_span(), Some(ByteSpan::new(10, 20)));
        assert_eq!(file.container(), None);

        let compressed = Provenance::ObjectStream {
            container: ObjId::new(7, 0),
            index: 3,
        };
        assert_eq!(compressed.file_span(), None);
        assert_eq!(compressed.container(), Some(ObjId::new(7, 0)));
    }
}

//! # `view` — "the document as it is right now", for every READ path
//!
//! This module is the answer to the defect recorded in
//! `docs/decisions/018-edited-state-is-what-the-canvas-renders.md`: from
//! Pass 3.1 through Pass 16.2 every editing feature wrote correctly into
//! [`EditSession`](crate::edit::EditSession)'s overlay, and **none of them
//! were visible**, because the renderer and the vector object model both
//! read [`EditSession::document()`](crate::edit::EditSession::document) —
//! whose own doc comment says *"this is the base revision, not the edited
//! state."* One shared read path, fourteen invisible features.
//!
//! ## What a read path actually needs
//!
//! Reading a PDF page needs exactly two things, and they are separable:
//!
//! 1. **An object graph** — "what is object `12 0` right now?", "what does
//!    the trailer's `/Root` point at?". That is
//!    [`ObjectGraph`], and it has had two
//!    implementations since Pass 3.2: [`Document`](crate::document::Document)
//!    (the file as loaded) and
//!    [`SessionGraph`](crate::edit::SessionGraph)/[`EditSession`](crate::edit::EditSession)
//!    (the file as the operator currently has it).
//! 2. **A byte source** — "give me the bytes this stream's
//!    [`ByteSpan`] covers". Every stream in pdfcer is span-backed rather
//!    than owning its payload, because that is what makes round-trip /
//!    minimal-diff editing possible at all (`ARCHITECTURE.md` §5,
//!    [`crate::span`]).
//!
//! [`DocumentView`] is that pair, plus the declared [`PdfVersion`]. It was
//! originally introduced inside `pageops::assemble` (Pass 3.2) for
//! extract/merge/split, which need to copy pages *out of* an open editing
//! session. Decision 018 promotes it here because the renderer, the vector
//! decomposer and the object-model provider need the identical abstraction
//! — and because a type that three subsystems depend on should not live in
//! the private corner of a fourth.
//!
//! ## Why the byte source is an ENUM and not a `&[u8]`
//!
//! For a plain [`Document`](crate::document::Document), the byte source is
//! one contiguous buffer: the file as loaded. For an
//! [`EditSession`](crate::edit::EditSession) it is **two disjoint buffers**:
//! the base file, plus the R45 staging buffer holding stream payloads the
//! session has authored (dimension and markup appearance streams, spliced
//! content streams). `EditSession::stage_bytes` (private) assigns those
//! payloads spans in a **single combined coordinate system**:
//!
//! ```text
//!   span.start = base.len() + (offset within staging)
//! ```
//!
//! so a span alone is enough to say which half it belongs to. There are
//! three ways to serve such a span and only one of them is acceptable on a
//! per-frame path:
//!
//! - [`EditSession::authored_source`](crate::edit::EditSession::authored_source)
//!   materializes `base ++ staging` as one buffer. Correct, and right for
//!   its once-per-operation `pageops` callers — but it is a `Cow::Owned`
//!   full memcpy of the whole file (~14 MB on decision 018's benchmark
//!   document). Calling it once per rendered frame is not an option.
//! - Caching that concatenation inside the session costs one base-sized
//!   allocation plus invalidation logic, and buys nothing over the third
//!   option.
//! - **[`StreamSource::Split`]** keeps the two halves borrowed and picks
//!   between them with one integer comparison. Zero copy, zero allocation,
//!   nothing to invalidate. That is what this module implements.
//!
//! ### The non-straddling invariant
//!
//! [`StreamSource::Split`]'s dispatch is only sound because **no span ever
//! crosses the base/staging boundary**, and that is a structural property,
//! not a hope:
//!
//! - A span with [`Provenance::File`](crate::object::Provenance::File) semantics — i.e. one
//!   parsed out of the base file — necessarily ends at or before
//!   `base.len()`, because it indexes bytes that came from that buffer.
//! - A staged span necessarily starts at or after `base.len()`, because
//!   `EditSession::stage_bytes` computes `start = base.len() +
//!   staging.len()` *before* appending.
//!
//! A span that straddles is therefore a provenance bug, and
//! [`StreamSource::slice`] answers it with `None` rather than splicing two
//! unrelated buffers together — the `ARCHITECTURE.md` §10 fail-clean
//! posture, and the same degradation [`ByteSpan::slice`] already uses for
//! an out-of-bounds span. The invariant has its own regression test
//! (`straddling_span_is_refused_not_spliced` below); if a future change to
//! the staging offset scheme breaks it, that test fails loudly instead of
//! the renderer quietly drawing garbage.
//!
//! ## ⚠️ `DocumentView` is a READ view. It must never become the writer's input.
//!
//! This prohibition is load-bearing enough that decision 018 §10 asks for
//! it to live in the type's own documentation, and it is repeated on
//! [`DocumentView`] itself. The short form: the writer's source of truth is
//! `&Document` plus
//! [`DirtySet::combined_source`](crate::writer::DirtySet::combined_source),
//! and a future refactor that generalized `save_full`/`save_incremental`
//! over `DocumentView` could mistake a session's [`StreamSource::Split`]
//! for base bytes and splice staged payloads at base offsets. That is a
//! **silent** violation of the §5 round-trip invariant — the saved file
//! would be wrong and nothing would say so. Nothing the writer would need
//! is implemented on this type, deliberately.
//!
//! ## Spec sources
//!
//! - `iso32000__s__7.3.10.md` — indirect objects and reference resolution
//!   (inherited wholesale from [`ObjectGraph`]'s provided methods).
//! - `iso32000__s__7.3.8.md` — stream objects: the payload is bytes in the
//!   file, which is why a span + a buffer is the natural representation.

use crate::PdfVersion;
use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};
use crate::span::ByteSpan;

/// Where a [`ByteSpan`] carried by a stream in some object graph resolves
/// to actual bytes.
///
/// Two variants, because pdfcer has exactly two kinds of readable document
/// (module docs): a loaded file, whose spans all index one buffer, and an
/// editing session, whose spans index the base file **or** the R45 staging
/// buffer under one combined coordinate system.
///
/// `Copy`, because it is a pair of shared borrows and threading it through
/// a render walk by value is cheaper and clearer than by reference.
///
/// # Examples
///
/// ```
/// use pdfcer_core::span::ByteSpan;
/// use pdfcer_core::view::StreamSource;
///
/// let base = b"BASE-BYTES";
/// let staged = b"STAGED";
/// let src = StreamSource::Split { base, staged };
///
/// // A base-side span reads the base half.
/// assert_eq!(src.slice(ByteSpan::new(0, 4)), Some(&b"BASE"[..]));
/// // A staged span is expressed as `base.len() + local` (R45).
/// assert_eq!(src.slice(ByteSpan::new(base.len(), 6)), Some(&b"STAGED"[..]));
/// // A span that crosses the boundary is a provenance bug: refused.
/// assert_eq!(src.slice(ByteSpan::new(base.len() - 1, 3)), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamSource<'a> {
    /// One buffer: a plain [`Document`](crate::document::Document)'s
    /// retained file bytes. Spans index it directly, exactly as
    /// [`ByteSpan::slice`] does.
    Contiguous(&'a [u8]),
    /// Two disjoint buffers under one coordinate system: an
    /// [`EditSession`](crate::edit::EditSession)'s base file plus its R45
    /// staging buffer.
    ///
    /// A span with `start < base.len()` belongs to `base`; a span with
    /// `start >= base.len()` belongs to `staged` at local offset
    /// `start - base.len()`. See the module docs for why a span can never
    /// belong to both.
    Split {
        /// The base revision as loaded from disk.
        base: &'a [u8],
        /// Stream payloads authored this session, whose spans are offset
        /// by `base.len()` (see
        /// [`EditSession::stage_bytes`](crate::edit::EditSession)).
        staged: &'a [u8],
    },
}

impl<'a> StreamSource<'a> {
    /// The bytes `span` covers, or `None` if it cannot be served.
    ///
    /// `None` has exactly three causes, all of them logic errors rather
    /// than data errors, and all of them degraded rather than panicked per
    /// the crate's panic-free policy:
    ///
    /// 1. the span is out of bounds for the buffer it names;
    /// 2. the span straddles the base/staging boundary (module docs —
    ///    structurally impossible today, refused so that it stays that
    ///    way);
    /// 3. the span was produced against a *different* document's buffer.
    ///
    /// Callers turn `None` into their own named refusal — the renderer
    /// counts it as an undecodable stream, `pageops` stages an empty
    /// payload — rather than substituting plausible bytes.
    #[must_use]
    pub fn slice(&self, span: ByteSpan) -> Option<&'a [u8]> {
        match *self {
            Self::Contiguous(buf) => span.slice(buf),
            Self::Split { base, staged } => {
                if span.start >= base.len() {
                    // Wholly in the staged half. Re-express in staging-local
                    // coordinates; `checked_sub` cannot fail under the guard
                    // above but is used anyway so the arithmetic is total.
                    let local = span.start.checked_sub(base.len())?;
                    ByteSpan::new(local, span.len).slice(staged)
                } else if span.end() <= base.len() {
                    // Wholly in the base half.
                    span.slice(base)
                } else {
                    // Straddles. See the module docs: structurally
                    // impossible, refused rather than spliced.
                    None
                }
            }
        }
    }

    /// Total addressable length of this source, in the span coordinate
    /// system.
    ///
    /// Diagnostic only — used by [`DocumentView`]'s `Debug` and by tests
    /// asserting the R45 offset scheme (see `EditSession::stage_bytes`). Deliberately NOT a way to get at a
    /// buffer: there is no accessor returning the concatenation, because
    /// materializing one is precisely the cost this type exists to avoid.
    #[must_use]
    pub const fn len(&self) -> usize {
        match *self {
            Self::Contiguous(buf) => buf.len(),
            Self::Split { base, staged } => base.len().saturating_add(staged.len()),
        }
    }

    /// Whether this source addresses no bytes at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A document to READ: its object graph, the byte source its stream spans
/// resolve against, and the version it declares.
///
/// Built by [`Document::view`](crate::document::Document::view) (the file
/// as loaded) or [`EditSession::view`](crate::edit::EditSession::view) (the
/// file as the operator currently has it). Every read path in pdfcer that
/// can meaningfully run against either — the rasterizer, the vector object
/// model, `pageops`' cross-document copier — takes one of these rather than
/// a `&Document`, which is what makes "does this show unsaved edits?" a
/// property of the *caller's choice of view* instead of a property
/// scattered across fifty call sites.
///
/// # `impl ObjectGraph` is the whole trick
///
/// `DocumentView` implements [`ObjectGraph`] by delegating `value` and
/// `trailer_entry` to the graph it wraps. Because `resolve`, `resolved`,
/// `catalog_dict` and `catalog_id` are *provided* trait methods with
/// signatures identical to [`Document`](crate::document::Document)'s
/// inherent ones, changing a function's parameter from `&Document` to
/// `&DocumentView<'_>` leaves its body untouched. Decision 018 measured
/// this: 45 of `pdfcer-render`'s 50 `Document` call sites compiled
/// unchanged. The only bodies that needed editing were the five
/// `span.slice(doc.bytes())` sites, which became
/// [`DocumentView::slice`].
///
/// # ⚠️ Read-only. Never hand this to the writer.
///
/// `docs/decisions/018` §10 hazard 1, restated where it cannot be missed:
/// the writer's source of truth is `&Document` +
/// [`DirtySet::combined_source`](crate::writer::DirtySet::combined_source).
/// A `DocumentView` over an
/// [`EditSession`](crate::edit::EditSession) carries a
/// [`StreamSource::Split`]; code that assumed it held base bytes would
/// splice staged payloads at base offsets and emit a corrupt file with no
/// error — a **silent** breach of the `ARCHITECTURE.md` §5 round-trip
/// invariant, which is the worst failure mode this project has. Nothing
/// the writer needs is implemented here, and nothing should be added.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::graph::ObjectGraph;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../fixtures/synthetic/hello.pdf").to_vec(),
/// )?;
/// let view = doc.view();
/// // The view answers graph questions exactly as the document does.
/// assert_eq!(view.catalog_id(), doc.catalog_id());
/// # Ok(())
/// # }
/// ```
pub struct DocumentView<'a> {
    graph: &'a dyn ObjectGraph,
    source: StreamSource<'a>,
    version: PdfVersion,
}

impl<'a> DocumentView<'a> {
    /// Build a view whose stream spans index one contiguous `bytes`
    /// buffer.
    ///
    /// This is the constructor every pre-decision-018 caller already uses
    /// (`DocumentView::new(&doc, doc.bytes(), doc.version())`), kept with
    /// its original signature so `pageops`, `pdfcer` and the page-op
    /// tests are untouched by the promotion out of `pageops::assemble`.
    ///
    /// For an editing session use
    /// [`EditSession::view`](crate::edit::EditSession::view), which builds
    /// the [`StreamSource::Split`] form — passing
    /// `session.document().bytes()` here would resolve authored appearance
    /// spans off the end of the base buffer (the X5 failure the original
    /// `DocumentView` doc comment was written to catch).
    #[must_use]
    pub const fn new(graph: &'a dyn ObjectGraph, bytes: &'a [u8], version: PdfVersion) -> Self {
        Self {
            graph,
            source: StreamSource::Contiguous(bytes),
            version,
        }
    }

    /// Build a view over an explicit [`StreamSource`].
    ///
    /// The general constructor; [`DocumentView::new`] is the contiguous
    /// special case. Used by
    /// [`EditSession::view`](crate::edit::EditSession::view) and by
    /// [`DocumentView::clone_view`](crate::pageops) to carry a source
    /// through unchanged.
    #[must_use]
    pub const fn with_source(
        graph: &'a dyn ObjectGraph,
        source: StreamSource<'a>,
        version: PdfVersion,
    ) -> Self {
        Self {
            graph,
            source,
            version,
        }
    }

    /// The object graph this view reads.
    ///
    /// Callers that already hold a `&DocumentView` should prefer the
    /// [`ObjectGraph`] methods directly on the view (`view.resolve(…)`);
    /// this accessor exists for the places that must pass a
    /// `&dyn ObjectGraph` onward, such as `pageops`' copier.
    #[must_use]
    pub const fn graph(&self) -> &'a dyn ObjectGraph {
        self.graph
    }

    /// The byte source this view's stream spans resolve against.
    #[must_use]
    pub const fn source(&self) -> StreamSource<'a> {
        self.source
    }

    /// The bytes `span` covers in this view's source, or `None`.
    ///
    /// The replacement for the `span.slice(doc.bytes())` idiom. See
    /// [`StreamSource::slice`] for what `None` means and why it is not a
    /// panic.
    #[must_use]
    pub fn slice(&self, span: ByteSpan) -> Option<&'a [u8]> {
        self.source.slice(span)
    }

    /// The single contiguous buffer this view's spans index, or `None`
    /// when the view is over an editing session and therefore has two.
    ///
    /// Kept (per decision 018 §8) for callers that genuinely need a whole
    /// buffer rather than one span's worth of it, but returning `Option`
    /// rather than the pre-018 `&[u8]`: for a
    /// [`StreamSource::Split`] view there is no such buffer, and any
    /// answer other than "there isn't one" would be the X5 mis-slice
    /// hazard wearing a plausible face. Prefer [`DocumentView::slice`],
    /// which works for both shapes.
    #[must_use]
    pub const fn bytes(&self) -> Option<&'a [u8]> {
        match self.source {
            StreamSource::Contiguous(buf) => Some(buf),
            StreamSource::Split { .. } => None,
        }
    }

    /// The version this view's document declares (§7.5.2 / the catalog's
    /// `/Version` override).
    #[must_use]
    pub const fn version(&self) -> PdfVersion {
        self.version
    }
}

/// Delegating impl — the mechanism that let decision 018 change 27
/// function signatures without touching 45 call-site bodies (type docs).
impl ObjectGraph for DocumentView<'_> {
    fn value(&self, id: ObjId) -> Option<&Object> {
        self.graph.value(id)
    }

    fn trailer_entry(&self, key: &[u8]) -> Option<&Object> {
        self.graph.trailer_entry(key)
    }
}

impl std::fmt::Debug for DocumentView<'_> {
    /// Hand-written because `&dyn ObjectGraph` is not `Debug` and adding
    /// that bound would infect every implementor for the sake of one
    /// derive. Prints the facts a debugging session actually wants —
    /// including which *shape* of source this is, since "why is my edit
    /// invisible?" is answered by exactly that.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentView")
            .field("version", &self.version)
            .field("source", &self.source)
            .field("source_len", &self.source.len())
            .finish()
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
    use crate::object::{Dict, Name};
    use std::collections::BTreeMap;

    /// A hand-built graph, so the delegation is tested without dragging a
    /// parsed file in (mirrors `graph.rs`'s own `TestGraph`).
    struct TestGraph {
        objects: BTreeMap<ObjId, Object>,
        trailer: Dict,
    }

    impl ObjectGraph for TestGraph {
        fn value(&self, id: ObjId) -> Option<&Object> {
            self.objects.get(&id)
        }
        fn trailer_entry(&self, key: &[u8]) -> Option<&Object> {
            self.trailer.get(key)
        }
    }

    fn graph() -> TestGraph {
        let mut objects = BTreeMap::new();
        let mut catalog = Dict::new();
        catalog.insert(Name::from(b"Type"), Object::Name(Name::from(b"Catalog")));
        objects.insert(ObjId::new(1, 0), Object::Dict(catalog));
        objects.insert(ObjId::new(2, 0), Object::Integer(7));
        let mut trailer = Dict::new();
        trailer.insert(Name::from(b"Root"), Object::Reference(ObjId::new(1, 0)));
        TestGraph { objects, trailer }
    }

    #[test]
    fn contiguous_source_slices_like_a_plain_buffer() {
        let src = StreamSource::Contiguous(b"hello world");
        assert_eq!(src.slice(ByteSpan::new(6, 5)), Some(&b"world"[..]));
        assert_eq!(src.slice(ByteSpan::new(6, 99)), None);
        assert_eq!(src.len(), 11);
    }

    #[test]
    fn split_source_dispatches_on_the_base_length() {
        let base = b"0123456789";
        let staged = b"ABCDEF";
        let src = StreamSource::Split { base, staged };

        // Base half.
        assert_eq!(src.slice(ByteSpan::new(2, 3)), Some(&b"234"[..]));
        // Exactly the base half.
        assert_eq!(src.slice(ByteSpan::new(0, 10)), Some(&base[..]));
        // Staged half, in the R45 combined coordinate system.
        assert_eq!(src.slice(ByteSpan::new(10, 6)), Some(&b"ABCDEF"[..]));
        assert_eq!(src.slice(ByteSpan::new(13, 2)), Some(&b"DE"[..]));
        // Past the staged half.
        assert_eq!(src.slice(ByteSpan::new(14, 9)), None);
        assert_eq!(src.len(), 16);
    }

    /// The invariant decision 018 §4 owes a test: no span may cross the
    /// base/staging boundary, and one that does is REFUSED rather than
    /// spliced out of two unrelated buffers.
    ///
    /// This exists so that a future change to
    /// [`EditSession::stage_bytes`](crate::edit::EditSession)' offset
    /// scheme fails loudly here instead of silently producing a stream
    /// payload with a seam in the middle of it.
    #[test]
    fn straddling_span_is_refused_not_spliced() {
        let base = b"0123456789";
        let staged = b"ABCDEF";
        let src = StreamSource::Split { base, staged };

        // Starts inside base, ends inside staged: impossible by
        // construction, refused on principle.
        assert_eq!(src.slice(ByteSpan::new(9, 3)), None);
        assert_eq!(src.slice(ByteSpan::new(0, 16)), None);
        // The two adjacent NON-straddling spans both resolve, so the
        // refusal above is about crossing, not about the neighbourhood.
        assert_eq!(src.slice(ByteSpan::new(9, 1)), Some(&b"9"[..]));
        assert_eq!(src.slice(ByteSpan::new(10, 1)), Some(&b"A"[..]));
    }

    /// A zero-length span sitting exactly on the boundary is served by
    /// the staged half (`start >= base.len()`) and yields the empty
    /// slice — not `None`, because nothing is out of bounds about it.
    #[test]
    fn empty_span_at_the_boundary_is_served_not_refused() {
        let src = StreamSource::Split {
            base: b"0123456789",
            staged: b"ABCDEF",
        };
        assert_eq!(src.slice(ByteSpan::new(10, 0)), Some(&b""[..]));
    }

    /// An empty staging buffer (a session that has authored nothing)
    /// behaves exactly like the contiguous case over the base.
    #[test]
    fn split_with_empty_staging_matches_contiguous() {
        let base = b"0123456789";
        let split = StreamSource::Split { base, staged: b"" };
        let contiguous = StreamSource::Contiguous(base);
        for start in 0..base.len() {
            for len in 0..=(base.len() - start) {
                let span = ByteSpan::new(start, len);
                assert_eq!(split.slice(span), contiguous.slice(span), "{span}");
            }
        }
    }

    #[test]
    fn the_view_delegates_every_graph_question() {
        let g = graph();
        let view = DocumentView::new(&g, b"", PdfVersion { major: 1, minor: 7 });
        assert_eq!(view.value(ObjId::new(2, 0)), Some(&Object::Integer(7)));
        assert_eq!(view.resolved(ObjId::new(2, 0)), &Object::Integer(7));
        assert_eq!(view.catalog_id(), Some(ObjId::new(1, 0)));
        assert!(view.catalog_dict().is_some());
        // §7.3.10 through the delegation: a dangling id is null, not an
        // error, exactly as it is on the underlying graph.
        assert_eq!(view.resolved(ObjId::new(99, 0)), &Object::Null);
    }

    #[test]
    fn bytes_is_none_for_a_split_view() {
        let g = graph();
        let contiguous = DocumentView::new(&g, b"abc", PdfVersion { major: 1, minor: 7 });
        assert_eq!(contiguous.bytes(), Some(&b"abc"[..]));

        let split = DocumentView::with_source(
            &g,
            StreamSource::Split {
                base: b"abc",
                staged: b"de",
            },
            PdfVersion { major: 1, minor: 7 },
        );
        assert_eq!(
            split.bytes(),
            None,
            "a split view has no single buffer; answering with one would be the X5 mis-slice"
        );
        assert_eq!(split.slice(ByteSpan::new(3, 2)), Some(&b"de"[..]));
    }
}

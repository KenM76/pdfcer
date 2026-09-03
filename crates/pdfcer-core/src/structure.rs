//! **Internal-structure inspection** — read a PDF's COS object graph and its
//! physical file layout, and render either as text (`Pass 193.0`).
//!
//! # Why this module exists
//!
//! pdfcer could parse, render, edit and save a PDF, and it could not **show an
//! operator what it had read**. That gap was found the expensive way: a real
//! rendering defect (a vector soft-mask bevel whose highlight drew and whose
//! shadow did not) could not be diagnosed without hand-decompressing the file's
//! object streams in a throwaway script, because every `/ExtGState` on the page
//! lived inside an object stream and no shipped verb could reach it.
//!
//! **A renderer that cannot show its operator what it read is one whose defects
//! can only be diagnosed by its author.** That is the argument for this being a
//! shipped capability rather than a debugging aid, and it is why this module is
//! `pub` and documented as a contract rather than tucked behind a `#[cfg]`.
//!
//! # The two questions, kept apart on purpose
//!
//! A PDF has a **logical** structure (a graph of objects reached from the
//! trailer's `/Root`) and a **physical** one (where those objects actually
//! live: an offset in the file, or a slot inside an object stream, in some
//! revision, indexed by a cross-reference table or a cross-reference stream).
//! They answer different questions and a reader who conflates them is misled:
//!
//! - [`walk`] and [`render_object`] answer *"what does this document say?"* —
//!   [`ObjectGraph`]-level, resolution-aware, and identical whether the object
//!   was compressed or not.
//! - [`layout`] answers *"how is this file put together?"* — the xref style,
//!   the revision chain, which objects are compressed inside which container,
//!   linearization, encryption, and whether the cross-reference table had to be
//!   rebuilt by scanning.
//!
//! `ROADMAP.md`'s standing rule against normalising a file as a side effect of
//! an unrelated edit is exactly the invariant [`layout`] makes checkable: an
//! operator can now *see* that a save preserved the file's shape rather than
//! taking pdfcer's word for it.
//!
//! # Everything here is bounded, and that is a security property
//!
//! `ARCHITECTURE.md` §10 requires an output-size ceiling on every filter
//! decoder and a depth/cycle guard on every recursive structure walker. This
//! module is both at once — it decodes untrusted streams *and* walks an
//! untrusted graph — so every bound in [`DumpOptions`] is a real defence, not
//! a display preference:
//!
//! - **Cycles are the normal case, not the pathological one.** Every page's
//!   `/Parent` points back at its `/Pages` node, so the very first realistic
//!   walk revisits objects. [`walk`] tracks visited ids and emits a back-
//!   reference marker instead of recursing.
//! - **A 12-byte stream can decode to gigabytes.** [`DumpOptions::max_stream_bytes`]
//!   truncates and *says so*; it never silently shortens.
//! - **An object count ceiling** bounds a walk over a file with a million
//!   objects, and the result reports the truncation rather than looking
//!   complete.
//!
//! # This is a REPORT, not a serialiser
//!
//! The text [`render_object`] produces is deliberately **not** valid PDF
//! syntax and must never be fed back to a parser. It expands indirect
//! references inline, annotates objects with their storage, truncates streams,
//! and marks cycles. Writing PDF is `crate::writer`'s job and duplicating it
//! here would create a second answer to "what does this object look like on
//! disk" — exactly the drift `R92` exists to prevent.
//!
//! # Fuzzy, never sneaky (project rule 4)
//!
//! Every place this module could not show something, it says so in the output:
//! a stream that would not decode carries its filter error, a truncated stream
//! carries its true length, a walk that hit a ceiling carries the ceiling it
//! hit, and a dangling reference is marked as unresolvable rather than printed
//! as `null`. §7.3.10 makes a dangling reference resolve to null for a
//! *reader*; for someone inspecting structure, "there is nothing there" and
//! "there is an explicit null there" are different facts and are printed
//! differently.

use crate::document::Document;
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::xref::{SectionShape, XrefEntry};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// How much of a stream's data to include in a dump.
///
/// Separate from [`DumpOptions::max_stream_bytes`] because "how much" and
/// "which bytes" are independent choices: an operator diagnosing a content
/// stream wants it decoded, one diagnosing a *filter* wants it raw, and one
/// reading a page tree wants neither and would rather not scroll past a font
/// program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamMode {
    /// Report the stream's dictionary and its length, and omit the data.
    ///
    /// The default, because most structural questions are about keys rather
    /// than payloads and an embedded font program is tens of kilobytes of
    /// noise in the middle of the answer.
    #[default]
    Omit,
    /// The bytes exactly as they sit in the file, still encoded.
    ///
    /// What you want when the *filter* is under suspicion — a `/FlateDecode`
    /// that will not inflate, a `/Length` that disagrees with the data, a
    /// predictor that produces the wrong row width.
    Raw,
    /// The bytes after every filter in `/Filter` has run.
    ///
    /// Fallible by nature: an untrusted file's stream may not decode at all.
    /// The failure is reported in place, with the filter error, and the dump
    /// continues — a single corrupt stream must not cost the operator the rest
    /// of the document (`ARCHITECTURE.md` §10 fail-clean).
    Decoded,
}

/// Bounds and formatting choices for a dump.
///
/// Constructed with [`DumpOptions::default`] and adjusted field by field. The
/// defaults are chosen to be **useful on a first look at an unknown file**
/// rather than exhaustive: a shallow expansion of a page's dictionary tells you
/// what it references, and a reader who wants the whole graph asks for it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DumpOptions {
    /// How many levels of indirect reference to expand.
    ///
    /// `0` prints references as `N G R` without following them. Each level
    /// resolves one more hop. This is the **reference** depth, not the
    /// container-nesting depth: a deeply nested direct dictionary prints in
    /// full at depth 0, because it is all one object and truncating it would
    /// hide keys the file really does carry in that object.
    pub max_depth: usize,
    /// Ceiling on how many distinct objects one walk will visit.
    ///
    /// A bound on work, not on correctness: hitting it is reported.
    pub max_objects: usize,
    /// Whether and how to include stream data.
    pub streams: StreamMode,
    /// Ceiling on the bytes of any one stream included in the output.
    ///
    /// Applied **after** decoding, because the decoded size is the one that can
    /// explode — §10's decompression-bomb case is a small stream that inflates
    /// enormously, and a ceiling on the encoded size would not catch it.
    pub max_stream_bytes: usize,
}

impl Default for DumpOptions {
    fn default() -> Self {
        Self {
            max_depth: 1,
            max_objects: 4096,
            streams: StreamMode::Omit,
            max_stream_bytes: 4096,
        }
    }
}

/// Builders, because `#[non_exhaustive]` makes a struct expression impossible
/// from outside this crate.
///
/// ★ **This was found by a consumer, not by a test.** Every in-crate test could
/// write `DumpOptions { max_depth: 2, ..Default::default() }` — an in-crate
/// caller is exempt from `#[non_exhaustive]` — and `pdfcer` could not,
/// because it is a different crate and feels the attribute the way `pdfcer-gui`
/// and the future web shell will. An options struct nobody outside the crate
/// can build is not an options struct.
///
/// The attribute is KEPT rather than dropped: it is what lets a future bound be
/// added without a breaking change, and these setters are the idiomatic way to
/// pay for it. Each is `#[must_use]` and consuming, so they chain.
impl DumpOptions {
    /// Set how many levels of indirect reference to expand.
    #[must_use]
    pub const fn with_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// Set the ceiling on distinct objects one walk will visit.
    #[must_use]
    pub const fn with_max_objects(mut self, max: usize) -> Self {
        self.max_objects = max;
        self
    }

    /// Set whether and how stream data is included.
    #[must_use]
    pub const fn with_streams(mut self, mode: StreamMode) -> Self {
        self.streams = mode;
        self
    }

    /// Set the ceiling on bytes shown for any one stream.
    #[must_use]
    pub const fn with_max_stream_bytes(mut self, max: usize) -> Self {
        self.max_stream_bytes = max;
        self
    }
}

/// Where an object physically lives.
///
/// The distinction the logical graph deliberately hides. It matters to anyone
/// asking why an object is invisible to `grep`, and to anyone checking that a
/// save preserved the file's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Storage {
    /// A top-level `N G obj` at this byte offset (xref type `n` / stream
    /// type 1).
    AtOffset {
        /// Byte offset of the object's `N G obj` header.
        offset: u64,
        /// The generation the cross-reference entry records.
        generation: u16,
    },
    /// Compressed inside an object stream (§7.5.7, xref stream type 2).
    ///
    /// Carries no generation, because a type-2 entry has none: §7.3.10 and
    /// §7.5.7 fix the generation of every compressed object at 0, and inventing
    /// one here would report information the file does not contain.
    InObjectStream {
        /// Object number of the containing object stream.
        container: u32,
        /// 0-based index within that container's pair table.
        index: u32,
    },
    /// The cross-reference marks this number free (type `f` / type 0).
    Free {
        /// Generation to assign when this number is next reused.
        generation: u16,
    },
    /// The object was parsed but the cross-reference table has no entry for it.
    ///
    /// Reachable on a **recovered** document, where the table was rebuilt by
    /// scanning (decision 013): the scan finds objects the original table never
    /// indexed. Named rather than folded into `Free`, because "the file said
    /// this number is unused" and "the file's index never mentioned it" are
    /// different facts about how damaged the document is.
    Unindexed,
}

/// One row of an object inventory.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ObjectRecord {
    /// The object's identity.
    pub id: ObjId,
    /// Where it lives in the file.
    pub storage: Storage,
    /// The COS type, as a stable lowercase word: `dictionary`, `stream`,
    /// `array`, `name`, `string`, `integer`, `real`, `boolean`, `null`,
    /// `reference`.
    pub kind: &'static str,
    /// The value of `/Type`, when the object is a dictionary or stream that
    /// has one.
    ///
    /// `/Type` is optional on many dictionaries (§7.3.7), so `None` is a
    /// perfectly ordinary answer and must not be read as "malformed".
    pub type_name: Option<String>,
    /// The value of `/Subtype`, on the same terms.
    pub subtype: Option<String>,
    /// Length of the raw stream data, for a stream object.
    pub stream_bytes: Option<usize>,
    /// Every object that holds a reference to this one.
    ///
    /// ★ **Parity-plus, and the expensive half of the inventory.** A forward
    /// reference is free to read; the reverse direction requires scanning every
    /// object in the document. It is worth the scan because the questions an
    /// operator actually asks are reverse ones — *"what still points at this?"*,
    /// *"is anything using this font?"*, *"why did deleting that orphan a
    /// stream?"* — and because a **empty** list on an object the catalog should
    /// reach is how an unreferenced object announces itself.
    ///
    /// The trailer is not an object, so an object referenced only from the
    /// trailer (the catalog, `/Info`, `/Encrypt`) has an empty list here. See
    /// [`Inventory::trailer_referenced`].
    pub referenced_by: Vec<ObjId>,
}

/// The result of [`inventory`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Inventory {
    /// One row per parsed object, ordered by object number then generation.
    pub objects: Vec<ObjectRecord>,
    /// Objects referenced directly by the trailer dictionary.
    ///
    /// Held separately because the trailer is not an object and cannot appear
    /// in an [`ObjectRecord::referenced_by`] list. Without this, the catalog
    /// would look unreferenced in every document.
    pub trailer_referenced: Vec<ObjId>,
}

impl Inventory {
    /// Objects that nothing references — neither another object nor the
    /// trailer.
    ///
    /// **An orphan is not necessarily a defect.** An incremental update leaves
    /// superseded objects behind by design, and §7.5.6 permits them; a file
    /// that has been edited and saved incrementally is *expected* to carry
    /// some. This reports them so an operator can judge, and deliberately does
    /// not call them errors.
    #[must_use]
    pub fn unreferenced(&self) -> Vec<ObjId> {
        self.objects
            .iter()
            .filter(|r| r.referenced_by.is_empty() && !self.trailer_referenced.contains(&r.id))
            .map(|r| r.id)
            .collect()
    }
}

/// The physical facts about how a file is put together.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FileLayout {
    /// The version the document reports, header and catalog reconciled.
    pub version: String,
    /// `xref table`, `xref stream`, or `xref table + XRefStm (hybrid)`.
    pub xref_style: String,
    /// The offset the file's own `startxref` names.
    pub startxref: u64,
    /// Number of parsed objects.
    pub object_count: usize,
    /// Highest object number the cross-reference chain mentions, before the
    /// `/Size` filter.
    pub highest_object_number: u32,
    /// How many cross-reference entries an under-reported `/Size` hid.
    ///
    /// Non-zero means the file is **hiding** entries, and that is worth
    /// surfacing rather than burying: raising `/Size` — which creating any
    /// object does — would expose every one of them.
    pub suppressed_by_size: usize,
    /// Object-stream container number → the object numbers it holds.
    pub object_streams: BTreeMap<u32, Vec<u32>>,
    /// Whether Annex F linearization was detected.
    pub linearized: bool,
    /// Whether the file was encrypted (and decrypted at load).
    pub encrypted: bool,
    /// `Some` when the cross-reference table had to be rebuilt by scanning
    /// because the stored one could not be parsed (decision 013).
    ///
    /// A recovered document cannot be saved incrementally — its base
    /// cross-reference is not trustworthy — so this is a fact with a
    /// consequence, not a curiosity.
    pub recovered: Option<String>,
}

/// Build the object inventory, including the reverse-reference map.
///
/// Cost is O(objects × references), one pass to collect forward edges and one
/// to invert them. On a large document this is the expensive call in the
/// module; [`layout`] and [`render_object`] are cheap by comparison.
#[must_use]
pub fn inventory(doc: &Document) -> Inventory {
    let mut refs_to: BTreeMap<ObjId, Vec<ObjId>> = BTreeMap::new();
    for obj in doc.objects() {
        let mut out = Vec::new();
        collect_references(&obj.value, &mut out);
        for target in out {
            refs_to.entry(target).or_default().push(obj.id);
        }
    }
    for v in refs_to.values_mut() {
        v.sort_unstable_by_key(|id| (id.num, id.generation));
        v.dedup();
    }

    let mut rows: Vec<ObjectRecord> = doc
        .objects()
        .map(|obj| {
            let dict: Option<&Dict> = match &obj.value {
                Object::Dict(d) => Some(d),
                Object::Stream(s) => Some(&s.dict),
                _ => None,
            };
            ObjectRecord {
                id: obj.id,
                storage: storage_of(doc, obj.id),
                kind: kind_of(&obj.value),
                type_name: dict.and_then(|d| name_value(d, b"Type")),
                subtype: dict.and_then(|d| name_value(d, b"Subtype")),
                stream_bytes: match &obj.value {
                    Object::Stream(s) => Some(s.data_span.len),
                    _ => None,
                },
                referenced_by: refs_to.get(&obj.id).cloned().unwrap_or_default(),
            }
        })
        .collect();
    rows.sort_by_key(|r| (r.id.num, r.id.generation));

    let mut trailer_referenced = Vec::new();
    for (_, v) in doc.trailer().iter() {
        collect_references(v, &mut trailer_referenced);
    }
    trailer_referenced.sort_unstable_by_key(|id| (id.num, id.generation));
    trailer_referenced.dedup();

    Inventory {
        objects: rows,
        trailer_referenced,
    }
}

/// Read the physical layout facts.
#[must_use]
pub fn layout(doc: &Document) -> FileLayout {
    let mut object_streams: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (num, entry) in doc.xref().iter() {
        if let XrefEntry::InStream { stream_num, .. } = entry {
            object_streams.entry(stream_num).or_default().push(num);
        }
    }
    for v in object_streams.values_mut() {
        v.sort_unstable();
    }

    FileLayout {
        version: doc.version().to_string(),
        xref_style: match doc.section_shape() {
            SectionShape::Classic { xref_stm: Some(_) } => {
                "xref table + XRefStm (hybrid-reference)".to_owned()
            }
            SectionShape::Classic { xref_stm: None } => "xref table".to_owned(),
            SectionShape::Stream { id, widths } => {
                format!("xref stream (object {id}, /W {widths:?})")
            }
        },
        startxref: doc.base_startxref(),
        object_count: doc.object_count(),
        highest_object_number: doc.next_object_number().map_or(0, |n| n.saturating_sub(1)),
        suppressed_by_size: doc.suppressed_object_count(),
        object_streams,
        linearized: doc.linearization().is_marked(),
        encrypted: doc.encryption().is_some(),
        recovered: doc.recovery().map(|r| format!("{r:?}")),
    }
}

/// Render one object as annotated text, expanding references to
/// [`DumpOptions::max_depth`].
///
/// Returns a description of the *absence* rather than an error when `id` does
/// not resolve — a missing object is a normal finding when inspecting a
/// damaged file, and returning `Err` would make the common case awkward for
/// every caller.
#[must_use]
pub fn render_object<G: ObjectGraph + ?Sized>(
    graph: &G,
    bytes: &[u8],
    id: ObjId,
    options: &DumpOptions,
) -> String {
    let mut out = String::new();
    let mut seen = BTreeSet::new();
    let Some(value) = graph.value(id) else {
        return format!("{id} — no such object (never defined, freed, or a stale generation)\n");
    };
    let _ = writeln!(out, "{id} obj");
    seen.insert(id);
    write_value(&mut out, graph, bytes, value, options, 0, 1, &mut seen);
    out.push('\n');
    out
}

/// Walk from `root`, rendering every object reachable within the options'
/// bounds.
///
/// The bound that matters here is [`DumpOptions::max_objects`]: a walk from the
/// catalog of a real document reaches everything, and "everything" on a large
/// file is not an answer anyone can read. When the ceiling stops the walk it is
/// stated in the output rather than left to look like completeness.
#[must_use]
pub fn walk<G: ObjectGraph + ?Sized>(
    graph: &G,
    bytes: &[u8],
    root: ObjId,
    options: &DumpOptions,
) -> String {
    let mut out = String::new();
    let mut seen: BTreeSet<ObjId> = BTreeSet::new();
    let mut queue: Vec<ObjId> = vec![root];
    let mut emitted = 0usize;

    while let Some(id) = queue.pop() {
        if !seen.insert(id) {
            continue;
        }
        if emitted >= options.max_objects {
            let _ = writeln!(
                out,
                "\n… stopped after {} object(s): max_objects reached. {} more were queued.",
                options.max_objects,
                queue.len() + 1
            );
            return out;
        }
        let Some(value) = graph.value(id) else {
            let _ = writeln!(out, "{id} — unresolvable reference\n");
            continue;
        };
        emitted += 1;
        let _ = writeln!(out, "{id} obj");
        // Depth 0 inside a walk: the walk itself supplies the traversal, so
        // expanding references here too would print the same object twice in
        // two different shapes.
        let flat = DumpOptions {
            max_depth: 0,
            ..options.clone()
        };
        let mut inner_seen = BTreeSet::new();
        write_value(&mut out, graph, bytes, value, &flat, 0, 1, &mut inner_seen);
        out.push('\n');

        let mut targets = Vec::new();
        collect_references(value, &mut targets);
        // Reverse so the pop-order above visits them in document order; a dump
        // whose object order depends on a stack's direction is one nobody can
        // diff against a previous run.
        targets.reverse();
        for t in targets {
            if !seen.contains(&t) {
                queue.push(t);
            }
        }
    }
    out
}

// ---- internals -----------------------------------------------------------

/// Where the cross-reference says `id` lives.
fn storage_of(doc: &Document, id: ObjId) -> Storage {
    match doc.xref().get(id.num) {
        Some(XrefEntry::InUse { offset, generation }) => Storage::AtOffset { offset, generation },
        Some(XrefEntry::InStream { stream_num, index }) => Storage::InObjectStream {
            container: stream_num,
            index,
        },
        Some(XrefEntry::Free { generation, .. }) => Storage::Free { generation },
        None => Storage::Unindexed,
    }
}

/// The stable one-word COS type name used throughout the reports.
const fn kind_of(o: &Object) -> &'static str {
    match o {
        Object::Null => "null",
        Object::Boolean(_) => "boolean",
        Object::Integer(_) => "integer",
        Object::Real(_) => "real",
        Object::String(_) => "string",
        Object::Name(_) => "name",
        Object::Array(_) => "array",
        Object::Dict(_) => "dictionary",
        Object::Stream(_) => "stream",
        Object::Reference(_) => "reference",
        // No catch-all arm. `Object` is `#[non_exhaustive]` for downstream
        // crates but exhaustive HERE, so omitting the wildcard makes a future
        // variant a compile error at this exact site rather than a silent
        // "unknown" in every report.
    }
}

/// A dictionary entry's value as a name, for the inventory's `/Type` columns.
///
/// Direct values only: resolving here would need a graph, and a `/Type` written
/// as an indirect reference is vanishingly rare and would be more honestly
/// reported as absent than silently followed.
fn name_value(d: &Dict, key: &[u8]) -> Option<String> {
    match d.get(key) {
        Some(Object::Name(n)) => Some(String::from_utf8_lossy(&n.0).into_owned()),
        _ => None,
    }
}

/// Every indirect reference `o` holds, transitively through DIRECT containers.
///
/// Stops at reference boundaries by design: a reference is an edge, and
/// following it here would make one object's edge list include another's.
fn collect_references(o: &Object, out: &mut Vec<ObjId>) {
    match o {
        Object::Reference(id) => out.push(*id),
        Object::Array(items) => {
            for i in items {
                collect_references(i, out);
            }
        }
        Object::Dict(d) => {
            for (_, v) in d.iter() {
                collect_references(v, out);
            }
        }
        Object::Stream(s) => {
            for (_, v) in s.dict.iter() {
                collect_references(v, out);
            }
        }
        _ => {}
    }
}

/// Indentation for nested containers, capped so a pathological nesting depth
/// cannot produce megabytes of spaces.
fn pad(level: usize) -> String {
    "  ".repeat(level.min(32))
}

/// Render one value, following references while `depth` remains.
#[allow(clippy::too_many_arguments)]
fn write_value<G: ObjectGraph + ?Sized>(
    out: &mut String,
    graph: &G,
    bytes: &[u8],
    value: &Object,
    options: &DumpOptions,
    depth: usize,
    level: usize,
    seen: &mut BTreeSet<ObjId>,
) {
    match value {
        Object::Null => out.push_str("null"),
        Object::Boolean(b) => {
            let _ = write!(out, "{b}");
        }
        Object::Integer(i) => {
            let _ = write!(out, "{i}");
        }
        Object::Real(r) => {
            let _ = write!(out, "{r}");
        }
        Object::Name(n) => {
            let _ = write!(out, "/{}", String::from_utf8_lossy(&n.0));
        }
        Object::String(s) => {
            let _ = write!(out, "({})", escape_string(s));
        }
        Object::Reference(id) => {
            if depth >= options.max_depth {
                let _ = write!(out, "{id} R");
            } else if seen.contains(id) {
                // A cycle, and on a page tree this is the COMMON case rather
                // than a malformed one: every page's `/Parent` points back up.
                let _ = write!(out, "{id} R  % already shown above (cycle)");
            } else {
                match graph.value(*id) {
                    Some(inner) => {
                        seen.insert(*id);
                        let _ = write!(out, "{id} R -> ");
                        write_value(out, graph, bytes, inner, options, depth + 1, level, seen);
                    }
                    None => {
                        // §7.3.10 makes this null to a READER. To someone
                        // inspecting structure it is a distinct fact, so it is
                        // printed as one.
                        let _ = write!(out, "{id} R  % UNRESOLVABLE (no such object)");
                    }
                }
            }
        }
        Object::Array(items) => {
            out.push_str("[ ");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_value(out, graph, bytes, item, options, depth, level, seen);
            }
            out.push_str(" ]");
        }
        Object::Dict(d) => write_dict(out, graph, bytes, d, options, depth, level, seen),
        Object::Stream(s) => {
            write_dict(out, graph, bytes, &s.dict, options, depth, level, seen);
            write_stream_data(out, bytes, s, options, level);
        } // No catch-all, for the reason `kind_of` gives: a new COS variant must
          // break this build rather than render as a placeholder.
    }
}

/// Render a dictionary, one key per line, keys in the file's own order.
#[allow(clippy::too_many_arguments)]
fn write_dict<G: ObjectGraph + ?Sized>(
    out: &mut String,
    graph: &G,
    bytes: &[u8],
    d: &Dict,
    options: &DumpOptions,
    depth: usize,
    level: usize,
    seen: &mut BTreeSet<ObjId>,
) {
    if d.is_empty() {
        out.push_str("<< >>");
        return;
    }
    out.push_str("<<\n");
    for (k, v) in d.iter() {
        let _ = write!(out, "{}/{} ", pad(level), String::from_utf8_lossy(&k.0));
        write_value(out, graph, bytes, v, options, depth, level + 1, seen);
        out.push('\n');
    }
    let _ = write!(out, "{}>>", pad(level.saturating_sub(1)));
}

/// Append a stream's data per [`StreamMode`], truncated and disclosed.
fn write_stream_data(
    out: &mut String,
    bytes: &[u8],
    s: &crate::object::Stream,
    options: &DumpOptions,
    level: usize,
) {
    let raw = s
        .data_span
        .start
        .checked_add(s.data_span.len)
        .filter(|end| *end <= bytes.len())
        .and_then(|end| bytes.get(s.data_span.start..end));
    let Some(raw) = raw else {
        let _ = write!(
            out,
            "\n{}% stream data span {}..{} lies outside the {}-byte buffer",
            pad(level),
            s.data_span.start,
            s.data_span.start + s.data_span.len,
            bytes.len()
        );
        return;
    };
    match options.streams {
        StreamMode::Omit => {
            let _ = write!(
                out,
                "\n{}stream … {} raw byte(s), omitted (--streams raw|decoded to include)\nendstream",
                pad(level),
                raw.len()
            );
        }
        StreamMode::Raw => {
            let _ = write!(out, "\n{}stream  % raw, as stored\n", pad(level));
            append_bounded(out, raw, options.max_stream_bytes);
            let _ = write!(out, "\n{}endstream", pad(level));
        }
        StreamMode::Decoded => match crate::filters::decode_stream(&s.dict, raw) {
            Ok(data) => {
                let _ = write!(
                    out,
                    "\n{}stream  % decoded, {} byte(s) from {} raw\n",
                    pad(level),
                    data.len(),
                    raw.len()
                );
                append_bounded(out, &data, options.max_stream_bytes);
                let _ = write!(out, "\n{}endstream", pad(level));
            }
            Err(e) => {
                // Reported, never fatal: one stream that will not decode must
                // not cost the operator the rest of the dump.
                let _ = write!(
                    out,
                    "\n{}stream  % WOULD NOT DECODE: {e}\n{}% {} raw byte(s) retained; use --streams raw to see them\n{}endstream",
                    pad(level),
                    pad(level),
                    raw.len(),
                    pad(level)
                );
            }
        },
    }
}

/// Append at most `limit` bytes, printable-escaped, disclosing any truncation.
fn append_bounded(out: &mut String, data: &[u8], limit: usize) {
    let shown = data.len().min(limit);
    out.push_str(&escape_bytes(data.get(..shown).unwrap_or(data)));
    if shown < data.len() {
        let _ = write!(
            out,
            "\n… truncated: {} of {} byte(s) shown (--max-stream-bytes to raise)",
            shown,
            data.len()
        );
    }
}

/// Escape a byte string for a literal-string rendering.
fn escape_string(s: &[u8]) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s {
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            0x20..=0x7e => out.push(b as char),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            _ => {
                let _ = write!(out, "\\{b:03o}");
            }
        }
    }
    out
}

/// Escape arbitrary stream bytes for display.
///
/// Newlines and tabs survive as themselves — a content stream is far more
/// readable with its line structure intact, and that is the overwhelmingly
/// common thing anyone dumps.
fn escape_bytes(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len());
    for &b in data {
        match b {
            b'\n' | b'\r' | b'\t' => out.push(b as char),
            0x20..=0x7e => out.push(b as char),
            _ => {
                let _ = write!(out, "\\x{b:02x}");
            }
        }
    }
    out
}

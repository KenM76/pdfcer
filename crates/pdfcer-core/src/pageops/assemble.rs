//! # Assembling a new document from pages of existing ones
//!
//! One function — [`assemble`] — underlies **all four** document-producing
//! structural operations, because they differ only in which pages go in
//! and which catalog entries come along:
//!
//! | Operation | `order` | `catalog_from` | outline |
//! |---|---|---|---|
//! | extract | a subset of one source | that source | subset |
//! | split | one range per output, repeatedly | the source | subset |
//! | merge | every page of every source | *(none — fresh)* | one top-level entry per source |
//! | insert | the target's pages with the source's spliced in | the target | subset |
//!
//! Writing four assemblers instead would mean four places for the
//! duplicate-field-name rule, four places for the dangling-destination
//! policy, and four opportunities for them to disagree. The Acrobat RAG
//! makes the same observation from the other direction:
//! `core_ops__split_document.md` records that *"splitting is
//! architecturally 'extract N times', not a distinct mechanism"*, and
//! `core_ops__reorder_move_pages.md` that a cross-document move is
//! *"Insert (into target) + optional Delete (from source) composed"*.
//!
//! ## The deep-copy closure, and the barrier that stops it
//!
//! Copying a page means copying everything reachable from it: content
//! streams, resources, fonts, images, annotations. A naive reachability
//! walk immediately drags in **the whole document**, by two routes:
//!
//! 1. a page's `/Parent` points at its `Pages` node, which points at
//!    every sibling page;
//! 2. a link annotation's destination points at some *other* page.
//!
//! So the walk carries a **barrier**: the set of page objects that are
//! *not* being copied. Any reference that lands on one is refused, and
//! the refusal propagates outward by one level —
//!
//! - a **dictionary entry** whose value hit the barrier is **dropped**
//!   (so an annotation keeps its rectangle and appearance but loses its
//!   `/Dest`, which is exactly what "this link now points nowhere"
//!   should look like);
//! - an **array** containing a barrier hit refuses **as a whole**, which
//!   propagates the refusal up to the enclosing dictionary entry.
//!
//! That second rule deserves its reasoning, because "null out the one
//! element" looks more conservative and is worse. Arrays are positional,
//! so an element cannot simply be removed — and the arrays in which a
//! **page reference** can appear are, in practice, exactly the explicit
//! destinations of §12.3.2.2, whose element 0 *is* the page. Nulling it
//! yields `/Dest [null /Fit]`: a destination that is present, malformed,
//! and points nowhere. Refusing the array instead yields an annotation
//! with **no** `/Dest`, which is a link that does nothing — the same
//! outcome, honestly spelled, and the one every reader handles.
//!
//! `/Parent` is handled separately — it is stripped from every copied
//! page before the walk starts, because the new document's page tree is
//! built fresh.
//!
//! Every barrier hit is counted into
//! [`AssembleReport::dangling_references`]. That is the honesty
//! obligation: `core_ops__extract_pages.md` records that Acrobat carries
//! *"all content, form fields, comments, and links from the original"*
//! and says nothing about what happens to a link whose target left, so
//! pdfcer carries the link, breaks the destination, and **says so**.
//!
//! ## Attribute materialization — the silent-corruption bug this avoids
//!
//! §7.7.3.4 makes `Resources`, `MediaBox`, `CropBox` and `Rotate`
//! inheritable. A page that inherits its `MediaBox` from a `Pages` node
//! is fine — until it is copied into a new document whose root node has
//! no such entry, at which point the page renders at the wrong size, or
//! fails to render at all.
//!
//! Every copied page therefore has its **raw** inherited values written
//! onto it, for the four attributes it does not already carry
//! ([`InheritedRaw::materialize_for`](crate::page_tree::InheritedRaw::materialize_for)).
//! Raw, not resolved: the value is usually a single indirect reference,
//! so the copy stays small and the shared resource dictionary stays
//! shared. Resolving would inline an entire resource tree per page.
//!
//! ## Stream data and the staging buffer
//!
//! [`Stream`](crate::object::Stream) stores a [`ByteSpan`] into a
//! retained source buffer rather than owning its bytes — the mechanism
//! §5's verbatim re-emission is built on. A cross-document copy has no
//! such buffer, so this module builds one: every copied stream's **raw,
//! still-filter-encoded** payload is appended to a staging buffer and the
//! copy's span is repointed at it.
//!
//! Two things fall out, both wanted. Filters are never run, so a
//! JPEG stays a JPEG and a Flate stream is never re-compressed at a
//! different level — the copy is byte-exact, not merely equivalent. And
//! the existing serializer works unmodified, because "a value tree plus
//! the buffer its spans index into" is precisely its interface.
//!
//! ## `/ID` on a genuinely new document (§14.4)
//!
//! `crate::writer`'s R39 discipline says pdfcer never *synthesises* an
//! `/ID` for a file that lacks one, and explicitly defers the exception:
//! *"Revisit when a genuine from-scratch / 'Save As new document' path
//! exists — that is the context in which `ID[0]` legitimately changes
//! too."* This module is that path. An extracted or merged file is a new
//! document, §14.4 says a file **should** carry an identifier, and there
//! is no prior identity to preserve.
//!
//! Both elements are derived **deterministically from the assembled body
//! bytes**. No clock, no host name, no build hash — so assembling the
//! same pages twice produces byte-identical output, which is what makes
//! the operations testable at all and keeps R41 satisfied.

use std::collections::{BTreeMap, HashMap, HashSet};

// NOTE: `crate::graph::ObjectGraph` is deliberately NOT imported here.
// Since decision 018 this module reaches the graph through
// `DocumentView::graph()`, which yields a `&dyn ObjectGraph` — and trait
// methods on a trait object resolve without the trait being in scope.
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::page_tree::{self, PageSlot};
use crate::pageops::separation::{self, SeparationImpact, SeparationPolicy};
use crate::settings::{TrailingEol, XrefEntryEol};
use crate::span::ByteSpan;
use crate::writer::encoder::IdentityEncoder;
use crate::writer::{fileid, serialize, xref_out};
use crate::{PdfVersion, pageops::PageOpError, pageops::outline};

/// Maximum nesting depth followed while transforming one object's value
/// tree (pdfcer policy, `ARCHITECTURE.md` §10).
///
/// Deep enough for any real resource dictionary; shallow enough that the
/// recursion cannot exhaust the stack on a hostile file. Exceeding it
/// degrades that sub-value to `null` and counts it, rather than
/// truncating the copy silently.
pub const MAX_COPY_DEPTH: usize = 64;

/// Maximum objects one assemble may copy (pdfcer policy).
///
/// Naturally bounded by the sources' object counts too; this is the
/// belt-and-braces bound that holds even if a future source view is
/// generated rather than parsed.
pub const MAX_COPIED_OBJECTS: usize = 5_000_000;

// `DocumentView` used to be DEFINED here. Decision 018 promoted it to the
// top-level [`crate::view`] module, because `pdfcer-render`, the vector
// object model and the GUI's hit-test provider all need the same
// abstraction and none of them should have to reach into `pageops` for it.
//
// The re-export is not vestigial politeness: `pageops::DocumentView` is the
// path every existing caller (`pdfcer`, `pageops`' own submodules, the
// page-op integration tests) already names, and decision 018's whole
// premise is that this change costs no call sites. The type's original R45
// doc comment — "the caller building a view over an editing session must
// pass `session.authored_source()` as `bytes`" — is now DISCHARGED
// STRUCTURALLY rather than by instruction: `EditSession::view()` builds a
// [`crate::view::StreamSource::Split`], which serves an authored
// appearance's span from the staging half without materializing anything.
pub use crate::view::DocumentView;

/// What outline (bookmark) entries the assembled document gets.
///
/// The Acrobat RAG leaves this genuinely undecided — bookmark carryover
/// on extract is a **GAP** (*"conservative reading … is 'not carried'
/// until verified empirically"*), while per-source bookmark generation on
/// combine is a **confirmed documented default**. pdfcer therefore adopts
/// one policy per operation and records it here rather than guessing at
/// an unverified Acrobat behaviour, exactly as
/// `core_ops__insert_pages.md` asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OutlinePolicy {
    /// Carry every outline entry whose destination lands on a copied
    /// page, rewritten to point at the copy; drop the rest, counted.
    ///
    /// This is the RAG's own recommendation for extract
    /// (*"carry any outline entry whose destination page falls within the
    /// extracted set, rewritten to the new page index; drop entries
    /// pointing elsewhere"*) and it is applied identically to split,
    /// because split **is** repeated extract and the bucket should not
    /// behave two ways.
    Subset,
    /// Generate one top-level entry per source document, with that
    /// source's own carried entries nested beneath it.
    ///
    /// Acrobat's documented Combine-Files default, and the one behaviour
    /// in this cluster that is confirmed rather than inferred.
    PerSource,
    /// No outline at all.
    Drop,
}

/// Options for one [`assemble`] call.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AssembleOptions {
    /// Which source's catalog supplies the carried entries
    /// (`/OCProperties`, `/Lang`, `/ViewerPreferences`, `/PageMode`,
    /// `/PageLayout`), or `None` for a fresh catalog.
    pub catalog_from: Option<usize>,
    /// Which source's `/Info` dictionary the output carries, or `None`.
    pub info_from: Option<usize>,
    /// What happens to outline entries.
    pub outline: OutlinePolicy,
    /// Human-facing titles for [`OutlinePolicy::PerSource`]'s generated
    /// entries, one per source, already encoded as PDF text strings
    /// (§7.9.2).
    ///
    /// Supplied by the caller rather than derived here because deriving
    /// one means turning a file path into display text, and
    /// `pdfcer-core` does not own user-facing text (decision 002 R1).
    pub source_titles: Vec<Vec<u8>>,
    /// Whether to carry the catalog source's `/PageLabels` number tree
    /// (§12.4.2) into the output, **stale**, rather than dropping it.
    ///
    /// The two answers are right for different operations, and the
    /// difference is whether the output's pages are a *subset* of a
    /// source's or a *superset*:
    ///
    /// - **Insert** adds pages to a document that keeps all of its own,
    ///   so `true`. `core_ops__page_labels_and_bates_interaction.md`
    ///   records Acrobat's baseline — *"Inserting pages does not renumber
    ///   a later label section to account for the shift"* — and
    ///   recommends pdfcer *"leave `/PageLabels` numerically stale exactly
    ///   as Acrobat does"* for this Pass. pdfcer matches that **and emits
    ///   a diagnostic**, which is the parity-plus half: Acrobat leaves
    ///   them stale and silent.
    /// - **Extract / split** produce a document whose pages are a subset
    ///   in a different order, so `false`. Carrying a label tree there
    ///   would not merely be stale, it would be *confidently wrong* about
    ///   pages that are not in the file — worse than absent.
    /// - **Merge** has no single source to inherit from, so `false`.
    pub carry_page_labels: bool,
    /// Whether to auto-rename AcroForm fields whose fully-qualified names
    /// collide across sources.
    ///
    /// Acrobat's confirmed Combine-Files default, reproduced with its own
    /// `Doc0_`/`Doc1_` prefix pattern because it is the one documented
    /// precedent in this cluster. Off for a single-source operation,
    /// where no collision is possible and renaming would be gratuitous.
    pub rename_duplicate_fields: bool,
    /// What to do when the selection splits a preseparated page set
    /// (§14.11.4).
    ///
    /// Defaults to [`SeparationPolicy::Repair`], which keeps the surviving
    /// members' `/Pages` arrays truthful. See
    /// [`crate::pageops::separation`] for the full reasoning, including
    /// why refusing is not the default and why this is a product policy
    /// rather than a spec ambiguity.
    pub separations: SeparationPolicy,
}

impl Default for AssembleOptions {
    /// Single-source defaults: carry source 0's catalog and metadata,
    /// subset its outline, rename nothing.
    fn default() -> Self {
        Self {
            catalog_from: Some(0),
            info_from: Some(0),
            outline: OutlinePolicy::Subset,
            source_titles: Vec::new(),
            carry_page_labels: false,
            rename_duplicate_fields: false,
            separations: SeparationPolicy::Repair,
        }
    }
}

impl AssembleOptions {
    /// Choose what happens when the selection splits a preseparated page
    /// set (§14.11.4).
    ///
    /// A builder method rather than a public field assignment because
    /// [`AssembleOptions`] is `#[non_exhaustive]`: callers outside
    /// `pdfcer-core` — `pdfcer`, `pdfce-gui`, and the integration tests
    /// — cannot write a struct expression for it at all, so without this
    /// the policy would be unreachable from every front end that needs it.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::pageops::{AssembleOptions, SeparationPolicy};
    ///
    /// let options = AssembleOptions::default().with_separations(SeparationPolicy::Refuse);
    /// assert_eq!(options.separations, SeparationPolicy::Refuse);
    /// ```
    #[must_use]
    pub const fn with_separations(mut self, policy: SeparationPolicy) -> Self {
        self.separations = policy;
        self
    }
}

/// What an [`assemble`] actually did — the honest report every front end
/// prints.
///
/// Every counter here exists because the corresponding thing is
/// **invisible in the output file**. An operator can see how many pages
/// they got; they cannot see that four bookmarks were dropped or that two
/// form fields were renamed underneath them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct AssembleReport {
    /// Pages in the assembled document.
    pub pages: usize,
    /// Indirect objects written.
    pub objects_copied: usize,
    /// References that pointed at a page which was not copied, and were
    /// therefore dropped or nulled. See the module docs' barrier section.
    pub dangling_references: usize,
    /// Outline entries carried into the output.
    pub outline_items_kept: usize,
    /// Outline entries dropped because their destination was not copied.
    pub outline_items_dropped: usize,
    /// AcroForm fields renamed to resolve a cross-source name collision.
    pub form_fields_renamed: usize,
    /// AcroForm fields dropped because their widget annotations straddle
    /// the copied/not-copied boundary.
    ///
    /// pdfcer declines to copy half a field: §12.7.3.1 identifies a field
    /// by name across the **whole document**, so a field that kept only
    /// some of its widgets would be a field whose value applies to
    /// controls that no longer exist. Dropping it whole, and counting it,
    /// is the honest form of that refusal. Always `0` for a merge, where
    /// every page of every source is copied.
    pub form_fields_dropped: usize,
    /// Named destinations (§12.3.2.3) present in a source and **not**
    /// carried into the output.
    ///
    /// pdfcer does not carry the `/Dests` name tree: a name tree merged
    /// across sources has no conflict-resolution rule in Acrobat's own
    /// documentation (`core_ops__merge_combine_files.md` records the
    /// **GAP**), and carrying a subset would silently change which names
    /// resolve. Outline entries that used a *name* are still carried —
    /// they are rewritten to explicit destinations, which need no tree.
    pub named_destinations_dropped: usize,
    /// Whether a source's `/PageLabels` number tree (§12.4.2) was
    /// dropped rather than carried.
    ///
    /// Carrying a label tree onto a *subset* of pages would produce
    /// labels that are confidently wrong about pages that are not in the
    /// file, which is worse than absent. See
    /// [`AssembleOptions::carry_page_labels`] for when the other answer
    /// is right.
    pub page_labels_dropped: bool,
    /// Whether a `/PageLabels` tree was **carried and is now stale** —
    /// the insert case.
    ///
    /// Acrobat leaves it stale silently; pdfcer leaves it stale and says
    /// so. Distinct from [`AssembleReport::page_labels_dropped`] because
    /// the operator's next action differs: a stale tree wants
    /// renumbering, an absent one wants creating.
    pub page_labels_stale: bool,
    /// Whether a source's `/StructTreeRoot` (§14.7, Tagged PDF) was
    /// dropped rather than carried.
    ///
    /// Subsetting a structure tree to a page subset is a real piece of
    /// accessibility engineering that this Pass does not do, and
    /// `core_ops__extract_pages.md` records that Acrobat's own behaviour
    /// here is an unverified **GAP**. Copying it wholesale would leave
    /// dangling marked-content references — a document that *claims* to
    /// be tagged and is not. Dropped, counted, named.
    pub struct_tree_dropped: bool,
    /// Whether `/OCProperties` (§8.11 optional content) was carried.
    ///
    /// Carried wholesale when a catalog is carried: layer *definitions*
    /// stay valid whatever page subset survives, and an unused OCG entry
    /// is inert. Reported because a merged document can end up with
    /// duplicate same-named layers —
    /// `core_ops__merge_combine_files.md` records that Acrobat has no
    /// documented coalescing here either.
    pub optional_content_carried: bool,
    /// What the selection did to preseparated page sets (§14.11.4).
    ///
    /// Empty — [`SeparationImpact::is_empty`] — for every document that is
    /// not preseparated, which is nearly all of them. Non-empty means the
    /// output contains pages that used to be separations of a logical page
    /// whose other plates did not come along, and the operator is being
    /// told which plates those were.
    ///
    /// This is the one field here that is **not** merely informational: a
    /// `/Pages` array left naming pages that were not copied is a
    /// non-conforming file, so the repair it reports is a correctness fix,
    /// not a courtesy.
    pub separations: SeparationImpact,
}

/// One page to place in the assembled document: which source, and which
/// page index within it.
pub type PageRef = (usize, usize);

/// Build a new PDF from pages of existing documents.
///
/// `order` is the output's page sequence; each entry names a source and a
/// **0-based page index within that source's document order**.
///
/// # Errors
///
/// - [`PageOpError::PageTree`] — a source's page tree could not be walked.
/// - [`PageOpError::PageOutOfRange`] — `order` names a page a source does
///   not have.
/// - [`PageOpError::DuplicatePage`] — `order` names one page twice.
/// - [`PageOpError::NoPages`] — `order` is empty. §7.7.3.3 requires a page
///   tree to have at least one page, and a zero-page PDF is refused
///   rather than written; Acrobat likewise *"cannot delete the only
///   remaining page"*.
/// - [`PageOpError::ObjectLimit`] — the copy exceeded [`MAX_COPIED_OBJECTS`].
pub fn assemble(
    sources: &[DocumentView<'_>],
    order: &[PageRef],
    options: &AssembleOptions,
) -> Result<(Vec<u8>, AssembleReport), PageOpError> {
    if order.is_empty() {
        return Err(PageOpError::NoPages);
    }

    // Every source's page slots: needed for the barrier (all page ids),
    // for inherited-attribute materialization, and for index → id.
    let mut slots: Vec<Vec<PageSlot>> = Vec::with_capacity(sources.len());
    for source in sources {
        slots.push(page_tree::page_slots(source.graph())?);
    }

    // Resolve `order` into (source, page id), refusing out-of-range and
    // duplicate entries before anything is written.
    let mut selected: Vec<(usize, ObjId)> = Vec::with_capacity(order.len());
    let mut seen: HashSet<(usize, ObjId)> = HashSet::new();
    for &(source_index, page_index) in order {
        let source_slots = slots
            .get(source_index)
            .ok_or(PageOpError::SourceOutOfRange {
                index: source_index,
            })?;
        let slot = source_slots
            .get(page_index)
            .ok_or(PageOpError::PageOutOfRange {
                index: page_index,
                count: source_slots.len(),
            })?;
        if !seen.insert((source_index, slot.id)) {
            return Err(PageOpError::DuplicatePage { index: page_index });
        }
        selected.push((source_index, slot.id));
    }

    // The barrier: every page object in every source that is NOT being
    // copied. See the module docs.
    let mut barrier: HashSet<(usize, ObjId)> = HashSet::new();
    for (source_index, source_slots) in slots.iter().enumerate() {
        for slot in source_slots {
            if !seen.contains(&(source_index, slot.id)) {
                barrier.insert((source_index, slot.id));
            }
        }
    }

    let mut copier = Copier::new(barrier);
    // Objects 1 and 2 are reserved for the catalog and the root `Pages`
    // node before anything else is allocated, so the output's first two
    // objects are always the two a human opening it in a hex editor wants
    // to find. Purely a readability choice; nothing depends on it.
    let catalog_num = copier.reserve();
    let root_num = copier.reserve();

    // Pre-register every selected page's output number, so that a
    // reference from one copied page to another (a link between two
    // extracted pages) maps to the copy rather than being re-copied.
    let mut page_numbers: Vec<u32> = Vec::with_capacity(selected.len());
    for &(source_index, id) in &selected {
        page_numbers.push(copier.reserve_for(source_index, id));
    }

    // Accumulated across the page loop because `report` is not built
    // until the tree is finished, below.
    let mut separations = SeparationImpact::default();

    // Copy each page. `/Parent` is stripped: the new tree is built below.
    for (position, &(source_index, id)) in selected.iter().enumerate() {
        let view = sources
            .get(source_index)
            .ok_or(PageOpError::SourceOutOfRange {
                index: source_index,
            })?;
        let slot = slots
            .get(source_index)
            .and_then(|s| s.iter().find(|s| s.id == id))
            .ok_or(PageOpError::PageOutOfRange {
                index: position,
                count: 0,
            })?;
        let Some(page_dict) = view.graph().resolved(id).as_dict() else {
            return Err(PageOpError::PageNotADictionary { id });
        };

        let mut copied = copier.copy_dict(view, source_index, page_dict, 0)?;
        copied.remove(b"Parent");
        copied.insert(
            Name::from(b"Parent"),
            Object::Reference(ObjId::new(root_num, 0)),
        );
        // §14.11.4: this page may be one plate of a preseparated set whose
        // other plates were not selected. The generic copy above has
        // already done the generically-right thing and the specifically-
        // wrong one — the `/Pages` array hit the barrier, refused as a
        // whole, and took the Required `/Pages` entry with it. Rebuild it
        // in output space. Done HERE rather than inside `copy_dict`
        // because this is the only place that knows it is holding a page.
        let per_page = separation::remap_copied(
            view.graph(),
            page_dict,
            &mut copied,
            |member| {
                selected
                    .iter()
                    .position(|&(source, id)| source == source_index && id == member)
                    .and_then(|at| page_numbers.get(at).copied())
            },
            options.separations,
        )?;
        separation::accumulate(&mut separations, &per_page);
        // §7.7.3.4: attributes the old ancestors supplied would be lost.
        for (key, value) in slot.inherited.materialize_for(page_dict) {
            let mapped = copier.copy_value(view, source_index, &value, 0)?;
            copied.insert(Name::from(key), mapped.unwrap_or(Object::Null));
        }
        let number = *page_numbers.get(position).ok_or(PageOpError::NoPages)?;
        copier.store(number, Object::Dict(copied));
    }

    // The root `Pages` node (Table 29). `/Count` is the number of LEAF
    // pages below it, which for a flat tree is the page count.
    let mut root = Dict::new();
    root.insert(Name::from(b"Type"), Object::Name(Name::from(b"Pages")));
    root.insert(
        Name::from(b"Kids"),
        Object::Array(
            page_numbers
                .iter()
                .map(|n| Object::Reference(ObjId::new(*n, 0)))
                .collect(),
        ),
    );
    root.insert(
        Name::from(b"Count"),
        Object::Integer(i64::try_from(page_numbers.len()).unwrap_or(i64::MAX)),
    );
    copier.store(root_num, Object::Dict(root));

    let mut report = AssembleReport {
        pages: page_numbers.len(),
        separations,
        ..AssembleReport::default()
    };

    // The catalog (Table 28).
    let mut catalog = Dict::new();
    catalog.insert(Name::from(b"Type"), Object::Name(Name::from(b"Catalog")));
    catalog.insert(
        Name::from(b"Pages"),
        Object::Reference(ObjId::new(root_num, 0)),
    );
    if let Some(index) = options.catalog_from
        && let Some(view) = sources.get(index)
        && let Some(source_catalog) = view.graph().catalog_dict()
    {
        carry_catalog_entries(
            &mut copier,
            view,
            index,
            source_catalog,
            options.carry_page_labels,
            &mut catalog,
            &mut report,
        )?;
    }
    // Named destinations are never carried; count what was dropped so the
    // omission is disclosed rather than invisible.
    for (index, view) in sources.iter().enumerate() {
        let _ = index;
        report.named_destinations_dropped +=
            crate::pageops::references::DestinationResolver::new(view.graph()).named_count();
    }

    // AcroForm (§12.7.2), then outlines (§12.3.3).
    build_acroform(
        &mut copier,
        sources,
        &slots,
        &selected,
        &page_numbers,
        options,
        &mut catalog,
        &mut report,
    )?;
    outline::build(
        &mut copier,
        sources,
        &selected,
        &page_numbers,
        options,
        &mut catalog,
        &mut report,
    )?;

    if let Some(index) = options.info_from
        && let Some(view) = sources.get(index)
        && let Some(info_id) = view
            .graph()
            .trailer_entry(b"Info")
            .and_then(Object::as_reference)
        && view.graph().value(info_id).is_some()
    {
        let number = copier.map_reference(view, index, info_id)?;
        if let Some(number) = number {
            copier.info = Some(number);
        }
    }

    copier.store(catalog_num, Object::Dict(catalog));
    copier.drain(sources)?;
    report.objects_copied = copier.objects.len();
    report.dangling_references += copier.dangling;

    let version = sources
        .iter()
        .map(DocumentView::version)
        .max()
        .unwrap_or(PdfVersion { major: 1, minor: 7 });
    let bytes = copier.finish(catalog_num, version);
    Ok((bytes, report))
}

/// Carry the catalog entries that remain meaningful for a different page
/// set, and record what was deliberately left behind.
///
/// The list is short and every omission is deliberate:
///
/// | Entry | Carried? | Why |
/// |---|---|---|
/// | `/OCProperties` | yes | layer *definitions* stay valid; an unused OCG is inert (§8.11) |
/// | `/Lang`, `/ViewerPreferences`, `/PageMode`, `/PageLayout` | yes | document-wide preferences, page-set independent |
/// | `/StructTreeRoot` | **no** | would leave dangling marked-content refs — a file that claims to be tagged and is not |
/// | `/PageLabels` | **only for insert** | see [`AssembleOptions::carry_page_labels`] |
/// | `/Names`, `/Dests` | **no** | no documented merge rule (RAG **GAP**); a subset silently changes which names resolve |
/// | `/Outlines` | **no** here | rebuilt by [`outline::build`] |
/// | `/AcroForm` | **no** here | rebuilt by [`build_acroform`] |
/// | `/Perms`, `/Encrypt` | **no** | signature permissions and encryption do not survive into a new document, and carrying either would be a claim pdfcer cannot honour |
/// | `/Threads` | **no** | article beads reference pages; a subset dangles |
fn carry_catalog_entries(
    copier: &mut Copier,
    view: &DocumentView<'_>,
    source_index: usize,
    source_catalog: &Dict,
    carry_page_labels: bool,
    catalog: &mut Dict,
    report: &mut AssembleReport,
) -> Result<(), PageOpError> {
    for key in [
        &b"OCProperties"[..],
        b"Lang",
        b"ViewerPreferences",
        b"PageMode",
        b"PageLayout",
    ] {
        let Some(value) = source_catalog.get(key) else {
            continue;
        };
        if let Some(mapped) = copier.copy_value(view, source_index, value, 0)? {
            catalog.insert(Name::from(key), mapped);
            if key == b"OCProperties" {
                report.optional_content_carried = true;
            }
        }
    }
    report.struct_tree_dropped |= source_catalog.contains_key(b"StructTreeRoot");
    if let Some(labels) = source_catalog.get(b"PageLabels") {
        if carry_page_labels {
            if let Some(mapped) = copier.copy_value(view, source_index, labels, 0)? {
                catalog.insert(Name::from(b"PageLabels"), mapped);
                report.page_labels_stale = true;
            }
        } else {
            report.page_labels_dropped = true;
        }
    }
    Ok(())
}

/// Rebuild the interactive-form dictionary (§12.7.2 Table 218) for the
/// assembled document.
///
/// Three jobs, in order:
///
/// 1. **Select** the top-level fields worth carrying. A field is carried
///    only if every widget annotation it owns sits on a copied page — see
///    [`AssembleReport::form_fields_dropped`] for why half a field is
///    refused rather than trimmed.
/// 2. **Copy** them, and merge each source's `/DR` (default resources)
///    into one, first-source-wins on a name collision. That collision
///    only affects appearance *generation* (`/NeedAppearances`), never an
///    appearance stream that already exists, so first-wins is safe; it is
///    recorded here because it is the kind of thing that looks like an
///    oversight later.
/// 3. **Rename** colliding fully-qualified names when
///    [`AssembleOptions::rename_duplicate_fields`] is set, using
///    Acrobat's own documented `Doc0_`/`Doc1_` prefix pattern.
///
/// That third step is not cosmetic. `core_ops__merge_combine_files.md`
/// records the failure it prevents: without it, *"same-named fields
/// across merged sources become LINKED as a single logical field — typing
/// into one instance fills every instance — a frequently-reported real
/// complaint, not a rare edge case."* §12.7.3.1 identifies a field by its
/// fully-qualified name across the whole document, so the linkage is the
/// format working as designed, not a bug to be fixed elsewhere.
#[allow(clippy::too_many_arguments)] // assemble state, threaded once
fn build_acroform(
    copier: &mut Copier,
    sources: &[DocumentView<'_>],
    slots: &[Vec<PageSlot>],
    selected: &[(usize, ObjId)],
    page_numbers: &[u32],
    options: &AssembleOptions,
    catalog: &mut Dict,
    report: &mut AssembleReport,
) -> Result<(), PageOpError> {
    let _ = (slots, page_numbers);
    // Which annotations live on a copied page, and which on any page.
    let mut copied_annots: HashSet<(usize, ObjId)> = HashSet::new();
    let selected_set: HashSet<(usize, ObjId)> = selected.iter().copied().collect();
    for (source_index, view) in sources.iter().enumerate() {
        for slot in slots.get(source_index).map(Vec::as_slice).unwrap_or(&[]) {
            let Some(page) = view.graph().resolved(slot.id).as_dict() else {
                continue;
            };
            let Some(annots) = page
                .get(b"Annots")
                .map(|o| view.graph().resolve(o))
                .and_then(Object::as_array)
            else {
                continue;
            };
            let on_copied_page = selected_set.contains(&(source_index, slot.id));
            for annot in annots {
                if let Some(id) = annot.as_reference()
                    && on_copied_page
                {
                    copied_annots.insert((source_index, id));
                }
            }
        }
    }

    let mut fields: Vec<Object> = Vec::new();
    let mut default_resources = Dict::new();
    let mut need_appearances = false;
    let mut sig_flags: i64 = 0;
    // Output object number → the `Doc<N>_` prefix its top-level field
    // should get, filled in after every field is copied so collisions are
    // detected across all sources at once.
    let mut top_level: Vec<(usize, u32)> = Vec::new();

    for (source_index, view) in sources.iter().enumerate() {
        let Some(acroform) = view
            .graph()
            .catalog_dict()
            .and_then(|c| c.get(b"AcroForm").map(|o| view.graph().resolve(o)))
            .and_then(Object::as_dict)
        else {
            continue;
        };
        need_appearances |= acroform
            .get(b"NeedAppearances")
            .map(|o| view.graph().resolve(o))
            .is_some_and(|v| matches!(v, Object::Boolean(true)));
        sig_flags |= acroform
            .get(b"SigFlags")
            .map(|o| view.graph().resolve(o))
            .and_then(Object::as_int)
            .unwrap_or(0);

        if let Some(dr) = acroform
            .get(b"DR")
            .map(|o| view.graph().resolve(o))
            .and_then(Object::as_dict)
        {
            for (category, value) in dr.iter() {
                if default_resources.get(category.as_bytes()).is_none()
                    && let Some(mapped) = copier.copy_value(view, source_index, value, 0)?
                {
                    default_resources.insert(category.clone(), mapped);
                }
            }
        }

        let Some(source_fields) = acroform
            .get(b"Fields")
            .map(|o| view.graph().resolve(o))
            .and_then(Object::as_array)
        else {
            continue;
        };
        for field in source_fields {
            let Some(field_id) = field.as_reference() else {
                continue;
            };
            match field_widget_coverage(view, source_index, field_id, &copied_annots) {
                WidgetCoverage::None => {}
                WidgetCoverage::Partial => report.form_fields_dropped += 1,
                WidgetCoverage::Full => {
                    if let Some(number) = copier.map_reference(view, source_index, field_id)? {
                        fields.push(Object::Reference(ObjId::new(number, 0)));
                        top_level.push((source_index, number));
                    }
                }
            }
        }
    }

    if fields.is_empty() {
        return Ok(());
    }
    copier.drain(sources)?;
    if options.rename_duplicate_fields {
        report.form_fields_renamed += rename_colliding_fields(copier, &top_level);
    }

    let mut acroform = Dict::new();
    acroform.insert(Name::from(b"Fields"), Object::Array(fields));
    if !default_resources.is_empty() {
        acroform.insert(Name::from(b"DR"), Object::Dict(default_resources));
    }
    if need_appearances {
        acroform.insert(Name::from(b"NeedAppearances"), Object::Boolean(true));
    }
    if sig_flags != 0 {
        acroform.insert(Name::from(b"SigFlags"), Object::Integer(sig_flags));
    }
    catalog.insert(Name::from(b"AcroForm"), Object::Dict(acroform));
    Ok(())
}

/// Whether a field's widget annotations are all on copied pages, some, or
/// none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WidgetCoverage {
    /// No widget of this field is on a copied page.
    None,
    /// Some are and some are not — the case pdfcer refuses.
    Partial,
    /// Every widget is on a copied page (including the degenerate case
    /// of a field with no widgets at all, which is a pure container).
    Full,
}

/// Classify one field's widget coverage by walking its `/Kids`.
///
/// §12.7.3.1 allows a field's widget to be merged into the field
/// dictionary itself (no `/Kids`), which is why the field id is tested
/// against the annotation set as well as its descendants.
fn field_widget_coverage(
    view: &DocumentView<'_>,
    source_index: usize,
    field_id: ObjId,
    copied_annots: &HashSet<(usize, ObjId)>,
) -> WidgetCoverage {
    let mut on_copied = 0usize;
    let mut off_copied = 0usize;
    let mut stack = vec![(field_id, 0usize)];
    let mut visited: HashSet<ObjId> = HashSet::new();

    while let Some((id, depth)) = stack.pop() {
        if depth > MAX_COPY_DEPTH || !visited.insert(id) {
            continue;
        }
        let Some(dict) = view.graph().resolved(id).as_dict() else {
            continue;
        };
        let kids = dict
            .get(b"Kids")
            .map(|o| view.graph().resolve(o))
            .and_then(Object::as_array);
        match kids {
            Some(kids) if !kids.is_empty() => {
                for kid in kids {
                    if let Some(kid_id) = kid.as_reference() {
                        stack.push((kid_id, depth + 1));
                    }
                }
            }
            // A leaf: either a merged field/widget or a widget kid.
            _ => {
                if copied_annots.contains(&(source_index, id)) {
                    on_copied += 1;
                } else if dict.contains_key(b"Rect") {
                    // Has a rectangle, so it IS a widget — just not one
                    // on a page being copied. A field with no `Rect`
                    // anywhere is a pure container and counts as neither.
                    off_copied += 1;
                }
            }
        }
    }

    match (on_copied, off_copied) {
        (0, 0) => WidgetCoverage::Full, // container-only field
        (0, _) => WidgetCoverage::None,
        (_, 0) => WidgetCoverage::Full,
        _ => WidgetCoverage::Partial,
    }
}

/// Give colliding top-level field names a per-source prefix, and return
/// how many were renamed.
///
/// Only *colliding* names are touched. Renaming every field would be
/// tidier to implement and much worse to use: a script that fills
/// `Invoice.Total` by name keeps working across a merge unless that name
/// genuinely appeared twice.
fn rename_colliding_fields(copier: &mut Copier, top_level: &[(usize, u32)]) -> usize {
    let mut by_name: HashMap<Vec<u8>, Vec<(usize, u32)>> = HashMap::new();
    for &(source_index, number) in top_level {
        let name = match copier.objects.get(&number).and_then(Object::as_dict) {
            Some(dict) => match dict.get(b"T") {
                Some(Object::String(bytes)) => bytes.clone(),
                _ => continue,
            },
            None => continue,
        };
        by_name
            .entry(name)
            .or_default()
            .push((source_index, number));
    }

    let mut renamed = 0usize;
    for (name, holders) in by_name {
        if holders.len() < 2 {
            continue;
        }
        for (source_index, number) in holders {
            let mut prefixed = format!("Doc{source_index}_").into_bytes();
            prefixed.extend_from_slice(&name);
            if let Some(Object::Dict(dict)) = copier.objects.get_mut(&number) {
                dict.insert(Name::from(b"T"), Object::String(prefixed));
                renamed += 1;
            }
        }
    }
    renamed
}

/// The deep-copy engine and output-object store.
///
/// Deliberately one type rather than a copier plus a builder: the
/// allocation of an output object number and the recording of its value
/// are the same event, and splitting them across two types would make
/// "allocated but never filled" representable.
pub struct Copier {
    /// Output objects by number. Always generation 0 — a freshly built
    /// document has no history to have incremented one.
    objects: BTreeMap<u32, Object>,
    /// Concatenated raw stream payloads; every copied stream's span
    /// indexes into this.
    staging: Vec<u8>,
    /// Next output object number. Starts at 1 — §7.5.4 reserves 0 for
    /// the free-list head.
    next: u32,
    /// `(source index, source id)` → output number.
    mapping: HashMap<(usize, ObjId), u32>,
    /// Objects allocated but not yet copied.
    queue: Vec<(usize, ObjId, u32)>,
    /// Page objects that must not be followed. See the module docs.
    barrier: HashSet<(usize, ObjId)>,
    /// References refused by the barrier, or pointing at nothing.
    dangling: usize,
    /// The output's `/Info` object, if one is being carried.
    info: Option<u32>,
}

impl Copier {
    /// A copier that refuses to follow references to `barrier` pages.
    fn new(barrier: HashSet<(usize, ObjId)>) -> Self {
        Self {
            objects: BTreeMap::new(),
            staging: Vec::new(),
            next: 1,
            mapping: HashMap::new(),
            queue: Vec::new(),
            barrier,
            dangling: 0,
            info: None,
        }
    }

    /// Allocate an output object number with no source object behind it
    /// (the catalog, the root `Pages` node, a generated outline item).
    pub fn reserve(&mut self) -> u32 {
        let number = self.next;
        self.next = self.next.saturating_add(1);
        number
    }

    /// Allocate an output number for a specific source object **without**
    /// queueing it for copying — the caller will build its value itself.
    fn reserve_for(&mut self, source_index: usize, id: ObjId) -> u32 {
        let number = self.reserve();
        self.mapping.insert((source_index, id), number);
        number
    }

    /// Record an output object's final value.
    pub fn store(&mut self, number: u32, value: Object) {
        self.objects.insert(number, value);
    }

    /// Map a source reference to an output number, queueing the object
    /// for copying if this is the first sighting.
    ///
    /// `Ok(None)` is the **barrier** answer: the reference points at a
    /// page that is not being copied, or at nothing at all. It is not an
    /// error — a link whose target left the document is a normal outcome
    /// of extraction, and the caller decides whether that means dropping
    /// a dictionary entry or nulling an array element.
    fn map_reference(
        &mut self,
        view: &DocumentView<'_>,
        source_index: usize,
        id: ObjId,
    ) -> Result<Option<u32>, PageOpError> {
        if let Some(existing) = self.mapping.get(&(source_index, id)) {
            return Ok(Some(*existing));
        }
        if self.barrier.contains(&(source_index, id)) || view.graph().value(id).is_none() {
            self.dangling += 1;
            return Ok(None);
        }
        if self.objects.len() + self.queue.len() >= MAX_COPIED_OBJECTS {
            return Err(PageOpError::ObjectLimit);
        }
        let number = self.reserve();
        self.mapping.insert((source_index, id), number);
        self.queue.push((source_index, id, number));
        Ok(Some(number))
    }

    /// Copy everything currently queued, and everything it reaches, until
    /// the queue is empty.
    ///
    /// A worklist rather than recursion: the object *graph* is arbitrarily
    /// large (a 5,000-page document's resource graph is deep and wide),
    /// and only the per-object *value tree* — which is small and depth-
    /// guarded — is walked recursively.
    fn drain(&mut self, sources: &[DocumentView<'_>]) -> Result<(), PageOpError> {
        while let Some((source_index, id, number)) = self.queue.pop() {
            let view = sources
                .get(source_index)
                .ok_or(PageOpError::SourceOutOfRange {
                    index: source_index,
                })?;
            let Some(value) = view.graph().value(id).cloned() else {
                self.store(number, Object::Null);
                continue;
            };
            let copied = self
                .copy_value(view, source_index, &value, 0)?
                .unwrap_or(Object::Null);
            self.store(number, copied);
        }
        Ok(())
    }

    /// Copy one value tree, rewriting every reference into the output's
    /// numbering.
    ///
    /// `Ok(None)` propagates a barrier hit to the caller; see
    /// [`Copier::map_reference`]. Exceeding [`MAX_COPY_DEPTH`] degrades
    /// that sub-tree to `null` and counts it, rather than failing the
    /// whole operation — a hostile 200-deep array should cost the
    /// operator one broken value, not the extraction.
    pub fn copy_value(
        &mut self,
        view: &DocumentView<'_>,
        source_index: usize,
        value: &Object,
        depth: usize,
    ) -> Result<Option<Object>, PageOpError> {
        if depth > MAX_COPY_DEPTH {
            self.dangling += 1;
            return Ok(Some(Object::Null));
        }
        Ok(match value {
            Object::Reference(id) => self
                .map_reference(view, source_index, *id)?
                .map(|number| Object::Reference(ObjId::new(number, 0))),
            Object::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    // A refused element refuses the whole array — see
                    // the module docs for why this beats nulling one
                    // slot of an explicit destination.
                    let Some(mapped) = self.copy_value(view, source_index, item, depth + 1)? else {
                        return Ok(None);
                    };
                    out.push(mapped);
                }
                Some(Object::Array(out))
            }
            Object::Dict(dict) => Some(Object::Dict(self.copy_dict(
                view,
                source_index,
                dict,
                depth,
            )?)),
            Object::Stream(stream) => {
                let dict = self.copy_dict(view, source_index, &stream.dict, depth)?;
                Some(Object::Stream(Stream {
                    dict,
                    data_span: self.stage(view, stream),
                }))
            }
            // Scalars carry no references and are copied by value.
            other => Some(other.clone()),
        })
    }

    /// Copy a dictionary, dropping entries whose value hit the barrier.
    ///
    /// Dropping rather than nulling: §7.3.7 makes a `null`-valued entry
    /// identical to an absent one, so the two are semantically the same —
    /// and an absent entry is the one that does not leave a visible
    /// `/Dest null` in the file for a reader to wonder about.
    fn copy_dict(
        &mut self,
        view: &DocumentView<'_>,
        source_index: usize,
        dict: &Dict,
        depth: usize,
    ) -> Result<Dict, PageOpError> {
        let mut out = Dict::new();
        for (key, value) in dict.iter() {
            if let Some(mapped) = self.copy_value(view, source_index, value, depth + 1)? {
                out.insert(key.clone(), mapped);
            }
        }
        Ok(out)
    }

    /// Append a stream's raw payload to the staging buffer and return its
    /// span there.
    ///
    /// The payload is **not decoded**: it is copied exactly as the source
    /// stored it, filters and all, so the copy is byte-identical rather
    /// than merely equivalent. A stream whose span does not lie in its
    /// own source buffer (a provenance bug) stages as empty, matching
    /// `crate::writer::serialize`'s documented degradation rule instead
    /// of introducing a second one.
    fn stage(&mut self, view: &DocumentView<'_>, stream: &Stream) -> ByteSpan {
        // `view.slice` rather than a raw `span.slice(view.bytes)`: the view
        // may be over an editing session, where the payload of an authored
        // appearance lives in the staging half of a
        // [`crate::view::StreamSource::Split`] and there is no single buffer
        // to index (decision 018 §4). Same degradation as before for an
        // unresolvable span — stage empty, per `crate::writer::serialize`'s
        // documented rule.
        let data = view.slice(stream.data_span).unwrap_or(&[]);
        let start = self.staging.len();
        self.staging.extend_from_slice(data);
        ByteSpan::new(start, data.len())
    }

    /// Serialize the assembled objects into a complete PDF file.
    ///
    /// A **classic** cross-reference table (§7.5.4), not a cross-reference
    /// stream. R33's never-normalize rule does not apply — there is no
    /// base file whose form must be preserved — so the choice is free, and
    /// a classic table is readable by every PDF consumer ever shipped
    /// while a stream requires 1.5. The output declares whatever version
    /// its sources did, so emitting a 1.5-only structure into a file
    /// claiming 1.4 is a trap this avoids by construction.
    fn finish(mut self, catalog_num: u32, version: PdfVersion) -> Vec<u8> {
        let mut out = format!("%PDF-{version}\n").into_bytes();
        // §7.5.2: a file containing binary data should carry a comment
        // line with at least four bytes >= 128, so transfer tools treat
        // it as binary. Any copied stream makes that true of this file.
        out.extend_from_slice(b"%\xE2\xE3\xCF\xD3\n");

        let body_start = out.len();
        let mut entries: BTreeMap<u32, crate::xref::XrefEntry> = BTreeMap::new();
        entries.insert(
            0,
            crate::xref::XrefEntry::Free {
                next_free: 0,
                generation: 65_535,
            },
        );
        let staging = std::mem::take(&mut self.staging);
        for (number, value) in &self.objects {
            let id = ObjId::new(*number, 0);
            entries.insert(
                *number,
                crate::xref::XrefEntry::InUse {
                    offset: out.len() as u64,
                    generation: 0,
                },
            );
            serialize::write_indirect(&mut out, id, value, &staging, &IdentityEncoder);
        }
        let body_end = out.len();
        let highest = self.objects.keys().copied().max().unwrap_or(0);
        // §7.5.4: one entry per object number from 0 to the maximum. Any
        // number reserved and never stored gets the "detached" free form
        // §7.5.4 explicitly permits.
        for number in 0..=highest {
            entries
                .entry(number)
                .or_insert(crate::xref::XrefEntry::Free {
                    next_free: 0,
                    generation: 65_535,
                });
        }

        let mut trailer = Dict::new();
        trailer.insert(
            Name::from(b"Size"),
            Object::Integer(i64::from(highest).saturating_add(1)),
        );
        trailer.insert(
            Name::from(b"Root"),
            Object::Reference(ObjId::new(catalog_num, 0)),
        );
        if let Some(info) = self.info {
            trailer.insert(Name::from(b"Info"), Object::Reference(ObjId::new(info, 0)));
        }
        // §14.4 — see the module docs on why a NEW document gets an /ID
        // when R39 forbids synthesising one for an existing file. Both
        // elements derive from the body bytes with distinct domain
        // separators, so they differ from each other and from any other
        // document's, deterministically and with no clock involved.
        let body = out.get(body_start..body_end).unwrap_or(&[]);
        let permanent = fileid::changing_identifier(b"pdfcer/new-document/permanent", 0, body);
        let changing = fileid::changing_identifier(b"pdfcer/new-document/changing", 0, body);
        trailer.insert(
            Name::from(b"ID"),
            Object::Array(vec![
                Object::String(permanent.to_vec()),
                Object::String(changing.to_vec()),
            ]),
        );

        let section_offset = out.len() as u64;
        // The only failure `write_classic_table` reports is an offset past
        // 9,999,999,999 (§7.5.4's ten-digit field) or a type-2 entry,
        // neither of which this builder can produce: it emits no
        // compressed objects, and a 10 GB output would have exhausted
        // memory long before. Degrading to a table-less file would produce
        // an unopenable PDF, so the result is deliberately swallowed here
        // and the (unreachable) case leaves the section empty rather than
        // panicking, per the crate's panic-free policy.
        //
        // The two §7.5 end-of-line knobs (`EOL-A1`/`EOL-A2`, R169) are at
        // their **defaults** here, not at the operator's persisted values,
        // and that is a stated limitation rather than an oversight: this
        // builder is reached through `pageops::extract_with` and friends,
        // whose parameter is a `SeparationPolicy`, not a `SaveOptions`.
        // Carrying the writer options down to it is a separate change with
        // its own call-site churn. The consequence is narrow and worth
        // naming: a document pdfcer ASSEMBLES (page extraction, split) ends
        // with `SP LF` entries and a trailing `LF` even for an operator who
        // set otherwise, whereas a document pdfcer SAVES honours the
        // setting. Both are conforming; they are simply not identical.
        let _ = xref_out::write_classic_table(&mut out, &entries, XrefEntryEol::default());
        xref_out::write_classic_tail(&mut out, &trailer, section_offset, TrailingEol::default());
        out
    }
}

//! # The document outline — bookmarks, read (ISO 32000-1 §12.3.3, §12.3.2)
//!
//! A PDF's *document outline* is the tree a viewer shows in its
//! bookmarks panel. This module turns the raw, pointer-linked structure
//! the file stores — `/Root /Outlines`, then `/First` / `/Next` /
//! `/Parent` chains of indirect references — into an owned
//! [`Vec<OutlineItem>`] with resolved titles, resolved destinations, and
//! a resolved **0-based page index** wherever the file makes one
//! reachable.
//!
//! It is a **reader**. Nothing here mutates a document, and nothing here
//! is on the round-trip path (`CLAUDE.md` rule 3): the outline is parsed
//! into a parallel value tree and the file's own objects are untouched.
//! Outline *authoring* and outline *carryover across page operations*
//! live elsewhere — see [`crate::pageops::outline`], which rebuilds an
//! outline for an assembled document and is a deliberately different job
//! with different simplifications.
//!
//! ## What the file actually stores, and why reading it is not trivial
//!
//! §12.3.3 stores the outline as a doubly-linked tree of indirect
//! references. Table 152 gives the **root** dictionary:
//!
//! | Key | Meaning |
//! |---|---|
//! | `/Type` | `/Outlines` (optional, but conventional) |
//! | `/First` / `/Last` | the first and last **top-level** items |
//! | `/Count` | the number of *visible* items at all levels |
//!
//! Table 153 gives each **item** dictionary: `/Title`, `/Parent`,
//! `/Prev`, `/Next`, `/First`, `/Last`, `/Count`, `/Dest`, `/A`, `/SE`,
//! `/C`, `/F`.
//!
//! Three properties of that encoding drive nearly every design decision
//! in this file:
//!
//! **1. There is no array anywhere.** A sibling list is a `/Next` chain
//! and a child list is `/First` plus a `/Next` chain. Nothing bounds
//! either. A `/Next` that points back at an earlier sibling is a
//! perfectly well-formed *file* — the syntax is valid, the references
//! resolve — and it describes an infinite list. A reader that follows
//! the chain until it ends never returns. See
//! [`MAX_OUTLINE_DEPTH`] and the cycle guard below; this is requirement
//! (4) of the module's brief and is treated as a correctness property,
//! not a hardening nicety. **A PDF reader that hangs on a bad outline is
//! worse than one that reports a truncated tree.**
//!
//! **2. `/Count` is not a child count.** This is the single easiest
//! thing to get wrong in §12.3.3, because the key is named `Count` and
//! sits next to `/First` and `/Last`. Its **sign** carries the item's
//! open/closed state and its **magnitude** counts *visible descendants
//! at all levels* — not immediate children. An item with two children,
//! each with three children of their own, all expanded, has `/Count 8`.
//! The same item collapsed has `/Count -2` (§12.3.3: for a closed item
//! the magnitude is the number of descendants that *would* become
//! visible if it were reopened, i.e. its immediate children). pdfcer
//! therefore reads **only the sign** for structure, records the declared
//! magnitude verbatim in [`OutlineItem::declared_count`] for
//! diagnostics, and derives the real child count from the traversal. The
//! fixture `basic-tree.pdf` exists to pin exactly this: it declares
//! `/Count 9` on an item with two children, so a reader that trusts the
//! magnitude fails visibly.
//!
//! **3. A destination is four different things.** §12.3.2 lets an item
//! reach a page by an explicit array, by a name resolved through either
//! of two catalog namespaces, or by an action dictionary that itself
//! carries any of those. All four are handled here; see
//! [`Destination`].
//!
//! ## Contract
//!
//! - **Infallible.** [`read_outline`] and [`parse_outline`] return a
//!   tree, never a `Result`. Malformed input yields a *partial* tree
//!   plus a populated [`OutlineDiagnostics`]. There is no input that
//!   makes them panic, abort, recurse without bound, or loop — that is
//!   the crate-wide panic-free policy (`lib.rs`'s `deny(unwrap_used,
//!   expect_used, panic, indexing_slicing)`) applied to a structure that
//!   is unusually easy to weaponise.
//! - **Bounded.** At most [`MAX_OUTLINE_ITEMS`] items are read, at most
//!   [`MAX_OUTLINE_DEPTH`] levels deep, and no object is visited twice.
//!   Every one of those three limits, when it bites, sets a flag on
//!   [`OutlineDiagnostics`]. A truncated tree is never silently
//!   presented as a complete one — that is `CLAUDE.md` rule 4
//!   (*fuzzy, never sneaky*) applied to a structural inference.
//! - **Nothing is dropped silently.** An item whose destination cannot
//!   be resolved keeps its place in the tree with the most specific
//!   [`Destination`] variant the file supports — an unresolvable name
//!   stays a [`Destination::Named`], a page object that is not in the
//!   page tree becomes a [`Destination::UnmappedPage`] carrying the
//!   object id that failed. Requirement (1) of the brief: *a destination
//!   that names a page object not in the tree is a real corruption case
//!   — surface it, do not silently drop the bookmark.*
//!
//! ## Relationship to [`crate::pageops::references`]
//!
//! `pageops::references::DestinationResolver` already resolves a
//! destination to a **page object id**, for the delete/extract dangling
//! census. It is deliberately *not* reused here, and that is a known,
//! recorded duplication rather than an oversight:
//!
//! `DestinationResolver` answers "*which page?*" and discards the view
//! parameters on the way. This module needs "*which page, and looking at
//! it how?*" — `/XYZ`'s left/top/zoom, `/FitR`'s rectangle — because a
//! bookmarks panel that navigates to the right page at the wrong zoom is
//! a visible defect. For a destination reached **by name** the view
//! parameters live in the name tree's value, which `DestinationResolver`
//! flattens into a private map with no accessor. Getting at them means
//! either flattening the name tree again (what this module does) or
//! adding a lookup accessor to `references.rs`.
//!
//! **The accessor is the better end state** — one flatten, one set of
//! semantics — and the refactor is small: expose
//! `DestinationResolver::lookup(&self, key: &[u8]) -> Option<&Object>`
//! and have [`NamedDestinations`] become a thin wrapper over it. It is
//! not done here only because this module was written under a file-
//! ownership constraint that put `references.rs` off limits. Until then,
//! the two flatteners are kept **behaviourally identical on purpose**,
//! including the collision rule (see [`NamedDestinations::new`]), so
//! that a document cannot resolve one way for the bookmarks panel and a
//! different way for the delete census.
//!
//! ## Spec sources
//!
//! **Every clause and table number here is ISO 32000-1:2008 (PDF 1.7).**
//! That is worth stating rather than assuming, because ISO 32000-2
//! renumbers the two tables this module depends on most, and in opposite
//! directions: 1.7's Table 153 (outline item) becomes 2.0's Table 151,
//! while 1.7's Table 151 (destination syntax) becomes 2.0's Table 149. A
//! citation without an edition is a citation that will eventually be
//! read against the wrong table.
//!
//! - `iso32000__s__12.3.3.md` — §12.3.3, Tables 152/153/154: the outline
//!   model, `/Count`'s two different meanings, the flag bits, and the
//!   worked example in Annex H.6 that pins the `/Count` arithmetic.
//! - `iso32000__s__12.3.2.md` — §12.3.2, Table 151: destination syntax
//!   for `/XYZ`, `/Fit`, `/FitH`, `/FitV`, `/FitR`, `/FitB`, `/FitBH`,
//!   `/FitBV`; §12.3.2.2 explicit destinations; §12.3.2.3 named
//!   destinations and the two namespaces.
//! - `iso32000__s__12.6.4.2.md` — §12.6.4.2/.3/.4: the `/GoTo`,
//!   `/GoToR` and `/GoToE` actions, Tables 199/200/201, and Table 198's
//!   registry of the twenty action types. (Note the standard's own
//!   erratum, recorded in that RAG entry: Table 193's `/S` row misdirects
//!   to "Table 194"; the registry is **Table 198**.)
//! - `iso32000__s__7.9.6.md` — name trees: `/Names` as an alternating
//!   key/value array, `/Kids` for interior nodes, and byte-by-byte key
//!   comparison.
//! - `iso32000__s__7.11.md` — file specifications, for `/GoToR`'s `/F`.
//! - `iso32000__s__7.9.2.md` — `/Title` is a *text string*: UTF-16BE
//!   when it starts `FE FF`, PDFDocEncoding (Annex D.3) otherwise. Table
//!   35 and §7.9.2.2 name "bookmark names" explicitly, so this is
//!   stated rather than inferred.
//! - `iso32000__s__7.3.10.md` — a dangling reference resolves to `null`
//!   and *"shall not be considered an error"*, which is why an
//!   unreadable link truncates a chain rather than failing the parse.
//!
//! ## Spec ambiguities this module resolves by policy
//!
//! Three questions the standard does not answer. Each is resolved here
//! the same way the rest of the crate resolves it, and each is
//! **disclosed** rather than silently decided — which is the shape
//! `CLAUDE.md` rule 4 asks for and the shape the PDF_Spec RAG's
//! ambiguity register is built to track.
//!
//! - **`OL-A1` — an item carrying both `/Dest` and `/A`.** Table 153
//!   marks them mutually exclusive in *both* rows and then says nothing
//!   about a file that carries both. See
//!   [`resolve_item_destination`]; counted in
//!   [`OutlineDiagnostics::dest_and_action_both_present`].
//! - **`DEST-A1` — a destination name defined in the "wrong"
//!   namespace.** §12.3.2.3 gives no precedence or fallback rule between
//!   the two, and the only discriminator it offers is the *type* of the
//!   reference value (a name object means the PDF 1.1 dictionary, a
//!   string means the PDF 1.2 tree). pdfcer searches both regardless of
//!   type, and counts the mismatches in
//!   [`OutlineDiagnostics::cross_namespace_resolutions`]. See
//!   [`NamedDestinations::new`].
//! - **`EF-A3` — `/UF` versus `/F` on a file specification.** No
//!   precedence rule exists; the RAG's derived reading prefers `/UF` for
//!   any textual use and recommends the choice be a setting. See
//!   [`file_spec_bytes`].
//!
//! ## Behavioural reference
//!
//! `D:\Dev\Rag-Specialized\Acrobat_Features\bookmarks__destinations_and_navigation.md`
//! records what Acrobat Reader honours, and pins three expectations this
//! module is built to meet: the `must_have` view types are `/XYZ`,
//! `/Fit`, `/FitH`, `/FitV` and `/FitR` in **both** direct and named
//! form; open/closed state comes from the `/Count` **sign**; and a
//! bookmark may carry any action type, not just navigation — for which
//! that RAG recommends the *recognize-and-disclose-never-execute*
//! posture already established for form-field JavaScript. This module
//! implements the "recognize and disclose" half:
//! [`Destination::NonNavigation`] names the action's `/S` and evaluates
//! nothing.

use std::collections::{HashMap, HashSet};

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::{PageTreeError, page_slots};
use crate::pageops::references::{MAX_NAME_TREE_DEPTH, MAX_NAME_TREE_NODES, MAX_OUTLINE_ITEMS};
use crate::textstring::decode_text_string;

/// Maximum outline nesting read (pdfcer policy, `ARCHITECTURE.md` §10).
///
/// §12.3.3 imposes no nesting limit, and Annex C's implementation limits
/// do not mention outlines either, so this is **pdfcer policy on
/// untrusted input** rather than a spec constant. The value matches
/// [`crate::pageops::outline`]'s own `MAX_OUTLINE_DEPTH` deliberately:
/// a tree the reader can display but the assembler would silently
/// flatten is a worse failure than either limit alone.
///
/// Thirty-two levels is far past anything a document produces on
/// purpose — a technical manual with parts, chapters, sections,
/// subsections and figures reaches five. Exceeding it sets
/// [`OutlineDiagnostics::depth_truncations`]; it never panics and never
/// drops the ancestors that were read.
pub const MAX_OUTLINE_DEPTH: usize = 32;

/// How many variants [`Destination`] has, pinned so that adding one cannot
/// ship without the consuming shells being told.
///
/// Not part of the public API surface in any meaningful sense — it exists
/// for `variant_count_is_pinned_so_a_new_one_cannot_ship_unannounced`,
/// whose doc comment explains why a tripwire is the only mechanism
/// available for this particular hazard.
#[cfg(test)]
const DESTINATION_VARIANTS: usize = 5;

/// Maximum hops followed while chasing a destination through names and
/// `/D` wrappers before giving up.
///
/// §12.3.2.3 does not forbid a named destination whose value is another
/// name, so the resolution is a small graph walk and needs its own
/// bound. Matches the bound
/// [`crate::pageops::references::DestinationResolver`] uses for the same
/// walk, for the consistency reason given in the module docs.
const MAX_DEST_HOPS: usize = 8;

// ---------------------------------------------------------------------
// Public data model
// ---------------------------------------------------------------------

/// One entry in the document outline, with its subtree.
///
/// Owned rather than borrowed from the graph: a bookmarks panel outlives
/// any single borrow of the document, and an outline is small enough
/// (thousands of short strings) that copying it is cheaper than
/// threading a lifetime through the GUI.
///
/// `#[non_exhaustive]` because Table 153 has entries this Pass does not
/// read yet — `/SE` (the structure element a bookmark corresponds to,
/// needed for tagged-PDF navigation) is the obvious next one — and
/// adding a field must not be a breaking change.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct OutlineItem {
    /// The object id of this item's own dictionary.
    ///
    /// Carried because identity is what a GUI needs and the tree cannot
    /// otherwise supply: selecting a bookmark, scrolling back to it
    /// after a reload, or (later) editing it all key off the object, not
    /// off a path through the tree that any edit invalidates.
    pub id: ObjId,
    /// `/Title` decoded as a §7.9.2 text string.
    ///
    /// Empty when `/Title` is absent or is not a string — both are
    /// malformed (Table 153 marks `/Title` **required**) and both are
    /// counted in [`OutlineDiagnostics::titles_unreadable`]. An empty
    /// title is not itself an error: a file may legitimately carry one.
    pub title: String,
    /// `false` when at least one byte of `/Title` could not be decoded
    /// and U+FFFD was substituted.
    ///
    /// Surfaced rather than folded into `title` because of `CLAUDE.md`
    /// rule 4: a title pdfcer partly guessed at must be visibly distinct
    /// from one it read exactly. See
    /// [`crate::textstring::DecodedText::exact`] for the three ways this
    /// goes false.
    pub title_exact: bool,
    /// Where this bookmark navigates, as far as the file makes knowable.
    ///
    /// `None` means the item carries neither `/Dest` nor `/A` — a legal
    /// and common shape for a pure grouping entry ("Part II") that only
    /// exists to hold children.
    pub destination: Option<Destination>,
    /// This item's children, in document order.
    pub children: Vec<OutlineItem>,
    /// Whether this item's children are shown expanded by default.
    ///
    /// **Derived from the sign of `/Count` and nothing else** (§12.3.3;
    /// see the module docs). Defaults to `false` when `/Count` is absent
    /// — see [`OutlineDiagnostics::open_state_defaulted`] for why closed
    /// is the safe default.
    pub open: bool,
    /// Nesting depth: `0` for a top-level item, `1` for its children,
    /// and so on.
    ///
    /// Redundant with the tree's shape, and carried anyway because every
    /// consumer that renders a flat list with indentation would
    /// otherwise recompute it, and one of them would eventually
    /// recompute it wrong.
    pub level: usize,
    /// The raw `/Count` integer as the file declared it, un-interpreted.
    ///
    /// Kept **verbatim** so a diagnostic can compare the file's claim
    /// against the traversal's finding without re-reading the document.
    /// `None` when `/Count` is absent or is not an integer. Do not use
    /// this to size anything — see the module docs on why the magnitude
    /// is not a child count.
    pub declared_count: Option<i64>,
    /// `/C` — the item's display colour in DeviceRGB, components in
    /// `0.0..=1.0` (Table 153, PDF 1.4).
    ///
    /// `None` when absent or not an array of three numbers. Not clamped:
    /// an out-of-range component is a defect the caller should see
    /// rather than one this module launders.
    pub color: Option<[f64; 3]>,
    /// `/F` — the item's display style flags as the file declared them
    /// (Table 153 declares the key, **Table 154** defines the bits; PDF
    /// 1.4, default 0).
    ///
    /// Stored raw rather than as booleans because the field is a bit
    /// field with room to grow and pdfcer should not silently discard
    /// bits it does not recognise. Use [`OutlineItem::is_italic`] and
    /// [`OutlineItem::is_bold`] for the two defined bits.
    pub style_flags: Option<i64>,
}

impl OutlineItem {
    /// `/F` bit **position 1** — italic (Table 154).
    ///
    /// Recorded as the mask `1 << (1 - 1)`. Table 154 numbers bits from
    /// 1 at the low-order end.
    const FLAG_ITALIC: i64 = 1;
    /// `/F` bit **position 2** — bold (Table 154).
    ///
    /// **The intuitive order is wrong** — the PDF_Spec RAG files this as
    /// `OL-T2` precisely because every reader's instinct is that bold
    /// comes first. Italic is bit 1; bold is bit 2. Getting these
    /// backwards produces a bookmarks panel that is *almost* right,
    /// which is the kind of defect that ships.
    const FLAG_BOLD: i64 = 2;

    /// Whether `/F` asks for an italic title.
    ///
    /// A named accessor rather than exposing the constant, so that a
    /// caller cannot accidentally test bit *value* 1 against bit
    /// *position* 1 — the classic off-by-one in PDF flag words, where
    /// the spec numbers positions from 1 and the arithmetic needs
    /// `1 << (position - 1)`.
    #[must_use]
    pub const fn is_italic(&self) -> bool {
        match self.style_flags {
            Some(flags) => flags & Self::FLAG_ITALIC != 0,
            None => false,
        }
    }

    /// Whether `/F` asks for a bold title.
    #[must_use]
    pub const fn is_bold(&self) -> bool {
        match self.style_flags {
            Some(flags) => flags & Self::FLAG_BOLD != 0,
            None => false,
        }
    }

    /// The 0-based page index this item navigates to within *this*
    /// document, if it reaches one.
    ///
    /// The convenience every caller wants and none should write twice:
    /// "jump to this bookmark" is the whole feature, and it needs
    /// exactly this number. Deliberately `None` for a remote
    /// (`/GoToR`) destination — that page index belongs to a different
    /// file and returning it here would let a caller scroll this
    /// document to a page the bookmark never meant.
    #[must_use]
    pub const fn page_index(&self) -> Option<usize> {
        match self.destination {
            Some(Destination::Page { page_index, .. }) => Some(page_index),
            _ => None,
        }
    }
}

/// Where a bookmark — or a `/Link`, or a `/Widget` pushbutton — points
/// (§12.3.2, §12.6.4.2, §12.6.4.3).
///
/// The variants are ordered by how much pdfcer could determine, from
/// "fully resolved" to "recognised but not a navigation at all". Every
/// variant except [`Destination::Page`] represents something the
/// operator may need told about, which is why none of them is folded
/// into a `None`.
///
/// # ★★ ADDING A VARIANT IS A CONSUMER-VISIBLE EVENT — announce it
///
/// This is `#[non_exhaustive]`, so a downstream `match` **must** carry a
/// catch-all, and that catch-all is a sentence describing whatever lands
/// in it. A new variant therefore does not break a consumer's build — it
/// silently acquires that consumer's existing wording, which was written
/// about a different thing.
///
/// This is not hypothetical. `pdfcer-gui` reported it on 2026-09-01, in its
/// own words, after wiring [`crate::annot::page_link_destinations`]:
///
/// > *"our `match` needs a catch-all, and a catch-all is exactly the shape
/// > that produces the defect your reply warns about. Ours says 'this link
/// > has no destination at all' rather than navigating — but the next
/// > variant you add will silently land there and be described wrongly.
/// > Not a request; a note that the wildcard exists and that we would
/// > rather be told when a sixth variant ships than discover it as a
/// > mis-worded sentence."*
///
/// ⇒ **If you add a variant, say so on the feature-request channel**
/// (`D:\Dev\FeatureRequests\pdfce_FeatureRequests\`) in the same Pass.
/// `variant_count_is_pinned_so_a_new_one_cannot_ship_unannounced` in this
/// module's tests will fail first and repeat the instruction; it exists
/// only to make sure the obligation is *reached*, since nothing else in
/// this workspace can observe a downstream `match` arm.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Destination {
    /// Fully resolved: a 0-based page index in **this** document, plus
    /// the view to establish on arrival.
    Page {
        /// 0-based index into the page tree's document-order page list,
        /// as produced by [`crate::page_tree::page_slots`].
        page_index: usize,
        /// The Table 151 fit style and its parameters.
        view: DestView,
    },
    /// An explicit destination array that pdfcer could **not** map to a
    /// page index.
    ///
    /// This is requirement (1) of this module's brief made visible. It
    /// covers three genuinely different corruptions, which are
    /// distinguished by `page` and by
    /// [`OutlineDiagnostics::page_tree_error`]:
    ///
    /// - `page: Some(id)` and no page-tree error — the array named a
    ///   real object that is **not a page in the page tree**. A
    ///   destination left behind by a page delete looks exactly like
    ///   this.
    /// - `page: Some(id)` with a page-tree error — the object might well
    ///   be a page; pdfcer could not walk the tree to find out.
    /// - `page: None` — element 0 was absent, `null`, or not an indirect
    ///   reference, contrary to §12.3.2.2's *"the page shall be
    ///   specified by an indirect reference"*.
    UnmappedPage {
        /// The object the array named, when it named one at all.
        page: Option<ObjId>,
        /// The fit style, which is readable even when the page is not.
        view: DestView,
    },
    /// A named destination (§12.3.2.3) that neither catalog namespace
    /// defines.
    ///
    /// Kept rather than discarded — brief requirement (2) — because the
    /// name is the only evidence of what the bookmark was for, and
    /// because a name that resolves in the producing workflow but not in
    /// this file is a repair case, not a non-entry.
    Named {
        /// The raw key bytes, exactly as the file spelled them.
        ///
        /// Bytes rather than a `String` because §7.9.6 name-tree keys
        /// are *strings* with no declared encoding — they are compared
        /// byte-wise, not textually, and round-tripping them through
        /// UTF-8 would corrupt the ones that are not text. Use
        /// [`Destination::name_lossy`] for display.
        name: Vec<u8>,
    },
    /// A `/GoToR` action (§12.6.4.3): a destination in **another** file.
    ///
    /// Never resolved to a page index of this document, by design — see
    /// [`OutlineItem::page_index`].
    Remote {
        /// The `/F` file specification, reduced to bytes for display.
        /// `None` when absent or in a shape this module does not read
        /// (see [`file_spec_bytes`]).
        file: Option<Vec<u8>>,
        /// How the remote destination names its page.
        target: RemoteTarget,
        /// The fit style to establish in the remote file.
        view: DestView,
        /// `/NewWindow`, when stated. `None` is meaningful: §12.6.4.3
        /// makes the entry optional and leaves the choice to the viewer,
        /// so absent is *not* the same as `false`.
        new_window: Option<bool>,
    },
    /// An action that is not a page navigation: `/URI`, `/Launch`,
    /// `/JavaScript`, `/Named`, `/Thread`, and anything else §12.6 or a
    /// later extension defines.
    ///
    /// **Recognised and disclosed, never executed.** That posture is the
    /// one `Acrobat_Features/bookmarks__destinations_and_navigation.md`
    /// recommends for bookmark actions by analogy with pdfcer's existing
    /// form-field JavaScript handling, and this variant is what makes
    /// disclosure possible: a UI can say *"this bookmark runs a script"*
    /// instead of appearing to be a broken bookmark.
    NonNavigation {
        /// The action's `/S` subtype. `None` when the `/A` value was not
        /// a dictionary, or carried no readable `/S` — malformed either
        /// way, and counted in
        /// [`OutlineDiagnostics::unreadable_actions`].
        action: Option<Name>,
    },
}

impl Destination {
    /// A named destination's key rendered for display, with invalid
    /// UTF-8 replaced.
    ///
    /// Lossy on purpose and named so: the exact bytes stay available in
    /// [`Destination::Named::name`] for anything that must match or
    /// rewrite them, and a panel that cannot show a slightly-mangled
    /// name is worse than one that shows it with U+FFFD in it — the same
    /// judgement [`crate::textstring::decode_text_string`] makes for
    /// titles.
    #[must_use]
    pub fn name_lossy(&self) -> Option<String> {
        match self {
            Self::Named { name } => Some(String::from_utf8_lossy(name).into_owned()),
            _ => None,
        }
    }
}

/// How a `/GoToR` destination names its page in the remote file
/// (§12.6.4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RemoteTarget {
    /// An integer page number in the remote file, **0-based**, carried
    /// verbatim.
    ///
    /// A remote destination cannot use an indirect reference — the
    /// target file's objects are not available to the producer — so
    /// §12.3.2.2 permits an integer here instead, and *only* here:
    /// *"No page object can be specified for a destination associated
    /// with a remote go-to action … the `page` parameter specifies an
    /// integer page number within the remote document."*
    ///
    /// **The basing is stated in an unexpected place.** *"The first page
    /// shall be numbered 0"* appears in **Table 200's `/D` row in
    /// §12.6.4.3** — not in §12.3.2.2, which describes the integer form
    /// and never says which end it counts from. A reader built from
    /// clause 12.3 alone therefore has no answer, and the standard is
    /// not even self-consistent across sites: `/PrintPageRange` (Table
    /// 150) is **1-based**. This is why the type carries the raw `i64`
    /// and an explicitly-named [`RemoteTarget::page_index`] accessor,
    /// rather than a bare `usize` that would silently absorb the
    /// question.
    PageNumber(i64),
    /// A named destination in the remote file.
    ///
    /// Unresolvable from here by construction: the names live in the
    /// other file's catalog.
    Named(Vec<u8>),
    /// `/D` was absent, or in no shape this module recognises.
    Unknown,
}

impl RemoteTarget {
    /// The 0-based page index in the **remote** file, when this target
    /// is a page number that can be one.
    ///
    /// Returns `None` for a negative page number as well as for the
    /// non-numeric variants. A negative value is malformed — Table 200
    /// numbers from 0 — and clamping it to 0 would silently turn a
    /// corrupt bookmark into one that convincingly opens the wrong
    /// file's first page.
    ///
    /// The name says *remote*, and that is load-bearing: nothing here
    /// indexes into the current document. See
    /// [`OutlineItem::page_index`], which deliberately returns `None`
    /// for every remote destination.
    #[must_use]
    pub const fn page_index(&self) -> Option<usize> {
        match *self {
            Self::PageNumber(number) if number >= 0 => Some(number as usize),
            _ => None,
        }
    }
}

/// A Table 151 destination fit style and its parameters (§12.3.2).
///
/// Coordinates are in the target page's **user space**, unmodified.
/// pdfcer does not apply `/CropBox`, `/Rotate` or any viewer-side
/// clamping here — that is the display layer's job, and doing it during
/// parsing would make the parsed value disagree with the file.
///
/// Every numeric parameter is an `Option<f64>`, including `/FitR`'s
/// four. For `/XYZ` that models the spec directly: a `null` parameter
/// means *retain the current value*, which is a real, distinct state
/// from "zero". For the others it models **malformation** — §12.3.2
/// requires their parameters, so a `None` there means the array was
/// short or carried a non-number, and
/// [`OutlineDiagnostics::malformed_views`] counts it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DestView {
    /// `[page /XYZ left top zoom]` — position the given point at the
    /// upper-left of the window at the given zoom. Any parameter may be
    /// `null`, meaning "leave that aspect of the current view alone".
    Xyz {
        /// Horizontal coordinate of the point to place at the window's
        /// left edge.
        left: Option<f64>,
        /// Vertical coordinate of the point to place at the window's
        /// top edge.
        top: Option<f64>,
        /// Magnification factor. See the gap note on
        /// [`DestView::zoom_is_retain`] for the zero case.
        zoom: Option<f64>,
    },
    /// `[page /Fit]` — fit the whole page in the window.
    Fit,
    /// `[page /FitH top]` — fit the page **width**, with `top` at the
    /// window's top edge.
    FitH {
        /// Vertical coordinate of the window's top edge.
        top: Option<f64>,
    },
    /// `[page /FitV left]` — fit the page **height**, with `left` at the
    /// window's left edge.
    FitV {
        /// Horizontal coordinate of the window's left edge.
        left: Option<f64>,
    },
    /// `[page /FitR left bottom right top]` — fit the given rectangle
    /// entirely in the window. If the horizontal and vertical fit
    /// factors differ, the **smaller** is used and the rectangle is
    /// centred in the other dimension.
    ///
    /// **This is not a `/Rect`, and must not be parsed as one.** The
    /// PDF_Spec RAG files that trap as `DEST-T1`. The permutation
    /// happens to match `[llx lly urx ury]`, so a `/Rect` parser looks
    /// like it fits — but §7.9.5 says a `/Rect` *"shall be normalised"*
    /// while Table 151 imposes **no** normalisation on `/FitR`.
    /// Reusing a normalising rectangle parser here would silently
    /// reorder a destination the producer wrote deliberately, so do not
    /// assume `left < right` or `bottom < top`.
    ///
    /// The four parameters are read **positionally in the order the
    /// array gives them**, confirmed verbatim against Table 151's
    /// `/FitR` row (and unchanged in ISO 32000-2's Table 149). See
    /// [`DestView::rect`] for the assembled form.
    FitR {
        /// Left edge of the rectangle to fit.
        left: Option<f64>,
        /// Bottom edge.
        bottom: Option<f64>,
        /// Right edge.
        right: Option<f64>,
        /// Top edge.
        top: Option<f64>,
    },
    /// `[page /FitB]` — fit the page's **bounding box** in the window.
    FitB,
    /// `[page /FitBH top]` — fit the bounding box's width.
    FitBH {
        /// Vertical coordinate of the window's top edge.
        top: Option<f64>,
    },
    /// `[page /FitBV left]` — fit the bounding box's height.
    FitBV {
        /// Horizontal coordinate of the window's left edge.
        left: Option<f64>,
    },
    /// The array named a fit style pdfcer does not know.
    ///
    /// Preserved by name rather than collapsed to [`DestView::Absent`]
    /// so that an extension's destination type shows up as *"pdfcer does
    /// not implement `/FitSomething`"* rather than as damage. Counted in
    /// [`OutlineDiagnostics::unknown_views`].
    Unknown {
        /// The unrecognised fit name, verbatim.
        fit: Name,
    },
    /// The array carried no fit-style name at all — it was empty, held
    /// only a page, or its second element was not a name.
    ///
    /// Malformed: §12.3.2 requires the style. Counted in
    /// [`OutlineDiagnostics::malformed_views`].
    Absent,
}

impl DestView {
    /// `/FitR`'s rectangle as `[left, bottom, right, top]`, when all four
    /// parameters were present and numeric.
    ///
    /// Returns `None` for every other variant *and* for a `/FitR` whose
    /// array was short — the caller that wants to draw or scroll to the
    /// rectangle needs all four or none of them, and forcing that
    /// through one accessor is what stops a partial rectangle being
    /// completed with plausible defaults.
    #[must_use]
    pub const fn rect(&self) -> Option<[f64; 4]> {
        match *self {
            Self::FitR {
                left: Some(l),
                bottom: Some(b),
                right: Some(r),
                top: Some(t),
            } => Some([l, b, r, t]),
            _ => None,
        }
    }

    /// Whether an `/XYZ` destination asks the viewer to **retain** the
    /// current zoom.
    ///
    /// True when `zoom` is `null`, and equally true when it is literal
    /// `0` — Table 151 states that equivalence verbatim, so this is a
    /// spec rule rather than a viewer convention.
    ///
    /// **The zero-means-null rule is `zoom`-only.** It does *not* extend
    /// to `left` or `top`, where `0` is a genuine coordinate: the
    /// standard's own worked example uses `/XYZ 0 792 0` to mean "top of
    /// the page, retain zoom", with a literal `left` of 0 and a
    /// retain-zoom of 0 side by side in the same array. A reader that
    /// generalised the rule to all three parameters would break exactly
    /// that example. Returns `false` for every non-`/XYZ` variant, which
    /// have no zoom to retain.
    #[must_use]
    pub fn zoom_is_retain(&self) -> bool {
        match *self {
            Self::Xyz { zoom, .. } => match zoom {
                None => true,
                Some(value) => value == 0.0,
            },
            _ => false,
        }
    }
}

/// What the outline read could not do, counted.
///
/// Every field is something a front end can put in a sentence, and every
/// non-zero field means the returned tree is a *reading* of the document
/// rather than a transcription of it. That distinction is `CLAUDE.md`
/// rule 4 applied to structure: a truncated or partly-guessed outline
/// must be visibly so.
///
/// Counted rather than itemised, following
/// [`crate::pageops::references::DanglingReport`]'s precedent: an
/// outline with 300 broken destinations should say "300", and the list
/// is what a future repair flow would need rather than something to
/// carry unused now.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct OutlineDiagnostics {
    /// Total items read, at every level. The tree's real size, as
    /// opposed to anything `/Count` claimed.
    pub items: usize,
    /// Deepest level reached, `0` for a flat outline.
    pub max_depth: usize,
    /// Whether [`MAX_OUTLINE_ITEMS`] was hit and the read stopped early.
    ///
    /// When true the tree is **incomplete** and must be presented as
    /// such.
    pub item_budget_exhausted: bool,
    /// How many subtrees were cut off at [`MAX_OUTLINE_DEPTH`].
    ///
    /// Each one is an item that has children in the file and none in the
    /// returned tree.
    pub depth_truncations: usize,
    /// How many links were refused because they pointed at an object
    /// already visited — the cycle guard firing.
    ///
    /// Non-zero means the file's outline contains a loop. Requirement
    /// (4) of the brief: bounded traversal, **reported**.
    pub cycles_broken: usize,
    /// How many `/First` or `/Next` values were present but not indirect
    /// references, truncating a chain.
    pub non_reference_links: usize,
    /// How many outline objects did not resolve to a dictionary — a
    /// dangling reference (§7.3.10) or an object of the wrong type.
    pub unreadable_items: usize,
    /// How many items had an absent or non-string `/Title`, which Table
    /// 153 marks required.
    pub titles_unreadable: usize,
    /// How many titles decoded with at least one U+FFFD substitution.
    pub titles_inexact: usize,
    /// How many items with children carried no usable `/Count`, so their
    /// open/closed state was defaulted.
    ///
    /// A genuine conformance failure, not merely an inconvenience: Table
    /// 153 makes `/Count` *"required if the item has any descendants"*,
    /// and because there is **no `/Open` key** the sign of `/Count` is
    /// the only place the state can live. An item with children and no
    /// `/Count` has not stated whether it is open, and no amount of
    /// reading elsewhere in the file recovers it.
    ///
    /// **The default is closed.** A wrongly-closed node still shows that
    /// children exist — the twisty is drawn either way — and costs the
    /// operator one click. A wrongly-*open* node on a large damaged
    /// outline floods the panel with entries the author meant to hide,
    /// and there is no equally cheap recovery from that.
    pub open_state_defaulted: usize,
    /// How many items' `/Count` **magnitude** disagreed with the number
    /// of visible descendants the traversal actually found.
    ///
    /// The cross-check the PDF_Spec RAG recommends: build the tree from
    /// the linked list, take only the open/closed **bit** from `/Count`,
    /// and then use the magnitude solely to detect that the file is
    /// internally inconsistent. A non-zero value here almost always
    /// means the document was edited by a tool that moved bookmarks
    /// without recomputing the counts — worth telling an operator,
    /// never worth acting on.
    ///
    /// The magnitude counts **visible descendants at all levels, the
    /// item itself excluded**, and a *closed* child contributes exactly
    /// one (itself) and none of its own subtree. See
    /// [`Outline::visible_item_count`] for the arithmetic, which is
    /// verified against the standard's Annex H.6 worked example.
    ///
    /// **Zero when the tree was truncated**, because a truncated
    /// traversal cannot honestly disagree with anything. See
    /// [`Outline::items`]'s producer, [`read_outline`].
    pub count_disagreements: usize,
    /// Whether the **root** outline dictionary's `/Count` disagreed with
    /// the visible-item total.
    ///
    /// Separate from [`OutlineDiagnostics::count_disagreements`] because
    /// the root's `/Count` counts a **different quantity**: all visible
    /// items at every level *including the top-level items themselves*,
    /// where an item's `/Count` excludes itself. Conflating the two
    /// readings is, per the PDF_Spec RAG, "the single most likely defect
    /// in an outline reader".
    pub root_count_disagreement: bool,
    /// The root outline dictionary's declared `/Count` (Table 152),
    /// verbatim.
    ///
    /// `None` when absent, which Table 152 says *"shall be omitted if
    /// there are no open outline items"*. **Never infer "this document
    /// has no bookmarks" from that** — the RAG files the root's omission
    /// rule as `OL-A2` because it contradicts its own value definition
    /// for a flat outline, where the top-level items are always visible
    /// and yet no item is "open". Use [`Outline::items`] being empty.
    pub declared_root_count: Option<i64>,
    /// How many items carried **both** `/Dest` and `/A`, which §12.3.3
    /// forbids.
    ///
    /// See [`resolve_item_destination`] for which one wins and why.
    pub dest_and_action_both_present: usize,
    /// How many explicit destinations could not be mapped to a page
    /// index — the [`Destination::UnmappedPage`] count.
    pub unmapped_pages: usize,
    /// How many named destinations neither namespace defined — the
    /// [`Destination::Named`] count.
    pub unresolved_names: usize,
    /// How many destination names resolved in the namespace their
    /// **type** did not point at — spec ambiguity `DEST-A1`, disclosed.
    ///
    /// §12.3.2.3 offers exactly one discriminator between its two
    /// namespaces: a `/Dest` that is a **name object** belongs to the
    /// PDF 1.1 catalog `/Dests` dictionary, and one that is a **string**
    /// belongs to the PDF 1.2 `/Names → /Dests` tree. It states no
    /// precedence and no fallback for a file that puts the key in the
    /// other one.
    ///
    /// pdfcer searches both regardless of type, because a type-strict
    /// resolver fails on documents other readers open — but that
    /// leniency is a **choice pdfcer made where the standard was silent**,
    /// and rule 4 says a choice like that is disclosed rather than
    /// assumed. Non-zero here means "these bookmarks work in pdfcer
    /// because pdfcer was lenient", which is exactly what an operator
    /// preparing a file for a stricter reader needs to know.
    pub cross_namespace_resolutions: usize,
    /// How many destination arrays named a fit style pdfcer does not
    /// implement.
    pub unknown_views: usize,
    /// How many destination arrays were missing a required fit-style
    /// name or a required numeric parameter.
    pub malformed_views: usize,
    /// How many `/A` values were not a dictionary, or carried no
    /// readable `/S`.
    pub unreadable_actions: usize,
    /// How many named destinations the document defines across both
    /// namespaces.
    ///
    /// Context rather than a defect: "4 of this file's 900 bookmarks did
    /// not resolve" reads very differently from "4 did not resolve and
    /// the file defines no names at all", and the second case points at
    /// a lost `/Names` tree rather than at four bad bookmarks.
    pub named_destinations_defined: usize,
    /// Why the page tree could not be walked, when it could not be.
    ///
    /// When this is `Some`, **no** explicit destination could be mapped
    /// to an index, and every one of them is a
    /// [`Destination::UnmappedPage`] for that reason alone rather than
    /// because it was broken. A UI must say so — reporting "900 broken
    /// bookmarks" when the real fault is one unreadable page tree sends
    /// the operator after the wrong problem.
    pub page_tree_error: Option<PageTreeError>,
}

impl OutlineDiagnostics {
    /// Whether the returned tree is a complete transcription of the
    /// file's outline, with nothing truncated, defaulted or guessed.
    ///
    /// Deliberately strict: a single U+FFFD in one title makes this
    /// `false`. The point is to give a UI one cheap test for *"can I
    /// present this as simply the document's bookmarks?"*, and anything
    /// looser would let a partly-inferred tree pass as a faithful one.
    ///
    /// [`OutlineDiagnostics::named_destinations_defined`] is excluded
    /// because it is context, not a defect. Everything else counts.
    #[must_use]
    pub const fn is_faithful(&self) -> bool {
        !self.item_budget_exhausted
            && self.depth_truncations == 0
            && self.cycles_broken == 0
            && self.non_reference_links == 0
            && self.unreadable_items == 0
            && self.titles_unreadable == 0
            && self.titles_inexact == 0
            && self.open_state_defaulted == 0
            && self.count_disagreements == 0
            && !self.root_count_disagreement
            && self.dest_and_action_both_present == 0
            && self.unmapped_pages == 0
            && self.unresolved_names == 0
            && self.cross_namespace_resolutions == 0
            && self.unknown_views == 0
            && self.malformed_views == 0
            && self.unreadable_actions == 0
            && self.page_tree_error.is_none()
    }
}

/// A document's outline plus the record of what reading it cost.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Outline {
    /// Top-level items in document order. Empty when the document has
    /// no outline, which is the common case and not an error.
    pub items: Vec<OutlineItem>,
    /// What could not be read exactly. See [`OutlineDiagnostics`].
    pub diagnostics: OutlineDiagnostics,
}

impl Outline {
    /// Every item, at every level, in **document order** — the order a
    /// bookmarks panel lists them with everything expanded.
    ///
    /// Iterative with an explicit stack rather than recursive. Depth is
    /// already bounded by [`MAX_OUTLINE_DEPTH`] so recursion would in
    /// fact be safe here, but this method is the one a caller is most
    /// likely to reach for on a tree it built itself rather than one
    /// this module produced — and that tree carries no such bound.
    #[must_use]
    pub fn flatten(&self) -> Vec<&OutlineItem> {
        let mut out = Vec::with_capacity(self.diagnostics.items);
        let mut stack: Vec<&OutlineItem> = self.items.iter().rev().collect();
        while let Some(item) = stack.pop() {
            out.push(item);
            stack.extend(item.children.iter().rev());
        }
        out
    }

    /// How many items a bookmarks panel would show on first open —
    /// the quantity Table 152's root `/Count` declares.
    ///
    /// *"Total number of visible outline items at all levels of the
    /// outline"*, **including the top-level items themselves**. An item
    /// is visible when every ancestor between it and the root is open;
    /// a closed item is still visible itself, and hides only its
    /// descendants.
    ///
    /// ## The arithmetic, and how it was verified
    ///
    /// `visible(items) = Σ over items of (1 + if open { visible(children) } else { 0 })`
    ///
    /// The consequence worth stating, because it is the part that is
    /// easy to get wrong: a **closed child contributes exactly one** —
    /// itself — and none of its own subtree, however large. It is *not*
    /// skipped, and its descendants are *not* counted.
    ///
    /// This is not derived from the prose alone. ISO 32000-1's Annex H.6
    /// prints the same six-item outline twice, once fully open and once
    /// with one node closed, and every value in both printings
    /// reproduces under this formula — root 6→5, one item 4→3 (not
    /// 4→2), another 1→−1. Annex F corroborates from the other
    /// direction: *"skipping over any subtree that is closed (that is,
    /// whose parent's `Count` value is negative)."*
    ///
    /// Iterative, so a hand-built tree deeper than [`MAX_OUTLINE_DEPTH`]
    /// cannot overflow the stack here.
    #[must_use]
    pub fn visible_item_count(&self) -> usize {
        visible_count(&self.items)
    }
}

/// The visible-item total for one sibling list. See
/// [`Outline::visible_item_count`] for the rule this implements and the
/// Annex H.6 verification behind it.
fn visible_count(items: &[OutlineItem]) -> usize {
    let mut total = 0usize;
    let mut stack: Vec<&OutlineItem> = items.iter().collect();
    while let Some(item) = stack.pop() {
        total += 1;
        // A CLOSED item is itself visible; its descendants are not.
        if item.open {
            stack.extend(item.children.iter());
        }
    }
    total
}

/// Compare every item's declared `/Count` magnitude against the visible
/// descendants the traversal actually found, counting disagreements.
///
/// Returns this sibling list's own visible-item total, so one bottom-up
/// pass serves both the per-item check and the root check.
///
/// Recursion is safe here in a way it would not be in a public method:
/// this runs only over a tree [`read_siblings`] just produced, whose
/// depth is hard-capped at [`MAX_OUTLINE_DEPTH`].
///
/// An item with descendants but **no** `/Count` is deliberately not
/// reported here — it is already counted in
/// [`OutlineDiagnostics::open_state_defaulted`], and reporting the same
/// defect twice would make a UI double-count it.
fn check_counts(items: &[OutlineItem], disagreements: &mut usize) -> usize {
    let mut visible = 0usize;
    for item in items {
        let below = check_counts(&item.children, disagreements);
        if let Some(declared) = item.declared_count
            && u64::try_from(below).unwrap_or(u64::MAX) != declared.unsigned_abs()
        {
            *disagreements += 1;
        }
        visible += 1 + if item.open { below } else { 0 };
    }
    visible
}

// ---------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------

/// Read `graph`'s document outline (§12.3.3) into a tree, with
/// diagnostics.
///
/// Infallible: a document with no `/Outlines`, an `/Outlines` that is
/// not a dictionary, a looping `/Next` chain, or a page tree that cannot
/// be walked all produce a value, never an error. See the module docs'
/// **Contract** section for exactly what "partial" is allowed to mean.
///
/// Works over any [`ObjectGraph`], so it reads the **edited** state when
/// handed an [`EditSession`](crate::edit::EditSession)'s overlay and the
/// **base** file when handed a [`Document`](crate::document::Document) —
/// the reason that trait exists at all (see [`crate::graph`]'s module
/// docs, which name the outline walk as one of its intended consumers).
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::outline::read_outline;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../fixtures/synthetic/outline/basic-tree.pdf").to_vec(),
/// )?;
/// let outline = read_outline(&doc);
///
/// // Two top-level chapters, five items in all.
/// assert_eq!(outline.items.len(), 2);
/// assert_eq!(outline.diagnostics.items, 5);
///
/// // /Count's SIGN is the open/closed state. Chapter 1 is +2: open.
/// let chapter1 = outline.items.first().ok_or("no first item")?;
/// assert_eq!(chapter1.title, "Chapter 1");
/// assert!(chapter1.open);
/// assert_eq!(chapter1.children.len(), 2);
/// assert_eq!(chapter1.page_index(), Some(0));
///
/// // Chapter 2 declares -1: CLOSED, and it still really has one child.
/// // Closed hides the child; it does not remove it.
/// let chapter2 = outline.items.get(1).ok_or("no second item")?;
/// assert!(!chapter2.open);
/// assert_eq!(chapter2.children.len(), 1);
///
/// // The root's /Count counts VISIBLE items including the top level:
/// // both chapters, plus Chapter 1's two children. Chapter 2 is closed,
/// // so its child contributes nothing.
/// assert_eq!(outline.visible_item_count(), 4);
/// assert_eq!(outline.diagnostics.declared_root_count, Some(4));
///
/// // Nothing was truncated, defaulted or guessed.
/// assert!(outline.diagnostics.is_faithful());
/// # Ok(())
/// # }
/// ```
#[must_use]
/// A copied bookmark subtree (`Pass 172.0`).
///
/// # ★ Acrobat cannot do this across documents at all
///
/// Adobe's own documentation says so by name: *"Bookmarks can't be copied
/// directly … from one file to another."* Acrobat offers cut and paste of a
/// bookmark **within** a document and nothing between two. So this is an
/// exceed over the parity reference rather than catching up to it, and the
/// interesting design question — what happens to a destination that names a
/// page the other document does not have — is one Acrobat never had to
/// answer.
///
/// # Why a model and not a raw dictionary
///
/// The [`RawAnnotation`](crate::vector::clip::RawAnnotation) trick does not
/// transfer. An outline item's dictionary is **all back-pointers**:
/// `/Parent`, `/Prev`, `/Next`, `/First`, `/Last` are the tree, and `/Count`
/// is derived from it. Carrying them would carry a shape that means nothing
/// in the destination; stripping them would leave a dictionary with no
/// content but `/Title`. So the clip carries the **logical** subtree and the
/// paste rebuilds the links.
///
/// # The destination is carried by PAGE INDEX, which is the whole trick
///
/// [`Destination::Page`](crate::outline::Destination) is already
/// document-relative — a 0-based index, not an object reference — so it means
/// the same thing in any document that HAS that page. A bookmark pointing at
/// page 3 pastes as a bookmark pointing at page 3.
///
/// When the destination document is shorter, the destination is **dropped and
/// disclosed** rather than clamped to the last page. Clamping would produce a
/// bookmark that navigates confidently to the wrong place, which is worse than
/// one that plainly does not navigate: §12.3.3 permits an item with no `/Dest`
/// (a pure grouping entry), so a destination-less bookmark is a legal, honest
/// shape and a wrong destination is not.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct OutlineClip {
    /// The copied roots, in document order. One for a single-subtree copy;
    /// several when a shell copied a multi-selection.
    pub items: Vec<OutlineClipItem>,
}

/// One bookmark in an [`OutlineClip`], with its children.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct OutlineClipItem {
    /// `/Title`, decoded.
    pub title: String,
    /// Where it navigates, or `None` for a pure grouping entry.
    pub destination: Option<Destination>,
    /// Whether its children show expanded.
    pub open: bool,
    /// `/C`, the display colour in DeviceRGB.
    pub color: Option<[f64; 3]>,
    /// `/F`, the display style flags (Table 154: bit 1 italic, bit 2 bold),
    /// carried raw so bits pdfcer does not model are not silently discarded.
    pub style_flags: Option<i64>,
    /// Its children, in document order.
    pub children: Vec<OutlineClipItem>,
}

impl OutlineClip {
    /// An empty clip — nothing copied.
    ///
    /// ★ Exists because [`OutlineClip`] is `#[non_exhaustive]`, so nothing
    /// outside this crate can write `OutlineClip { items: vec![] }`. A shell
    /// needs the empty value to represent *"the clipboard holds no
    /// bookmarks"*, and without a constructor its only route was to copy
    /// something and hope.
    ///
    /// Found the same way [`PageClip::from_bytes`](crate::pageops::PageClip::from_bytes)
    /// was: an out-of-crate test failing to compile. An in-crate test can
    /// build the struct and would never have noticed — which is the argument
    /// for integration tests living outside the crate they exercise.
    pub const fn empty() -> Self {
        Self { items: Vec::new() }
    }

    /// How many bookmarks the clip holds, counting every descendant.
    #[must_use]
    pub fn len(&self) -> usize {
        fn count(items: &[OutlineClipItem]) -> usize {
            items.iter().map(|i| 1 + count(&i.children)).sum()
        }
        count(&self.items)
    }

    /// Whether the clip holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The deepest page index any bookmark in the clip navigates to.
    ///
    /// A shell can compare it against the destination document's page count
    /// **before** the press and say how many destinations will not survive,
    /// rather than reporting it afterwards.
    #[must_use]
    pub fn deepest_page(&self) -> Option<usize> {
        fn walk(items: &[OutlineClipItem], best: &mut Option<usize>) {
            for item in items {
                if let Some(Destination::Page { page_index, .. }) = item.destination {
                    *best = Some(best.map_or(page_index, |b: usize| b.max(page_index)));
                }
                walk(&item.children, best);
            }
        }
        let mut best = None;
        walk(&self.items, &mut best);
        best
    }

    /// Serialise the clip so it survives leaving this process.
    ///
    /// A COS object through the crate's own writer, for the same reason every
    /// other clipboard in pdfcer takes that route: the grammar has one
    /// implementation on each side rather than a second one per format.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(OUTLINE_CLIP_MAGIC);
        let mut items = Vec::with_capacity(self.items.len());
        for item in &self.items {
            items.push(encode_item(item, 0));
        }
        let mut encoded = Vec::new();
        crate::writer::serialize::write_object(
            &mut encoded,
            &Object::Array(items),
            crate::object::ObjId::new(0, 0),
            &[],
            &crate::writer::encoder::IdentityEncoder,
        );
        out.extend_from_slice(&encoded);
        out
    }

    /// Parse a payload written by [`Self::to_bytes`].
    ///
    /// # Errors
    ///
    /// [`OutlineClipError::NotAClip`] when the magic does not match — checked
    /// first, so an unrelated payload is refused with a sentence rather than
    /// with whatever the parser makes of the wrong bytes — and
    /// [`OutlineClipError::Content`] when the payload is not the COS shape
    /// this format writes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, OutlineClipError> {
        let Some(body) = bytes.strip_prefix(OUTLINE_CLIP_MAGIC.as_slice()) else {
            return Err(OutlineClipError::NotAClip);
        };
        let value = crate::parser::Parser::at(body, 0)
            .parse_object()
            .map_err(|e| OutlineClipError::Content(e.to_string()))?;
        let Some(array) = value.as_array() else {
            return Err(OutlineClipError::Content(
                "the payload is not an array of bookmarks".to_owned(),
            ));
        };
        Ok(Self {
            items: array.iter().filter_map(|o| decode_item(o, 0)).collect(),
        })
    }
}

/// The signature every outline-clip payload starts with.
pub const OUTLINE_CLIP_MAGIC: &[u8; 12] = b"PDFCEBKM\x00\x00\x00\x01";

/// Why an outline clip could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum OutlineClipError {
    /// The payload does not carry the bookmark-clip signature.
    #[error("this is not a pdfcer bookmark payload (it does not carry the clip signature)")]
    NotAClip,
    /// The payload is not the COS shape this format writes.
    #[error("the bookmark payload could not be read: {0}")]
    Content(String),
}

fn encode_item(item: &OutlineClipItem, depth: usize) -> Object {
    let mut d = Dict::new();
    d.insert(
        Name::from(b"T"),
        Object::String(crate::edit::encode_text_string(&item.title)),
    );
    if item.open {
        d.insert(Name::from(b"O"), Object::Boolean(true));
    }
    if let Some([r, g, b]) = item.color {
        d.insert(
            Name::from(b"C"),
            Object::Array(vec![Object::Real(r), Object::Real(g), Object::Real(b)]),
        );
    }
    if let Some(flags) = item.style_flags {
        d.insert(Name::from(b"F"), Object::Integer(flags));
    }
    // Only a RESOLVED destination is carried. An unresolvable one already
    // failed to name a page in the document it came from, so carrying it
    // would move a defect rather than a bookmark.
    if let Some(Destination::Page { page_index, view }) = &item.destination {
        d.insert(
            Name::from(b"P"),
            Object::Integer(i64::try_from(*page_index).unwrap_or(0)),
        );
        d.insert(Name::from(b"V"), encode_view(view));
    }
    // Depth-guarded for the same reason the outline reader is: a clip is
    // untrusted input, and a hostile nesting must cost a truncated subtree
    // rather than a stack.
    if depth < crate::outline::MAX_OUTLINE_DEPTH && !item.children.is_empty() {
        d.insert(
            Name::from(b"K"),
            Object::Array(
                item.children
                    .iter()
                    .map(|c| encode_item(c, depth + 1))
                    .collect(),
            ),
        );
    }
    Object::Dict(d)
}

fn decode_item(obj: &Object, depth: usize) -> Option<OutlineClipItem> {
    let d = obj.as_dict()?;
    let title = match d.get(b"T") {
        Some(Object::String(bytes)) => crate::edit::decode_text_string(bytes).text,
        _ => String::new(),
    };
    let children = if depth < crate::outline::MAX_OUTLINE_DEPTH {
        match d.get(b"K") {
            Some(Object::Array(kids)) => kids
                .iter()
                .filter_map(|k| decode_item(k, depth + 1))
                .collect(),
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };
    Some(OutlineClipItem {
        title,
        destination: d
            .get(b"P")
            .and_then(Object::as_int)
            .and_then(|n| usize::try_from(n).ok())
            .map(|page_index| Destination::Page {
                page_index,
                view: decode_view(d.get(b"V")),
            }),
        open: matches!(d.get(b"O"), Some(Object::Boolean(true))),
        color: match d.get(b"C") {
            Some(Object::Array(a)) => {
                let n: Vec<f64> = a.iter().filter_map(Object::as_number).collect();
                match n[..] {
                    [r, g, b] => Some([r, g, b]),
                    _ => None,
                }
            }
            _ => None,
        },
        style_flags: d.get(b"F").and_then(Object::as_int),
        children,
    })
}

/// Encode a [`DestView`] as a COS array `[/Fit …params]`, so a pasted
/// bookmark arrives at the same zoom and scroll position it was copied from.
///
/// The alternative was to substitute `/Fit` and disclose the loss. That would
/// have been honest and still wrong for the gesture: an operator copying a
/// bookmark to *"Detail B — 400%"* is copying the zoom as much as the page.
///
/// A `null` parameter is written as `null`, because §12.3.2 gives it a
/// meaning — *"leave that aspect of the current view alone"* — distinct from
/// zero.
fn encode_view(view: &DestView) -> Object {
    let num = |v: Option<f64>| v.map_or(Object::Null, Object::Real);
    let mut out = Vec::new();
    match view {
        DestView::Xyz { left, top, zoom } => {
            out.push(Object::Name(Name::from(b"XYZ")));
            out.extend([num(*left), num(*top), num(*zoom)]);
        }
        DestView::Fit => out.push(Object::Name(Name::from(b"Fit"))),
        DestView::FitH { top } => {
            out.push(Object::Name(Name::from(b"FitH")));
            out.push(num(*top));
        }
        DestView::FitV { left } => {
            out.push(Object::Name(Name::from(b"FitV")));
            out.push(num(*left));
        }
        DestView::FitR {
            left,
            bottom,
            right,
            top,
        } => {
            out.push(Object::Name(Name::from(b"FitR")));
            out.extend([num(*left), num(*bottom), num(*right), num(*top)]);
        }
        DestView::FitB => out.push(Object::Name(Name::from(b"FitB"))),
        DestView::FitBH { top } => {
            out.push(Object::Name(Name::from(b"FitBH")));
            out.push(num(*top));
        }
        DestView::FitBV { left } => {
            out.push(Object::Name(Name::from(b"FitBV")));
            out.push(num(*left));
        }
        // An unrecognised fit name is carried VERBATIM rather than degraded
        // to `/Fit`: pdfcer not modelling a name is not evidence the name is
        // wrong, and a viewer that knows it should still get it.
        DestView::Unknown { fit, .. } => {
            out.push(Object::Name(fit.clone()));
        }
        DestView::Absent => out.push(Object::Name(Name::from(b"Fit"))),
    }
    Object::Array(out)
}

/// Read back what [`encode_view`] wrote. Anything unrecognised becomes
/// [`DestView::Fit`] — the one view every viewer implements.
fn decode_view(obj: Option<&Object>) -> DestView {
    let Some(Object::Array(items)) = obj else {
        return DestView::Fit;
    };
    let name = match items.first() {
        Some(Object::Name(n)) => n.as_bytes().to_vec(),
        _ => return DestView::Fit,
    };
    let at = |i: usize| items.get(i).and_then(Object::as_number);
    match name.as_slice() {
        b"XYZ" => DestView::Xyz {
            left: at(1),
            top: at(2),
            zoom: at(3),
        },
        b"FitH" => DestView::FitH { top: at(1) },
        b"FitV" => DestView::FitV { left: at(1) },
        b"FitR" => DestView::FitR {
            left: at(1),
            bottom: at(2),
            right: at(3),
            top: at(4),
        },
        b"FitB" => DestView::FitB,
        b"FitBH" => DestView::FitBH { top: at(1) },
        b"FitBV" => DestView::FitBV { left: at(1) },
        _ => DestView::Fit,
    }
}

/// Read `graph`'s document outline (§12.3.3) with the diagnostics the read
/// produced.
///
/// **The entry point of this module**, and the one a UI should call. Its
/// sibling [`parse_outline`] is the same read with the diagnostics thrown
/// away, which is a convenience and a hazard: only this one can tell you
/// the tree was TRUNCATED.
///
/// # Contract
///
/// **Infallible.** Returns an [`Outline`], never a `Result`. Malformed
/// input yields a *partial* tree plus a populated [`OutlineDiagnostics`] --
/// there is no input that makes it panic, abort, recurse without bound or
/// loop. That is the crate-wide panic-free policy applied to a structure
/// that is unusually easy to weaponise: a `/Next` chain is an
/// attacker-controlled linked list.
///
/// **Bounded**, three ways, each of which sets a diagnostic flag when it
/// bites: at most [`MAX_OUTLINE_ITEMS`] items, at most
/// [`MAX_OUTLINE_DEPTH`] levels, and no object visited twice.
///
/// **Nothing is dropped silently.** An item whose destination will not
/// resolve keeps its place in the tree carrying the most specific
/// [`Destination`] variant the file supports -- see that enum.
///
/// # Why it walks the page tree with [`page_slots`] and not `pages`
///
/// `pages` also resolves `/Resources` and `/MediaBox` and fails the whole
/// walk for a page missing either. A bookmark should still know which page
/// it points at when that page has no `/MediaBox`, so the structural walk
/// is the right one -- the same choice [`DestinationReader::new`] makes,
/// for the same reason.
///
/// # A truncated tree must be PRESENTED as truncated
///
/// [`OutlineDiagnostics::is_faithful`] is the single question a caller
/// should ask before drawing a bookmarks panel. Showing a guard-rail-
/// truncated tree as if it were the document's own is `CLAUDE.md` rule 4
/// (*fuzzy, never sneaky*) applied to a structural inference, and the
/// diagnostics exist so that it can be avoided without the caller
/// re-deriving what happened.
///
/// # ★ This function had NO doc comment until `Pass 224.0`
///
/// It is cited by name in `docs/core-api/`, it is the module's headline
/// entry point, and its explanation lived only in the module header where
/// nothing tied the two together. Recorded rather than quietly fixed
/// because the shape recurs: the *most* documented modules are where an
/// individual item's docs go missing, since the module header makes the
/// absence invisible to a reader who already knows the subject.
pub fn read_outline<G: ObjectGraph + ?Sized>(graph: &G) -> Outline {
    let mut diagnostics = OutlineDiagnostics::default();

    // The page-object -> 0-based-index map, built once. `page_slots` is
    // used rather than `pages` deliberately: `pages` also resolves
    // `/Resources` and `/MediaBox` and can fail with `MissingRequired`
    // for a damaged file, and a bookmark should still know which page it
    // points at when that page has no MediaBox. `page_slots`'s own docs
    // make the same argument for structural operations.
    let page_index: HashMap<ObjId, usize> = match page_slots(graph) {
        Ok(slots) => slots
            .iter()
            .enumerate()
            .map(|(index, slot)| (slot.id, index))
            .collect(),
        Err(error) => {
            diagnostics.page_tree_error = Some(error);
            HashMap::new()
        }
    };

    let named = NamedDestinations::new(graph);
    diagnostics.named_destinations_defined = named.len();

    // §12.3.3: the outline is reached from the catalog's `/Outlines`.
    // Absent is the common case, and `Outline::default()` — an empty
    // tree with clean diagnostics — is the honest answer for it.
    let Some(root) = graph
        .catalog_dict()
        .and_then(|catalog| catalog.get(b"Outlines").map(|value| graph.resolve(value)))
        .and_then(Object::as_dict)
    else {
        return Outline {
            items: Vec::new(),
            diagnostics,
        };
    };

    // Table 152's root `/Count` is the VISIBLE-ITEM TOTAL, a different
    // quantity from an item's `/Count`, and it *"cannot be negative"* —
    // the root has no collapsed state of its own. It is recorded and
    // cross-checked, never used to build the tree: the tree comes from
    // the linked list, which is the only structure the spec says
    // display order follows.
    diagnostics.declared_root_count = graph
        .resolve(root.get(b"Count").unwrap_or(&Object::Null))
        .as_int();

    let mut context = ReadContext {
        budget: MAX_OUTLINE_ITEMS,
        visited: HashSet::new(),
        page_index: &page_index,
        named: &named,
        diagnostics,
    };

    let first = link(graph, root, b"First", &mut context);
    let items = read_siblings(graph, first, 0, &mut context);
    let mut diagnostics = context.diagnostics;

    // The `/Count`-magnitude cross-check, but ONLY over a tree that was
    // read whole. A traversal stopped by the item budget, the depth cap,
    // a cycle or an unreadable link has fewer descendants than the file
    // describes *by construction*, so every count would "disagree" and
    // the diagnostic would report the reader's own guard rails as
    // document corruption — noise precisely when the operator most needs
    // signal.
    let truncated = diagnostics.item_budget_exhausted
        || diagnostics.depth_truncations > 0
        || diagnostics.cycles_broken > 0
        || diagnostics.unreadable_items > 0
        || diagnostics.non_reference_links > 0;
    if !truncated {
        let mut disagreements = 0usize;
        let visible = check_counts(&items, &mut disagreements);
        diagnostics.count_disagreements = disagreements;
        // Table 152's total INCLUDES the top-level items themselves,
        // which `check_counts` already returns for the top-level list.
        diagnostics.root_count_disagreement =
            diagnostics.declared_root_count.is_some_and(|declared| {
                u64::try_from(visible).unwrap_or(u64::MAX) != declared.unsigned_abs()
            });
    }

    Outline { items, diagnostics }
}

/// Read `graph`'s document outline, discarding the diagnostics.
///
/// The shape named in this module's brief, and a genuine convenience for
/// the many callers that only need the tree — but note what it throws
/// away: [`read_outline`] is the entry point that can tell you the tree
/// was **truncated**, and any UI that presents an outline to an operator
/// should use that one instead. This is for tests, for scripted dumps,
/// and for callers that have already checked.
#[must_use]
pub fn parse_outline<G: ObjectGraph + ?Sized>(graph: &G) -> Vec<OutlineItem> {
    read_outline(graph).items
}

// ---------------------------------------------------------------------
// Destinations reached from carriers OTHER than an outline item
// ---------------------------------------------------------------------

/// A reusable resolver that turns **any** destination *carrier*
/// dictionary into a fully resolved [`Destination`].
///
/// ## What a "carrier" is, and why one type covers three of them
///
/// A destination is never written on its own. It is written *on*
/// something, and ISO 32000-1 gives that something exactly two keys:
///
/// - **`/Dest`** — a destination value directly (§12.3.2.2 array,
///   §12.3.2.3 name or byte string), and
/// - **`/A`** — an action dictionary (§12.6) which, *if* its `/S` is
///   `/GoTo` or `/GoToR`, carries a destination in its own `/D`.
///
/// Three unrelated-looking objects carry that identical pair:
///
/// | Carrier | Clause | Table (1.7) |
/// |---|---|---|
/// | Outline item (a bookmark) | §12.3.3 | 153 |
/// | **`/Link` annotation** | **§12.5.6.5** | **173** |
/// | `/Widget` annotation (a pushbutton) | §12.5.6.19 | 188 (`/A`) |
///
/// Table 173 is explicit that a link's `/Dest` *"shall not be present if
/// an `A` entry is present"*, which is the same mutual exclusion Table
/// 153 states for a bookmark — so the precedence rule, the malformed
/// both-present case, the name-tree walk, the `<< /D … >>` wrapper
/// tolerance and the `/GoToR` refusal are **the same rules on all
/// three**. One resolver is therefore not a convenience; two would be a
/// drift hazard of exactly the kind this module's header warns about
/// between itself and [`crate::pageops::references`].
///
/// ## Why this exists as a type rather than a free function
///
/// Resolving a destination needs two document-wide tables:
///
/// 1. the page-object → 0-based-index map, from
///    [`crate::page_tree::page_slots`], and
/// 2. both §12.3.2.3 named-destination namespaces, flattened.
///
/// Both are **O(document)** to build and neither depends on the carrier.
/// A free `link_destination(graph, annot)` would rebuild both on every
/// call, so a page of 200 links would walk the page tree 200 times. The
/// type makes the cost explicit and paid once. Build it when a document
/// is opened; keep it as long as the document is unmodified.
///
/// **It is a snapshot.** It reads the catalog at construction, so a
/// reader built before an edit that adds a named destination, deletes a
/// page, or reorders the page tree will resolve against the *old*
/// structure. Rebuild after any structural edit — this is why it is not
/// cached inside `EditSession`.
///
/// ## Resolved, never raw
///
/// [`Self::destination`] hands back a [`Destination`], not an
/// [`Object`]. That is deliberate and is the whole point of the type: a
/// `/Dest` may be an array, a name into the PDF 1.1 `/Dests` dictionary,
/// or a byte string into the PDF 1.2 `/Names → /Dests` name tree, and a
/// `/GoTo` action's `/D` is the same three-way again. Handing a consumer
/// the unresolved object would put the name-tree walk — with its
/// collision rule, its cycle guard and its [`MAX_DEST_HOPS`] bound — into
/// every consumer that wanted to follow a link, and those copies would
/// diverge from this one.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::annot::page_annotations;
/// use pdfcer_core::document::Document;
/// use pdfcer_core::outline::{Destination, DestinationReader};
/// use pdfcer_core::page_tree::pages;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::load(std::path::Path::new("input.pdf"))?;
/// // Built ONCE for the document, not once per link.
/// let reader = DestinationReader::new(&doc);
///
/// for page in pages(&doc)? {
///     for annot in page_annotations(&doc, page.id) {
///         if annot.subtype != b"Link" {
///             continue;
///         }
///         match annot.destination(&doc, &reader) {
///             Some(Destination::Page { page_index, view }) => {
///                 println!("link at {:?} goes to page {} as {view:?}", annot.rect, page_index + 1);
///             }
///             Some(other) => println!("link is not a local page jump: {other:?}"),
///             None => println!("link carries no destination at all"),
///         }
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct DestinationReader {
    /// Page object id to 0-based document-order index. Empty when the
    /// page tree could not be walked; see [`Self::page_tree_error`].
    page_index: HashMap<ObjId, usize>,
    /// Both catalog named-destination namespaces, pre-flattened.
    named: NamedDestinations,
    /// Why [`Self::page_index`] is empty, when it is empty because the
    /// walk failed rather than because the document has no pages.
    page_tree_error: Option<PageTreeError>,
}

impl DestinationReader {
    /// Build a reader for `graph`'s catalog.
    ///
    /// Infallible by design, exactly as [`read_outline`] is. A document
    /// whose page tree is damaged still has readable links — they simply
    /// resolve to [`Destination::UnmappedPage`] instead of
    /// [`Destination::Page`], and [`Self::page_tree_error`] says why.
    /// Returning a `Result` here would force every caller to choose
    /// between refusing to show links at all and unwrapping, and the
    /// first is worse for the operator than a link that reports it
    /// cannot find its page.
    ///
    /// [`page_slots`] is used rather than [`crate::page_tree::pages`]
    /// for the same reason [`read_outline`] gives: `pages` also resolves
    /// `/Resources` and `/MediaBox` and fails the whole walk for a page
    /// missing either, and a link should still know which page it points
    /// at when that page has no `/MediaBox`.
    #[must_use]
    pub fn new<G: ObjectGraph + ?Sized>(graph: &G) -> Self {
        let (page_index, page_tree_error) = match page_slots(graph) {
            Ok(slots) => (
                slots
                    .iter()
                    .enumerate()
                    .map(|(index, slot)| (slot.id, index))
                    .collect(),
                None,
            ),
            Err(error) => (HashMap::new(), Some(error)),
        };
        Self {
            page_index,
            named: NamedDestinations::new(graph),
            page_tree_error,
        }
    }

    /// Why the page-object map is empty, when the page-tree walk failed.
    ///
    /// `None` is the ordinary case. `Some` means **every**
    /// [`Destination`] this reader produces from an explicit array will
    /// be [`Destination::UnmappedPage`] regardless of whether the target
    /// page exists — the reader could not build the map to check
    /// against. A consumer that reports "this link is broken" without
    /// checking this would blame the document for the reader's own
    /// failure, which is the exact instrument-blindness this project
    /// keeps finding.
    #[must_use]
    pub fn page_tree_error(&self) -> Option<&PageTreeError> {
        self.page_tree_error.as_ref()
    }

    /// How many named destinations the document defines, across **both**
    /// §12.3.2.3 namespaces after collision merging.
    ///
    /// `0` with links present is a useful signal: every by-name link in
    /// the document will resolve to [`Destination::Named`], which usually
    /// means the `/Names` tree was lost by a producer or a page-range
    /// extraction.
    #[must_use]
    pub fn named_destination_count(&self) -> usize {
        self.named.len()
    }

    /// Resolve the destination carried by `carrier`, if it carries one.
    ///
    /// `carrier` is the *dictionary of the thing that was clicked* — a
    /// `/Link` annotation, a `/Widget`, or an outline item. It is read
    /// for `/Dest` first and `/A` second, per Table 153 and Table 173's
    /// identical precedence.
    ///
    /// # Returns
    ///
    /// - `None` — the carrier has **neither** `/Dest` nor `/A`. For a
    ///   `/Link` that is a malformed annotation (Table 173 requires one
    ///   of them for the annotation to do anything); for a `/Widget` it
    ///   is entirely ordinary — most widgets are not buttons.
    /// - `Some(`[`Destination::Page`]`)` — a page in **this** document,
    ///   with the Table 151 view to establish on arrival. The only
    ///   variant a viewer can navigate directly.
    /// - `Some(`[`Destination::UnmappedPage`]`)` — the destination named
    ///   an object that is not a page in this document's tree. A link
    ///   left behind by a page delete looks exactly like this.
    /// - `Some(`[`Destination::Named`]`)` — a name neither namespace
    ///   defines.
    /// - `Some(`[`Destination::Remote`]`)` — `/GoToR`, a page of another
    ///   file. **Never resolved against this document's names**, by
    ///   design; see [`read_remote`].
    /// - `Some(`[`Destination::NonNavigation`]`)` — an action that is not
    ///   a page jump: `/URI`, `/Launch`, `/JavaScript`, `/SubmitForm`,
    ///   `/Named`, `/GoToE`, anything else. **Recognised and disclosed,
    ///   never executed.**
    ///
    /// The last four are the reason this returns the enum rather than an
    /// `Option<usize>` page index: a viewer that collapses them into
    /// "no link here" tells the operator nothing, and a viewer that
    /// collapses them into a page jump lies.
    ///
    /// Diagnostics accumulated during the read are discarded. Use
    /// [`Self::destination_with_diagnostics`] to keep them.
    #[must_use]
    pub fn destination<G: ObjectGraph + ?Sized>(
        &self,
        graph: &G,
        carrier: &Dict,
    ) -> Option<Destination> {
        self.destination_with_diagnostics(graph, carrier).0
    }

    /// [`Self::destination`], keeping the diagnostics the read produced.
    ///
    /// Only the destination-related counters can be non-zero — this does
    /// no tree traversal, so `items`, `max_depth`, `cycles_broken` and
    /// the title counters are always at their defaults. What can fire is
    /// [`OutlineDiagnostics::dest_and_action_both_present`] (Table 173
    /// forbids it and pdfcer takes `/Dest`),
    /// [`OutlineDiagnostics::unmapped_pages`],
    /// [`OutlineDiagnostics::unresolved_names`],
    /// [`OutlineDiagnostics::cross_namespace_resolutions`],
    /// [`OutlineDiagnostics::unknown_views`],
    /// [`OutlineDiagnostics::malformed_views`] and
    /// [`OutlineDiagnostics::unreadable_actions`].
    ///
    /// `page_tree_error` and `named_destinations_defined` are copied in
    /// from the reader rather than measured per call, so they report the
    /// document, not this carrier.
    #[must_use]
    pub fn destination_with_diagnostics<G: ObjectGraph + ?Sized>(
        &self,
        graph: &G,
        carrier: &Dict,
    ) -> (Option<Destination>, OutlineDiagnostics) {
        // The budget and visited set are inert here: neither
        // `resolve_item_destination` nor anything it calls reads them —
        // they bound the *sibling/child* traversal, which this entry
        // point does not perform. They are given their ordinary values
        // rather than zero so that a future change which does start
        // consulting them is bounded correctly instead of refusing every
        // read.
        let mut context = ReadContext {
            budget: MAX_OUTLINE_ITEMS,
            visited: HashSet::new(),
            page_index: &self.page_index,
            named: &self.named,
            diagnostics: OutlineDiagnostics {
                page_tree_error: self.page_tree_error.clone(),
                named_destinations_defined: self.named.len(),
                ..OutlineDiagnostics::default()
            },
        };
        let resolved = resolve_item_destination(graph, carrier, &mut context);
        (resolved, context.diagnostics)
    }
}

// ---------------------------------------------------------------------
// Traversal
// ---------------------------------------------------------------------

/// Mutable state threaded through the whole read.
///
/// A struct rather than eight arguments because every one of these
/// crosses every frame of the traversal, and because the budget and the
/// visited set are only correct if they are *shared* — a per-level copy
/// of either would bound each branch separately and leave the total
/// unbounded, which is the exact bug the guards exist to prevent.
struct ReadContext<'a> {
    /// Items still permitted. Decremented once per item accepted.
    budget: usize,
    /// Every outline object already visited, anywhere in the tree.
    ///
    /// Global, not per-branch. An item reachable twice is malformed
    /// however it is reached, and a per-branch set would let a diamond
    /// expand exponentially while every individual path stayed short.
    visited: HashSet<ObjId>,
    /// Page object id to 0-based document-order index.
    page_index: &'a HashMap<ObjId, usize>,
    /// Both catalog named-destination namespaces, pre-flattened.
    named: &'a NamedDestinations,
    /// Accumulating record of what could not be read exactly.
    diagnostics: OutlineDiagnostics,
}

/// Read a `/Next` chain and everything below it.
///
/// **Iterative across siblings, recursive across levels**, which is the
/// same split [`crate::pageops::references::walk_outline`] makes and for
/// the same reason: the sibling chain is the unbounded direction in real
/// files — a flat 10,000-entry outline is ordinary — while nesting is
/// shallow and hard-capped at [`MAX_OUTLINE_DEPTH`]. Recursing on
/// siblings would overflow the stack on exactly the documents this needs
/// to work for, and no `#[deny(panic)]` catches a stack overflow.
///
/// Every early exit records *why*. A chain that stops because the budget
/// ran out and a chain that stops because it genuinely ended must not be
/// indistinguishable in the output — see [`OutlineDiagnostics`].
fn read_siblings<G: ObjectGraph + ?Sized>(
    graph: &G,
    first: Option<ObjId>,
    level: usize,
    context: &mut ReadContext<'_>,
) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    let mut current = first;

    while let Some(id) = current {
        if context.budget == 0 {
            context.diagnostics.item_budget_exhausted = true;
            break;
        }
        // The cycle guard. Note it fires on the *link*, before any work:
        // a `/Next` pointing back at an earlier sibling ends the chain
        // here rather than re-reading a subtree that is already in the
        // output.
        if !context.visited.insert(id) {
            context.diagnostics.cycles_broken += 1;
            break;
        }
        context.budget -= 1;

        // §7.3.10: a dangling reference is `null`, not an error. There
        // is no way to continue a chain through an object that is not a
        // dictionary — the `/Next` would have to come from it — so the
        // chain ends, and the truncation is recorded.
        let Some(dict) = graph.resolved(id).as_dict() else {
            context.diagnostics.unreadable_items += 1;
            break;
        };

        let item = read_item(graph, id, dict, level, context);
        items.push(item);

        current = link(graph, dict, b"Next", context);
    }

    items
}

/// Build one [`OutlineItem`] from its dictionary, then descend.
///
/// Split out from [`read_siblings`] so the sibling loop stays readable
/// as a loop: the per-item work is a dozen independent field reads and
/// interleaving them with the chain-walking control flow is how a
/// `continue` eventually skips the wrong thing.
fn read_item<G: ObjectGraph + ?Sized>(
    graph: &G,
    id: ObjId,
    dict: &Dict,
    level: usize,
    context: &mut ReadContext<'_>,
) -> OutlineItem {
    context.diagnostics.items += 1;
    context.diagnostics.max_depth = context.diagnostics.max_depth.max(level);

    // --- /Title (Table 153, required; §7.9.2 text string) ------------
    let (title, title_exact) = match graph.resolve(dict.get(b"Title").unwrap_or(&Object::Null)) {
        Object::String(bytes) => {
            let decoded = decode_text_string(bytes);
            if !decoded.exact {
                context.diagnostics.titles_inexact += 1;
            }
            (decoded.text, decoded.exact)
        }
        _ => {
            context.diagnostics.titles_unreadable += 1;
            (String::new(), true)
        }
    };

    // --- /Count (Table 153) — SIGN only; see the module docs ---------
    let declared_count = graph
        .resolve(dict.get(b"Count").unwrap_or(&Object::Null))
        .as_int();
    let has_children_link = dict.contains_key(b"First");
    let open = match declared_count {
        Some(count) => count > 0,
        None => {
            // Only a defect when there is something to expand. A leaf
            // with no `/Count` is entirely ordinary and must not inflate
            // the diagnostic.
            if has_children_link {
                context.diagnostics.open_state_defaulted += 1;
            }
            false
        }
    };

    // --- /C and /F (Table 153, PDF 1.4) ------------------------------
    let color = read_color(graph, dict);
    let style_flags = graph
        .resolve(dict.get(b"F").unwrap_or(&Object::Null))
        .as_int();

    // --- /Dest and /A (Table 153; §12.3.2, §12.6) --------------------
    let destination = resolve_item_destination(graph, dict, context);

    // --- children ----------------------------------------------------
    // The depth cap is checked *before* descending, so an item at the
    // limit keeps everything already read about itself and loses only
    // its subtree.
    let children = if level + 1 >= MAX_OUTLINE_DEPTH {
        if has_children_link {
            context.diagnostics.depth_truncations += 1;
        }
        Vec::new()
    } else {
        let first = link(graph, dict, b"First", context);
        read_siblings(graph, first, level + 1, context)
    };

    OutlineItem {
        id,
        title,
        title_exact,
        destination,
        children,
        open,
        level,
        declared_count,
        color,
        style_flags,
    }
}

/// Read a structural link (`/First` or `/Next`) as an object id.
///
/// Absent is normal — it is how both chains terminate — but *present and
/// not a reference* is a defect worth counting: §12.3.3's links are
/// indirect references, and a direct dictionary there means a producer
/// inlined a node that the `/Prev`/`/Parent` back-links can no longer
/// name. The chain has to stop either way; the counter is what stops
/// that stop from looking like a normal end.
fn link<G: ObjectGraph + ?Sized>(
    graph: &G,
    dict: &Dict,
    key: &[u8],
    context: &mut ReadContext<'_>,
) -> Option<ObjId> {
    let value = dict.get(key)?;
    match value.as_reference() {
        Some(id) => Some(id),
        None => {
            // A reference that resolves to null is a *dangling* link,
            // already covered by `as_reference` returning the id and the
            // caller finding no dictionary. Reaching here means the
            // value was never a reference at all.
            if !matches!(graph.resolve(value), Object::Null) {
                context.diagnostics.non_reference_links += 1;
            }
            None
        }
    }
}

/// `/C` — a DeviceRGB triple (Table 153, PDF 1.4).
///
/// Returns `None` for anything that is not exactly three numbers.
/// Silently tolerating a two- or four-element array would mean guessing
/// which component was missing, and a bookmark drawn in the wrong colour
/// is a defect that never announces itself.
fn read_color<G: ObjectGraph + ?Sized>(graph: &G, dict: &Dict) -> Option<[f64; 3]> {
    let array = graph
        .resolve(dict.get(b"C")?)
        .as_array()
        .filter(|items| items.len() == 3)?;
    let mut out = [0.0f64; 3];
    for (slot, value) in out.iter_mut().zip(array.iter()) {
        *slot = graph.resolve(value).as_number()?;
    }
    Some(out)
}

// ---------------------------------------------------------------------
// Destination resolution
// ---------------------------------------------------------------------

/// Resolve an item's `/Dest` and/or `/A` to a [`Destination`].
///
/// ## Precedence, and why it is `/Dest` — spec ambiguity `OL-A1`
///
/// Table 153 marks `/Dest` and `/A` **mutually exclusive in both of
/// their rows** — an item carrying both is malformed — and then the
/// standard says nothing whatever about how to read one that does. This
/// is therefore a product decision, and it is made as follows.
///
/// 1. **The standard states a preference between the two forms, even
///    though it states no precedence.** §12.6.4.2's NOTE says a `/GoTo`
///    action and a direct `/Dest` *"have the same effect"*, but that the
///    action *"is less compact and is not compatible with PDF 1.0;
///    therefore, using a direct destination is preferable."* Where a
///    file contradicts itself, honouring the form the spec calls
///    preferable is the least surprising reading available.
/// 2. **[`crate::pageops::references::DestinationResolver::resolve_target`]
///    already does exactly this**, and it is the function that decides
///    whether deleting a page reports this bookmark as broken. If the
///    bookmarks panel and the delete census disagreed about where a
///    bookmark points, the operator would be told a bookmark is fine and
///    then watch it break. Crate-internal consistency outranks any
///    independent judgement about which key is "better" here.
///
/// The fall-through matches that function too: `/A` is consulted when
/// `/Dest` is **absent or wholly unreadable**, not when `/Dest` merely
/// failed to reach a live page. A `/Dest` naming a missing page is an
/// *answer* — the bookmark is broken, and saying so is the point — and
/// quietly substituting the `/A` would hide the corruption behind a
/// working link.
fn resolve_item_destination<G: ObjectGraph + ?Sized>(
    graph: &G,
    dict: &Dict,
    context: &mut ReadContext<'_>,
) -> Option<Destination> {
    let has_dest = dict.contains_key(b"Dest");
    let has_action = dict.contains_key(b"A");
    if has_dest && has_action {
        context.diagnostics.dest_and_action_both_present += 1;
    }

    if let Some(dest) = dict.get(b"Dest")
        && let Some(resolved) = resolve_destination_value(graph, dest, context)
    {
        return Some(resolved);
    }

    if has_action {
        return Some(read_action(graph, dict, context));
    }
    None
}

/// Read an item's `/A` action dictionary (§12.6) as a [`Destination`].
///
/// **Any** of Table 198's twenty action types may appear on a bookmark;
/// only `/GoTo` (Table 199) and `/GoToR` (Table 200) are page
/// navigations. Everything else — `/URI`, `/Launch`, `/JavaScript`,
/// `/Named`, `/Thread`, and any future extension — becomes
/// [`Destination::NonNavigation`] carrying its `/S`, which is the
/// *recognise and disclose, never execute* posture the Acrobat-parity
/// RAG recommends for bookmark actions.
///
/// `/GoToE` (embedded go-to, Table 201) is deliberately in the second
/// group rather than the first. It targets a page inside an *embedded
/// file*, reached through a `/T` target chain, and pdfcer has no
/// embedded-file navigation to resolve it against. Reporting it as a
/// disclosed action is honest; giving it a page index of this document
/// would not be.
///
/// `/Next` on an action dictionary (§12.6.1's action chaining) is
/// deliberately **not** followed: a bookmark whose first action is a
/// navigation navigates, and one whose first action is a script is
/// disclosed as a script regardless of what it chains to. Following the
/// chain would let a `/JavaScript` action be reported as a page jump
/// because something further down the chain was a `/GoTo`.
fn read_action<G: ObjectGraph + ?Sized>(
    graph: &G,
    dict: &Dict,
    context: &mut ReadContext<'_>,
) -> Destination {
    let Some(action) = dict
        .get(b"A")
        .map(|value| graph.resolve(value))
        .and_then(Object::as_dict)
    else {
        context.diagnostics.unreadable_actions += 1;
        return Destination::NonNavigation { action: None };
    };
    let subtype = graph
        .resolve(action.get(b"S").unwrap_or(&Object::Null))
        .as_name()
        .cloned();
    let Some(subtype) = subtype else {
        context.diagnostics.unreadable_actions += 1;
        return Destination::NonNavigation { action: None };
    };

    match subtype.as_bytes() {
        // §12.6.4.2 — same document. `/D` is a destination in any of
        // §12.3.2's forms, so it goes through the same resolver the
        // item-level `/Dest` uses.
        b"GoTo" => action
            .get(b"D")
            .and_then(|dest| resolve_destination_value(graph, dest, context))
            .unwrap_or(Destination::NonNavigation {
                action: Some(subtype),
            }),
        // §12.6.4.3 — another file.
        b"GoToR" => read_remote(graph, action, context),
        _ => Destination::NonNavigation {
            action: Some(subtype),
        },
    }
}

/// Read a `/GoToR` action (§12.6.4.3) as [`Destination::Remote`].
///
/// The remote destination's `/D` is resolved **without** consulting this
/// document's name trees, which is the whole reason `/GoToR` cannot
/// share [`resolve_destination_value`]. A name in a remote destination
/// belongs to the *target* file's namespace; looking it up here would,
/// on a document that happens to define the same name, silently
/// navigate to a page of the wrong file — a wrong answer that looks
/// entirely convincing.
fn read_remote<G: ObjectGraph + ?Sized>(
    graph: &G,
    action: &Dict,
    context: &mut ReadContext<'_>,
) -> Destination {
    let file = action
        .get(b"F")
        .and_then(|spec| file_spec_bytes(graph, spec));
    let new_window = match graph.resolve(action.get(b"NewWindow").unwrap_or(&Object::Null)) {
        Object::Boolean(value) => Some(*value),
        _ => None,
    };

    // Chase `/D` through any `<< /D … >>` wrappers, bounded, but never
    // through this document's named-destination map.
    let mut current = action
        .get(b"D")
        .map_or(Object::Null, |value| graph.resolve(value).clone());
    let mut target = RemoteTarget::Unknown;
    let mut view = DestView::Absent;
    for _ in 0..MAX_DEST_HOPS {
        match current {
            Object::Array(ref items) => {
                view = read_view(graph, items, context);
                target = match items.first().map(|first| graph.resolve(first)) {
                    Some(Object::Integer(number)) => RemoteTarget::PageNumber(*number),
                    _ => RemoteTarget::Unknown,
                };
                break;
            }
            Object::String(ref bytes) => {
                target = RemoteTarget::Named(bytes.clone());
                break;
            }
            Object::Name(ref name) => {
                target = RemoteTarget::Named(name.as_bytes().to_vec());
                break;
            }
            Object::Dict(ref dict) => match dict.get(b"D") {
                Some(inner) => current = graph.resolve(inner).clone(),
                None => break,
            },
            _ => break,
        }
    }

    Destination::Remote {
        file,
        target,
        view,
        new_window,
    }
}

/// Reduce a file specification (§7.11) to display bytes.
///
/// Handles the two shapes that actually occur on `/GoToR`: a bare
/// string (§7.11.2), and a file-specification dictionary (Table 44).
///
/// ## `/UF` before `/F` — spec ambiguity `EF-A3`, resolved by policy
///
/// **ISO 32000-1 states no precedence between `/UF` and `/F`.**
/// Everything it says is a `should` and none of it orders the two:
/// `/UF` *"should be used **in addition to** the `F` entry"*, is
/// *"recommended if the `F` entry exists"*, and supplies
/// *"cross-platform and cross-language compatibility"* where `/F`
/// supplies *"backwards compatibility"*.
///
/// pdfcer prefers `/UF` here because this value is used **textually** —
/// it is shown to an operator as "this bookmark opens *that* file" —
/// and `/UF` is the only one of the two with a defined character
/// encoding (PDFDocEncoding or UTF-16BE with a BOM, §7.9.2.2). `/F` is
/// a byte string that §7.11.2.1 says must be handed to the operating
/// system *"without interpretation or conversion of any sort"*, which
/// makes it the right value to **open** a file with and the wrong one
/// to **display**.
///
/// The PDF_Spec RAG recommends this be a setting
/// (`filename_source = uf_then_f | f_then_uf | f_only`, default
/// `uf_then_f`) rather than hard-coded, which matches the project's
/// standing rule that a choice the standard leaves open becomes a
/// setting. It is hard-coded to the recommended default here only
/// because this module could not reach `crate::settings`; see the Pass
/// report.
///
/// ## Stated gap
///
/// §7.11's full model — `/DOS`, `/Mac`, `/Unix`, `/FS /URL`,
/// relative-path resolution, embedded-file streams — is not implemented.
/// Anything not covered returns `None` rather than a guess, so a caller
/// sees "pdfcer could not read this file reference" instead of a
/// plausible wrong path.
fn file_spec_bytes<G: ObjectGraph + ?Sized>(graph: &G, spec: &Object) -> Option<Vec<u8>> {
    match graph.resolve(spec) {
        Object::String(bytes) => Some(bytes.clone()),
        Object::Dict(dict) => {
            for key in [b"UF".as_slice(), b"F".as_slice()] {
                if let Some(Object::String(bytes)) = dict.get(key).map(|v| graph.resolve(v)) {
                    return Some(bytes.clone());
                }
            }
            None
        }
        _ => None,
    }
}

/// Resolve a **same-document** destination value (§12.3.2) — any of its
/// four shapes — to a [`Destination`].
///
/// Returns `None` only when the value is nothing a destination can be
/// (a number, a boolean, `null`, a dangling reference). Every other
/// outcome, including complete failure to reach a page, is a `Some` with
/// a variant that says what went wrong — brief requirements (1) and (2).
///
/// The loop is bounded by [`MAX_DEST_HOPS`] because §12.3.2.3 does not
/// forbid a named destination whose value is another name, and a
/// two-name cycle is as easy to author as a `/Next` cycle.
fn resolve_destination_value<G: ObjectGraph + ?Sized>(
    graph: &G,
    dest: &Object,
    context: &mut ReadContext<'_>,
) -> Option<Destination> {
    // Owned, because a hop through the named-destination map lands on a
    // value the map owns and a borrow would tie the loop's lifetime to
    // the first iteration.
    let mut current = graph.resolve(dest).clone();
    // Names already followed, so `/A -> /B -> /A` terminates at the
    // cycle rather than at the hop budget. Almost always empty.
    let mut seen: Vec<Vec<u8>> = Vec::new();

    for _ in 0..MAX_DEST_HOPS {
        match current {
            // Shape 1: the explicit array. §12.3.2.2 requires element 0
            // to be an indirect reference to a page object.
            Object::Array(ref items) => {
                let view = read_view(graph, items, context);
                let page = items.first().and_then(Object::as_reference);
                return Some(match page.and_then(|id| context.page_index.get(&id)) {
                    Some(&page_index) => Destination::Page { page_index, view },
                    None => {
                        context.diagnostics.unmapped_pages += 1;
                        Destination::UnmappedPage { page, view }
                    }
                });
            }
            // Shapes 3 and 4: a NAME object keys the PDF 1.1 `/Dests`
            // dictionary; a STRING keys the PDF 1.2 `/Names -> /Dests`
            // tree. §12.3.2.3 offers that type as its only discriminator
            // and states no fallback, so pdfcer searches both and
            // discloses when the type pointed the other way — spec
            // ambiguity `DEST-A1`. See `NamedDestinations::new`.
            Object::Name(ref name) => {
                let key = name.as_bytes().to_vec();
                current = match next_named(context, &key, Namespace::LegacyDict, &mut seen) {
                    Step::Value(value) => value,
                    Step::Unresolved => {
                        context.diagnostics.unresolved_names += 1;
                        return Some(Destination::Named { name: key });
                    }
                };
            }
            Object::String(ref bytes) => {
                let key = bytes.clone();
                current = match next_named(context, &key, Namespace::NameTree, &mut seen) {
                    Step::Value(value) => value,
                    Step::Unresolved => {
                        context.diagnostics.unresolved_names += 1;
                        return Some(Destination::Named { name: key });
                    }
                };
            }
            // §12.3.2.3: a named destination's VALUE may be a dictionary
            // rather than the array itself. NOTE 2 gives that wrapper
            // two forms — a `/D` holding the destination array, or a
            // go-to **action** — and both are followed here.
            //
            // Note this is the value's shape, not `/Dest`'s: Table 153
            // types an outline item's `/Dest` as "name, byte string, or
            // array", so a dictionary written directly there is
            // malformed. Following it anyway is deliberate tolerance,
            // costs nothing, and matches the rest of the crate.
            Object::Dict(ref dict) => {
                current = match dict.get(b"D") {
                    Some(inner) => graph.resolve(inner).clone(),
                    None => {
                        let action = graph.resolve(dict.get(b"A")?).as_dict()?;
                        let is_goto = action
                            .get(b"S")
                            .map(|value| graph.resolve(value))
                            .and_then(Object::as_name)
                            .map(Name::as_bytes)
                            .is_some_and(|subtype| subtype == b"GoTo");
                        if !is_goto {
                            return None;
                        }
                        graph.resolve(action.get(b"D")?).clone()
                    }
                };
            }
            _ => return None,
        }
    }
    None
}

/// One step of the named-destination walk.
enum Step {
    /// The name resolved; here is what it resolved to.
    Value(Object),
    /// Neither namespace defines it, or following it would loop.
    Unresolved,
}

/// Look up one destination name, refusing to revisit one already
/// followed, and disclosing a namespace mismatch.
///
/// `expected` is the namespace the *reference's type* pointed at — the
/// only discriminator §12.3.2.3 provides. Resolving in the other one
/// still succeeds (pdfcer is lenient here so that files other readers
/// open do not break), but it increments
/// [`OutlineDiagnostics::cross_namespace_resolutions`] so the leniency
/// is visible rather than assumed. Spec ambiguity `DEST-A1`.
///
/// The `seen` list is a `Vec` rather than a `HashSet` on purpose: it is
/// empty for every destination in every well-formed document and holds
/// one entry for almost every remaining one, so a linear scan over at
/// most [`MAX_DEST_HOPS`] entries beats hashing a byte string.
fn next_named(
    context: &mut ReadContext<'_>,
    key: &[u8],
    expected: Namespace,
    seen: &mut Vec<Vec<u8>>,
) -> Step {
    if seen.iter().any(|previous| previous == key) {
        return Step::Unresolved;
    }
    seen.push(key.to_vec());
    match context.named.lookup(key) {
        Some((value, found)) => {
            if *found != expected {
                context.diagnostics.cross_namespace_resolutions += 1;
            }
            Step::Value(value.clone())
        }
        None => Step::Unresolved,
    }
}

/// Read the fit style and parameters from a destination array
/// (§12.3.2, Table 151).
///
/// Element 0 is the page (handled by the caller, which needs it in a
/// different form for the local and remote cases); element 1 is the fit
/// name; elements 2 onward are its parameters, **positional**.
///
/// A `null` parameter and a *missing* parameter both become `None`, and
/// that conflation is deliberate: §12.3.2 gives `null` the meaning
/// "retain the current value" for `/XYZ`, and a viewer handed a short
/// `/XYZ` array has no better option than the same behaviour. For the
/// styles whose parameters are required, `None` is malformation and is
/// counted — the difference between the two readings is recorded in
/// [`OutlineDiagnostics::malformed_views`] rather than in the value.
fn read_view<G: ObjectGraph + ?Sized>(
    graph: &G,
    items: &[Object],
    context: &mut ReadContext<'_>,
) -> DestView {
    /// Positional parameter `index` (0-based *after* the fit name), as a
    /// number, or `None` for absent / `null` / non-numeric.
    fn param<G: ObjectGraph + ?Sized>(graph: &G, items: &[Object], index: usize) -> Option<f64> {
        graph.resolve(items.get(index + 2)?).as_number()
    }

    let Some(fit) = items
        .get(1)
        .map(|value| graph.resolve(value))
        .and_then(Object::as_name)
    else {
        context.diagnostics.malformed_views += 1;
        return DestView::Absent;
    };

    let mut required_missing = 0usize;
    /// Count a required parameter that was absent, so the caller can
    /// report the array as malformed without duplicating the test.
    macro_rules! required {
        ($value:expr) => {{
            let value = $value;
            if value.is_none() {
                required_missing += 1;
            }
            value
        }};
    }

    let view = match fit.as_bytes() {
        // `/XYZ`'s three parameters are the ONLY ones the spec gives a
        // null meaning to, so they are read without the `required!`
        // wrapper — an absent one is a documented state, not damage.
        b"XYZ" => DestView::Xyz {
            left: param(graph, items, 0),
            top: param(graph, items, 1),
            zoom: param(graph, items, 2),
        },
        b"Fit" => DestView::Fit,
        b"FitH" => DestView::FitH {
            top: required!(param(graph, items, 0)),
        },
        b"FitV" => DestView::FitV {
            left: required!(param(graph, items, 0)),
        },
        b"FitR" => DestView::FitR {
            left: required!(param(graph, items, 0)),
            bottom: required!(param(graph, items, 1)),
            right: required!(param(graph, items, 2)),
            top: required!(param(graph, items, 3)),
        },
        b"FitB" => DestView::FitB,
        b"FitBH" => DestView::FitBH {
            top: required!(param(graph, items, 0)),
        },
        b"FitBV" => DestView::FitBV {
            left: required!(param(graph, items, 0)),
        },
        _ => {
            context.diagnostics.unknown_views += 1;
            DestView::Unknown { fit: fit.clone() }
        }
    };

    if required_missing > 0 {
        context.diagnostics.malformed_views += 1;
    }
    view
}

// ---------------------------------------------------------------------
// Named destinations (§12.3.2.3, §7.9.6)
// ---------------------------------------------------------------------

/// Both catalog named-destination namespaces, flattened once.
///
/// Flattening eagerly is what keeps the outline read linear: resolving a
/// name means walking a name tree, and doing that per bookmark on a
/// 5,000-entry outline is quadratic. Same argument, same shape, as
/// [`crate::pageops::references::DestinationResolver`] — see this
/// module's docs for why the two exist separately and what should be
/// done about it.
#[derive(Debug, Default)]
struct NamedDestinations {
    /// Name bytes to (destination value, the namespace it came from).
    ///
    /// The value is an array, or a `<< /D … >>` / `<< /A … >>` wrapper
    /// dictionary, or — in a malformed file — another name. The
    /// namespace is carried so [`next_named`] can disclose a lookup that
    /// crossed from one to the other; see `DEST-A1` in the module docs.
    map: HashMap<Vec<u8>, (Object, Namespace)>,
}

/// Which of §12.3.2.3's two named-destination namespaces a key came
/// from.
///
/// Recorded rather than discarded because the *type* of a `/Dest`
/// reference is the only discriminator the standard offers between them
/// (name object ⇒ [`Namespace::LegacyDict`], string ⇒
/// [`Namespace::NameTree`]), and pdfcer deliberately ignores that
/// discriminator when resolving. Keeping the provenance is what turns
/// that leniency from a silent policy into a reported one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Namespace {
    /// The PDF 1.1 catalog `/Dests` **dictionary**, keyed by name
    /// objects.
    LegacyDict,
    /// The PDF 1.2 catalog `/Names → /Dests` **name tree**, keyed by
    /// strings.
    NameTree,
}

impl NamedDestinations {
    /// Flatten `graph`'s catalog `/Dests` dictionary and
    /// `/Names → /Dests` name tree into one lookup table.
    ///
    /// ## The two namespaces
    ///
    /// §12.3.2.3 defines two, from different PDF versions:
    ///
    /// - **PDF 1.1** — catalog `/Dests`, a plain dictionary whose keys
    ///   are *name objects*, referenced as `/Dest /SomeName`.
    /// - **PDF 1.2** — catalog `/Names → /Dests`, a §7.9.6 *name tree*
    ///   whose keys are *strings*, referenced as `/Dest (SomeName)`.
    ///
    /// ## Why they are merged, and who wins a collision
    ///
    /// They are merged into one table, keyed by raw bytes, because a
    /// `/Dest` value gives no hint which namespace its author meant and
    /// real files are not consistent about the name-vs-string spelling.
    /// A resolver that searched only the matching namespace would fail
    /// on documents that other readers open fine.
    ///
    /// The legacy dictionary is loaded **first** and the name tree
    /// second, so **the name tree wins** a colliding key. That ordering
    /// is not a considered ruling on which namespace is more
    /// authoritative — the spec says nothing about collisions, and no
    /// observed file populates both with the same key. It is copied
    /// verbatim from
    /// [`crate::pageops::references::DestinationResolver::new`] so the
    /// bookmarks panel and the page-delete census cannot resolve the
    /// same name two different ways. If that ordering is ever revisited,
    /// **both** must change together.
    fn new<G: ObjectGraph + ?Sized>(graph: &G) -> Self {
        let mut map = HashMap::new();
        let Some(catalog) = graph.catalog_dict() else {
            return Self { map };
        };

        // §12.3.2.3, PDF 1.1: a plain dictionary keyed by name objects.
        if let Some(dests) = catalog
            .get(b"Dests")
            .map(|value| graph.resolve(value))
            .and_then(Object::as_dict)
        {
            for (key, value) in dests.iter() {
                map.insert(
                    key.as_bytes().to_vec(),
                    (value.clone(), Namespace::LegacyDict),
                );
            }
        }

        // §12.3.2.3 + §7.9.6, PDF 1.2: a name tree.
        if let Some(tree) = catalog
            .get(b"Names")
            .map(|value| graph.resolve(value))
            .and_then(Object::as_dict)
            .and_then(|names| names.get(b"Dests").map(|value| graph.resolve(value)))
            .and_then(Object::as_dict)
        {
            let mut budget = MAX_NAME_TREE_NODES;
            let mut visited = HashSet::new();
            flatten_name_tree(graph, tree, 0, &mut budget, &mut visited, &mut map);
        }

        Self { map }
    }

    /// How many names are defined across both namespaces.
    fn len(&self) -> usize {
        self.map.len()
    }

    /// The value `key` names and the namespace that defined it, if
    /// either does.
    fn lookup(&self, key: &[u8]) -> Option<&(Object, Namespace)> {
        self.map.get(key)
    }
}

/// Flatten a §7.9.6 name tree into `out`.
///
/// `/Names` alternates key, value, key, value — *"an array of the form
/// `[key₁ value₁ key₂ value₂ … keyₙ valueₙ]`"* — and `/Kids` holds
/// interior nodes. A malformed file can carry both at one node, so both
/// are read wherever present rather than dispatched on; `/Limits` is
/// deliberately **ignored**, because this is a full flatten rather than
/// a binary search and trusting a `/Limits` range that disagrees with
/// its node's contents would drop real entries.
///
/// Bounded three ways — depth, node budget, and a visited set — for the
/// same reason the outline walk is: a `/Kids` array pointing back at an
/// ancestor is trivial to author, and without the visited set every
/// branch would re-walk the whole subtree until the depth guard fired,
/// which is exponential rather than merely bounded.
fn flatten_name_tree<G: ObjectGraph + ?Sized>(
    graph: &G,
    node: &Dict,
    depth: usize,
    budget: &mut usize,
    visited: &mut HashSet<ObjId>,
    out: &mut HashMap<Vec<u8>, (Object, Namespace)>,
) {
    if depth > MAX_NAME_TREE_DEPTH || *budget == 0 {
        return;
    }
    *budget -= 1;

    if let Some(pairs) = node
        .get(b"Names")
        .map(|value| graph.resolve(value))
        .and_then(Object::as_array)
    {
        for pair in pairs.chunks_exact(2) {
            let (Some(key), Some(value)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            // §7.9.6 says keys "shall be strings", and that they are
            // compared "on a simple byte-by-byte basis" with any
            // self-consistent encoding — which is exactly why the map is
            // keyed by raw bytes and never by decoded text. A file using
            // name objects here is malformed but readable, and both are
            // accepted for the same reason the two namespaces are merged
            // at all.
            let key_bytes = match graph.resolve(key) {
                Object::String(bytes) => bytes.clone(),
                Object::Name(name) => name.as_bytes().to_vec(),
                _ => continue,
            };
            out.insert(key_bytes, (value.clone(), Namespace::NameTree));
        }
    }

    if let Some(kids) = node
        .get(b"Kids")
        .map(|value| graph.resolve(value))
        .and_then(Object::as_array)
    {
        for kid in kids {
            if let Some(id) = kid.as_reference()
                && !visited.insert(id)
            {
                continue;
            }
            if let Some(dict) = graph.resolve(kid).as_dict() {
                flatten_name_tree(graph, dict, depth + 1, budget, visited, out);
            }
        }
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
    use std::collections::BTreeMap;

    /// A hand-built graph, so the traversal's guards can be tested on
    /// shapes no fixture generator should have to express — a `/First`
    /// pointing at an object of the wrong type, for instance.
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

    /// Build a dictionary from `(key, value)` pairs.
    fn dict(entries: Vec<(&[u8], Object)>) -> Dict {
        let mut d = Dict::new();
        for (key, value) in entries {
            d.insert(Name::from(key), value);
        }
        d
    }

    fn reference(num: u32) -> Object {
        Object::Reference(ObjId::new(num, 0))
    }

    /// A one-page document whose catalog points at outline object 10,
    /// with `extra` objects laid on top.
    fn graph_with(extra: Vec<(u32, Object)>) -> TestGraph {
        let mut objects = BTreeMap::new();
        objects.insert(
            ObjId::new(1, 0),
            Object::Dict(dict(vec![
                (b"Type", Object::Name(Name::from(b"Catalog"))),
                (b"Pages", reference(2)),
                (b"Outlines", reference(10)),
            ])),
        );
        objects.insert(
            ObjId::new(2, 0),
            Object::Dict(dict(vec![
                (b"Type", Object::Name(Name::from(b"Pages"))),
                (b"Kids", Object::Array(vec![reference(3), reference(4)])),
                (b"Count", Object::Integer(2)),
            ])),
        );
        for num in [3u32, 4] {
            objects.insert(
                ObjId::new(num, 0),
                Object::Dict(dict(vec![
                    (b"Type", Object::Name(Name::from(b"Page"))),
                    (b"Parent", reference(2)),
                ])),
            );
        }
        for (num, value) in extra {
            objects.insert(ObjId::new(num, 0), value);
        }
        let mut trailer = Dict::new();
        trailer.insert(Name::from(b"Root"), Object::Reference(ObjId::new(1, 0)));
        TestGraph { objects, trailer }
    }

    /// An outline root dictionary pointing at `first`.
    fn outline_root(first: u32) -> Object {
        Object::Dict(dict(vec![
            (b"Type", Object::Name(Name::from(b"Outlines"))),
            (b"First", reference(first)),
            (b"Last", reference(first)),
        ]))
    }

    /// Would catch: a document with no `/Outlines` being treated as an
    /// error, or as a reason to return a non-empty tree.
    #[test]
    fn a_document_without_an_outline_reads_as_empty_and_faithful() {
        let mut graph = graph_with(vec![]);
        // Remove the /Outlines entry entirely.
        graph.objects.insert(
            ObjId::new(1, 0),
            Object::Dict(dict(vec![
                (b"Type", Object::Name(Name::from(b"Catalog"))),
                (b"Pages", reference(2)),
            ])),
        );
        let outline = read_outline(&graph);
        assert!(outline.items.is_empty());
        assert_eq!(outline.diagnostics.items, 0);
        assert!(outline.diagnostics.is_faithful());
    }

    /// Would catch: `/Count`'s magnitude being mistaken for a child
    /// count, and a positive/negative sign being read backwards. The
    /// table pins BOTH mistakes at once — a reader that returns
    /// `count.abs()` children fails row 1, and one that inverts the sign
    /// fails every row.
    #[test]
    fn count_sign_alone_decides_open_state() {
        // (declared /Count, expected `open`)
        let cases: &[(Option<i64>, bool)] = &[
            (Some(9), true),   // magnitude lies; sign says open
            (Some(1), true),   // the ordinary open case
            (Some(-1), false), // the ordinary closed case
            (Some(-7), false), // magnitude lies; sign says closed
            (Some(0), false),  // zero is not positive => closed
            (None, false),     // absent => defaulted closed
        ];
        for &(count, expected_open) in cases {
            let mut item = vec![
                (b"Title".as_slice(), Object::String(b"Parent".to_vec())),
                (b"First".as_slice(), reference(12)),
                (b"Last".as_slice(), reference(12)),
            ];
            if let Some(value) = count {
                item.push((b"Count".as_slice(), Object::Integer(value)));
            }
            let graph = graph_with(vec![
                (10, outline_root(11)),
                (11, Object::Dict(dict(item))),
                (
                    12,
                    Object::Dict(dict(vec![(
                        b"Title".as_slice(),
                        Object::String(b"Child".to_vec()),
                    )])),
                ),
            ]);
            let outline = read_outline(&graph);
            let parent = &outline.items[0];
            assert_eq!(parent.open, expected_open, "for /Count {count:?}");
            // The real child count comes from the traversal, never from
            // the declared magnitude.
            assert_eq!(parent.children.len(), 1, "for /Count {count:?}");
            assert_eq!(parent.declared_count, count);
        }
        // Only the absent case counts as a defaulted open state.
        let graph = graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Parent".to_vec())),
                    (b"First", reference(12)),
                ])),
            ),
            (12, Object::Dict(dict(vec![]))),
        ]);
        assert_eq!(read_outline(&graph).diagnostics.open_state_defaulted, 1);
    }

    /// Would catch: a leaf with no `/Count` being reported as a
    /// defaulted open state, which would make
    /// `OutlineDiagnostics::is_faithful` false for almost every real
    /// document and train callers to ignore it.
    #[test]
    fn a_childless_item_without_a_count_is_not_a_defect() {
        let graph = graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Leaf".to_vec())),
                    (
                        b"Dest",
                        Object::Array(vec![reference(3), Object::Name(Name::from(b"Fit"))]),
                    ),
                ])),
            ),
        ]);
        let outline = read_outline(&graph);
        assert_eq!(outline.diagnostics.open_state_defaulted, 0);
        assert!(outline.diagnostics.is_faithful());
    }

    /// Would catch: the sibling cycle guard being absent — this test
    /// does not fail, it **hangs**, which is exactly the failure mode a
    /// reader must not have. Also catches a guard that silently breaks
    /// the loop without recording it.
    #[test]
    fn a_next_cycle_terminates_and_is_reported() {
        let graph = graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Ping".to_vec())),
                    (b"Next", reference(12)),
                ])),
            ),
            (
                12,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Pong".to_vec())),
                    (b"Next", reference(11)),
                ])),
            ),
        ]);
        let outline = read_outline(&graph);
        assert_eq!(outline.items.len(), 2);
        assert_eq!(outline.diagnostics.cycles_broken, 1);
        assert!(!outline.diagnostics.is_faithful());
    }

    /// Would catch: a `/First` pointing at its own item recursing until
    /// the stack overflows. The depth guard alone would *bound* this at
    /// 32 frames; only the visited set stops it at one.
    #[test]
    fn a_self_parenting_item_terminates_at_one_level() {
        let graph = graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Ouroboros".to_vec())),
                    (b"First", reference(11)),
                    (b"Count", Object::Integer(1)),
                ])),
            ),
        ]);
        let outline = read_outline(&graph);
        assert_eq!(outline.items.len(), 1);
        assert!(outline.items[0].children.is_empty());
        assert_eq!(outline.diagnostics.cycles_broken, 1);
    }

    /// Would catch: the depth cap silently dropping a subtree without
    /// saying so, or the cap being applied one level early/late.
    #[test]
    fn nesting_past_the_cap_is_truncated_and_reported() {
        // A chain of MAX_OUTLINE_DEPTH + 5 nested items, objects 11..
        let mut extra = vec![(10u32, outline_root(11))];
        let levels = MAX_OUTLINE_DEPTH + 5;
        for depth in 0..levels {
            let num = 11 + u32::try_from(depth).unwrap();
            let mut entries = vec![(
                b"Title".as_slice(),
                Object::String(format!("L{depth}").into_bytes()),
            )];
            if depth + 1 < levels {
                entries.push((b"First".as_slice(), reference(num + 1)));
                entries.push((b"Count".as_slice(), Object::Integer(1)));
            }
            extra.push((num, Object::Dict(dict(entries))));
        }
        let outline = read_outline(&graph_with(extra));

        // Walk down and confirm exactly MAX_OUTLINE_DEPTH levels exist.
        let mut node = &outline.items[0];
        let mut seen = 1usize;
        while let Some(child) = node.children.first() {
            node = child;
            seen += 1;
        }
        assert_eq!(seen, MAX_OUTLINE_DEPTH);
        assert_eq!(node.level, MAX_OUTLINE_DEPTH - 1);
        assert_eq!(outline.diagnostics.depth_truncations, 1);
        assert_eq!(outline.diagnostics.max_depth, MAX_OUTLINE_DEPTH - 1);
        assert!(!outline.diagnostics.is_faithful());
    }

    /// Would catch: [`Outline::visible_item_count`] counting a closed
    /// item's descendants (it must not) or skipping the closed item
    /// itself (it must not).
    ///
    /// The two errors move the total in opposite directions and would
    /// cancel out on a tree with one closed leaf-parent, so the table
    /// varies the shape until they cannot.
    #[test]
    fn visible_item_count_stops_at_a_closed_node_but_counts_it() {
        // (parent open?, grandchild present?, expected visible total)
        //
        // Shape: root -> Parent -> Child -> [Grandchild]
        let cases: &[(bool, bool, usize)] = &[
            (true, false, 2),  // Parent + Child
            (false, false, 1), // Parent only; Child hidden
            (true, true, 3),   // Parent + Child + Grandchild (Child open)
            (false, true, 1),  // Parent only; a whole subtree hidden
        ];
        for &(open, grandchild, expected) in cases {
            let mut child = vec![(b"Title".as_slice(), Object::String(b"Child".to_vec()))];
            if grandchild {
                child.push((b"First".as_slice(), reference(13)));
                child.push((b"Count".as_slice(), Object::Integer(1)));
            }
            let outline = read_outline(&graph_with(vec![
                (10, outline_root(11)),
                (
                    11,
                    Object::Dict(dict(vec![
                        (b"Title", Object::String(b"Parent".to_vec())),
                        (b"First", reference(12)),
                        (b"Count", Object::Integer(if open { 1 } else { -1 })),
                    ])),
                ),
                (12, Object::Dict(dict(child))),
                (
                    13,
                    Object::Dict(dict(vec![(
                        b"Title".as_slice(),
                        Object::String(b"Grandchild".to_vec()),
                    )])),
                ),
            ]));
            assert_eq!(
                outline.visible_item_count(),
                expected,
                "parent open={open}, grandchild={grandchild}"
            );
        }
    }

    /// Would catch: the `/Count` magnitude cross-check comparing against
    /// the wrong quantity — immediate children instead of *visible
    /// descendants*, or including the item itself.
    ///
    /// The tree here has a parent with one child that itself has one
    /// child. Open throughout, the parent's true magnitude is **2**;
    /// a reader comparing against immediate children expects 1 and
    /// reports a false disagreement.
    #[test]
    fn the_count_cross_check_compares_visible_descendants() {
        // (parent's declared /Count, expected disagreement)
        let cases: &[(i64, bool)] = &[(2, false), (1, true), (3, true), (-2, false)];
        for &(declared, disagrees) in cases {
            let outline = read_outline(&graph_with(vec![
                (10, outline_root(11)),
                (
                    11,
                    Object::Dict(dict(vec![
                        (b"Title", Object::String(b"Parent".to_vec())),
                        (b"First", reference(12)),
                        (b"Count", Object::Integer(declared)),
                    ])),
                ),
                (
                    12,
                    Object::Dict(dict(vec![
                        (b"Title", Object::String(b"Child".to_vec())),
                        (b"First", reference(13)),
                        (b"Count", Object::Integer(1)),
                    ])),
                ),
                (
                    13,
                    Object::Dict(dict(vec![(
                        b"Title".as_slice(),
                        Object::String(b"Grandchild".to_vec()),
                    )])),
                ),
            ]));
            assert_eq!(
                outline.diagnostics.count_disagreements,
                usize::from(disagrees),
                "for /Count {declared}"
            );
            // A CLOSED parent (-2) still declares the same magnitude:
            // the count is what WOULD be visible if it were reopened.
            assert_eq!(outline.items[0].open, declared > 0);
        }
    }

    /// Would catch: the cross-check firing on a tree the reader itself
    /// truncated, which would report pdfcer's own guard rails as document
    /// corruption — noise exactly when the operator needs signal.
    #[test]
    fn the_count_cross_check_is_suppressed_on_a_truncated_tree() {
        // A sibling cycle: the tree is short by construction, and the
        // parent's /Count of 5 would "disagree" for that reason alone.
        let outline = read_outline(&graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Parent".to_vec())),
                    (b"First", reference(12)),
                    (b"Count", Object::Integer(5)),
                ])),
            ),
            (
                12,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Loop".to_vec())),
                    (b"Next", reference(12)),
                ])),
            ),
        ]));
        assert_eq!(outline.diagnostics.cycles_broken, 1);
        assert_eq!(
            outline.diagnostics.count_disagreements, 0,
            "a truncated traversal cannot honestly disagree with a count"
        );
        assert!(!outline.diagnostics.root_count_disagreement);
        // But the tree is still reported as unfaithful, via the cycle.
        assert!(!outline.diagnostics.is_faithful());
    }

    /// Would catch: a sixth [`Destination`] variant shipping without the
    /// consuming shells being told.
    ///
    /// # ★★ This test asserts NOTHING about correctness, deliberately
    ///
    /// It is a **tripwire**, not a check. The count it pins carries no
    /// meaning; changing it is not a failure and the fix is one line. What
    /// it buys is that the line gets edited by somebody who has just read
    /// why it exists.
    ///
    /// The problem it guards is invisible from inside this workspace.
    /// [`Destination`] is `#[non_exhaustive]`, so a downstream `match`
    /// carries a catch-all, and a new variant lands there **without
    /// breaking that consumer's build** — it silently inherits a sentence
    /// written about something else. `pdfcer-gui`'s catch-all currently
    /// reads *"this link has no destination at all"*, which would be an
    /// actively false statement about, say, an embedded-file target.
    ///
    /// No compiler warning, no gate and no test in this repository can see
    /// that. The only mechanism available is to make the *addition* stop
    /// and say something, which is what this does.
    #[test]
    fn variant_count_is_pinned_so_a_new_one_cannot_ship_unannounced() {
        // Exhaustive on purpose -- no `_` arm. Adding a variant makes this
        // match fail to compile, which is the loudest half of the tripwire;
        // the count below is the half that carries the instruction.
        let probe = Destination::Named { name: Vec::new() };
        let count = match probe {
            Destination::Page { .. } => 1,
            Destination::UnmappedPage { .. } => 2,
            Destination::Named { .. } => 3,
            Destination::Remote { .. } => 4,
            Destination::NonNavigation { .. } => 5,
        };
        assert_eq!(
            count, 3,
            "this arm's own number, so the match above cannot be reduced to a stub"
        );
        assert_eq!(
            DESTINATION_VARIANTS, 5,
            "A Destination variant was added or removed.\n\
             \n\
             Update this number -- and then ANNOUNCE IT on the feature-request\n\
             channel at D:\\Dev\\FeatureRequests\\pdfce_FeatureRequests\\.\n\
             \n\
             Destination is #[non_exhaustive], so every downstream match has a\n\
             catch-all. A new variant does NOT break their build -- it lands in\n\
             that catch-all and inherits a sentence written about something\n\
             else. pdfcer-gui's currently reads \"this link has no destination at\n\
             all\". Nothing in this repository can observe that, which is why\n\
             this tripwire exists."
        );
    }

    /// Would catch: a name-tree value that wraps a **go-to action**
    /// rather than a `/D` array (§12.3.2.3 NOTE 2) being treated as
    /// unresolvable, which would silently break a whole class of
    /// producer's bookmarks.
    #[test]
    fn a_named_destination_may_wrap_a_goto_action() {
        let mut graph = graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Wrapped".to_vec())),
                    (b"Dest", Object::String(b"via-action".to_vec())),
                ])),
            ),
            (
                20,
                Object::Dict(dict(vec![(
                    b"Names",
                    Object::Array(vec![
                        Object::String(b"via-action".to_vec()),
                        Object::Dict(dict(vec![(
                            b"A",
                            Object::Dict(dict(vec![
                                (b"S", Object::Name(Name::from(b"GoTo"))),
                                (
                                    b"D",
                                    Object::Array(vec![
                                        reference(4),
                                        Object::Name(Name::from(b"FitB")),
                                    ]),
                                ),
                            ])),
                        )])),
                    ]),
                )])),
            ),
        ]);
        graph.objects.insert(
            ObjId::new(1, 0),
            Object::Dict(dict(vec![
                (b"Type", Object::Name(Name::from(b"Catalog"))),
                (b"Pages", reference(2)),
                (b"Outlines", reference(10)),
                (
                    b"Names",
                    Object::Dict(dict(vec![(b"Dests", reference(20))])),
                ),
            ])),
        );
        let outline = read_outline(&graph);
        assert_eq!(
            outline.items[0].destination,
            Some(Destination::Page {
                page_index: 1,
                view: DestView::FitB,
            })
        );
    }

    /// Would catch: a destination name resolving across §12.3.2.3's two
    /// namespaces without the leniency being disclosed — spec ambiguity
    /// `DEST-A1`.
    ///
    /// The reference's *type* is the only discriminator the spec gives:
    /// a string should mean the name tree. Here a string finds a key the
    /// legacy dictionary defines. pdfcer resolves it (so the file works)
    /// and says it did (so the operator knows a stricter reader may not).
    #[test]
    fn a_cross_namespace_name_resolves_and_is_disclosed() {
        // (how /Dest spells the key, expected cross-namespace count)
        let cases: &[(Object, usize)] = &[
            // A NAME finding a legacy-dictionary key: as the type says.
            (Object::Name(Name::from(b"intro")), 0),
            // A STRING finding that same legacy-dictionary key: crossed.
            (Object::String(b"intro".to_vec()), 1),
        ];
        for (dest, expected) in cases {
            let mut graph = graph_with(vec![
                (10, outline_root(11)),
                (
                    11,
                    Object::Dict(dict(vec![
                        (b"Title", Object::String(b"Intro".to_vec())),
                        (b"Dest", dest.clone()),
                    ])),
                ),
                (
                    20,
                    Object::Dict(dict(vec![(
                        b"intro",
                        Object::Array(vec![reference(3), Object::Name(Name::from(b"Fit"))]),
                    )])),
                ),
            ]);
            graph.objects.insert(
                ObjId::new(1, 0),
                Object::Dict(dict(vec![
                    (b"Type", Object::Name(Name::from(b"Catalog"))),
                    (b"Pages", reference(2)),
                    (b"Outlines", reference(10)),
                    (b"Dests", reference(20)),
                ])),
            );
            let outline = read_outline(&graph);
            // Either way the bookmark WORKS.
            assert_eq!(outline.items[0].page_index(), Some(0), "for {dest:?}");
            assert_eq!(
                outline.diagnostics.cross_namespace_resolutions, *expected,
                "for {dest:?}"
            );
        }
    }

    /// Would catch: `RemoteTarget::page_index` clamping a negative page
    /// number to zero, which would turn a corrupt remote bookmark into
    /// one that convincingly opens the wrong file's first page.
    #[test]
    fn a_remote_page_number_is_zero_based_and_rejects_negatives() {
        let cases: &[(RemoteTarget, Option<usize>)] = &[
            (RemoteTarget::PageNumber(0), Some(0)),
            (RemoteTarget::PageNumber(7), Some(7)),
            (RemoteTarget::PageNumber(-1), None),
            (RemoteTarget::Named(b"x".to_vec()), None),
            (RemoteTarget::Unknown, None),
        ];
        for (target, expected) in cases {
            assert_eq!(target.page_index(), *expected, "for {target:?}");
        }
    }

    /// Would catch: an explicit destination's page reference not being
    /// mapped to a 0-based index, or being mapped to the object NUMBER
    /// (3 and 4 here) instead of the index (0 and 1) — a confusion the
    /// fixture's object numbering is chosen to expose.
    #[test]
    fn explicit_destinations_map_to_zero_based_page_indices() {
        let graph = graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"First page".to_vec())),
                    (
                        b"Dest",
                        Object::Array(vec![reference(3), Object::Name(Name::from(b"Fit"))]),
                    ),
                    (b"Next", reference(12)),
                ])),
            ),
            (
                12,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Second page".to_vec())),
                    (
                        b"Dest",
                        Object::Array(vec![reference(4), Object::Name(Name::from(b"Fit"))]),
                    ),
                ])),
            ),
        ]);
        let outline = read_outline(&graph);
        assert_eq!(outline.items[0].page_index(), Some(0));
        assert_eq!(outline.items[1].page_index(), Some(1));
        assert!(outline.diagnostics.is_faithful());
    }

    /// Would catch: a bookmark whose destination names a missing or
    /// non-page object being silently dropped from the tree, or being
    /// reported as pointing at page 0. Brief requirement (1).
    #[test]
    fn a_destination_naming_no_page_survives_as_unmapped() {
        // (destination array, expected page object recorded)
        let cases: Vec<(Vec<Object>, Option<ObjId>)> = vec![
            // Object 99 does not exist at all.
            (
                vec![reference(99), Object::Name(Name::from(b"Fit"))],
                Some(ObjId::new(99, 0)),
            ),
            // Object 1 exists but is the catalog, not a page.
            (
                vec![reference(1), Object::Name(Name::from(b"Fit"))],
                Some(ObjId::new(1, 0)),
            ),
            // Element 0 is not a reference at all (§12.3.2.2 violation).
            (
                vec![Object::Integer(0), Object::Name(Name::from(b"Fit"))],
                None,
            ),
            // An empty array: no page, and no fit style either.
            (vec![], None),
        ];
        for (array, expected_page) in cases {
            let graph = graph_with(vec![
                (10, outline_root(11)),
                (
                    11,
                    Object::Dict(dict(vec![
                        (b"Title", Object::String(b"Broken".to_vec())),
                        (b"Dest", Object::Array(array.clone())),
                    ])),
                ),
            ]);
            let outline = read_outline(&graph);
            // The bookmark is still there.
            assert_eq!(outline.items.len(), 1, "for {array:?}");
            assert_eq!(outline.items[0].title, "Broken");
            match &outline.items[0].destination {
                Some(Destination::UnmappedPage { page, .. }) => {
                    assert_eq!(*page, expected_page, "for {array:?}");
                }
                other => panic!("expected UnmappedPage for {array:?}, got {other:?}"),
            }
            assert_eq!(outline.diagnostics.unmapped_pages, 1);
        }
    }

    /// Would catch: a named destination that neither namespace defines
    /// being discarded instead of preserved. Brief requirement (2).
    #[test]
    fn an_unresolvable_name_is_kept_not_dropped() {
        let graph = graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Ghost".to_vec())),
                    (b"Dest", Object::String(b"nowhere".to_vec())),
                ])),
            ),
        ]);
        let outline = read_outline(&graph);
        assert_eq!(
            outline.items[0].destination,
            Some(Destination::Named {
                name: b"nowhere".to_vec()
            })
        );
        assert_eq!(outline.diagnostics.unresolved_names, 1);
        assert_eq!(
            outline.items[0].destination.as_ref().unwrap().name_lossy(),
            Some("nowhere".to_string())
        );
    }

    /// Would catch: a two-name destination cycle looping, or exhausting
    /// the hop budget instead of terminating at the repeat.
    #[test]
    fn a_named_destination_cycle_terminates() {
        let mut graph = graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Loop".to_vec())),
                    (b"Dest", Object::String(b"a".to_vec())),
                ])),
            ),
            (
                20,
                Object::Dict(dict(vec![
                    (b"a", Object::String(b"b".to_vec())),
                    (b"b", Object::String(b"a".to_vec())),
                ])),
            ),
        ]);
        graph.objects.insert(
            ObjId::new(1, 0),
            Object::Dict(dict(vec![
                (b"Type", Object::Name(Name::from(b"Catalog"))),
                (b"Pages", reference(2)),
                (b"Outlines", reference(10)),
                (b"Dests", reference(20)),
            ])),
        );
        let outline = read_outline(&graph);
        // Terminates at the repeat, reporting the LAST name tried.
        assert!(matches!(
            outline.items[0].destination,
            Some(Destination::Named { .. })
        ));
        assert_eq!(outline.diagnostics.named_destinations_defined, 2);
    }

    /// Would catch: `/GoToR`'s `/D` being resolved against THIS
    /// document's name table — the silent-wrongness case where a remote
    /// bookmark navigates the wrong file convincingly.
    #[test]
    fn a_remote_destination_never_resolves_against_this_documents_names() {
        let mut graph = graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Elsewhere".to_vec())),
                    (
                        b"A",
                        Object::Dict(dict(vec![
                            (b"S", Object::Name(Name::from(b"GoToR"))),
                            (b"F", Object::String(b"other.pdf".to_vec())),
                            // A name this document DOES define.
                            (b"D", Object::String(b"shared".to_vec())),
                            (b"NewWindow", Object::Boolean(true)),
                        ])),
                    ),
                ])),
            ),
            (
                20,
                Object::Dict(dict(vec![(
                    b"shared",
                    Object::Array(vec![reference(4), Object::Name(Name::from(b"Fit"))]),
                )])),
            ),
        ]);
        graph.objects.insert(
            ObjId::new(1, 0),
            Object::Dict(dict(vec![
                (b"Type", Object::Name(Name::from(b"Catalog"))),
                (b"Pages", reference(2)),
                (b"Outlines", reference(10)),
                (b"Dests", reference(20)),
            ])),
        );
        let outline = read_outline(&graph);
        match &outline.items[0].destination {
            Some(Destination::Remote {
                file,
                target,
                new_window,
                ..
            }) => {
                assert_eq!(file.as_deref(), Some(b"other.pdf".as_slice()));
                assert_eq!(*target, RemoteTarget::Named(b"shared".to_vec()));
                assert_eq!(*new_window, Some(true));
            }
            other => panic!("expected Remote, got {other:?}"),
        }
        // And crucially: NOT resolved to this document's page 1.
        assert_eq!(outline.items[0].page_index(), None);
    }

    /// Would catch: a non-navigation action being reported as a broken
    /// bookmark (destination `None`) rather than as a disclosed action,
    /// or — far worse — being treated as a navigation.
    #[test]
    fn non_navigation_actions_are_named_not_executed() {
        for subtype in [b"URI".as_slice(), b"JavaScript", b"Launch", b"Named"] {
            let graph = graph_with(vec![
                (10, outline_root(11)),
                (
                    11,
                    Object::Dict(dict(vec![
                        (b"Title", Object::String(b"Action".to_vec())),
                        (
                            b"A",
                            Object::Dict(dict(vec![(b"S", Object::Name(Name::from(subtype)))])),
                        ),
                    ])),
                ),
            ]);
            let outline = read_outline(&graph);
            assert_eq!(
                outline.items[0].destination,
                Some(Destination::NonNavigation {
                    action: Some(Name::from(subtype))
                }),
                "for /S /{}",
                String::from_utf8_lossy(subtype)
            );
            assert_eq!(outline.items[0].page_index(), None);
        }
    }

    /// Would catch: the `/Dest`-over-`/A` precedence drifting away from
    /// `pageops::references::resolve_target`, which would let the
    /// bookmarks panel and the page-delete census disagree about where
    /// one bookmark points.
    #[test]
    fn dest_wins_over_a_and_the_conflict_is_reported() {
        let graph = graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Contested".to_vec())),
                    // /Dest -> page index 0
                    (
                        b"Dest",
                        Object::Array(vec![reference(3), Object::Name(Name::from(b"Fit"))]),
                    ),
                    // /A -> page index 1
                    (
                        b"A",
                        Object::Dict(dict(vec![
                            (b"S", Object::Name(Name::from(b"GoTo"))),
                            (
                                b"D",
                                Object::Array(vec![reference(4), Object::Name(Name::from(b"Fit"))]),
                            ),
                        ])),
                    ),
                ])),
            ),
        ]);
        let outline = read_outline(&graph);
        assert_eq!(outline.items[0].page_index(), Some(0));
        assert_eq!(outline.diagnostics.dest_and_action_both_present, 1);
    }

    /// Would catch: destination view parameters being read at the wrong
    /// array offsets — the classic off-by-one where `/FitH`'s `top` is
    /// taken from the fit-name slot — and `/XYZ`'s `null` being turned
    /// into `0.0` instead of "retain".
    #[test]
    fn view_parameters_are_read_positionally() {
        let name = |bytes: &[u8]| Object::Name(Name::from(bytes));
        let num = |value: f64| Object::Real(value);
        let cases: Vec<(Vec<Object>, DestView)> = vec![
            (vec![reference(3), name(b"Fit")], DestView::Fit),
            (vec![reference(3), name(b"FitB")], DestView::FitB),
            (
                vec![reference(3), name(b"FitH"), num(700.0)],
                DestView::FitH { top: Some(700.0) },
            ),
            (
                vec![reference(3), name(b"FitV"), num(40.0)],
                DestView::FitV { left: Some(40.0) },
            ),
            (
                vec![reference(3), name(b"FitBH"), num(12.0)],
                DestView::FitBH { top: Some(12.0) },
            ),
            (
                vec![reference(3), name(b"FitBV"), num(13.0)],
                DestView::FitBV { left: Some(13.0) },
            ),
            (
                vec![
                    reference(3),
                    name(b"XYZ"),
                    num(72.0),
                    num(720.0),
                    Object::Null,
                ],
                DestView::Xyz {
                    left: Some(72.0),
                    top: Some(720.0),
                    zoom: None,
                },
            ),
            (
                vec![
                    reference(3),
                    name(b"FitR"),
                    num(10.0),
                    num(20.0),
                    num(300.0),
                    num(400.0),
                ],
                DestView::FitR {
                    left: Some(10.0),
                    bottom: Some(20.0),
                    right: Some(300.0),
                    top: Some(400.0),
                },
            ),
            (
                vec![reference(3), name(b"FitSideways")],
                DestView::Unknown {
                    fit: Name::from(b"FitSideways"),
                },
            ),
            (vec![reference(3)], DestView::Absent),
        ];
        for (array, expected) in cases {
            let graph = graph_with(vec![
                (10, outline_root(11)),
                (
                    11,
                    Object::Dict(dict(vec![
                        (b"Title", Object::String(b"V".to_vec())),
                        (b"Dest", Object::Array(array.clone())),
                    ])),
                ),
            ]);
            let outline = read_outline(&graph);
            let view = match &outline.items[0].destination {
                Some(Destination::Page { view, .. }) => view.clone(),
                Some(Destination::UnmappedPage { view, .. }) => view.clone(),
                other => panic!("expected a page destination for {array:?}, got {other:?}"),
            };
            assert_eq!(view, expected, "for {array:?}");
        }
    }

    /// Would catch: `DestView::rect` inventing a default for a `/FitR`
    /// whose array was short, which would scroll the viewer to a
    /// plausible but wrong rectangle.
    #[test]
    fn a_short_fitr_yields_no_rectangle_and_is_reported_malformed() {
        let graph = graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Short".to_vec())),
                    (
                        b"Dest",
                        Object::Array(vec![
                            reference(3),
                            Object::Name(Name::from(b"FitR")),
                            Object::Real(10.0),
                            Object::Real(20.0),
                        ]),
                    ),
                ])),
            ),
        ]);
        let outline = read_outline(&graph);
        let Some(Destination::Page { view, .. }) = &outline.items[0].destination else {
            panic!("expected a resolved page destination");
        };
        assert_eq!(view.rect(), None);
        assert_eq!(outline.diagnostics.malformed_views, 1);
    }

    /// Would catch: `/Title` being taken as raw bytes rather than a
    /// §7.9.2 text string — which shows up as mojibake for UTF-16BE
    /// titles and as the wrong character for PDFDocEncoding's
    /// non-Latin-1 codes — and an undecodable byte being hidden.
    #[test]
    fn titles_decode_as_text_strings_and_disclose_inexactness() {
        // (raw /Title bytes, expected text, expected `exact`)
        let cases: &[(&[u8], &str, bool)] = &[
            (b"Plain", "Plain", true),
            // UTF-16BE, discriminated by the FE FF BOM and nothing else.
            (b"\xfe\xff\x00H\x00i", "Hi", true),
            // 0xA0 is EURO in PDFDocEncoding (Annex D.3), NOT a no-break
            // space — a Latin-1 reader gets this wrong and looks fine.
            (b"\xa05", "\u{20AC}5", true),
            // 0xAD is an UNDEFINED PDFDocEncoding code.
            (b"bad\xadbyte", "bad\u{FFFD}byte", false),
        ];
        for &(raw, expected, exact) in cases {
            let graph = graph_with(vec![
                (10, outline_root(11)),
                (
                    11,
                    Object::Dict(dict(vec![(b"Title", Object::String(raw.to_vec()))])),
                ),
            ]);
            let outline = read_outline(&graph);
            assert_eq!(outline.items[0].title, expected, "for {raw:?}");
            assert_eq!(outline.items[0].title_exact, exact, "for {raw:?}");
        }
    }

    /// Would catch: `/F`'s bit POSITIONS (numbered from 1 in Table 153)
    /// being confused with bit VALUES, which would report italic for a
    /// bold-only bookmark.
    #[test]
    fn style_flags_map_to_italic_and_bold() {
        // (/F value, italic, bold)
        let cases: &[(i64, bool, bool)] = &[
            (0, false, false),
            (1, true, false),
            (2, false, true),
            (3, true, true),
            // An unknown high bit must not disturb the two defined ones.
            (0b1001, true, false),
        ];
        for &(flags, italic, bold) in cases {
            let graph = graph_with(vec![
                (10, outline_root(11)),
                (
                    11,
                    Object::Dict(dict(vec![
                        (b"Title", Object::String(b"Styled".to_vec())),
                        (b"F", Object::Integer(flags)),
                        (
                            b"C",
                            Object::Array(vec![
                                Object::Real(1.0),
                                Object::Real(0.5),
                                Object::Integer(0),
                            ]),
                        ),
                    ])),
                ),
            ]);
            let outline = read_outline(&graph);
            assert_eq!(outline.items[0].is_italic(), italic, "for /F {flags}");
            assert_eq!(outline.items[0].is_bold(), bold, "for /F {flags}");
            // An integer component widens to f64 (§7.3.3 NOTE 2).
            assert_eq!(outline.items[0].color, Some([1.0, 0.5, 0.0]));
        }
    }

    /// Would catch: `flatten` visiting the tree in the wrong order, or
    /// `level` not matching the depth the tree actually places an item
    /// at — the two things a flat, indented bookmark list depends on.
    #[test]
    fn flatten_walks_in_document_order_with_correct_levels() {
        let graph = graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"A".to_vec())),
                    (b"First", reference(12)),
                    (b"Count", Object::Integer(2)),
                    (b"Next", reference(14)),
                ])),
            ),
            (
                12,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"A.1".to_vec())),
                    (b"Next", reference(13)),
                ])),
            ),
            (
                13,
                Object::Dict(dict(vec![(b"Title", Object::String(b"A.2".to_vec()))])),
            ),
            (
                14,
                Object::Dict(dict(vec![(b"Title", Object::String(b"B".to_vec()))])),
            ),
        ]);
        let outline = read_outline(&graph);
        let flat = outline.flatten();
        let seen: Vec<(&str, usize)> = flat
            .iter()
            .map(|item| (item.title.as_str(), item.level))
            .collect();
        assert_eq!(
            seen,
            vec![("A", 0), ("A.1", 1), ("A.2", 1), ("B", 0)],
            "document order, with children between their parent and its next sibling"
        );
        assert_eq!(outline.diagnostics.items, 4);
        assert_eq!(outline.diagnostics.max_depth, 1);
    }

    /// Would catch: an outline chain that runs through a dangling
    /// reference aborting the whole read, or silently ending without
    /// distinguishing itself from a chain that genuinely finished.
    #[test]
    fn a_dangling_link_truncates_the_chain_and_is_reported() {
        let graph = graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![
                    (b"Title", Object::String(b"Real".to_vec())),
                    // Object 77 is never defined.
                    (b"Next", reference(77)),
                ])),
            ),
        ]);
        let outline = read_outline(&graph);
        assert_eq!(outline.items.len(), 1);
        assert_eq!(outline.diagnostics.unreadable_items, 1);
        assert!(!outline.diagnostics.is_faithful());
    }

    /// Would catch: an `/Outlines` that is not a dictionary (a stray
    /// array, a number) producing anything other than an empty tree.
    #[test]
    fn a_non_dictionary_outlines_entry_reads_as_empty() {
        let graph = graph_with(vec![(10, Object::Integer(5))]);
        let outline = read_outline(&graph);
        assert!(outline.items.is_empty());
    }

    /// Would catch: `parse_outline` diverging from `read_outline`'s
    /// tree, which would let a caller that used the convenience get a
    /// different answer from one that read the diagnostics.
    #[test]
    fn parse_outline_returns_read_outlines_items() {
        let graph = graph_with(vec![
            (10, outline_root(11)),
            (
                11,
                Object::Dict(dict(vec![(b"Title", Object::String(b"Only".to_vec()))])),
            ),
        ]);
        assert_eq!(parse_outline(&graph), read_outline(&graph).items);
    }
}

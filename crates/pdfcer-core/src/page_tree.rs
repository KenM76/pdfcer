//! # Page tree walk + attribute inheritance (ISO 32000-1 §7.7.3, §7.9.5)
//!
//! Flattens the document's page tree into ordered, fully-resolved
//! [`Page`]s. Spec sources: `iso32000__s__7.7.3.md` (tree structure,
//! Tables 29/30, the inheritance rules) and `iso32000__s__7.9.5.md`
//! (rectangle corner normalization) in the PDF-spec RAG. Clause
//! numbers are ISO 32000-1:2008.
//!
//! ## The four inheritable attributes (§7.7.3.4 — exactly four)
//!
//! `Resources`, `MediaBox`, `CropBox`, `Rotate` — verified in the RAG
//! by exhaustive search of Table 30's inheritable markers. Nothing
//! else inherits; in particular `BleedBox`/`TrimBox`/`ArtBox` do NOT
//! walk ancestors (they default to the resolved `CropBox`).
//!
//! Resolution, per attribute (§7.7.3.4):
//! - `Resources`: own → nearest ancestor; **required** to resolve.
//!   **Present-but-empty `<< >>` means "no resources" and STOPS
//!   inheritance — different from absent (absent = inherit).** This
//!   normative distinction is a common implementation bug (RAG note).
//! - `MediaBox`: own → nearest ancestor; required to resolve.
//! - `CropBox`: own → nearest ancestor → default = resolved `MediaBox`.
//! - `Rotate`: own → nearest ancestor → default = 0. Shall be a
//!   multiple of 90; negative multiples are legal and normalize via
//!   positive modulo (`-90` → `270`).
//!
//! Inheritance is resolved **top-down during the walk** (O(n), needs
//! no `Parent` chasing), not bottom-up via `/Parent`.
//!
//! ## Structure tolerance and guards
//!
//! "Conforming products shall be prepared to handle any form of tree
//! structure" (§7.7.3.1) — so no shape assumptions. Node kind is
//! dispatched on `/Type` where present, falling back to structural
//! detection (`Kids` present ⇒ intermediate node) where absent —
//! Table 29/30 mark `/Type` Required, so absence is malformed but
//! recoverable, and §7.3.7 says type is "almost always inferable from
//! context". `Count` is a cross-check only, never trusted — a damaged
//! `Count` must not truncate the walk (RAG guard note). `Kids` cycles
//! and hostile depth are guarded ([`MAX_TREE_DEPTH`], [`MAX_PAGES`],
//! plus a visited-set — ARCHITECTURE.md §10 policy values, not spec).

use std::collections::HashSet;

use crate::document::Document;
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};

/// Maximum page-tree nesting depth (pdfcer policy, ARCHITECTURE.md
/// §10): legitimate trees are shallow (balanced fan-out ~25–50); a
/// deeper chain is damage or hostility.
pub const MAX_TREE_DEPTH: usize = 64;

/// Maximum number of pages walked (pdfcer policy, ARCHITECTURE.md
/// §10.1): bounds allocation against a hostile tree. Far beyond any
/// legitimate document.
pub const MAX_PAGES: usize = 1_000_000;

/// A rectangle normalized per §7.9.5: the array `[x1 y1 x2 y2]` gives
/// two diagonally opposite corners **in either order**, so consumers
/// must normalize — this type stores (min, min) → (max, max) always.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Lower-left x (the smaller x).
    pub llx: f64,
    /// Lower-left y (the smaller y).
    pub lly: f64,
    /// Upper-right x (the larger x).
    pub urx: f64,
    /// Upper-right y (the larger y).
    pub ury: f64,
}

impl Rect {
    /// Normalize two arbitrary diagonal corners into a `Rect`
    /// (§7.9.5's corners-in-any-order rule).
    #[must_use]
    pub fn from_corners(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        Self {
            llx: x1.min(x2),
            lly: y1.min(y2),
            urx: x1.max(x2),
            ury: y1.max(y2),
        }
    }

    /// Width (always non-negative by construction).
    #[must_use]
    pub fn width(&self) -> f64 {
        self.urx - self.llx
    }

    /// Height (always non-negative by construction).
    #[must_use]
    pub fn height(&self) -> f64 {
        self.ury - self.lly
    }

    /// Whether `other` lies wholly inside this rectangle, edges included.
    ///
    /// **Inclusive on every edge**, which is the answer the page-boundary
    /// questions want: §14.11.2's boxes routinely coincide exactly (Table
    /// 30 makes `CropBox` *default* to `MediaBox`, and the defaulted case
    /// must not report itself as "outside"), and a page whose crop box
    /// equals its media box is the overwhelmingly common shape.
    ///
    /// Both rectangles are assumed §7.9.5-normalized — true by
    /// construction for anything from [`Rect::from_corners`] or from the
    /// page-tree walk, which is every `Rect` pdfcer produces.
    #[must_use]
    pub fn contains(&self, other: &Self) -> bool {
        other.llx >= self.llx
            && other.lly >= self.lly
            && other.urx <= self.urx
            && other.ury <= self.ury
    }
}

/// One page, fully resolved: every inheritable attribute has its final
/// value, defaults applied.
#[derive(Debug, Clone)]
pub struct Page {
    /// The page object's identity (pages are always reached through
    /// indirect `Kids` references, so this is always known).
    pub id: ObjId,
    /// Resolved `Resources` (§7.8.3) — own, inherited, or the page's
    /// explicit empty dictionary.
    pub resources: Dict,
    /// Resolved `MediaBox`, normalized (§7.9.5).
    pub media_box: Rect,
    /// Resolved `CropBox`, normalized; defaults to `media_box`.
    /// Content is clipped to this at display time (Table 30).
    pub crop_box: Rect,
    /// Resolved `Rotate`, normalized to {0, 90, 180, 270} — clockwise
    /// display rotation (Table 30).
    pub rotate: u16,
    /// The `Contents` streams, in order (Table 30: a single stream or
    /// an array whose streams concatenate; streams are always indirect
    /// per §7.3.8.1, so these are always resolvable ids). Empty =
    /// an empty page (absent `Contents` is NOT an error).
    pub contents: Vec<ObjId>,
    /// How many `Contents` entries named an object that is **not in the
    /// file**, and therefore contribute nothing to this page.
    ///
    /// §7.3.10: "An indirect reference to an undefined object is not an
    /// error; it shall be treated as a reference to the null object."
    /// Table 30 then makes the consequence explicit for this key —
    /// `Contents` is optional, and "if this entry is absent, the page
    /// shall be empty". A dangling element is therefore a page that is
    /// *incomplete*, not a document that is *invalid*, and refusing the
    /// whole document over one would violate the §10 fail-clean posture
    /// (a damaged part must not cost the operator the whole file).
    ///
    /// Non-zero means content the page asked for could not be drawn or
    /// extracted. It is counted rather than silently swallowed because
    /// "fuzzy, never sneaky" applies to omissions as much as to
    /// suggestions: a silently-empty page is indistinguishable from a
    /// genuinely blank one, and the operator would have no way to tell
    /// that text they expected is missing.
    ///
    /// Note what this does **not** cover: an entry of the wrong *type* —
    /// a number, a dictionary, an array element that is not a reference —
    /// is a genuine structural error and still yields
    /// [`PageTreeError::BadContents`]. Only "the reference resolves to
    /// null" degrades.
    pub contents_unresolved: usize,
    /// How many **nested arrays** were flattened out of this page's
    /// `/Contents` on the way in — the count of a specific, recognisable
    /// damage that **pdfcer itself wrote** (`Pass 111.0`).
    ///
    /// # What the damage is, and why the repair is exact rather than a guess
    ///
    /// Until 2026-08-20, appending a content stream to a page whose
    /// `/Contents` was an *indirect reference to an array* wrapped that
    /// reference instead of splicing into it, producing
    /// `/Contents [38 0 R, new]` where `38 0 R` dereferences to
    /// `[7 0 R, 37 0 R]`. Nothing in Table 30 permits an array inside the
    /// `/Contents` array, so this walker rejected the page with
    /// [`PageTreeError::BadContents`] — and the file was one pdfcer had just
    /// written and reported `Ok` for.
    ///
    /// **The repair is deterministic, which is the whole reason it is
    /// allowed.** Flattening `[[a, b], c]` to `[a, b, c]` recovers exactly the
    /// streams the page had, in exactly the order Table 30 concatenates them.
    /// Nothing is inferred, chosen, or approximated — the nesting is pure
    /// noise, and the correct reading is the only reading. Contrast
    /// [`Self::contents_unresolved`], where content genuinely IS missing.
    ///
    /// # It is READ-side only — nothing is rewritten
    ///
    /// This makes a damaged document open, render and extract. It does **not**
    /// repair the file: `ARCHITECTURE.md` §5 forbids normalising structure as
    /// a side effect of reading, and an incremental save with no edits still
    /// emits nothing. The page is repaired in the file only when something
    /// else legitimately rewrites that page dictionary — at which point the
    /// fixed [`append_content_stream`] writes the flat form.
    ///
    /// Non-zero therefore means: *this document was damaged by a pdfcer build
    /// older than `Pass 111.0`, and other readers may still refuse it.* That
    /// is worth surfacing to an operator, which is why it is a counted
    /// disclosure and not a silent kindness.
    pub contents_flattened: usize,
}

/// Page-tree structural errors.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum PageTreeError {
    /// The catalog has no usable `/Pages` entry.
    #[error("catalog /Pages missing or not a dictionary")]
    NoPageTreeRoot,
    /// A `Kids` element resolved to something that is not a
    /// dictionary (including a dangling reference's null).
    #[error("page tree kid is not a dictionary (object {0})")]
    BadKid(ObjId),
    /// A node was visited twice — the tree has a cycle.
    #[error("page tree cycle at object {0}")]
    Cycle(ObjId),
    /// Nesting exceeded [`MAX_TREE_DEPTH`] (pdfcer guard).
    #[error("page tree exceeds MAX_TREE_DEPTH ({MAX_TREE_DEPTH})")]
    TooDeep,
    /// More than [`MAX_PAGES`] leaves (pdfcer guard).
    #[error("page tree exceeds MAX_PAGES ({MAX_PAGES})")]
    TooManyPages,
    /// A required inheritable attribute (`Resources` or `MediaBox`)
    /// resolved nowhere on the path to the root (§7.7.3.4: "a value
    /// shall be supplied in an ancestor node").
    #[error("required page attribute {0} missing on page and all ancestors")]
    MissingRequired(&'static str),
    /// A rectangle entry wasn't an array of four numbers.
    #[error("malformed rectangle in page attribute {0}")]
    BadRectangle(&'static str),
    /// `Rotate` was not an integer multiple of 90 (Table 30 "shall").
    #[error("page /Rotate value {0} is not a multiple of 90")]
    BadRotate(i64),
    /// `Contents` held a value of the wrong **type** — something that is
    /// neither a stream reference, an array of them, nor null.
    ///
    /// Deliberately narrower than it looks: an element that resolves to
    /// **null** (a dangling reference to an object the file does not
    /// contain) is NOT this error. §7.3.10 defines such a reference to be
    /// the null object and Table 30 makes an absent `Contents` an empty
    /// page, so that case degrades — the element contributes nothing and
    /// the omission is counted in [`Page::contents_unresolved`]. This
    /// variant is reserved for values with no spec-sanctioned reading (a
    /// number, a name, a dictionary, a non-reference array element), which
    /// are real structural defects and must not be laundered into a silent
    /// blank page.
    #[error("page /Contents is neither a stream nor an array of streams")]
    BadContents,
}

/// Inheritable-attribute state carried down the walk (raw objects —
/// resolution to `Rect`/`Dict` happens once, at the leaf, so an
/// intermediate node's malformed value only errors if a page actually
/// inherits it).
#[derive(Clone, Default)]
struct Inherited<'a> {
    resources: Option<&'a Object>,
    media_box: Option<&'a Object>,
    crop_box: Option<&'a Object>,
    rotate: Option<&'a Object>,
}

/// Walk the **loaded file's** page tree and return its pages in
/// document order, attributes fully resolved.
///
/// A thin wrapper over [`pages_in`]. Kept as a named function because
/// "the pages of this document" is what the renderer and every read-only
/// consumer actually want, and spelling it `pages_in(doc)` at forty call
/// sites would obscure that the interesting case is the *other* one.
///
/// ⚠️ **This is the base revision, not the edited state.** Anything that
/// must see unsaved structural edits calls
/// [`EditSession::pages`](crate::edit::EditSession::pages), which walks
/// the overlay through the same code.
///
/// # Errors
///
/// [`PageTreeError`] — structural damage, guard violations, or
/// unresolvable required attributes. A well-formed empty tree
/// (`/Count 0`, empty `Kids`) returns an empty vec, not an error.
pub fn pages(doc: &Document) -> Result<Vec<Page>, PageTreeError> {
    pages_in(doc)
}

/// Walk any [`ObjectGraph`]'s page tree and return its pages in document
/// order, attributes fully resolved.
///
/// The generic form exists for exactly one reason, recorded in
/// [`crate::graph`]'s module docs: from Pass 3.2 an edit can change the
/// page tree's **shape**, so the walk must be able to run over the
/// session overlay rather than over the base file. There is deliberately
/// only one walk — a session-specialised copy is how a build eventually
/// renders a page the operator deleted.
///
/// # Errors
///
/// [`PageTreeError`] — as [`pages`].
pub fn pages_in<G: ObjectGraph + ?Sized>(graph: &G) -> Result<Vec<Page>, PageTreeError> {
    let catalog = graph.catalog_dict().ok_or(PageTreeError::NoPageTreeRoot)?;
    let root_obj = catalog
        .get(b"Pages")
        .map(|o| graph.resolve(o))
        .ok_or(PageTreeError::NoPageTreeRoot)?;
    let root = root_obj.as_dict().ok_or(PageTreeError::NoPageTreeRoot)?;

    // The root's id (for cycle tracking) if it was reached by
    // reference — it always is in well-formed files (`/Pages` "shall
    // be an indirect reference", Table 15/28).
    let root_id = catalog.get(b"Pages").and_then(Object::as_reference);

    let mut out = Vec::new();
    let mut visited: HashSet<ObjId> = root_id.into_iter().collect();
    walk(graph, root, Inherited::default(), 0, &mut visited, &mut out)?;
    Ok(out)
}

/// One page's **structural** position in the tree: which node holds it,
/// where in that node's `Kids`, and what its ancestors are.
///
/// [`Page`] answers *"what does this page look like?"*; this answers
/// *"where does this page live?"*, which is the only question a
/// structural operation asks. Splitting them keeps the renderer's type
/// free of bookkeeping it has no use for, and keeps this type free of
/// the expensive attribute resolution a delete does not need.
///
/// `inherited` carries the **raw, unresolved** values an ancestor
/// supplies for the four inheritable attributes (§7.7.3.4), and it is
/// the reason this type is worth building at all. When a page moves to a
/// different parent — a reorder across nodes, an extract into a fresh
/// document — the attributes it used to inherit may no longer reach it.
/// Writing the *raw* value back onto the page (usually a single
/// reference) preserves rendering exactly, whereas writing a *resolved*
/// value would inline an entire resource dictionary and change bytes far
/// beyond the edit.
#[derive(Debug, Clone)]
pub struct PageSlot {
    /// The page object's identity.
    pub id: ObjId,
    /// The `Pages` node whose `Kids` array holds this page, if the walk
    /// reached that node by reference (it always does in a well-formed
    /// file — Table 29 requires `Kids` elements to be indirect).
    pub parent: Option<ObjId>,
    /// This page's index within `parent`'s `Kids` array.
    pub index_in_parent: usize,
    /// Every ancestor `Pages` node, **root first**, excluding the page
    /// itself. A delete must decrement `/Count` on all of them.
    pub ancestors: Vec<ObjId>,
    /// What the ancestors supply for the four inheritable attributes,
    /// unresolved. `None` for an attribute no ancestor sets.
    pub inherited: InheritedRaw,
}

/// The raw inheritable-attribute values an ancestor chain supplies
/// (§7.7.3.4 — exactly four attributes; see the module docs).
///
/// Deliberately **not** merged with the page's own entries: the whole
/// point is to know what would be *lost* if the page left this chain.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InheritedRaw {
    /// Nearest ancestor `/Resources`.
    pub resources: Option<Object>,
    /// Nearest ancestor `/MediaBox`.
    pub media_box: Option<Object>,
    /// Nearest ancestor `/CropBox`.
    pub crop_box: Option<Object>,
    /// Nearest ancestor `/Rotate`.
    pub rotate: Option<Object>,
}

impl InheritedRaw {
    /// The `(key, value)` pairs a page would need written onto it to
    /// keep these attributes after leaving this ancestor chain.
    ///
    /// Only entries the page does not already carry itself are
    /// returned, because a page's own entry always wins (§7.7.3.4) and
    /// restating it would modify an object pdfcer was not asked to
    /// modify (§5).
    #[must_use]
    pub fn materialize_for(&self, page: &Dict) -> Vec<(&'static [u8], Object)> {
        let mut out = Vec::new();
        for (key, value) in [
            (&b"Resources"[..], &self.resources),
            (&b"MediaBox"[..], &self.media_box),
            (&b"CropBox"[..], &self.crop_box),
            (&b"Rotate"[..], &self.rotate),
        ] {
            // `contains_key` collapses a null-valued entry to absent
            // (§7.3.7), which is the right test: a page carrying
            // `/Rotate null` inherits, exactly as one with no entry does.
            if let Some(v) = value
                && !page.contains_key(key)
            {
                out.push((key, v.clone()));
            }
        }
        out
    }
}

/// Walk the page tree recording each page's **structural position**
/// rather than its resolved appearance.
///
/// Same traversal, same guards, same tolerance as [`pages_in`] — and
/// deliberately a separate function rather than an extra field on
/// [`Page`], because resolving `Resources`/`MediaBox` can *fail* for a
/// damaged file (`MissingRequired`) and a structural operation should
/// not be blocked by an attribute it never reads. A page with no
/// `MediaBox` anywhere is still a page that can be deleted.
///
/// # Errors
///
/// [`PageTreeError`] — structural damage or a guard violation. Notably
/// **not** [`PageTreeError::MissingRequired`], which this walk cannot
/// produce.
pub fn page_slots<G: ObjectGraph + ?Sized>(graph: &G) -> Result<Vec<PageSlot>, PageTreeError> {
    let catalog = graph.catalog_dict().ok_or(PageTreeError::NoPageTreeRoot)?;
    let root_id = catalog
        .get(b"Pages")
        .and_then(Object::as_reference)
        .ok_or(PageTreeError::NoPageTreeRoot)?;
    let root = graph
        .resolved(root_id)
        .as_dict()
        .ok_or(PageTreeError::NoPageTreeRoot)?;

    let mut out = Vec::new();
    let mut visited: HashSet<ObjId> = [root_id].into_iter().collect();
    walk_slots(
        graph,
        root_id,
        root,
        &InheritedRaw::default(),
        &[],
        0,
        &mut visited,
        &mut out,
    )?;
    Ok(out)
}

/// Recursive half of [`page_slots`].
#[allow(clippy::too_many_arguments)] // every argument is walk state; a
// struct here would only rename the same eight values.
fn walk_slots<G: ObjectGraph + ?Sized>(
    graph: &G,
    node_id: ObjId,
    node: &Dict,
    inherited: &InheritedRaw,
    ancestors: &[ObjId],
    depth: usize,
    visited: &mut HashSet<ObjId>,
    out: &mut Vec<PageSlot>,
) -> Result<(), PageTreeError> {
    if depth > MAX_TREE_DEPTH {
        return Err(PageTreeError::TooDeep);
    }
    let here = InheritedRaw {
        resources: node
            .get(b"Resources")
            .cloned()
            .or_else(|| inherited.resources.clone()),
        media_box: node
            .get(b"MediaBox")
            .cloned()
            .or_else(|| inherited.media_box.clone()),
        crop_box: node
            .get(b"CropBox")
            .cloned()
            .or_else(|| inherited.crop_box.clone()),
        rotate: node
            .get(b"Rotate")
            .cloned()
            .or_else(|| inherited.rotate.clone()),
    };
    let mut chain = ancestors.to_vec();
    chain.push(node_id);

    let Some(kids) = node
        .get(b"Kids")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
    else {
        return Err(PageTreeError::NoPageTreeRoot);
    };

    for (index, kid) in kids.iter().enumerate() {
        let Some(kid_id) = kid.as_reference() else {
            return Err(PageTreeError::BadKid(ObjId::new(0, 0)));
        };
        if !visited.insert(kid_id) {
            return Err(PageTreeError::Cycle(kid_id));
        }
        let Some(kid_dict) = graph.resolved(kid_id).as_dict() else {
            return Err(PageTreeError::BadKid(kid_id));
        };
        if is_pages_node(graph, kid_dict) {
            walk_slots(
                graph,
                kid_id,
                kid_dict,
                &here,
                &chain,
                depth + 1,
                visited,
                out,
            )?;
        } else {
            if out.len() >= MAX_PAGES {
                return Err(PageTreeError::TooManyPages);
            }
            out.push(PageSlot {
                id: kid_id,
                parent: Some(node_id),
                index_in_parent: index,
                ancestors: chain.clone(),
                inherited: here.clone(),
            });
        }
    }
    Ok(())
}

/// Node-kind dispatch: `/Type` where present, structural fallback
/// (`Kids` present ⇒ intermediate node) where absent.
///
/// Shared by both walks so the two cannot disagree about what a node
/// with no `/Type` is — a disagreement that would show up as a page
/// count that changes depending on which walk asked.
fn is_pages_node<G: ObjectGraph + ?Sized>(graph: &G, node: &Dict) -> bool {
    match node
        .get(b"Type")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_name)
        .map(crate::object::Name::as_bytes)
    {
        Some(b"Pages") => true,
        Some(b"Page") => false,
        _ => node.contains_key(b"Kids"),
    }
}

/// Recursive top-down walk (see module docs for why top-down).
fn walk<'a, G: ObjectGraph + ?Sized>(
    doc: &'a G,
    node: &'a Dict,
    inherited: Inherited<'a>,
    depth: usize,
    visited: &mut HashSet<ObjId>,
    out: &mut Vec<Page>,
) -> Result<(), PageTreeError> {
    if depth > MAX_TREE_DEPTH {
        return Err(PageTreeError::TooDeep);
    }

    // Layer this node's own inheritable entries over what came down.
    // Values stay raw here; leaves resolve them (module docs).
    let inherited = Inherited {
        resources: node.get(b"Resources").or(inherited.resources),
        media_box: node.get(b"MediaBox").or(inherited.media_box),
        crop_box: node.get(b"CropBox").or(inherited.crop_box),
        rotate: node.get(b"Rotate").or(inherited.rotate),
    };

    // Node-kind dispatch: /Type where present, structural fallback
    // (Kids ⇒ intermediate) where absent (module docs).
    let type_name = node
        .get(b"Type")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_name)
        .map(crate::object::Name::as_bytes);
    let is_pages_node = match type_name {
        Some(b"Pages") => true,
        Some(b"Page") => false,
        _ => node.contains_key(b"Kids"),
    };

    if !is_pages_node {
        return Err(PageTreeError::BadKid(ObjId::new(0, 0)));
        // unreachable in practice: leaves are emitted by the caller
        // below; kept as a defensive arm for a root that is itself a
        // Page (malformed — a root must be a Pages node).
    }

    let kids = node
        .get(b"Kids")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_array)
        .ok_or(PageTreeError::NoPageTreeRoot)?;

    for kid in kids {
        // Kids "shall only be page objects or other page tree nodes",
        // referenced indirectly (Table 29).
        let Some(kid_id) = kid.as_reference() else {
            return Err(PageTreeError::BadKid(ObjId::new(0, 0)));
        };
        if !visited.insert(kid_id) {
            return Err(PageTreeError::Cycle(kid_id));
        }
        let Some(kid_dict) = doc.resolve(kid).as_dict() else {
            return Err(PageTreeError::BadKid(kid_id));
        };

        let kid_type = kid_dict
            .get(b"Type")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_name)
            .map(crate::object::Name::as_bytes);
        let kid_is_pages = match kid_type {
            Some(b"Pages") => true,
            Some(b"Page") => false,
            _ => kid_dict.contains_key(b"Kids"),
        };

        if kid_is_pages {
            walk(doc, kid_dict, inherited.clone(), depth + 1, visited, out)?;
        } else {
            if out.len() >= MAX_PAGES {
                return Err(PageTreeError::TooManyPages);
            }
            out.push(resolve_page(doc, kid_id, kid_dict, &inherited)?);
        }
    }
    Ok(())
}

/// Resolve one leaf into a [`Page`], applying the §7.7.3.4 resolution
/// order and defaults documented in the module docs.
fn resolve_page<G: ObjectGraph + ?Sized>(
    doc: &G,
    id: ObjId,
    page: &Dict,
    inherited: &Inherited<'_>,
) -> Result<Page, PageTreeError> {
    // Resources: own → ancestor; required. NOTE the empty-vs-absent
    // distinction is preserved automatically here: a page with
    // `/Resources << >>` has an OWN entry (empty dict), which wins
    // over any ancestor value.
    let resources = page
        .get(b"Resources")
        .or(inherited.resources)
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)
        .cloned()
        .ok_or(PageTreeError::MissingRequired("Resources"))?;

    let media_box = page
        .get(b"MediaBox")
        .or(inherited.media_box)
        .ok_or(PageTreeError::MissingRequired("MediaBox"))
        .and_then(|o| parse_rect(doc, o, "MediaBox"))?;

    // CropBox: own → ancestor → default = resolved MediaBox.
    let crop_box = match page.get(b"CropBox").or(inherited.crop_box) {
        Some(o) => parse_rect(doc, o, "CropBox")?,
        None => media_box,
    };

    // Rotate: own → ancestor → 0; multiple of 90; positive modulo.
    let rotate = match page
        .get(b"Rotate")
        .or(inherited.rotate)
        .map(|o| doc.resolve(o))
    {
        None => 0,
        Some(Object::Integer(v)) if v % 90 == 0 => u16::try_from(v.rem_euclid(360)).unwrap_or(0),
        Some(Object::Integer(v)) => return Err(PageTreeError::BadRotate(*v)),
        Some(_) => return Err(PageTreeError::BadRotate(i64::MIN)),
    };

    // Contents: absent = empty page; single stream ref; or array of
    // stream refs (streams are always indirect, §7.3.8.1).
    //
    // The two failure modes are deliberately NOT treated alike:
    //
    //   * A reference that resolves to **null** — because the object is
    //     absent from the file, or because the file explicitly wrote
    //     `null` — is not an error at all. §7.3.10 makes a reference to an
    //     undefined object *be* the null object, and §7.3.9 makes a null
    //     value equivalent to omitting the entry; Table 30 then says an
    //     absent `Contents` means an empty page. So it degrades: that
    //     element contributes nothing, the rest of the page still loads,
    //     and the omission is COUNTED into `contents_unresolved` for
    //     disclosure.
    //   * A value of the wrong **type** — a number, a name, a dictionary,
    //     a direct non-reference array element — is a genuine structural
    //     error with no spec-sanctioned reading, and still fails the page
    //     with `BadContents`. Degrading these too would convert a real
    //     defect into a silent blank page, which is precisely the outcome
    //     "fuzzy, never sneaky" forbids.
    let (contents, contents_unresolved, contents_flattened) = match page.get(b"Contents") {
        None => (Vec::new(), 0, 0),
        // A direct `null` value: §7.3.9 "equivalent to omitting the entry",
        // so this is a well-formed empty page, not a degradation — nothing
        // is missing, so nothing is counted.
        Some(Object::Null) => (Vec::new(), 0, 0),
        Some(entry @ Object::Reference(r)) => match doc.resolve(entry) {
            Object::Stream(_) => (vec![*r], 0, 0),
            // A reference to an ARRAY of streams is also legal
            // (substitutability, §7.3.10) — recurse into the array.
            Object::Array(items) => contents_from_array(doc, items, 0)?,
            // §7.3.10 + Table 30: the whole `Contents` value is the null
            // object, which reads as absent — an empty page, disclosed.
            Object::Null => (Vec::new(), 1, 0),
            _ => return Err(PageTreeError::BadContents),
        },
        Some(Object::Array(items)) => contents_from_array(doc, items, 0)?,
        Some(_) => return Err(PageTreeError::BadContents),
    };

    Ok(Page {
        id,
        resources,
        media_box,
        crop_box,
        rotate,
        contents,
        contents_unresolved,
        contents_flattened,
    })
}

/// The deepest nesting [`contents_from_array`] will flatten before refusing.
///
/// Realistically the damage is ONE level deep — a wrapped reference — and two
/// only if a damaged page was damaged again. The bound is generous rather than
/// tight because its job is to stop unbounded recursion on hostile input, not
/// to second-guess an unusual file (`ARCHITECTURE.md` §10).
const MAX_CONTENTS_NESTING: usize = 8;

/// Collect the stream ids of a `Contents` array, degrading unresolvable
/// elements, FLATTENING nested arrays, and rejecting wrong-typed ones.
///
/// Returns the ids that resolved to real streams, in order, plus the count
/// of elements that resolved to null (§7.3.10) and so contribute nothing.
/// The surviving elements still concatenate in order — Table 30's "the
/// division between streams may occur only at the boundaries between
/// lexical tokens" means a dropped element leaves a *shorter* content
/// stream, never a syntactically broken one.
///
/// # Errors
///
/// [`PageTreeError::BadContents`] if an element is not an indirect
/// reference at all, or resolves to something other than a stream or null
/// — a type error, which stays an error (see the caller's commentary).
fn contents_from_array<G: ObjectGraph + ?Sized>(
    doc: &G,
    items: &[Object],
    depth: usize,
) -> Result<(Vec<ObjId>, usize, usize), PageTreeError> {
    if depth > MAX_CONTENTS_NESTING {
        return Err(PageTreeError::BadContents);
    }
    let mut ids = Vec::with_capacity(items.len());
    let mut unresolved = 0usize;
    let mut flattened = 0usize;
    for item in items {
        match (item.as_reference(), doc.resolve(item)) {
            (Some(id), Object::Stream(_)) => ids.push(id),
            // A reference whose target is missing from the file: §7.3.10
            // says this IS the null object, not an error. Count and skip.
            (Some(_), Object::Null) => unresolved += 1,
            // A DIRECT null element (`[ 4 0 R null 6 0 R ]`): §7.3.9 makes
            // it equivalent to an omitted value. Nothing is missing from
            // the file, so it is skipped WITHOUT counting — the count is
            // reserved for content that should have been there and wasn't.
            (None, Object::Null) => {}
            // ★ A NESTED ARRAY. Not legal, and specifically the damage a
            // pdfcer build older than `Pass 111.0` wrote into any page whose
            // `/Contents` was an indirect reference to an array (see
            // `Page::contents_flattened`). Flattened rather than refused,
            // because the repair is EXACT — the nesting carries no
            // information and the streams inside it are already in the order
            // Table 30 concatenates them — and because refusing costs the
            // operator a document pdfcer itself damaged.
            //
            // Depth-guarded (`MAX_CONTENTS_NESTING`) rather than trusted:
            // this is a recursive walk over attacker-controllable structure,
            // which `ARCHITECTURE.md` §10 requires a bound on. A cycle
            // (`38 0 obj [ 38 0 R ] endobj`) terminates by depth, not by
            // luck.
            (_, Object::Array(inner)) => {
                let (mut nested, unres, flat) = contents_from_array(doc, inner, depth + 1)?;
                ids.append(&mut nested);
                unresolved += unres;
                flattened += flat + 1;
            }
            // Anything else — a number, a dict, a reference to a non-stream
            // — is a type error.
            _ => return Err(PageTreeError::BadContents),
        }
    }
    Ok((ids, unresolved, flattened))
}

/// Append `new_id` to a page's `/Contents`, returning the value to write —
/// **the one place the writer's model of `/Contents` lives** (Table 30).
///
/// # Why this is in `page_tree` and not next to a verb
///
/// It sits beside [`contents_from_array`], the READER, deliberately. The
/// question *"what shapes can `/Contents` take?"* has exactly one answer, and
/// on 2026-08-20 the writer and the reader held **different** ones: the reader
/// accepted a reference-to-an-array (correctly — §7.3.10 substitutability), and
/// the writer wrapped that reference instead of splicing into it, producing an
/// array whose first element dereferenced to another array. Nothing in PDF
/// permits that, so `pages()` rejected pages that `add_image` had just written
/// and returned `Ok` for. **The two halves of this crate disagreed with each
/// other rather than with the spec.**
///
/// ★ **It was written TWICE, and both copies were wrong the same way.**
/// `EditSession::append_page_content` served `add_image` and `flatten_fields`;
/// `text_edit::addtext::append_contents` served `add_text` and the OCR text
/// layer. Neither resolved. That is R92's failure mode — one question answered
/// in two places — and the fix is not "correct both" but "have one".
///
/// # The four shapes, and what each becomes
///
/// | `/Contents` before | after |
/// |---|---|
/// | absent | `[new]` |
/// | `R` → a stream | `[R, new]` — the reference is re-emitted **as written** |
/// | `[a, b]` a direct array | `[a, b, new]` |
/// | `R` → **an array** `[a, b]` | `[a, b, new]` — **spliced, not wrapped** |
///
/// The last row is the fix. `R` is left in the file, now unreferenced from this
/// page; it is deliberately not deleted, because it may be shared with another
/// page and because deleting it would be a second, unrelated mutation. An
/// orphaned array object costs a few bytes and breaks nothing.
///
/// Splicing the ELEMENTS rather than rewriting the array object in place is
/// also deliberate: `R` may be shared between pages (rare but legal), and
/// appending to it would put the new content on every page that names it.
/// Changing only this page's dictionary cannot do that.
///
/// # A malformed `/Contents` is preserved, not dropped
///
/// If the value is neither a stream, an array, nor a reference to either, the
/// page is already one [`pages`] rejects. The old value is still carried into
/// the array rather than discarded — a page that was unreadable stays
/// unreadable, but nothing the operator had is silently thrown away on the way
/// past.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::object::ObjId;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n"
///         .to_vec(),
/// )?;
/// // An absent `/Contents` becomes a one-element array.
/// let appended = pdfcer_core::page_tree::append_content_stream(&doc, None, ObjId::new(9, 0));
/// assert_eq!(appended.as_array().map(<[_]>::len), Some(1));
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn append_content_stream<G: ObjectGraph + ?Sized>(
    graph: &G,
    before: Option<&Object>,
    new_id: ObjId,
) -> Object {
    let Some(before) = before else {
        return Object::Array(vec![Object::Reference(new_id)]);
    };
    // `resolve` follows a reference CHAIN and is depth-guarded (§7.3.10,
    // `MAX_RESOLVE_DEPTH`), which answers the reference-to-a-reference case
    // without this function needing to know it exists.
    match graph.resolve(before) {
        Object::Array(items) => {
            let mut spliced = Vec::with_capacity(items.len() + 1);
            spliced.extend(items.iter().cloned());
            spliced.push(Object::Reference(new_id));
            Object::Array(spliced)
        }
        // A stream, or anything else. `before` is re-emitted verbatim so an
        // indirect reference stays an indirect reference — a stream shall be
        // an indirect object (§7.3.8), so inlining one here would itself be
        // malformed.
        _ => Object::Array(vec![before.clone(), Object::Reference(new_id)]),
    }
}

/// Parse (and resolve) a rectangle attribute: an array of four
/// numbers, each possibly an indirect reference (§7.3.10
/// substitutability), normalized per §7.9.5.
pub(crate) fn parse_rect<G: ObjectGraph + ?Sized>(
    doc: &G,
    obj: &Object,
    attr: &'static str,
) -> Result<Rect, PageTreeError> {
    let arr = doc
        .resolve(obj)
        .as_array()
        .ok_or(PageTreeError::BadRectangle(attr))?;
    let nums: Vec<f64> = arr
        .iter()
        .filter_map(|o| doc.resolve(o).as_number())
        .collect();
    match nums.as_slice() {
        &[x1, y1, x2, y2] => Ok(Rect::from_corners(x1, y1, x2, y2)),
        _ => Err(PageTreeError::BadRectangle(attr)),
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

    /// Same builder as document::tests — duplicated deliberately so
    /// each module's tests stay self-contained and readable.
    fn build_pdf(objects: &[(u32, &str)]) -> Document {
        let mut buf = b"%PDF-1.4\n".to_vec();
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        for (num, body) in objects {
            offsets.push((*num, buf.len()));
            buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        let xref_at = buf.len();
        let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
        buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
        buf.extend_from_slice(b"0000000000 65535 f\r\n");
        for num in 1..=max_num {
            match offsets.iter().find(|(n, _)| *n == num) {
                Some((_, off)) => {
                    buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
                }
                None => buf.extend_from_slice(b"0000000000 65535 f\r\n"),
            }
        }
        buf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
                max_num + 1
            )
            .as_bytes(),
        );
        Document::from_bytes(buf).unwrap()
    }

    #[test]
    fn inheritance_spec_figure_6_style() {
        // Root sets MediaBox + Rotate 90; page 3 overrides Rotate to
        // 270; page 4 has its own MediaBox. Mirrors the §7.7.3.4
        // Figure 6 pattern.
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 \
                 /MediaBox [0 0 612 792] /Rotate 90 /Resources << >> >>",
            ),
            (3, "<< /Type /Page /Parent 2 0 R >>"),
            (4, "<< /Type /Page /Parent 2 0 R /Rotate 270 >>"),
            (5, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>"),
        ]);
        let pages = pages(&doc).unwrap();
        assert_eq!(pages.len(), 3);
        // Page 1: everything inherited.
        assert_eq!(pages[0].media_box.width(), 612.0);
        assert_eq!(pages[0].rotate, 90);
        // CropBox defaulted to the resolved MediaBox.
        assert_eq!(pages[0].crop_box, pages[0].media_box);
        // Page 2: Rotate overridden.
        assert_eq!(pages[1].rotate, 270);
        // Page 3: MediaBox overridden, Rotate still inherited.
        assert_eq!(pages[2].media_box.width(), 200.0);
        assert_eq!(pages[2].rotate, 90);
    }

    #[test]
    fn deep_tree_and_intermediate_inheritance() {
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 \
                 /MediaBox [0 0 100 100] /Resources << >> >>",
            ),
            (
                3,
                "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 /Rotate 180 >>",
            ),
            (4, "<< /Type /Page /Parent 3 0 R >>"),
        ]);
        let pages = pages(&doc).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].rotate, 180);
        assert_eq!(pages[0].media_box.width(), 100.0);
    }

    #[test]
    fn empty_own_resources_beats_inherited() {
        // §7.7.3.3: an explicit empty /Resources means "no resources"
        // — it must NOT fall through to the ancestor's non-empty one.
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 \
                 /MediaBox [0 0 10 10] /Resources << /Font << >> >> >>",
            ),
            (3, "<< /Type /Page /Parent 2 0 R /Resources << >> >>"),
        ]);
        let pages = pages(&doc).unwrap();
        assert!(pages[0].resources.is_empty());
    }

    #[test]
    fn rectangle_corners_normalize_in_any_order() {
        // §7.9.5: corners may come in either order.
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /Resources << >> >>",
            ),
            (3, "<< /Type /Page /Parent 2 0 R /MediaBox [612 792 0 0] >>"),
        ]);
        let pages = pages(&doc).unwrap();
        assert_eq!(pages[0].media_box.llx, 0.0);
        assert_eq!(pages[0].media_box.urx, 612.0);
        assert_eq!(pages[0].media_box.height(), 792.0);
    }

    #[test]
    fn negative_rotate_normalizes_positive() {
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 \
                 /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
            (3, "<< /Type /Page /Parent 2 0 R /Rotate -90 >>"),
        ]);
        assert_eq!(pages(&doc).unwrap()[0].rotate, 270);
    }

    #[test]
    fn non_multiple_of_90_rotate_is_error() {
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 \
                 /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
            (3, "<< /Type /Page /Parent 2 0 R /Rotate 45 >>"),
        ]);
        assert_eq!(pages(&doc).unwrap_err(), PageTreeError::BadRotate(45));
    }

    #[test]
    fn missing_mediabox_everywhere_is_error() {
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /Resources << >> >>",
            ),
            (3, "<< /Type /Page /Parent 2 0 R >>"),
        ]);
        assert_eq!(
            pages(&doc).unwrap_err(),
            PageTreeError::MissingRequired("MediaBox")
        );
    }

    #[test]
    fn kids_cycle_is_detected() {
        // Node 2's Kids points back at node 2.
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [2 0 R] /Count 1 \
                 /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
        ]);
        assert_eq!(
            pages(&doc).unwrap_err(),
            PageTreeError::Cycle(ObjId::new(2, 0))
        );
    }

    #[test]
    fn count_is_not_trusted() {
        // Declared /Count 99 with one real page: the walk wins.
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 99 \
                 /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
            (3, "<< /Type /Page /Parent 2 0 R >>"),
        ]);
        assert_eq!(pages(&doc).unwrap().len(), 1);
    }

    #[test]
    fn contents_single_and_array_and_absent() {
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 \
                 /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
            (3, "<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>"),
            (4, "<< /Type /Page /Parent 2 0 R /Contents [6 0 R 7 0 R] >>"),
            (5, "<< /Type /Page /Parent 2 0 R >>"),
            (6, "<< /Length 2 >>\nstream\nq \nendstream"),
            (7, "<< /Length 2 >>\nstream\nQ \nendstream"),
        ]);
        let pages = pages(&doc).unwrap();
        assert_eq!(pages[0].contents, vec![ObjId::new(6, 0)]);
        assert_eq!(pages[1].contents, vec![ObjId::new(6, 0), ObjId::new(7, 0)]);
        assert!(pages[2].contents.is_empty(), "absent Contents = empty page");
        // Nothing degraded here: every named stream was present.
        assert!(pages.iter().all(|p| p.contents_unresolved == 0));
    }

    /// A **dangling element** in a `/Contents` array degrades: the element
    /// contributes nothing, the surviving streams still load in order, and
    /// the omission is disclosed via `contents_unresolved`.
    ///
    /// §7.3.10: "An indirect reference to an undefined object ... shall be
    /// treated as a reference to the null object." Object 7 is never
    /// defined, so `[6 0 R 7 0 R 8 0 R]` is `[stream, null, stream]` and
    /// the page's content is the concatenation of the two real streams.
    /// Before this fix the whole DOCUMENT was unopenable for this shape.
    #[test]
    fn contents_array_with_dangling_element_degrades_and_is_disclosed() {
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 \
                 /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
            // Object 7 is deliberately never defined.
            (
                3,
                "<< /Type /Page /Parent 2 0 R /Contents [6 0 R 7 0 R 8 0 R] >>",
            ),
            (6, "<< /Length 2 >>\nstream\nq \nendstream"),
            (8, "<< /Length 2 >>\nstream\nQ \nendstream"),
        ]);
        let pages = pages(&doc).expect("a dangling element must not fail the document");
        assert_eq!(pages.len(), 1);
        assert_eq!(
            pages[0].contents,
            vec![ObjId::new(6, 0), ObjId::new(8, 0)],
            "the streams that DO exist still load, in order"
        );
        assert_eq!(
            pages[0].contents_unresolved, 1,
            "the omission is counted, not swallowed"
        );
    }

    /// An **entirely unresolvable** `/Contents` (a single reference to an
    /// object the file does not contain) yields an empty page, disclosed —
    /// not a failed load.
    ///
    /// This is the dominant real-world shape: of 341 corpus files that
    /// failed with `BadContents`, ~300 were this single-reference form.
    /// §7.3.10 makes the value the null object; Table 30 then says
    /// "`Contents` ... if this entry is absent, the page shall be empty".
    #[test]
    fn contents_entirely_unresolvable_is_an_empty_page_disclosed() {
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 \
                 /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
            // Object 9 is never defined anywhere in the file.
            (3, "<< /Type /Page /Parent 2 0 R /Contents 9 0 R >>"),
        ]);
        let pages = pages(&doc).expect("an unresolvable /Contents must not fail the document");
        assert_eq!(pages.len(), 1);
        assert!(pages[0].contents.is_empty(), "empty page, per Table 30");
        assert_eq!(
            pages[0].contents_unresolved, 1,
            "an empty page from a MISSING stream is disclosed, unlike a genuinely blank one"
        );
    }

    /// A **wrong-typed** `/Contents` is still a hard error. This is the
    /// blast-radius guard: degrading a dangling reference must not also
    /// launder a value that has no spec-sanctioned reading into a silent
    /// blank page.
    #[test]
    fn contents_of_the_wrong_type_is_still_an_error() {
        // A direct integer — no reading under any clause.
        let numeric = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 \
                 /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
            (3, "<< /Type /Page /Parent 2 0 R /Contents 42 >>"),
        ]);
        assert_eq!(pages(&numeric).unwrap_err(), PageTreeError::BadContents);

        // A reference to a plain DICTIONARY (an object that exists, but is
        // not a stream): present, wrong type — distinct from absent.
        let dict_target = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 \
                 /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
            (3, "<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>"),
            (6, "<< /NotAStream true >>"),
        ]);
        assert_eq!(pages(&dict_target).unwrap_err(), PageTreeError::BadContents);

        // A wrong-typed ELEMENT inside an otherwise fine array: one bad
        // element still condemns the page (it is not an omission).
        let bad_element = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 \
                 /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
            (3, "<< /Type /Page /Parent 2 0 R /Contents [6 0 R 42] >>"),
            (6, "<< /Length 2 >>\nstream\nq \nendstream"),
        ]);
        assert_eq!(pages(&bad_element).unwrap_err(), PageTreeError::BadContents);
    }

    /// An EXPLICIT `null` is equivalent to omitting the entry (§7.3.9), so
    /// it is a well-formed empty page and is NOT counted as a degradation
    /// — nothing is missing from the file. Distinguishing this from a
    /// dangling reference is what keeps `contents_unresolved` meaningful.
    #[test]
    fn explicit_null_contents_is_absent_not_a_degradation() {
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 \
                 /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
            (3, "<< /Type /Page /Parent 2 0 R /Contents null >>"),
            (4, "<< /Type /Page /Parent 2 0 R /Contents [6 0 R null] >>"),
            (6, "<< /Length 2 >>\nstream\nq \nendstream"),
        ]);
        let pages = pages(&doc).unwrap();
        assert!(pages[0].contents.is_empty());
        assert_eq!(
            pages[0].contents_unresolved, 0,
            "an explicit null is an omitted entry, not missing content"
        );
        assert_eq!(pages[1].contents, vec![ObjId::new(6, 0)]);
        assert_eq!(pages[1].contents_unresolved, 0);
    }

    #[test]
    fn missing_type_falls_back_to_structural_detection() {
        // Node 2 lacks /Type but has /Kids ⇒ treated as Pages node;
        // node 3 lacks /Type and has no /Kids ⇒ treated as Page.
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
            (3, "<< /Parent 2 0 R >>"),
        ]);
        assert_eq!(pages(&doc).unwrap().len(), 1);
    }
}

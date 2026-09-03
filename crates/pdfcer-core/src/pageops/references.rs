//! # Which page does this thing point at? (ISO 32000-1 §12.3.2, §12.6.4.2)
//!
//! Two consumers, one question, so one implementation:
//!
//! - **Delete** needs a census of outline items, link annotations and
//!   named destinations that target a page about to be removed, because
//!   pdfcer discloses them (`core_ops__delete_pages.md` recommends pdfcer
//!   *exceed* Acrobat here — Acrobat leaves them silently broken).
//! - **Extract / split** need the same resolution to decide which
//!   outline entries belong in the output.
//!
//! Writing that twice is how the two eventually disagree about, say,
//! whether a `/GoTo` action counts, and then the delete disclosure
//! under-reports.
//!
//! ## The four shapes a destination can take (§12.3.2)
//!
//! A destination reaches a page in one of four ways, and a resolver that
//! handles fewer under-reports silently:
//!
//! 1. **Explicit array** — `[page /XYZ left top zoom]`, `[page /Fit]`,
//!    etc. Element 0 is an indirect reference to the page object.
//!    §12.3.2.2: *"the page shall be specified by an indirect
//!    reference"* in a document-level destination.
//! 2. **Explicit array reached through an action** — the object carries
//!    `/A` (an action dictionary, §12.6) whose `/S` is `/GoTo` and whose
//!    `/D` is a destination in any of these four forms.
//! 3. **A name** — `/Dest /SomeName`, resolved through the catalog's
//!    `/Names → /Dests` **name tree** (§7.9.6, PDF 1.2+).
//! 4. **A byte string** — the same, resolved through the catalog's
//!    `/Dests` **dictionary** (§12.3.2.3, the PDF 1.1 form, keyed by
//!    name objects).
//!
//! Forms 3 and 4 are why this module carries a resolver *struct* rather
//! than offering a free function: resolving a name means walking a name
//! tree, and doing that once per outline item on a 5,000-item outline
//! would be quadratic. The tree is flattened once, on construction.
//!
//! ## What is deliberately not resolved
//!
//! A destination in a **remote** `/GoToR` action names a page by *index*
//! in another file (§12.6.4.3). It cannot dangle against this document
//! and is not this module's business. Same for `/Launch`, `/URI`, and
//! JavaScript actions — a JavaScript action could navigate anywhere, and
//! pretending to analyse that would be a claim pdfcer cannot support.

use std::collections::{HashMap, HashSet};

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};

/// Maximum name-tree nodes walked while flattening `/Names → /Dests`
/// (pdfcer policy, `ARCHITECTURE.md` §10 — untrusted input, unbounded in
/// the spec).
pub const MAX_NAME_TREE_NODES: usize = 100_000;

/// Maximum name-tree depth (pdfcer policy). §7.9.6 balances these trees;
/// anything this deep is damage or hostility.
pub const MAX_NAME_TREE_DEPTH: usize = 64;

/// Maximum outline items walked (pdfcer policy). Bounds both the
/// dangling census and outline subsetting against a `/Next` chain that
/// does not terminate.
pub const MAX_OUTLINE_ITEMS: usize = 200_000;

/// Resolves destinations to page object ids for one document.
///
/// Construct once per document per operation; it flattens the name
/// trees eagerly so that resolving N destinations is O(N) rather than
/// O(N × tree).
#[derive(Debug, Default)]
pub struct DestinationResolver {
    /// Flattened `/Names → /Dests` (§7.9.6) plus the legacy `/Dests`
    /// dictionary (§12.3.2.3), keyed by the raw name/string bytes. The
    /// two namespaces are merged because a destination reference gives
    /// no hint which it meant, and no real file populates both with
    /// colliding keys.
    named: HashMap<Vec<u8>, Object>,
}

impl DestinationResolver {
    /// Build a resolver for `graph`'s catalog.
    #[must_use]
    pub fn new<G: ObjectGraph + ?Sized>(graph: &G) -> Self {
        let mut named = HashMap::new();
        let Some(catalog) = graph.catalog_dict() else {
            return Self { named };
        };

        // §12.3.2.3, the PDF 1.1 form: catalog `/Dests` is a plain
        // dictionary whose keys are the destination names.
        if let Some(dests) = catalog
            .get(b"Dests")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_dict)
        {
            for (key, value) in dests.iter() {
                named.insert(key.as_bytes().to_vec(), value.clone());
            }
        }

        // §7.9.6 / §12.3.2.3, the PDF 1.2 form: a name TREE under
        // `/Names → /Dests`.
        if let Some(tree) = catalog
            .get(b"Names")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_dict)
            .and_then(|names| names.get(b"Dests").map(|o| graph.resolve(o)))
            .and_then(Object::as_dict)
        {
            let mut budget = MAX_NAME_TREE_NODES;
            let mut visited = HashSet::new();
            flatten_name_tree(graph, tree, 0, &mut budget, &mut visited, &mut named);
        }

        Self { named }
    }

    /// How many named destinations this document defines.
    #[must_use]
    pub fn named_count(&self) -> usize {
        self.named.len()
    }

    /// The destination value a key names, from **either** namespace, or
    /// `None` if nothing defines it.
    ///
    /// # Why this is not [`Self::resolve_destination`]
    ///
    /// That one answers *"which page?"* and folds three different situations
    /// into `None`: undefined, defined-but-dangling, and remote. Correct for
    /// its callers — a destination that already pointed nowhere is not newly
    /// broken by a delete.
    ///
    /// A **writer** asking "may I define this key?" needs the three kept
    /// apart. A key that is defined but points at a deleted page would
    /// answer `None` there, and silently overwriting it is exactly the
    /// collision `EditError::NamedDestinationTaken` exists to refuse. This
    /// asks the membership question directly.
    ///
    /// Keys are compared **byte-for-byte** (§7.9.6), across both the PDF 1.1
    /// catalog `/Dests` dictionary and the PDF 1.2 `/Names` tree, because
    /// this type flattens both at construction.
    #[must_use]
    pub fn lookup(&self, key: &[u8]) -> Option<&Object> {
        self.named.get(key)
    }
    /// Every named destination this document defines, as
    /// `(key bytes, destination value)`.
    ///
    /// Flattened across **both** namespaces (§12.3.2.3) — the PDF 1.1 catalog
    /// `/Dests` dictionary and the PDF 1.2 `/Names` tree — because this type
    /// merges them at construction and a consumer copying destinations into
    /// another document wants all of them, not the ones that happen to live
    /// in the namespace it thought to look in.
    ///
    /// Order is unspecified: the flattening is a `HashMap`, and §7.9.6's
    /// lexical ordering is a property of the name TREE, not of a set of
    /// destinations. A caller writing a tree must sort (see
    /// `EditSession::add_named_destination`, which does).
    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &Object)> {
        self.named.iter().map(|(k, v)| (k.as_slice(), v))
    }
    /// Every named destination whose target page is in `pages`.
    ///
    /// Used by the delete census (to count the ones about to dangle) and
    /// by extract (to count the ones being dropped).
    pub fn names_targeting<'a, G: ObjectGraph + ?Sized>(
        &'a self,
        graph: &'a G,
        pages: &'a HashSet<ObjId>,
    ) -> impl Iterator<Item = &'a [u8]> + 'a {
        self.named.iter().filter_map(move |(key, value)| {
            let target = self.resolve_destination(graph, value)?;
            pages.contains(&target).then_some(key.as_slice())
        })
    }

    /// Resolve a destination **value** (any of §12.3.2's four shapes) to
    /// the page object it targets, or `None`.
    ///
    /// `None` means "no page could be determined", which folds together
    /// *already* dangling, remote, and simply-not-a-destination. That is
    /// the right fold for both consumers: a destination that already
    /// pointed nowhere is not newly broken by a delete, and is not worth
    /// carrying into an extract.
    #[must_use]
    pub fn resolve_destination<G: ObjectGraph + ?Sized>(
        &self,
        graph: &G,
        dest: &Object,
    ) -> Option<ObjId> {
        // Bounded because a name may resolve to another name.
        let mut current = graph.resolve(dest).clone();
        for _ in 0..8 {
            match current {
                // Shape 1: `[page /Fit …]`. Element 0 is the page.
                Object::Array(ref items) => {
                    return items.first().and_then(Object::as_reference);
                }
                // Shape 3 / 4: a name or a byte string keying a tree.
                Object::Name(ref name) => {
                    current = self.named.get(name.as_bytes())?.clone();
                }
                Object::String(ref bytes) => {
                    current = self.named.get(bytes.as_slice())?.clone();
                }
                // §7.9.6: a name-tree VALUE may be a dictionary with a
                // `/D` entry rather than the array itself.
                Object::Dict(ref dict) => {
                    current = graph.resolve(dict.get(b"D")?).clone();
                }
                _ => return None,
            }
        }
        None
    }

    /// Resolve the destination of an object that carries `/Dest` and/or
    /// `/A` — an outline item (§12.3.3) or **any** annotation (§12.5.6), link
    /// or widget or otherwise.
    ///
    /// `/Dest` is checked first because §12.3.3 makes the two mutually
    /// exclusive (*"shall not be present"* together) and a malformed file
    /// carrying both is most cheaply read as meaning its direct one.
    #[must_use]
    pub fn resolve_target<G: ObjectGraph + ?Sized>(&self, graph: &G, dict: &Dict) -> Option<ObjId> {
        if let Some(dest) = dict.get(b"Dest")
            && let Some(page) = self.resolve_destination(graph, dest)
        {
            return Some(page);
        }
        // §12.6: an action dictionary. Only `/GoTo` names a page in THIS
        // document; `/GoToR` names one by index in another file, and
        // `/URI`/`/Launch`/JavaScript name none.
        let action = graph.resolve(dict.get(b"A")?).as_dict()?;
        let is_goto = action
            .get(b"S")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_name)
            .map(Name::as_bytes)
            .is_some_and(|s| s == b"GoTo");
        if !is_goto {
            return None;
        }
        self.resolve_destination(graph, action.get(b"D")?)
    }
}

/// Flatten a §7.9.6 name tree into `out`.
///
/// The `/Names` array alternates key, value, key, value — *"shall be an
/// array of the form `[key₁ value₁ key₂ value₂ … keyₙ valueₙ]`"* — and
/// `/Kids` holds intermediate nodes. Both may appear at a leaf/root in
/// malformed files, so both are read wherever present rather than
/// dispatched on.
fn flatten_name_tree<G: ObjectGraph + ?Sized>(
    graph: &G,
    node: &Dict,
    depth: usize,
    budget: &mut usize,
    visited: &mut HashSet<ObjId>,
    out: &mut HashMap<Vec<u8>, Object>,
) {
    if depth > MAX_NAME_TREE_DEPTH || *budget == 0 {
        return;
    }
    *budget -= 1;

    if let Some(pairs) = node
        .get(b"Names")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
    {
        for pair in pairs.chunks_exact(2) {
            let (Some(key), Some(value)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            // §7.9.6: keys "shall be strings". A file using names is
            // malformed but readable, and both are accepted here for the
            // same reason `resolve_destination` accepts both.
            let key_bytes = match graph.resolve(key) {
                Object::String(bytes) => bytes.clone(),
                Object::Name(name) => name.as_bytes().to_vec(),
                _ => continue,
            };
            out.insert(key_bytes, value.clone());
        }
    }

    if let Some(kids) = node
        .get(b"Kids")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
    {
        for kid in kids {
            // Cycle guard: a `/Kids` array pointing back at an ancestor
            // is trivial to author and would otherwise recurse until the
            // depth guard fires on every branch — quadratic, not just
            // bounded.
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

/// What a structural operation would break, counted.
///
/// Every field is something a front end can put in a sentence, because
/// that is the whole point: `core_ops__delete_pages.md` records that
/// Acrobat *"does not auto-delete, auto-repoint, or warn by default"*,
/// and recommends pdfcer surface these instead of leaving them silently
/// broken. This type is that recommendation's data.
///
/// Counted rather than named: a delete that orphans 300 bookmarks should
/// say "300", not list them. The list is what a future "repair / remove /
/// ignore" review flow would need, and it can be added when that flow
/// exists rather than being carried unused now.
///
/// # ★★ It does not cover field-name targets, and `is_empty()` will not say so
///
/// `/ResetForm`, `/SubmitForm` and `/Hide` name their targets by
/// fully-qualified **name string**. A name is not a reference, so removing
/// the field it names breaks the button while leaving this whole report at
/// zero. See [`census_dangling`]'s own note; the companion numbers live on
/// `EditSession::delete_field` and `EditSession::rename_field`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct DanglingReport {
    /// Outline (bookmark) items whose destination is a removed page
    /// (§12.3.3).
    pub outline_items: usize,
    /// Link annotations on **surviving** pages whose destination is a
    /// removed page (§12.5.6.5).
    ///
    /// Links on removed pages are deliberately excluded: they leave with
    /// their page, so reporting them would inflate the number with
    /// references that no longer exist to be broken.
    pub links: usize,
    /// **Non-link annotations** on surviving pages whose `/A` `/GoTo` names a
    /// removed page (`Pass 183.0`).
    ///
    /// # ★ Why this is a separate field and not a widening of [`Self::links`]
    ///
    /// Until `Pass 183.0` this census walked link annotations **only**, and
    /// that was correct: a `/GoTo` could only reach a page from an outline
    /// item, a named destination or a link, because those were the only
    /// carriers anything authored. Then `set_button_action` learned to write
    /// `/A << /S /GoTo … >>` **on a push button's widget**, and the census
    /// went from complete to under-reporting in the same commit — silently,
    /// because an under-reporting counter reads exactly like a clean bill of
    /// health.
    ///
    /// That is a defect class this project keeps meeting (the `/A`-versus-
    /// `/AA` network-hazard blindness of `Pass 133.0` is the same shape), so
    /// the fix here counts **every** annotation subtype that is not a link
    /// rather than adding widgets and waiting for the next carrier. `/Screen`,
    /// `/Movie` and any future subtype carry `/A` too.
    ///
    /// Kept separate from [`Self::links`] because the operator sentence is
    /// different: a broken link is *"a link in the text goes nowhere"*, a
    /// broken widget action is *"a button on the form stopped working"*.
    pub non_link_annotations: usize,
    /// Named destinations (§12.3.2.3) that resolve to a removed page.
    pub named_destinations: usize,
    /// Whether the document carries a `/PageLabels` number tree
    /// (§12.4.2) that this operation leaves numerically stale.
    ///
    /// A boolean, not a count: the tree is one object and the operator's
    /// question is "are my page numbers wrong now?", which has a yes/no
    /// answer. Matching Acrobat's documented non-repair baseline
    /// (`core_ops__page_labels_and_bates_interaction.md`) while saying so
    /// is the parity-plus this project asked for — Acrobat leaves them
    /// stale *and silent*.
    pub page_labels_stale: bool,
}

impl DanglingReport {
    /// Whether anything at all was reported.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.outline_items == 0
            && self.links == 0
            && self.non_link_annotations == 0
            && self.named_destinations == 0
            && !self.page_labels_stale
    }
}

/// Census the references that removing `removed` would break.
///
/// `surviving` is needed as well as `removed` because link annotations
/// are only interesting when they *stay* — see
/// [`DanglingReport::links`].
///
/// # ★★ WHAT THIS CENSUS CANNOT SEE, and the reader who most needs to know
///
/// **A field-name STRING is not a reference, so nothing here counts one.**
/// `/ResetForm` and `/SubmitForm` name their targets in `/Fields`, and
/// `/Hide` names its in `/T`, as fully-qualified **name strings** — which is
/// pdfcer's own deliberate choice on the authoring side, because a name
/// survives a field being renumbered or copied between documents where an
/// indirect reference does not.
///
/// Deleting the field those names point at leaves **no dangling object
/// reference**, so every count below stays `0` and
/// [`DanglingReport::is_empty`] returns `true` on a document whose buttons
/// just stopped working. That is not a bug in this function; it is the
/// boundary of the question it asks, which is about the object graph.
///
/// The other half is answered by `EditSession::delete_field`'s
/// `action_targets_orphaned` and `EditSession::rename_field`'s
/// `action_targets_retargeted` (`Pass 184.0`), which sweep objects rather
/// than references. **A caller reporting document health needs both**;
/// neither subsumes the other.
///
/// # Errors
///
/// None: an unreadable outline or annotation array yields a smaller
/// count, never a failure. A delete must not be blocked because the
/// disclosure could not be computed — that would trade a real capability
/// for a warning.
#[must_use]
pub fn census_dangling<G: ObjectGraph + ?Sized>(
    graph: &G,
    removed: &HashSet<ObjId>,
    surviving: &[ObjId],
) -> DanglingReport {
    let resolver = DestinationResolver::new(graph);
    let mut report = DanglingReport::default();

    // Outline items (§12.3.3): a doubly-linked sibling chain per level,
    // with `/First` descending.
    if let Some(outlines) = graph
        .catalog_dict()
        .and_then(|catalog| catalog.get(b"Outlines").map(|o| graph.resolve(o)))
        .and_then(Object::as_dict)
    {
        let mut budget = MAX_OUTLINE_ITEMS;
        let mut visited = HashSet::new();
        walk_outline(
            graph,
            outlines.get(b"First").and_then(Object::as_reference),
            0,
            &mut budget,
            &mut visited,
            &mut |dict| {
                if let Some(target) = resolver.resolve_target(graph, dict)
                    && removed.contains(&target)
                {
                    report.outline_items += 1;
                }
            },
        );
    }

    // Link annotations on surviving pages.
    for page_id in surviving {
        let Some(page) = graph.resolved(*page_id).as_dict() else {
            continue;
        };
        let Some(annots) = page
            .get(b"Annots")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_array)
        else {
            continue;
        };
        for annot in annots {
            let Some(dict) = graph.resolve(annot).as_dict() else {
                continue;
            };
            let is_link = dict
                .get(b"Subtype")
                .map(|o| graph.resolve(o))
                .and_then(Object::as_name)
                .map(Name::as_bytes)
                .is_some_and(|s| s == b"Link");
            if let Some(target) = resolver.resolve_target(graph, dict)
                && removed.contains(&target)
            {
                // Every annotation subtype is asked, not just `/Link` --
                // see `DanglingReport::non_link_annotations` for why the
                // subtype filter that used to sit above this line was a
                // silent under-report the moment buttons gained actions.
                if is_link {
                    report.links += 1;
                } else {
                    report.non_link_annotations += 1;
                }
            }
        }
    }

    report.named_destinations = resolver.names_targeting(graph, removed).count();
    report.page_labels_stale = graph
        .catalog_dict()
        .is_some_and(|catalog| catalog.contains_key(b"PageLabels"));

    report
}

/// Walk an outline sibling chain and its descendants, calling `visit`
/// for each item.
///
/// Iterative across siblings and recursive across levels: the sibling
/// chain is the unbounded direction in real files (a flat 10,000-entry
/// outline is ordinary), while nesting is shallow. Recursing on siblings
/// would overflow the stack on exactly the documents this needs to work
/// for.
pub fn walk_outline<G: ObjectGraph + ?Sized>(
    graph: &G,
    first: Option<ObjId>,
    depth: usize,
    budget: &mut usize,
    visited: &mut HashSet<ObjId>,
    visit: &mut impl FnMut(&Dict),
) {
    if depth > MAX_NAME_TREE_DEPTH {
        return;
    }
    let mut current = first;
    while let Some(id) = current {
        if *budget == 0 || !visited.insert(id) {
            return;
        }
        *budget -= 1;
        let Some(dict) = graph.resolved(id).as_dict() else {
            return;
        };
        visit(dict);
        walk_outline(
            graph,
            dict.get(b"First").and_then(Object::as_reference),
            depth + 1,
            budget,
            visited,
            visit,
        );
        current = dict.get(b"Next").and_then(Object::as_reference);
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
    use crate::document::Document;
    use crate::pageops::tests_support::build_pdf;

    /// Two pages, one outline item per page, one named destination, and
    /// a link on page 1 pointing at page 2.
    fn linked_doc() -> Document {
        build_pdf(&[
            (
                1,
                "<< /Type /Catalog /Pages 2 0 R /Outlines 6 0 R \
                 /Names << /Dests 9 0 R >> /PageLabels << /Nums [0 << /S /D >>] >> >>",
            ),
            (2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> \
                 /Annots [5 0 R] >>",
            ),
            (
                4,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
            (
                5,
                "<< /Type /Annot /Subtype /Link /Rect [0 0 1 1] /A << /S /GoTo /D [4 0 R /Fit] >> >>",
            ),
            (6, "<< /Type /Outlines /First 7 0 R /Last 8 0 R /Count 2 >>"),
            (
                7,
                "<< /Title (One) /Parent 6 0 R /Next 8 0 R /Dest [3 0 R /Fit] >>",
            ),
            (
                8,
                "<< /Title (Two) /Parent 6 0 R /Prev 7 0 R /Dest [4 0 R /XYZ null null 0] >>",
            ),
            (9, "<< /Names [(chapter2) [4 0 R /Fit]] >>"),
        ])
    }

    #[test]
    fn deleting_a_page_counts_every_kind_of_reference_to_it() {
        let doc = linked_doc();
        let removed: HashSet<ObjId> = [ObjId::new(4, 0)].into_iter().collect();
        let report = census_dangling(&doc, &removed, &[ObjId::new(3, 0)]);
        assert_eq!(report.outline_items, 1, "the 'Two' bookmark");
        assert_eq!(report.links, 1, "the link on the surviving page 1");
        assert_eq!(report.named_destinations, 1, "(chapter2)");
        assert!(report.page_labels_stale);
        assert!(!report.is_empty());
    }

    #[test]
    fn links_on_removed_pages_are_not_counted() {
        // The link lives on page 1 and points at page 2. Removing page 1
        // takes the link with it, so nothing is left dangling — counting
        // it would inflate the disclosure with references that no longer
        // exist.
        let doc = linked_doc();
        let removed: HashSet<ObjId> = [ObjId::new(3, 0)].into_iter().collect();
        let report = census_dangling(&doc, &removed, &[ObjId::new(4, 0)]);
        assert_eq!(report.links, 0);
        assert_eq!(report.outline_items, 1, "the 'One' bookmark still counts");
    }

    #[test]
    fn a_clean_delete_reports_nothing_but_the_label_tree() {
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
        ]);
        let removed: HashSet<ObjId> = [ObjId::new(3, 0)].into_iter().collect();
        assert!(census_dangling(&doc, &removed, &[]).is_empty());
    }

    #[test]
    fn destinations_resolve_through_both_name_forms_and_through_actions() {
        let doc = linked_doc();
        let resolver = DestinationResolver::new(&doc);
        assert_eq!(resolver.named_count(), 1);
        // Shape 3/4: by name.
        assert_eq!(
            resolver.resolve_destination(&doc, &Object::String(b"chapter2".to_vec())),
            Some(ObjId::new(4, 0))
        );
        // Shape 2: through a /GoTo action on the link annotation.
        let annot = doc.get(ObjId::new(5, 0)).unwrap().value.as_dict().unwrap();
        assert_eq!(resolver.resolve_target(&doc, annot), Some(ObjId::new(4, 0)));
    }

    #[test]
    fn a_remote_action_resolves_to_nothing_rather_than_guessing() {
        // §12.6.4.3 /GoToR names a page by index in ANOTHER file. It
        // cannot dangle against this document, and treating its /D array
        // as a local destination would be a fabricated result.
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> \
                 /Annots [4 0 R] >>",
            ),
            (
                4,
                "<< /Type /Annot /Subtype /Link /Rect [0 0 1 1] \
                 /A << /S /GoToR /F (other.pdf) /D [0 /Fit] >> >>",
            ),
        ]);
        let resolver = DestinationResolver::new(&doc);
        let annot = doc.get(ObjId::new(4, 0)).unwrap().value.as_dict().unwrap();
        assert_eq!(resolver.resolve_target(&doc, annot), None);
    }

    #[test]
    fn an_outline_cycle_terminates() {
        // `/Next` pointing back at an earlier sibling is trivial to
        // author and must not hang the census.
        let doc = build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> >>",
            ),
            (4, "<< /Type /Outlines /First 5 0 R >>"),
            (5, "<< /Title (Loop) /Next 5 0 R /Dest [3 0 R /Fit] >>"),
        ]);
        let removed: HashSet<ObjId> = [ObjId::new(3, 0)].into_iter().collect();
        assert_eq!(census_dangling(&doc, &removed, &[]).outline_items, 1);
    }
}

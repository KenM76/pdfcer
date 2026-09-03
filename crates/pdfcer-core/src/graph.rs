//! # `ObjectGraph` — one read view over "the document as it is right now"
//!
//! Pass 3.1 could get away without this. Pass 3.2 cannot, and the reason
//! is recorded verbatim in [`crate::edit`]'s own module docs:
//!
//! > ⚠️ Pass 3.2 must replace this with an overlay-aware walk. The moment
//! > an edit can add or remove a `Kids` entry, patching a base-derived
//! > list is not an approximation — it is **wrong**.
//!
//! Pass 3.1's mutations (`/Info`, `/Rotate`) changed dictionary *values*
//! only, so [`crate::page_tree::pages`] could walk the **base** document
//! and then patch each leaf's rotation from the overlay. A structural
//! operation changes the page tree's *shape*: after a delete, the base
//! walk still returns the deleted page; after a reorder, it returns the
//! old order. Patching cannot fix either.
//!
//! ## The shape of the fix, and why it is a trait
//!
//! Every consumer of the object graph — the page-tree walk, the outline
//! walk, the cross-document copier, the dangling-reference census —
//! needs exactly three primitives:
//!
//! 1. fetch an indirect object by identity;
//! 2. follow a reference chain to a non-reference value (§7.3.10);
//! 3. read a trailer entry (in practice only `/Root`).
//!
//! [`Document`](crate::document::Document) provides all three, and so
//! does an [`EditSession`](crate::edit::EditSession) *with its overlay
//! applied*. Making that a trait rather than duplicating each walk means
//! there is exactly **one** page-tree walk in pdfcer, and it is
//! automatically correct for both views. A second copy specialised to
//! sessions is precisely the kind of drift that eventually renders a
//! deleted page.
//!
//! ## Why the resolution rules live here as provided methods
//!
//! §7.3.10's rules — dangling reference yields `null` and *"shall not be
//! considered an error"*, a generation mismatch is a dangling reference,
//! a reference chain is depth-guarded — are **spec behaviour, not view
//! behaviour**. Implementing them once as provided methods on the trait
//! makes it impossible for a new view to get them subtly different; an
//! implementor supplies only [`ObjectGraph::value`] and
//! [`ObjectGraph::trailer_entry`], which are the parts that genuinely
//! differ.
//!
//! ## Spec sources
//!
//! - `iso32000__s__7.3.10.md` — indirect objects, dangling references,
//!   substitutability
//! - `iso32000__s__7.7.2.md` — the document catalog, reached via the
//!   trailer's `/Root`

use crate::document::MAX_RESOLVE_DEPTH;
use crate::object::{Dict, ObjId, Object};

/// A read-only view of one PDF document's object graph.
///
/// Implemented by [`Document`](crate::document::Document) (the file as
/// loaded) and by [`EditSession`](crate::edit::EditSession)'s overlay
/// view (the file as the operator currently has it). See the module
/// docs for why this exists at all.
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
/// // The catalog is reached through the trailer's /Root, whichever
/// // view is doing the reaching.
/// assert!(doc.catalog_dict().is_some());
/// # Ok(())
/// # }
/// ```
///
/// # Thread safety — why this trait requires `Send + Sync`
///
/// **The bound exists so a page can be rasterized off the UI thread.**
/// A GUI that rasterizes inline freezes for the length of the render,
/// and on a real CAD sheet that is ~10 s at 1× and ~58 s at 2× — not a
/// slow redraw but a dead application, with no repaint, no progress and
/// no way to cancel. Moving that work to a worker requires the rendered
/// value to cross a thread boundary, and
/// [`DocumentView`](crate::view::DocumentView) is what crosses.
///
/// **Only the trait object was ever the obstacle.** `Document`,
/// `EditSession`, `SessionGraph`, `PendingGraph` and the test graphs
/// were all already thread-safe; `DocumentView<'a>` holds
/// `&'a dyn ObjectGraph`, and an unbounded `dyn Trait` is neither
/// `Send` nor `Sync` regardless of what implements it. Adding the
/// supertrait compiled with **zero errors** across the workspace — it
/// records a property every implementor already had rather than
/// imposing a new one.
///
/// **Why now.** Adding a supertrait bound to a public trait is a
/// breaking change for downstream implementors. pdfcer has no remote and
/// no release (`LEGAL.md` rule 8 — publishing awaits an explicit
/// go-ahead), so the set of affected implementors is exactly the ones
/// in this repository, and it is empty. This is the cheapest moment
/// this bound will ever have.
///
/// **The cost, stated.** Any future `ObjectGraph` implementor must be
/// thread-safe. That forecloses one holding an `Rc`, a `RefCell`, or a
/// non-thread-safe foreign handle. Rust API guideline **`C-SEND-SYNC`**
/// asks for types to be `Send`/`Sync` where possible and for deliberate
/// exceptions to be documented; an object graph is plain data behind an
/// accessor, so an implementor that could not meet this would be the
/// surprising one.
pub trait ObjectGraph: Send + Sync {
    // ^^^^^^^^^^^^ Added 2026-08-07. See the `# Thread safety` section
    // in this trait's doc comment above for why, and why now.

    /// The current value of `id`, or `None` when this view has no such
    /// object.
    ///
    /// "No such object" folds together every §7.3.10/§7.5.4 way an id
    /// can fail to resolve — never defined, freed, or named with a
    /// stale generation — because they are indistinguishable to a
    /// reader and the spec gives them one outcome.
    fn value(&self, id: ObjId) -> Option<&Object>;

    /// The current value of the trailer entry `key`, or `None`.
    ///
    /// Only `/Root` is consulted by anything in this crate today, but
    /// the accessor is general because an overlay view must be able to
    /// answer for a trailer the operator has patched (creating `/Info`
    /// is already such a patch, from Pass 3.1).
    fn trailer_entry(&self, key: &[u8]) -> Option<&Object>;

    /// Follow a reference chain to a non-reference value, applying
    /// §7.3.10's rules: an unresolvable reference yields `null` rather
    /// than an error, and the chain is depth-guarded
    /// ([`MAX_RESOLVE_DEPTH`]) so `5 0 obj 5 0 R endobj` — legal syntax
    /// — cannot loop.
    fn resolve<'a>(&'a self, obj: &'a Object) -> &'a Object {
        const NULL: &Object = &Object::Null;
        let mut current = obj;
        for _ in 0..MAX_RESOLVE_DEPTH {
            match current {
                Object::Reference(id) => match self.value(*id) {
                    Some(value) => current = value,
                    None => return NULL,
                },
                other => return other,
            }
        }
        NULL
    }

    /// Resolve `id` directly to a non-reference value, or `null`.
    ///
    /// Convenience for the very common *"I have an id, give me the
    /// dictionary"* shape, which would otherwise need a temporary
    /// [`Object::Reference`] to hand to [`ObjectGraph::resolve`] — and a
    /// temporary cannot outlive the call, so the borrow does not
    /// compile.
    ///
    /// Iterative, not recursive, and depth-guarded for the same reason
    /// [`ObjectGraph::resolve`] is: `4 0 obj 4 0 R endobj` is legal
    /// syntax, and a recursive implementation would answer it with a
    /// stack overflow — which `pdfcer-core`'s panic-free policy forbids
    /// on untrusted input just as firmly as it forbids an `unwrap`.
    fn resolved(&self, id: ObjId) -> &Object {
        const NULL: &Object = &Object::Null;
        let mut current = id;
        for _ in 0..MAX_RESOLVE_DEPTH {
            match self.value(current) {
                Some(Object::Reference(next)) => current = *next,
                Some(other) => return other,
                None => return NULL,
            }
        }
        NULL
    }

    /// The document catalog (§7.7.2) — the dictionary the trailer's
    /// `/Root` points at — or `None` if the file has no usable one.
    ///
    /// Returns `Option` rather than `Result` because every caller in the
    /// structural-operations path already has its own error type and
    /// maps this to a variant of it; a second error type in the middle
    /// would only be unwrapped and rewrapped.
    fn catalog_dict(&self) -> Option<&Dict> {
        self.trailer_entry(b"Root")
            .map(|root| self.resolve(root))
            .and_then(Object::as_dict)
    }

    /// The object id the trailer's `/Root` names, if it is a reference.
    ///
    /// Needed separately from [`ObjectGraph::catalog_dict`] by anything
    /// that must *write* the catalog back (structural operations all
    /// do), because a dictionary borrowed out of the graph carries no
    /// record of which object it came from.
    fn catalog_id(&self) -> Option<ObjId> {
        self.trailer_entry(b"Root").and_then(Object::as_reference)
    }
}

/// The loaded file, as a graph.
///
/// `Document` already has inherent `resolve`/`catalog` methods with the
/// same semantics; those keep working and keep winning method
/// resolution (inherent methods are preferred), so this impl adds a
/// generic entry point without changing a single existing call site.
impl ObjectGraph for crate::document::Document {
    fn value(&self, id: ObjId) -> Option<&Object> {
        self.get(id).map(|io| &io.value)
    }

    fn trailer_entry(&self, key: &[u8]) -> Option<&Object> {
        self.trailer().get(key)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::object::Name;
    use std::collections::BTreeMap;

    /// A minimal hand-built graph, so the trait's provided methods are
    /// tested without dragging a parsed file in.
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
        // A two-hop chain: 2 -> 3 -> the integer.
        objects.insert(ObjId::new(2, 0), Object::Reference(ObjId::new(3, 0)));
        objects.insert(ObjId::new(3, 0), Object::Integer(42));
        // A self-reference, which is legal syntax and must not loop.
        objects.insert(ObjId::new(4, 0), Object::Reference(ObjId::new(4, 0)));
        let mut trailer = Dict::new();
        trailer.insert(Name::from(b"Root"), Object::Reference(ObjId::new(1, 0)));
        TestGraph { objects, trailer }
    }

    #[test]
    fn resolve_follows_chains_and_nulls_dangling() {
        let g = graph();
        assert_eq!(g.resolved(ObjId::new(2, 0)), &Object::Integer(42));
        // §7.3.10: a reference to nothing is null, not an error.
        assert_eq!(g.resolved(ObjId::new(99, 0)), &Object::Null);
    }

    #[test]
    fn a_reference_cycle_resolves_to_null_rather_than_looping() {
        let g = graph();
        assert_eq!(g.resolved(ObjId::new(4, 0)), &Object::Null);
        let cyclic = Object::Reference(ObjId::new(4, 0));
        assert_eq!(g.resolve(&cyclic), &Object::Null);
    }

    #[test]
    fn the_catalog_is_reached_through_the_trailer() {
        let g = graph();
        assert_eq!(g.catalog_id(), Some(ObjId::new(1, 0)));
        assert!(g.catalog_dict().unwrap().contains_key(b"Type"));
    }
}

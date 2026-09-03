//! Which optional-content groups are hidden **for this render**.
//!
//! # Purpose
//!
//! ISO 32000-1 §8.11.4.3's default configuration (`/OCProperties /D`)
//! says which layers a document wants shown when it is opened. That is a
//! property of the *file*, and [`pdfcer_core::annot::optional_content_default_off`]
//! reads it. This module carries the other thing: which layers the
//! **operator** currently wants hidden, which is a property of the
//! *session* and exists nowhere in the file.
//!
//! # Contract
//!
//! A [`LayerVisibility`] is a COMPLETE answer, not a patch. When one is
//! supplied, the renderer uses it **instead of** the document's default
//! configuration — it does not union, subtract or otherwise merge the
//! two.
//!
//! That is the whole design decision here, and it is deliberate. A merge
//! needs rules for three cases nobody can state confidently: a group the
//! operator turned on that the document turns off, a group the operator
//! turned off that the document never registered, and a group that
//! appears in neither. Every one of those rules would be invisible at the
//! call site and would decide what appears on a page. So the caller
//! computes the final set — starting from the document's defaults, which
//! it can read with the same function the renderer would have used — and
//! the renderer obeys it.
//!
//! The practical consequence for a front end: build the set from
//! `optional_content_default_off(graph)` and apply the operator's
//! toggles to it. Do not pass only the groups the operator touched; that
//! would show every layer the document had turned off.
//!
//! # Why it is not part of [`crate::RenderPolicy`]'s value semantics
//!
//! `RenderPolicy` is `Copy`, and a set is not. It travels as a borrow
//! (`Option<&LayerVisibility>`) held by the policy, which keeps the
//! policy `Copy` while the set itself is owned by the caller's
//! [`crate::RenderOptions`] for the render's duration. Two renders of the
//! same page must never differ for a reason invisible at the call site
//! (`RenderPolicy`'s own docs) — a `static` or thread-local would destroy
//! that, and so would a set the renderer mutated.
//!
//! # Relationship to `pdfcer_core::layers`
//!
//! `pdfcer-core`'s `layers` module ENUMERATES what a document declares —
//! names, order, radio groups, lock flags, and every diagnostic about a
//! malformed registry. This type carries none of that. It is the answer
//! to one question the renderer asks per group (`is it hidden?`), and
//! keeping it that narrow is what stops a rendering decision from
//! depending on how well a panel parsed a name.

use std::collections::BTreeSet;

use pdfcer_core::object::ObjId;

/// The set of optional-content groups hidden for one render.
///
/// See the module docs: this REPLACES the document's default
/// configuration rather than amending it.
///
/// # Examples
///
/// ```
/// use pdfcer_render::LayerVisibility;
/// use pdfcer_core::object::ObjId;
///
/// let id = ObjId::new(10, 0);
/// let v = LayerVisibility::hiding([id]);
/// assert!(v.is_hidden(id));
/// assert!(!v.is_hidden(ObjId::new(11, 0)));
///
/// // An empty set means "show everything", which is a real and useful
/// // state — it is what "turn all layers on" produces, and it is NOT
/// // the same as passing no override at all (that means "obey the
/// // document").
/// assert!(!LayerVisibility::hiding([]).is_hidden(id));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayerVisibility {
    hidden: BTreeSet<ObjId>,
}

impl LayerVisibility {
    /// The visibility state in which exactly `hidden` is hidden.
    #[must_use]
    pub fn hiding<I: IntoIterator<Item = ObjId>>(hidden: I) -> Self {
        Self {
            hidden: hidden.into_iter().collect(),
        }
    }

    /// Is this group hidden?
    #[must_use]
    pub fn is_hidden(&self, id: ObjId) -> bool {
        self.hidden.contains(&id)
    }

    /// The hidden set itself, for the interpreter's membership tests
    /// (`/OCMD` resolution takes a set, not a predicate).
    #[must_use]
    pub(crate) fn hidden_set(&self) -> &BTreeSet<ObjId> {
        &self.hidden
    }

    /// How many groups are hidden — the number a front end reports so an
    /// operator can tell an override is in force.
    #[must_use]
    pub fn hidden_count(&self) -> usize {
        self.hidden.len()
    }
}

//! What an in-session page insert carries besides the pages:
//! `EditSession::insert_pages_with_options`'s options.

use super::{InsertedPageLabels, SeparationPolicy};

/// What happens to the source's bookmarks (ISO 32000-1 §12.3.3) on an
/// in-session insert.
///
/// Only items whose destination is an inserted page arrive, with that
/// destination rebound to the copy; an item kept only because a descendant
/// arrives keeps its title and loses its destination. Every arriving item is
/// closed. The target's own outline is never changed except to append.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum InsertedOutline {
    /// The source's bookmarks do not come across
    /// (`InsertOutcome::source_outline_dropped` says when it had some).
    #[default]
    Drop,
    /// The arriving items follow the target's last top-level bookmark.
    Append,
    /// The arriving items go under one new top-level bookmark with this
    /// title, aimed at the first inserted page (`/Fit`). It is written even
    /// when no source bookmark arrives: the operator asked for a heading
    /// over these pages.
    UnderParent(String),
}

/// Options for `EditSession::insert_pages_with_options`.
///
/// # Examples
///
/// ```
/// use pdfcer_core::pageops::{InsertOptions, InsertedOutline};
///
/// let opts = InsertOptions::default()
///     .with_outline(InsertedOutline::Append)
///     .with_named_destinations(true);
/// assert!(opts.named_destinations);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct InsertOptions {
    /// Page-label policy (§12.4.2).
    pub labels: InsertedPageLabels,
    /// Bookmark policy (§12.3.3).
    pub outline: InsertedOutline,
    /// Carry the source's named destinations (§12.3.2.3) that name an
    /// inserted page into the target's `/Names /Dests` tree, suffixing a key
    /// the target already defines. Links on the inserted pages and carried
    /// bookmarks that used a carried name are rewritten to its final key.
    /// Off by default.
    pub named_destinations: bool,
    /// What happens to an inserted plate of a preseparated set (§14.11.4)
    /// whose sibling plates are not inserted in the same call; the
    /// default repairs its `/Pages` to the plates that did arrive.
    pub separations: SeparationPolicy,
}

impl InsertOptions {
    /// Set [`Self::labels`].
    #[must_use]
    pub fn with_labels(mut self, labels: InsertedPageLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Set [`Self::outline`].
    #[must_use]
    pub fn with_outline(mut self, outline: InsertedOutline) -> Self {
        self.outline = outline;
        self
    }

    /// Set [`Self::named_destinations`].
    #[must_use]
    pub fn with_named_destinations(mut self, carry: bool) -> Self {
        self.named_destinations = carry;
        self
    }

    /// Set [`Self::separations`].
    #[must_use]
    pub fn with_separations(mut self, policy: SeparationPolicy) -> Self {
        self.separations = policy;
        self
    }
}

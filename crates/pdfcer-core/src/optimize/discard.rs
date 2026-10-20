//! Discarding user data during an optimize run (Pass 616.3).
//!
//! Each [`MetadataKind`] the metadata inventory lists can be selected on its
//! own; every item of a selected kind is removed through
//! [`remove_metadata`](crate::edit::EditSession::remove_metadata), so an
//! optimize discard and a `metadata remove` share one implementation.

use crate::doc_metadata::MetadataKind;

/// Which kinds of user data an optimize run removes.
///
/// ```
/// use pdfcer_core::doc_metadata::MetadataKind;
/// use pdfcer_core::optimize::DiscardOptions;
///
/// let d = DiscardOptions::none().with(MetadataKind::Thumbnail);
/// assert!(d.contains(MetadataKind::Thumbnail));
/// assert!(!d.without(MetadataKind::Thumbnail).contains(MetadataKind::Thumbnail));
/// assert_eq!(d.kinds().collect::<Vec<_>>(), [MetadataKind::Thumbnail]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DiscardOptions {
    kinds: u32,
}

impl DiscardOptions {
    /// Nothing discarded.
    #[must_use]
    pub const fn none() -> Self {
        Self { kinds: 0 }
    }

    /// Every kind in [`MetadataKind::ALL`].
    #[must_use]
    pub const fn all() -> Self {
        // `bit` is the declaration index, and `ALL` lists every variant.
        Self {
            kinds: (1 << MetadataKind::ALL.len()) - 1,
        }
    }

    /// These options with `kind` discarded too.
    #[must_use]
    pub const fn with(self, kind: MetadataKind) -> Self {
        Self {
            kinds: self.kinds | bit(kind),
        }
    }

    /// These options with `kind` kept.
    #[must_use]
    pub const fn without(self, kind: MetadataKind) -> Self {
        Self {
            kinds: self.kinds & !bit(kind),
        }
    }

    /// Whether `kind` is discarded.
    #[must_use]
    pub const fn contains(self, kind: MetadataKind) -> bool {
        self.kinds & bit(kind) != 0
    }

    /// Whether nothing is discarded.
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.kinds == 0
    }

    /// The discarded kinds, in [`MetadataKind::ALL`] order.
    pub fn kinds(self) -> impl Iterator<Item = MetadataKind> {
        MetadataKind::ALL
            .into_iter()
            .filter(move |k| self.contains(*k))
    }
}

const fn bit(kind: MetadataKind) -> u32 {
    1 << (kind as u32)
}

//! Discarding user data during an optimize run (Pass 616.3).
//!
//! Each [`MetadataKind`] the metadata inventory lists can be selected on its
//! own; every item of a selected kind is removed through
//! [`remove_metadata`](crate::edit::EditSession::remove_metadata), so an
//! optimize discard and a `metadata remove` share one implementation. The
//! document-object classes ([`DiscardObject`]) are selected the same way.

use super::objects::DiscardObject;
use crate::doc_metadata::MetadataKind;

/// Which kinds of user data, and which classes of document object, an
/// optimize run removes.
///
/// ```
/// use pdfcer_core::doc_metadata::MetadataKind;
/// use pdfcer_core::optimize::DiscardOptions;
///
/// let d = DiscardOptions::none().with(MetadataKind::Thumbnail);
/// assert!(d.contains(MetadataKind::Thumbnail));
/// assert!(!d.without(MetadataKind::Thumbnail).contains(MetadataKind::Thumbnail));
/// assert_eq!(d.kinds().collect::<Vec<_>>(), [MetadataKind::Thumbnail]);
///
/// use pdfcer_core::optimize::DiscardObject;
/// let d = d.with_object(DiscardObject::Bookmarks);
/// assert!(d.contains_object(DiscardObject::Bookmarks) && !d.contains_object(DiscardObject::Tags));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DiscardOptions {
    kinds: u32,
    objects: u32,
}

impl DiscardOptions {
    /// Nothing discarded.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            kinds: 0,
            objects: 0,
        }
    }

    /// Every kind in [`MetadataKind::ALL`] and every class in
    /// [`DiscardObject::ALL`].
    #[must_use]
    pub const fn all() -> Self {
        // A bit is the declaration index, and each `ALL` lists every variant.
        Self {
            kinds: (1 << MetadataKind::ALL.len()) - 1,
            objects: (1 << DiscardObject::ALL.len()) - 1,
        }
    }

    /// These options with `kind` discarded too.
    #[must_use]
    pub const fn with(self, kind: MetadataKind) -> Self {
        Self {
            kinds: self.kinds | bit(kind),
            ..self
        }
    }

    /// These options with `kind` kept.
    #[must_use]
    pub const fn without(self, kind: MetadataKind) -> Self {
        Self {
            kinds: self.kinds & !bit(kind),
            ..self
        }
    }

    /// Whether `kind` is discarded.
    #[must_use]
    pub const fn contains(self, kind: MetadataKind) -> bool {
        self.kinds & bit(kind) != 0
    }

    /// These options with `object` discarded too.
    #[must_use]
    pub const fn with_object(self, object: DiscardObject) -> Self {
        Self {
            objects: self.objects | object_bit(object),
            ..self
        }
    }

    /// These options with `object` kept.
    #[must_use]
    pub const fn without_object(self, object: DiscardObject) -> Self {
        Self {
            objects: self.objects & !object_bit(object),
            ..self
        }
    }

    /// Whether `object` is discarded.
    #[must_use]
    pub const fn contains_object(self, object: DiscardObject) -> bool {
        self.objects & object_bit(object) != 0
    }

    /// Whether nothing is discarded.
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.kinds == 0 && self.objects == 0
    }

    /// The discarded kinds, in [`MetadataKind::ALL`] order.
    pub fn kinds(self) -> impl Iterator<Item = MetadataKind> {
        MetadataKind::ALL
            .into_iter()
            .filter(move |k| self.contains(*k))
    }

    /// The discarded object classes, in [`DiscardObject::ALL`] order.
    pub fn objects(self) -> impl Iterator<Item = DiscardObject> {
        DiscardObject::ALL
            .into_iter()
            .filter(move |o| self.contains_object(*o))
    }

    /// These options with what `name` names discarded too: a
    /// [`MetadataKind::as_str`] or [`DiscardObject::as_str`] name, or `all`
    /// for everything. `None` for a name not in [`Self::known_names`].
    ///
    /// ```
    /// use pdfcer_core::optimize::{DiscardObject, DiscardOptions};
    ///
    /// let d = DiscardOptions::none().with_named("tags").unwrap();
    /// assert!(d.contains_object(DiscardObject::Tags));
    /// assert_eq!(d.with_named("all"), Some(DiscardOptions::all()));
    /// assert_eq!(d.without_named("tags"), Some(DiscardOptions::none()));
    /// assert_eq!(d.with_named("thumbs"), None);
    /// ```
    #[must_use]
    pub fn with_named(self, name: &str) -> Option<Self> {
        Some(match Named::parse(name)? {
            Named::All => Self::all(),
            Named::Kind(k) => self.with(k),
            Named::Object(o) => self.with_object(o),
        })
    }

    /// These options with what `name` names kept; `all` keeps everything.
    #[must_use]
    pub fn without_named(self, name: &str) -> Option<Self> {
        Some(match Named::parse(name)? {
            Named::All => Self::none(),
            Named::Kind(k) => self.without(k),
            Named::Object(o) => self.without_object(o),
        })
    }

    /// The names of what is discarded: kinds, then object classes.
    pub fn names(self) -> impl Iterator<Item = &'static str> {
        self.kinds()
            .map(MetadataKind::as_str)
            .chain(self.objects().map(DiscardObject::as_str))
    }

    /// Every name [`Self::with_named`] accepts, `all` last.
    pub fn known_names() -> impl Iterator<Item = &'static str> {
        Self::all().names().chain(std::iter::once("all"))
    }
}

enum Named {
    All,
    Kind(MetadataKind),
    Object(DiscardObject),
}

impl Named {
    fn parse(name: &str) -> Option<Self> {
        if name == "all" {
            return Some(Self::All);
        }
        MetadataKind::ALL
            .into_iter()
            .find(|k| k.as_str() == name)
            .map(Self::Kind)
            .or_else(|| DiscardObject::from_name(name).map(Self::Object))
    }
}

const fn bit(kind: MetadataKind) -> u32 {
    1 << (kind as u32)
}

const fn object_bit(object: DiscardObject) -> u32 {
    1 << (object as u32)
}

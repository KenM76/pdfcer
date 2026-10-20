//! Navigation clean-up: links and bookmarks that go nowhere, and named
//! destinations nothing names (Pass 616.2).
//!
//! A destination is **broken** when it names a page object that is not in
//! the page tree (what a page delete leaves behind) or a named destination
//! neither catalog namespace defines (§12.3.2.3). A remote, launch, URI or
//! other non-navigation target is never broken, nor is a destination whose
//! first element is not a reference: some readers take a page number there,
//! so its intent is unknown. When the page tree cannot be walked, nothing is
//! judged broken.

use crate::object::ObjId;

/// Which clean-ups an optimize run performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CleanupOptions {
    /// Delete Link annotations whose destination is broken.
    pub invalid_links: bool,
    /// Delete bookmarks whose destination is broken and whose children are
    /// all deleted too. A broken bookmark with a working descendant stays,
    /// so no working bookmark is lost.
    pub invalid_bookmarks: bool,
    /// Delete named destinations no `/Dest` or `/D` in the file names.
    /// A link from another file (`file.pdf#nameddest=x`) cannot be seen, so
    /// this can break one. Skipped when the file holds JavaScript, which
    /// can name a destination in code.
    pub unreferenced_destinations: bool,
}

impl CleanupOptions {
    /// Everything on.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            invalid_links: true,
            invalid_bookmarks: true,
            unreferenced_destinations: true,
        }
    }

    /// Everything off.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            invalid_links: false,
            invalid_bookmarks: false,
            unreferenced_destinations: false,
        }
    }

    /// Whether no clean-up is on.
    #[must_use]
    pub const fn is_none(&self) -> bool {
        !self.invalid_links && !self.invalid_bookmarks && !self.unreferenced_destinations
    }
}

impl Default for CleanupOptions {
    fn default() -> Self {
        Self::all()
    }
}

/// Why a destination is broken.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BrokenTarget {
    /// The destination names this object, which is not a page in the tree.
    MissingPage(ObjId),
    /// The destination names this destination, which nothing defines. Raw
    /// bytes: names are compared byte-wise (§7.9.6).
    UndefinedName(Vec<u8>),
}

impl std::fmt::Display for BrokenTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPage(id) => write!(f, "page {id} is not in the document"),
            Self::UndefinedName(name) => write!(
                f,
                "destination {:?} is not defined",
                String::from_utf8_lossy(name)
            ),
        }
    }
}

/// A Link annotation the clean-up deletes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BrokenLink {
    /// 0-based page the link is on.
    pub page_index: usize,
    /// The annotation.
    pub id: ObjId,
    /// Why it goes nowhere.
    pub target: BrokenTarget,
}

/// A bookmark the clean-up deletes, with its whole subtree.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BrokenBookmark {
    /// The outline item.
    pub id: ObjId,
    /// Its title.
    pub title: String,
    /// Why it goes nowhere; `None` for an item with no destination that
    /// only grouped children, all of them deleted.
    pub target: Option<BrokenTarget>,
    /// Items under it, all broken or targetless, deleted with it.
    pub descendants: usize,
}

/// What the clean-up removes. Empty when every option is off.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct CleanupPlan {
    /// Links deleted, in page order.
    pub links: Vec<BrokenLink>,
    /// Topmost bookmarks deleted, in outline order.
    pub bookmarks: Vec<BrokenBookmark>,
    /// Named destinations deleted, sorted byte-wise.
    pub destinations: Vec<Vec<u8>>,
}

impl CleanupPlan {
    /// Whether the plan removes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.links.is_empty() && self.bookmarks.is_empty() && self.destinations.is_empty()
    }
}

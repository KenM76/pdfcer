//! Whether an explicit encoder may raise the document's version.

use std::fmt;

/// What happens when `encode=` names a filter newer than the document's
/// version (decision 193): JBIG2 needs PDF 1.4 (ISO 32000-1 §7.4.7).
/// `auto` never picks such a filter, whatever this says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum VersionRaise {
    /// Write the filter and set the catalog's `/Version` to the version it
    /// needs (ISO 32000-2 §7.7.2); the plan discloses the raise.
    #[default]
    Allow,
    /// Skip the image with the reason; the version is never changed.
    Refuse,
}

impl VersionRaise {
    /// Every policy, in spelling order.
    pub const ALL: [Self; 2] = [Self::Allow, Self::Refuse];

    /// The profile spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Refuse => "refuse",
        }
    }

    /// The policy spelt `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }
}

impl fmt::Display for VersionRaise {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

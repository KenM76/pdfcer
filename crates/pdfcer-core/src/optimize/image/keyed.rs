//! How a colour-key masked image is downsampled.

use std::fmt;

/// How an image with a colour-key `/Mask` array (ISO 32000-2 §8.9.6.4)
/// is downsampled. The key matches exact sample values, so blending pixels
/// would invent fringe values that no longer match it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum KeyedResample {
    /// By nearest pixel, whatever `method` says: every output sample is a
    /// source sample, so the key still masks exactly what it masked.
    #[default]
    Subsample,
    /// Not at all; the image is skipped with the reason.
    Skip,
}

impl KeyedResample {
    /// Every policy, in spelling order.
    pub const ALL: [Self; 2] = [Self::Subsample, Self::Skip];

    /// The profile spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Subsample => "subsample",
            Self::Skip => "skip",
        }
    }

    /// The policy spelt `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }
}

impl fmt::Display for KeyedResample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

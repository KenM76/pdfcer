//! When 3D artwork activates (ISO 32000-1 §13.6.2 Table 299 `/A`; RichMedia
//! `/Activation /Condition`, ISO 32000-2 §13.7).

use crate::graph::ObjectGraph;
use crate::object::Dict;

use super::name_of;

/// When a `/3D` annotation's artwork activates (§13.6.2 Table 299 `/A`).
/// Until then, and whenever the page prints under [`Self::Click`], the
/// reader shows the poster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ThreeDActivation {
    /// `/XA` — the reader clicks the annotation (the spec default).
    #[default]
    Click,
    /// `/PO` — the page opens.
    PageOpen,
    /// `/PV` — the page becomes visible.
    PageVisible,
}

impl ThreeDActivation {
    /// The `/A` name bytes.
    #[must_use]
    pub fn name(self) -> &'static [u8] {
        match self {
            Self::Click => b"XA",
            Self::PageOpen => b"PO",
            Self::PageVisible => b"PV",
        }
    }

    /// The activation an `/A` (or RichMedia `/Condition`) name states, or
    /// `None` for a name the standard does not list.
    ///
    /// ```
    /// use pdfcer_core::threed::ThreeDActivation;
    /// assert_eq!(ThreeDActivation::from_name(b"PO"), Some(ThreeDActivation::PageOpen));
    /// assert_eq!(ThreeDActivation::from_name(b"XA").map(ThreeDActivation::name), Some(&b"XA"[..]));
    /// assert_eq!(ThreeDActivation::from_name(b"Now"), None);
    /// ```
    #[must_use]
    pub fn from_name(name: &[u8]) -> Option<Self> {
        [Self::Click, Self::PageOpen, Self::PageVisible]
            .into_iter()
            .find(|a| a.name() == name)
    }
}

/// The activation a `/3D` or RichMedia annotation states, else the default.
pub(super) fn activation_of<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict) -> ThreeDActivation {
    let sub = |d: &Dict, key: &[u8]| d.get(key).and_then(|o| graph.resolve(o).as_dict()).cloned();
    let stated = match name_of(graph, annot, b"Subtype") {
        Some(b"3D") => {
            sub(annot, b"3DA").and_then(|a| name_of(graph, &a, b"A").map(<[u8]>::to_vec))
        }
        Some(b"RichMedia") => sub(annot, b"RichMediaSettings")
            .and_then(|s| sub(&s, b"Activation"))
            .and_then(|a| name_of(graph, &a, b"Condition").map(<[u8]>::to_vec)),
        _ => None,
    };
    stated
        .and_then(|n| ThreeDActivation::from_name(&n))
        .unwrap_or_default()
}

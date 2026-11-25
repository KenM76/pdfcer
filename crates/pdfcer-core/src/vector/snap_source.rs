//! What a snap candidate came from, so a shell can say what it snapped to.

/// The kind of page object a [`SnapCandidate`](super::SnapCandidate) or a
/// [`SnapGeometry`](super::SnapGeometry) belongs to.
///
/// Says nothing about priority: [`SnapKind`](super::SnapKind) alone orders a
/// candidate list. New sources are added as pdfcer learns to snap to more
/// object kinds, so a `match` on it needs a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[non_exhaustive]
pub enum SnapSource {
    /// Not one object: a page axis, a grid point.
    #[default]
    Page,
    /// A path in the page's content, or an intersection between two.
    Path,
    /// A placed image (an image XObject or an inline image), by the
    /// quadrilateral its transform maps the unit square to (ISO 32000-1
    /// §8.9.4).
    Image,
    /// A run of text in the page's content.
    Text,
    /// A shape markup: `/Square`, `/Circle`, `/Line`, `/PolyLine`,
    /// `/Polygon`, `/Ink`, `/FreeText`.
    Markup,
    /// A text-markup annotation's quadrilaterals: `/Highlight`,
    /// `/Underline`, `/StrikeOut`, `/Squiggly` (§12.5.6.10).
    TextMarkup,
    /// A form field's widget annotation (§12.5.6.19).
    FormField,
    /// A ce dimension (one pdfcer authored).
    CeDimension,
    /// Any other annotation, by its `/Rect`: a stamp, a note icon, a link, a
    /// file attachment.
    Annotation,
}

/// Which ce dimension a [`SnapCandidate`](super::SnapCandidate) came from,
/// carried in [`SnapCandidate::ce_dimension`](super::SnapCandidate::ce_dimension)
/// so a shell can bind to the object without mapping a point back to it.
///
/// `drawn` is `false` for a target on geometry the ce dimension does not
/// draw: the circle a centre mark was fitted to, or the circle of a
/// radius/diameter ce dimension that hides it. Such a target is offered, and
/// the shell owes the operator a word that it is not visible (CLAUDE.md
/// rule 4).
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::DimensionId;
/// use pdfcer_core::vector::CeDimensionTarget;
///
/// let t = CeDimensionTarget::new(DimensionId(3), false);
/// assert_eq!((t.id, t.drawn), (DimensionId(3), false));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct CeDimensionTarget {
    /// The ce dimension.
    pub id: crate::dimension::DimensionId,
    /// Whether the geometry the target lies on is drawn.
    pub drawn: bool,
}

impl CeDimensionTarget {
    /// A target on ce dimension `id`; `drawn` as the field says.
    #[must_use]
    pub const fn new(id: crate::dimension::DimensionId, drawn: bool) -> Self {
        Self { id, drawn }
    }
}

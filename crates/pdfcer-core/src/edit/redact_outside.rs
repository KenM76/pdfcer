//! Mark everything on a page **outside** a kept rectangle for redaction
//! (Pass 694.1).

use super::{EditError, EditSession};
use crate::annot_author::{Quad, RedactAppearance};
use crate::object::ObjId;
use crate::page_tree::Rect;

/// Margin added around the outer extent, in points, so content whose
/// bounds touch the MediaBox edge is strictly inside a strip.
const OUTER_MARGIN: f64 = 1.0;

impl EditSession {
    /// Mark for redaction everything on page `page_index` **outside**
    /// `keep`, as one `/Redact` annotation (ISO 32000-1 §12.5.6.23) whose
    /// `/QuadPoints` are the up-to-four strips around it.
    ///
    /// The outer extent is the page's MediaBox united with the painted
    /// bounds of every content object (plus a 1 pt margin), **not** the
    /// CropBox: content a crop hides, or content drawn off the MediaBox,
    /// is outside `keep` and must not survive the apply. Like every mark,
    /// this removes nothing until [`Self::apply_redactions`]; anything that
    /// crosses `keep`'s edge is caught by a strip and goes on apply.
    ///
    /// `keep` is clamped to the outer extent. This marks one page; a shell
    /// offering a page range calls it per page with that page's own
    /// rectangle.
    ///
    /// # Errors
    ///
    /// [`EditError::PageOutOfRange`] for an unknown page;
    /// [`EditError::RedactOutsideNothing`] when `keep` covers the whole
    /// outer extent; [`EditError::VectorEditContent`] if the page content
    /// cannot be read; plus every refusal of [`Self::add_redaction`].
    pub fn mark_redaction_outside(
        &mut self,
        page_index: usize,
        keep: Rect,
        appearance: &RedactAppearance,
    ) -> Result<ObjId, EditError> {
        let pages = self.pages()?;
        let count = pages.len();
        let page = pages.get(page_index).ok_or(EditError::PageOutOfRange {
            index: page_index,
            count,
        })?;
        let mut outer = page.media_box;
        match self.page_objects(page_index) {
            Ok(model) => {
                let b = model.page_bbox();
                if !b.is_empty() {
                    outer = Rect::from_corners(
                        outer.llx.min(b.min.x),
                        outer.lly.min(b.min.y),
                        outer.urx.max(b.max.x),
                        outer.ury.max(b.max.y),
                    );
                }
            }
            Err(EditError::VectorEditNoContents { .. }) => {}
            Err(err) => return Err(err),
        }
        let outer = Rect::from_corners(
            outer.llx - OUTER_MARGIN,
            outer.lly - OUTER_MARGIN,
            outer.urx + OUTER_MARGIN,
            outer.ury + OUTER_MARGIN,
        );
        let quads = Quad::outside(keep, outer);
        if quads.is_empty() {
            return Err(EditError::RedactOutsideNothing { page_index });
        }
        self.add_redaction(page_index, &appearance.to_spec(quads))
    }
}

impl Quad {
    /// The up-to-four axis-aligned strips covering `outer` minus `keep`:
    /// bottom and top span `outer`'s full width, left and right fill the
    /// band between them. `keep` is clamped to `outer`; empty strips are
    /// omitted, so a `keep` covering `outer` yields none.
    #[must_use]
    pub fn outside(keep: Rect, outer: Rect) -> Vec<Self> {
        let k = Rect::from_corners(
            keep.llx.clamp(outer.llx, outer.urx),
            keep.lly.clamp(outer.lly, outer.ury),
            keep.urx.clamp(outer.llx, outer.urx),
            keep.ury.clamp(outer.lly, outer.ury),
        );
        [
            (outer.llx, outer.lly, outer.urx, k.lly),
            (outer.llx, k.ury, outer.urx, outer.ury),
            (outer.llx, k.lly, k.llx, k.ury),
            (k.urx, k.lly, outer.urx, k.ury),
        ]
        .into_iter()
        .filter(|&(x0, y0, x1, y1)| x1 > x0 && y1 > y0)
        .map(|(x0, y0, x1, y1)| Self::from_rect(Rect::from_corners(x0, y0, x1, y1)))
        .collect()
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // A short slice is a failing test, not a crash in the engine.
mod tests {
    use super::*;

    fn r(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect::from_corners(x0, y0, x1, y1)
    }

    #[test]
    fn a_centred_keep_leaves_four_strips_that_tile_the_rest() {
        let q = Quad::outside(r(10.0, 20.0, 30.0, 40.0), r(0.0, 0.0, 100.0, 100.0));
        assert_eq!(q.len(), 4);
        let area: f64 = q
            .iter()
            .map(|q| (q.ur.0 - q.ul.0) * (q.ul.1 - q.ll.1))
            .sum();
        assert!((area - (10_000.0 - 400.0)).abs() < 1e-9, "{area}");
    }

    #[test]
    fn a_keep_touching_an_edge_drops_that_strip() {
        let q = Quad::outside(r(0.0, 0.0, 50.0, 100.0), r(0.0, 0.0, 100.0, 100.0));
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].ll, (50.0, 0.0));
    }

    #[test]
    fn a_keep_covering_everything_leaves_nothing() {
        assert!(Quad::outside(r(-5.0, -5.0, 200.0, 200.0), r(0.0, 0.0, 100.0, 100.0)).is_empty());
    }
}

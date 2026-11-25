//! Snap targets for a page of the session: markups (pdfcer-gui request G281)
//! and every other page object (G324).

use std::collections::BTreeSet;

use super::{EditError, EditSession};
use crate::annot::page_annotations;
use crate::dimension::style::resolve_style;
use crate::dimension::{DimensionKind, DimensionModel, DimensionRecord, DimensionStyle};
use crate::object::ObjId;
use crate::page_tree::Page;
use crate::vector::{
    MarkupSnapGeometry, OutlineKind, PageObjects, SnapGeometry, SnapSource, annotation_is_hidden,
    annotation_snap_geometry, content_snap_geometry, markup_snap_geometry,
};

impl EditSession {
    /// The snap targets of the markups on `page_index`, as the session
    /// currently holds them, for
    /// [`snap_candidates_with_markups`](crate::vector::snap_candidates_with_markups).
    ///
    /// [`markup_snap_geometry`](crate::vector::markup_snap_geometry) with the
    /// session's ce dimensions excluded: a ce dimension is placed by its own
    /// points, not snapped to as a markup. `hidden_layers` is the set of
    /// layers switched off, as [`crate::annot::oc_is_hidden`] takes it.
    ///
    /// Reads only. Build once per page and again after any edit to the page's
    /// annotations.
    ///
    /// # Errors
    ///
    /// [`EditError::PageOutOfRange`]; a page tree that cannot be read.
    pub fn markup_snap_geometry(
        &self,
        page_index: usize,
        hidden_layers: &BTreeSet<ObjId>,
    ) -> Result<Vec<MarkupSnapGeometry>, EditError> {
        let page = self.snap_page(page_index)?;
        Ok(markup_snap_geometry(
            &self.graph(),
            page.id,
            hidden_layers,
            &ce_dimension_annots(&self.dimension_model()),
        ))
    }

    /// The snap targets of **every** object on `page_index`, for
    /// [`snap_candidates_with_markups`](crate::vector::snap_candidates_with_markups):
    /// what [`Self::markup_snap_geometry`] reads, plus every other annotation
    /// ([`annotation_snap_geometry`]), the page's ce dimensions and, when
    /// `model` is given, its images and text ([`content_snap_geometry`]).
    /// Each geometry names its [`SnapGeometry::source`].
    ///
    /// A ce dimension ([`SnapSource::CeDimension`]) gives what is drawn and
    /// what it measured: a linear one its dimension line (both ends, the
    /// middle, the nearest point on it) and each extension line it draws
    /// (both ends); a circular one its centre and its circle's rim; an
    /// angular one its apex; a perimeter one its measured points; a centre
    /// mark its centre, its strokes and the rim of the circle it was fitted
    /// to. Each candidate names the ce dimension in
    /// [`SnapCandidate::ce_dimension`](crate::vector::SnapCandidate::ce_dimension);
    /// a rim that is not drawn (a centre mark's) is offered with `drawn`
    /// `false`. One whose group is hidden is left out, as is one on a hidden
    /// layer.
    ///
    /// `model` is the page's decomposition, as passed to the query; `None`
    /// leaves images and text out. Paths stay the query's own, from `model`.
    /// `hidden_layers` is the set of layers switched off.
    ///
    /// Reads only. Build once per page and again after any edit to the page.
    ///
    /// # Errors
    ///
    /// [`EditError::PageOutOfRange`]; a page tree that cannot be read.
    pub fn page_snap_geometry(
        &self,
        page_index: usize,
        model: Option<&PageObjects>,
        hidden_layers: &BTreeSet<ObjId>,
    ) -> Result<Vec<SnapGeometry>, EditError> {
        let page = self.snap_page(page_index)?;
        let graph = self.graph();
        let dims = self.dimension_model();
        let ce = ce_dimension_annots(&dims);
        let mut out = markup_snap_geometry(&graph, page.id, hidden_layers, &ce);
        out.extend(annotation_snap_geometry(
            &graph,
            page.id,
            hidden_layers,
            &ce,
        ));
        let shown: BTreeSet<ObjId> = page_annotations(&graph, page.id)
            .iter()
            .filter(|a| !annotation_is_hidden(&graph, a, hidden_layers))
            .filter_map(|a| a.id)
            .collect();
        out.extend(
            dims.dimensions()
                .iter()
                .filter(|d| d.annot.is_some_and(|id| shown.contains(&id)))
                .filter(|d| group_shown(&graph, &dims, d, hidden_layers))
                .map(|d| {
                    let style = dims.group(d.group).map(|group| {
                        resolve_style(group, &d.style).on_user_unit(self.dimension_user_unit(d))
                    });
                    ce_dimension_geometry(d, style)
                })
                .filter(|g| !g.is_empty()),
        );
        if let Some(model) = model {
            out.extend(content_snap_geometry(&graph, model, hidden_layers));
        }
        Ok(out)
    }

    fn snap_page(&self, page_index: usize) -> Result<Page, EditError> {
        let pages = self.pages()?;
        let count = pages.len();
        pages
            .into_iter()
            .nth(page_index)
            .ok_or(EditError::PageOutOfRange {
                index: page_index,
                count,
            })
    }
}

fn ce_dimension_annots(dims: &DimensionModel) -> BTreeSet<ObjId> {
    dims.dimensions().iter().filter_map(|d| d.annot).collect()
}

fn group_shown<G: crate::graph::ObjectGraph + ?Sized>(
    graph: &G,
    dims: &DimensionModel,
    d: &DimensionRecord,
    hidden_layers: &BTreeSet<ObjId>,
) -> bool {
    dims.group(d.group).is_none_or(|g| {
        g.visible
            && !g
                .ocg
                .is_some_and(|oc| crate::annot::oc_is_hidden(graph, oc, hidden_layers))
    })
}

/// One ce dimension's targets. A new [`DimensionKind`] adds its arm here.
/// `style` is the one the baker draws `d` with; `None` leaves out the
/// extension lines, whose drawn extent depends on it.
fn ce_dimension_geometry(d: &DimensionRecord, style: Option<DimensionStyle>) -> SnapGeometry {
    let mut g = SnapGeometry::new(SnapSource::CeDimension, d.annot, None);
    g.set_dimension(d.id);
    g.set_anchor_targets(
        match &d.kind {
            DimensionKind::CentreLine { a, b } => Some((*a, *b)),
            _ => None,
        },
        d.kind.centre_mark_centre().is_some() || matches!(d.kind, DimensionKind::Circular { .. }),
    );
    match &d.kind {
        DimensionKind::Linear { .. } => {
            if let Some((dim_a, dim_b, _, _)) = d.kind.linear_geometry() {
                g.push_points(&[dim_a, dim_b], false, OutlineKind::Polygonal);
            }
            let drawn = style.and_then(|s| d.kind.extension_segments(s));
            for (start, end) in drawn.into_iter().flatten().flatten() {
                g.push_points(&[start, end], false, OutlineKind::EndsOnly);
            }
        }
        DimensionKind::Circular {
            fit, show_circle, ..
        } => {
            g.push_center_point(fit.center);
            g.push_circle(fit.center, fit.radius, *show_circle);
        }
        DimensionKind::Angular { apex, .. } => g.push_node(*apex),
        DimensionKind::Perimeter { points, .. } => {
            g.push_points(points, false, OutlineKind::EndsOnly);
        }
        DimensionKind::CentreLine { a, b } => {
            g.push_points(&[*a, *b], false, OutlineKind::Polygonal);
        }
        DimensionKind::CentreMark { fit, .. } => {
            g.push_center_point(fit.center);
            g.push_circle(fit.center, fit.radius, false);
        }
    }
    // A centre mark's cross arms and extension lines, standalone or on a
    // radius/diameter; their length depends on the style.
    if !matches!(d.kind, DimensionKind::CentreLine { .. })
        && let Some(s) = style
    {
        for (start, end) in d.kind.centre_snap_segments(s) {
            g.push_points(&[start, end], false, OutlineKind::EndsOnly);
        }
    }
    g
}

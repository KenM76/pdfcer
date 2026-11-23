//! Snap targets from markup annotations, alongside the page's own paths.
//!
//! [`markup_snap_geometry`] reads a page's markups once into
//! [`MarkupSnapGeometry`] values; [`snap_candidates_with_markups`] answers a
//! point query over the page objects and those markups together, in the one
//! ordering [`snap_candidates`](super::snap_candidates) defines.
//!
//! Geometry is the stroke centreline, the same line a page path snaps to: a
//! `/Square`, `/Circle` or `/FreeText` box is `/Rect` less `/RD`
//! (ISO 32000-1 §12.5.6.8 Table 180, §12.5.6.6 Table 174) inset by half the
//! border width; `/Line` is `/L` (§12.5.6.7), `/Polygon` and `/PolyLine` are
//! `/Vertices` (§12.5.6.9), `/Ink` is `/InkList` (§12.5.6.13), a callout is
//! `/CL`. A cloudy border snaps to the polygon under the scallops.

use std::collections::BTreeSet;

use super::decompose::PageObjects;
use super::geometry::{Bounds, Point};
use super::snap::{self, SnapCandidate, SnapConfig, SnapKind};
use super::snap_source::SnapSource;
use crate::annot::{Annotation, oc_is_hidden, page_annotations};
use crate::annot_author::{
    inset_by_rect_differences, read_border_effect, read_border_width, read_numbers,
};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::page_tree::Rect;

/// Most points one annotation contributes; an `/InkList` past it is cut short
/// (`docs/ARCHITECTURE.md` §10).
pub const MAX_MARKUP_SNAP_POINTS: usize = 4096;

/// Chords an ellipse is flattened to for the on-edge projection.
const ELLIPSE_CHORDS: usize = 64;

/// How an outline's points become candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutlineKind {
    /// Every point a node (a free end of an open outline an endpoint), every
    /// edge a midpoint and an on-edge projection.
    Polygonal,
    /// A sampled curve: free ends and one on-edge projection only.
    Smooth,
    /// The two points alone; the segment between them is not drawn.
    EndsOnly,
}

#[derive(Debug, Clone, PartialEq)]
struct Outline {
    points: Vec<Point>,
    closed: bool,
    kind: OutlineKind,
}

/// One page object's snap targets, in page space: a markup, any other
/// annotation, a ce dimension, an image or a text object.
///
/// Built by [`markup_snap_geometry`], [`annotation_snap_geometry`](super::annotation_snap_geometry),
/// [`content_snap_geometry`](super::content_snap_geometry) and
/// [`EditSession::page_snap_geometry`](crate::edit::EditSession::page_snap_geometry);
/// answered by [`snap_candidates_with_markups`].
#[derive(Debug, Clone, PartialEq)]
pub struct SnapGeometry {
    source: SnapSource,
    annotation: Option<ObjId>,
    object: Option<usize>,
    outlines: Vec<Outline>,
    nodes: Vec<Point>,
    centers: Vec<Point>,
    bounds: Bounds,
}

/// The name this type had when it held markups only.
pub type MarkupSnapGeometry = SnapGeometry;

impl SnapGeometry {
    /// What kind of object these targets belong to.
    #[must_use]
    pub const fn source(&self) -> SnapSource {
        self.source
    }

    /// The annotation these targets belong to; `None` for page content.
    #[must_use]
    pub const fn annotation(&self) -> Option<ObjId> {
        self.annotation
    }

    /// The index into [`PageObjects::objects`] of the content object these
    /// targets belong to; `None` for an annotation and for an object inside a
    /// form XObject (a [`FormLeaf`](super::FormLeaf)).
    #[must_use]
    pub const fn object(&self) -> Option<usize> {
        self.object
    }

    /// The box around every target, page space.
    #[must_use]
    pub const fn bounds(&self) -> Bounds {
        self.bounds
    }

    /// An empty geometry from `source`, naming its annotation or object.
    pub(crate) fn new(
        source: SnapSource,
        annotation: Option<ObjId>,
        object: Option<usize>,
    ) -> Self {
        Self {
            source,
            annotation,
            object,
            outlines: Vec::new(),
            nodes: Vec::new(),
            centers: Vec::new(),
            bounds: Bounds::EMPTY,
        }
    }

    fn point_count(&self) -> usize {
        self.outlines.iter().map(|o| o.points.len()).sum()
    }

    /// Add an outline, dropping non-finite points and honouring the cap.
    pub(crate) fn push_outline(&mut self, points: &[(f64, f64)], closed: bool, kind: OutlineKind) {
        let room = MAX_MARKUP_SNAP_POINTS.saturating_sub(self.point_count());
        let points: Vec<Point> = points
            .iter()
            .map(|&(x, y)| Point::new(x, y))
            .filter(|p| p.is_finite())
            .take(room)
            .collect();
        if points.is_empty() {
            return;
        }
        for &p in &points {
            self.bounds = self.bounds.union_point(p);
        }
        self.outlines.push(Outline {
            points,
            closed,
            kind,
        });
    }

    /// `r` as a closed outline: corners, edge middles, nearest edge point.
    pub(crate) fn push_rect(&mut self, r: Rect) {
        let corners = [
            (r.llx, r.lly),
            (r.urx, r.lly),
            (r.urx, r.ury),
            (r.llx, r.ury),
        ];
        self.push_outline(&corners, true, OutlineKind::Polygonal);
    }

    /// The centre of `r` as a [`SnapKind::Center`] target.
    pub(crate) fn push_center(&mut self, r: Rect) {
        self.push_center_point(Point::new((r.llx + r.urx) / 2.0, (r.lly + r.ury) / 2.0));
    }

    /// Add a [`SnapKind::Center`] target.
    pub(crate) fn push_center_point(&mut self, c: Point) {
        if c.is_finite() {
            self.centers.push(c);
            self.bounds = self.bounds.union_point(c);
        }
    }

    fn push_ellipse(&mut self, r: Rect) {
        let (cx, cy) = ((r.llx + r.urx) / 2.0, (r.lly + r.ury) / 2.0);
        let (rx, ry) = ((r.urx - r.llx) / 2.0, (r.ury - r.lly) / 2.0);
        let ring: Vec<(f64, f64)> = (0..ELLIPSE_CHORDS)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / ELLIPSE_CHORDS as f64;
                (cx + rx * t.cos(), cy + ry * t.sin())
            })
            .collect();
        self.push_outline(&ring, true, OutlineKind::Smooth);
        for (x, y) in [(cx + rx, cy), (cx, cy + ry), (cx - rx, cy), (cx, cy - ry)] {
            let p = Point::new(x, y);
            self.push_node(p);
        }
    }

    /// Add a [`SnapKind::Node`] target.
    pub(crate) fn push_node(&mut self, p: Point) {
        if p.is_finite() {
            self.nodes.push(p);
            self.bounds = self.bounds.union_point(p);
        }
    }

    /// [`Self::push_outline`] over points.
    pub(crate) fn push_points(&mut self, points: &[Point], closed: bool, kind: OutlineKind) {
        let pairs: Vec<(f64, f64)> = points.iter().map(|p| (p.x, p.y)).collect();
        self.push_outline(&pairs, closed, kind);
    }

    /// Whether no target was pushed, so the geometry can be dropped.
    pub(crate) fn is_empty(&self) -> bool {
        self.outlines.is_empty() && self.nodes.is_empty() && self.centers.is_empty()
    }
}

/// The snap targets of every markup on a page.
///
/// Covers `/Square`, `/Circle`, `/Line`, `/PolyLine`, `/Polygon` (cloudy or
/// not), `/Ink` and `/FreeText` (its box and its callout line). Left out:
/// every other subtype (so widgets and popups), an annotation that is hidden
/// or not shown on screen (`/F` Hidden or NoView, §12.5.3), one whose `/OC`
/// is hidden under `hidden_layers` (the set [`oc_is_hidden`] takes), one named
/// in `exclude`, and one written directly in `/Annots` (it has no object
/// number to report).
///
/// A `/Line` with leader lines (`/LL`) or a caption (`/Cap`) gives its two
/// `/L` points and nothing between them: the line drawn is offset from `/L`.
///
/// Reads only; a malformed entry gives no targets.
#[must_use]
pub fn markup_snap_geometry<G: ObjectGraph + ?Sized>(
    graph: &G,
    page_id: ObjId,
    hidden_layers: &BTreeSet<ObjId>,
    exclude: &BTreeSet<ObjId>,
) -> Vec<MarkupSnapGeometry> {
    page_annotations(graph, page_id)
        .iter()
        .filter_map(|a| {
            let id = a.id?;
            if a.is_popup
                || a.flags.suppressed_on_screen()
                || exclude.contains(&id)
                || a.oc
                    .is_some_and(|oc| oc_is_hidden(graph, oc, hidden_layers))
            {
                return None;
            }
            let dict = graph.resolved(id).as_dict()?;
            let geom = geometry_of(graph, id, a, dict);
            (!geom.is_empty()).then_some(geom)
        })
        .collect()
}

fn geometry_of<G: ObjectGraph + ?Sized>(
    graph: &G,
    id: ObjId,
    a: &Annotation,
    dict: &Dict,
) -> MarkupSnapGeometry {
    let mut g = SnapGeometry::new(SnapSource::Markup, Some(id), None);
    match a.subtype.as_slice() {
        b"Square" | b"Circle" | b"FreeText" => {
            let Some(rect) = a.rect else { return g };
            let cloudy = read_border_effect(graph, dict).is_some();
            let shape = drawn_box(graph, dict, rect, cloudy);
            if a.subtype == b"Circle" {
                g.push_ellipse(shape);
            } else {
                g.push_rect(shape);
            }
            g.push_center(shape);
            if a.subtype == b"FreeText" {
                let callout = pairs(&read_numbers(graph, dict, b"CL"));
                g.push_outline(&callout, false, OutlineKind::Polygonal);
            }
        }
        b"Line" => {
            if let Some([start, end]) = a.line {
                let kind = if line_is_offset(graph, dict) {
                    OutlineKind::EndsOnly
                } else {
                    OutlineKind::Polygonal
                };
                g.push_outline(&[start, end], false, kind);
            }
        }
        b"Polygon" | b"PolyLine" => {
            if let Some(v) = &a.vertices {
                g.push_outline(v, a.subtype == b"Polygon", OutlineKind::Polygonal);
            }
        }
        b"Ink" => {
            for stroke in a.ink_list.iter().flatten() {
                g.push_outline(stroke, false, OutlineKind::Smooth);
            }
        }
        _ => {}
    }
    g
}

/// `/Rect` less `/RD`, then inset to the stroke centreline unless the border
/// is cloudy (the scallops sit on the box itself).
fn drawn_box<G: ObjectGraph + ?Sized>(graph: &G, dict: &Dict, rect: Rect, cloudy: bool) -> Rect {
    let inner = inset_by_rect_differences(graph, dict, rect);
    let half = read_border_width(graph, dict) / 2.0;
    if cloudy || !(half.is_finite() && half > 0.0) {
        return inner;
    }
    let inset = Rect {
        llx: inner.llx + half,
        lly: inner.lly + half,
        urx: inner.urx - half,
        ury: inner.ury - half,
    };
    if inset.urx > inset.llx && inset.ury > inset.lly {
        inset
    } else {
        inner
    }
}

/// §12.5.6.7 Table 175: non-zero `/LL` or `/Cap true` moves the drawn line
/// off `/L`.
fn line_is_offset<G: ObjectGraph + ?Sized>(graph: &G, dict: &Dict) -> bool {
    let leader = dict
        .get(b"LL")
        .and_then(|o| graph.resolve(o).as_number())
        .is_some_and(|v| v != 0.0);
    let cap = matches!(
        dict.get(b"Cap").map(|o| graph.resolve(o)),
        Some(Object::Boolean(true))
    );
    leader || cap
}

fn pairs(nums: &[f64]) -> Vec<(f64, f64)> {
    nums.as_chunks::<2>()
        .0
        .iter()
        .map(|&[x, y]| (x, y))
        .collect()
}

/// [`snap_candidates`](super::snap_candidates) over the page objects and any
/// [`SnapGeometry`] together: the page's markups, or every source
/// [`EditSession::page_snap_geometry`](crate::edit::EditSession::page_snap_geometry)
/// reads.
///
/// A candidate carries its geometry's [`SnapGeometry::source`] in
/// [`SnapCandidate::source`], its annotation in
/// [`SnapCandidate::source_annotation`] and its content object in
/// `source_object`. Kinds
/// and their order are the ones page paths use: a vertex or box corner is
/// [`SnapKind::Node`], a free end [`SnapKind::Endpoint`], a box or ellipse
/// centre [`SnapKind::Center`], an edge's middle [`SnapKind::Midpoint`], the
/// nearest point on an edge [`SnapKind::SegmentCenterline`]. An ellipse gives
/// its four axis ends as nodes.
///
/// `model` is `None` for a page with no content of its own. Intersections
/// (when [`SnapConfig::intersections`] is on) are found between page paths
/// only, never with a markup edge.
///
/// Never panics; a non-finite `query` or unusable tolerance gives an empty
/// list.
///
/// # Examples
///
/// ```
/// use pdfcer_core::vector::{snap_candidates_with_markups, Point, SnapConfig};
/// let cfg = SnapConfig::new(0.5);
/// let found = snap_candidates_with_markups(Point::new(3.0, 4.0), &cfg, None, &[]);
/// assert!(found.iter().all(|c| c.source_annotation.is_none()));
/// ```
#[must_use]
pub fn snap_candidates_with_markups(
    query: Point,
    config: &SnapConfig,
    model: Option<&PageObjects>,
    markups: &[MarkupSnapGeometry],
) -> Vec<SnapCandidate> {
    let Some(mut out) = snap::collect_candidates(query, config, model) else {
        return Vec::new();
    };
    let tol = config.tolerance;
    for m in markups {
        if !m.bounds.inflate(tol).contains(query) {
            continue;
        }
        let mut push = |point: Point, kind: SnapKind| {
            if point.is_finite() && point.distance(query) <= tol {
                out.push(SnapCandidate {
                    point,
                    kind,
                    source_object: m.object,
                    source_annotation: m.annotation,
                    source: m.source,
                });
            }
        };
        for &c in &m.centers {
            push(c, SnapKind::Center);
        }
        for &n in &m.nodes {
            push(n, SnapKind::Node);
        }
        for o in &m.outlines {
            outline_targets(o, query, &mut push);
        }
    }
    snap::finalize(out, query)
}

fn outline_targets(o: &Outline, query: Point, push: &mut impl FnMut(Point, SnapKind)) {
    let n = o.points.len();
    let free_end = |i: usize| !o.closed && (i == 0 || i + 1 == n);
    for (i, &p) in o.points.iter().enumerate() {
        match o.kind {
            OutlineKind::Polygonal if !free_end(i) => push(p, SnapKind::Node),
            OutlineKind::Smooth if !free_end(i) => {}
            _ => push(p, SnapKind::Endpoint),
        }
    }
    if o.kind == OutlineKind::EndsOnly || n < 2 {
        return;
    }
    let edges = if o.closed { n } else { n - 1 };
    let mut nearest: Option<(Point, f64)> = None;
    let ends = o.points.iter().cycle().skip(1);
    for (&a, &b) in o.points.iter().zip(ends).take(edges) {
        let proj = snap::project_onto_segment(query, a, b);
        if o.kind == OutlineKind::Polygonal {
            push(a.midpoint(b), SnapKind::Midpoint);
            push(proj, SnapKind::SegmentCenterline);
        } else if nearest.is_none_or(|(_, d)| proj.distance(query) < d) {
            nearest = Some((proj, proj.distance(query)));
        }
    }
    if let Some((p, _)) = nearest {
        push(p, SnapKind::SegmentCenterline);
    }
}

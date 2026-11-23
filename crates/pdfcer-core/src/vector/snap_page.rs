//! Snap targets from annotations that are not shape markups, and from the
//! images and text in a page's content.
//!
//! Each reader returns [`SnapGeometry`] for
//! [`snap_candidates_with_markups`](super::snap_candidates_with_markups),
//! alongside what [`markup_snap_geometry`](super::markup_snap_geometry) reads.

use std::collections::BTreeSet;

use super::decompose::{
    ImageObject, ImageSource, PageObjects, TextBoundsBasis, TextObject, VectorObject,
};
use super::geometry::{Bounds, Point};
use super::snap_annot::{OutlineKind, SnapGeometry};
use super::snap_source::SnapSource;
use crate::annot::{Annotation, oc_is_hidden, page_annotations};
use crate::annot_author::{Quad, read_numbers};
use crate::graph::ObjectGraph;
use crate::object::ObjId;
use crate::page_tree::Rect;

/// Subtypes [`markup_snap_geometry`](super::markup_snap_geometry) reads.
const SHAPE_MARKUPS: [&[u8]; 7] = [
    b"Square",
    b"Circle",
    b"Line",
    b"PolyLine",
    b"Polygon",
    b"Ink",
    b"FreeText",
];

/// Subtypes whose geometry is `/QuadPoints` (ISO 32000-1 §12.5.6.10).
const TEXT_MARKUPS: [&[u8]; 4] = [b"Highlight", b"Underline", b"StrikeOut", b"Squiggly"];

/// Below this, a direction component counts as zero: the text runs along a
/// page axis and its box is its own outline.
const AXIS_ALIGNED: f64 = 1.0e-9;

/// The snap targets of every annotation on a page that
/// [`markup_snap_geometry`](super::markup_snap_geometry) does not read.
///
/// - A widget ([`SnapSource::FormField`]) and any other subtype
///   ([`SnapSource::Annotation`]: a stamp, a note icon, a link, a file
///   attachment) give their `/Rect`: corners ([`SnapKind::Node`](super::SnapKind::Node)),
///   edge middles, the centre and the nearest point on an edge.
/// - `/Highlight`, `/Underline`, `/StrikeOut` and `/Squiggly`
///   ([`SnapSource::TextMarkup`]) give each `/QuadPoints` quadrilateral the
///   same way, read in any corner order (ambiguity `QP-A1`); one with no
///   usable quads gives its `/Rect`.
///
/// Left out, as [`markup_snap_geometry`](super::markup_snap_geometry) leaves
/// them out: shape markups (that reader's), popups, anything `/F` Hidden or
/// NoView, anything whose `/OC` is hidden under `hidden_layers`, anything in
/// `exclude`, and an annotation written directly in `/Annots`.
///
/// Reads only; a malformed entry gives no targets.
#[must_use]
pub fn annotation_snap_geometry<G: ObjectGraph + ?Sized>(
    graph: &G,
    page_id: ObjId,
    hidden_layers: &BTreeSet<ObjId>,
    exclude: &BTreeSet<ObjId>,
) -> Vec<SnapGeometry> {
    page_annotations(graph, page_id)
        .iter()
        .filter_map(|a| {
            let id = a.id?;
            if a.is_popup
                || SHAPE_MARKUPS.contains(&a.subtype.as_slice())
                || exclude.contains(&id)
                || is_hidden(graph, a, hidden_layers)
            {
                return None;
            }
            let geom = other_annotation(graph, id, a);
            (!geom.is_empty()).then_some(geom)
        })
        .collect()
}

/// Whether `a` is not shown on screen with `hidden_layers` switched off.
pub(crate) fn is_hidden<G: ObjectGraph + ?Sized>(
    graph: &G,
    a: &Annotation,
    hidden_layers: &BTreeSet<ObjId>,
) -> bool {
    a.flags.suppressed_on_screen()
        || a.oc
            .is_some_and(|oc| oc_is_hidden(graph, oc, hidden_layers))
}

fn other_annotation<G: ObjectGraph + ?Sized>(graph: &G, id: ObjId, a: &Annotation) -> SnapGeometry {
    let subtype = a.subtype.as_slice();
    let source = if subtype == b"Widget" {
        SnapSource::FormField
    } else if TEXT_MARKUPS.contains(&subtype) {
        SnapSource::TextMarkup
    } else {
        SnapSource::Annotation
    };
    let mut g = SnapGeometry::new(source, Some(id), None);
    if source == SnapSource::TextMarkup
        && let Some(dict) = graph.resolved(id).as_dict()
    {
        for q in read_numbers(graph, dict, b"QuadPoints").as_chunks::<8>().0 {
            push_quad(&mut g, Quad::from_quad_points(q));
        }
    }
    if g.is_empty()
        && let Some(rect) = a.rect
    {
        g.push_rect(rect);
        g.push_center(rect);
    }
    g
}

fn push_quad(g: &mut SnapGeometry, q: Quad) {
    let ring = [q.ll, q.lr, q.ur, q.ul];
    g.push_outline(&ring, true, OutlineKind::Polygonal);
    let (sx, sy) = ring
        .iter()
        .fold((0.0, 0.0), |(sx, sy), &(x, y)| (sx + x, sy + y));
    g.push_center_point(Point::new(sx / 4.0, sy / 4.0));
}

/// The snap targets of the images and text in a page's content, from the
/// page's own objects and those inside its form XObjects
/// ([`PageObjects::leaves`]).
///
/// - An image ([`SnapSource::Image`]) gives the quadrilateral its transform
///   maps the unit square to (ISO 32000-1 §8.9.4), so a rotated or skewed
///   placement snaps to its real corners: corners
///   ([`SnapKind::Node`](super::SnapKind::Node)), edge middles, the centre and
///   the nearest point on an edge. A form XObject gives nothing itself; its
///   contents are its leaves.
/// - Each text run ([`SnapSource::Text`]) gives its baseline start, the pen
///   position its show operator began at, and its box centre. A run along a
///   page axis also gives its box corners, edge middles and baseline end.
///   A rotated run gives neither, because its box is the axis-aligned hull,
///   not the run's outline. A text object whose box is the em-box fallback
///   ([`TextBoundsBasis::EmBox`]) gives baseline starts only, since its box
///   was guessed.
///
/// A geometry from [`PageObjects::objects`] carries its index in
/// [`SnapGeometry::object`]; one from a leaf carries `None`. An object
/// hidden under `hidden_layers` (as
/// [`VectorObject::is_hidden_under`] decides it) gives nothing.
///
/// Reads only.
#[must_use]
pub fn content_snap_geometry<G: ObjectGraph + ?Sized>(
    graph: &G,
    model: &PageObjects,
    hidden_layers: &BTreeSet<ObjId>,
) -> Vec<SnapGeometry> {
    let own = model.objects.iter().enumerate().map(|(i, o)| (Some(i), o));
    let leaves = model.leaves.iter().map(|l| (None, &l.object));
    own.chain(leaves)
        .filter(|(_, o)| !o.is_hidden_under(graph, hidden_layers))
        .filter_map(|(index, o)| {
            let geom = match o {
                VectorObject::Image(img) => image_geometry(img, index),
                VectorObject::Text(text) => text_geometry(text, index),
                VectorObject::Path(_) => return None,
            };
            (!geom.is_empty()).then_some(geom)
        })
        .collect()
}

/// The indices into `model.objects` of the objects the renderer leaves
/// unpainted with `hidden_layers` switched off: the `skip` set for
/// [`snap_candidates_skipping`](super::snap_candidates_skipping).
///
/// Reads only.
#[must_use]
pub fn hidden_objects<G: ObjectGraph + ?Sized>(
    graph: &G,
    model: &PageObjects,
    hidden_layers: &BTreeSet<ObjId>,
) -> BTreeSet<usize> {
    if hidden_layers.is_empty() {
        return BTreeSet::new();
    }
    model
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| o.is_hidden_under(graph, hidden_layers))
        .map(|(i, _)| i)
        .collect()
}

fn image_geometry(img: &ImageObject, index: Option<usize>) -> SnapGeometry {
    let mut g = SnapGeometry::new(SnapSource::Image, None, index);
    if img.source == ImageSource::Form {
        return g;
    }
    let at = |x: f64, y: f64| img.ctm.map_point(Point::new(x, y));
    let quad = [at(0.0, 0.0), at(1.0, 0.0), at(1.0, 1.0), at(0.0, 1.0)];
    g.push_points(&quad, true, OutlineKind::Polygonal);
    g.push_center_point(at(0.5, 0.5));
    g
}

fn text_geometry(text: &TextObject, index: Option<usize>) -> SnapGeometry {
    let mut g = SnapGeometry::new(SnapSource::Text, None, index);
    let guessed = text.bounds_basis == TextBoundsBasis::EmBox;
    for run in &text.runs {
        let m = run.text_matrix;
        let start = text.ctm.map_point(Point::new(m.e, m.f));
        if guessed || run.bounds.is_empty() {
            g.push_points(&[start], false, OutlineKind::EndsOnly);
            continue;
        }
        let b = run.bounds;
        g.push_center_point(b.min.midpoint(b.max));
        let dir = text.ctm.map_vector(Point::new(m.a, m.b));
        match baseline_end(start, dir, b) {
            Some(end) => {
                g.push_points(&[start, end], false, OutlineKind::Polygonal);
                g.push_rect(rect_of(b));
            }
            None => g.push_points(&[start], false, OutlineKind::EndsOnly),
        }
    }
    g
}

/// Where a run along a page axis ends: `start` carried along `dir` to the
/// far edge of its box. `None` for a rotated or degenerate direction.
fn baseline_end(start: Point, dir: Point, b: Bounds) -> Option<Point> {
    let len = dir.x.hypot(dir.y);
    if !len.is_finite() || len <= f64::EPSILON {
        return None;
    }
    let (ux, uy) = (dir.x / len, dir.y / len);
    let end = if uy.abs() <= AXIS_ALIGNED {
        Point::new(if ux > 0.0 { b.max.x } else { b.min.x }, start.y)
    } else if ux.abs() <= AXIS_ALIGNED {
        Point::new(start.x, if uy > 0.0 { b.max.y } else { b.min.y })
    } else {
        return None;
    };
    end.is_finite().then_some(end)
}

const fn rect_of(b: Bounds) -> Rect {
    Rect {
        llx: b.min.x,
        lly: b.min.y,
        urx: b.max.x,
        ury: b.max.y,
    }
}

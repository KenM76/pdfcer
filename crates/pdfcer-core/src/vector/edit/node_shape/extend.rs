//! Continue an open subpath from one of its ends, and open a closed subpath
//! at a node.
//!
//! Path construction and the closing rules: ISO 32000-1 §8.5.2.1 Table 59
//! (`h` draws a line to the subpath's start and leaves the current point
//! there), §8.5.3.1 Table 60 (`s`, `b` and `b*` close the last subpath before
//! painting).

use crate::content::ContentStream;

use super::super::super::decompose::{PathObject, Segment};
use super::super::super::geometry::Point;
use super::super::{
    AnchorKind, AnchorSite, PlannedEdit, VectorEditError, emit_op, enumerate_anchors,
    is_clipping_path, ops_in_range, splice,
};
use super::{CLIP_DISCLOSURE, Node, RECT_DISCLOSURE, Shape, Work};

type Edit = (usize, usize, Vec<u8>);

/// Where a node's subpath sits in its object.
struct Place {
    /// The subpath's index in the object.
    subpath: usize,
    /// The subpath's anchor sites, first node first.
    sites: Vec<AnchorSite>,
}

fn place(content: &ContentStream, obj: &PathObject, node_index: usize) -> Option<Place> {
    let mut first = 0usize;
    for (subpath, sp) in obj.subpaths.iter().enumerate() {
        let n = sp.anchors().count();
        if node_index < first + n {
            let anchors = enumerate_anchors(content, obj.tokens.start, obj.tokens.end);
            let sites = anchors.get(first..first + n)?.to_vec();
            return Some(Place { subpath, sites });
        }
        first += n;
    }
    None
}

fn finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

fn segment_is_finite(seg: &Segment) -> bool {
    match *seg {
        Segment::Line { to } => finite(to),
        Segment::Cubic { c1, c2, to } => finite(c1) && finite(c2) && finite(to),
    }
}

impl Work {
    fn user_of(&self, node: Node) -> [f64; 2] {
        node.user.unwrap_or_else(|| self.user(node.page))
    }

    /// The operator drawing a new segment that ends at user-space `end`.
    fn new_op(&self, c: Option<(Point, Point)>, end: [f64; 2]) -> Vec<u8> {
        match c {
            None => emit_op(&end, b"l"),
            Some((c1, c2)) => {
                let (a, b) = (self.user(c1), self.user(c2));
                emit_op(&[a[0], a[1], b[0], b[1], end[0], end[1]], b"c")
            }
        }
    }

    /// The operator drawing segment `k`, with a closing edge written out as
    /// the line an `h` would have drawn.
    fn drawn(&self, k: usize) -> Result<Vec<u8>, VectorEditError> {
        let seg = self.segs.get(k).ok_or(VectorEditError::MalformedOperand)?;
        match seg.shape {
            Shape::Line => Ok(emit_op(&self.user_of(self.node(k + 1)?), b"l")),
            Shape::Cubic(..) => self.seg_bytes(k),
        }
    }

    /// New segments after the subpath's last node.
    fn append(&self, segments: &[Segment], sites: &[AnchorSite]) -> Result<Edit, VectorEditError> {
        let at = sites
            .last()
            .map(|s| s.byte_end)
            .ok_or(VectorEditError::MalformedOperand)?;
        let mut bytes = Vec::new();
        for seg in segments {
            bytes.push(b' ');
            let op = match *seg {
                Segment::Line { to } => self.new_op(None, self.user(to)),
                Segment::Cubic { c1, c2, to } => self.new_op(Some((c1, c2)), self.user(to)),
            };
            bytes.extend_from_slice(&op);
        }
        Ok((at, at, bytes))
    }

    /// New segments before the subpath's first node: its `m` moves to the far
    /// end, and the new segments are drawn back toward the old first node.
    /// Every existing operator after the `m` is left alone.
    fn prepend(
        &self,
        segments: &[Segment],
        sites: &[AnchorSite],
        index: usize,
    ) -> Result<Edit, VectorEditError> {
        let start = sites.first().ok_or(VectorEditError::MalformedOperand)?;
        if start.kind != AnchorKind::Editable || start.keyword != b"m" {
            return Err(VectorEditError::SubpathStartsImplicitly { index });
        }
        let far = segments
            .last()
            .ok_or(VectorEditError::NoSegmentsGiven)?
            .end();
        let mut bytes = emit_op(&self.user(far), b"m");
        for (i, seg) in segments.iter().enumerate().rev() {
            let end = match i.checked_sub(1).and_then(|p| segments.get(p)) {
                Some(prev) => self.user(prev.end()),
                None => self.user_of(self.node(0)?),
            };
            let handles = match *seg {
                Segment::Line { .. } => None,
                Segment::Cubic { c1, c2, .. } => Some((c2, c1)),
            };
            bytes.push(b' ');
            bytes.extend_from_slice(&self.new_op(handles, end));
        }
        Ok((start.byte_start, start.byte_end, bytes))
    }

    /// The whole subpath rewritten to start, and end, at node `k`.
    fn rotated(&self, k: usize) -> Result<Vec<u8>, VectorEditError> {
        let count = self.segs.len();
        let from = if k < count { k } else { 0 };
        let mut bytes = emit_op(&self.user_of(self.node(from)?), b"m");
        for j in 0..count {
            bytes.push(b' ');
            bytes.extend_from_slice(&self.drawn((from + j) % count)?);
        }
        Ok(bytes)
    }
}

/// Plan **continuing an open subpath**: draw `segments`, in order, onward from
/// node `node_index`, which must be the first or the last node of an open
/// subpath. Each segment's points are in page space.
///
/// From the last node the new operators follow the subpath's own, and nothing
/// else is rewritten; the new nodes follow `node_index` and later indices in
/// the object shift up by `segments.len()`. From the first node the
/// subpath's `m` moves to the far end of the new segments, which are drawn
/// back toward the old first node, so the far end becomes the subpath's first
/// node and every index from `node_index` on shifts up by `segments.len()`.
/// A subpath of one node is continued from its end.
///
/// A new curve is always written as `c`. Extending a clipping path is
/// disclosed.
///
/// # Errors
///
/// [`VectorEditError::NoSegmentsGiven`], [`VectorEditError::NonFinitePoint`],
/// [`VectorEditError::SubpathClosed`] (a closed subpath, an `re` rectangle, or
/// one the painting operator closes), [`VectorEditError::NotAnEndNode`],
/// [`VectorEditError::SubpathStartsImplicitly`] (continuing from the first
/// node of a subpath that has no `m` of its own), plus
/// [`VectorEditError::NodeOutOfRange`], [`VectorEditError::DegenerateCtm`] and
/// [`VectorEditError::MalformedOperand`].
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{decompose, NoXObjects, Matrix, Point, Segment, VectorObject};
/// use pdfcer_core::vector::edit::plan_extend_path;
///
/// let cs = ContentStream::parse(b"0 0 m 10 0 l S".to_vec()).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// let VectorObject::Path(path) = &model.objects[0] else { unreachable!() };
/// let more = [Segment::Line { to: Point::new(10.0, 5.0) }];
/// let plan = plan_extend_path(&cs, path, 1, &more).unwrap();
/// assert_eq!(plan.content, b"0 0 m 10 0 l 10 5 l S");
/// let plan = plan_extend_path(&cs, path, 0, &more).unwrap();
/// assert_eq!(plan.content, b"10 5 m 0 0 l 10 0 l S");
/// ```
pub fn plan_extend_path(
    content: &ContentStream,
    obj: &PathObject,
    node_index: usize,
    segments: &[Segment],
) -> Result<PlannedEdit, VectorEditError> {
    if segments.is_empty() {
        return Err(VectorEditError::NoSegmentsGiven);
    }
    if !segments.iter().all(segment_is_finite) {
        return Err(VectorEditError::NonFinitePoint);
    }
    let (work, k) = Work::open(content, obj, node_index)?;
    if work.closed {
        return Err(VectorEditError::SubpathClosed { index: node_index });
    }
    let here = place(content, obj, node_index).ok_or(VectorEditError::MalformedOperand)?;
    let edit = if k + 1 == work.nodes.len() {
        work.append(segments, &here.sites)?
    } else if k == 0 {
        work.prepend(segments, &here.sites, node_index)?
    } else {
        return Err(VectorEditError::NotAnEndNode { index: node_index });
    };
    let mut disclosures = Vec::new();
    if is_clipping_path(content, obj.tokens.start, obj.tokens.end) {
        disclosures.push(CLIP_DISCLOSURE.to_owned());
    }
    Ok(PlannedEdit {
        operators_touched: 1,
        content: splice(&content.buf, &mut [edit]),
        disclosures,
    })
}

/// The edit that stops the object's painting operator closing its last
/// subpath: `s`, `b` and `b*` become `S`, `B` and `B*`.
fn unclosing_paint(content: &ContentStream, obj: &PathObject) -> Option<Edit> {
    let ops = ops_in_range(content, obj.tokens.start, obj.tokens.end);
    let last = ops.last()?;
    let open: &[u8] = match last.keyword(&content.buf)? {
        b"s" => b"S",
        b"b" => b"B",
        b"b*" => b"B*",
        _ => return None,
    };
    let end = last.byte_end();
    Some((end.checked_sub(open.len())?, end, open.to_vec()))
}

/// Plan **opening a closed subpath** at node `node_index`: the subpath then
/// starts and ends there, as two separate ends.
///
/// Every edge is still drawn, the closing edge as an explicit line, so a fill
/// and a clip are unchanged; a stroke gains two line caps where it had a
/// join, and a dash pattern starts from the opened node. Opened at its first
/// node, only the `h` is rewritten; anywhere else the subpath's operators are
/// rewritten in their new order. The subpath's nodes are renumbered from the
/// opened node, which is also its last; later indices in the object shift up
/// by one, or not at all when the subpath's last node already sat on its
/// first. An `re` rectangle is rewritten as `m`/`l`, and that is disclosed.
///
/// A painting operator that would close the subpath again (`s`, `b`, `b*`) is
/// replaced by its non-closing form.
///
/// # Errors
///
/// [`VectorEditError::SubpathAlreadyOpen`],
/// [`VectorEditError::SubpathStartsImplicitly`] (the subpath has no `m` of its
/// own), plus [`VectorEditError::NodeOutOfRange`],
/// [`VectorEditError::DegenerateCtm`] and [`VectorEditError::MalformedOperand`].
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{decompose, NoXObjects, Matrix, VectorObject};
/// use pdfcer_core::vector::edit::plan_open_subpath;
///
/// let cs = ContentStream::parse(b"0 0 m 10 0 l 10 10 l h S".to_vec()).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// let VectorObject::Path(path) = &model.objects[0] else { unreachable!() };
/// let plan = plan_open_subpath(&cs, path, 0).unwrap();
/// assert_eq!(plan.content, b"0 0 m 10 0 l 10 10 l 0 0 l S");
/// let plan = plan_open_subpath(&cs, path, 1).unwrap();
/// assert_eq!(plan.content, b"10 0 m 10 10 l 0 0 l 10 0 l S");
/// ```
pub fn plan_open_subpath(
    content: &ContentStream,
    obj: &PathObject,
    node_index: usize,
) -> Result<PlannedEdit, VectorEditError> {
    let (work, k) = Work::open(content, obj, node_index)?;
    if !work.closed {
        return Err(VectorEditError::SubpathAlreadyOpen { index: node_index });
    }
    let here = place(content, obj, node_index).ok_or(VectorEditError::MalformedOperand)?;
    let (first, last) = match (here.sites.first(), here.sites.last()) {
        (Some(first), Some(last)) => (first, last),
        _ => return Err(VectorEditError::MalformedOperand),
    };
    let implicit =
        work.rect.is_none() && (first.kind != AnchorKind::Editable || first.keyword != b"m");
    if implicit {
        return Err(VectorEditError::SubpathStartsImplicitly { index: node_index });
    }
    let (start, end) = match (work.rect, work.close_at) {
        (Some(rect), _) => rect,
        (None, Some(h)) => (first.byte_start, h + 1),
        (None, None) => (first.byte_start, last.byte_end),
    };
    let at_start = k == 0 || k >= work.segs.len();
    let mut edit: Edit = if at_start && work.rect.is_none() {
        let closing = work.closing_length() > super::SAME_POINT;
        let line = if closing {
            emit_op(&work.user_of(work.node(0)?), b"l")
        } else {
            Vec::new()
        };
        match work.close_at {
            Some(h) => (h, h + 1, line),
            None if closing => (end, end, [b" ".as_slice(), &line].concat()),
            None => (end, end, Vec::new()),
        }
    } else {
        (start, end, work.rotated(k)?)
    };
    let next_implicit = obj
        .subpaths
        .get(here.subpath + 1)
        .is_some_and(|sp| sp.starts_implicitly);
    if next_implicit {
        // The next subpath relied on the `h` to put the current point back.
        edit.2.push(b' ');
        edit.2
            .extend_from_slice(&emit_op(&work.user_of(work.node(0)?), b"m"));
    }
    let mut edits = vec![edit];
    if here.subpath + 1 == obj.subpaths.len() {
        edits.extend(unclosing_paint(content, obj));
    }
    let mut disclosures = Vec::new();
    if work.rect.is_some() {
        disclosures.push(RECT_DISCLOSURE.to_owned());
    }
    Ok(PlannedEdit {
        operators_touched: edits.len(),
        content: splice(&content.buf, &mut edits),
        disclosures,
    })
}

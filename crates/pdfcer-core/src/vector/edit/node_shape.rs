//! Insert a node on a segment, and convert a node's or a segment's kind
//! (pdfcer-gui request `G154`).
//!
//! Geometry is computed in page space from the decomposition (so "smooth" and
//! "symmetric" mean what the operator sees) and mapped back through the
//! object's inverse CTM. Operand pairs an edit leaves alone are re-emitted from
//! the original operands, so an unmoved point never picks up rounding noise,
//! and only the operators an edit changes are rewritten (§5, R46).
//!
//! Path construction operators and their implicit points: ISO 32000-1 §8.5.2.1
//! Table 59 (`v` repeats the current point as its first control point, `y`
//! repeats the end point as its second; `re` is `x y m`, three `l`, `h`).

use crate::content::ContentStream;

use super::super::decompose::{PathObject, Segment, Subpath};
use super::super::geometry::{Matrix, Point, sub};
use super::{
    AnchorKind, AnchorSite, PlannedEdit, VectorEditError, emit_op, enumerate_anchors,
    is_clipping_path, ops_in_range, splice,
};

/// What [`plan_convert_node`] makes of a node.
///
/// pdfcer stores no node type: a PDF path has none (§8.5.2.1), and the type
/// is re-derivable from the handles. So each kind is a change to the handles,
/// not a label. A geometry-preserving "cusp" has nothing to write; a shell
/// that wants one simply stops linking the two handles when one is dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NodeKind {
    /// Both handles are pulled into the node, so the segments meet at a sharp
    /// point. A straight side is left alone.
    Corner,
    /// The handles are turned onto one line through the node, each keeping its
    /// length. A straight side stays straight and the curve's handle lines up
    /// with it; if both sides are straight they become curves first.
    Smooth,
    /// As [`NodeKind::Smooth`], and both handles take their mean length. A
    /// straight side becomes a curve first, since a line has no handle to
    /// mirror.
    Symmetric,
}

/// What [`plan_convert_segment`] makes of a segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SegmentKind {
    /// A straight line between the segment's two nodes; a curve's handles are
    /// discarded.
    Line,
    /// A cubic Bézier. A line becomes one with its handles at a third and two
    /// thirds of its length, which draws identically.
    Curve,
}

/// Distance in page units below which two points are the same node.
const SAME_POINT: f64 = 1e-6;

const RECT_DISCLOSURE: &str = "This shape was stored as a rectangle, which can only describe a box \
     with straight sides. It has been rewritten as a move, lines and curves so the change could \
     be made.";

const LINES_BECAME_CURVES: &str = "A straight side of this point had no handle to line up, so it \
     was turned into a curve first.";

const CLIP_DISCLOSURE: &str = "This shape is a clipping region: it draws nothing itself, it \
     controls which OTHER content on the page is visible. Reshaping it changes what shows \
     through elsewhere on the page.";

#[derive(Debug, Clone, Copy)]
enum Shape {
    Line,
    Cubic(Point, Point),
}

#[derive(Debug, Clone)]
enum Origin {
    /// A segment operator in the stream.
    Op {
        start: usize,
        end: usize,
        keyword: Vec<u8>,
        operands: Vec<f64>,
    },
    /// The closing edge an `h` draws.
    Close,
    /// An edge of an `re`, which is rewritten whole.
    Rect,
    /// A segment this edit adds.
    New,
}

#[derive(Debug, Clone)]
struct Seg {
    shape: Shape,
    origin: Origin,
    c1_new: bool,
    c2_new: bool,
    dirty: bool,
}

#[derive(Debug, Clone, Copy)]
struct Node {
    page: Point,
    /// The node's own operand pair, when the stream writes one.
    user: Option<[f64; 2]>,
}

/// One subpath, opened for editing. Segment `k` leaves `nodes[k]` and ends at
/// `nodes[(k + 1) % nodes.len()]`; a closing edge of non-zero length is the
/// last segment.
struct Work {
    nodes: Vec<Node>,
    segs: Vec<Seg>,
    closed: bool,
    rect: Option<(usize, usize)>,
    close_at: Option<usize>,
    inv: Matrix,
    disclosures: Vec<String>,
}

fn lerp(a: Point, b: Point, t: f64) -> Point {
    Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

fn len(v: Point) -> f64 {
    v.x.hypot(v.y)
}

fn unit(v: Point) -> Option<Point> {
    let l = len(v);
    (l > SAME_POINT).then(|| Point::new(v.x / l, v.y / l))
}

fn pair(operands: &[f64], at: usize) -> Option<[f64; 2]> {
    Some([*operands.get(at)?, *operands.get(at + 1)?])
}

impl Work {
    /// Open the subpath holding object-scoped node `node_index`; returns the
    /// node's index within it.
    fn open(
        content: &ContentStream,
        obj: &PathObject,
        node_index: usize,
    ) -> Result<(Self, usize), VectorEditError> {
        let inv = obj.ctm.inverse().ok_or(VectorEditError::DegenerateCtm)?;
        let anchors = enumerate_anchors(content, obj.tokens.start, obj.tokens.end);
        let count = anchors.len();
        let out_of_range = VectorEditError::NodeOutOfRange {
            index: node_index,
            count,
        };
        let mut first = 0usize;
        let page = obj.page_subpaths();
        let sp = page
            .iter()
            .find(|sp| {
                let n = sp.anchors().count();
                let hit = node_index < first + n;
                if !hit {
                    first += n;
                }
                hit
            })
            .ok_or(out_of_range)?;
        let n = sp.anchors().count();
        let sites = anchors
            .get(first..first + n)
            .ok_or(VectorEditError::MalformedOperand)?;
        let rect = match sites.first().map(|s| (s.kind, s.byte_start, s.byte_end)) {
            Some((AnchorKind::Rectangle { .. }, bs, be)) => Some((bs, be)),
            _ => None,
        };
        let nodes = nodes_of(sp, sites, rect.is_some());
        let segs = segs_of(sp, sites, rect.is_some())?;
        let close_at = if sp.closed && rect.is_none() {
            Some(close_of(content, obj, sites)?)
        } else {
            None
        };
        let mut work = Self {
            nodes,
            segs,
            closed: sp.closed,
            rect,
            close_at,
            inv,
            disclosures: Vec::new(),
        };
        if work.closed && work.closing_length() > SAME_POINT {
            let origin = if rect.is_some() {
                Origin::Rect
            } else {
                Origin::Close
            };
            work.segs.push(Seg::new(Shape::Line, origin));
        }
        Ok((work, node_index - first))
    }

    fn closing_length(&self) -> f64 {
        match (self.nodes.first(), self.nodes.last()) {
            (Some(a), Some(b)) => a.page.distance(b.page),
            _ => 0.0,
        }
    }

    fn node(&self, i: usize) -> Result<Node, VectorEditError> {
        let n = self.nodes.len().max(1);
        self.nodes
            .get(i % n)
            .copied()
            .ok_or(VectorEditError::MalformedOperand)
    }

    /// The segment ending at node `k`, wrapping to the last segment for the
    /// first node of a closed subpath.
    fn incoming(&self, k: usize) -> Option<usize> {
        if k > 0 {
            return Some(k - 1);
        }
        (self.closed && !self.segs.is_empty()).then(|| self.segs.len() - 1)
    }

    /// The segment leaving node `k`, wrapping to the first segment for the
    /// last node of a closed subpath whose last node sits on its first.
    fn outgoing(&self, k: usize) -> Option<usize> {
        if k < self.segs.len() {
            return Some(k);
        }
        let wraps = self.closed && k + 1 == self.nodes.len() && !self.segs.is_empty();
        wraps.then_some(0)
    }

    fn insert(&mut self, k: usize, t: f64, index: usize) -> Result<(), VectorEditError> {
        let seg = self
            .segs
            .get(k)
            .cloned()
            .ok_or(VectorEditError::NoSegmentHere { index })?;
        let (p0, p3) = (self.node(k)?.page, self.node(k + 1)?.page);
        let (first, mid) = match seg.shape {
            Shape::Line => (Shape::Line, lerp(p0, p3, t)),
            Shape::Cubic(c1, c2) => {
                let (a, b, c) = (lerp(p0, c1, t), lerp(c1, c2, t), lerp(c2, p3, t));
                let (d, e) = (lerp(a, b, t), lerp(b, c, t));
                if let Some(rest) = self.segs.get_mut(k) {
                    rest.set_c1(e);
                    rest.set_c2(c);
                }
                (Shape::Cubic(a, d), lerp(d, e, t))
            }
        };
        let mut new = Seg::new(first, Origin::New);
        new.dirty = true;
        self.segs.insert(k, new);
        self.nodes.insert(
            k + 1,
            Node {
                page: mid,
                user: None,
            },
        );
        Ok(())
    }

    fn convert_segment(
        &mut self,
        k: usize,
        kind: SegmentKind,
        index: usize,
    ) -> Result<(), VectorEditError> {
        let (p0, p3) = (self.node(k)?.page, self.node(k + 1)?.page);
        let seg = self
            .segs
            .get_mut(k)
            .ok_or(VectorEditError::NoSegmentHere { index })?;
        match (kind, seg.shape) {
            (SegmentKind::Line, Shape::Cubic(..)) => {
                seg.shape = Shape::Line;
                seg.dirty = true;
            }
            (SegmentKind::Curve, Shape::Line) => seg.curve_from_line(p0, p3),
            _ => {}
        }
        Ok(())
    }

    fn convert_node(
        &mut self,
        k: usize,
        kind: NodeKind,
        index: usize,
    ) -> Result<(), VectorEditError> {
        let at = self.node(k)?.page;
        let (inc, out) = (self.incoming(k), self.outgoing(k));
        if kind == NodeKind::Corner {
            if let Some(Shape::Cubic(..)) = inc.and_then(|i| self.segs.get(i)).map(|s| s.shape) {
                self.seg_mut(inc)?.set_c2(at);
            }
            if let Some(Shape::Cubic(..)) = out.and_then(|i| self.segs.get(i)).map(|s| s.shape) {
                self.seg_mut(out)?.set_c1(at);
            }
            return Ok(());
        }
        let (Some(i), Some(o)) = (inc, out) else {
            return Err(VectorEditError::NodeHasOneSide { index });
        };
        let prev = self.node(i)?.page;
        let next = self.node(o + 1)?.page;
        let is_line =
            |w: &Self, s: usize| matches!(w.segs.get(s).map(|s| s.shape), Some(Shape::Line));
        let promote_in = is_line(self, i) && (kind == NodeKind::Symmetric || is_line(self, o));
        let promote_out = is_line(self, o) && (kind == NodeKind::Symmetric || is_line(self, i));
        if promote_in || promote_out {
            self.disclosures.push(LINES_BECAME_CURVES.to_owned());
        }
        if promote_in {
            self.seg_mut(Some(i))?.curve_from_line(prev, at);
        }
        if promote_out {
            self.seg_mut(Some(o))?.curve_from_line(at, next);
        }
        let handle_in = match self.segs.get(i).map(|s| s.shape) {
            Some(Shape::Cubic(_, c2)) => Some(nonzero(sub(c2, at), sub(prev, at))),
            _ => None,
        };
        let handle_out = match self.segs.get(o).map(|s| s.shape) {
            Some(Shape::Cubic(c1, _)) => Some(nonzero(sub(c1, at), sub(next, at))),
            _ => None,
        };
        let dir = match (handle_in, handle_out) {
            (Some(a), Some(b)) => unit(sub(unit(b).unwrap_or(b), unit(a).unwrap_or(a)))
                .or_else(|| unit(sub(next, prev)))
                .or_else(|| unit(b)),
            (None, Some(_)) => unit(sub(at, prev)),
            (Some(_), None) => unit(sub(next, at)),
            (None, None) => None,
        };
        let Some(u) = dir else {
            return Ok(());
        };
        let (mut la, mut lb) = (handle_in.map_or(0.0, len), handle_out.map_or(0.0, len));
        if kind == NodeKind::Symmetric {
            let mean = (la + lb) / 2.0;
            (la, lb) = (mean, mean);
        }
        if handle_in.is_some() {
            let target = Point::new(at.x - u.x * la, at.y - u.y * la);
            self.seg_mut(Some(i))?.move_c2(target);
        }
        if handle_out.is_some() {
            let target = Point::new(at.x + u.x * lb, at.y + u.y * lb);
            self.seg_mut(Some(o))?.move_c1(target);
        }
        Ok(())
    }

    fn seg_mut(&mut self, k: Option<usize>) -> Result<&mut Seg, VectorEditError> {
        k.and_then(|k| self.segs.get_mut(k))
            .ok_or(VectorEditError::MalformedOperand)
    }

    fn user(&self, p: Point) -> [f64; 2] {
        let u = self.inv.map_point(p);
        [u.x, u.y]
    }

    /// The bytes that draw segment `k`; empty for a straight closing edge,
    /// which the `h` draws.
    fn seg_bytes(&self, k: usize) -> Result<Vec<u8>, VectorEditError> {
        let seg = self.segs.get(k).ok_or(VectorEditError::MalformedOperand)?;
        let start = self.node(k)?;
        let end = self.node(k + 1)?;
        let end_u = end.user.unwrap_or_else(|| self.user(end.page));
        let (keyword, operands): (&[u8], &[f64]) = match &seg.origin {
            Origin::Op {
                keyword, operands, ..
            } => (keyword, operands),
            _ => (b"", &[]),
        };
        let Shape::Cubic(c1, c2) = seg.shape else {
            if matches!(seg.origin, Origin::Close | Origin::Rect) && k + 1 == self.segs.len() {
                return Ok(Vec::new());
            }
            return Ok(emit_op(&end_u, b"l"));
        };
        let orig_c1 = match keyword {
            b"c" | b"y" => pair(operands, 0),
            _ => None,
        };
        let orig_c2 = match keyword {
            b"c" => pair(operands, 2),
            b"v" => pair(operands, 0),
            _ => None,
        };
        let c1_u = match orig_c1 {
            Some(p) if !seg.c1_new => p,
            _ => self.user(c1),
        };
        let c2_u = match orig_c2 {
            Some(p) if !seg.c2_new => p,
            _ => self.user(c2),
        };
        let bytes = if c1 == start.page {
            emit_op(&[c2_u[0], c2_u[1], end_u[0], end_u[1]], b"v")
        } else if c2 == end.page {
            emit_op(&[c1_u[0], c1_u[1], end_u[0], end_u[1]], b"y")
        } else {
            emit_op(
                &[c1_u[0], c1_u[1], c2_u[0], c2_u[1], end_u[0], end_u[1]],
                b"c",
            )
        };
        Ok(bytes)
    }

    /// Splice the edited subpath back into `content`.
    fn finish(self, content: &ContentStream, clip: bool) -> Result<PlannedEdit, VectorEditError> {
        let mut edits: Vec<(usize, usize, Vec<u8>)> = Vec::new();
        let mut disclosures = self.disclosures.clone();
        if let Some((start, end)) = self.rect {
            if self.segs.iter().any(|s| s.dirty) {
                let first = self.node(0)?;
                let mut bytes = emit_op(&first.user.unwrap_or_else(|| self.user(first.page)), b"m");
                for k in 0..self.segs.len() {
                    let seg = self.seg_bytes(k)?;
                    if !seg.is_empty() {
                        bytes.push(b' ');
                        bytes.extend_from_slice(&seg);
                    }
                }
                bytes.extend_from_slice(b" h");
                edits.push((start, end, bytes));
                disclosures.push(RECT_DISCLOSURE.to_owned());
            }
        } else {
            self.op_edits(content, &mut edits)?;
        }
        if clip && !edits.is_empty() {
            disclosures.push(CLIP_DISCLOSURE.to_owned());
        }
        Ok(PlannedEdit {
            operators_touched: edits.len(),
            content: splice(&content.buf, &mut edits),
            disclosures,
        })
    }

    fn op_edits(
        &self,
        content: &ContentStream,
        edits: &mut Vec<(usize, usize, Vec<u8>)>,
    ) -> Result<(), VectorEditError> {
        let mut pending: Vec<u8> = Vec::new();
        for (k, seg) in self.segs.iter().enumerate() {
            match &seg.origin {
                Origin::New => {
                    pending.extend_from_slice(&self.seg_bytes(k)?);
                    pending.push(b' ');
                }
                Origin::Op { start, end, .. } if seg.dirty || !pending.is_empty() => {
                    let mut bytes = std::mem::take(&mut pending);
                    if seg.dirty {
                        bytes.extend_from_slice(&self.seg_bytes(k)?);
                    } else {
                        let own = content
                            .buf
                            .get(*start..*end)
                            .ok_or(VectorEditError::MalformedOperand)?;
                        bytes.extend_from_slice(own);
                    }
                    edits.push((*start, *end, bytes));
                }
                Origin::Close => {
                    let mut bytes = std::mem::take(&mut pending);
                    let own = self.seg_bytes(k)?;
                    if !own.is_empty() {
                        bytes.extend_from_slice(&own);
                        bytes.push(b' ');
                    }
                    if !bytes.is_empty() {
                        let at = self.close_at.ok_or(VectorEditError::MalformedOperand)?;
                        edits.push((at, at, bytes));
                    }
                }
                _ => {}
            }
        }
        if pending.is_empty() {
            Ok(())
        } else {
            Err(VectorEditError::MalformedOperand)
        }
    }
}

/// The subpath's nodes, each with its own operand pair where one names it.
fn nodes_of(sp: &Subpath, sites: &[AnchorSite], rect: bool) -> Vec<Node> {
    let mut nodes: Vec<Node> = sp
        .anchors()
        .zip(sites)
        .map(|(page, site)| Node {
            page,
            user: match site.kind {
                AnchorKind::Editable => pair(&site.operands, site.pair_index * 2),
                _ => None,
            },
        })
        .collect();
    if rect
        && let Some(site) = sites.first()
        && let &[x, y, w, h] = site.operands.as_slice()
    {
        let corners = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
        for (node, c) in nodes.iter_mut().zip(corners) {
            node.user = Some(c);
        }
    }
    nodes
}

/// The subpath's written segments (not its closing edge).
fn segs_of(sp: &Subpath, sites: &[AnchorSite], rect: bool) -> Result<Vec<Seg>, VectorEditError> {
    let mut segs = Vec::with_capacity(sites.len());
    for (seg, site) in sp.segments.iter().zip(sites.iter().skip(1)) {
        let origin = if rect {
            Origin::Rect
        } else if site.kind == AnchorKind::Editable {
            Origin::Op {
                start: site.byte_start,
                end: site.byte_end,
                keyword: site.keyword.clone(),
                operands: site.operands.clone(),
            }
        } else {
            return Err(VectorEditError::MalformedOperand);
        };
        let shape = match *seg {
            Segment::Line { .. } => Shape::Line,
            Segment::Cubic { c1, c2, .. } => Shape::Cubic(c1, c2),
        };
        segs.push(Seg::new(shape, origin));
    }
    Ok(segs)
}

/// The byte offset of the `h` closing the subpath whose anchors are `sites`.
fn close_of(
    content: &ContentStream,
    obj: &PathObject,
    sites: &[AnchorSite],
) -> Result<usize, VectorEditError> {
    let after = sites.last().map_or(0, |s| s.byte_end);
    ops_in_range(content, obj.tokens.start, obj.tokens.end)
        .iter()
        .find(|op| op.byte_start() >= after && op.keyword(&content.buf) == Some(b"h".as_slice()))
        .map(|op| op.byte_start())
        .ok_or(VectorEditError::MalformedOperand)
}

/// A handle vector, or a third of the way to the neighbouring node when the
/// handle sits on its node and so has no direction.
fn nonzero(handle: Point, toward: Point) -> Point {
    if len(handle) > SAME_POINT {
        handle
    } else {
        Point::new(toward.x / 3.0, toward.y / 3.0)
    }
}

impl Seg {
    fn new(shape: Shape, origin: Origin) -> Self {
        Self {
            shape,
            origin,
            c1_new: false,
            c2_new: false,
            dirty: false,
        }
    }

    fn set_c1(&mut self, p: Point) {
        if let Shape::Cubic(_, c2) = self.shape {
            self.shape = Shape::Cubic(p, c2);
            (self.c1_new, self.dirty) = (true, true);
        }
    }

    fn set_c2(&mut self, p: Point) {
        if let Shape::Cubic(c1, _) = self.shape {
            self.shape = Shape::Cubic(c1, p);
            (self.c2_new, self.dirty) = (true, true);
        }
    }

    /// [`Seg::set_c1`] unless the handle is already there.
    fn move_c1(&mut self, p: Point) {
        if let Shape::Cubic(c1, _) = self.shape
            && c1.distance(p) > SAME_POINT
        {
            self.set_c1(p);
        }
    }

    fn move_c2(&mut self, p: Point) {
        if let Shape::Cubic(_, c2) = self.shape
            && c2.distance(p) > SAME_POINT
        {
            self.set_c2(p);
        }
    }

    fn curve_from_line(&mut self, from: Point, to: Point) {
        self.shape = Shape::Cubic(lerp(from, to, 1.0 / 3.0), lerp(from, to, 2.0 / 3.0));
        (self.c1_new, self.c2_new, self.dirty) = (true, true, true);
    }
}

/// Plan **inserting a node** on the segment leaving node `node_index`, at
/// parameter `t` along it (`0 < t < 1`; for a curve, the Bézier parameter, not
/// arc length).
///
/// The shape does not change: a line is split into two lines, a curve into two
/// curves by de Casteljau subdivision, and a closed subpath's closing edge gets
/// a line to the new node before its `h`. The new node is `node_index + 1`;
/// every later index in the object shifts up by one. An `re` rectangle is
/// rewritten as `m`/`l`/`h` first, and that is disclosed.
///
/// # Errors
///
/// [`VectorEditError::InvalidSegmentParameter`] for `t` outside `(0, 1)`,
/// [`VectorEditError::NodeOutOfRange`], [`VectorEditError::NoSegmentHere`]
/// (the last node of an open subpath, or of a closed one whose closing edge
/// has no length), [`VectorEditError::DegenerateCtm`],
/// [`VectorEditError::MalformedOperand`].
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{decompose, NoXObjects, Matrix, VectorObject};
/// use pdfcer_core::vector::edit::plan_insert_node;
///
/// let cs = ContentStream::parse(b"0 0 m 20 0 l S".to_vec()).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// let VectorObject::Path(path) = &model.objects[0] else { unreachable!() };
/// let plan = plan_insert_node(&cs, path, 0, 0.25).unwrap();
/// assert_eq!(plan.content, b"0 0 m 5 0 l 20 0 l S");
/// ```
pub fn plan_insert_node(
    content: &ContentStream,
    obj: &PathObject,
    node_index: usize,
    t: f64,
) -> Result<PlannedEdit, VectorEditError> {
    if !(t.is_finite() && t > 0.0 && t < 1.0) {
        return Err(VectorEditError::InvalidSegmentParameter);
    }
    let (mut work, k) = Work::open(content, obj, node_index)?;
    work.insert(k, t, node_index)?;
    work.finish(content, false)
}

/// Plan **converting the segment** leaving node `node_index` to a line or a
/// curve (see [`SegmentKind`]).
///
/// Asking for the kind the segment already is plans an unchanged buffer.
/// Turning a closed subpath's closing edge into a curve writes it as a `c`
/// ending on the first node, which adds one node at the end of that subpath
/// and shifts every later index in the object up by one.
///
/// # Errors
///
/// As [`plan_insert_node`], without `InvalidSegmentParameter`.
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{decompose, NoXObjects, Matrix, VectorObject};
/// use pdfcer_core::vector::edit::{plan_convert_segment, SegmentKind};
///
/// let cs = ContentStream::parse(b"0 0 m 30 0 l S".to_vec()).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// let VectorObject::Path(path) = &model.objects[0] else { unreachable!() };
/// let plan = plan_convert_segment(&cs, path, 0, SegmentKind::Curve).unwrap();
/// assert_eq!(plan.content, b"0 0 m 10 0 20 0 30 0 c S");
/// ```
pub fn plan_convert_segment(
    content: &ContentStream,
    obj: &PathObject,
    node_index: usize,
    kind: SegmentKind,
) -> Result<PlannedEdit, VectorEditError> {
    let (mut work, k) = Work::open(content, obj, node_index)?;
    work.convert_segment(k, kind, node_index)?;
    let clip = is_clipping_path(content, obj.tokens.start, obj.tokens.end);
    work.finish(content, clip)
}

/// Plan **converting node** `node_index` to a corner, smooth or symmetric node
/// (see [`NodeKind`]).
///
/// The node itself never moves; only the handles on either side change. On a
/// closed subpath whose last node sits on its first, the two are one node and
/// either index converts it. A node already of the asked kind plans an
/// unchanged buffer. Lines turned into curves are disclosed, and a closing
/// edge turned into a curve adds a node as in [`plan_convert_segment`].
///
/// # Errors
///
/// [`VectorEditError::NodeHasOneSide`] for smooth or symmetric at the end of
/// an open subpath, plus [`VectorEditError::NodeOutOfRange`],
/// [`VectorEditError::DegenerateCtm`], [`VectorEditError::MalformedOperand`].
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{decompose, NoXObjects, Matrix, VectorObject};
/// use pdfcer_core::vector::edit::{plan_convert_node, NodeKind};
///
/// // Two curves meeting at node 1. As a corner, each handle at node 1 sits on
/// // the node, so the curves are written with the shorter `y` and `v`.
/// let cs = ContentStream::parse(b"0 0 m 0 10 10 10 10 0 c 10 10 20 10 20 0 c S".to_vec()).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// let VectorObject::Path(path) = &model.objects[0] else { unreachable!() };
/// let plan = plan_convert_node(&cs, path, 1, NodeKind::Corner).unwrap();
/// assert_eq!(plan.content, b"0 0 m 0 10 10 0 y 20 10 20 0 v S");
/// ```
pub fn plan_convert_node(
    content: &ContentStream,
    obj: &PathObject,
    node_index: usize,
    kind: NodeKind,
) -> Result<PlannedEdit, VectorEditError> {
    let (mut work, k) = Work::open(content, obj, node_index)?;
    work.convert_node(k, kind, node_index)?;
    let clip = is_clipping_path(content, obj.tokens.start, obj.tokens.end);
    work.finish(content, clip)
}

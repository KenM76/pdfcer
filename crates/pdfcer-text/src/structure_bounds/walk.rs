//! The paint walk behind [`super::read_painted_bounds`]: one page's content,
//! with every form it invokes, reduced to the area each marked-content
//! sequence paints (ISO 32000-1 §8.5 path painting, §8.5.4 clipping, §8.9
//! images, §8.7.4.2 `sh`, §8.10.1 forms, §14.6 marked content).
//!
//! Geometry is in default user space and conservative: a Bézier segment
//! counts its control points, a stroke widens by half the line width scaled
//! by the CTM's larger axis (joins, caps and dashes are not modelled), and a
//! glyph-outline clip (`Tr` 4–7) does not shrink the clip.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use pdfcer_model::content::{ContentStream, ContentTokenKind, MAX_FORM_DEPTH, Operation};
use pdfcer_model::graph::ObjectGraph;
use pdfcer_model::object::{Dict, ObjId, Object};
use pdfcer_model::page_tree::{Page, Rect};
use pdfcer_model::view::DocumentView;

use crate::text_extract::ContentStreamRef;
use crate::text_extract::page::operand_object;

/// Operations one page may execute, forms included. Bounds the work a
/// hostile file can demand by invoking a form many times from a form that
/// is itself invoked many times (pdfcer policy, ARCHITECTURE.md §10).
pub(super) const MAX_OPERATIONS_PER_PAGE: usize = 4_000_000;

/// Graphics-state stack depth guard (pdfcer policy, as `text_extract`'s).
const MAX_STACK: usize = 256;

/// One marked-content sequence within a page: its stream and MCID.
pub(super) type SeqKey = (ContentStreamRef, u32);

/// The active clip, in default user space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Clip {
    /// No clip has been set.
    Unbounded,
    /// Paint is confined to this rectangle.
    Within(Rect),
    /// The clip has no area: nothing paints.
    Empty,
}

impl Clip {
    fn narrow(self, r: Rect) -> Self {
        match self {
            Self::Unbounded => Self::Within(r),
            Self::Within(c) => intersect(c, r).map_or(Self::Empty, Self::Within),
            Self::Empty => Self::Empty,
        }
    }

    /// The union of two clips (used for the clips text was shown under).
    pub(super) fn widen(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unbounded, _) | (_, Self::Unbounded) => Self::Unbounded,
            (Self::Empty, c) | (c, Self::Empty) => c,
            (Self::Within(a), Self::Within(b)) => Self::Within(union(a, b)),
        }
    }

    /// `r` confined to this clip, or `None` when nothing of it shows.
    pub(super) fn apply(self, r: Rect) -> Option<Rect> {
        match self {
            Self::Unbounded => Some(r),
            Self::Within(c) => intersect(c, r),
            Self::Empty => None,
        }
    }
}

/// What one page's walk found.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct PageBounds {
    /// Non-text paint per sequence, clipped.
    pub sequences: HashMap<SeqKey, Rect>,
    /// The union of the clips text was shown under, per sequence.
    pub text_clips: HashMap<SeqKey, Clip>,
    /// Every sequence a `BDC` declared.
    pub declared: HashSet<SeqKey>,
    /// Where each indirect XObject was placed: an image's unit square, a
    /// form's `/BBox`, under the CTM and clipped.
    pub placements: HashMap<ObjId, Rect>,
    /// The walk stopped at [`MAX_OPERATIONS_PER_PAGE`].
    pub budget_exhausted: bool,
    /// Form invocations skipped at [`MAX_FORM_DEPTH`].
    pub form_depth_overflows: usize,
    /// Form invocations skipped because the form was already executing.
    pub form_cycles: usize,
}

/// A 2-D affine matrix `[a b c d e f]` (§8.3.4).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Affine([f64; 6]);

impl Affine {
    const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// `self × then`: apply `self` first, then `then`.
    fn then(self, then: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = then.0;
        Self([
            a * a2 + b * c2,
            a * b2 + b * d2,
            c * a2 + d * c2,
            c * b2 + d * d2,
            e * a2 + f * c2 + e2,
            e * b2 + f * d2 + f2,
        ])
    }

    fn apply(self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    /// The larger of the lengths the unit axes map to.
    fn scale(self) -> f64 {
        let [a, b, c, d, ..] = self.0;
        a.hypot(b).max(c.hypot(d))
    }

    fn map_rect(self, r: Rect) -> Option<Rect> {
        let mut acc = None;
        for (x, y) in [
            (r.llx, r.lly),
            (r.urx, r.lly),
            (r.llx, r.ury),
            (r.urx, r.ury),
        ] {
            let (x, y) = self.apply(x, y);
            acc = Some(extend(acc, x, y)?);
        }
        acc
    }
}

/// The overlap of two rectangles, zero-area overlaps included (a hairline
/// on a clip edge still paints).
pub(super) fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let r = Rect {
        llx: a.llx.max(b.llx),
        lly: a.lly.max(b.lly),
        urx: a.urx.min(b.urx),
        ury: a.ury.min(b.ury),
    };
    (r.urx >= r.llx && r.ury >= r.lly).then_some(r)
}

/// The smallest rectangle containing both `a` and `b`.
pub(super) fn union(a: Rect, b: Rect) -> Rect {
    Rect {
        llx: a.llx.min(b.llx),
        lly: a.lly.min(b.lly),
        urx: a.urx.max(b.urx),
        ury: a.ury.max(b.ury),
    }
}

/// `acc` grown to cover `(x, y)`; `None` for a non-finite point.
fn extend(acc: Option<Rect>, x: f64, y: f64) -> Option<Rect> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let p = Rect {
        llx: x,
        lly: y,
        urx: x,
        ury: y,
    };
    Some(acc.map_or(p, |a| union(a, p)))
}

#[derive(Debug, Clone, Copy)]
struct Gs {
    ctm: Affine,
    clip: Clip,
    line_width: f64,
}

struct Walker<'a, 'v> {
    doc: &'a DocumentView<'v>,
    crop: Rect,
    gs: Gs,
    stack: Vec<Gs>,
    floor: usize,
    /// The current path's points under the CTM; `Err` once a point was
    /// non-finite (the path then paints nothing).
    path: Option<Result<Rect, ()>>,
    clip_pending: bool,
    marked: Vec<Option<SeqKey>>,
    marked_floor: usize,
    stream: ContentStreamRef,
    active_forms: Vec<u32>,
    forms: HashMap<u32, Option<Rc<ContentStream>>>,
    ops_left: usize,
    out: PageBounds,
}

/// Walk one page's content.
pub(super) fn walk_page(
    doc: &DocumentView<'_>,
    page: &Page,
    content: &ContentStream,
) -> PageBounds {
    let mut w = Walker {
        doc,
        crop: page.crop_box,
        gs: Gs {
            ctm: Affine::IDENTITY,
            clip: Clip::Unbounded,
            line_width: 1.0,
        },
        stack: Vec::new(),
        floor: 0,
        path: None,
        clip_pending: false,
        marked: Vec::new(),
        marked_floor: 0,
        stream: ContentStreamRef::Page,
        active_forms: Vec::new(),
        forms: HashMap::new(),
        ops_left: MAX_OPERATIONS_PER_PAGE,
        out: PageBounds::default(),
    };
    w.run(content, &page.resources);
    w.out
}

impl Walker<'_, '_> {
    fn run(&mut self, content: &ContentStream, resources: &Dict) {
        for op in content.operations() {
            if self.ops_left == 0 {
                self.out.budget_exhausted = true;
                return;
            }
            self.ops_left -= 1;
            match &op.operator.kind {
                ContentTokenKind::InlineImage { .. } => {
                    self.paint_unit_square();
                }
                _ => {
                    if let Some(name) = op.operator_name(&content.buf) {
                        self.operator(name, &op, resources);
                    }
                }
            }
        }
    }

    fn operator(&mut self, name: &[u8], op: &Operation<'_>, resources: &Dict) {
        match name {
            b"q" => {
                if self.stack.len() < MAX_STACK {
                    self.stack.push(self.gs);
                }
            }
            b"Q" => {
                if self.stack.len() > self.floor
                    && let Some(gs) = self.stack.pop()
                {
                    self.gs = gs;
                }
            }
            b"cm" => {
                if let [a, b, c, d, e, f] = numbers(op).as_slice() {
                    let m = Affine([*a, *b, *c, *d, *e, *f]);
                    self.gs.ctm = m.then(self.gs.ctm);
                }
            }
            b"w" => {
                if let [lw] = numbers(op).as_slice() {
                    self.gs.line_width = lw.abs();
                }
            }
            b"gs" => self.ext_gstate(op, resources),
            b"m" | b"l" | b"c" | b"v" | b"y" => self.path_points(op),
            b"re" => self.path_rect(op),
            b"W" | b"W*" => self.clip_pending = true,
            b"S" | b"s" => self.paint_path(false, true),
            b"f" | b"F" | b"f*" => self.paint_path(true, false),
            b"B" | b"B*" | b"b" | b"b*" => self.paint_path(true, true),
            b"n" => self.paint_path(false, false),
            b"Tj" | b"TJ" | b"'" | b"\"" => self.text_shown(),
            b"sh" => self.shade(),
            b"Do" => self.do_xobject(op, resources),
            b"BMC" => self.begin_marked(None),
            b"BDC" => {
                let props = self.properties(op, resources);
                self.begin_marked(props);
            }
            b"EMC" if self.marked.len() > self.marked_floor => {
                self.marked.pop();
            }
            _ => {}
        }
    }

    fn key(&self) -> Option<SeqKey> {
        self.marked.last().copied().flatten()
    }

    /// Record `area` (already in default user space) for the current
    /// sequence, clipped.
    fn paint(&mut self, area: Rect) {
        let Some(key) = self.key() else {
            return;
        };
        if let Some(r) = self.gs.clip.apply(area) {
            let slot = self.out.sequences.entry(key).or_insert(r);
            *slot = union(*slot, r);
        }
    }

    fn path_points(&mut self, op: &Operation<'_>) {
        let n = numbers(op);
        for [x, y] in n.as_chunks::<2>().0 {
            let (x, y) = self.gs.ctm.apply(*x, *y);
            self.path = Some(match self.path.take() {
                Some(Err(())) => Err(()),
                Some(Ok(r)) => extend(Some(r), x, y).ok_or(()),
                None => extend(None, x, y).ok_or(()),
            });
        }
    }

    fn path_rect(&mut self, op: &Operation<'_>) {
        if let [x, y, w, h] = numbers(op).as_slice() {
            let r = Rect::from_corners(*x, *y, x + w, y + h);
            let mapped = self.gs.ctm.map_rect(r).ok_or(());
            self.path = Some(match (self.path.take(), mapped) {
                (Some(Err(())), _) | (_, Err(())) => Err(()),
                (Some(Ok(a)), Ok(b)) => Ok(union(a, b)),
                (None, Ok(b)) => Ok(b),
            });
        }
    }

    /// A path-painting operator: paint, then apply a pending `W` (§8.5.4:
    /// the clip takes effect after painting) and end the path.
    fn paint_path(&mut self, fill: bool, stroke: bool) {
        let path = self.path.take();
        if let Some(Ok(r)) = path {
            if stroke {
                let half = self.gs.line_width * self.gs.ctm.scale() / 2.0;
                if half.is_finite() {
                    self.paint(Rect {
                        llx: r.llx - half,
                        lly: r.lly - half,
                        urx: r.urx + half,
                        ury: r.ury + half,
                    });
                }
            } else if fill {
                self.paint(r);
            }
        }
        if std::mem::take(&mut self.clip_pending) {
            self.gs.clip = match path {
                Some(Ok(r)) => self.gs.clip.narrow(r),
                _ => Clip::Empty,
            };
        }
    }

    fn text_shown(&mut self) {
        if let Some(key) = self.key() {
            let clip = self.gs.clip;
            let slot = self.out.text_clips.entry(key).or_insert(Clip::Empty);
            *slot = slot.widen(clip);
        }
    }

    /// `sh` fills the clip; with no clip set, the page's crop box.
    fn shade(&mut self) {
        let area = match self.gs.clip {
            Clip::Within(r) => r,
            Clip::Unbounded => self.crop,
            Clip::Empty => return,
        };
        self.paint(area);
    }

    fn paint_unit_square(&mut self) -> Option<Rect> {
        let unit = Rect::from_corners(0.0, 0.0, 1.0, 1.0);
        let r = self.gs.ctm.map_rect(unit)?;
        self.paint(r);
        self.gs.clip.apply(r)
    }

    fn ext_gstate(&mut self, op: &Operation<'_>, resources: &Dict) {
        let Some(name) = operand_name(op) else {
            return;
        };
        let lw = self
            .doc
            .resolve(resources.get(b"ExtGState").unwrap_or(&Object::Null))
            .as_dict()
            .and_then(|d| d.get(name))
            .map(|o| self.doc.resolve(o))
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"LW"))
            .and_then(|o| self.doc.resolve(o).as_number());
        if let Some(lw) = lw {
            self.gs.line_width = lw.abs();
        }
    }

    fn properties(&self, op: &Operation<'_>, resources: &Dict) -> Option<u32> {
        let props = match op.operands.last().and_then(operand_object)? {
            Object::Dict(d) => d,
            Object::Name(n) => self
                .doc
                .resolve(resources.get(b"Properties")?)
                .as_dict()?
                .get(n.as_bytes())
                .map(|o| self.doc.resolve(o))
                .and_then(Object::as_dict)?,
            _ => return None,
        };
        let mcid = self.doc.resolve(props.get(b"MCID")?).as_int()?;
        u32::try_from(mcid).ok()
    }

    /// Push a marked-content level. An `/MCID` opens a sequence of its own
    /// (the innermost owner wins, as in `text_extract`); a level without
    /// one inherits the enclosing sequence.
    fn begin_marked(&mut self, mcid: Option<u32>) {
        if self.marked.len() >= MAX_STACK {
            return;
        }
        let key = match mcid {
            Some(m) => {
                let key = (self.stream, m);
                self.out.declared.insert(key);
                Some(key)
            }
            None => self.key(),
        };
        self.marked.push(key);
    }

    fn do_xobject(&mut self, op: &Operation<'_>, resources: &Dict) {
        let doc = self.doc;
        let Some(entry) = operand_name(op).and_then(|name| {
            doc.resolve(resources.get(b"XObject").unwrap_or(&Object::Null))
                .as_dict()
                .and_then(|d| d.get(name))
        }) else {
            return;
        };
        let id = entry.as_reference();
        let Object::Stream(stream) = doc.resolve(entry) else {
            return;
        };
        let subtype = doc
            .resolve(stream.dict.get(b"Subtype").unwrap_or(&Object::Null))
            .as_name()
            .map(|n| n.as_bytes().to_vec());
        let placed = match subtype.as_deref() {
            Some(b"Image") => self.paint_unit_square(),
            Some(b"Form") => self.run_form(id, &stream.dict, resources),
            _ => None,
        };
        if let (Some(id), Some(r)) = (id, placed) {
            let slot = self.out.placements.entry(id).or_insert(r);
            *slot = union(*slot, r);
        }
    }

    /// Execute a form XObject (§8.10.1); returns its clipped `/BBox`.
    fn run_form(&mut self, id: Option<ObjId>, dict: &Dict, resources: &Dict) -> Option<Rect> {
        let num = id.map(|i| i.num);
        if self.active_forms.len() >= MAX_FORM_DEPTH {
            self.out.form_depth_overflows += 1;
            return None;
        }
        if num.is_some_and(|n| self.active_forms.contains(&n)) {
            self.out.form_cycles += 1;
            return None;
        }
        let doc = self.doc;
        let matrix = matrix_of(doc, dict).unwrap_or(Affine::IDENTITY);
        let bbox = dict
            .get(b"BBox")
            .and_then(|o| pdfcer_model::page_tree::parse_rect(doc, o, "BBox").ok());
        let ctm = matrix.then(self.gs.ctm);
        let placed = bbox.and_then(|b| ctm.map_rect(b));
        let Some(content) = self.form_content(id) else {
            return placed.and_then(|r| self.gs.clip.apply(r));
        };
        let inner = doc
            .resolve(dict.get(b"Resources").unwrap_or(&Object::Null))
            .as_dict()
            .cloned()
            .unwrap_or_else(|| resources.clone());
        let saved = (self.gs, self.floor, self.marked_floor, self.stream);
        let saved_marked = self.marked.len();
        self.gs.ctm = ctm;
        if let Some(r) = placed {
            self.gs.clip = self.gs.clip.narrow(r);
        }
        let shown = placed.and_then(|r| saved.0.clip.apply(r));
        self.floor = self.stack.len();
        self.marked_floor = saved_marked;
        if let Some(n) = num {
            self.stream = ContentStreamRef::Form { object: n };
            self.active_forms.push(n);
        }
        self.path = None;
        self.run(&content, &inner);
        if num.is_some() {
            self.active_forms.pop();
        }
        self.stack.truncate(self.floor);
        self.marked.truncate(saved_marked);
        (self.gs, self.floor, self.marked_floor, self.stream) = saved;
        self.path = None;
        self.clip_pending = false;
        shown
    }

    /// A form's parsed content, decoded once per page walk.
    fn form_content(&mut self, id: Option<ObjId>) -> Option<Rc<ContentStream>> {
        let id = id?;
        if let Some(cached) = self.forms.get(&id.num) {
            return cached.clone();
        }
        let parsed = ContentStream::from_form(self.doc, id).ok().map(Rc::new);
        self.forms.insert(id.num, parsed.clone());
        parsed
    }
}

/// The numeric operands, in order; any non-number makes the list short so
/// the caller's slice pattern fails.
fn numbers(op: &Operation<'_>) -> Vec<f64> {
    op.operands
        .iter()
        .filter_map(operand_object)
        .filter_map(Object::as_number)
        .collect()
}

fn operand_name<'o>(op: &Operation<'o>) -> Option<&'o [u8]> {
    op.operands
        .first()
        .and_then(operand_object)
        .and_then(Object::as_name)
        .map(|n| n.as_bytes())
}

fn matrix_of(doc: &DocumentView<'_>, dict: &Dict) -> Option<Affine> {
    let items = doc.resolve(dict.get(b"Matrix")?).as_array()?;
    let v: Vec<f64> = items
        .iter()
        .filter_map(|o| doc.resolve(o).as_number())
        .collect();
    match v.as_slice() {
        [a, b, c, d, e, f] => Some(Affine([*a, *b, *c, *d, *e, *f])),
        _ => None,
    }
}

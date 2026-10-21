//! Convert smooth lines to curves (Pass 616.3): a run of straight `l`
//! segments tracing a curve, as CAD exporters write arcs and splines,
//! becomes fitted cubic `c` segments (ISO 32000-2 §8.5.2.2, Table 58), or
//! one `l` where it is straight, within a tolerance measured on the page.
//!
//! What it keeps: every run's end points exactly, every corner sharper
//! than 30°, and every dashed path (moving a vertex moves the dashes).
//! A run is rewritten only when the rewrite is shorter. Only page content
//! is scanned; form XObjects are left as they are.

use crate::content::{ContentStream, ContentTokenKind, Operation};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::vector::geometry::{Matrix, Point};
use crate::view::DocumentView;
use crate::writer::content::emit_number;

use super::fragments::numbers;
use super::objects::{DiscardObject, ObjectDiscard};
use super::smooth_fit::{self, Piece};

/// Segments a run needs before fitting is worth trying.
const MIN_SEGMENTS: usize = 4;
/// Vertices fitted at once; a longer run is fitted in pieces of this size.
const MAX_RUN: usize = 4096;
/// The cosine of 30°: a sharper turn is a corner and stays one.
const CORNER_COS: f64 = 0.866_025_403_784_438_6;
/// Graphics-state nesting tracked; deeper `q` stops the scan.
const MAX_NESTING: usize = 4096;

/// A run of consecutive `l` operators in one subpath.
struct Run {
    /// From the first `l`'s first operand to the last `l`.
    start: usize,
    end: usize,
    /// The current point before the run, then each `l`'s point, in user
    /// space.
    points: Vec<Point>,
    ctm: Matrix,
}

/// One page's rewrite.
#[derive(Debug, Default)]
pub(crate) struct Smoothed {
    /// `(start, end, bytes)` replacements in the decoded content.
    pub(crate) edits: Vec<(usize, usize, Vec<u8>)>,
    pub(crate) segments: usize,
    pub(crate) curves: usize,
    pub(crate) lines: usize,
    /// The largest distance, in points, any replacement moved from its
    /// original lines.
    pub(crate) deviation: f64,
    /// Content bytes saved, before compression.
    pub(crate) saved: u64,
}

/// The smoothing of `content`, drawn with `resources`, at `tol` points.
pub(crate) fn smooth_content(
    view: &DocumentView<'_>,
    content: &ContentStream,
    resources: &Dict,
    tol: f64,
) -> Smoothed {
    let mut out = Smoothed::default();
    if !(tol.is_finite() && tol > 0.0) {
        return out;
    }
    for run in runs(view, content, resources) {
        let Some(original) = content.buf.get(run.start..run.end) else {
            continue;
        };
        let Some(r) = rewrite(&run, tol) else {
            continue;
        };
        if r.bytes.len() < original.len() {
            out.segments += run.points.len() - 1;
            out.curves += r.curves;
            out.lines += r.lines;
            out.deviation = out.deviation.max(r.deviation);
            out.saved += (original.len() - r.bytes.len()) as u64;
            out.edits.push((run.start, run.end, r.bytes));
        }
    }
    out
}

/// One item per page with a run worth smoothing at `tol` points.
pub(crate) fn smooth_carriers(view: &DocumentView<'_>, tol: f64) -> Vec<ObjectDiscard> {
    let pages = crate::page_tree::pages_in(view).unwrap_or_default();
    let mut out = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        let Ok(content) = ContentStream::from_page(view, page) else {
            continue;
        };
        let s = smooth_content(view, &content, &page.resources, tol);
        if s.edits.is_empty() {
            continue;
        }
        out.push(ObjectDiscard {
            object: DiscardObject::SmoothLines,
            owner: page.id,
            location: format!("page {}: {}", index + 1, s.describe()),
            bytes: s.saved,
            strips: Vec::new(),
            rewrites: Vec::new(),
            drops: Vec::new(),
        });
    }
    out
}

impl Smoothed {
    /// `120 line segments into 6 curves and 2 lines (max deviation 0.031 pt)`.
    pub(crate) fn describe(&self) -> String {
        let lines = match self.lines {
            0 => String::new(),
            1 => " and 1 line".to_owned(),
            n => format!(" and {n} lines"),
        };
        format!(
            "{} line segments into {} curves{lines} (max deviation {:.3} pt)",
            self.segments, self.curves, self.deviation
        )
    }
}

/// Scan state: the CTM and whether the line is dashed, saved by `q`.
#[derive(Clone, Copy)]
struct Gs {
    ctm: Matrix,
    dashed: bool,
}

fn last_point(n: &[f64]) -> Option<Point> {
    match n {
        [.., x, y] => Some(Point::new(*x, *y)),
        _ => None,
    }
}

/// Every run of at least `MIN_SEGMENTS` segments drawn undashed under an
/// invertible CTM.
fn runs(view: &DocumentView<'_>, content: &ContentStream, resources: &Dict) -> Vec<Run> {
    let buf = &content.buf;
    let mut out = Vec::new();
    let mut gs = Gs {
        ctm: Matrix::IDENTITY,
        dashed: false,
    };
    let mut stack: Vec<Gs> = Vec::new();
    let (mut current, mut subpath) = (None, None);
    let mut run: Option<Run> = None;
    for op in content.operations() {
        let name = op.operator_name(buf).unwrap_or_default();
        let nums = numbers(&op);
        if let (b"l", Some([x, y])) = (name, nums.as_deref()) {
            let p = Point::new(*x, *y);
            match (&mut run, current) {
                (Some(r), _) => r.points.push(p),
                (None, Some(c)) => {
                    let start = op
                        .operands
                        .first()
                        .map_or(op.operator.span.start, |t| t.span.start);
                    run = Some(Run {
                        start,
                        end: 0,
                        points: vec![c, p],
                        ctm: gs.ctm,
                    });
                }
                (None, None) => {}
            }
            if let Some(r) = &mut run {
                r.end = op.operator.span.end();
            }
            current = Some(p);
            continue;
        }
        if let Some(r) = run.take()
            && r.points.len() > MIN_SEGMENTS
            && !gs.dashed
            && r.ctm.is_invertible()
        {
            out.push(r);
        }
        let nums = nums.unwrap_or_default();
        current = match name {
            b"m" => {
                subpath = last_point(&nums);
                subpath
            }
            b"c" | b"v" | b"y" => last_point(&nums),
            b"re" => match nums.as_slice() {
                [x, y, _, _] => Some(Point::new(*x, *y)),
                _ => None,
            },
            b"h" => subpath,
            _ => None,
        };
        match name {
            b"q" if stack.len() < MAX_NESTING => stack.push(gs),
            b"q" => break,
            b"Q" => gs = stack.pop().unwrap_or(gs),
            b"cm" => {
                if let [a, b, c, d, e, f] = nums.as_slice() {
                    gs.ctm = Matrix::new(*a, *b, *c, *d, *e, *f).post_concat(gs.ctm);
                }
            }
            b"d" => gs.dashed = dash_operand(op.operands.first()),
            b"gs" => {
                if let Some(dashed) = ext_gstate_dash(view, resources, &op) {
                    gs.dashed = dashed;
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether a `d` operator's dash array (or an ExtGState `/D` entry's first
/// element) dashes the line: any non-empty array does.
fn dash_operand(token: Option<&crate::content::ContentToken>) -> bool {
    match token.map(|t| &t.kind) {
        Some(ContentTokenKind::Operand(Object::Array(a))) => !a.is_empty(),
        _ => true,
    }
}

/// The dash an ExtGState sets (Table 57 `/D`), when it sets one.
fn ext_gstate_dash(view: &DocumentView<'_>, resources: &Dict, op: &Operation<'_>) -> Option<bool> {
    let name = match op.operands.first().map(|t| &t.kind) {
        Some(ContentTokenKind::Operand(Object::Name(n))) => n,
        _ => return None,
    };
    let states = view.resolve(resources.get(b"ExtGState")?).as_dict()?;
    let state = view.resolve(states.get(name.as_bytes())?).as_dict()?;
    let dash = view.resolve(state.get(b"D")?).as_array()?;
    let pattern = view.resolve(dash.first()?).as_array()?;
    Some(!pattern.is_empty())
}

struct Rewrite {
    bytes: Vec<u8>,
    curves: usize,
    lines: usize,
    deviation: f64,
}

/// The run's replacement: fitted in page space, emitted in user space with
/// the original end points, each curve re-measured after rounding.
fn rewrite(run: &Run, tol: f64) -> Option<Rewrite> {
    let inverse = run.ctm.inverse()?;
    let page: Vec<Point> = run.points.iter().map(|&p| run.ctm.map_point(p)).collect();
    let mut kept: Vec<usize> = vec![0];
    for (i, p) in page.iter().enumerate().skip(1) {
        let last = kept.last().and_then(|&k| page.get(k))?;
        if p.distance(*last) > 1e-9 {
            kept.push(i);
        }
    }
    let pts: Vec<Point> = kept.iter().filter_map(|&i| page.get(i).copied()).collect();
    let mut e = Emitter {
        run,
        kept: &kept,
        pts: &pts,
        inverse,
        scale: 10f64.powi(decimals(run.ctm, tol)),
        tol,
        out: Rewrite {
            bytes: Vec::new(),
            curves: 0,
            lines: 0,
            deviation: 0.0,
        },
    };
    let mut from = 0;
    while from + 1 < pts.len() {
        let to = (from + MAX_RUN).min(pts.len() - 1);
        let mut start = from;
        for piece in smooth_fit::fit(pts.get(from..=to)?, tol * 0.9, CORNER_COS) {
            let end = piece.end() + from;
            match piece {
                Piece::Line { .. } => e.line(start, end),
                Piece::Curve { c1, c2, .. } => e.curve(start, end, c1, c2),
            }
            start = end;
        }
        from = to;
    }
    Some(e.out)
}

/// Decimal places for a control point: rounding moves it, on the page, at
/// most a twentieth of `tol` (the Frobenius norm bounds the CTM's stretch).
fn decimals(ctm: Matrix, tol: f64) -> i32 {
    let stretch = (ctm.a * ctm.a + ctm.b * ctm.b + ctm.c * ctm.c + ctm.d * ctm.d).sqrt();
    let places = (stretch / (0.07 * tol)).log10().ceil();
    if places.is_finite() {
        places.clamp(0.0, 6.0) as i32
    } else {
        6
    }
}

struct Emitter<'a> {
    run: &'a Run,
    /// For each fitted vertex, its index in `run.points`.
    kept: &'a [usize],
    /// The fitted vertices, on the page.
    pts: &'a [Point],
    inverse: Matrix,
    scale: f64,
    tol: f64,
    out: Rewrite,
}

impl Emitter<'_> {
    fn page(&self, i: usize) -> Point {
        self.pts.get(i).copied().unwrap_or(Point::new(0.0, 0.0))
    }

    fn numbers(&mut self, values: &[f64], op: &[u8]) {
        if !self.out.bytes.is_empty() {
            self.out.bytes.push(b'\n');
        }
        for v in values {
            emit_number(&mut self.out.bytes, *v);
            self.out.bytes.push(b' ');
        }
        self.out.bytes.extend_from_slice(op);
    }

    /// The original point of fitted vertex `i`.
    fn original(&self, i: usize) -> Point {
        self.kept
            .get(i)
            .and_then(|&k| self.run.points.get(k))
            .copied()
            .unwrap_or(Point::new(0.0, 0.0))
    }

    fn line(&mut self, start: usize, end: usize) {
        let d = smooth_fit::deviation(
            self.pts.get(start..=end).unwrap_or(&[]),
            smooth_fit::straight(self.page(start), self.page(end)),
        );
        self.out.deviation = self.out.deviation.max(d);
        let p = self.original(end);
        self.numbers(&[p.x, p.y], b"l");
        self.out.lines += 1;
    }

    fn round(&self, page: Point) -> Point {
        let p = self.inverse.map_point(page);
        Point::new(
            (p.x * self.scale).round() / self.scale,
            (p.y * self.scale).round() / self.scale,
        )
    }

    /// The curve, re-measured with its rounded control points; past the
    /// tolerance the original lines stay.
    fn curve(&mut self, start: usize, end: usize, c1: Point, c2: Point) {
        let (u1, u2) = (self.round(c1), self.round(c2));
        let bez = [
            self.page(start),
            self.run.ctm.map_point(u1),
            self.run.ctm.map_point(u2),
            self.page(end),
        ];
        let d = smooth_fit::deviation(self.pts.get(start..=end).unwrap_or(&[]), bez);
        if d > self.tol {
            for i in start + 1..=end {
                let p = self.original(i);
                self.numbers(&[p.x, p.y], b"l");
                self.out.lines += 1;
            }
            return;
        }
        self.out.deviation = self.out.deviation.max(d);
        let p = self.original(end);
        self.numbers(&[u1.x, u1.y, u2.x, u2.y, p.x, p.y], b"c");
        self.out.curves += 1;
    }
}

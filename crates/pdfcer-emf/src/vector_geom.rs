//! Recorded geometry as `pdfcer-flatten` shapes: device-space polylines of a
//! fill or a stroke outline, cut by the op's clip chain.

use pdfcer_flatten::{FillRule as Rule, Point, Shape};
use pdfcer_render::writer_seam::{ClipDef, ClipId, DeviceBounds};
use tiny_skia::{FillRule, Path, PathBuilder, PathSegment, Stroke, Transform};

/// Most line segments one curve is cut into.
const MAX_STEPS: f32 = 64.0;

fn rule(r: FillRule) -> Rule {
    match r {
        FillRule::Winding => Rule::NonZero,
        FillRule::EvenOdd => Rule::EvenOdd,
    }
}

/// `path` under `ctm`, filled by `r`. `None` when the transform fails.
pub(super) fn fill(path: &Path, r: FillRule, ctm: Transform) -> Option<Shape> {
    let device = path.clone().transform(ctm)?;
    Some(Shape {
        contours: polylines(&device),
        rule: rule(r),
    })
}

/// The outline `stroke` paints along `path`, dash applied in path space as
/// the renderer applies it. `None` when the stroker produces nothing.
pub(super) fn stroke(path: &Path, stroke: &Stroke, ctm: Transform) -> Option<Shape> {
    let det = (ctm.sx * ctm.sy - ctm.kx * ctm.ky).abs();
    let res = if det.is_finite() && det > 0.0 {
        det.sqrt()
    } else {
        1.0
    };
    let dashed = match &stroke.dash {
        Some(d) => path.dash(d, res)?,
        None => path.clone(),
    };
    let outline = dashed.stroke(stroke, res)?;
    fill(&outline, FillRule::Winding, ctm)
}

/// How many chords a curve with control polygon length `len` (device px)
/// is cut into: the sagitta of a chord `c` on radius `r` is `c²/8r`, so
/// chords of `√len / 2` keep it well under a pixel at any radius the
/// curve can have.
fn steps(len: f32) -> usize {
    // Clamped to 1..=MAX_STEPS before the cast.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = (len.max(0.0).sqrt() * 2.0).ceil().clamp(1.0, MAX_STEPS) as usize;
    n
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// The contours of `p`, curves cut into chords. Open subpaths are closed:
/// a fill closes them.
fn polylines(p: &Path) -> Vec<Vec<Point>> {
    let mut out = Vec::new();
    let mut cur: Vec<Point> = Vec::new();
    let mut last = (0.0f32, 0.0f32);
    let push = |cur: &mut Vec<Point>, q: (f32, f32)| {
        cur.push([f64::from(q.0), f64::from(q.1)]);
    };
    for seg in p.segments() {
        match seg {
            PathSegment::MoveTo(q) => {
                flush(&mut out, &mut cur);
                last = (q.x, q.y);
                push(&mut cur, last);
            }
            PathSegment::LineTo(q) => {
                last = (q.x, q.y);
                push(&mut cur, last);
            }
            PathSegment::QuadTo(c, q) => {
                let (c, q) = ((c.x, c.y), (q.x, q.y));
                let n = steps(dist(last, c) + dist(c, q));
                for k in 1..=n {
                    // n <= MAX_STEPS, exact in f32.
                    #[allow(clippy::cast_precision_loss)]
                    let t = k as f32 / n as f32;
                    let u = 1.0 - t;
                    let at = |a: f32, b: f32, e: f32| u * u * a + 2.0 * u * t * b + t * t * e;
                    push(&mut cur, (at(last.0, c.0, q.0), at(last.1, c.1, q.1)));
                }
                last = q;
            }
            PathSegment::CubicTo(c1, c2, q) => {
                let (c1, c2, q) = ((c1.x, c1.y), (c2.x, c2.y), (q.x, q.y));
                let n = steps(dist(last, c1) + dist(c1, c2) + dist(c2, q));
                for k in 1..=n {
                    // n <= MAX_STEPS, exact in f32.
                    #[allow(clippy::cast_precision_loss)]
                    let t = k as f32 / n as f32;
                    let u = 1.0 - t;
                    let at = |a: f32, b: f32, c: f32, e: f32| {
                        u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * e
                    };
                    push(
                        &mut cur,
                        (at(last.0, c1.0, c2.0, q.0), at(last.1, c1.1, c2.1, q.1)),
                    );
                }
                last = q;
            }
            PathSegment::Close => flush(&mut out, &mut cur),
        }
    }
    flush(&mut out, &mut cur);
    out
}

fn flush(out: &mut Vec<Vec<Point>>, cur: &mut Vec<Point>) {
    let c = std::mem::take(cur);
    if c.len() >= 3 {
        out.push(c);
    }
}

/// The clip chains of one recording as shapes, each computed once.
pub(super) struct Clips<'a> {
    defs: &'a [ClipDef],
    memo: Vec<Option<Option<Shape>>>,
}

impl<'a> Clips<'a> {
    /// A resolver over the recording's clip table, nothing resolved yet.
    pub(super) fn new(defs: &'a [ClipDef]) -> Self {
        Self {
            defs,
            memo: vec![None; defs.len()],
        }
    }

    /// `shape` cut by clip `id` and its parents. `None` when a clip in the
    /// chain cannot be read.
    pub(super) fn apply(&mut self, shape: Shape, id: Option<ClipId>) -> Option<Shape> {
        match id {
            None => Some(shape),
            Some(id) => Some(shape.intersection(self.chain(id)?)),
        }
    }

    fn chain(&mut self, id: ClipId) -> Option<&Shape> {
        let i = id.index();
        if self.memo.get(i)?.is_none() {
            let shape = self.build(id);
            *self.memo.get_mut(i)? = Some(shape);
        }
        self.memo.get(i)?.as_ref()?.as_ref()
    }

    /// Root to leaf, each intersected with the one before. A chain longer
    /// than the table is a cycle and refused.
    fn build(&self, id: ClipId) -> Option<Shape> {
        let mut chain = Vec::new();
        let mut cursor = Some(id);
        while let Some(c) = cursor {
            if chain.len() > self.defs.len() {
                return None;
            }
            chain.push(c);
            cursor = self.defs.get(c.index())?.parent;
        }
        let mut acc: Option<Shape> = None;
        for c in chain.into_iter().rev() {
            let def = self.defs.get(c.index())?;
            // §8.5.4's empty clip admits nothing.
            let own = match def.path.as_deref() {
                Some(p) => fill(p, def.rule, def.ctm)?,
                None => Shape::default(),
            };
            acc = Some(match acc {
                Some(a) => a.intersection(&own),
                None => own,
            });
        }
        acc
    }
}

/// One closed device-space path of `contours`, and its box.
pub(super) fn path_of(contours: &[Vec<Point>]) -> Option<(Path, DeviceBounds)> {
    let mut pb = PathBuilder::new();
    let mut b = DeviceBounds {
        left: f32::INFINITY,
        top: f32::INFINITY,
        right: f32::NEG_INFINITY,
        bottom: f32::NEG_INFINITY,
    };
    for c in contours {
        let mut it = c.iter();
        let Some(first) = it.next() else {
            continue;
        };
        // Device coordinates; f32 is the path type's precision.
        #[allow(clippy::cast_possible_truncation)]
        let f = |p: &Point| (p[0] as f32, p[1] as f32);
        let (x, y) = f(first);
        pb.move_to(x, y);
        for p in it {
            let (x, y) = f(p);
            pb.line_to(x, y);
            b.left = b.left.min(x);
            b.top = b.top.min(y);
            b.right = b.right.max(x);
            b.bottom = b.bottom.max(y);
        }
        b.left = b.left.min(x);
        b.top = b.top.min(y);
        b.right = b.right.max(x);
        b.bottom = b.bottom.max(y);
        pb.close();
    }
    Some((pb.finish()?, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cubic_is_cut_into_chords_ending_on_its_end_point() {
        let mut pb = PathBuilder::new();
        pb.move_to(0.0, 0.0);
        pb.cubic_to(0.0, 50.0, 50.0, 50.0, 50.0, 0.0);
        pb.close();
        let path = pb.finish();
        let lines = path.map(|p| polylines(&p)).unwrap_or_default();
        assert_eq!(lines.len(), 1);
        let c = lines.first().map(Vec::as_slice).unwrap_or_default();
        assert!(c.len() > 8);
        assert_eq!(c.last(), Some(&[50.0, 0.0]));
    }
}

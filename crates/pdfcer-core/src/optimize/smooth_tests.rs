#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code: a failed lookup should fail the test loudly

use super::tests::{build, only, session};
use super::*;
use crate::content::{ContentStream, ContentTokenKind};
use crate::edit::EditSession;
use crate::vector::geometry::{Matrix, Point};

/// A page drawing `path` (content operators) with `pre` before it; object
/// 5 is an ExtGState that dashes.
fn sample(pre: &str, path: &str, extra_resources: &str) -> Vec<u8> {
    let content = format!("{pre}1 w\n{path}S\n");
    let b = |s: &str| s.as_bytes().to_vec();
    build(&[
        b("<< /Type /Catalog /Pages 2 0 R >>"),
        b("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        b(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R /Resources << {extra_resources}>> >>"
        )),
        b(&format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        )),
        b("<< /Type /ExtGState /D [[4 2] 0] >>"),
    ])
}

/// `n` segments of the arc of radius `r` about (`cx`, `cy`) from `a0` to
/// `a1` degrees, the first point drawn with `lead`, three decimals as CAD
/// writes them.
fn arc(cx: f64, cy: f64, r: f64, a0: f64, a1: f64, n: usize, lead: &str) -> String {
    let mut s = String::new();
    for i in 0..=n {
        let a = (a0 + (a1 - a0) * i as f64 / n as f64).to_radians();
        let op = if i == 0 { lead } else { "l" };
        s.push_str(&format!(
            "{:.3} {:.3} {op}\n",
            cx + r * a.cos(),
            cy + r * a.sin()
        ));
    }
    s
}

fn smooth(tol: f64) -> OptimizeSettings {
    let mut s = only(DiscardOptions::none().with_object(DiscardObject::SmoothLines));
    s.smooth_tolerance = tol;
    s
}

fn content(s: &EditSession) -> ContentStream {
    let view = s.view();
    let page = crate::page_tree::pages_in(&view).unwrap().remove(0);
    ContentStream::from_page(&view, &page).unwrap()
}

/// The page path as points: vertices for `m`/`l`, 64 samples per `c`, all
/// mapped through `cm` operators (no `q` nesting in these fixtures).
fn traced(c: &ContentStream) -> Vec<Point> {
    let mut ctm = Matrix::IDENTITY;
    let mut out: Vec<Point> = Vec::new();
    for op in c.operations() {
        let n: Vec<f64> = op
            .operands
            .iter()
            .filter_map(|t| match &t.kind {
                ContentTokenKind::Operand(o) => o.as_number(),
                _ => None,
            })
            .collect();
        match op.operator_name(&c.buf).unwrap_or_default() {
            b"cm" => ctm = Matrix::new(n[0], n[1], n[2], n[3], n[4], n[5]).post_concat(ctm),
            b"m" | b"l" => out.push(ctm.map_point(Point::new(n[0], n[1]))),
            b"c" => {
                let p0 = *out.last().unwrap();
                let q = |i: usize| ctm.map_point(Point::new(n[i], n[i + 1]));
                let (c1, c2, p3) = (q(0), q(2), q(4));
                for k in 1..=64 {
                    let t = f64::from(k) / 64.0;
                    let u = 1.0 - t;
                    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                    out.push(Point::new(
                        w[0] * p0.x + w[1] * c1.x + w[2] * c2.x + w[3] * p3.x,
                        w[0] * p0.y + w[1] * c1.y + w[2] * c2.y + w[3] * p3.y,
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

/// The largest distance from any point of `a` to the polyline `b`.
fn farthest(a: &[Point], b: &[Point]) -> f64 {
    a.iter()
        .map(|p| {
            b.windows(2)
                .map(|w| {
                    let v = Point::new(w[1].x - w[0].x, w[1].y - w[0].y);
                    let d = Point::new(p.x - w[0].x, p.y - w[0].y);
                    let len2 = v.x * v.x + v.y * v.y;
                    let t = if len2 > 0.0 {
                        ((d.x * v.x + d.y * v.y) / len2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    (d.x - t * v.x).hypot(d.y - t * v.y)
                })
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max)
}

fn ops(c: &ContentStream, name: &[u8]) -> usize {
    c.operations()
        .filter(|o| o.operator_name(&c.buf) == Some(name))
        .count()
}

/// Apply at `tol` and check the two-way distance between the old and new
/// drawings on the page, measured here rather than by the fitter.
fn apply_and_measure(bytes: Vec<u8>, tol: f64) -> (EditSession, ContentStream, ContentStream) {
    let mut s = session(bytes);
    let before = content(&s);
    let plan = s.plan_optimize(&smooth(tol)).unwrap();
    assert_eq!(plan.discard_objects.len(), 1, "{:?}", plan.discard_objects);
    s.apply_optimize(&plan).unwrap();
    let after = content(&s);
    let (old, new) = (traced(&before), traced(&after));
    let moved = farthest(&old, &new).max(farthest(&new, &old));
    assert!(moved <= tol * 1.02, "moved {moved}");
    (s, before, after)
}

#[test]
fn an_arc_becomes_a_few_curves_within_the_tolerance() {
    let bytes = sample("", &arc(150.0, 150.0, 100.0, 0.0, 180.0, 90, "m"), "");
    let plan = session(bytes.clone()).plan_optimize(&smooth(0.1)).unwrap();
    let item = &plan.discard_objects[0];
    assert!(
        item.location.starts_with("page 1: 90 line segments into "),
        "{}",
        item.location
    );
    assert!(item.bytes > 0);
    assert!(plan.notes.iter().any(|n| n.contains("redrawn as curves")));
    let (_, before, after) = apply_and_measure(bytes, 0.1);
    assert_eq!(ops(&after, b"l"), 0);
    assert!(
        (1..=8).contains(&ops(&after, b"c")),
        "{}",
        ops(&after, b"c")
    );
    assert!(after.buf.len() < before.buf.len() / 3);
}

#[test]
fn a_looser_tolerance_needs_fewer_curves() {
    let path = arc(150.0, 150.0, 100.0, 0.0, 270.0, 200, "m");
    let (_, _, tight) = apply_and_measure(sample("", &path, ""), 0.01);
    let (_, _, loose) = apply_and_measure(sample("", &path, ""), 1.0);
    assert!(ops(&loose, b"c") < ops(&tight, b"c"));
}

#[test]
fn a_corner_stays_a_corner_and_end_points_are_kept() {
    let mut path = arc(100.0, 100.0, 50.0, 0.0, 90.0, 30, "m");
    path.push_str(&arc(100.0, 250.0, 100.0, 270.0, 360.0, 30, "l"));
    let (_, _, after) = apply_and_measure(sample("", &path, ""), 0.1);
    let text = String::from_utf8(after.buf.clone()).unwrap();
    // The turn where the arcs meet is still a vertex, and so is each end.
    assert!(text.starts_with("1 w\n150.000 100.000 m\n"), "{text}");
    assert!(
        text.contains("100 150 c\n") || text.contains("100 150 l\n"),
        "{text}"
    );
    assert!(
        text.contains("200 250 c\n") || text.contains("200 250 l\n"),
        "{text}"
    );
}

#[test]
fn a_straight_run_becomes_one_line() {
    let mut path = String::from("10 10 m\n");
    for i in 1..=12 {
        path.push_str(&format!("{} {} l\n", 10 + 20 * i, 10 + 10 * i));
    }
    let plan = session(sample("", &path, ""))
        .plan_optimize(&smooth(0.1))
        .unwrap();
    let location = &plan.discard_objects[0].location;
    assert!(location.contains("into 0 curves and 1 line"), "{location}");
    let (_, _, after) = apply_and_measure(sample("", &path, ""), 0.1);
    assert_eq!(ops(&after, b"l"), 1);
}

#[test]
fn the_tolerance_is_in_page_points_under_a_scaled_ctm() {
    let path = arc(15.0, 15.0, 10.0, 0.0, 180.0, 90, "m");
    let (_, _, after) = apply_and_measure(sample("10 0 0 10 0 0 cm\n", &path, ""), 0.05);
    assert!(ops(&after, b"c") >= 1);
}

#[test]
fn dashed_lines_and_bad_tolerances_are_left() {
    let path = arc(150.0, 150.0, 100.0, 0.0, 180.0, 90, "m");
    let none = |bytes: Vec<u8>, tol: f64| {
        let plan = session(bytes).plan_optimize(&smooth(tol)).unwrap();
        assert!(
            plan.discard_objects.is_empty(),
            "{:?}",
            plan.discard_objects
        );
    };
    none(sample("[3 2] 0 d\n", &path, ""), 0.1);
    none(sample("/G gs\n", &path, "/ExtGState << /G 5 0 R >> "), 0.1);
    none(sample("", &path, ""), 0.0);
    none(sample("", &path, ""), f64::NAN);
    // A dash undone by `Q` smooths again.
    let plan = session(sample("q [3 2] 0 d Q\n", &path, ""))
        .plan_optimize(&smooth(0.1))
        .unwrap();
    assert_eq!(plan.discard_objects.len(), 1);
}

#[test]
fn undo_restores_the_lines() {
    let bytes = sample("", &arc(150.0, 150.0, 100.0, 0.0, 180.0, 90, "m"), "");
    let (mut s, before, _) = apply_and_measure(bytes, 0.1);
    s.undo();
    assert_eq!(content(&s).buf, before.buf);
}

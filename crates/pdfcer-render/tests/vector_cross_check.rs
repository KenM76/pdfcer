//! # Pass 9a acceptance — object geometry matches the renderer's own walk
//!
//! Decision 011 Appendix A Pass 9a criterion: *"Object node geometry
//! matches the rendered geometry (cross-check against pdfcer-render's own
//! walk on fixtures)."* This is the Z2 "agree by construction" gate — the
//! guarantee that the read-only object model
//! (`pdfcer_core::vector::decompose`) and the renderer do not decompose the
//! same content two different ways.
//!
//! It compares, per fixture and per path object, the **page-space point
//! sequence** the two pipelines produce:
//!
//! - the object model: [`pdfcer_core::vector::decompose_page`] under
//!   `Matrix::IDENTITY`, so page space is PDF user space;
//! - the renderer: [`pdfcer_render::interpret::trace_paths`], which runs the
//!   REAL interpreter (the same `paint` path the rasterizer uses) over the
//!   same content with an identity initial CTM, recording each finished
//!   path's nodes + captured `path_ctm` — NOT a second copy of the walk.
//!
//! Every on-curve anchor AND Bézier control point is compared, in
//! construction order, within a tight epsilon (the only difference is the
//! renderer's `f32` vs the object model's `f64`). A divergence here is
//! caught by this test rather than by a mis-placed dimension.

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree::pages;
use pdfcer_core::settings::CmykIntent;
use pdfcer_core::vector::{Matrix, Segment, VectorObject, decompose_page};

use pdfcer_render::RenderOptions;
use pdfcer_render::gstate::GraphicsState;
use pdfcer_render::interpret::{TracedNode, TracedPath, trace_paths};
use pdfcer_render::tiny_skia::{Point as SkPoint, Transform};

/// Tolerance for the f64-vs-f32 comparison. Coordinates run to a few
/// hundred; f32 relative precision (~1e-7) gives sub-1e-3 absolute error
/// even after a rotation, so 1e-2 is comfortable and still tight.
const EPS: f64 = 1e-2;

fn fixture(name: &str) -> Document {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/vector")
        .join(name);
    Document::from_bytes(std::fs::read(&path).expect("fixture file")).expect("fixture parses")
}

/// The object model's page-space point sequence for one path object
/// (start + every segment's control/anchor points, in order).
fn core_points(subpaths: &[pdfcer_core::vector::Subpath]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for sp in subpaths {
        out.push((sp.start.x, sp.start.y));
        for seg in &sp.segments {
            match *seg {
                Segment::Line { to } => out.push((to.x, to.y)),
                Segment::Cubic { c1, c2, to } => {
                    out.push((c1.x, c1.y));
                    out.push((c2.x, c2.y));
                    out.push((to.x, to.y));
                }
            }
        }
    }
    out
}

/// The renderer's page-space point sequence for one traced path (each
/// user-space node mapped through the captured CTM).
fn render_points(tp: &TracedPath) -> Vec<(f64, f64)> {
    let map = |x: f32, y: f32| -> (f64, f64) {
        let mut pts = [SkPoint::from_xy(x, y)];
        tp.ctm.map_points(&mut pts);
        (f64::from(pts[0].x), f64::from(pts[0].y))
    };
    let mut out = Vec::new();
    for node in &tp.nodes {
        match *node {
            TracedNode::Move(x, y) | TracedNode::Line(x, y) => out.push(map(x, y)),
            TracedNode::Cubic(a, b, c, d, e, f) => {
                out.push(map(a, b));
                out.push(map(c, d));
                out.push(map(e, f));
            }
            TracedNode::Close => {}
        }
    }
    out
}

/// Run both pipelines over one fixture's first page and assert the path
/// geometry agrees point-for-point.
fn cross_check(name: &str) {
    let doc = fixture(name);
    let view = doc.view();
    let page = &pages(&doc).expect("page tree")[0];

    // Object model: paths in paint order (skip text/image — the renderer's
    // trace records only paths).
    let model = decompose_page(&view, page, Matrix::IDENTITY).expect("decompose");
    let core: Vec<Vec<(f64, f64)>> = model
        .objects
        .iter()
        .filter_map(|o| match o {
            VectorObject::Path(p) => Some(core_points(&p.page_subpaths())),
            _ => None,
        })
        .collect();

    // Renderer: the real interpreter's walk, identity initial CTM.
    let content = ContentStream::from_page(&view, page).expect("content");
    let traced = trace_paths(
        &view,
        &content,
        &page.resources,
        &pdfcer_render::FontEnvironment::bundled(),
        GraphicsState::default_with_ctm(Transform::identity()),
        RenderOptions::default()
            .with_cmyk_intent(CmykIntent::Calibrated)
            .policy(),
    );
    let render: Vec<Vec<(f64, f64)>> = traced.iter().map(render_points).collect();

    assert_eq!(
        core.len(),
        render.len(),
        "{name}: object model and renderer disagree on the NUMBER of path objects"
    );
    for (i, (c, r)) in core.iter().zip(&render).enumerate() {
        assert_eq!(
            c.len(),
            r.len(),
            "{name}: path {i}: point-count mismatch (core {} vs render {})",
            c.len(),
            r.len()
        );
        for (j, (cp, rp)) in c.iter().zip(r).enumerate() {
            assert!(
                (cp.0 - rp.0).abs() < EPS && (cp.1 - rp.1).abs() < EPS,
                "{name}: path {i} point {j}: core {cp:?} vs render {rp:?}"
            );
        }
    }
}

#[test]
fn paths_fixture_geometry_matches_the_renderer() {
    cross_check("paths.pdf");
}

#[test]
fn curves_fixture_geometry_matches_the_renderer() {
    // The kappa circle + v/y operators: the shared cubic primitives must
    // produce the SAME control points the renderer builds.
    cross_check("curves.pdf");
}

#[test]
fn mixed_fixture_path_geometry_matches_the_renderer() {
    // Text and image objects are present in the object model but absent
    // from the trace; the single stroked path must still cross-check.
    cross_check("mixed.pdf");
}

#[test]
fn centerline_fixture_geometry_matches_the_renderer() {
    // Includes a `cm`-rotated bar: the captured CTMs must agree so the
    // rotated page-space geometry lines up.
    cross_check("centerline.pdf");
}

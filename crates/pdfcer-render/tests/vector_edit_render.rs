//! # Pass 9c-min render-fidelity gate (R59, in-tree pipeline oracle)
//!
//! Decision 011 §2.5 makes basic vector editing *"the first subsystem whose
//! oracle is independent visual fidelity"* — the surgery re-render must be
//! visually correct (R59, ridden from Pass 11/C). pdfium is the external
//! oracle run out-of-band (`tools/annot-pdfium-diff.py`, per the ROADMAP);
//! this file is the **in-tree** half of that gate, and it is strong: it
//! proves the surgery output is content the **real renderer interpreter**
//! (the exact `paint` walk the rasterizer uses — the R49/R60 "one pipeline")
//! traces at the **intended, moved geometry**, and that the edited page
//! rasterizes without error.
//!
//! Method, per operation (move / delete / drag-node):
//!
//! 1. drive `EditSession::{move_object, delete_object, move_node}` on the
//!    synthetic `edit.pdf` fixture, save incrementally, reload;
//! 2. cross-check the reloaded page the SAME way
//!    `vector_cross_check.rs` does for un-edited content — the object model
//!    ([`decompose_page`]) and the renderer ([`trace_paths`]) must agree
//!    point-for-point on the EDITED content (agree-by-construction survives
//!    the surgery);
//! 3. additionally pin the ABSOLUTE moved coordinates in BOTH pipelines, so
//!    the test proves the edit is not merely self-consistent but *correct*
//!    (the object moved to where the operator asked);
//! 4. rasterize the edited page ([`render_page`]) to prove it paints.

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree::{self, pages};
use pdfcer_core::settings::CmykIntent;
use pdfcer_core::vector::{Matrix, Point, Segment, VectorObject, decompose_page};
use pdfcer_core::writer::SaveOptions;

use pdfcer_render::RenderOptions;
use pdfcer_render::gstate::GraphicsState;
use pdfcer_render::interpret::{TracedNode, TracedPath, trace_paths};
use pdfcer_render::render_page;
use pdfcer_render::tiny_skia::{Point as SkPoint, Transform};

const EPS: f64 = 1e-2;

fn edit_fixture_bytes() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/vector/edit.pdf");
    std::fs::read(&path).expect("edit.pdf fixture")
}

/// The object-model point sequence for one path (start + every control /
/// anchor point, page space).
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

/// The renderer's point sequence for one traced path (each node mapped
/// through its captured CTM).
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

/// Assert the object model and the renderer agree point-for-point on a
/// document's first page (the R59 agree-by-construction check).
fn cross_check(doc: &Document) -> Vec<Vec<(f64, f64)>> {
    let page = &pages(doc).expect("page tree")[0];
    let view = doc.view();
    let model = decompose_page(&view, page, Matrix::IDENTITY).expect("decompose");
    let core: Vec<Vec<(f64, f64)>> = model
        .objects
        .iter()
        .filter_map(|o| match o {
            VectorObject::Path(p) => Some(core_points(&p.page_subpaths())),
            _ => None,
        })
        .collect();

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
        "edited page: object model and renderer disagree on path count"
    );
    for (i, (c, r)) in core.iter().zip(&render).enumerate() {
        assert_eq!(c.len(), r.len(), "edited path {i}: point-count mismatch");
        for (j, (cp, rp)) in c.iter().zip(r).enumerate() {
            assert!(
                (cp.0 - rp.0).abs() < EPS && (cp.1 - rp.1).abs() < EPS,
                "edited path {i} point {j}: core {cp:?} vs render {rp:?}"
            );
        }
    }
    core
}

/// Reload the bytes produced by `edit` applied to `edit.pdf`.
fn edited(edit: impl FnOnce(&mut EditSession)) -> Document {
    let mut s = EditSession::new(Document::from_bytes(edit_fixture_bytes()).unwrap());
    edit(&mut s);
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    Document::from_bytes(bytes).unwrap()
}

/// Rasterize a document's first page — proves the edited content paints.
fn rasterizes(doc: &Document) {
    let page = &page_tree::pages(doc).unwrap()[0];
    let out = render_page(doc, page, 1.0).expect("the edited page rasterizes");
    assert!(
        out.pixmap.width() > 0 && out.pixmap.height() > 0,
        "a non-empty raster"
    );
}

#[test]
fn a_moved_object_renders_at_the_moved_geometry() {
    // Move object 0 (the line `50 50 m 150 150 l S`) by +30,-20.
    let doc = edited(|s| {
        s.move_object(0, 0, 30.0, -20.0).unwrap();
    });
    let core = cross_check(&doc);
    // The first path is the moved line: start (80,30), end (180,130) — in
    // BOTH pipelines (cross_check already proved they agree, so checking core
    // pins the absolute correctness).
    assert!(
        (core[0][0].0 - 80.0).abs() < EPS && (core[0][0].1 - 30.0).abs() < EPS,
        "moved start: {:?}",
        core[0][0]
    );
    assert!(
        (core[0][1].0 - 180.0).abs() < EPS && (core[0][1].1 - 130.0).abs() < EPS,
        "moved end: {:?}",
        core[0][1]
    );
    rasterizes(&doc);
}

#[test]
fn a_dragged_node_renders_at_the_new_anchor() {
    // Drag node 1 (the line's endpoint) to (200,100).
    let doc = edited(|s| {
        s.move_node(0, 0, 1, Point::new(200.0, 100.0)).unwrap();
    });
    let core = cross_check(&doc);
    assert!(
        (core[0][0].0 - 50.0).abs() < EPS && (core[0][0].1 - 50.0).abs() < EPS,
        "start unchanged: {:?}",
        core[0][0]
    );
    assert!(
        (core[0][1].0 - 200.0).abs() < EPS && (core[0][1].1 - 100.0).abs() < EPS,
        "dragged endpoint: {:?}",
        core[0][1]
    );
    rasterizes(&doc);
}

#[test]
fn a_deleted_object_is_gone_and_the_rest_still_renders() {
    // edit.pdf has three path objects (line, rectangle, triangle); deleting
    // the line leaves two, and they still agree + rasterize.
    let before = cross_check(&Document::from_bytes(edit_fixture_bytes()).unwrap());
    assert_eq!(before.len(), 3);
    let doc = edited(|s| {
        s.delete_object(0, 0).unwrap();
    });
    let after = cross_check(&doc);
    assert_eq!(after.len(), 2, "one path object removed");
    rasterizes(&doc);
}

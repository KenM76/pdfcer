//! # `vector` — the read-only vector object / selection model (Pass 9a)
//!
//! The first buildable slice of the operator's first beta — the scaled
//! measurement/dimensioning tool
//! (`docs/decisions/011-first-beta-scaled-measurement-dimensioning-tool.md`).
//! This module is a **read-only decomposition layer that indexes** the
//! lossless content-token model ([`crate::content`]); it never rewrites a
//! byte (Pass 9a is byte-inert, R46). It provides three things the
//! dimensioning subsystem, the snapping engine (12.M1), and the GUI
//! selection all consume:
//!
//! 1. **Object decomposition** ([`decompose`], [`decompose_page`]) — the
//!    page content walked into selectable [`VectorObject`]s: path objects
//!    with user-space node lists, an effective CTM, paint style, and the
//!    content-token index range that is the future editing handle; text and
//!    image/form objects as bbox-only selectables.
//! 2. **Hit-test geometry** ([`hit`]) — point→object and marquee→objects in
//!    page space.
//! 3. **Centerline derivation** ([`centerline`]) — the "line center, not
//!    thickness" requirement: a filled thin bar's midline, offered as a
//!    confirmable fuzzy candidate (rule 4), never auto-applied.
//! 4. **Snapping engine** ([`snap`], Pass 12.M1) — over the same object
//!    geometry, [`snap::snap_candidates`] computes the fixed-priority snap
//!    targets (nodes, endpoints, circle centres, midpoints, intersections,
//!    on-segment projections, page axes) within a zoom-invariant tolerance,
//!    plus the H/V alignment constraint ([`snap::constrained_second_point`]).
//!    A shared, GUI-free service consumed by the 12.M2 dimension tools and
//!    the future 9c-min node-drag alike.
//!
//! ## Crate placement (GUI-core separation, binding invariant)
//!
//! Everything here lives in `pdfcer-core` and has **no GUI/windowing
//! dependency** (no egui/eframe/winit/wgpu, and — unlike `pdfcer-render` —
//! not even `tiny-skia`): the object model, hit-test math, and centerline
//! derivation are pure geometry over the token model. The GUI's
//! `CanvasTargetProvider` (Pass 12.0) is a thin `pdfce-gui` adapter that
//! calls into this module and only translates coordinate spaces. This is
//! what keeps the eventual WASM engine fork a shell-crate swap
//! (`docs/ARCHITECTURE.md` §3).
//!
//! ## Agreement with the renderer (decision 011 Z2)
//!
//! The decomposition reuses the SAME construction primitives
//! ([`geometry::cubic_from_v`]/[`geometry::cubic_from_y`]/
//! [`geometry::rect_corners`]) `pdfcer-render`'s interpreter calls, and the
//! same `cm` composition ([`geometry::Matrix::post_concat`]). A cross-check
//! acceptance test in `pdfcer-render` compares the full page-space geometry
//! the two pipelines produce on the vector fixtures, so the object model
//! and the render agree by construction — the geometry analogue of the
//! R49/R60 "one pipeline" discipline.

pub mod centerline;
pub mod clip;
pub mod decompose;
pub mod edit;
pub mod geometry;
pub mod hit;
/// Picking a straight LINE (two endpoints) rather than a point, and deciding
/// whether two of them are parallel or meet at an angle — the pick model a
/// CAD-style "dimension between these two edges" workflow needs.
pub mod linepick;
pub mod snap;

// Re-export the primary surface at `crate::vector::…` so callers do not
// reach through the submodule paths for the everyday types.
pub use centerline::{
    CENTERLINE_ASPECT_THRESHOLD, CenterlineCandidate, derive_from_path, page_candidates,
};
pub use clip::{
    CLIP_MAGIC, CLIP_VERSION, ClipAnnotation, ClipBinding, ClipError, ClipItem, ClipObject,
    ClipPdf, ObjectClip, PastePlan, plan_paste,
};
pub(crate) use decompose::collect_form_leaves;
#[allow(unused_imports)]
pub use decompose::{};
pub use decompose::{
    DecomposeDiagnostics, DevicePaintSpace, DocumentFonts, DocumentXObjects, FillRule,
    FontResolver, ImageObject, ImageSource, MAX_FONT_NAME_BYTES, MAX_NODES, MAX_OBJECTS,
    MAX_TEXT_PREVIEW_CHARS, NoFonts, NoXObjects, PageObjects, PaintStyle, PathObject, PathPaint,
    RunPositioning, Segment, Subpath, TextBoundsBasis, TextFont, TextObject, TextPreview, TextRun,
    TokenRange, VectorObject, XObjectResolver, XObjectShape, decompose, decompose_page,
    decompose_with_fonts,
};
pub use edit::{
    Handle, MixedSelection, PlannedEdit, SingularPolicy, TransformOptions, VectorEditError,
    anchor_count, plan_delete, plan_delete_many, plan_delete_node, plan_delete_subpath,
    plan_delete_text_run, plan_move, plan_move_handle, plan_move_many, plan_move_node,
    plan_move_nodes, plan_move_subpath, plan_recolour, plan_transform_many,
    remap_index_after_delete,
};
pub use geometry::{Bounds, Matrix, Point, Rgb, cubic_from_v, cubic_from_y, rect_corners};
pub use hit::{
    FLATTEN_STEPS, FormMarquee, HitTarget, MarqueeMode, hit_test_point, hit_test_point_all,
    hit_test_point_deep, hit_test_rect, hit_test_rect_deep, hit_test_subpaths, hit_test_text_runs,
    subpath_bounds,
};
pub use snap::{
    AxisConstraint, MAX_CANDIDATES, MAX_NEIGHBOURHOOD_SEGMENTS, SNAP_FLATTEN_STEPS, SnapCandidate,
    SnapConfig, SnapKind, constrained_second_point, measured_length, polyline_length,
    snap_candidates,
};
